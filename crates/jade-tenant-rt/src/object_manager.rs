//! A production in-memory object-manager [`Tenant`] — the native Rust counterpart of
//! `packages/jade-js/tenants/multi.ts`'s `Tenant`, and the object manager the native-Rust
//! interpreter (`jade-vm-native`) and the native-Rust test262 cell run against.
//!
//! Unlike the `jade-primordial-rt` tests' `TestTenant` double (which errors on anything the
//! tests don't exercise), this implementation aims to run real Jade bytecode end to end:
//! every trait operation has MultiTenant-faithful behavior.
//!
//! Deliberate divergences from TS MultiTenant (each recorded in `docs/test262-plan.md`'s
//! native-row notes; the test262 expectations ratchet records any behavioral drift they
//! cause against the `interp` baseline):
//!
//! - **`own_keys`/`own_property_keys` ordering is spec-ordered** (array-index keys ascending,
//!   then string keys in insertion order, then symbol keys in insertion order) rather than
//!   MultiTenant's global first-use insertion order across all objects.
//! - **`define_property` merges** a partial descriptor into the target's existing state
//!   (unsupplied fields keep their current values), per this crate's trait-level doc comment,
//!   rather than replace-building with `false` defaults the way MultiTenant does.
//! - **No garbage collection**: the arena is never freed. Acceptable for the short-lived
//!   conformance cells this tenant serves; a general-purpose embedding would want real GC.
//!
//! Everything else mirrors MultiTenant exactly, including its quirks: `get` reads only the
//! object's *own* properties (there is no prototype walk anywhere in MultiTenant), the
//! prototype lives in an own `"__proto__"` data property installed by `make`, `set` replaces
//! the descriptor of an existing writable property with all-`true` attributes, and
//! non-configurable/non-writable invariant violations are reported with `Ok(false)` rather
//! than an error.

use std::collections::HashMap;

use crate::{
    PropertyKey, Tenant, TenantCallableExoticHandler, TenantError, TenantExoticHandler, TenantInvocation,
    TenantPropertyDescriptor, ValueTag,
};

/// The ObjectManager's value representation. Object identity is an arena index; there is no
/// guest `Symbol` value variant yet (no primordial in the current IR-lowered coverage
/// constructs one — `property_key_value` errors on symbol keys exactly as the trait doc
/// sanctions).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Object(usize),
    Number(f64),
    Str(String),
    Bool(bool),
    Undefined,
    Null,
}

impl Value {
    pub fn as_object(&self) -> Option<usize> {
        match self {
            Value::Object(id) => Some(*id),
            _ => None,
        }
    }
}

/// `Object.is(a, b)` — used by the `define_property` invariant checks, where MultiTenant
/// compares with `Object.is`. Differs from `Value`'s derived `PartialEq` (which matches `===`)
/// at exactly the two IEEE-754 corners: `NaN` equals `NaN`, and `+0` differs from `-0`.
fn object_is(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_nan() && y.is_nan() {
                true
            } else if *x == 0.0 && *y == 0.0 {
                x.is_sign_negative() == y.is_sign_negative()
            } else {
                x == y
            }
        }
        _ => a == b,
    }
}

/// The stored form of a property: always complete. The trait's partial
/// `TenantPropertyDescriptor` is converted at the `define_property` boundary via the merge
/// semantics described in the module doc comment.
#[derive(Debug, Clone)]
enum Descriptor {
    Data {
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    },
    Accessor {
        get: Option<Value>,
        set: Option<Value>,
        enumerable: bool,
        configurable: bool,
    },
}

impl Descriptor {
    fn enumerable(&self) -> bool {
        match self {
            Descriptor::Data { enumerable, .. } | Descriptor::Accessor { enumerable, .. } => *enumerable,
        }
    }

    fn configurable(&self) -> bool {
        match self {
            Descriptor::Data { configurable, .. } | Descriptor::Accessor { configurable, .. } => *configurable,
        }
    }

    /// The full-field `TenantPropertyDescriptor` view, for `get_own_property_descriptor`.
    fn to_tenant_descriptor(&self) -> TenantPropertyDescriptor<Value> {
        match self {
            Descriptor::Data {
                value,
                writable,
                enumerable,
                configurable,
            } => TenantPropertyDescriptor {
                value: Some(value.clone()),
                writable: Some(*writable),
                get: None,
                set: None,
                enumerable: Some(*enumerable),
                configurable: Some(*configurable),
            },
            Descriptor::Accessor {
                get,
                set,
                enumerable,
                configurable,
            } => TenantPropertyDescriptor {
                value: None,
                writable: None,
                get: get.clone(),
                set: set.clone(),
                enumerable: Some(*enumerable),
                configurable: Some(*configurable),
            },
        }
    }
}

/// Merges a partial descriptor into `current` (or into spec defaults for a brand-new
/// property), per the trait doc's merge semantics: unsupplied fields keep the existing
/// value. Switching between data and accessor shape keeps `enumerable`/`configurable` but
/// resets the shape-specific fields (`value`/`writable` vs `get`/`set`) to the supplied ones
/// or their defaults, matching `ValidateAndApplyPropertyDescriptor`'s field-set split.
fn merge_descriptor(current: Option<&Descriptor>, partial: TenantPropertyDescriptor<Value>) -> Descriptor {
    let accessor_shape = partial.get.is_some() || partial.set.is_some();
    let (cur_enum, cur_conf) = current.map(|d| (d.enumerable(), d.configurable())).unwrap_or((false, false));
    let enumerable = partial.enumerable.unwrap_or(cur_enum);
    let configurable = partial.configurable.unwrap_or(cur_conf);
    if accessor_shape {
        let (cur_get, cur_set) = match current {
            Some(Descriptor::Accessor { get, set, .. }) => (get.clone(), set.clone()),
            _ => (None, None),
        };
        Descriptor::Accessor {
            get: partial.get.or(cur_get),
            set: partial.set.or(cur_set),
            enumerable,
            configurable,
        }
    } else {
        let (cur_value, cur_writable) = match current {
            Some(Descriptor::Data { value, writable, .. }) => (value.clone(), *writable),
            _ => (Value::Undefined, false),
        };
        Descriptor::Data {
            value: partial.value.unwrap_or(cur_value),
            writable: partial.writable.unwrap_or(cur_writable),
            enumerable,
            configurable,
        }
    }
}

/// A guest function adopted by the tenant — the native counterpart of a
/// `markGuestFn`-registered closure adopted through MultiTenant's `makeFunction`. Crucially,
/// adopted guest functions are **not** exotics: their property operations use the ordinary
/// object record (shadow property behavior), exactly like MultiTenant-adopted functions;
/// only `invoke`/`invoke_trap` dispatch to this trait.
///
/// The native interpreter's `op_fn` boxes a closure over the callee's bytecode/variant/param
/// slots as a `GuestCallable`; `ObjectManager::invoke` drives it.
pub trait GuestCallable {
    fn apply(&mut self, tenant: &mut ObjectManager, this_arg: &Value, args: &[Value]) -> Result<Value, TenantError>;
    fn construct(&mut self, _tenant: &mut ObjectManager, _new_target: &Value, _args: &[Value]) -> Result<Value, TenantError> {
        Err(TenantError::type_error("guest function is not a constructor"))
    }
}

/// A `Box<dyn TenantCallableExoticHandler>` can't be split into two trait objects, so the
/// callable/non-callable distinction is an enum; `handler()` views either variant through
/// the shared supertrait (trait upcasting).
enum ExoticMeta {
    Plain(Box<dyn TenantExoticHandler<ObjectManager>>),
    Callable(Box<dyn TenantCallableExoticHandler<ObjectManager>>),
}

impl ExoticMeta {
    fn handler(&mut self) -> &mut dyn TenantExoticHandler<ObjectManager> {
        match self {
            ExoticMeta::Plain(handler) => handler.as_mut(),
            ExoticMeta::Callable(handler) => handler.as_mut(),
        }
    }

    fn callable(&mut self) -> Option<&mut dyn TenantCallableExoticHandler<ObjectManager>> {
        match self {
            ExoticMeta::Plain(_) => None,
            ExoticMeta::Callable(handler) => Some(handler.as_mut()),
        }
    }
}

#[derive(Debug, Default)]
struct ObjectRecord {
    props: HashMap<PropertyKey, Descriptor>,
    /// Insertion order of `props`, for spec-ordered key enumeration. The `"__proto__"`
    /// entry installed by `make` sits first, mirroring that MultiTenant writes it at
    /// creation time.
    key_order: Vec<PropertyKey>,
    extensible: bool,
}

impl ObjectRecord {
    fn insert(&mut self, key: PropertyKey, descriptor: Descriptor) {
        if !self.props.contains_key(&key) {
            self.key_order.push(key.clone());
        }
        self.props.insert(key, descriptor);
    }

    fn remove(&mut self, key: &PropertyKey) {
        if self.props.remove(key).is_some() {
            self.key_order.retain(|k| k != key);
        }
    }
}

/// An arena-based object-manager tenant. See the module doc comment for the MultiTenant
/// parity/divergence contract.
#[derive(Default)]
pub struct ObjectManager {
    objects: Vec<ObjectRecord>,
    exotics: HashMap<usize, ExoticMeta>,
    guest_fns: HashMap<usize, Box<dyn GuestCallable>>,
}

fn require_object(value: &Value) -> Result<usize, TenantError> {
    value
        .as_object()
        .ok_or_else(|| TenantError::type_error("ObjectManager: expected an object value"))
}

/// ES `IsArrayIndex`-ish: canonical decimal string of a `u32` other than `u32::MAX`
/// (`"4294967295"` is excluded per spec), no leading zeros, no `+`/`-`/fraction/exponent.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if key.len() > 1 && key.starts_with('0') {
        return None;
    }
    match key.parse::<u32>() {
        Ok(n) if n != u32::MAX => Some(n),
        _ => None,
    }
}

/// ES `ToString` for the primitives this value type can represent. Numbers use the ordinary
/// JS conventions (`NaN`, `Infinity`, no trailing `.0`); base-10 exponent corner cases
/// (`1e21`, `1e-7`) deliberately defer to Rust's shortest-round-trip formatting, which agrees
/// with JS inside the ranges the conformance rows actually exercise.
fn js_to_string(value: &Value) -> String {
    match value {
        Value::Number(n) => {
            if n.is_nan() {
                "NaN".to_string()
            } else if *n == 0.0 {
                "0".to_string()
            } else if n.is_infinite() {
                if *n > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
            } else if n.fract() == 0.0 && n.abs() < 1e21 {
                format!("{n:.0}")
            } else {
                format!("{n}")
            }
        }
        Value::Str(s) => s.clone(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        // No ToPrimitive primordial exists on the native side yet; a plain object's default
        // conversion is the honest answer for the cases that reach here (e.g. `obj[{}]`).
        Value::Object(_) => "[object Object]".to_string(),
    }
}

impl ObjectManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adopt a guest function with shadow (ordinary-record) property behavior, mirroring
    /// MultiTenant's `makeFunction`. Not part of the `Tenant` trait because the trait models
    /// the TS surface, where adoption is the same `makeFunction` the VM's `FN` op already
    /// calls; on the native side the `FN` op constructs the callable itself and adopts it
    /// here.
    pub fn adopt_guest_fn(&mut self, proto: Option<Value>, callable: Box<dyn GuestCallable>) -> Value {
        let idx = self.alloc_record(proto);
        self.guest_fns.insert(idx, callable);
        Value::Object(idx)
    }

    /// Allocates an object record with the `"__proto__"` own data property MultiTenant's
    /// `make`/`makeFunction` install (`{writable: true, enumerable: false, configurable:
    /// false}`).
    fn alloc_record(&mut self, proto: Option<Value>) -> usize {
        let mut record = ObjectRecord {
            extensible: true,
            ..Default::default()
        };
        record.insert(
            PropertyKey::from("__proto__"),
            Descriptor::Data {
                value: proto.unwrap_or(Value::Null),
                writable: true,
                enumerable: false,
                configurable: false,
            },
        );
        self.objects.push(record);
        self.objects.len() - 1
    }

    /// Take-call-restore dance for exotic handlers (a handler stored inside `self` can't be
    /// invoked with `&mut self` at the same time). Returns `None` when `idx` isn't exotic.
    fn with_exotic<R>(
        &mut self,
        idx: usize,
        f: impl FnOnce(&mut Self, &mut ExoticMeta) -> R,
    ) -> Option<R> {
        let mut meta = self.exotics.remove(&idx)?;
        let result = f(self, &mut meta);
        self.exotics.insert(idx, meta);
        Some(result)
    }

    /// Same dance for adopted guest functions.
    fn with_guest_fn<R>(
        &mut self,
        idx: usize,
        f: impl FnOnce(&mut Self, &mut dyn GuestCallable) -> R,
    ) -> Option<R> {
        let mut callable = self.guest_fns.remove(&idx)?;
        let result = f(self, callable.as_mut());
        self.guest_fns.insert(idx, callable);
        Some(result)
    }

    /// Reads a guest descriptor object (the values of the `descriptors` map `define`
    /// receives) into a stored [`Descriptor`], with MultiTenant's `DESCRIPTOR_SPEC` defaults:
    /// absent `writable`/`enumerable`/`configurable` default to `false`, absent `value` to
    /// `undefined`, and the shape is accessor iff `get` or `set` is present.
    fn read_guest_descriptor(&mut self, raw: &Value) -> Result<Descriptor, TenantError> {
        let flag = |tenant: &mut Self, field: &str| -> Result<bool, TenantError> {
            let key = PropertyKey::from(field);
            if !tenant.has(raw, &key)? {
                return Ok(false);
            }
            let value = tenant.get(raw, &key)?;
            Ok(tenant.to_boolean(&value))
        };
        let field = |tenant: &mut Self, name: &str| -> Result<Option<Value>, TenantError> {
            let key = PropertyKey::from(name);
            if !tenant.has(raw, &key)? {
                return Ok(None);
            }
            Ok(Some(tenant.get(raw, &key)?))
        };
        let get = field(self, "get")?;
        let set = field(self, "set")?;
        if get.is_some() || set.is_some() {
            Ok(Descriptor::Accessor {
                get,
                set,
                enumerable: flag(self, "enumerable")?,
                configurable: flag(self, "configurable")?,
            })
        } else {
            Ok(Descriptor::Data {
                value: field(self, "value")?.unwrap_or(Value::Undefined),
                writable: flag(self, "writable")?,
                enumerable: flag(self, "enumerable")?,
                configurable: flag(self, "configurable")?,
            })
        }
    }

    /// Spec-ordered own keys of a plain record: array-index keys ascending, then string keys
    /// in insertion order, then symbol keys in insertion order.
    fn ordered_keys(record: &ObjectRecord) -> Vec<PropertyKey> {
        let mut indices: Vec<(u32, &PropertyKey)> = Vec::new();
        let mut strings: Vec<&PropertyKey> = Vec::new();
        let mut symbols: Vec<&PropertyKey> = Vec::new();
        for key in &record.key_order {
            match key {
                PropertyKey::String(s) => match array_index(s) {
                    Some(n) => indices.push((n, key)),
                    None => strings.push(key),
                },
                PropertyKey::Symbol(_) => symbols.push(key),
            }
        }
        indices.sort_by_key(|(n, _)| *n);
        indices
            .into_iter()
            .map(|(_, k)| k.clone())
            .chain(strings.into_iter().cloned())
            .chain(symbols.into_iter().cloned())
            .collect()
    }
}

impl Tenant for ObjectManager {
    type Value = Value;
    type ObjectId = usize;

    fn typeof_tag(&self, value: &Self::Value) -> ValueTag {
        match value {
            Value::Object(id) if matches!(self.exotics.get(id), Some(ExoticMeta::Callable(_))) => ValueTag::Function,
            Value::Object(id) if self.guest_fns.contains_key(id) => ValueTag::Function,
            Value::Object(_) => ValueTag::Object,
            Value::Number(_) => ValueTag::Number,
            Value::Str(_) => ValueTag::String,
            Value::Bool(_) => ValueTag::Boolean,
            Value::Undefined => ValueTag::Undefined,
            Value::Null => ValueTag::Null,
        }
    }

    fn to_number(&self, value: &Self::Value) -> f64 {
        match value {
            Value::Number(n) => *n,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            // JS `Number("")`/`Number("   ")` are `0`; Rust's `parse` rejects empty input, so
            // handle the whitespace-only case explicitly. `Infinity`/hex/binary string forms
            // JS accepts are deferred (no conformance row exercises them yet).
            Value::Str(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    0.0
                } else {
                    trimmed.parse().unwrap_or(f64::NAN)
                }
            }
            Value::Null => 0.0,
            Value::Undefined | Value::Object(_) => f64::NAN,
        }
    }

    fn to_boolean(&self, value: &Self::Value) -> bool {
        match value {
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Undefined | Value::Null => false,
            Value::Object(_) => true,
        }
    }

    fn boolean_value(&mut self, value: bool) -> Self::Value {
        Value::Bool(value)
    }

    fn string_value(&mut self, value: &str) -> Self::Value {
        Value::Str(value.to_string())
    }

    fn number_value(&mut self, value: f64) -> Self::Value {
        Value::Number(value)
    }

    fn undefined_value(&mut self) -> Self::Value {
        Value::Undefined
    }

    fn null_value(&mut self) -> Self::Value {
        Value::Null
    }

    fn to_string_value(&self, value: &Self::Value) -> String {
        js_to_string(value)
    }

    fn to_property_key(&self, value: &Self::Value) -> PropertyKey {
        PropertyKey::String(js_to_string(value))
    }

    fn nullable(&self, value: &Self::Value) -> Option<Self::Value> {
        match value {
            Value::Null => None,
            other => Some(other.clone()),
        }
    }

    fn indexed_collection(&mut self, values: Vec<Self::Value>) -> Result<Self::Value, TenantError> {
        // Backed by a real object record (not a side representation) so guest `get`/`has`
        // against the result works: indexed data properties plus a real-array-shaped
        // `length` (`{writable, non-enumerable, non-configurable}`), per the trait doc's
        // "indexed numeric access and a `length` property" contract.
        let idx = self.alloc_record(None);
        let length = values.len();
        for (i, value) in values.into_iter().enumerate() {
            self.objects[idx].insert(
                PropertyKey::String(i.to_string()),
                Descriptor::Data {
                    value,
                    writable: true,
                    enumerable: true,
                    configurable: true,
                },
            );
        }
        self.objects[idx].insert(
            PropertyKey::from("length"),
            Descriptor::Data {
                value: Value::Number(length as f64),
                writable: true,
                enumerable: false,
                configurable: false,
            },
        );
        Ok(Value::Object(idx))
    }

    fn property_key_value(&mut self, key: &PropertyKey) -> Result<Self::Value, TenantError> {
        match key {
            PropertyKey::String(s) => Ok(Value::Str(s.clone())),
            PropertyKey::Symbol(_) => Err(TenantError::type_error(
                "ObjectManager has no guest-value representation for a Symbol property key",
            )),
        }
    }

    fn make(&mut self, proto: Option<Self::Value>) -> Result<Self::Value, TenantError> {
        Ok(Value::Object(self.alloc_record(proto)))
    }

    fn get(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<Self::Value, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().get(tenant, &obj, &key))
                .unwrap();
        }
        // No prototype walk, mirroring MultiTenant: only the object's own properties.
        match self.objects[idx].props.get(key) {
            None => Ok(Value::Undefined),
            Some(Descriptor::Data { value, .. }) => Ok(value.clone()),
            Some(Descriptor::Accessor { get: Some(getter), .. }) => {
                let getter = getter.clone();
                let obj = obj.clone();
                self.invoke_trap(&getter, &obj, &[])
            }
            Some(Descriptor::Accessor { get: None, .. }) => Ok(Value::Undefined),
        }
    }

    fn set(&mut self, obj: &Self::Value, key: &PropertyKey, value: Self::Value) -> Result<(), TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().set(tenant, &obj, &key, value))
                .unwrap();
        }
        match self.objects[idx].props.get(key) {
            Some(Descriptor::Accessor { set: Some(setter), .. }) => {
                let setter = setter.clone();
                let obj = obj.clone();
                self.invoke_trap(&setter, &obj, &[value])?;
                return Ok(());
            }
            // An accessor without a setter silently swallows the write (MultiTenant parity).
            Some(Descriptor::Accessor { set: None, .. }) => return Ok(()),
            Some(Descriptor::Data { writable: false, .. }) => return Ok(()),
            None if !self.objects[idx].extensible => return Ok(()),
            _ => {}
        }
        // MultiTenant parity quirk: this *replaces* an existing writable descriptor with
        // all-`true` attributes rather than preserving its enumerable/configurable flags.
        self.objects[idx].insert(
            key.clone(),
            Descriptor::Data {
                value,
                writable: true,
                enumerable: true,
                configurable: true,
            },
        );
        Ok(())
    }

    fn has(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().has(tenant, &obj, &key))
                .unwrap();
        }
        Ok(self.objects[idx].props.contains_key(key))
    }

    fn delete(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<(), TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().delete(tenant, &obj, &key))
                .unwrap();
        }
        if let Some(descriptor) = self.objects[idx].props.get(key) {
            if !descriptor.configurable() {
                // MultiTenant silently keeps non-configurable properties.
                return Ok(());
            }
        }
        self.objects[idx].remove(key);
        Ok(())
    }

    fn own_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().own_keys(tenant, &obj))
                .unwrap();
        }
        Ok(Self::ordered_keys(&self.objects[idx])
            .into_iter()
            .filter(|k| self.objects[idx].props[k].enumerable())
            .collect())
    }

    fn own_property_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().own_property_keys(tenant, &obj))
                .unwrap();
        }
        Ok(Self::ordered_keys(&self.objects[idx]))
    }

    fn get_own_property_descriptor(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
    ) -> Result<Option<TenantPropertyDescriptor<Self::Value>>, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().get_own_property_descriptor(tenant, &obj, &key))
                .unwrap();
        }
        Ok(self.objects[idx].props.get(key).map(Descriptor::to_tenant_descriptor))
    }

    fn define_property(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
        descriptor: TenantPropertyDescriptor<Self::Value>,
    ) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().define_property(tenant, &obj, &key, descriptor))
                .unwrap();
        }
        let current = self.objects[idx].props.get(key);
        // Invariant checks mirrored from MultiTenant's defineProperty (reported as
        // `Ok(false)`, never an error).
        if current.is_none() && !self.objects[idx].extensible {
            return Ok(false);
        }
        if let Some(current) = current {
            if !current.configurable() {
                if descriptor.configurable == Some(true) {
                    return Ok(false);
                }
                if let Descriptor::Data {
                    value: cur_value,
                    writable: false,
                    ..
                } = current
                {
                    if descriptor.writable == Some(true) {
                        return Ok(false);
                    }
                    if let Some(new_value) = &descriptor.value {
                        if !object_is(new_value, cur_value) {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        let merged = merge_descriptor(current, descriptor);
        self.objects[idx].insert(key.clone(), merged);
        Ok(true)
    }

    fn get_prototype_of(&mut self, obj: &Self::Value) -> Result<Option<Self::Value>, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().get_prototype_of(tenant, &obj))
                .unwrap();
        }
        match self.objects[idx].props.get(&PropertyKey::from("__proto__")) {
            Some(Descriptor::Data { value, .. }) => Ok(self.nullable(value)),
            _ => Ok(None),
        }
    }

    fn set_prototype_of(&mut self, obj: &Self::Value, prototype: Option<Self::Value>) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().set_prototype_of(tenant, &obj, prototype))
                .unwrap();
        }
        let proto_key = PropertyKey::from("__proto__");
        let new_proto = prototype.unwrap_or(Value::Null);
        let unchanged = matches!(
            self.objects[idx].props.get(&proto_key),
            Some(Descriptor::Data { value, .. }) if *value == new_proto
        );
        if !self.objects[idx].extensible && !unchanged {
            return Ok(false);
        }
        self.objects[idx].insert(
            proto_key,
            Descriptor::Data {
                value: new_proto,
                writable: true,
                enumerable: false,
                configurable: false,
            },
        );
        Ok(true)
    }

    fn is_extensible(&mut self, obj: &Self::Value) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().is_extensible(tenant, &obj))
                .unwrap();
        }
        Ok(self.objects[idx].extensible)
    }

    fn prevent_extensions(&mut self, obj: &Self::Value) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.exotics.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().prevent_extensions(tenant, &obj))
                .unwrap();
        }
        self.objects[idx].extensible = false;
        Ok(true)
    }

    fn define(&mut self, target: &Self::Value, descriptors: &Self::Value) -> Result<(), TenantError> {
        let idx = require_object(target)?;
        if self.exotics.contains_key(&idx) {
            let target = target.clone();
            let descriptors = descriptors.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().define(tenant, &target, &descriptors))
                .unwrap();
        }
        // MultiTenant's `define`: enumerate the descriptors object's own enumerable keys,
        // read each descriptor object, and replace the target's property with a descriptor
        // whose flags default to `false` (the TS code narrows with exactly those defaults).
        for key in self.own_keys(descriptors)? {
            let raw = self.get(descriptors, &key)?;
            if !matches!(self.typeof_tag(&raw), ValueTag::Object | ValueTag::Function) {
                continue;
            }
            let descriptor = self.read_guest_descriptor(&raw)?;
            let target_idx = require_object(target)?;
            self.objects[target_idx].insert(key, descriptor);
        }
        Ok(())
    }

    fn assign(&mut self, dst: &Self::Value, src: &Self::Value) -> Result<(), TenantError> {
        let idx = require_object(dst)?;
        if self.exotics.contains_key(&idx) {
            let dst = dst.clone();
            let src = src.clone();
            return self
                .with_exotic(idx, |tenant, meta| meta.handler().assign(tenant, &dst, &src))
                .unwrap();
        }
        for key in self.own_keys(src)? {
            let value = self.get(src, &key)?;
            self.set(dst, &key, value)?;
        }
        Ok(())
    }

    fn make_exotic(
        &mut self,
        proto: Option<Self::Value>,
        handler: Box<dyn TenantExoticHandler<Self>>,
    ) -> Result<Self::Value, TenantError> {
        let idx = self.alloc_record(proto);
        self.exotics.insert(idx, ExoticMeta::Plain(handler));
        Ok(Value::Object(idx))
    }

    fn make_callable_exotic(
        &mut self,
        proto: Option<Self::Value>,
        handler: Box<dyn TenantCallableExoticHandler<Self>>,
    ) -> Result<Self::Value, TenantError> {
        let idx = self.alloc_record(proto);
        self.exotics.insert(idx, ExoticMeta::Callable(handler));
        Ok(Value::Object(idx))
    }

    fn invoke(&mut self, callee: &Self::Value, invocation: TenantInvocation<Self::Value>) -> Result<Self::Value, TenantError> {
        let idx = require_object(callee)?;
        let callee = callee.clone();
        if self.exotics.contains_key(&idx) {
            return self
                .with_exotic(idx, |tenant, meta| match meta.callable() {
                    None => Err(TenantError::type_error("tenant exotic has no call trap")),
                    Some(callable) => match invocation {
                        TenantInvocation::Apply { this_arg, args } => callable.apply(tenant, &callee, &this_arg, &args),
                        TenantInvocation::Construct { args, new_target } => {
                            callable.construct(tenant, &callee, &new_target, &args)
                        }
                    },
                })
                .unwrap();
        }
        if self.guest_fns.contains_key(&idx) {
            return self
                .with_guest_fn(idx, |tenant, callable| match invocation {
                    TenantInvocation::Apply { this_arg, args } => callable.apply(tenant, &this_arg, &args),
                    TenantInvocation::Construct { args, new_target } => callable.construct(tenant, &new_target, &args),
                })
                .unwrap();
        }
        Err(TenantError::type_error(format!(
            "{} is not a function",
            js_to_string(&callee)
        )))
    }

    fn invoke_trap(&mut self, f: &Self::Value, receiver: &Self::Value, args: &[Self::Value]) -> Result<Self::Value, TenantError> {
        self.invoke(f, TenantInvocation::Apply {
            this_arg: receiver.clone(),
            args: args.to_vec(),
        })
    }

    fn object_id(&self, value: &Self::Value) -> Self::ObjectId {
        // Only object identity is meaningful for the non-tenant-keyed caches this backs; a
        // primitive collapses to a fixed sentinel (same convention as the primordial-rt
        // tests' double).
        value.as_object().unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ApplyToNumber;
    impl TenantExoticHandler<ObjectManager> for ApplyToNumber {}
    impl TenantCallableExoticHandler<ObjectManager> for ApplyToNumber {
        fn apply(&mut self, tenant: &mut ObjectManager, _receiver: &Value, _this: &Value, args: &[Value]) -> Result<Value, TenantError> {
            Ok(tenant.number_value(args.len() as f64))
        }
        fn construct(&mut self, tenant: &mut ObjectManager, _receiver: &Value, _new_target: &Value, _args: &[Value]) -> Result<Value, TenantError> {
            Ok(tenant.number_value(-1.0))
        }
    }

    struct ConstFn(Value);
    impl GuestCallable for ConstFn {
        fn apply(&mut self, _tenant: &mut ObjectManager, _this: &Value, _args: &[Value]) -> Result<Value, TenantError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn get_on_missing_property_yields_undefined() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        assert_eq!(t.get(&obj, &"nope".into()).unwrap(), Value::Undefined);
    }

    #[test]
    fn set_get_roundtrip_and_no_prototype_walk() {
        let mut t = ObjectManager::new();
        let proto = t.make(None).unwrap();
        t.set(&proto, &"inherited".into(), Value::Number(1.0)).unwrap();
        let obj = t.make(Some(proto)).unwrap();
        assert_eq!(t.get(&obj, &"inherited".into()).unwrap(), Value::Undefined);
        t.set(&obj, &"own".into(), Value::Str("v".into())).unwrap();
        assert_eq!(t.get(&obj, &"own".into()).unwrap(), Value::Str("v".into()));
    }

    #[test]
    fn proto_is_an_own_data_property_like_multitenant() {
        let mut t = ObjectManager::new();
        let proto = t.make(None).unwrap();
        let obj = t.make(Some(proto.clone())).unwrap();
        assert_eq!(t.get(&obj, &"__proto__".into()).unwrap(), proto);
        assert_eq!(t.get_prototype_of(&obj).unwrap(), Some(proto));
        assert!(t.own_property_keys(&obj).unwrap().contains(&PropertyKey::from("__proto__")));
        // ...but non-enumerable, so own_keys excludes it.
        assert!(!t.own_keys(&obj).unwrap().contains(&PropertyKey::from("__proto__")));
    }

    #[test]
    fn set_respects_non_writable_and_extensibility() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        t.define_property(&obj, &"frozen".into(), TenantPropertyDescriptor {
            value: Some(Value::Number(1.0)),
            writable: Some(false),
            ..Default::default()
        })
        .unwrap();
        t.set(&obj, &"frozen".into(), Value::Number(2.0)).unwrap();
        assert_eq!(t.get(&obj, &"frozen".into()).unwrap(), Value::Number(1.0));

        t.prevent_extensions(&obj).unwrap();
        t.set(&obj, &"new".into(), Value::Number(3.0)).unwrap();
        assert_eq!(t.get(&obj, &"new".into()).unwrap(), Value::Undefined);
    }

    #[test]
    fn define_property_merges_partial_descriptors() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        t.define_property(&obj, &"p".into(), TenantPropertyDescriptor {
            value: Some(Value::Number(1.0)),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            ..Default::default()
        })
        .unwrap();
        // Supplying only `writable` keeps the existing value and flags.
        t.define_property(&obj, &"p".into(), TenantPropertyDescriptor {
            writable: Some(false),
            ..Default::default()
        })
        .unwrap();
        let d = t.get_own_property_descriptor(&obj, &"p".into()).unwrap().unwrap();
        assert_eq!(d.value, Some(Value::Number(1.0)));
        assert_eq!(d.writable, Some(false));
        assert_eq!(d.enumerable, Some(true));
        assert_eq!(d.configurable, Some(true));
    }

    #[test]
    fn define_property_invariants_match_multitenant() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        t.define_property(&obj, &"p".into(), TenantPropertyDescriptor {
            value: Some(Value::Number(f64::NAN)),
            writable: Some(false),
            configurable: Some(false),
            ..Default::default()
        })
        .unwrap();
        // configurable: true over a non-configurable property → false.
        assert!(!t
            .define_property(&obj, &"p".into(), TenantPropertyDescriptor {
                configurable: Some(true),
                ..Default::default()
            })
            .unwrap());
        // writable: true over a non-configurable non-writable data property → false.
        assert!(!t
            .define_property(&obj, &"p".into(), TenantPropertyDescriptor {
                writable: Some(true),
                ..Default::default()
            })
            .unwrap());
        // Same value via Object.is (NaN) → allowed.
        assert!(t
            .define_property(&obj, &"p".into(), TenantPropertyDescriptor {
                value: Some(Value::Number(f64::NAN)),
                ..Default::default()
            })
            .unwrap());
        // Different value → false.
        assert!(!t
            .define_property(&obj, &"p".into(), TenantPropertyDescriptor {
                value: Some(Value::Number(1.0)),
                ..Default::default()
            })
            .unwrap());
    }

    #[test]
    fn own_keys_are_spec_ordered() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        for key in ["b", "2", "a", "10", "1"] {
            t.set(&obj, &key.into(), Value::Bool(true)).unwrap();
        }
        let names: Vec<String> = t
            .own_keys(&obj)
            .unwrap()
            .into_iter()
            .map(|k| k.to_string())
            .collect();
        assert_eq!(names, vec!["1", "2", "10", "b", "a"]);
    }

    #[test]
    fn delete_keeps_non_configurable() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        t.define_property(&obj, &"stay".into(), TenantPropertyDescriptor {
            value: Some(Value::Number(1.0)),
            configurable: Some(false),
            ..Default::default()
        })
        .unwrap();
        t.delete(&obj, &"stay".into()).unwrap();
        assert!(t.has(&obj, &"stay".into()).unwrap());
        t.set(&obj, &"go".into(), Value::Number(2.0)).unwrap();
        t.delete(&obj, &"go".into()).unwrap();
        assert!(!t.has(&obj, &"go".into()).unwrap());
    }

    #[test]
    fn accessor_get_and_set_route_through_invoke_trap() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        let getter = t.adopt_guest_fn(None, Box::new(ConstFn(Value::Number(7.0))));
        t.define_property(&obj, &"p".into(), TenantPropertyDescriptor {
            get: Some(getter),
            enumerable: Some(true),
            configurable: Some(true),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(t.get(&obj, &"p".into()).unwrap(), Value::Number(7.0));
        // Accessor without a setter silently swallows writes.
        t.set(&obj, &"p".into(), Value::Number(9.0)).unwrap();
        assert_eq!(t.get(&obj, &"p".into()).unwrap(), Value::Number(7.0));
    }

    #[test]
    fn adopted_guest_fn_invokes_and_keeps_shadow_properties() {
        let mut t = ObjectManager::new();
        let f = t.adopt_guest_fn(None, Box::new(ConstFn(Value::Str("hit".into()))));
        assert_eq!(t.typeof_tag(&f), ValueTag::Function);
        assert_eq!(
            t.invoke(&f, TenantInvocation::Apply {
                this_arg: Value::Undefined,
                args: vec![],
            })
            .unwrap(),
            Value::Str("hit".into())
        );
        t.set(&f, &"custom".into(), Value::Number(1.0)).unwrap();
        assert_eq!(t.get(&f, &"custom".into()).unwrap(), Value::Number(1.0));
    }

    #[test]
    fn non_callable_invoke_errors() {
        let mut t = ObjectManager::new();
        let obj = t.make(None).unwrap();
        assert!(t
            .invoke(&obj, TenantInvocation::Apply {
                this_arg: Value::Undefined,
                args: vec![],
            })
            .is_err());
    }

    #[test]
    fn callable_exotic_dispatches_apply_and_construct() {
        let mut t = ObjectManager::new();
        let f = t.make_callable_exotic(None, Box::new(ApplyToNumber)).unwrap();
        assert_eq!(t.typeof_tag(&f), ValueTag::Function);
        assert_eq!(
            t.invoke(&f, TenantInvocation::Apply {
                this_arg: Value::Undefined,
                args: vec![Value::Null, Value::Null],
            })
            .unwrap(),
            Value::Number(2.0)
        );
        assert_eq!(
            t.invoke(&f, TenantInvocation::Construct {
                args: vec![],
                new_target: f.clone(),
            })
            .unwrap(),
            Value::Number(-1.0)
        );
    }

    #[test]
    fn plain_exotic_fails_closed() {
        struct NoTraps;
        impl TenantExoticHandler<ObjectManager> for NoTraps {}
        let mut t = ObjectManager::new();
        let obj = t.make_exotic(None, Box::new(NoTraps)).unwrap();
        assert!(t.get(&obj, &"x".into()).is_err());
        assert!(t.own_keys(&obj).is_err());
    }

    #[test]
    fn indexed_collection_is_a_real_object() {
        let mut t = ObjectManager::new();
        let list = t.indexed_collection(vec![Value::Str("a".into()), Value::Str("b".into())]).unwrap();
        assert_eq!(t.get(&list, &"length".into()).unwrap(), Value::Number(2.0));
        assert_eq!(t.get(&list, &"0".into()).unwrap(), Value::Str("a".into()));
        let keys = t.own_keys(&list).unwrap();
        assert_eq!(keys, vec![PropertyKey::from("0"), PropertyKey::from("1")]);
    }

    #[test]
    fn to_property_key_normalizes_numbers() {
        let t = ObjectManager::new();
        assert_eq!(t.to_property_key(&Value::Number(5.0)), PropertyKey::from("5"));
        assert_eq!(t.to_property_key(&Value::Null), PropertyKey::from("null"));
    }
}

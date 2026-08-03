//! Hand-written Rust port of `packages/jade-js/primordials/types.ts`'s shared helpers.
//!
//! **Not generated.** `types.ts` is deliberately *shimmed* rather than run through
//! `jade-primordial-ir`'s lowering: its `makeBuiltin` closes an exotic-handler object literal
//! over `this` (`*define`/`*assign` call `this.defineProperty!`/`this.set!` on sibling trap
//! methods), and `readGuestDescriptor`/`descriptorObject` do `key in descriptor`/
//! `descriptor[key]` dynamic-by-name field access on what's a fixed Rust struct
//! (`TenantPropertyDescriptor`) on this side — both are natural, ordinary Rust once hand-written
//! (a struct implementing `TenantCallableExoticHandler`, calling its own methods via `self.foo()`;
//! six unrolled field arms instead of a dynamic accessor) but would need real dynamic-dispatch/
//! reflection machinery in the IR to derive automatically. See the primordial-IR plan's shimming
//! addendum: any primordial file that imports from `./types.ts` keeps that import and that call
//! form unchanged when *its own* source is IR-lowered and re-emitted as TypeScript (nothing
//! about `types.ts` itself needs to round-trip), while the Rust emitter resolves the same call
//! to this module instead (see `jade-primordial-ir::shims`).
//!
//! Because `Tenant::Value` is opaque, a few functions here take `tenant: &T`/`&mut T` where the
//! original TS didn't need it (`assertObject`, `toIndex`) — TS gets `typeof`/`Number()` for
//! free from the host engine; Rust needs `Tenant::typeof_tag`/`to_number` instead. This is a
//! deliberate, signature-level deviation from the TS source, not an oversight.

use std::collections::HashMap;

use portal_solutions_jade_tenant_rt::intrinsics::{is_array_index_string, is_integer};
use portal_solutions_jade_tenant_rt::{
    PropertyKey, Tenant, TenantCallableExoticHandler, TenantError, TenantExoticHandler, TenantInvocation,
    TenantPropertyDescriptor, ValueTag,
};

const DESCRIPTOR_FIELDS: &[&str] = &["value", "writable", "get", "set", "enumerable", "configurable"];

/// Reads a guest-visible descriptor-shaped object (as passed to e.g. `Object.defineProperty`)
/// into a host-side [`TenantPropertyDescriptor`] control record. Mirrors `readGuestDescriptor`.
pub fn read_guest_descriptor<T: Tenant>(
    tenant: &mut T,
    source: &T::Value,
) -> Result<TenantPropertyDescriptor<T::Value>, TenantError> {
    let mut out = TenantPropertyDescriptor::default();
    for field in DESCRIPTOR_FIELDS {
        let key = PropertyKey::from(*field);
        if !tenant.has(source, &key)? {
            continue;
        }
        let value = tenant.get(source, &key)?;
        match *field {
            "value" => out.value = Some(value),
            "get" => out.get = Some(value),
            "set" => out.set = Some(value),
            "writable" => out.writable = Some(tenant.to_boolean(&value)),
            "enumerable" => out.enumerable = Some(tenant.to_boolean(&value)),
            "configurable" => out.configurable = Some(tenant.to_boolean(&value)),
            _ => unreachable!(),
        }
    }
    Ok(out)
}

/// Materializes a host-side [`TenantPropertyDescriptor`] as a fresh guest-visible object.
/// Mirrors `descriptorObject`.
pub fn descriptor_object<T: Tenant>(
    tenant: &mut T,
    descriptor: &TenantPropertyDescriptor<T::Value>,
) -> Result<T::Value, TenantError> {
    let out = tenant.make(None)?;
    if let Some(value) = &descriptor.value {
        tenant.set(&out, &PropertyKey::from("value"), value.clone())?;
    }
    if let Some(value) = &descriptor.get {
        tenant.set(&out, &PropertyKey::from("get"), value.clone())?;
    }
    if let Some(value) = &descriptor.set {
        tenant.set(&out, &PropertyKey::from("set"), value.clone())?;
    }
    if let Some(b) = descriptor.writable {
        let value = tenant.boolean_value(b);
        tenant.set(&out, &PropertyKey::from("writable"), value)?;
    }
    if let Some(b) = descriptor.enumerable {
        let value = tenant.boolean_value(b);
        tenant.set(&out, &PropertyKey::from("enumerable"), value)?;
    }
    if let Some(b) = descriptor.configurable {
        let value = tenant.boolean_value(b);
        tenant.set(&out, &PropertyKey::from("configurable"), value)?;
    }
    Ok(out)
}

/// Reads a guest array-like object's dense numeric-index values, in ascending index order.
/// Mirrors `guestArrayLike`.
pub fn guest_array_like<T: Tenant>(tenant: &mut T, source: &T::Value) -> Result<Vec<T::Value>, TenantError> {
    let keys = tenant.own_property_keys(source)?;
    let mut numeric: Vec<(usize, PropertyKey)> = keys
        .into_iter()
        .filter_map(|key| match &key {
            PropertyKey::String(s) if is_array_index_string(s) => s.parse().ok().map(|n| (n, key)),
            _ => None,
        })
        .collect();
    numeric.sort_by_key(|(n, _)| *n);
    numeric.into_iter().map(|(_, key)| tenant.get(source, &key)).collect()
}

/// Defines one own data property on `target` from `value` plus optional attribute overrides
/// (each defaulting the same way `Object.defineProperty` would: `writable`/`configurable`
/// default `true`, `enumerable` defaults `false`). Mirrors `defineData`.
pub fn define_data<T: Tenant>(
    tenant: &mut T,
    target: &T::Value,
    key: &PropertyKey,
    value: &T::Value,
    attributes: TenantPropertyDescriptor<T::Value>,
) -> Result<(), TenantError> {
    let ok = tenant.define_property(
        target,
        key,
        TenantPropertyDescriptor {
            value: Some(value.clone()),
            writable: Some(attributes.writable.unwrap_or(true)),
            get: None,
            set: None,
            enumerable: Some(attributes.enumerable.unwrap_or(false)),
            configurable: Some(attributes.configurable.unwrap_or(true)),
        },
    )?;
    if !ok {
        return Err(TenantError::type_error(format!("cannot define primordial property {key}")));
    }
    Ok(())
}

/// Rejects `value` unless it is an object or function (never nullish, never any other
/// primitive). Mirrors `assertObject` — see this module's doc comment for why it takes
/// `tenant: &T` where the TS version didn't need to.
pub fn assert_object<T: Tenant>(tenant: &T, value: &T::Value, message: &str) -> Result<(), TenantError> {
    let tag = tenant.typeof_tag(value);
    if tag == ValueTag::Null || (tag != ValueTag::Object && tag != ValueTag::Function) {
        return Err(TenantError::type_error(message));
    }
    Ok(())
}

/// Coerces `value` to a non-negative integer index, rejecting anything else. Mirrors `toIndex`
/// — returns `usize` rather than TS's `number`, since every real call site immediately uses the
/// result as an index; see this module's doc comment on the `tenant` parameter.
pub fn to_index<T: Tenant>(tenant: &T, value: &T::Value) -> Result<usize, TenantError> {
    let number = tenant.to_number(value);
    if !is_integer(number) || number < 0.0 {
        return Err(TenantError::range_error("expected a non-negative integer"));
    }
    Ok(number as usize)
}

type ApplyFn<T> = Box<dyn FnMut(&mut T, <T as Tenant>::Value, &[<T as Tenant>::Value]) -> Result<<T as Tenant>::Value, TenantError>>;
type ConstructFn<T> = Box<dyn FnMut(&mut T, <T as Tenant>::Value, &[<T as Tenant>::Value]) -> Result<<T as Tenant>::Value, TenantError>>;

/// A tenant-owned callable with private descriptor storage — the exotic-handler backing for
/// [`make_builtin`]. Mirrors `makeBuiltin`'s inline `handler` object literal; see this module's
/// doc comment for why this is a struct rather than IR-derived.
struct BuiltinHandler<T: Tenant> {
    name: String,
    apply: ApplyFn<T>,
    construct: Option<ConstructFn<T>>,
    descriptors: HashMap<PropertyKey, TenantPropertyDescriptor<T::Value>>,
    prototype: Option<T::Value>,
    extensible: bool,
}

impl<T: Tenant> TenantExoticHandler<T> for BuiltinHandler<T> {
    fn get(&mut self, tenant: &mut T, receiver: &T::Value, key: &PropertyKey) -> Result<T::Value, TenantError> {
        let Some(descriptor) = self.descriptors.get(key).cloned() else {
            return Ok(tenant.undefined_value());
        };
        if let Some(getter) = &descriptor.get {
            return tenant.invoke_trap(getter, receiver, &[]);
        }
        Ok(descriptor.value.unwrap_or_else(|| tenant.undefined_value()))
    }

    fn set(&mut self, tenant: &mut T, receiver: &T::Value, key: &PropertyKey, value: T::Value) -> Result<(), TenantError> {
        let existing = self.descriptors.get(key).cloned();
        if let Some(setter) = existing.as_ref().and_then(|d| d.set.as_ref()) {
            tenant.invoke_trap(setter, receiver, &[value])?;
            return Ok(());
        }
        let writable_ok = existing.as_ref().map(|d| d.writable != Some(false)).unwrap_or(true);
        let allowed = existing.is_some() || self.extensible;
        if writable_ok && allowed {
            self.descriptors.insert(
                key.clone(),
                TenantPropertyDescriptor {
                    value: Some(value),
                    writable: Some(true),
                    enumerable: Some(true),
                    configurable: Some(true),
                    ..Default::default()
                },
            );
        }
        Ok(())
    }

    fn has(&mut self, _tenant: &mut T, _receiver: &T::Value, key: &PropertyKey) -> Result<bool, TenantError> {
        Ok(self.descriptors.contains_key(key))
    }

    fn delete(&mut self, _tenant: &mut T, _receiver: &T::Value, key: &PropertyKey) -> Result<(), TenantError> {
        let allowed = self.descriptors.get(key).map(|d| d.configurable != Some(false)).unwrap_or(true);
        if allowed {
            self.descriptors.remove(key);
        }
        Ok(())
    }

    fn own_keys(&mut self, _tenant: &mut T, _receiver: &T::Value) -> Result<Vec<PropertyKey>, TenantError> {
        Ok(self
            .descriptors
            .iter()
            .filter(|(_, d)| d.enumerable == Some(true))
            .map(|(k, _)| k.clone())
            .collect())
    }

    fn own_property_keys(&mut self, _tenant: &mut T, _receiver: &T::Value) -> Result<Vec<PropertyKey>, TenantError> {
        Ok(self.descriptors.keys().cloned().collect())
    }

    fn get_own_property_descriptor(
        &mut self,
        _tenant: &mut T,
        _receiver: &T::Value,
        key: &PropertyKey,
    ) -> Result<Option<TenantPropertyDescriptor<T::Value>>, TenantError> {
        Ok(self.descriptors.get(key).cloned())
    }

    fn define_property(
        &mut self,
        _tenant: &mut T,
        _receiver: &T::Value,
        key: &PropertyKey,
        descriptor: TenantPropertyDescriptor<T::Value>,
    ) -> Result<bool, TenantError> {
        let current = self.descriptors.get(key);
        if current.is_none() && !self.extensible {
            return Ok(false);
        }
        if current.map(|d| d.configurable == Some(false)).unwrap_or(false) && descriptor.configurable == Some(true) {
            return Ok(false);
        }
        self.descriptors.insert(key.clone(), descriptor);
        Ok(true)
    }

    fn get_prototype_of(&mut self, _tenant: &mut T, _receiver: &T::Value) -> Result<Option<T::Value>, TenantError> {
        Ok(self.prototype.clone())
    }

    fn set_prototype_of(&mut self, _tenant: &mut T, _receiver: &T::Value, next: Option<T::Value>) -> Result<bool, TenantError> {
        if !self.extensible && self.prototype != next {
            return Ok(false);
        }
        self.prototype = next;
        Ok(true)
    }

    fn is_extensible(&mut self, _tenant: &mut T, _receiver: &T::Value) -> Result<bool, TenantError> {
        Ok(self.extensible)
    }

    fn prevent_extensions(&mut self, _tenant: &mut T, _receiver: &T::Value) -> Result<bool, TenantError> {
        self.extensible = false;
        Ok(true)
    }

    fn define(&mut self, tenant: &mut T, receiver: &T::Value, source: &T::Value) -> Result<(), TenantError> {
        for key in tenant.own_keys(source)? {
            let descriptor_source = tenant.get(source, &key)?;
            let descriptor = read_guest_descriptor(tenant, &descriptor_source)?;
            self.define_property(tenant, receiver, &key, descriptor)?;
        }
        Ok(())
    }

    fn assign(&mut self, tenant: &mut T, receiver: &T::Value, source: &T::Value) -> Result<(), TenantError> {
        for key in tenant.own_keys(source)? {
            let value = tenant.get(source, &key)?;
            self.set(tenant, receiver, &key, value)?;
        }
        Ok(())
    }
}

impl<T: Tenant> TenantCallableExoticHandler<T> for BuiltinHandler<T> {
    fn apply(&mut self, tenant: &mut T, _receiver: &T::Value, this_arg: &T::Value, args: &[T::Value]) -> Result<T::Value, TenantError> {
        (self.apply)(tenant, this_arg.clone(), args)
    }

    fn construct(&mut self, tenant: &mut T, _receiver: &T::Value, new_target: &T::Value, args: &[T::Value]) -> Result<T::Value, TenantError> {
        let Some(construct) = &mut self.construct else {
            return Err(TenantError::type_error(format!("{} is not a constructor", self.name)));
        };
        construct(tenant, new_target.clone(), args)
    }
}

/// Creates a tenant-owned callable with private descriptor storage. Mirrors `makeBuiltin` — see
/// this module's doc comment for why the exotic handler is a hand-written struct.
pub fn make_builtin<T: Tenant + 'static>(
    tenant: &mut T,
    name: impl AsRef<str>,
    apply: impl FnMut(&mut T, T::Value, &[T::Value]) -> Result<T::Value, TenantError> + 'static,
    construct: Option<ConstructFn<T>>,
    proto: Option<T::Value>,
) -> Result<T::Value, TenantError> {
    let handler = BuiltinHandler {
        name: name.as_ref().to_string(),
        apply: Box::new(apply),
        construct,
        descriptors: HashMap::new(),
        prototype: proto.clone(),
        extensible: true,
    };
    let shell = tenant.make_callable_exotic(proto, Box::new(handler))?;
    let name_value = tenant.string_value(name.as_ref());
    define_data(
        tenant,
        &shell,
        &PropertyKey::from("name"),
        &name_value,
        TenantPropertyDescriptor {
            writable: Some(false),
            configurable: Some(true),
            ..Default::default()
        },
    )?;
    Ok(shell)
}

/// Convenience wrapper matching `tenant.invoke(callee, { kind: "apply", ... })`'s common shape,
/// used by call sites in this module and future shimmed/generated primordial code alike.
pub fn invoke_apply<T: Tenant>(tenant: &mut T, callee: &T::Value, this_arg: T::Value, args: Vec<T::Value>) -> Result<T::Value, TenantError> {
    tenant.invoke(callee, TenantInvocation::Apply { this_arg, args })
}

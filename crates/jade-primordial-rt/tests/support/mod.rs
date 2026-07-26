//! A minimal in-memory `Tenant` test double, used by `jade-primordial-rt`'s integration tests
//! to exercise generated/shimmed primordial code end to end. Only the operations the tests
//! actually exercise have real behavior; everything else returns a clear "not implemented in
//! test double" error rather than panicking, so a test that unexpectedly reaches an
//! unimplemented path fails with a readable message instead of a stack unwind.

use std::collections::HashMap;

use portal_solutions_jade_tenant_rt::{
    HostAsyncCapability, HostTaskId, PropertyKey, Tenant, TenantCallableExoticHandler, TenantError,
    TenantExoticHandler, TenantInvocation, TenantPropertyDescriptor, ValueTag,
};

/// Unlike the object-handle-only `usize` used in the first slice, `TestValue` also represents
/// primitives — needed now that `Tenant::typeof_tag`/`to_number`/`to_boolean`/`boolean_value`/
/// `undefined_value` exist for `types_shim.rs`'s helpers to call.
#[derive(Debug, Clone, PartialEq)]
pub enum TestValue {
    Object(usize),
    Number(f64),
    Str(String),
    Bool(bool),
    Undefined,
    Null,
}

impl TestValue {
    pub fn as_object(&self) -> Option<usize> {
        match self {
            TestValue::Object(id) => Some(*id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ObjectRecord {
    pub proto: Option<TestValue>,
    pub extensible: bool,
    pub props: HashMap<PropertyKey, TenantPropertyDescriptor<TestValue>>,
}

/// An object index present in `callables` is a callable exotic: every property/prototype/
/// extensibility operation on it routes through its handler's own traps instead of `objects`'
/// plain property map, matching how a real `makeCallableExotic` shell's properties are owned
/// entirely by its handler (see `TenantExoticHandler`'s doc comment on fail-closed traps).
#[derive(Default)]
pub struct TestTenant {
    pub objects: Vec<ObjectRecord>,
    callables: HashMap<usize, Box<dyn TenantCallableExoticHandler<TestTenant>>>,
}

fn unimplemented(op: &str) -> TenantError {
    TenantError::type_error(format!("test double: '{op}' is not implemented"))
}

fn require_object(value: &TestValue) -> Result<usize, TenantError> {
    value
        .as_object()
        .ok_or_else(|| TenantError::type_error("test double: expected an object value"))
}

impl TestTenant {
    /// Temporarily removes `idx`'s handler so it can be called with `&mut self` as the tenant
    /// argument (a handler stored inside `self.callables` can't otherwise be invoked with
    /// `&mut self` at the same time — a standard take-call-restore dance for this shape of
    /// self-referential mutable borrow), then restores it. Returns `None` if `idx` isn't a
    /// callable exotic.
    fn with_handler<R>(
        &mut self,
        idx: usize,
        f: impl FnOnce(&mut Self, &mut dyn TenantCallableExoticHandler<Self>) -> R,
    ) -> Option<R> {
        let mut handler = self.callables.remove(&idx)?;
        let result = f(self, handler.as_mut());
        self.callables.insert(idx, handler);
        Some(result)
    }
}

impl Tenant for TestTenant {
    type Value = TestValue;
    type ObjectId = usize;

    fn typeof_tag(&self, value: &Self::Value) -> ValueTag {
        match value {
            TestValue::Object(_) => ValueTag::Object,
            TestValue::Number(_) => ValueTag::Number,
            TestValue::Str(_) => ValueTag::String,
            TestValue::Bool(_) => ValueTag::Boolean,
            TestValue::Undefined => ValueTag::Undefined,
            TestValue::Null => ValueTag::Null,
        }
    }

    fn to_number(&self, value: &Self::Value) -> f64 {
        match value {
            TestValue::Number(n) => *n,
            TestValue::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            TestValue::Str(s) => s.trim().parse().unwrap_or(f64::NAN),
            TestValue::Null => 0.0,
            TestValue::Undefined | TestValue::Object(_) => f64::NAN,
        }
    }

    fn to_boolean(&self, value: &Self::Value) -> bool {
        match value {
            TestValue::Bool(b) => *b,
            TestValue::Number(n) => *n != 0.0 && !n.is_nan(),
            TestValue::Str(s) => !s.is_empty(),
            TestValue::Undefined | TestValue::Null => false,
            TestValue::Object(_) => true,
        }
    }

    fn boolean_value(&mut self, value: bool) -> Self::Value {
        TestValue::Bool(value)
    }

    fn string_value(&mut self, value: &str) -> Self::Value {
        TestValue::Str(value.to_string())
    }

    fn number_value(&mut self, value: f64) -> Self::Value {
        TestValue::Number(value)
    }

    fn undefined_value(&mut self) -> Self::Value {
        TestValue::Undefined
    }

    fn null_value(&mut self) -> Self::Value {
        TestValue::Null
    }

    fn to_string_value(&self, value: &Self::Value) -> String {
        match value {
            TestValue::Str(s) => s.clone(),
            other => format!("{other:?}"),
        }
    }

    fn to_property_key(&self, value: &Self::Value) -> PropertyKey {
        PropertyKey::String(self.to_string_value(value))
    }

    fn nullable(&self, value: &Self::Value) -> Option<Self::Value> {
        match value {
            TestValue::Null => None,
            other => Some(other.clone()),
        }
    }

    fn make(&mut self, proto: Option<Self::Value>) -> Result<Self::Value, TenantError> {
        self.objects.push(ObjectRecord {
            proto,
            extensible: true,
            props: HashMap::new(),
        });
        Ok(TestValue::Object(self.objects.len() - 1))
    }

    fn get(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<Self::Value, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self.with_handler(idx, |tenant, handler| handler.get(tenant, &obj, &key)).unwrap();
        }
        self.objects[idx]
            .props
            .get(key)
            .and_then(|d| d.value.clone())
            .ok_or_else(|| unimplemented("get (missing/non-data property)"))
    }

    fn set(&mut self, obj: &Self::Value, key: &PropertyKey, value: Self::Value) -> Result<(), TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.set(tenant, &obj, &key, value))
                .unwrap();
        }
        self.objects[idx].props.insert(
            key.clone(),
            TenantPropertyDescriptor {
                value: Some(value),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            },
        );
        Ok(())
    }

    fn has(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self.with_handler(idx, |tenant, handler| handler.has(tenant, &obj, &key)).unwrap();
        }
        Ok(self.objects[idx].props.contains_key(key))
    }

    fn delete(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<(), TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.delete(tenant, &obj, &key))
                .unwrap();
        }
        self.objects[idx].props.remove(key);
        Ok(())
    }

    fn own_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            return self.with_handler(idx, |tenant, handler| handler.own_keys(tenant, &obj)).unwrap();
        }
        Ok(self.objects[idx]
            .props
            .iter()
            .filter(|(_, d)| d.enumerable != Some(false))
            .map(|(k, _)| k.clone())
            .collect())
    }

    fn own_property_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.own_property_keys(tenant, &obj))
                .unwrap();
        }
        Ok(self.objects[idx].props.keys().cloned().collect())
    }

    fn get_own_property_descriptor(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
    ) -> Result<Option<TenantPropertyDescriptor<Self::Value>>, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.get_own_property_descriptor(tenant, &obj, &key))
                .unwrap();
        }
        Ok(self.objects[idx].props.get(key).cloned())
    }

    fn define_property(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
        descriptor: TenantPropertyDescriptor<Self::Value>,
    ) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            let key = key.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.define_property(tenant, &obj, &key, descriptor))
                .unwrap();
        }
        let record = &mut self.objects[idx];
        if !record.props.contains_key(key) && !record.extensible {
            return Ok(false);
        }
        record.props.insert(key.clone(), descriptor);
        Ok(true)
    }

    fn get_prototype_of(&mut self, obj: &Self::Value) -> Result<Option<Self::Value>, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.get_prototype_of(tenant, &obj))
                .unwrap();
        }
        Ok(self.objects[idx].proto.clone())
    }

    fn set_prototype_of(&mut self, obj: &Self::Value, prototype: Option<Self::Value>) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.set_prototype_of(tenant, &obj, prototype))
                .unwrap();
        }
        self.objects[idx].proto = prototype;
        Ok(true)
    }

    fn is_extensible(&mut self, obj: &Self::Value) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.is_extensible(tenant, &obj))
                .unwrap();
        }
        Ok(self.objects[idx].extensible)
    }

    fn prevent_extensions(&mut self, obj: &Self::Value) -> Result<bool, TenantError> {
        let idx = require_object(obj)?;
        if self.callables.contains_key(&idx) {
            let obj = obj.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.prevent_extensions(tenant, &obj))
                .unwrap();
        }
        self.objects[idx].extensible = false;
        Ok(true)
    }

    fn define(&mut self, target: &Self::Value, descriptors: &Self::Value) -> Result<(), TenantError> {
        let idx = require_object(target)?;
        if self.callables.contains_key(&idx) {
            let target = target.clone();
            let descriptors = descriptors.clone();
            return self
                .with_handler(idx, |tenant, handler| handler.define(tenant, &target, &descriptors))
                .unwrap();
        }
        Err(unimplemented("define (non-exotic target)"))
    }

    fn assign(&mut self, dst: &Self::Value, src: &Self::Value) -> Result<(), TenantError> {
        let idx = require_object(dst)?;
        if self.callables.contains_key(&idx) {
            let dst = dst.clone();
            let src = src.clone();
            return self.with_handler(idx, |tenant, handler| handler.assign(tenant, &dst, &src)).unwrap();
        }
        Err(unimplemented("assign (non-exotic target)"))
    }

    fn make_exotic(
        &mut self,
        _proto: Option<Self::Value>,
        _handler: Box<dyn TenantExoticHandler<Self>>,
    ) -> Result<Self::Value, TenantError> {
        Err(unimplemented("make_exotic (non-callable exotics aren't backed by this test double yet)"))
    }

    fn make_callable_exotic(
        &mut self,
        proto: Option<Self::Value>,
        handler: Box<dyn TenantCallableExoticHandler<Self>>,
    ) -> Result<Self::Value, TenantError> {
        self.objects.push(ObjectRecord {
            proto,
            extensible: true,
            props: HashMap::new(),
        });
        let idx = self.objects.len() - 1;
        self.callables.insert(idx, handler);
        Ok(TestValue::Object(idx))
    }

    fn invoke(&mut self, callee: &Self::Value, invocation: TenantInvocation<Self::Value>) -> Result<Self::Value, TenantError> {
        let idx = require_object(callee)?;
        let callee = callee.clone();
        self.with_handler(idx, |tenant, handler| match invocation {
            TenantInvocation::Apply { this_arg, args } => handler.apply(tenant, &callee, &this_arg, &args),
            TenantInvocation::Construct { args, new_target } => handler.construct(tenant, &callee, &new_target, &args),
        })
        .unwrap_or_else(|| Err(unimplemented("invoke (not a callable exotic)")))
    }

    fn invoke_trap(&mut self, _f: &Self::Value, _receiver: &Self::Value, _args: &[Self::Value]) -> Result<Self::Value, TenantError> {
        Err(unimplemented("invoke_trap"))
    }

    fn object_id(&self, value: &Self::Value) -> Self::ObjectId {
        // Only object identity is meaningful for the non-tenant-keyed caches this backs; a
        // primitive collapses to a fixed sentinel since no current test double consumer keys a
        // cache off a primitive value.
        value.as_object().unwrap_or(usize::MAX)
    }
}

/// A synchronous test `HostAsyncCapability`: `enqueue_microtask` runs its job immediately, so
/// tests get deterministic ordering without an event loop.
#[derive(Default)]
pub struct SyncHostAsync;

impl HostAsyncCapability for SyncHostAsync {
    type Value = TestValue;

    fn enqueue_microtask(&mut self, job: Box<dyn FnOnce(&mut Self)>) {
        job(self);
    }

    fn observe(
        &mut self,
        _task: HostTaskId,
        _on_fulfilled: Box<dyn FnOnce(&mut Self, Self::Value)>,
        _on_rejected: Box<dyn FnOnce(&mut Self, Self::Value)>,
    ) {
    }

    fn on_unhandled_rejection(&mut self, _reason: Self::Value, _promise: Self::Value) {}
}

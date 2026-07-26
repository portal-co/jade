//! A minimal in-memory `Tenant` test double, used by `jade-primordial-rt`'s integration tests
//! to exercise generated primordial code end to end. Only the operations the tests actually
//! exercise have real behavior; everything else returns a clear "not implemented in test
//! double" error rather than panicking, so a test that unexpectedly reaches an unimplemented
//! path fails with a readable message instead of a stack unwind.

use std::collections::HashMap;

use portal_solutions_jade_tenant_rt::{
    HostAsyncCapability, HostTaskId, PropertyKey, Tenant, TenantCallableExoticHandler, TenantError,
    TenantExoticHandler, TenantInvocation, TenantPropertyDescriptor,
};

#[derive(Debug, Clone, Default)]
pub struct ObjectRecord {
    pub proto: Option<usize>,
    pub extensible: bool,
    pub props: HashMap<PropertyKey, TenantPropertyDescriptor<usize>>,
}

#[derive(Default)]
pub struct TestTenant {
    pub objects: Vec<ObjectRecord>,
}

fn unimplemented(op: &str) -> TenantError {
    TenantError::type_error(format!("test double: '{op}' is not implemented"))
}

impl Tenant for TestTenant {
    type Value = usize;
    type ObjectId = usize;

    fn make(&mut self, proto: Option<Self::Value>) -> Result<Self::Value, TenantError> {
        self.objects.push(ObjectRecord {
            proto,
            extensible: true,
            props: HashMap::new(),
        });
        Ok(self.objects.len() - 1)
    }

    fn get(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<Self::Value, TenantError> {
        self.objects[*obj]
            .props
            .get(key)
            .and_then(|d| d.value)
            .ok_or_else(|| unimplemented("get (missing/non-data property)"))
    }

    fn set(&mut self, obj: &Self::Value, key: &PropertyKey, value: Self::Value) -> Result<(), TenantError> {
        self.objects[*obj].props.insert(
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
        Ok(self.objects[*obj].props.contains_key(key))
    }

    fn delete(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<(), TenantError> {
        self.objects[*obj].props.remove(key);
        Ok(())
    }

    fn own_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError> {
        Ok(self.objects[*obj]
            .props
            .iter()
            .filter(|(_, d)| d.enumerable != Some(false))
            .map(|(k, _)| k.clone())
            .collect())
    }

    fn own_property_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError> {
        Ok(self.objects[*obj].props.keys().cloned().collect())
    }

    fn get_own_property_descriptor(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
    ) -> Result<Option<TenantPropertyDescriptor<Self::Value>>, TenantError> {
        Ok(self.objects[*obj].props.get(key).cloned())
    }

    fn define_property(
        &mut self,
        obj: &Self::Value,
        key: &PropertyKey,
        descriptor: TenantPropertyDescriptor<Self::Value>,
    ) -> Result<bool, TenantError> {
        let record = &mut self.objects[*obj];
        if !record.props.contains_key(key) && !record.extensible {
            return Ok(false);
        }
        record.props.insert(key.clone(), descriptor);
        Ok(true)
    }

    fn get_prototype_of(&mut self, obj: &Self::Value) -> Result<Option<Self::Value>, TenantError> {
        Ok(self.objects[*obj].proto)
    }

    fn set_prototype_of(&mut self, obj: &Self::Value, prototype: Option<Self::Value>) -> Result<bool, TenantError> {
        self.objects[*obj].proto = prototype;
        Ok(true)
    }

    fn is_extensible(&mut self, obj: &Self::Value) -> Result<bool, TenantError> {
        Ok(self.objects[*obj].extensible)
    }

    fn prevent_extensions(&mut self, obj: &Self::Value) -> Result<bool, TenantError> {
        self.objects[*obj].extensible = false;
        Ok(true)
    }

    fn define(&mut self, _target: &Self::Value, _descriptors: &Self::Value) -> Result<(), TenantError> {
        Err(unimplemented("define"))
    }

    fn assign(&mut self, _dst: &Self::Value, _src: &Self::Value) -> Result<(), TenantError> {
        Err(unimplemented("assign"))
    }

    fn make_exotic(
        &mut self,
        _proto: Option<Self::Value>,
        _handler: Box<dyn TenantExoticHandler<Self::Value>>,
    ) -> Result<Self::Value, TenantError> {
        Err(unimplemented("make_exotic"))
    }

    fn make_callable_exotic(
        &mut self,
        _proto: Option<Self::Value>,
        _handler: Box<dyn TenantCallableExoticHandler<Self::Value>>,
    ) -> Result<Self::Value, TenantError> {
        Err(unimplemented("make_callable_exotic"))
    }

    fn invoke(&mut self, _callee: &Self::Value, _invocation: TenantInvocation<Self::Value>) -> Result<Self::Value, TenantError> {
        Err(unimplemented("invoke"))
    }

    fn invoke_trap(&mut self, _f: &Self::Value, _receiver: &Self::Value, _args: &[Self::Value]) -> Result<Self::Value, TenantError> {
        Err(unimplemented("invoke_trap"))
    }

    fn object_id(&self, value: &Self::Value) -> Self::ObjectId {
        *value
    }
}

/// A synchronous test `HostAsyncCapability`: `enqueue_microtask` runs its job immediately, so
/// tests get deterministic ordering without an event loop.
#[derive(Default)]
pub struct SyncHostAsync;

impl HostAsyncCapability for SyncHostAsync {
    type Value = usize;

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

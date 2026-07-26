/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/object.ts. */
#[allow(unused_imports)]
use portal_solutions_jade_tenant_rt::{
    DynFields, PropertyKey, Tenant, TenantError, TenantPropertyDescriptor, ValueTag,
};
pub struct ObjectPrimordial<T: Tenant> {
    pub object: T::Value,
    pub object_prototype: T::Value,
}
impl<T: Tenant> Clone for ObjectPrimordial<T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            object_prototype: self.object_prototype.clone(),
        }
    }
}
pub struct ObjectPrimordialCache<T: Tenant> {
    entry: Option<ObjectPrimordial<T>>,
}
impl<T: Tenant> Default for ObjectPrimordialCache<T> {
    fn default() -> Self {
        Self { entry: None }
    }
}
pub fn install_method<T: Tenant + 'static>(
    tenant: &mut T,
    target: &T::Value,
    name: &str,
    apply: impl FnMut(&mut T, T::Value, &[T::Value]) -> Result<T::Value, TenantError> + 'static,
) -> Result<T::Value, TenantError> {
    let mut fn_ = (crate::types_shim::make_builtin(tenant, name, apply, None, None))?;
    (crate::types_shim::define_data(
        tenant,
        target,
        &PropertyKey::from(name),
        &fn_,
        TenantPropertyDescriptor {
            value: None,
            writable: Some(true),
            get: None,
            set: None,
            enumerable: None,
            configurable: Some(true),
        },
    ))?;
    return Ok(fn_);
}
pub fn lock<T: Tenant + 'static>(
    tenant: &mut T,
    object: &T::Value,
    freeze: bool,
) -> Result<T::Value, TenantError> {
    for key in (tenant.own_property_keys(object))? {
        let mut descriptor = (tenant.get_own_property_descriptor(object, &key))?;
        let Some(descriptor) = descriptor else {
            continue;
        };
        let mut next = TenantPropertyDescriptor {
            configurable: Some(false),
            ..(descriptor).clone()
        };
        if (freeze && ((&next).has_field("value") || (next.writable.is_some()))) {
            next.writable = Some(false);
        }
        if (!((tenant.define_property(object, &key, next))?)) {
            return Err(TenantError::TypeError(
                ("cannot update object descriptor").to_string(),
            ));
        }
    }
    if (!((tenant.prevent_extensions(object))?)) {
        return Err(TenantError::TypeError(
            ("cannot prevent extensions").to_string(),
        ));
    }
    return Ok((object).clone());
}

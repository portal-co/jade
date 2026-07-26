/* This is GENERATED code by gen-primordials, from packages/jade-js/primordials/defineData.ts. */
#[allow(unused_imports)]
use portal_solutions_jade_tenant_rt::{
    DynFields, PropertyKey, Tenant, TenantError, TenantPropertyDescriptor,
};
pub fn define_data<T: Tenant>(
    tenant: &mut T,
    target: &T::Value,
    key: &PropertyKey,
    value: &T::Value,
    attributes: TenantPropertyDescriptor<T::Value>,
) -> Result<(), TenantError> {
    let ok = (tenant.define_property(
        target,
        key,
        TenantPropertyDescriptor {
            value: Some((value).clone()),
            writable: Some((attributes.writable).unwrap_or(true)),
            get: None,
            set: None,
            enumerable: Some((attributes.enumerable).unwrap_or(false)),
            configurable: Some((attributes.configurable).unwrap_or(true)),
        },
    ))?;
    if (!ok) {
        return Err(TenantError::TypeError(
            (format!("cannot define primordial property {}", (key).to_string())).to_string(),
        ));
    }
    Ok(())
}

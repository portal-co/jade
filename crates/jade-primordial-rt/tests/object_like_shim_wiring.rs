//! Proves the shim-registry wiring end to end: `set_name` below is the exact output of
//! `gen-primordials --rust` run over a snippet that imports `assertObject`/`defineData` from
//! `./types.ts` (see the primordial-IR plan's shimming addendum) — pasted here verbatim rather
//! than re-run by a build script, since Phase 8's real sweep-and-check-in tooling isn't built
//! yet. If this ever drifts from what the tool actually emits, regenerate it with:
//!   cargo run -p portal-solutions-jade-primordial-ir --bin gen-primordials -- --file <snippet> --rust

mod support;

use portal_solutions_jade_tenant_rt::{DynFields, PropertyKey, Tenant, TenantError, TenantPropertyDescriptor};
use support::{TestTenant, TestValue};

pub fn set_name<T: Tenant>(tenant: &mut T, target: &T::Value, name: &T::Value) -> Result<(), TenantError> {
    (portal_solutions_jade_primordial_rt::types_shim::assert_object(tenant, target, "expected an object"))?;
    (portal_solutions_jade_primordial_rt::types_shim::define_data(
        tenant,
        target,
        &PropertyKey::from("name"),
        name,
        TenantPropertyDescriptor {
            value: None,
            writable: Some(false),
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        },
    ))?;
    Ok(())
}

#[test]
fn sets_the_name_property_via_the_shimmed_helpers() {
    let mut tenant = TestTenant::default();
    let target = tenant.make(None).unwrap();

    set_name(&mut tenant, &target, &TestValue::Str("widget".to_string())).unwrap();

    let descriptor = tenant.get_own_property_descriptor(&target, &PropertyKey::from("name")).unwrap().unwrap();
    assert_eq!(descriptor.value, Some(TestValue::Str("widget".to_string())));
    assert_eq!(descriptor.writable, Some(false));
}

#[test]
fn rejects_a_non_object_target() {
    let mut tenant = TestTenant::default();
    let err = set_name(&mut tenant, &TestValue::Number(1.0), &TestValue::Str("x".to_string())).unwrap_err();
    assert!(matches!(err, TenantError::TypeError(m) if m == "expected an object"));
}

mod support;

use portal_solutions_jade_primordial_rt::types_shim::define_data;
use portal_solutions_jade_tenant_rt::{PropertyKey, Tenant, TenantPropertyDescriptor};
use support::{TestTenant, TestValue};

#[test]
fn defines_a_data_property_with_defaults() {
    let mut tenant = TestTenant::default();
    let target = tenant.make(None).unwrap();

    define_data(
        &mut tenant,
        &target,
        &PropertyKey::from("name"),
        &TestValue::Number(42.0),
        TenantPropertyDescriptor::default(),
    )
    .expect("define_data should succeed");

    let descriptor = tenant
        .get_own_property_descriptor(&target, &PropertyKey::from("name"))
        .unwrap()
        .expect("property should exist");
    assert_eq!(descriptor.value, Some(TestValue::Number(42.0)));
    assert_eq!(descriptor.writable, Some(true));
    assert_eq!(descriptor.enumerable, Some(false));
    assert_eq!(descriptor.configurable, Some(true));
}

#[test]
fn honors_explicit_attributes() {
    let mut tenant = TestTenant::default();
    let target = tenant.make(None).unwrap();

    define_data(
        &mut tenant,
        &target,
        &PropertyKey::from("frozen"),
        &TestValue::Number(7.0),
        TenantPropertyDescriptor {
            writable: Some(false),
            enumerable: Some(true),
            configurable: Some(false),
            ..Default::default()
        },
    )
    .expect("define_data should succeed");

    let descriptor = tenant
        .get_own_property_descriptor(&target, &PropertyKey::from("frozen"))
        .unwrap()
        .expect("property should exist");
    assert_eq!(descriptor.value, Some(TestValue::Number(7.0)));
    assert_eq!(descriptor.writable, Some(false));
    assert_eq!(descriptor.enumerable, Some(true));
    assert_eq!(descriptor.configurable, Some(false));
}

#[test]
fn fails_closed_when_the_target_is_non_extensible_and_the_key_is_new() {
    let mut tenant = TestTenant::default();
    let target = tenant.make(None).unwrap();
    tenant.prevent_extensions(&target).unwrap();

    let err = define_data(
        &mut tenant,
        &target,
        &PropertyKey::from("blocked"),
        &TestValue::Number(1.0),
        TenantPropertyDescriptor::default(),
    )
    .expect_err("define_data should fail on a non-extensible target");

    assert!(matches!(err, portal_solutions_jade_tenant_rt::TenantError::TypeError(_)));
}

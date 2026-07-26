//! Behavioral tests for the parts of the IR-generated `object.rs` that currently compile —
//! `install_method` and `lock` (`ObjectPrimordial`/`object_primordial` itself don't yet: see
//! their `compile_error!` in `crates/jade-primordial-rt/src/object.rs`, a named, principled gap
//! — `Object.keys`'s return value needs a guest Array primordial that doesn't exist yet).

mod support;

use portal_solutions_jade_primordial_rt::object::{install_method, lock};
use portal_solutions_jade_tenant_rt::{PropertyKey, Tenant, TenantInvocation};
use support::{TestTenant, TestValue};

#[test]
fn install_method_defines_a_writable_configurable_builtin() {
    let mut tenant = TestTenant::default();
    let target = tenant.make(None).unwrap();

    let installed = install_method(&mut tenant, &target, "double", |_tenant, _this_arg, args| {
        let TestValue::Number(n) = args[0] else { panic!("expected a number arg") };
        Ok(TestValue::Number(n * 2.0))
    })
    .expect("install_method should succeed");

    // The installed function is reachable from `target` under the given name...
    let looked_up = tenant.get(&target, &PropertyKey::from("double")).unwrap();
    assert_eq!(looked_up, installed);

    let descriptor = tenant
        .get_own_property_descriptor(&target, &PropertyKey::from("double"))
        .unwrap()
        .expect("property should exist");
    assert_eq!(descriptor.writable, Some(true));
    assert_eq!(descriptor.configurable, Some(true));
    assert_eq!(descriptor.enumerable, Some(false));

    // ...and calling it actually runs the closure through the tenant invocation ABI.
    let result = tenant
        .invoke(
            &installed,
            TenantInvocation::Apply { this_arg: TestValue::Undefined, args: vec![TestValue::Number(21.0)] },
        )
        .unwrap();
    assert_eq!(result, TestValue::Number(42.0));
}

#[test]
fn lock_seals_every_own_property_and_prevents_extensions() {
    let mut tenant = TestTenant::default();
    let target = tenant.make(None).unwrap();
    tenant
        .define_property(
            &target,
            &PropertyKey::from("x"),
            portal_solutions_jade_tenant_rt::TenantPropertyDescriptor {
                value: Some(TestValue::Number(1.0)),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            },
        )
        .unwrap();

    lock(&mut tenant, &target, /* freeze */ false).expect("seal should succeed");

    let descriptor = tenant.get_own_property_descriptor(&target, &PropertyKey::from("x")).unwrap().unwrap();
    assert_eq!(descriptor.configurable, Some(false));
    // seal (freeze=false) leaves `writable` alone.
    assert_eq!(descriptor.writable, Some(true));
    assert!(!tenant.is_extensible(&target).unwrap());
}

#[test]
fn lock_with_freeze_also_clears_writable() {
    let mut tenant = TestTenant::default();
    let target = tenant.make(None).unwrap();
    tenant
        .define_property(
            &target,
            &PropertyKey::from("x"),
            portal_solutions_jade_tenant_rt::TenantPropertyDescriptor {
                value: Some(TestValue::Number(1.0)),
                writable: Some(true),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            },
        )
        .unwrap();

    lock(&mut tenant, &target, /* freeze */ true).expect("freeze should succeed");

    let descriptor = tenant.get_own_property_descriptor(&target, &PropertyKey::from("x")).unwrap().unwrap();
    assert_eq!(descriptor.configurable, Some(false));
    assert_eq!(descriptor.writable, Some(false));
}

mod support;

use portal_solutions_jade_primordial_rt::types_shim::{
    assert_object, descriptor_object, guest_array_like, make_builtin, read_guest_descriptor, to_index,
};
use portal_solutions_jade_tenant_rt::{PropertyKey, Tenant, TenantError, TenantInvocation, TenantPropertyDescriptor};
use support::{TestTenant, TestValue};

#[test]
fn assert_object_accepts_objects_and_rejects_nullish_and_primitives() {
    let mut tenant = TestTenant::default();
    let obj = tenant.make(None).unwrap();
    assert!(assert_object(&tenant, &obj, "x").is_ok());

    for bad in [TestValue::Null, TestValue::Undefined, TestValue::Number(1.0), TestValue::Bool(true)] {
        let err = assert_object(&tenant, &bad, "expected an object").unwrap_err();
        assert!(matches!(err, TenantError::TypeError(m) if m == "expected an object"));
    }
}

#[test]
fn to_index_accepts_non_negative_integers_and_rejects_the_rest() {
    let tenant = TestTenant::default();
    assert_eq!(to_index(&tenant, &TestValue::Number(0.0)).unwrap(), 0);
    assert_eq!(to_index(&tenant, &TestValue::Number(7.0)).unwrap(), 7);

    for bad in [TestValue::Number(-1.0), TestValue::Number(1.5), TestValue::Number(f64::NAN)] {
        let err = to_index(&tenant, &bad).unwrap_err();
        assert!(matches!(err, TenantError::RangeError(_)));
    }
}

#[test]
fn descriptor_round_trips_through_a_guest_object() {
    let mut tenant = TestTenant::default();
    let value = TestValue::Number(9.0);
    let descriptor = TenantPropertyDescriptor {
        value: Some(value.clone()),
        writable: Some(true),
        enumerable: Some(false),
        configurable: Some(true),
        ..Default::default()
    };

    let guest = descriptor_object(&mut tenant, &descriptor).unwrap();
    let round_tripped = read_guest_descriptor(&mut tenant, &guest).unwrap();

    assert_eq!(round_tripped.value, Some(value));
    assert_eq!(round_tripped.writable, Some(true));
    assert_eq!(round_tripped.enumerable, Some(false));
    assert_eq!(round_tripped.configurable, Some(true));
    assert_eq!(round_tripped.get, None);
    assert_eq!(round_tripped.set, None);
}

#[test]
fn guest_array_like_reads_dense_numeric_keys_in_order() {
    let mut tenant = TestTenant::default();
    let arr = tenant.make(None).unwrap();
    tenant.set(&arr, &PropertyKey::from("1"), TestValue::Number(20.0)).unwrap();
    tenant.set(&arr, &PropertyKey::from("0"), TestValue::Number(10.0)).unwrap();
    tenant.set(&arr, &PropertyKey::from("2"), TestValue::Number(30.0)).unwrap();
    // A non-index key must not appear in the result.
    tenant.set(&arr, &PropertyKey::from("length"), TestValue::Number(3.0)).unwrap();

    let values = guest_array_like(&mut tenant, &arr).unwrap();
    assert_eq!(values, vec![TestValue::Number(10.0), TestValue::Number(20.0), TestValue::Number(30.0)]);
}

#[test]
fn make_builtin_creates_a_callable_with_a_name_property_and_apply_behavior() {
    let mut tenant = TestTenant::default();
    let builtin = make_builtin(
        &mut tenant,
        "double",
        |tenant, _this_arg, args| {
            let n = tenant.to_number(&args[0]);
            Ok(tenant.number_value(n * 2.0))
        },
        None,
        None,
    )
    .unwrap();

    let name = tenant.get(&builtin, &PropertyKey::from("name")).unwrap();
    assert_eq!(name, TestValue::Str("double".to_string()));

    let result = tenant
        .invoke(
            &builtin,
            TenantInvocation::Apply {
                this_arg: TestValue::Undefined,
                args: vec![TestValue::Number(21.0)],
            },
        )
        .unwrap();
    assert_eq!(result, TestValue::Number(42.0));
}

#[test]
fn make_builtin_without_a_construct_closure_rejects_construction() {
    let mut tenant = TestTenant::default();
    let builtin = make_builtin(&mut tenant, "notCtor", |_tenant, _this_arg, _args| Ok(TestValue::Undefined), None, None).unwrap();

    let err = tenant
        .invoke(
            &builtin,
            TenantInvocation::Construct {
                args: vec![],
                new_target: builtin.clone(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, TenantError::TypeError(m) if m == "notCtor is not a constructor"));
}

#[test]
fn make_builtin_own_properties_are_independent_per_instance() {
    let mut tenant = TestTenant::default();
    let a = make_builtin(&mut tenant, "a", |_t, _this, _args| Ok(TestValue::Undefined), None, None).unwrap();
    let b = make_builtin(&mut tenant, "b", |_t, _this, _args| Ok(TestValue::Undefined), None, None).unwrap();

    tenant.set(&a, &PropertyKey::from("extra"), TestValue::Number(1.0)).unwrap();
    assert!(tenant.has(&a, &PropertyKey::from("extra")).unwrap());
    assert!(!tenant.has(&b, &PropertyKey::from("extra")).unwrap());
}

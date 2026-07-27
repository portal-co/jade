//! Behavioral tests for `reflect_primordial` (`reflect.ts`) — the second cross-file consumer of
//! `object_primordial` (for `ObjectPrototype`), and the second use of
//! `Tenant::indexed_collection`/`property_key_value` (`Reflect.ownKeys`, same shape as
//! `Object.keys` in `object_primordial.rs`).

mod support;

use portal_solutions_jade_primordial_rt::object::ObjectPrimordialCache;
use portal_solutions_jade_primordial_rt::reflect::{reflect_primordial, ReflectPrimordialCache};
use portal_solutions_jade_primordial_rt::types_shim::make_builtin;
use portal_solutions_jade_tenant_rt::{PropertyKey, Tenant, TenantInvocation};
use support::{TestTenant, TestValue};

fn setup(tenant: &mut TestTenant) -> TestValue {
    let mut reflect_cache = ReflectPrimordialCache::default();
    let mut object_cache = ObjectPrimordialCache::default();
    reflect_primordial(tenant, &mut reflect_cache, &mut object_cache).unwrap().reflect
}

fn call_method(tenant: &mut TestTenant, reflect: &TestValue, name: &str, args: Vec<TestValue>) -> TestValue {
    let method = tenant.get(reflect, &PropertyKey::from(name)).unwrap();
    tenant.invoke(&method, TenantInvocation::Apply { this_arg: TestValue::Undefined, args }).unwrap()
}

#[test]
fn same_tenant_gets_the_same_cached_reflect_primordial() {
    let mut tenant = TestTenant::default();
    let mut reflect_cache = ReflectPrimordialCache::default();
    let mut object_cache = ObjectPrimordialCache::default();

    let first = reflect_primordial(&mut tenant, &mut reflect_cache, &mut object_cache).unwrap();
    let second = reflect_primordial(&mut tenant, &mut reflect_cache, &mut object_cache).unwrap();
    assert_eq!(first.reflect, second.reflect);
}

#[test]
fn get_set_has_delete_property_round_trip() {
    let mut tenant = TestTenant::default();
    let reflect = setup(&mut tenant);
    let target = tenant.make(None).unwrap();

    let set_ok = call_method(&mut tenant, &reflect, "set", vec![target.clone(), TestValue::Str("x".into()), TestValue::Number(1.0)]);
    assert_eq!(set_ok, TestValue::Bool(true));

    let has = call_method(&mut tenant, &reflect, "has", vec![target.clone(), TestValue::Str("x".into())]);
    assert_eq!(has, TestValue::Bool(true));

    let got = call_method(&mut tenant, &reflect, "get", vec![target.clone(), TestValue::Str("x".into())]);
    assert_eq!(got, TestValue::Number(1.0));

    let deleted = call_method(&mut tenant, &reflect, "deleteProperty", vec![target.clone(), TestValue::Str("x".into())]);
    assert_eq!(deleted, TestValue::Bool(true));
    let has_after = call_method(&mut tenant, &reflect, "has", vec![target, TestValue::Str("x".into())]);
    assert_eq!(has_after, TestValue::Bool(false));
}

#[test]
fn own_keys_returns_an_indexed_collection() {
    let mut tenant = TestTenant::default();
    let reflect = setup(&mut tenant);
    let target = tenant.make(None).unwrap();
    tenant.set(&target, &PropertyKey::from("only"), TestValue::Bool(true)).unwrap();

    let keys = call_method(&mut tenant, &reflect, "ownKeys", vec![target]);
    assert_eq!(keys, TestValue::List(vec![TestValue::Str("only".to_string())]));
}

#[test]
fn get_own_property_descriptor_round_trips_through_reflect_define_property() {
    let mut tenant = TestTenant::default();
    let reflect = setup(&mut tenant);
    let target = tenant.make(None).unwrap();

    let descriptor = tenant.make(None).unwrap();
    tenant.set(&descriptor, &PropertyKey::from("value"), TestValue::Number(7.0)).unwrap();
    tenant.set(&descriptor, &PropertyKey::from("writable"), TestValue::Bool(true)).unwrap();
    tenant.set(&descriptor, &PropertyKey::from("enumerable"), TestValue::Bool(true)).unwrap();
    tenant.set(&descriptor, &PropertyKey::from("configurable"), TestValue::Bool(true)).unwrap();

    let defined = call_method(&mut tenant, &reflect, "defineProperty", vec![target.clone(), TestValue::Str("y".into()), descriptor]);
    assert_eq!(defined, TestValue::Bool(true));

    let got_descriptor = call_method(&mut tenant, &reflect, "getOwnPropertyDescriptor", vec![target, TestValue::Str("y".into())]);
    let TestValue::Object(_) = got_descriptor else { panic!("expected a materialized descriptor object, got {got_descriptor:?}") };
    let value = tenant.get(&got_descriptor, &PropertyKey::from("value")).unwrap();
    assert_eq!(value, TestValue::Number(7.0));
}

#[test]
fn get_own_property_descriptor_returns_undefined_for_a_missing_key() {
    let mut tenant = TestTenant::default();
    let reflect = setup(&mut tenant);
    let target = tenant.make(None).unwrap();

    let result = call_method(&mut tenant, &reflect, "getOwnPropertyDescriptor", vec![target, TestValue::Str("missing".into())]);
    assert_eq!(result, TestValue::Undefined);
}

#[test]
fn apply_invokes_with_this_arg_and_a_guest_array_like_arguments_object() {
    let mut tenant = TestTenant::default();
    let reflect = setup(&mut tenant);
    let echo = make_builtin(
        &mut tenant,
        "echo",
        |_tenant, this_arg, args| {
            let mut packed = vec![this_arg];
            packed.extend_from_slice(args);
            Ok(TestValue::List(packed))
        },
        None,
        None,
    )
    .unwrap();

    let arg_list = tenant.make(None).unwrap();
    tenant.set(&arg_list, &PropertyKey::from("0"), TestValue::Number(1.0)).unwrap();

    let result = call_method(&mut tenant, &reflect, "apply", vec![echo, TestValue::Str("this".into()), arg_list]);
    assert_eq!(result, TestValue::List(vec![TestValue::Str("this".into()), TestValue::Number(1.0)]));
}

#[test]
fn construct_invokes_with_a_new_target_defaulting_to_the_target_itself() {
    let mut tenant = TestTenant::default();
    let reflect = setup(&mut tenant);
    let ctor = make_builtin(
        &mut tenant,
        "Ctor",
        |_tenant, _this_arg, _args| Ok(TestValue::Undefined),
        Some(Box::new(|_tenant, new_target, _args| Ok(new_target))),
        None,
    )
    .unwrap();

    let empty_args = tenant.make(None).unwrap();
    let result = call_method(&mut tenant, &reflect, "construct", vec![ctor.clone(), empty_args]);
    assert_eq!(result, ctor);
}

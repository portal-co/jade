//! Behavioral tests for `object_primordial` itself (`object.ts`'s `Object` factory) — the piece
//! `docs/array-primordial-gap-plan.md` was blocking until `Tenant::indexed_collection`/
//! `property_key_value` existed. `object_helpers.rs` covers `install_method`/`lock` in
//! isolation; this file covers the assembled `Object` builtin's own methods.

mod support;

use portal_solutions_jade_primordial_rt::object::{object_primordial, ObjectPrimordialCache};
use portal_solutions_jade_tenant_rt::{PropertyKey, Tenant, TenantInvocation};
use support::{TestTenant, TestValue};

fn call_method(tenant: &mut TestTenant, object_fn: &TestValue, name: &str, args: Vec<TestValue>) -> TestValue {
    let method = tenant.get(object_fn, &PropertyKey::from(name)).unwrap();
    tenant.invoke(&method, TenantInvocation::Apply { this_arg: TestValue::Undefined, args }).unwrap()
}

#[test]
fn same_tenant_gets_the_same_cached_object_primordial() {
    let mut tenant = TestTenant::default();
    let mut cache = ObjectPrimordialCache::default();

    let first = object_primordial(&mut tenant, &mut cache).unwrap();
    let second = object_primordial(&mut tenant, &mut cache).unwrap();

    assert_eq!(first.object, second.object);
    assert_eq!(first.object_prototype, second.object_prototype);
}

#[test]
fn distinct_caches_get_distinct_object_primordials() {
    let mut tenant = TestTenant::default();
    let mut cache_a = ObjectPrimordialCache::default();
    let mut cache_b = ObjectPrimordialCache::default();

    let a = object_primordial(&mut tenant, &mut cache_a).unwrap();
    let b = object_primordial(&mut tenant, &mut cache_b).unwrap();

    assert_ne!(a.object, b.object);
}

#[test]
fn object_keys_returns_an_indexed_collection_of_the_own_enumerable_keys() {
    let mut tenant = TestTenant::default();
    let mut cache = ObjectPrimordialCache::default();
    let object = object_primordial(&mut tenant, &mut cache).unwrap();

    let target = tenant.make(None).unwrap();
    tenant.set(&target, &PropertyKey::from("a"), TestValue::Number(1.0)).unwrap();
    tenant.set(&target, &PropertyKey::from("b"), TestValue::Number(2.0)).unwrap();

    let keys = call_method(&mut tenant, &object.object, "keys", vec![target]);
    let TestValue::List(keys) = keys else { panic!("Object.keys should return an indexed_collection, got {keys:?}") };
    assert_eq!(keys, vec![TestValue::Str("a".to_string()), TestValue::Str("b".to_string())]);
}

#[test]
fn object_keys_on_an_object_with_no_own_keys_returns_an_empty_collection() {
    let mut tenant = TestTenant::default();
    let mut cache = ObjectPrimordialCache::default();
    let object = object_primordial(&mut tenant, &mut cache).unwrap();
    let target = tenant.make(None).unwrap();

    let keys = call_method(&mut tenant, &object.object, "keys", vec![target]);
    assert_eq!(keys, TestValue::List(vec![]));
}

#[test]
fn object_create_allocates_with_the_given_prototype() {
    let mut tenant = TestTenant::default();
    let mut cache = ObjectPrimordialCache::default();
    let object = object_primordial(&mut tenant, &mut cache).unwrap();

    let proto = tenant.make(None).unwrap();
    let created = call_method(&mut tenant, &object.object, "create", vec![proto.clone()]);
    assert_eq!(tenant.get_prototype_of(&created).unwrap(), Some(proto));
}

#[test]
fn object_assign_copies_own_enumerable_properties_from_every_source() {
    let mut tenant = TestTenant::default();
    let mut cache = ObjectPrimordialCache::default();
    let object = object_primordial(&mut tenant, &mut cache).unwrap();

    let target = tenant.make(None).unwrap();
    let source = tenant.make(None).unwrap();
    tenant.set(&source, &PropertyKey::from("x"), TestValue::Number(9.0)).unwrap();

    call_method(&mut tenant, &object.object, "assign", vec![target.clone(), source]);
    assert_eq!(tenant.get(&target, &PropertyKey::from("x")).unwrap(), TestValue::Number(9.0));
}

#[test]
fn object_has_own_property_reports_only_own_keys() {
    let mut tenant = TestTenant::default();
    let mut cache = ObjectPrimordialCache::default();
    let object = object_primordial(&mut tenant, &mut cache).unwrap();

    let proto = tenant.make(None).unwrap();
    tenant.set(&proto, &PropertyKey::from("inherited"), TestValue::Bool(true)).unwrap();
    let target = tenant.make(Some(proto)).unwrap();
    tenant.set(&target, &PropertyKey::from("own"), TestValue::Bool(true)).unwrap();

    let has_own_property = tenant.get(&object.object_prototype, &PropertyKey::from("hasOwnProperty")).unwrap();
    let owns_own = tenant
        .invoke(&has_own_property, TenantInvocation::Apply { this_arg: target.clone(), args: vec![TestValue::Str("own".into())] })
        .unwrap();
    let owns_inherited = tenant
        .invoke(&has_own_property, TenantInvocation::Apply { this_arg: target, args: vec![TestValue::Str("inherited".into())] })
        .unwrap();
    assert_eq!(owns_own, TestValue::Bool(true));
    assert_eq!(owns_inherited, TestValue::Bool(false));
}

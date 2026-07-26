//! Behavioral tests for `function_primordial` (`function.ts`) — the first cross-file consumer
//! of `object_primordial` (for `ObjectPrototype`), exercising `Function.prototype.call`/`apply`/
//! `bind` through the tenant invocation ABI. See `docs/array-primordial-gap-plan.md`: this file
//! was generated but unwired until `object.rs`'s `Object.keys` gap closed.

mod support;

use portal_solutions_jade_primordial_rt::function::{function_primordial, FunctionPrimordialCache};
use portal_solutions_jade_primordial_rt::object::ObjectPrimordialCache;
use portal_solutions_jade_primordial_rt::types_shim::make_builtin;
use portal_solutions_jade_tenant_rt::{PropertyKey, Tenant, TenantInvocation};
use support::{TestTenant, TestValue};

/// A test callable that returns `[thisArg, args...]` packed as a `TestValue::List`, so a single
/// assertion can check both what `thisArg` and what `args` a given invocation style produced.
fn make_echo(tenant: &mut TestTenant) -> TestValue {
    make_builtin(
        tenant,
        "echo",
        |_tenant, this_arg, args| {
            let mut packed = vec![this_arg];
            packed.extend_from_slice(args);
            Ok(TestValue::List(packed))
        },
        None,
        None,
    )
    .unwrap()
}

fn call_method(tenant: &mut TestTenant, function_prototype: &TestValue, name: &str, this_arg: TestValue, args: Vec<TestValue>) -> TestValue {
    let method = tenant.get(function_prototype, &PropertyKey::from(name)).unwrap();
    tenant.invoke(&method, TenantInvocation::Apply { this_arg, args }).unwrap()
}

fn setup(tenant: &mut TestTenant) -> portal_solutions_jade_primordial_rt::function::FunctionPrimordial<TestTenant> {
    let mut function_cache = FunctionPrimordialCache::default();
    let mut object_cache = ObjectPrimordialCache::default();
    function_primordial(tenant, &mut function_cache, &mut object_cache).unwrap()
}

#[test]
fn same_tenant_gets_the_same_cached_function_primordial() {
    let mut tenant = TestTenant::default();
    let mut function_cache = FunctionPrimordialCache::default();
    let mut object_cache = ObjectPrimordialCache::default();

    let first = function_primordial(&mut tenant, &mut function_cache, &mut object_cache).unwrap();
    let second = function_primordial(&mut tenant, &mut function_cache, &mut object_cache).unwrap();
    assert_eq!(first.function, second.function);
}

#[test]
fn call_invokes_with_the_given_this_arg_and_args() {
    let mut tenant = TestTenant::default();
    let function = setup(&mut tenant);
    let echo = make_echo(&mut tenant);

    let result = call_method(
        &mut tenant,
        &function.function_prototype,
        "call",
        echo,
        vec![TestValue::Str("this".into()), TestValue::Number(1.0), TestValue::Number(2.0)],
    );
    assert_eq!(
        result,
        TestValue::List(vec![TestValue::Str("this".into()), TestValue::Number(1.0), TestValue::Number(2.0)])
    );
}

#[test]
fn apply_spreads_an_array_like_arguments_object() {
    let mut tenant = TestTenant::default();
    let function = setup(&mut tenant);
    let echo = make_echo(&mut tenant);

    let arg_list = tenant.make(None).unwrap();
    tenant.set(&arg_list, &PropertyKey::from("0"), TestValue::Number(10.0)).unwrap();
    tenant.set(&arg_list, &PropertyKey::from("1"), TestValue::Number(20.0)).unwrap();

    let result = call_method(&mut tenant, &function.function_prototype, "apply", echo, vec![TestValue::Str("this".into()), arg_list]);
    assert_eq!(result, TestValue::List(vec![TestValue::Str("this".into()), TestValue::Number(10.0), TestValue::Number(20.0)]));
}

#[test]
fn bind_fixes_this_arg_and_prepends_bound_args() {
    let mut tenant = TestTenant::default();
    let function = setup(&mut tenant);
    let echo = make_echo(&mut tenant);

    let bind = tenant.get(&function.function_prototype, &PropertyKey::from("bind")).unwrap();
    let bound = tenant
        .invoke(
            &bind,
            TenantInvocation::Apply { this_arg: echo, args: vec![TestValue::Str("bound-this".into()), TestValue::Number(1.0)] },
        )
        .unwrap();

    let result = tenant
        .invoke(&bound, TenantInvocation::Apply { this_arg: TestValue::Undefined, args: vec![TestValue::Number(2.0)] })
        .unwrap();
    assert_eq!(result, TestValue::List(vec![TestValue::Str("bound-this".into()), TestValue::Number(1.0), TestValue::Number(2.0)]));
}

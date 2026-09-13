//! The pure-Rust cell (`env = "native"`): execute bytecode in-process against
//! `jade-tenant-rt`'s ObjectManager with primordials from `jade-primordial-rt` — no
//! Node, no JS engine anywhere in the process.
//!
//! The realm mirrors `createPrimordialRealm` minus the primordials `jade-primordial-rt`
//! does not generate (Promise — the native interpreter is sync-only — and the buffer
//! family); everything actually installed is byte-for-byte the TS realm's install:
//! `Object`/`Function` + the `globalThis` self-reference + `undefined`/`NaN`/`Infinity`,
//! all via `define_data` (non-writable, non-enumerable, configurable).
//!
//! The harness mirrors `packages/jade-js/test262/harness.ts` (`sta.js`/`assert.js`
//! equivalents), hand-written against tenant operations exactly like the TS version.
//! The one deliberate divergence is the failure channel: the TS harness *throws* a
//! host `Test262Error` out of the assertion and the driver catches it; the native
//! harness *returns* `TenantError::TypeError("Test262Error: <msg>")` from the
//! assertion's apply (Rust has no host exceptions), and `execute_cell` classifies the
//! prefix back into `fail "assertion failed"`.

use portal_solutions_jade_primordial_rt::function::{FunctionPrimordialCache, function_primordial};
use portal_solutions_jade_primordial_rt::object::{ObjectPrimordialCache, object_primordial};
use portal_solutions_jade_primordial_rt::types_shim::define_data;
use portal_solutions_jade_tenant_rt::object_manager::{GuestCallable, ObjectManager, Value};
use portal_solutions_jade_tenant_rt::{
    PropertyKey, Tenant, TenantError, TenantPropertyDescriptor,
};
use portal_solutions_jade_vm_native as vm_native;

use crate::model::{CellVerdict, VerdictKind};

/// Execute one compiled test in-process. `harness` is `!meta.has_flag("raw")`.
pub fn execute_cell(bytes: &[u8], harness: bool) -> CellVerdict {
    let mut tenant = ObjectManager::new();
    let global_this = match create_realm(&mut tenant) {
        Ok(g) => g,
        Err(e) => {
            let mut c = CellVerdict::new(VerdictKind::Crash);
            c.error = Some(format!("realm setup: {e}"));
            return c;
        }
    };
    if harness {
        if let Err(e) = install_harness(&mut tenant, &global_this) {
            let mut c = CellVerdict::new(VerdictKind::Crash);
            c.error = Some(format!("harness setup: {e}"));
            return c;
        }
    }
    match vm_native::run(bytes, &global_this, &mut tenant) {
        Ok(value) => {
            let mut c = CellVerdict::new(VerdictKind::Pass);
            c.value = Some(snapshot(&mut tenant, &value));
            c
        }
        Err(e) => {
            let text = e.to_string();
            let assertion = text.contains("Test262Error");
            let mut c = CellVerdict::fail(if assertion {
                "assertion failed".to_string()
            } else {
                "threw during execution".to_string()
            });
            c.error = Some(text);
            c
        }
    }
}

/// The native subset of `createPrimordialRealm`: Object/Function + globalThis
/// self-reference + value globals. Returns the realm global object.
fn create_realm(tenant: &mut ObjectManager) -> Result<Value, TenantError> {
    let mut object_cache = ObjectPrimordialCache::default();
    let mut function_cache = FunctionPrimordialCache::default();
    let object = object_primordial(tenant, &mut object_cache)?;
    let functions = function_primordial(tenant, &mut function_cache, &mut object_cache)?;
    let global_this = tenant.make(Some(object.object_prototype.clone()))?;
    let undef = tenant.undefined_value();
    for (key, value) in [
        ("Object", object.object.clone()),
        ("Function", functions.function.clone()),
        ("globalThis", global_this.clone()),
        ("undefined", undef.clone()),
        ("NaN", tenant.number_value(f64::NAN)),
        ("Infinity", tenant.number_value(f64::INFINITY)),
    ] {
        define_data(tenant, &global_this, &PropertyKey::from(key), &value, data_attributes())?;
    }
    Ok(global_this)
}

/// `defineData` in the primordials installs non-writable, non-enumerable,
/// configurable data properties (the standard shape of a global binding).
fn data_attributes() -> TenantPropertyDescriptor<Value> {
    TenantPropertyDescriptor {
        writable: Some(false),
        enumerable: Some(false),
        configurable: Some(true),
        ..Default::default()
    }
}

/// A guest function adopted by the tenant — the native counterpart of harness.ts's
/// `tenant.makeFunction(impl)` shells: callable via `apply`, but otherwise an ordinary
/// shadow-property object (so `tenant.set(assert, "sameValue", …)` works on it exactly
/// like on the TS harness's adopted functions, which are *not* exotics).
struct HostFn<F>(F);

impl<F> GuestCallable for HostFn<F>
where
    F: FnMut(&mut ObjectManager, Value, Vec<Value>) -> Result<Value, TenantError>,
{
    fn apply(
        &mut self,
        tenant: &mut ObjectManager,
        this_arg: &Value,
        args: &[Value],
    ) -> Result<Value, TenantError> {
        (self.0)(tenant, this_arg.clone(), args.to_vec())
    }
}

fn host_fn(
    tenant: &mut ObjectManager,
    apply: impl FnMut(&mut ObjectManager, Value, Vec<Value>) -> Result<Value, TenantError> + 'static,
) -> Result<Value, TenantError> {
    Ok(tenant.adopt_guest_fn(None, Box::new(HostFn(apply))))
}

fn set_global(
    tenant: &mut ObjectManager,
    global_this: &Value,
    key: &str,
    value: &Value,
) -> Result<(), TenantError> {
    // `tenant.set` on the global, mirroring harness.ts's `setGlobal`.
    tenant.set(global_this, &PropertyKey::from(key), value.clone())
}

fn test262_error(message: &str) -> TenantError {
    // Deliberate divergence from the TS harness (see the module doc): the assertion
    // *returns* the marker instead of throwing it. `execute_cell` classifies the prefix.
    TenantError::TypeError(format!("Test262Error: {message}"))
}

fn message_of(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `Object.is(a, b)` for the harness's SameValue assertions — the `object_is` helper in
/// `object_manager.rs` is module-private, so the two IEEE corners are restated here.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x.is_nan() && y.is_nan() {
                true
            } else if *x == 0.0 && *y == 0.0 {
                x.is_sign_negative() == y.is_sign_negative()
            } else {
                x == y
            }
        }
        _ => a == b,
    }
}

/// The native mirror of `installHarness`: Test262Error, $ERROR, assert (+ sameValue /
/// notSameValue / compareArray), print, $262.global.
fn install_harness(tenant: &mut ObjectManager, global_this: &Value) -> Result<(), TenantError> {
    // --- sta.js -------------------------------------------------------------------
    let error_ctor = host_fn(tenant, |_tenant, _this, args| {
        Err(test262_error(&format!(
            "constructed: {}",
            args.first().map(message_of).unwrap_or_default()
        )))
    })?;
    set_global(tenant, global_this, "Test262Error", &error_ctor)?;

    let dollar_error = host_fn(tenant, |_tenant, _this, args| {
        Err(test262_error(
            &args.first().map(message_of).unwrap_or_default(),
        ))
    })?;
    set_global(tenant, global_this, "$ERROR", &dollar_error)?;

    // --- assert.js ------------------------------------------------------------------
    let assert_fn = host_fn(tenant, |_tenant, _this, args| {
        let condition = args.first().cloned().unwrap_or(Value::Undefined);
        if !truthy(&condition) {
            let msg = args
                .get(1)
                .map(message_of)
                .filter(|m| !m.is_empty() && m != "undefined")
                .unwrap_or_else(|| "assertion failed".to_string());
            return Err(test262_error(&msg));
        }
        Ok(Value::Undefined)
    })?;

    let same_value_fn = host_fn(tenant, |_tenant, _this, args| {
        let actual = args.first().cloned().unwrap_or(Value::Undefined);
        let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
        if !same_value(&actual, &expected) {
            return Err(test262_error(&format!(
                "{}: expected {}, got {}",
                args.get(2)
                    .map(message_of)
                    .filter(|m| m != "undefined")
                    .unwrap_or_else(|| "assert.sameValue".to_string()),
                format_value(&expected),
                format_value(&actual),
            )));
        }
        Ok(Value::Undefined)
    })?;
    tenant.set(&assert_fn, &PropertyKey::from("sameValue"), same_value_fn)?;

    let not_same_value_fn = host_fn(tenant, |_tenant, _this, args| {
        let actual = args.first().cloned().unwrap_or(Value::Undefined);
        let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
        if same_value(&actual, &expected) {
            return Err(test262_error(&format!(
                "assert.notSameValue: expected anything but {}",
                format_value(&actual)
            )));
        }
        Ok(Value::Undefined)
    })?;
    tenant.set(&assert_fn, &PropertyKey::from("notSameValue"), not_same_value_fn)?;

    let compare_array_fn = host_fn(tenant, |tenant, _this, args| {
        let actual = args.first().cloned().unwrap_or(Value::Undefined);
        let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
        let mut len = |v: &Value, tenant: &mut ObjectManager| -> Result<Option<u32>, TenantError> {
            match tenant.get(v, &PropertyKey::from("length"))? {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => Ok(Some(n as u32)),
                _ => Ok(None),
            }
        };
        let (Some(alen), Some(elen)) = (len(&actual, tenant)?, len(&expected, tenant)?) else {
            return Err(test262_error("assert.compareArray: not array-like"));
        };
        if alen != elen {
            return Err(test262_error(&format!(
                "assert.compareArray: length {alen} != {elen}"
            )));
        }
        for i in 0..alen {
            let key = PropertyKey::from(i.to_string());
            let a = tenant.get(&actual, &key)?;
            let e = tenant.get(&expected, &key)?;
            if !same_value(&a, &e) {
                return Err(test262_error(&format!(
                    "assert.compareArray: index {i}: expected {}, got {}",
                    format_value(&e),
                    format_value(&a),
                )));
            }
        }
        Ok(Value::Undefined)
    })?;
    tenant.set(&assert_fn, &PropertyKey::from("compareArray"), compare_array_fn)?;
    set_global(tenant, global_this, "assert", &assert_fn)?;

    // --- misc -----------------------------------------------------------------------
    let print_fn = host_fn(tenant, |_tenant, _this, args| {
        println!(
            "[test] {}",
            args.iter().map(message_of).collect::<Vec<_>>().join(" ")
        );
        Ok(Value::Undefined)
    })?;
    set_global(tenant, global_this, "print", &print_fn)?;

    let dollar_262 = tenant.make(None)?;
    tenant.set(&dollar_262, &PropertyKey::from("global"), global_this.clone())?;
    set_global(tenant, global_this, "$262", &dollar_262)?;

    Ok(())
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        Value::Object(_) => true,
    }
}

fn format_value(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{s:?}"),
        other => message_of(other),
    }
}

/// Deterministic completion-value snapshot for the differential check, mirroring
/// `exec.ts`'s `snapshot`: undefined -> `[undefined]`, non-finite numbers -> null,
/// functions -> `[function anonymous]`, objects -> a JSON-shaped own-enumerable map
/// (the TS side gets the same shape from host `JSON.stringify`, which shallow-JSONs
/// host arrays/objects and sees through the one-frame shell).
fn snapshot(tenant: &mut ObjectManager, v: &Value) -> String {
    fn inner(tenant: &mut ObjectManager, v: &Value, depth: usize) -> String {
        if depth > 16 {
            return "\"[deep]\"".to_string();
        }
        match v {
            Value::Undefined => "\"[undefined]\"".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => {
                if n.is_finite() {
                    // serde_json prints f64 the way JSON.stringify does for the values
                    // test262 completes with (integers print without a fraction).
                    serde_json::to_string(n).unwrap_or_else(|_| "null".to_string())
                } else {
                    "null".to_string()
                }
            }
            Value::Str(s) => serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()),
            Value::Object(_) => {
                if matches!(
                    tenant.typeof_tag(v),
                    portal_solutions_jade_tenant_rt::ValueTag::Function
                ) {
                    return "\"[function anonymous]\"".to_string();
                }
                let mut entries = Vec::new();
                match tenant.own_keys(v) {
                    Ok(keys) => {
                        for key in keys {
                            let name = match &key {
                                PropertyKey::String(s) => s.clone(),
                                PropertyKey::Symbol(_) => continue,
                            };
                            match tenant.get(v, &key) {
                                Ok(val) => entries.push(format!(
                                    "{}:{}",
                                    serde_json::to_string(&name)
                                        .unwrap_or_else(|_| "\"\"".to_string()),
                                    inner(tenant, &val, depth + 1)
                                )),
                                Err(_) => entries.push(format!(
                                    "{}:\"[unreadable]\"",
                                    serde_json::to_string(&name)
                                        .unwrap_or_else(|_| "\"\"".to_string())
                                )),
                            }
                        }
                    }
                    Err(_) => return "\"[object Object]\"".to_string(),
                }
                format!("{{{}}}", entries.join(","))
            }
        }
    }
    inner(tenant, v, 0)
}

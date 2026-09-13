//! The native-Rust interpreter: a `jade-vm-core` dispatch platform that runs Jade bytecode
//! directly against the [`ObjectManager`] tenant, with no JS engine anywhere in the loop.
//! This is the engine behind the test262 plan's native-Rust cell (row 7).
//!
//! Design mirrors `jade-vm-wasm`'s `WasmPlatform`, simplified by the fact that the native
//! value type *is* the tenant's own [`Value`]:
//!
//! - **State is a flat `Vec<Value>`** indexed by slot (growing on demand; out-of-range reads
//!   yield `undefined`, matching the JS state object's hole behavior). There is no cache
//!   layer, so `flush`/`flush_and_invalidate` are no-ops.
//! - **No function registry**: `op_call` is simply `tenant.invoke(...)`. The ObjectManager
//!   dispatches adopted guest functions (see [`GuestCallable`]) synchronously in-process —
//!   that *is* the jade-to-jade fast path the WASM side needs its `WeakMap` registry for.
//! - **`op_fn`** boxes a [`GuestClosure`] (bytecode offset, variant, param slot ids) and
//!   adopts it via `ObjectManager::adopt_guest_fn` with shadow property behavior, mirroring
//!   `makeFunction`. Applying it builds a fresh child state and binds call arguments into
//!   the param slots (the FN opcode's `params` operand), exactly like the WASM slow path.
//! - **Errors**: `op_get`/`op_set`/`op_litobj`/`define_properties` have infallible
//!   signatures in the generated `Ops` trait but call fallible tenant operations; tenant
//!   errors there are stashed in a pending slot that the driving loop checks after every
//!   `exec_op`. `op_call`/`op_fn` return `Result` directly.
//! - **Sync only**: `Await`/`Yield`/`Yieldstar` and applying an async/generator-variant
//!   guest function return [`NativeError::Unsupported`] — async and generator cells are
//!   test262 Phase 5 scope, and the native cell currently runs script-goal, sync tests.
//!
//! Comparison semantics follow the TS interpreter (not the WASM backend): strict equality
//! is full JS `===` (`Value`'s derived `PartialEq` matches it exactly: `NaN !== NaN`,
//! `+0 === -0`, reference identity for objects), and relational operators do the full JS
//! string-or-number comparison rather than the WASM backend's numbers-only approximation.

use std::cell::RefCell;
use std::cmp::Ordering;

use portal_solutions_jade_tenant_rt::object_manager::{GuestCallable, ObjectManager, Value};
use portal_solutions_jade_tenant_rt::{PropertyKey, Tenant, TenantError, TenantInvocation};
use portal_solutions_jade_vm::Operation;
use portal_solutions_jade_vm_core as jade_vm_core;

/// The native interpreter's error type. `Ops::Error` is set to this so `exec_op`'s own
/// invariant failure (`P::err`) and `op_call`/`op_fn`'s tenant errors share one channel.
#[derive(Debug)]
pub enum NativeError {
    /// A tenant operation failed (including guest-thrown errors, which surface from
    /// `Tenant::invoke` as `TenantError`s).
    Tenant(TenantError),
    /// A bytecode feature the native sync cell does not drive yet (await/yield, or applying
    /// an async/generator-variant guest function).
    Unsupported(&'static str),
    /// `Operation::parse` hit the end of the bytecode mid-instruction — a corrupt or
    /// truncated artifact, never a guest-visible condition.
    Truncated,
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NativeError::Tenant(e) => write!(f, "{e}"),
            NativeError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            NativeError::Truncated => write!(f, "truncated bytecode"),
        }
    }
}

impl std::error::Error for NativeError {}

impl From<TenantError> for NativeError {
    fn from(value: TenantError) -> Self {
        NativeError::Tenant(value)
    }
}

/// Grows `state` to cover `slot` and writes `value`. Out-of-range reads yield `undefined`,
/// matching the JS state object.
fn set_slot(state: &mut Vec<Value>, slot: u32, value: Value) {
    let idx = slot as usize;
    if idx >= state.len() {
        state.resize(idx + 1, Value::Undefined);
    }
    state[idx] = value;
}

/// JS truthiness (`ToBoolean`) for the native value type — same rules as
/// `Tenant::to_boolean`, as a free function so the driving loop can use it without borrowing
/// the tenant.
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        Value::Undefined | Value::Null => false,
        Value::Object(_) => true,
    }
}

/// `ToNumber` for a native primitive (mirrors `ObjectManager::to_number`; objects convert
/// through their ordinary `ToPrimitive` first, handled by [`js_rel`]).
fn js_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Str(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse().unwrap_or(f64::NAN)
            }
        }
        Value::Null => 0.0,
        Value::Undefined | Value::Object(_) => f64::NAN,
    }
}

/// The abstract relational comparison's operand classification: both operands stringify →
/// string comparison, otherwise numeric comparison. `ToPrimitive` on objects is the ordinary
/// host default (`"[object Object]"`) — no `valueOf`/`toString`/`Symbol.toPrimitive`
/// primordial exists on the native side yet, same rule as `js_to_string` in the tenant.
enum Rel {
    Str(Ordering),
    Num(f64, f64),
}

fn js_rel(a: &Value, b: &Value) -> Rel {
    let ordinary = |v: &Value| match v {
        Value::Object(_) => Value::Str("[object Object]".to_string()),
        other => other.clone(),
    };
    let (pa, pb) = (ordinary(a), ordinary(b));
    if let (Value::Str(x), Value::Str(y)) = (&pa, &pb) {
        // Rust's `str` ordering is byte-wise, which coincides with JS's UTF-16 code-unit
        // ordering across the BMP (UTF-8 preserves code-point order). Astral-plane strings
        // reorder differently under the two schemes — a known, recorded edge.
        return Rel::Str(x.cmp(y));
    }
    Rel::Num(js_number(&pa), js_number(&pb))
}

/// `SWITCH` case selection: each `u32` case literal compares against the resolved value with
/// `===` (so only an exact numeric value matches; the WASM backend's truncate-to-`u32` tag
/// comparison is a known divergence this side deliberately does not copy).
fn switch_target(val: &Value, cases: &[(u32, u32)], default_target: u32) -> u32 {
    let num = match val {
        Value::Number(n) => *n,
        _ => return default_target,
    };
    cases
        .iter()
        .find(|(cv, _)| num == *cv as f64)
        .map(|(_, tgt)| *tgt)
        .unwrap_or(default_target)
}

/// A guest function created by the FN opcode: owns the bytecode region it jumps into plus
/// the param slot ids the FN operand advertises. Applying it runs the callee synchronously
/// on a fresh child state (closure capture remains unimplemented — the frontend rejects
/// captures before this ever matters).
struct GuestClosure {
    code: Vec<u8>,
    j: u32,
    effective_variant: u32,
    param_slots: Vec<u32>,
    global_this: Value,
    add_async: bool,
    add_gen: bool,
}

impl GuestCallable for GuestClosure {
    fn apply(&mut self, tenant: &mut ObjectManager, this_arg: &Value, args: &[Value]) -> Result<Value, TenantError> {
        if self.effective_variant != 0 {
            return Err(TenantError::type_error(
                "native sync cell cannot drive async/generator guest functions yet",
            ));
        }
        // `this_arg` is deliberately not threaded: no opcode lets a guest body observe
        // `this` (GLOBAL/NEW_TARGET cover the reachable surface), so dropping it here is
        // unobservable today — same convention as the WASM fast path.
        let _ = this_arg;
        let mut child: Vec<Value> = Vec::new();
        for (i, &slot) in self.param_slots.iter().enumerate() {
            let value = args.get(i).cloned().unwrap_or(Value::Undefined);
            set_slot(&mut child, slot, value);
        }
        run_sync(
            &self.code,
            &mut child,
            self.j as usize,
            &self.global_this,
            &Value::Undefined,
            tenant,
            self.add_async,
            self.add_gen,
        )
        .map_err(|e| match e {
            NativeError::Tenant(t) => t,
            other => TenantError::type_error(other.to_string()),
        })
    }
}

/// The dispatch platform. `tenant` lives behind a `RefCell` because the generated `Ops`
/// trait takes `&self` on most tenant-touching handlers (`op_get`/`op_set`/`op_litobj`/
/// `define_properties`) while `Tenant` operations need `&mut ObjectManager`; reentrant
/// tenant use (a guest-function apply starting a nested interpreter) builds a *fresh*
/// platform around the `&mut ObjectManager` the trait hands the handler, so no runtime
/// borrow conflict arises.
struct NativePlatform<'a> {
    state: &'a mut Vec<Value>,
    global_this: &'a Value,
    nt: &'a Value,
    tenant: RefCell<&'a mut ObjectManager>,
    add_async: bool,
    add_gen: bool,
    /// Tenant error stashed by an infallible-signature op; the driving loop drains it after
    /// every `exec_op`.
    pending: RefCell<Option<TenantError>>,
}

impl NativePlatform<'_> {
    /// Runs `f` against the tenant, stashing any error in `pending` and returning
    /// `on_err` — the infallible-`Ops`-signature error channel.
    fn infallible_tenant<R>(&self, on_err: R, f: impl FnOnce(&mut ObjectManager) -> Result<R, TenantError>) -> R {
        match f(&mut self.tenant.borrow_mut()) {
            Ok(value) => value,
            Err(error) => {
                *self.pending.borrow_mut() = Some(error);
                on_err
            }
        }
    }
}

impl jade_vm_core::State for NativePlatform<'_> {
    type Value = Value;

    fn get(&mut self, idx: u32) -> Value {
        self.state.get(idx as usize).cloned().unwrap_or(Value::Undefined)
    }

    fn set(&mut self, idx: u32, val: Value) {
        set_slot(self.state, idx, val);
    }

    fn flush(&mut self) {}

    fn flush_and_invalidate(&mut self) {}

    fn state_ref(&self) -> Value {
        // Closure capture is unimplemented, so the parent-state reference the FN opcode
        // would capture is never read; `undefined` is the honest placeholder.
        Value::Undefined
    }
}

impl jade_vm_core::Ops for NativePlatform<'_> {
    type Value = Value;
    type Error = NativeError;

    fn f64_val(&self, v: f64) -> Value {
        Value::Number(v)
    }

    fn str_val(&self, s: &str) -> Value {
        Value::Str(s.to_string())
    }

    fn undefined(&self) -> Value {
        Value::Undefined
    }

    fn err(msg: &'static str) -> NativeError {
        NativeError::Unsupported(msg)
    }

    fn define_properties(&self, target: &Value, props: Value) {
        self.infallible_tenant((), |tenant| tenant.define(target, &props));
    }

    fn op_global(&self) -> Value {
        self.global_this.clone()
    }

    fn op_fn(
        &mut self,
        code: &[u8],
        variant: Value,
        closure_args: Value,
        spanner: Value,
        params: Value,
        j: u32,
        _parent_state: Value,
    ) -> Result<Value, NativeError> {
        let declared_variant = match &variant {
            Value::Number(n) => (*n as u32) & 3,
            _ => 0,
        };
        let effective_variant = declared_variant | (self.add_async as u32) | ((self.add_gen as u32) << 1);

        // `closure_args`/`params` are `Operand::Literal(0)` when absent — falsy means an
        // empty slot list, anything else is an array(-like) of slot ids built by the
        // frontend's LIT32+ARR sequence at the FN site.
        let slot_list = |tenant: &mut ObjectManager, v: &Value| -> Result<Vec<u32>, TenantError> {
            if !js_truthy(v) {
                return Ok(Vec::new());
            }
            let length = match tenant.get(v, &"length".into())? {
                Value::Number(n) => n as u32,
                _ => 0,
            };
            let mut out = Vec::with_capacity(length as usize);
            for i in 0..length {
                match tenant.get(v, &PropertyKey::from(i.to_string()))? {
                    Value::Number(n) => out.push(n as u32),
                    _ => out.push(0),
                }
            }
            Ok(out)
        };
        let param_slots = slot_list(self.tenant.get_mut(), &params)?;
        // Closure capture is unimplemented (the frontend rejects captures, and the closure
        // plan documents `spanner` as a not-yet-wired decorator hook), so both are decoded
        // for future use and otherwise ignored.
        let _closure_slots = slot_list(self.tenant.get_mut(), &closure_args)?;
        let _ = spanner;

        let closure = GuestClosure {
            code: code.to_vec(),
            j,
            effective_variant,
            param_slots,
            global_this: self.global_this.clone(),
            add_async: self.add_async,
            add_gen: self.add_gen,
        };
        Ok(self.tenant.get_mut().adopt_guest_fn(None, Box::new(closure)))
    }

    fn op_lit32(&self, val: u32) -> Value {
        Value::Number(val as f64)
    }

    fn op_arr(&self, items: Vec<Value>) -> Value {
        // The TS interpreter builds a raw host array here; the native side backs it with
        // `indexed_collection` (a real tenant object with indexed properties and
        // `length`) — a recorded divergence: no `Array.prototype` methods exist natively.
        self.infallible_tenant(Value::Undefined, |tenant| tenant.indexed_collection(items))
    }

    fn op_str(&self, cps: Vec<Value>) -> Value {
        let s: String = cps
            .into_iter()
            .filter_map(|v| match v {
                Value::Number(n) => char::from_u32(n as u32),
                _ => None,
            })
            .collect();
        Value::Str(s)
    }

    fn op_litobj(&self, spread: Option<Value>, pairs: Vec<(Value, Value)>) -> Value {
        self.infallible_tenant(Value::Undefined, |tenant| {
            let obj = tenant.make(None)?;
            if let Some(src) = spread {
                tenant.assign(&obj, &src)?;
            }
            for (k, v) in pairs {
                let key = tenant.to_property_key(&k);
                tenant.set(&obj, &key, v)?;
            }
            Ok(obj)
        })
    }

    fn op_new_target(&self) -> Value {
        self.nt.clone()
    }

    fn op_call(&mut self, _code: &[u8], fn_val: Value, this_val: Value, args: Vec<Value>) -> Result<Value, NativeError> {
        // The ObjectManager's invoke dispatch *is* the jade-to-jade fast path: adopted
        // guest functions run synchronously in-process via GuestCallable::apply, with no
        // registry (the WASM side needs a WeakMap registry because its functions live
        // across the JS boundary).
        Ok(self.tenant.get_mut().invoke(&fn_val, TenantInvocation::Apply {
            this_arg: this_val,
            args,
        })?)
    }

    fn op_bool(&self, val: bool) -> Value {
        Value::Bool(val)
    }

    fn op_eq(&self, a: Value, b: Value) -> Value {
        // `Value`'s derived PartialEq matches `===` exactly: NaN ≠ NaN, +0 === -0, object
        // identity by arena index, cross-type always false.
        Value::Bool(a == b)
    }

    fn op_ne(&self, a: Value, b: Value) -> Value {
        Value::Bool(a != b)
    }

    fn op_lt(&self, a: Value, b: Value) -> Value {
        Value::Bool(match js_rel(&a, &b) {
            Rel::Str(ord) => ord == Ordering::Less,
            Rel::Num(x, y) => x < y,
        })
    }

    fn op_le(&self, a: Value, b: Value) -> Value {
        Value::Bool(match js_rel(&a, &b) {
            Rel::Str(ord) => ord != Ordering::Greater,
            Rel::Num(x, y) => x <= y,
        })
    }

    fn op_gt(&self, a: Value, b: Value) -> Value {
        Value::Bool(match js_rel(&a, &b) {
            Rel::Str(ord) => ord == Ordering::Greater,
            Rel::Num(x, y) => x > y,
        })
    }

    fn op_ge(&self, a: Value, b: Value) -> Value {
        Value::Bool(match js_rel(&a, &b) {
            Rel::Str(ord) => ord != Ordering::Less,
            Rel::Num(x, y) => x >= y,
        })
    }

    fn op_sel(&self, cond: Value, then: Value, else_: Value) -> Value {
        if js_truthy(&cond) {
            then
        } else {
            else_
        }
    }

    fn op_get(&self, obj: Value, key: Value) -> Value {
        self.infallible_tenant(Value::Undefined, |tenant| {
            let key = tenant.to_property_key(&key);
            tenant.get(&obj, &key)
        })
    }

    fn op_set(&self, obj: Value, key: Value, val: Value) -> Value {
        let result = self.infallible_tenant((), |tenant| {
            let key = tenant.to_property_key(&key);
            tenant.set(&obj, &key, val.clone())
        });
        let _ = result;
        // SET writes the assigned value itself to dest (TS parity).
        val
    }
}

/// The synchronous driving loop — mirrors `jade-vm-wasm`'s `run_sync`: loop-level opcodes
/// (`Ret`/jumps/`Switch`) are handled here, everything else goes to `exec_op`, and the
/// pending-error channel is drained after every dispatched op.
pub fn run_sync(
    code: &[u8],
    state: &mut Vec<Value>,
    start_ip: usize,
    global_this: &Value,
    nt: &Value,
    tenant: &mut ObjectManager,
    add_async: bool,
    add_gen: bool,
) -> Result<Value, NativeError> {
    let mut ip = start_ip;
    let mut platform = NativePlatform {
        state,
        global_this,
        nt,
        tenant: RefCell::new(tenant),
        add_async,
        add_gen,
        pending: RefCell::new(None),
    };
    loop {
        let (op, rest) = Operation::parse(&code[ip..]).ok_or(NativeError::Truncated)?;
        ip = code.len() - rest.len();
        match op {
            Operation::Ret(val_op) => {
                return Ok(jade_vm_core::resolve(val_op, &mut platform));
            }
            Operation::Await { .. } | Operation::Yield { .. } | Operation::Yieldstar { .. } => {
                return Err(NativeError::Unsupported(
                    "await/yield require the async/generator driver (native sync cell)",
                ));
            }
            Operation::Jmp { target } => {
                ip = target as usize;
            }
            Operation::CondJmp {
                cond,
                if_true,
                if_false,
            } => {
                let cond_val = jade_vm_core::resolve(cond, &mut platform);
                ip = if js_truthy(&cond_val) {
                    if_true as usize
                } else {
                    if_false as usize
                };
            }
            Operation::Switch {
                val,
                cases,
                default_target,
            } => {
                let val_v = jade_vm_core::resolve(val, &mut platform);
                ip = switch_target(&val_v, &cases, default_target) as usize;
            }
            _ => {
                jade_vm_core::exec_op(op, code, &mut platform)?;
                if let Some(error) = platform.pending.borrow_mut().take() {
                    return Err(NativeError::Tenant(error));
                }
            }
        }
    }
}

/// Top-level entry point for the native sync cell: fresh state, `ip = 0`, no `new.target`,
/// sync-only variant flags.
pub fn run(code: &[u8], global_this: &Value, tenant: &mut ObjectManager) -> Result<Value, NativeError> {
    let mut state = Vec::new();
    run_sync(code, &mut state, 0, global_this, &Value::Undefined, tenant, false, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compiles `src` with the real frontend and runs it natively against a bare
    /// ObjectManager whose `globalThis` is a fresh, empty object.
    fn run_script(src: &str) -> Result<Value, NativeError> {
        let code = portal_solutions_jade_vm_frontend::compile_to_bytecode(src).expect("compile");
        let mut tenant = ObjectManager::new();
        let global_this = tenant.make(None).expect("make globalThis");
        run(&code, &global_this, &mut tenant)
    }

    #[test]
    fn returns_a_literal() {
        assert_eq!(run_script("return 42;").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn falls_off_the_end_as_undefined() {
        assert_eq!(run_script("var x = 1;").unwrap(), Value::Undefined);
    }

    #[test]
    fn branches_and_compares_numbers() {
        assert_eq!(
            run_script("if (1 < 2) { return 'yes'; } return 'no';").unwrap(),
            Value::Str("yes".into())
        );
    }

    #[test]
    fn compares_strings_lexicographically() {
        // Full JS string relational semantics (the WASM backend's numbers-only comparison
        // would answer `false` here).
        assert_eq!(run_script("return 'b' > 'a' ? 1 : 0;").unwrap(), Value::Number(1.0));
        assert_eq!(run_script("return 'abc' <= 'abc' ? 1 : 0;").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn strict_equality_matches_js() {
        // Cross-type `===` is always false without coercion.
        assert_eq!(run_script("return '1' === 1 ? 1 : 0;").unwrap(), Value::Number(0.0));
        // NaN !== NaN: the derived-PartialEq `===` semantics are IEEE (unary minus isn't
        // lowered by the frontend yet, so NaN arrives via the global object instead of
        // a `-0` literal).
        let code = portal_solutions_jade_vm_frontend::compile_to_bytecode(
            "return nanInstalled === nanInstalled ? 1 : 0;",
        )
        .expect("compile");
        let mut tenant = ObjectManager::new();
        let global_this = tenant.make(None).expect("make globalThis");
        tenant
            .set(&global_this, &"nanInstalled".into(), Value::Number(f64::NAN))
            .unwrap();
        assert_eq!(run(&code, &global_this, &mut tenant).unwrap(), Value::Number(0.0));
    }

    #[test]
    fn runs_a_while_loop() {
        // `+` has no opcode yet, so loops count by flipping values (same pattern as the
        // test262 smoke fixtures).
        assert_eq!(
            run_script("var x = true; while (x) { x = false; } return x;").unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            run_script("var i = 0; var done = false; while (done === false) { i = 3; done = true; } return i;").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn calls_a_function_with_parameters() {
        assert_eq!(
            run_script("var f = function(x, y) { return x === y; }; return f(3, 3);").unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn missing_arguments_read_undefined_and_extras_are_dropped() {
        assert_eq!(
            run_script("var f = function(x) { return x === undefined; }; return f();").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            run_script("var g = function(x) { return x; }; return g(7, 8, 9);").unwrap(),
            Value::Number(7.0)
        );
    }

    #[test]
    fn object_get_set_roundtrip() {
        assert_eq!(
            run_script("var o = {}; o.x = 5; return o.x;").unwrap(),
            Value::Number(5.0)
        );
    }

    #[test]
    fn reads_through_the_global_object() {
        // Free-identifier reads resolve through globalThis (the frontend's global pass);
        // a property installed on the native global object is visible to the guest.
        let code = portal_solutions_jade_vm_frontend::compile_to_bytecode("return installed;").expect("compile");
        let mut tenant = ObjectManager::new();
        let global_this = tenant.make(None).expect("make globalThis");
        tenant
            .set(&global_this, &"installed".into(), Value::Str("from-native".into()))
            .unwrap();
        assert_eq!(
            run(&code, &global_this, &mut tenant).unwrap(),
            Value::Str("from-native".into())
        );
    }
}


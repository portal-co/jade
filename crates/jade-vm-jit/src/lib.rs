//! JS-emitting JIT backend for the Jade VM.
//!
//! This crate implements the [`State`] + [`Ops`] traits from
//! `portal_solutions_jade_vm_core` over a backend whose `Value` is a *JavaScript
//! variable / expression*. Driving the shared `exec_op` dispatch over a chunk of
//! bytecode therefore emits a textual JavaScript program that, when run in any JS
//! runtime (in-browser via a WASM host, or remotely), reproduces the bytecode's
//! behaviour.
//!
//! Design points (see `goals.md`):
//! - **`Value`s are variable IDs.** Each produced value is either a freshly
//!   minted `v{n}` JS variable (for computed ops) or an inline leaf expression
//!   (literals, `state[idx]` reads).
//! - **All branches are emitted.** `if_op` emits *both* arms; `switch_op` emits
//!   every case and the default — the branch closures are all invoked once so
//!   their code is captured.
//! - **`while` bodies are emitted once.** `while_op` invokes the body closure a
//!   single time, wrapping the emitted statements in a real JS `while`.
//! - **Function registration is delegated.** `op_fn` compiles the function body
//!   into its own source string and hands it to a user-supplied [`FnRegistry`];
//!   registered functions always take the implicit `tenant` and `nt` parameters
//!   first, i.e. `function(tenant, nt, ...args){ … }`. [`VecRegistry`] also emits a
//!   `markGuestFn(name, {abi: "leading-tenant-nt"})` call per function so tenant-side
//!   code (`invokeTrap` in `narrow.ts`) knows to invoke it with that ABI rather than as
//!   a plain host function — `markGuestFn` must be in scope at the use site, same as
//!   `createGuestGen`/`unpackGuestGen` are for the addGen path.
//!
//! State is modelled as a real JS object named `state` in the emitted code, so
//! mutations made inside emitted branches and loop iterations are observed by
//! later reads exactly as the interpreter would observe them.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;

use portal_solutions_jade_vm::Operation;
use portal_solutions_jade_vm_core::{self as core_vm, Ops, State, exec_op, resolve};

/// A JavaScript value handle: either a minted variable (`v{n}`) or an inline
/// leaf expression (a literal, `globalThis`, `nt`, or a `state[idx]` read).
#[derive(Clone)]
pub enum JsVar {
    /// A `v{n}` temporary previously bound with `const`.
    Var(u32),
    /// An inline JS expression spliced directly into use sites.
    Expr(String),
}

impl fmt::Display for JsVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsVar::Var(n) => write!(f, "v{n}"),
            JsVar::Expr(s) => f.write_str(s),
        }
    }
}

/// The four Jade function variants, matching the `FN` opcode's `variant`
/// operand (`0` sync, `1` async, `2` sync generator, `3` async generator).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FnVariant {
    Sync,
    Async,
    SyncGen,
    AsyncGen,
}

impl FnVariant {
    /// The JS declaration keyword for this variant
    /// (`function`, `async function`, `function*`, `async function*`).
    pub fn keyword(self) -> &'static str {
        match self {
            FnVariant::Sync => "function",
            FnVariant::Async => "async function",
            FnVariant::SyncGen => "function*",
            FnVariant::AsyncGen => "async function*",
        }
    }
}

/// One tenant method extracted from its own source text, ready to be spliced into
/// generated code as `(function(${params.join(",")})${body_block})(${args...})` instead
/// of a `tenant.<method>(...)` call. See `docs/pluggable-tenant-interface-plan.md`.
///
/// This is a plain data type — parsing tenant source (SWC, private-field scanning) lives
/// in `portal_solutions_jade_vm_frontend::tenant_inline` (this crate stays free of a full
/// parser dependency; it only ever splices already-extracted text). Construct these via
/// that crate's `extract_tenant_methods`, or directly for tests.
#[derive(Clone, Default)]
pub struct InlinableTenantMethod {
    /// Parameter names, in declaration order.
    pub params: Vec<String>,
    /// The exact original source text of the method's `{ ... }` body, braces included.
    pub body_block: String,
}

/// Splice `m`'s body into a call-once function expression bound to `args` — real JS
/// function-parameter binding, so this is correct regardless of what identifiers `args`
/// happen to contain (no risk of capturing/shadowing anything in the surrounding scope).
fn inline_call(m: &InlinableTenantMethod, args: &[&str]) -> String {
    format!("(function({}){})({})", m.params.join(", "), m.body_block, args.join(", "))
}

/// Ambient capability flags for the JIT.  Propagated into every nested function
/// compiled in the same session so the whole program upgrades uniformly.
#[derive(Clone, Default)]
pub struct Config {
    /// Add async capability on top of every function's declared variant.
    pub add_async: bool,
    /// Add generator capability on top of every function's declared variant.
    pub add_gen: bool,
    /// Tenant methods (keyed by name — `"get"`, `"set"`, etc; see
    /// `portal_solutions_jade_vm_frontend::tenant_inline::TENANT_METHOD_NAMES`) safe to
    /// inline directly instead of calling through `tenant.<method>(...)`. Empty by
    /// default, which reproduces the JIT's original (always-correct, never-inlined)
    /// behavior exactly.
    pub tenant_methods: alloc::collections::BTreeMap<String, InlinableTenantMethod>,
}

/// Decode a resolved `variant` value (a `JsVar`) into an [`FnVariant`], then
/// apply the ambient `Config` flags to compute the effective variant.
fn fn_variant(v: &JsVar, cfg: &Config) -> FnVariant {
    let declared = if let JsVar::Expr(s) = v {
        match s.trim() {
            "1" => FnVariant::Async,
            "2" => FnVariant::SyncGen,
            "3" => FnVariant::AsyncGen,
            _ => FnVariant::Sync,
        }
    } else {
        FnVariant::Sync
    };
    // OR the declared bits with the ambient flag bits.
    let declared_idx = match declared {
        FnVariant::Sync => 0u32,
        FnVariant::Async => 1,
        FnVariant::SyncGen => 2,
        FnVariant::AsyncGen => 3,
    };
    let effective_idx = declared_idx
        | (cfg.add_async as u32)
        | ((cfg.add_gen as u32) << 1);
    match effective_idx {
        1 => FnVariant::Async,
        2 => FnVariant::SyncGen,
        3 => FnVariant::AsyncGen,
        _ => FnVariant::Sync,
    }
}

/// Sink for functions produced by the `FN` opcode.
///
/// `op_fn` compiles a function body to a statement string and calls
/// [`register`](FnRegistry::register) with the function `variant` and parameter
/// list (always leading with `tenant` and `nt`). The returned string is a JS
/// *reference expression* (e.g. a name like `__fn0`) that the JIT emits where
/// the function value is needed.
pub trait FnRegistry {
    /// Register a function of the given `variant` whose parameters are `params`
    /// and whose body is the statement block `body`. Return a JS expression that
    /// evaluates to the function (typically the name it was bound to).
    fn register(&mut self, variant: FnVariant, params: &[&str], body: &str) -> String;
}

/// A simple [`FnRegistry`] that accumulates `const __fn{n} = function(...){...}`
/// declarations and hands back the `__fn{n}` names. Read [`prelude`] afterwards
/// to obtain the declarations to prepend to the emitted program.
#[derive(Default)]
pub struct VecRegistry {
    decls: Vec<String>,
}

impl VecRegistry {
    pub fn new() -> Self {
        Self { decls: Vec::new() }
    }
    /// The accumulated `const __fnN = …;` declarations, newline-joined.
    pub fn prelude(&self) -> String {
        self.decls.join("\n")
    }
    /// The raw declaration list.
    pub fn decls(&self) -> &[String] {
        &self.decls
    }
}

impl FnRegistry for VecRegistry {
    fn register(&mut self, variant: FnVariant, params: &[&str], body: &str) -> String {
        let name = format!("__fn{}", self.decls.len());
        // Every function this JIT compiles uses the "leading-tenant-nt" guest ABI (its
        // `params` always lead with `tenant`, `nt`); register that with `markGuestFn` so
        // any embedder-side code (e.g. a tenant's getter/setter trap invocation via
        // `invokeTrap`) calls it with the correct parameters instead of a plain `.call()`.
        // `markGuestFn` must be in scope at the use site (imported from `narrow.ts`), same
        // as `createGuestGen`/`unpackGuestGen` are for the addGen path.
        self.decls.push(format!(
            "const {name} = {}({}){{\n{body}\n}};\nmarkGuestFn({name}, {{abi: \"leading-tenant-nt\"}});",
            variant.keyword(),
            params.join(", ")
        ));
        name
    }
}

/// Mutable emission state: the statement buffer and the variable counter.
struct Emit {
    buf: String,
    next: u32,
}

impl Emit {
    fn new() -> Self {
        Self { buf: String::new(), next: 0 }
    }
}

/// The JIT backend. Drives `exec_op` and accumulates JS statements.
pub struct JsJit<'a, R: FnRegistry> {
    reg: &'a RefCell<R>,
    emit: RefCell<Emit>,
    cfg: Config,
    /// Whether the function currently being compiled is effectively a generator.
    is_gen: bool,
    /// Whether the function currently being compiled is effectively async.
    is_async: bool,
    /// doubleGen: declared gen + add_gen → YIELD values are THROUGH-tagged.
    double_gen: bool,
}

impl<'a, R: FnRegistry> JsJit<'a, R> {
    fn new(reg: &'a RefCell<R>) -> Self {
        Self::with_config(reg, Config::default(), false, false, false)
    }

    fn with_config(
        reg: &'a RefCell<R>,
        cfg: Config,
        is_gen: bool,
        is_async: bool,
        double_gen: bool,
    ) -> Self {
        Self { reg, emit: RefCell::new(Emit::new()), cfg, is_gen, is_async, double_gen }
    }

    /// Mint a fresh `v{n}` variable id.
    fn fresh(&self) -> u32 {
        let mut e = self.emit.borrow_mut();
        let n = e.next;
        e.next += 1;
        n
    }

    /// Append a statement line.
    fn line(&self, s: impl AsRef<str>) {
        let mut e = self.emit.borrow_mut();
        e.buf.push_str(s.as_ref());
        e.buf.push('\n');
    }

    /// Emit `const v{n} = {expr};` and return the new variable.
    fn bind(&self, expr: impl fmt::Display) -> JsVar {
        let n = self.fresh();
        self.line(format!("const v{n} = {expr};"));
        JsVar::Var(n)
    }
}

/// JS string literal escaping (the subset needed for emitted source).
fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl<'a, R: FnRegistry> State for JsJit<'a, R> {
    type Value = JsVar;

    fn get(&mut self, idx: u32) -> JsVar {
        JsVar::Expr(format!("state[{idx}]"))
    }
    fn set(&mut self, idx: u32, val: JsVar) {
        self.line(format!("state[{idx}] = {val};"));
    }
    // No emission-side cache: the emitted code reads/writes `state` directly.
    fn flush(&mut self) {}
    fn flush_and_invalidate(&mut self) {}
    fn state_ref(&self) -> JsVar {
        JsVar::Expr("state".to_string())
    }
}

impl<'a, R: FnRegistry> Ops for JsJit<'a, R> {
    type Value = JsVar;
    type Error = String;

    fn f64_val(&self, v: f64) -> JsVar {
        // Render integers without a trailing `.0` for readable, valid JS.
        if v.fract() == 0.0 && v.is_finite() {
            JsVar::Expr(format!("{}", v as i64))
        } else {
            JsVar::Expr(format!("{v}"))
        }
    }
    fn str_val(&self, s: &str) -> JsVar {
        JsVar::Expr(js_string(s))
    }
    fn undefined(&self) -> JsVar {
        JsVar::Expr("undefined".to_string())
    }
    fn err(msg: &'static str) -> String {
        msg.to_string()
    }

    fn define_properties(&self, target: &JsVar, props: JsVar) {
        self.line(format!("tenant.define({target}, {props});"));
    }

    fn op_global(&self) -> JsVar {
        JsVar::Expr("globalThis".to_string())
    }

    fn op_fn(
        &mut self,
        code: &[u8],
        variant: JsVar,
        _closure_args: JsVar,
        _spanner: JsVar,
        j: u32,
        _parent_state: JsVar,
    ) -> Result<JsVar, String> {
        let eff = fn_variant(&variant, &self.cfg);
        // doubleGen: declared gen | add_gen → declared gen bit was set already
        let declared_is_gen = if let JsVar::Expr(s) = &variant {
            matches!(s.trim(), "2" | "3")
        } else {
            false
        };
        let child_double_gen = self.cfg.add_gen && declared_is_gen;
        let child_is_gen = matches!(eff, FnVariant::SyncGen | FnVariant::AsyncGen);
        let child_is_async = matches!(eff, FnVariant::Async | FnVariant::AsyncGen);

        // Compile the function body into its own statement block with the
        // context flags for the child (effective variant's gen/async/doubleGen).
        let body = {
            let mut nested = JsJit::with_config(
                self.reg, self.cfg.clone(), child_is_gen, child_is_async, child_double_gen,
            );
            emit_program(&mut nested, code, j as usize)?;
            format!("const state = Object.create(null);\n{}", nested.emit.into_inner().buf)
        };
        // NOTE: closure-slot capture and decorator (`spanner`) application are
        // not yet wired in this first backend.
        let reference = self.reg.borrow_mut().register(eff, &["tenant", "nt", "...args"], &body);
        Ok(self.bind(reference))
    }

    fn op_lit32(&self, val: u32) -> JsVar {
        JsVar::Expr(format!("{val}"))
    }

    fn op_arr(&self, items: Vec<JsVar>) -> JsVar {
        let parts: Vec<String> = items.iter().map(|v| v.to_string()).collect();
        self.bind(format!("[{}]", parts.join(", ")))
    }

    fn op_str(&self, items: Vec<JsVar>) -> JsVar {
        let parts: Vec<String> = items.iter().map(|v| v.to_string()).collect();
        self.bind(format!("String.fromCodePoint({})", parts.join(", ")))
    }

    fn op_litobj(&self, spread: Option<JsVar>, pairs: Vec<(JsVar, JsVar)>) -> JsVar {
        // Build the object through the tenant object manager.
        let n = self.fresh();
        self.line(format!("const v{n} = tenant.make(null);"));
        if let Some(s) = spread {
            self.line(format!("tenant.assign(v{n}, {s});"));
        }
        for (k, v) in pairs {
            self.line(format!("tenant.set(v{n}, {k}, {v});"));
        }
        JsVar::Var(n)
    }

    fn op_new_target(&self) -> JsVar {
        JsVar::Expr("nt".to_string())
    }

    fn op_call(&mut self, _code: &[u8], fn_val: JsVar, args: Vec<JsVar>) -> Result<JsVar, String> {
        // Jade functions are registered as `function(tenant, nt, ...args)`, so a
        // call threads the enclosing `tenant` and `nt` ahead of the user args.
        let mut parts = Vec::with_capacity(args.len() + 2);
        parts.push("tenant".to_string());
        parts.push("nt".to_string());
        parts.extend(args.iter().map(|v| v.to_string()));
        let raw_call = format!("Reflect.apply({fn_val}, undefined, [{}])", parts.join(", "));
        if self.cfg.add_gen {
            // In addGen mode every Jade callee runs as a generator; wrap the
            // result in a guest-side generator object via the shims helper.
            // `createGuestGen` must be in scope at the call site (imported from shims).
            let raw = self.bind(raw_call);
            Ok(self.bind(format!(
                "({raw} && typeof {raw}.next === 'function') ? createGuestGen({raw}, tenant) : {raw}"
            )))
        } else {
            Ok(self.bind(raw_call))
        }
    }

    fn op_bool(&self, val: bool) -> JsVar {
        JsVar::Expr(if val { "true" } else { "false" }.to_string())
    }

    fn op_eq(&self, a: JsVar, b: JsVar) -> JsVar {
        self.bind(format!("({a} === {b})"))
    }
    fn op_ne(&self, a: JsVar, b: JsVar) -> JsVar {
        self.bind(format!("({a} !== {b})"))
    }
    fn op_lt(&self, a: JsVar, b: JsVar) -> JsVar {
        self.bind(format!("({a} < {b})"))
    }
    fn op_le(&self, a: JsVar, b: JsVar) -> JsVar {
        self.bind(format!("({a} <= {b})"))
    }
    fn op_gt(&self, a: JsVar, b: JsVar) -> JsVar {
        self.bind(format!("({a} > {b})"))
    }
    fn op_ge(&self, a: JsVar, b: JsVar) -> JsVar {
        self.bind(format!("({a} >= {b})"))
    }
    fn op_sel(&self, cond: JsVar, then: JsVar, else_: JsVar) -> JsVar {
        self.bind(format!("({cond} ? {then} : {else_})"))
    }

    fn op_get(&self, obj: JsVar, key: JsVar) -> JsVar {
        match self.cfg.tenant_methods.get("get") {
            Some(m) if m.params.len() == 2 => self.bind(inline_call(m, &[&obj.to_string(), &key.to_string()])),
            _ => self.bind(format!("tenant.get({obj}, {key})")),
        }
    }

    fn op_set(&self, obj: JsVar, key: JsVar, val: JsVar) -> JsVar {
        // Write through the tenant; the assignment evaluates to the value.
        match self.cfg.tenant_methods.get("set") {
            Some(m) if m.params.len() == 3 => {
                self.line(format!("{};", inline_call(m, &[&obj.to_string(), &key.to_string(), &val.to_string()])));
            }
            _ => self.line(format!("tenant.set({obj}, {key}, {val});")),
        }
        val
    }

    fn while_op<Ctx, F>(&mut self, ctx: &mut Ctx, init: JsVar, mut body: F) -> Result<JsVar, String>
    where
        F: FnMut(&mut Self, &mut Ctx) -> Result<JsVar, String>,
    {
        // Bind the loop's condition variable, then emit a real JS `while` whose
        // body is emitted exactly once.
        let n = self.fresh();
        self.line(format!("let v{n} = {init};"));
        self.line(format!("while (v{n}) {{"));
        let next = body(self, ctx)?;
        self.line(format!("v{n} = {next};"));
        self.line("}");
        Ok(JsVar::Var(n))
    }

    fn if_op<Ctx, FT, FE>(
        &mut self,
        ctx: &mut Ctx,
        cond: JsVar,
        then_body: FT,
        else_body: FE,
    ) -> Result<JsVar, String>
    where
        FT: FnOnce(&mut Self, &mut Ctx) -> Result<JsVar, String>,
        FE: FnOnce(&mut Self, &mut Ctx) -> Result<JsVar, String>,
    {
        // Both arms are always emitted.
        self.line(format!("if ({cond}) {{"));
        then_body(self, ctx)?;
        self.line("} else {");
        else_body(self, ctx)?;
        self.line("}");
        Ok(self.undefined())
    }

    fn switch_op<Ctx, F, D>(
        &mut self,
        ctx: &mut Ctx,
        val: JsVar,
        cases: impl IntoIterator<Item = (u32, F)>,
        default_body: D,
    ) -> Result<JsVar, String>
    where
        F: FnOnce(&mut Self, &mut Ctx) -> Result<JsVar, String>,
        D: FnOnce(&mut Self, &mut Ctx) -> Result<JsVar, String>,
    {
        // Every case and the default are always emitted.
        self.line(format!("switch ({val}) {{"));
        for (cv, branch) in cases {
            self.line(format!("case {cv}: {{"));
            branch(self, ctx)?;
            self.line("break; }");
        }
        self.line("default: {");
        default_body(self, ctx)?;
        self.line("}");
        self.line("}");
        Ok(self.undefined())
    }
}

/// Drive `exec_op` over the bytecode at `start_ip`, emitting statements into
/// `jit` until a `RET` (which becomes a JS `return`).
fn emit_program<R: FnRegistry>(
    jit: &mut JsJit<'_, R>,
    code: &[u8],
    start_ip: usize,
) -> Result<(), String> {
    let mut ip = start_ip;
    loop {
        let (op, rest) = Operation::parse(&code[ip..]).ok_or("jit: unexpected end of bytecode")?;
        ip = code.len() - rest.len();
        match op {
            Operation::Ret(val_op) => {
                let val = resolve(val_op, jit);
                jit.line(format!("return {val};"));
                return Ok(());
            }
            Operation::Await { val: val_op, dest } => {
                if !jit.is_async {
                    return Err("jit: AWAIT in non-async function".to_string());
                }
                let val = resolve(val_op, jit);
                jit.line(format!("state[{dest}] = await {val};"));
            }
            Operation::Yield { val: val_op, dest } => {
                if !jit.is_gen {
                    return Err("jit: YIELD in non-generator function".to_string());
                }
                let val = resolve(val_op, jit);
                if jit.double_gen {
                    // doubleGen: tag native yields as pass-throughs.
                    jit.line(format!(
                        "state[{dest}] = yield {{value: {val}, [Symbol.for(\"jade.through\")]: true}};"
                    ));
                } else {
                    jit.line(format!("state[{dest}] = yield {val};"));
                }
            }
            Operation::Yieldstar { val: val_op, dest } => {
                if !jit.is_gen {
                    return Err("jit: YIELDSTAR in non-generator function".to_string());
                }
                let val = resolve(val_op, jit);
                if jit.double_gen {
                    // doubleGen: use the unpack shim to handle guest-gen protocol.
                    // `unpackGuestGen` must be in scope (imported from shims).
                    jit.line(format!("state[{dest}] = yield* unpackGuestGen({val});"));
                } else {
                    jit.line(format!("state[{dest}] = yield* {val};"));
                }
            }
            _ => exec_op(op, code, jit, &mut ())?,
        }
    }
}

/// Compile a Jade bytecode chunk into a JavaScript statement block.
///
/// The emitted block reads and writes a `state` object and `tenant`/`nt` that
/// must be in scope at the use site; it ends with a `return`. Functions created
/// by the `FN` opcode are registered via `reg`. Returns the emitted statements
/// and the (now-populated) registry.
///
/// Pass a non-default [`Config`] to enable ambient `add_async`/`add_gen` flags.
/// When `add_gen` is active the emitted calls to `createGuestGen` and
/// `unpackGuestGen` (from `jade-js/shims.ts`) must be in scope at runtime.
pub fn compile<R: FnRegistry>(code: &[u8], reg: R, cfg: Config) -> Result<(String, R), String> {
    let cell = RefCell::new(reg);
    let body = {
        let mut jit = JsJit::with_config(&cell, cfg, false, false, false);
        emit_program(&mut jit, code, 0)?;
        jit.emit.into_inner().buf
    };
    Ok((body, cell.into_inner()))
}

// Re-export the driven traits so embedders can name them without depending on
// jade-vm-core directly.
pub use core_vm::{Ops as JitOps, State as JitState};

#[cfg(test)]
mod tests {
    use super::*;
    use portal_solutions_jade_vm::Operand;

    /// Concatenate the emitted bytes of a sequence of ops into one chunk.
    fn chunk(ops: &[Operation]) -> Vec<u8> {
        ops.iter().flat_map(|o| o.emit()).collect()
    }

    #[test]
    fn emits_binop_and_return() {
        // state[2] = (state[0] === state[1]); return state[2];
        let code = chunk(&[
            Operation::Eq { a: Operand::StateRef(0), b: Operand::StateRef(1), dest: 2 },
            Operation::Ret(Operand::StateRef(2)),
        ]);
        let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        assert!(js.contains("state[0] === state[1]"), "got:\n{js}");
        assert!(js.contains("state[2] ="), "got:\n{js}");
        assert!(js.contains("return state[2];"), "got:\n{js}");
    }

    #[test]
    fn emits_if_both_branches() {
        let then_body = chunk(&[Operation::Lit32 { dest: 1, val: 10 }]);
        let else_body = chunk(&[Operation::Lit32 { dest: 1, val: 20 }]);
        let code = chunk(&[
            Operation::If { cond: Operand::StateRef(0), then_body, else_body },
            Operation::Ret(Operand::StateRef(1)),
        ]);
        let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        assert!(js.contains("if (state[0]) {"), "got:\n{js}");
        assert!(js.contains("} else {"), "got:\n{js}");
        // Both arms emitted.
        assert!(js.contains("state[1] = 10;"), "got:\n{js}");
        assert!(js.contains("state[1] = 20;"), "got:\n{js}");
    }

    #[test]
    fn emits_while_loop_once() {
        // while (state[0]) { state[0] = (state[0] - is faked via Lit32) }
        let body = chunk(&[Operation::Lit32 { dest: 0, val: 0 }]);
        let code = chunk(&[
            Operation::While { cond: Operand::StateRef(0), body, next: Operand::StateRef(0) },
            Operation::Ret(Operand::StateRef(0)),
        ]);
        let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        assert!(js.contains("while (v"), "got:\n{js}");
        // The body's single statement is emitted exactly once.
        assert_eq!(js.matches("state[0] = 0;").count(), 1, "got:\n{js}");
    }

    /// Build a program whose first op is `FN` (with `variant`) pointing at a
    /// trivial body placed immediately after it, then a top-level `RET`.
    fn program_with_fn(variant: Operand) -> Vec<u8> {
        let fn_body = chunk(&[
            Operation::Lit32 { dest: 0, val: 1 },
            Operation::Ret(Operand::StateRef(0)),
        ]);
        let mk = |j: u32| Operation::Fn {
            variant,
            closure_args: Operand::Literal(0),
            spanner: Operand::Literal(0),
            j,
            dest: 5,
        };
        let mut code = chunk(&[mk(0)]);
        let j = code.len() as u32;
        code.extend_from_slice(&fn_body);
        // Re-encode FN now that the body offset `j` is known (same byte length).
        let fn_bytes: Vec<u8> = mk(j).emit().collect();
        code[..fn_bytes.len()].copy_from_slice(&fn_bytes);
        code.extend(Operation::Ret(Operand::StateRef(5)).emit());
        code
    }

    #[test]
    fn emits_function_with_tenant_nt_params() {
        let (js, reg) = compile(&program_with_fn(Operand::Literal(0)), VecRegistry::new(), Config::default()).unwrap();
        let prelude = reg.prelude();
        assert!(prelude.contains("function(tenant, nt, ...args)"), "got:\n{prelude}");
        assert!(js.contains("__fn0"), "got:\n{js}");
    }

    #[test]
    fn emits_mark_guest_fn_registration() {
        // Every compiled function must register itself as a "leading-tenant-nt" guest
        // function so tenant-side trap invocation (`invokeTrap`) calls it correctly.
        let (_js, reg) = compile(&program_with_fn(Operand::Literal(0)), VecRegistry::new(), Config::default()).unwrap();
        let prelude = reg.prelude();
        assert!(
            prelude.contains("markGuestFn(__fn0, {abi: \"leading-tenant-nt\"})"),
            "got:\n{prelude}"
        );
    }

    #[test]
    fn emits_variant_declaration_forms() {
        let kw = |variant: u32| {
            let (_js, reg) =
                compile(&program_with_fn(Operand::Literal(variant)), VecRegistry::new(), Config::default()).unwrap();
            reg.prelude()
        };
        assert!(kw(1).contains("async function(tenant, nt, ...args)"), "async: {}", kw(1));
        assert!(kw(2).contains("function*(tenant, nt, ...args)"), "gen: {}", kw(2));
        assert!(kw(3).contains("async function*(tenant, nt, ...args)"), "asyncgen: {}", kw(3));
    }

    #[test]
    fn emits_object_via_tenant_manager() {
        // obj = {}; obj[7] = 42; return obj[7];
        let code = chunk(&[
            Operation::Lit32 { dest: 0, val: 42 },
            Operation::Lit32 { dest: 1, val: 7 },
            Operation::Litobj {
                c: portal_solutions_jade_vm::SignedOperand::Positive(0),
                pairs: alloc::vec![],
                key: Operand::Literal(2), // store the new object into slot 2
            },
            Operation::Set {
                obj: Operand::StateRef(2),
                key: Operand::StateRef(1),
                val: Operand::StateRef(0),
                dest: 3,
            },
            Operation::Get { obj: Operand::StateRef(2), key: Operand::StateRef(1), dest: 4 },
            Operation::Ret(Operand::StateRef(4)),
        ]);
        let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        assert!(js.contains("tenant.make(null)"), "got:\n{js}");
        assert!(js.contains("tenant.set(state[2], state[1], state[0])"), "got:\n{js}");
        assert!(js.contains("tenant.get(state[2], state[1])"), "got:\n{js}");
        assert!(!js.contains("tenant.clean"), "got:\n{js}");
    }

    #[test]
    fn op_call_threads_tenant_and_nt() {
        // state[1] = call state[0]( state[2] ); return state[1];
        let code = chunk(&[
            Operation::Call {
                fn_op: Operand::StateRef(0),
                args: alloc::vec![Operand::StateRef(2)],
                dest: 1,
            },
            Operation::Ret(Operand::StateRef(1)),
        ]);
        let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        assert!(
            js.contains("Reflect.apply(state[0], undefined, [tenant, nt, state[2]])"),
            "got:\n{js}"
        );
    }

    fn get_set_code() -> Vec<u8> {
        // state[3] = get(state[0], state[1]); set(state[0], state[1], state[2]); return state[3];
        chunk(&[
            Operation::Get { obj: Operand::StateRef(0), key: Operand::StateRef(1), dest: 3 },
            Operation::Set {
                obj: Operand::StateRef(0),
                key: Operand::StateRef(1),
                val: Operand::StateRef(2),
                dest: 4,
            },
            Operation::Ret(Operand::StateRef(3)),
        ])
    }

    #[test]
    fn get_set_fall_back_to_tenant_calls_by_default() {
        let (js, _reg) = compile(&get_set_code(), VecRegistry::new(), Config::default()).unwrap();
        assert!(js.contains("tenant.get(state[0], state[1])"), "got:\n{js}");
        assert!(js.contains("tenant.set(state[0], state[1], state[2]);"), "got:\n{js}");
    }

    #[test]
    fn get_set_inline_when_tenant_methods_provided() {
        let mut cfg = Config::default();
        cfg.tenant_methods.insert(
            "get".to_string(),
            InlinableTenantMethod {
                params: alloc::vec!["o".to_string(), "k".to_string()],
                body_block: "{ return o.get(k); }".to_string(),
            },
        );
        cfg.tenant_methods.insert(
            "set".to_string(),
            InlinableTenantMethod {
                params: alloc::vec!["o".to_string(), "k".to_string(), "v".to_string()],
                body_block: "{ o.set(k, v); }".to_string(),
            },
        );
        let (js, _reg) = compile(&get_set_code(), VecRegistry::new(), cfg).unwrap();
        assert!(!js.contains("tenant.get("), "got:\n{js}");
        assert!(!js.contains("tenant.set("), "got:\n{js}");
        assert!(
            js.contains("(function(o, k){ return o.get(k); })(state[0], state[1])"),
            "got:\n{js}"
        );
        assert!(
            js.contains("(function(o, k, v){ o.set(k, v); })(state[0], state[1], state[2]);"),
            "got:\n{js}"
        );
    }

    #[test]
    fn get_falls_back_when_param_count_mismatches() {
        // A malformed/unexpected entry (wrong arity) must not get spliced in blindly.
        let mut cfg = Config::default();
        cfg.tenant_methods.insert(
            "get".to_string(),
            InlinableTenantMethod { params: alloc::vec!["only_one".to_string()], body_block: "{}".to_string() },
        );
        let (js, _reg) = compile(&get_set_code(), VecRegistry::new(), cfg).unwrap();
        assert!(js.contains("tenant.get(state[0], state[1])"), "got:\n{js}");
    }
}

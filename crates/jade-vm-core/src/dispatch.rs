/* This is GENERATED code by `regen.ts` */
use portal_solutions_jade_vm::{Operand, Operation};

/// State access for the Jade VM.
pub trait State {
    /// The platform's value type.
    type Value: Clone;

    /// Read state slot `idx`.
    fn get(&mut self, idx: u32) -> Self::Value;
    /// Write state slot `idx`.
    fn set(&mut self, idx: u32, val: Self::Value);
    /// Flush dirty cache entries to the underlying state storage.
    fn flush(&mut self);
    /// Flush then invalidate – call before a foreign function may mutate state.
    fn flush_and_invalidate(&mut self);
    /// Return the underlying state object; used by the FN opcode to capture the
    /// parent state reference in child closures.
    fn state_ref(&self) -> Self::Value;
}

/// Opcode operations for the Jade VM.
///
/// Each handler receives fully-resolved `Self::Value` arguments and
/// returns a value (or `Result<Self::Value, Self::Error>` for fallible ops).
/// Handlers do **not** write to state – `exec_op` calls `State::set`
/// after each handler returns.
pub trait Ops {
    /// The platform's value type.
    type Value: Clone;
    /// The platform's error type.
    type Error;

    // Value constructors --------------------------------------------------
    fn f64_val(&self, v: f64) -> Self::Value;
    fn str_val(&self, s: &str) -> Self::Value;
    fn undefined(&self) -> Self::Value;
    /// Construct an error value from a static message.
    fn err(msg: &'static str) -> Self::Error;

    /// Apply property descriptors in `props` to `target` in place.
    fn define_properties(&self, target: &Self::Value, props: Self::Value);

    // Opcode handlers (one per non-loop, non-block opcode) ----------------
    fn op_global(&self) -> Self::Value;
    fn op_fn(&mut self, code: &[u8], variant: Self::Value, closure_args: Self::Value, spanner: Self::Value, j: u32, parent_state: Self::Value) -> Result<Self::Value, Self::Error>;
    fn op_lit32(&self, val: u32) -> Self::Value;
    fn op_arr(&self, items: Vec<Self::Value>) -> Self::Value;
    fn op_str(&self, items: Vec<Self::Value>) -> Self::Value;
    fn op_litobj(&self, spread: Option<Self::Value>, pairs: Vec<(Self::Value, Self::Value)>) -> Self::Value;
    fn op_new_target(&self) -> Self::Value;
    fn op_call(&mut self, code: &[u8], fn_val: Self::Value, args: Vec<Self::Value>) -> Result<Self::Value, Self::Error>;
    fn op_bool(&self, val: bool) -> Self::Value;
    fn op_eq(&self, a: Self::Value, b: Self::Value) -> Self::Value;
    fn op_ne(&self, a: Self::Value, b: Self::Value) -> Self::Value;
    fn op_lt(&self, a: Self::Value, b: Self::Value) -> Self::Value;
    fn op_le(&self, a: Self::Value, b: Self::Value) -> Self::Value;
    fn op_gt(&self, a: Self::Value, b: Self::Value) -> Self::Value;
    fn op_ge(&self, a: Self::Value, b: Self::Value) -> Self::Value;
    fn op_sel(&self, cond: Self::Value, then: Self::Value, else_: Self::Value) -> Self::Value;
    fn op_get(&self, obj: Self::Value, key: Self::Value) -> Self::Value;
    fn op_set(&self, obj: Self::Value, key: Self::Value, val: Self::Value) -> Self::Value;

    // Control-flow handlers -----------------------------------------------
    // Ctx is a method-level generic so JIT backends can pass a compilation
    // context without it appearing in the trait bounds.

    /// Run a `while` loop. `init` is the initial condition value; each
    /// `body` call runs the loop body and returns the next condition value.
    /// Iteration continues while the condition is truthy; the final condition
    /// value is returned.
    fn while_op<Ctx, F>(
        &mut self,
        ctx: &mut Ctx,
        init: Self::Value,
        body: F,
    ) -> Result<Self::Value, Self::Error>
    where
        F: FnMut(&mut Self, &mut Ctx) -> Result<Self::Value, Self::Error>;

    /// Evaluate one of two branches depending on `cond`, returning the branch value.
    fn if_op<Ctx, FT, FE>(
        &mut self,
        ctx: &mut Ctx,
        cond: Self::Value,
        then_body: FT,
        else_body: FE,
    ) -> Result<Self::Value, Self::Error>
    where
        FT: FnOnce(&mut Self, &mut Ctx) -> Result<Self::Value, Self::Error>,
        FE: FnOnce(&mut Self, &mut Ctx) -> Result<Self::Value, Self::Error>;

    /// Match `val` against `cases` (by raw u32 tag), running the matching
    /// branch or `default_body`, returning the branch value.
    fn switch_op<Ctx, F, D>(
        &mut self,
        ctx: &mut Ctx,
        val: Self::Value,
        cases: impl IntoIterator<Item = (u32, F)>,
        default_body: D,
    ) -> Result<Self::Value, Self::Error>
    where
        F: FnOnce(&mut Self, &mut Ctx) -> Result<Self::Value, Self::Error>,
        D: FnOnce(&mut Self, &mut Ctx) -> Result<Self::Value, Self::Error>;
}

/// Resolve an `Operand`: literal → numeric value via `Ops::f64_val`,
/// state-ref → slot read via `State::get`.
pub fn resolve<P>(op: Operand, platform: &mut P) -> <P as State>::Value
where
    P: State + Ops<Value = <P as State>::Value>,
{
    match op {
        Operand::Literal(v) => platform.f64_val(v as f64),
        Operand::StateRef(idx) => platform.get(idx),
    }
}

/// Dispatch an `Operation` through the platform.
///
/// Resolves operands, calls the appropriate `Ops` method, and writes the
/// result to the dest slot via `State::set`. Block opcodes (WHILE, IF,
/// SWITCH) carry embedded bytecode; their bodies are wrapped in closures (which
/// recurse back into `exec_op`) and forwarded to the matching `Ops`
/// control-flow method.
///
/// `ctx` is threaded into the control-flow handlers as a method-level
/// generic: the interpreter passes `&mut ()`; a JIT backend passes its own
/// compilation context.
///
/// Returns `Err` only for the loop-level opcodes (RET/AWAIT/YIELD/YIELDSTAR),
/// which the VM loop handles directly before reaching here.
pub fn exec_op<P, Ctx>(
    op: Operation,
    code: &[u8],
    platform: &mut P,
    ctx: &mut Ctx,
) -> Result<(), <P as Ops>::Error>
where
    P: State + Ops<Value = <P as State>::Value>,
{
    match op {
        Operation::Global(dest) => {
            let val = platform.op_global();
            platform.set(dest, val);
            Ok(())
        }
        Operation::Fn { variant, closure_args, spanner, j, dest } => {
            platform.flush();
            let parent = platform.state_ref();
            let r0 = resolve(variant, platform);
            let r1 = resolve(closure_args, platform);
            let r2 = resolve(spanner, platform);
            let fn_val = platform.op_fn(code, r0, r1, r2, j, parent)?;
            platform.set(dest, fn_val);
            Ok(())
        }
        Operation::Lit32 { dest, val } => {
            let v = platform.op_lit32(val);
            platform.set(dest, v);
            Ok(())
        }
        Operation::Arr(items, dest) => {
            let items: Vec<_> = items.into_iter().map(|op| resolve(op, platform)).collect();
            let val = platform.op_arr(items);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Str(items, dest) => {
            let items: Vec<_> = items.into_iter().map(|op| resolve(op, platform)).collect();
            let val = platform.op_str(items);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Litobj { c, pairs, key } => {
            let mut iter = pairs.into_iter();
            let spread = if c.as_i32() < 0 {
                Some(resolve(iter.next().unwrap().0, platform))
            } else { None };
            let kv: Vec<_> = iter.map(|(k, v)| {
                let k = resolve(k, platform);
                let v = resolve(v, platform);
                (k, v)
            }).collect();
            let obj = platform.op_litobj(spread, kv);
            match key {
                Operand::Literal(idx) => platform.set(idx, obj),
                Operand::StateRef(idx) => {
                    let target = platform.get(idx);
                    platform.define_properties(&target, obj);
                }
            }
            Ok(())
        }
        Operation::NewTarget(dest) => {
            let val = platform.op_new_target();
            platform.set(dest, val);
            Ok(())
        }
        Operation::Call { fn_op, args, dest } => {
            let fn_val = resolve(fn_op, platform);
            let args: Vec<_> = args.into_iter().map(|op| resolve(op, platform)).collect();
            let result = platform.op_call(code, fn_val, args)?;
            platform.set(dest, result);
            Ok(())
        }
        Operation::Bool { val, dest } => {
            let v = platform.op_bool(val);
            platform.set(dest, v);
            Ok(())
        }
        Operation::Eq { a, b, dest } => {
            let av = resolve(a, platform);
            let bv = resolve(b, platform);
            let val = platform.op_eq(av, bv);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Ne { a, b, dest } => {
            let av = resolve(a, platform);
            let bv = resolve(b, platform);
            let val = platform.op_ne(av, bv);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Lt { a, b, dest } => {
            let av = resolve(a, platform);
            let bv = resolve(b, platform);
            let val = platform.op_lt(av, bv);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Le { a, b, dest } => {
            let av = resolve(a, platform);
            let bv = resolve(b, platform);
            let val = platform.op_le(av, bv);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Gt { a, b, dest } => {
            let av = resolve(a, platform);
            let bv = resolve(b, platform);
            let val = platform.op_gt(av, bv);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Ge { a, b, dest } => {
            let av = resolve(a, platform);
            let bv = resolve(b, platform);
            let val = platform.op_ge(av, bv);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Sel { cond, then, else_, dest } => {
            let cv = resolve(cond, platform);
            let tv = resolve(then, platform);
            let ev = resolve(else_, platform);
            let val = platform.op_sel(cv, tv, ev);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Get { obj, key, dest } => {
            let o = resolve(obj, platform);
            let k = resolve(key, platform);
            let val = platform.op_get(o, k);
            platform.set(dest, val);
            Ok(())
        }
        Operation::Set { obj, key, val, dest } => {
            let o = resolve(obj, platform);
            let k = resolve(key, platform);
            let v = resolve(val, platform);
            let r = platform.op_set(o, k, v);
            platform.set(dest, r);
            Ok(())
        }
        Operation::While { cond, body, next } => {
            let init = resolve(cond, platform);
            let result = platform.while_op(ctx, init, |p, c| {
                { let mut rem: &[u8] = &body; while let Some((op, rest)) = Operation::parse(rem) { rem = rest; exec_op(op, code, p, c)?; } }
                Ok(resolve(next, p))
            })?;
            let _ = result;
            Ok(())
        }
        Operation::If { cond, then_body, else_body } => {
            let cond_val = resolve(cond, platform);
            let result = platform.if_op(
                ctx,
                cond_val,
                |p, c| { { let mut rem: &[u8] = &then_body; while let Some((op, rest)) = Operation::parse(rem) { rem = rest; exec_op(op, code, p, c)?; } } Ok(p.undefined()) },
                |p, c| { { let mut rem: &[u8] = &else_body; while let Some((op, rest)) = Operation::parse(rem) { rem = rest; exec_op(op, code, p, c)?; } } Ok(p.undefined()) },
            )?;
            let _ = result;
            Ok(())
        }
        Operation::Switch { val, cases, default_body } => {
            let val_v = resolve(val, platform);
            let result = platform.switch_op(
                ctx,
                val_v,
                cases.into_iter().map(|(cv, cb)| (cv, move |p: &mut P, c: &mut Ctx| { { let mut rem: &[u8] = &cb; while let Some((op, rest)) = Operation::parse(rem) { rem = rest; exec_op(op, code, p, c)?; } } Ok(p.undefined()) })),
                |p: &mut P, c: &mut Ctx| { { let mut rem: &[u8] = &default_body; while let Some((op, rest)) = Operation::parse(rem) { rem = rest; exec_op(op, code, p, c)?; } } Ok(p.undefined()) },
            )?;
            let _ = result;
            Ok(())
        }
        _ => Err(P::err("exec_op: unexpected opcode")),
    }
}

/* This is GENERATED code by `regen.ts` */
use portal_solutions_jade_vm::{Operand, Operation};

/// State access for the Jade VM.
///
/// Provides slot read/write, cache flushing, and a snapshot of the underlying
/// state object needed by the FN opcode to capture the parent state in closures.
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
///
/// `Value` must match the associated `State::Value` for types that
/// implement both traits.
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
    /// Used by the LITOBJ opcode when its key operand is a state reference.
    fn define_properties(&self, target: &Self::Value, props: Self::Value);

    // Opcode handlers (one per non-loop opcode) ---------------------------
    // Each returns the computed value; exec_op writes it to the dest slot.
    fn op_global(&self) -> Self::Value;
    fn op_fn(&mut self, code: &[u8], variant: Self::Value, closure_args: Self::Value, spanner: Self::Value, j: u32, parent_state: Self::Value) -> Result<Self::Value, Self::Error>;
    fn op_lit32(&self, val: u32) -> Self::Value;
    fn op_arr(&self, items: Vec<Self::Value>) -> Self::Value;
    fn op_str(&self, items: Vec<Self::Value>) -> Self::Value;
    fn op_litobj(&self, spread: Option<Self::Value>, pairs: Vec<(Self::Value, Self::Value)>) -> Self::Value;
    fn op_new_target(&self) -> Self::Value;
    fn op_call(&mut self, code: &[u8], fn_val: Self::Value, args: Vec<Self::Value>) -> Result<Self::Value, Self::Error>;
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

/// Dispatch a non-loop `Operation` through the platform.
///
/// Resolves operands, calls the appropriate `Ops` method, and writes the
/// result to the dest slot via `State::set`.
/// Returns `Err` only for unknown opcodes.
pub fn exec_op<P>(
    op: Operation,
    code: &[u8],
    platform: &mut P,
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
        _ => Err(P::err("exec_op: unexpected opcode")),
    }
}

/* This is GENERATED code by `update.mjs` */
use portal_solutions_jade_vm::Operation;

/// Platform abstraction for the Jade VM interpreter.
///
/// Implementors provide:
/// - State access (get/set with optional caching, flush for persistence)
/// - Value constructors (f64, string, undefined)
/// - Opcode handlers for all non-loop opcodes
///
/// The two interpreter-loop opcodes (RET via "src", and AWAIT/YIELD/YIELDSTAR
/// via "src_dest") are handled by the caller using `crate::resolve`.
pub trait Platform {
    /// The platform's value type (e.g. JsValue on WASM).
    type Value: Clone;
    /// The platform's error type.
    type Error;

    // State access --------------------------------------------------------
    /// Read state slot `idx`.
    fn get(&mut self, idx: u32) -> Self::Value;
    /// Write state slot `idx`.
    fn set(&mut self, idx: u32, val: Self::Value);
    /// Flush dirty cache entries to the underlying state storage.
    fn flush(&mut self);
    /// Flush then invalidate the cache (call before a foreign function may mutate state).
    fn flush_and_invalidate(&mut self);

    // Value construction --------------------------------------------------
    fn f64_val(&self, v: f64) -> Self::Value;
    fn str_val(&self, s: &str) -> Self::Value;
    fn undefined(&self) -> Self::Value;

    // Error construction --------------------------------------------------
    fn err(msg: &'static str) -> Self::Error;

    // Opcode handlers (one per non-loop opcode) ---------------------------
    fn op_global(&mut self, dest: u32) -> Result<(), Self::Error>;
    fn op_fn(&mut self, code: &[u8], variant: Self::Value, closure_args: Self::Value, spanner: Self::Value, j: u32, dest: u32) -> Result<(), Self::Error>;
    fn op_lit32(&mut self, dest: u32, val: u32) -> Result<(), Self::Error>;
    fn op_arr(&mut self, items: Vec<Self::Value>, dest: u32) -> Result<(), Self::Error>;
    fn op_str(&mut self, items: Vec<Self::Value>, dest: u32) -> Result<(), Self::Error>;
    fn op_litobj(&mut self, spread: Option<Self::Value>, pairs: Vec<(Self::Value, Self::Value)>, key: portal_solutions_jade_vm::Operand) -> Result<(), Self::Error>;
    fn op_new_target(&mut self, dest: u32) -> Result<(), Self::Error>;
    fn op_call(&mut self, code: &[u8], fn_val: Self::Value, args: Vec<Self::Value>, dest: u32) -> Result<(), Self::Error>;
}

/// Dispatch a non-loop `Operation` through the platform.
///
/// Operands are resolved before being passed to trait methods, so each
/// method receives ready-to-use `Self::Value` arguments.
/// Returns `Err` only for truly unknown opcodes (should never happen with
/// a well-formed bytecode stream).
pub fn exec_op<P: Platform>(
    op: Operation,
    code: &[u8],
    platform: &mut P,
) -> Result<(), P::Error> {
    match op {
        Operation::Global(dest) => platform.op_global(dest),
        Operation::Fn { variant, closure_args, spanner, j, dest } => {
            let r0 = crate::resolve(variant, platform);
            let r1 = crate::resolve(closure_args, platform);
            let r2 = crate::resolve(spanner, platform);
            platform.op_fn(code, r0, r1, r2, j, dest)
        }
        Operation::Lit32 { dest, val } => platform.op_lit32(dest, val),
        Operation::Arr(items, dest) => {
            let items: Vec<_> = items.into_iter().map(|op| crate::resolve(op, platform)).collect();
            platform.op_arr(items, dest)
        }
        Operation::Str(items, dest) => {
            let items: Vec<_> = items.into_iter().map(|op| crate::resolve(op, platform)).collect();
            platform.op_str(items, dest)
        }
        Operation::Litobj { c, pairs, key } => {
            let mut iter = pairs.into_iter();
            let spread = if c.as_i32() < 0 {
                Some(crate::resolve(iter.next().unwrap().0, platform))
            } else { None };
            let kv: Vec<_> = iter.map(|(k, v)| {
                let k = crate::resolve(k, platform);
                let v = crate::resolve(v, platform);
                (k, v)
            }).collect();
            platform.op_litobj(spread, kv, key)
        }
        Operation::NewTarget(dest) => platform.op_new_target(dest),
        Operation::Call { fn_op, args, dest } => {
            let fn_val = crate::resolve(fn_op, platform);
            let args: Vec<_> = args.into_iter().map(|op| crate::resolve(op, platform)).collect();
            platform.op_call(code, fn_val, args, dest)
        }
        _ => Err(P::err("exec_op: unexpected opcode")),
    }
}

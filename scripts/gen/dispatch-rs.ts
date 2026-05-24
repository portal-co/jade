import { pascal, LOOP_LEVEL, BLOCK_ARGS } from "./shared.ts";

function opsMethod(name: string, args: string): string | null {
  const mn = `op_${name.toLowerCase()}`;
  switch (args) {
    case "dest":
      return `    fn ${mn}(&self) -> Self::Value;`;
    case "lit32":
      return `    fn op_lit32(&self, val: u32) -> Self::Value;`;
    case "bool":
      return `    fn op_bool(&self, val: bool) -> Self::Value;`;
    case "binop":
      return `    fn ${mn}(&self, a: Self::Value, b: Self::Value) -> Self::Value;`;
    case "sel":
      return `    fn op_sel(&self, cond: Self::Value, then: Self::Value, else_: Self::Value) -> Self::Value;`;
    case "fn":
      return `    fn op_fn(&mut self, code: &[u8], variant: Self::Value, closure_args: Self::Value, spanner: Self::Value, j: u32, parent_state: Self::Value) -> Result<Self::Value, Self::Error>;`;
    case "array":
      return `    fn ${mn}(&self, items: Vec<Self::Value>) -> Self::Value;`;
    case "object":
      return `    fn op_litobj(&self, spread: Option<Self::Value>, pairs: Vec<(Self::Value, Self::Value)>) -> Self::Value;`;
    case "call":
      return `    fn op_call(&mut self, code: &[u8], fn_val: Self::Value, args: Vec<Self::Value>) -> Result<Self::Value, Self::Error>;`;
    default:
      return null;
  }
}

function execArm(name: string, args: string): string | null {
  const v = pascal(name);
  const mn = `op_${name.toLowerCase()}`;
  switch (args) {
    case "dest":
      return `        Operation::${v}(dest) => {
            let val = platform.${mn}();
            platform.set(dest, val);
            Ok(())
        }`;
    case "lit32":
      return `        Operation::Lit32 { dest, val } => {
            let v = platform.op_lit32(val);
            platform.set(dest, v);
            Ok(())
        }`;
    case "bool":
      return `        Operation::${v} { val, dest } => {
            let v = platform.op_bool(val);
            platform.set(dest, v);
            Ok(())
        }`;
    case "binop":
      return `        Operation::${v} { a, b, dest } => {
            let av = resolve(a, platform);
            let bv = resolve(b, platform);
            let val = platform.${mn}(av, bv);
            platform.set(dest, val);
            Ok(())
        }`;
    case "sel":
      return `        Operation::Sel { cond, then, else_, dest } => {
            let cv = resolve(cond, platform);
            let tv = resolve(then, platform);
            let ev = resolve(else_, platform);
            let val = platform.op_sel(cv, tv, ev);
            platform.set(dest, val);
            Ok(())
        }`;
    case "fn":
      return `        Operation::Fn { variant, closure_args, spanner, j, dest } => {
            platform.flush();
            let parent = platform.state_ref();
            let r0 = resolve(variant, platform);
            let r1 = resolve(closure_args, platform);
            let r2 = resolve(spanner, platform);
            let fn_val = platform.op_fn(code, r0, r1, r2, j, parent)?;
            platform.set(dest, fn_val);
            Ok(())
        }`;
    case "array":
      return `        Operation::${v}(items, dest) => {
            let items: Vec<_> = items.into_iter().map(|op| resolve(op, platform)).collect();
            let val = platform.${mn}(items);
            platform.set(dest, val);
            Ok(())
        }`;
    case "object":
      return `        Operation::Litobj { c, pairs, key } => {
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
        }`;
    case "call":
      return `        Operation::Call { fn_op, args, dest } => {
            let fn_val = resolve(fn_op, platform);
            let args: Vec<_> = args.into_iter().map(|op| resolve(op, platform)).collect();
            let result = platform.op_call(code, fn_val, args)?;
            platform.set(dest, result);
            Ok(())
        }`;
    default:
      return null;
  }
}

function execBlockArm(name: string, args: string): string | null {
  const v = pascal(name);
  // Helper snippet: interpret all ops in a byte slice, writing results to state via exec_op.
  // `code` is `Copy` (&[u8]) so it is safe to capture in move closures.
  const runBody = (slice: string) =>
    `{ let mut rem: &[u8] = ${slice}; while let Some((op, rest)) = Operation::parse(rem) { rem = rest; exec_op(op, code, p)?; } Ok(p.undefined()) }`;
  switch (args) {
    case "fixpoint_block":
      return `        Operation::${v}(body) => {
            let init = platform.undefined();
            let result = platform.fixpoint(ctx, init, |p, _c, _val| ${runBody("&body")})?;
            let _ = result;
            Ok(())
        }`;
    case "if_block":
      return `        Operation::${v} { cond, then_body, else_body } => {
            let cond_val = resolve(cond, platform);
            let result = platform.if_op(
                ctx,
                cond_val,
                |p, _c| ${runBody("&then_body")},
                |p, _c| ${runBody("&else_body")},
            )?;
            let _ = result;
            Ok(())
        }`;
    case "switch_block":
      return `        Operation::${v} { val, cases, default_body } => {
            let val_v = resolve(val, platform);
            let result = platform.switch_op(
                ctx,
                val_v,
                cases.into_iter().map(|(cv, cb)| (cv, move |p: &mut P, _c: &mut Ctx| ${runBody("&cb")})),
                |p: &mut P, _c: &mut Ctx| ${runBody("&default_body")},
            )?;
            let _ = result;
            Ok(())
        }`;
    default:
      return null;
  }
}

export function genDispatchRs(opcodes: Record<string, any>): string {
  const entries = Object.entries(opcodes) as [string, any][];

  const opsMethods = entries
    .filter(([, { args }]) => !LOOP_LEVEL.has(args) && !BLOCK_ARGS.has(args))
    .map(([name, { args }]) => opsMethod(name, args))
    .filter(Boolean)
    // deduplicate: lit32, sel, bool each appear once regardless of how many opcodes share the type
    .filter((v, i, a) => a.indexOf(v) === i)
    .join("\n");

  const execArms = entries
    .filter(([, { args }]) => !LOOP_LEVEL.has(args) && !BLOCK_ARGS.has(args))
    .map(([name, { args }]) => execArm(name, args))
    .filter(Boolean)
    .join("\n");

  const execBlockArms = entries
    .filter(([, { args }]) => BLOCK_ARGS.has(args))
    .map(([name, { args }]) => execBlockArm(name, args))
    .filter(Boolean)
    .join("\n");

  const BT = "\x60";
  return `/* This is GENERATED code by ${BT}regen.ts${BT} */
use portal_solutions_jade_vm::{Operand, Operation};

/// State access for the Jade VM.
pub trait State {
    /// The platform's value type.
    type Value: Clone;

    /// Read state slot ${BT}idx${BT}.
    fn get(&mut self, idx: u32) -> Self::Value;
    /// Write state slot ${BT}idx${BT}.
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
/// Each handler receives fully-resolved ${BT}Self::Value${BT} arguments and
/// returns a value (or ${BT}Result<Self::Value, Self::Error>${BT} for fallible ops).
/// Handlers do **not** write to state – ${BT}exec_op${BT} calls ${BT}State::set${BT}
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

    /// Apply property descriptors in ${BT}props${BT} to ${BT}target${BT} in place.
    fn define_properties(&self, target: &Self::Value, props: Self::Value);

    // Opcode handlers (one per non-loop, non-block opcode) ----------------
${opsMethods}

    // Control-flow handlers -----------------------------------------------
    // Ctx is a method-level generic so JIT backends can pass a compilation
    // context without it appearing in the trait bounds.

    /// Run ${BT}body${BT} repeatedly, threading ${BT}Self::Value${BT} through each iteration.
    /// ${BT}body${BT} returns the next value; iteration continues until the implementor
    /// decides the result is stable.
    fn fixpoint<Ctx, F>(
        &mut self,
        ctx: &mut Ctx,
        init: Self::Value,
        body: F,
    ) -> Result<Self::Value, Self::Error>
    where
        F: FnMut(&mut Self, &mut Ctx, Self::Value) -> Result<Self::Value, Self::Error>;

    /// Evaluate one of two branches depending on ${BT}cond${BT}, returning the branch value.
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

    /// Match ${BT}val${BT} against ${BT}cases${BT} (by raw u32 tag), running the matching
    /// branch or ${BT}default_body${BT}, returning the branch value.
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

/// Resolve an ${BT}Operand${BT}: literal → numeric value via ${BT}Ops::f64_val${BT},
/// state-ref → slot read via ${BT}State::get${BT}.
pub fn resolve<P>(op: Operand, platform: &mut P) -> <P as State>::Value
where
    P: State + Ops<Value = <P as State>::Value>,
{
    match op {
        Operand::Literal(v) => platform.f64_val(v as f64),
        Operand::StateRef(idx) => platform.get(idx),
    }
}

/// Dispatch a non-loop, non-block ${BT}Operation${BT} through the platform.
///
/// Resolves operands, calls the appropriate ${BT}Ops${BT} method, and writes the
/// result to the dest slot via ${BT}State::set${BT}.
/// Returns ${BT}Err${BT} only for unknown or block opcodes (use ${BT}exec_block_op${BT} for those).
pub fn exec_op<P>(
    op: Operation,
    code: &[u8],
    platform: &mut P,
) -> Result<(), <P as Ops>::Error>
where
    P: State + Ops<Value = <P as State>::Value>,
{
    match op {
${execArms}
        _ => Err(P::err("exec_op: unexpected opcode")),
    }
}

/// Dispatch a block ${BT}Operation${BT} (FIXPOINT, IF, SWITCH) through the platform.
///
/// Block opcodes contain embedded bytecode; this function wraps the body bytes
/// in closures and forwards them to the appropriate ${BT}Ops${BT} control-flow method.
/// Pass ${BT}ctx: &mut ()${BT} for the interpreter; a JIT backend passes its own context.
pub fn exec_block_op<P, Ctx>(
    op: Operation,
    code: &[u8],
    platform: &mut P,
    ctx: &mut Ctx,
) -> Result<(), <P as Ops>::Error>
where
    P: State + Ops<Value = <P as State>::Value>,
{
    match op {
${execBlockArms}
        _ => Err(P::err("exec_block_op: not a block opcode")),
    }
}
`;
}

import { pascal, LOOP_LEVEL } from "./shared.ts";

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
    case "member_get":
      return `    fn op_get(&self, obj: Self::Value, key: Self::Value) -> Self::Value;`;
    case "member_set":
      return `    fn op_set(&self, obj: Self::Value, key: Self::Value, val: Self::Value) -> Self::Value;`;
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
    case "member_get":
      return `        Operation::${v} { obj, key, dest } => {
            let o = resolve(obj, platform);
            let k = resolve(key, platform);
            let val = platform.op_get(o, k);
            platform.set(dest, val);
            Ok(())
        }`;
    case "member_set":
      return `        Operation::${v} { obj, key, val, dest } => {
            let o = resolve(obj, platform);
            let k = resolve(key, platform);
            let v = resolve(val, platform);
            let r = platform.op_set(o, k, v);
            platform.set(dest, r);
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

export function genDispatchRs(opcodes: Record<string, any>): string {
  const entries = Object.entries(opcodes) as [string, any][];

  const opsMethods = entries
    .filter(([, { args }]) => !LOOP_LEVEL.has(args))
    .map(([name, { args }]) => opsMethod(name, args))
    .filter(Boolean)
    // deduplicate: lit32, sel, bool each appear once regardless of how many opcodes share the type
    .filter((v, i, a) => a.indexOf(v) === i)
    .join("\n");

  const execArms = entries
    .filter(([, { args }]) => !LOOP_LEVEL.has(args))
    .map(([name, { args }]) => execArm(name, args))
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

    // Opcode handlers (one per value-producing opcode) --------------------
${opsMethods}
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

/// Dispatch an ${BT}Operation${BT} through the platform.
///
/// Resolves operands, calls the appropriate ${BT}Ops${BT} method, and writes the
/// result to the dest slot via ${BT}State::set${BT}.
///
/// Returns ${BT}Err${BT} for every "loop-level" opcode (${BT}RET${BT}/${BT}AWAIT${BT}/${BT}YIELD${BT}/
/// ${BT}YIELDSTAR${BT}, and the jump-family control-transfer ops ${BT}JMP${BT}/${BT}CONDJMP${BT}/${BT}SWITCH${BT}) —
/// none of these produce a value via a normal ${BT}op_*()${BT}+${BT}set()${BT} pair; each caller's own
/// driving loop handles them directly (updating its own program counter, or
/// returning) before ever reaching this dispatcher.
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
`;
}

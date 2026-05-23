import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { writeFileSync } from "node:fs";

import { opcodes,handlers } from "../packages/jade-data/dist/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));



type VMOpt = { async?: boolean; gen?: boolean };

const vmcode = [{ async: true, gen: true }, { async: true }, { gen: true }, {}]
  .map((o: VMOpt) => {
    const isAsync = "async" in o;
    const isGenerator = "gen" in o;
    const functionName = ({ isAsync: ak_ = isAsync, isGenerator: gk_ = isGenerator }: { isAsync?: boolean; isGenerator?: boolean }) =>
      `runVirtualized${ak_ ? "A" : ""}${gk_ ? "G" : ""}`;
    const parameters = `(unshift(args,freeze({__proto__:null,ip:ip-2,globalThis,nt,tenant})),unshift(args,state),unshift(args,code),args)`;
    return `
export ${isAsync ? "async" : ""} function${isGenerator ? "*" : ""} runVirtualized${
      isAsync ? "A" : ""
    }${
      isGenerator ? "G" : ""
    }(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): ${
      isAsync ? (isGenerator ? `AsyncGenerator<any,any,any>` : `Promise<any>`) : `any`
    }{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || ${isAsync ? "op === 1" : "false"} || ${
      isGenerator ? "op === 2 || op === 3 " : "false"
    }) ? arg() : undefined;
        switch(op){
            
            case ${opcodes.AWAIT.id}: ${
      isAsync
        ? `state[code().getUint32(ip,true)]=await val;ip += 4;break;`
        : `return apply(${functionName({
            isAsync: true,
          })},this,${parameters});`
    }
            case ${opcodes.YIELD.id}: ${
      isGenerator
        ? `state[code().getUint32(ip,true)]=yield val;ip += 4;break;`
        : `return apply(${functionName({
            isGenerator: true,
          })},this,${parameters});`
    }
            case ${opcodes.YIELDSTAR.id}: ${
      isGenerator
        ? `state[code().getUint32(ip,true)]=yield* val;ip += 4;break;`
        : `return apply(${functionName({
            isGenerator: true,
          })},this,${parameters});`
    }
    ${Reflect.ownKeys(opcodes)
      .filter((op) => op !== "AWAIT" && op !== "YIELD" && op !== "YIELDSTAR")
      .map((op) => `case ${opcodes[op].id}: ${handlers[op]}`)
      .join("")}
        }
    }
}`;
  })
  .join("\n");

writeFileSync(
  `${__dirname}/../packages/jade-js/vm.ts`,
  `
/* This is GENERATED code by \`regen.ts\` */
import {type Tenant} from "./index.ts"
const {apply} = Reflect;
const {create,defineProperties,freeze} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
const unshift = Array.prototype.unshift.call.bind(Array.prototype.unshift);
${vmcode}`
);
writeFileSync(
  `${__dirname}/../crates/jade-vm/src/data.rs`,
  (() => {
    const entries = Object.entries(opcodes) as [string, any][];
    const pascal = (s: string) =>
      s.split("_").map((p) => p[0] + p.slice(1).toLowerCase()).join("");

    // -----------------------------------------------------------------------
    // Rust enum variant, parse, emit, and gen-block arms.
    // Each args type maps to a precise bytecode layout.
    //
    // "src"      – [LSB val]                             (e.g. RET)
    // "src_dest" – [LSB val] [raw dest]                 (e.g. AWAIT/YIELD/YIELDSTAR)
    // "dest"     – [raw dest]                            (e.g. GLOBAL/NEW_TARGET)
    // "fn"       – [LSB var][LSB clos][LSB span][raw j][raw dest]
    // "lit32"    – [raw dest][raw val]
    // "array"    – [raw len][LSB items…][raw dest]
    // "object"   – [i32 c][spread?][LSB pairs…][LSB key]
    // "call"     – [LSB fn][raw n][LSB args…][raw dest]
    // -----------------------------------------------------------------------

    const O = (code: string) => `wtr.push(${code} as u8); wtr.push((${code}>>8) as u8);`;
    const Oy = (code: string) => `yield_!(${code} as u8); yield_!((${code}>>8) as u8);`;

    const enumVariants = entries
      .map(([name, info]) => {
        const v = pascal(name);
        switch (info.args) {
          case "src":
            return `    ${v}(crate::Operand),`;
          case "src_dest":
            return `    ${v} { val: crate::Operand, dest: u32 },`;
          case "dest":
            return `    ${v}(u32),`;
          case "fn":
            return `    Fn { variant: crate::Operand, closure_args: crate::Operand, spanner: crate::Operand, j: u32, dest: u32 },`;
          case "lit32":
            return `    Lit32 { dest: u32, val: u32 },`;
          case "array":
            return `    #[cfg(feature = "alloc")]\n    ${v}(Vec<crate::Operand>, u32),`;
          case "object":
            return `    #[cfg(feature = "alloc")]\n    ${v}{ c: crate::SignedOperand, pairs: Vec<(crate::Operand, crate::Operand)>, key: crate::Operand },`;
          case "call":
            return `    #[cfg(feature = "alloc")]\n    ${v}{ fn_op: crate::Operand, args: Vec<crate::Operand>, dest: u32 },`;
          default:
            return `    ${v},`;
        }
      })
      .join("\n");

    const parseArms = entries
      .map(([name, info]) => {
        const { id } = info;
        const v = pascal(name);
        switch (info.args) {
          case "src":
            return `            ${id} => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::${v}(crate::Operand::decode(a)), &buf[off..])) },`;
          case "src_dest":
            return `            ${id} => { let (a,no) = read_u32_le(buf, off)?; off = no; let (dest,no) = read_u32_le(buf, off)?; off = no; Some((Operation::${v}{ val: crate::Operand::decode(a), dest }, &buf[off..])) },`;
          case "dest":
            return `            ${id} => { let (dest,no) = read_u32_le(buf, off)?; off = no; Some((Operation::${v}(dest), &buf[off..])) },`;
          case "fn":
            return `            ${id} => { let (var_r,no)=read_u32_le(buf,off)?; off=no; let (clos_r,no)=read_u32_le(buf,off)?; off=no; let (span_r,no)=read_u32_le(buf,off)?; off=no; let (j,no)=read_u32_le(buf,off)?; off=no; let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::Fn{ variant: crate::Operand::decode(var_r), closure_args: crate::Operand::decode(clos_r), spanner: crate::Operand::decode(span_r), j, dest }, &buf[off..])) },`;
          case "lit32":
            return `            ${id} => { let (dest,no)=read_u32_le(buf,off)?; off=no; let (val,no)=read_u32_le(buf,off)?; off=no; Some((Operation::Lit32{ dest, val }, &buf[off..])) },`;
          case "array":
            return `            #[cfg(feature = "alloc")]
            ${id} => { let (len,no)=read_u32_le(buf,off)?; off=no; let mut items=Vec::with_capacity(len as usize); for _ in 0..len { let (x,no2)=read_u32_le(buf,off)?; off=no2; items.push(crate::Operand::decode(x)); } let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}(items, dest), &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            ${id} => { return None },`;
          case "object":
            return `            #[cfg(feature = "alloc")]
            ${id} => { let (c,no)=read_i32_le(buf,off)?; off=no; let mut pairs=Vec::new(); let mut cnt=if c>=0{c as usize}else{(-c) as usize}; if c<0 { let (sp,no2)=read_u32_le(buf,off)?; off=no2; pairs.push((crate::Operand::decode(sp), crate::Operand::decode(0))); } while cnt>0 { let (k,no2)=read_u32_le(buf,off)?; off=no2; let (v,no3)=read_u32_le(buf,off)?; off=no3; pairs.push((crate::Operand::decode(k), crate::Operand::decode(v))); cnt-=1; } let (key,no4)=read_u32_le(buf,off)?; off=no4; Some((Operation::${v}{ c: crate::SignedOperand::decode(c as u32), pairs, key: crate::Operand::decode(key) }, &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            ${id} => { return None },`;
          case "call":
            return `            #[cfg(feature = "alloc")]
            ${id} => { let (fn_r,no)=read_u32_le(buf,off)?; off=no; let (len,no)=read_u32_le(buf,off)?; off=no; let mut args=Vec::with_capacity(len as usize); for _ in 0..len { let (x,no2)=read_u32_le(buf,off)?; off=no2; args.push(crate::Operand::decode(x)); } let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ fn_op: crate::Operand::decode(fn_r), args, dest }, &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            ${id} => { return None },`;
          default:
            return `            ${id} => Some((Operation::${v}, &buf[off..])),`;
        }
      })
      .join("\n");

    const emitArms = entries
      .map(([name, info]) => {
        const { id } = info;
        const v = pascal(name);
        const hdr = O(String(id));
        switch (info.args) {
          case "src":
            return `            Operation::${v}(a) => { ${hdr} wtr.extend_from_slice(&a.encode().to_le_bytes()); },`;
          case "src_dest":
            return `            Operation::${v}{val, dest} => { ${hdr} wtr.extend_from_slice(&val.encode().to_le_bytes()); wtr.extend_from_slice(&dest.to_le_bytes()); },`;
          case "dest":
            return `            Operation::${v}(dest) => { ${hdr} wtr.extend_from_slice(&dest.to_le_bytes()); },`;
          case "fn":
            return `            Operation::Fn{variant, closure_args, spanner, j, dest} => { ${hdr} wtr.extend_from_slice(&variant.encode().to_le_bytes()); wtr.extend_from_slice(&closure_args.encode().to_le_bytes()); wtr.extend_from_slice(&spanner.encode().to_le_bytes()); wtr.extend_from_slice(&j.to_le_bytes()); wtr.extend_from_slice(&dest.to_le_bytes()); },`;
          case "lit32":
            return `            Operation::Lit32{dest, val} => { ${hdr} wtr.extend_from_slice(&dest.to_le_bytes()); wtr.extend_from_slice(&val.to_le_bytes()); },`;
          case "array":
            return `            #[cfg(feature = "alloc")]
            Operation::${v}(items, dest) => { ${hdr} wtr.extend_from_slice(&(items.len() as u32).to_le_bytes()); for x in items { wtr.extend_from_slice(&x.encode().to_le_bytes()); } wtr.extend_from_slice(&dest.to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}(..) => { /* alloc disabled: cannot emit */ },`;
          case "object":
            return `            #[cfg(feature = "alloc")]
            Operation::${v}{c, pairs, key} => { ${hdr} wtr.extend_from_slice(&c.encode().to_le_bytes()); for (k, v) in pairs { wtr.extend_from_slice(&k.encode().to_le_bytes()); wtr.extend_from_slice(&v.encode().to_le_bytes()); } wtr.extend_from_slice(&key.encode().to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}{..} => { /* alloc disabled: cannot emit */ },`;
          case "call":
            return `            #[cfg(feature = "alloc")]
            Operation::${v}{fn_op, args, dest} => { ${hdr} wtr.extend_from_slice(&fn_op.encode().to_le_bytes()); wtr.extend_from_slice(&(args.len() as u32).to_le_bytes()); for x in args { wtr.extend_from_slice(&x.encode().to_le_bytes()); } wtr.extend_from_slice(&dest.to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}{..} => { /* alloc disabled: cannot emit */ },`;
          default:
            return `            Operation::${v} => { ${hdr} },`;
        }
      })
      .join("\n");

    const genArms = entries
      .map(([name, info]) => {
        const { id } = info;
        const v = pascal(name);
        const hdr = Oy(String(id));
        switch (info.args) {
          case "src":
            return `            Operation::${v}(a) => { ${hdr} for b in a.encode().to_le_bytes() { yield_! b; } },`;
          case "src_dest":
            return `            Operation::${v}{val, dest} => { ${hdr} for b in val.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },`;
          case "dest":
            return `            Operation::${v}(dest) => { ${hdr} for b in dest.to_le_bytes() { yield_! b; } },`;
          case "fn":
            return `            Operation::Fn{variant, closure_args, spanner, j, dest} => { ${hdr} for b in variant.encode().to_le_bytes() { yield_! b; } for b in closure_args.encode().to_le_bytes() { yield_! b; } for b in spanner.encode().to_le_bytes() { yield_! b; } for b in j.to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },`;
          case "lit32":
            return `            Operation::Lit32{dest, val} => { ${hdr} for b in dest.to_le_bytes() { yield_! b; } for b in val.to_le_bytes() { yield_! b; } },`;
          case "array":
            return `            #[cfg(feature = "alloc")]
            Operation::${v}(items, dest) => { ${hdr} for b in (items.len() as u32).to_le_bytes() { yield_! b; } for x in items { for b in x.encode().to_le_bytes() { yield_! b; } } for b in dest.to_le_bytes() { yield_! b; } },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}(..) => { /* alloc disabled: cannot emit */ },`;
          case "object":
            return `            #[cfg(feature = "alloc")]
            Operation::${v}{c, pairs, key} => { ${hdr} for b in c.encode().to_le_bytes() { yield_! b; } for (k, v) in pairs { for b in k.encode().to_le_bytes() { yield_! b; } for b in v.encode().to_le_bytes() { yield_! b; } } for b in key.encode().to_le_bytes() { yield_! b; } },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}{..} => { /* alloc disabled: cannot emit */ },`;
          case "call":
            return `            #[cfg(feature = "alloc")]
            Operation::${v}{fn_op, args, dest} => { ${hdr} for b in fn_op.encode().to_le_bytes() { yield_! b; } for b in (args.len() as u32).to_le_bytes() { yield_! b; } for x in args { for b in x.encode().to_le_bytes() { yield_! b; } } for b in dest.to_le_bytes() { yield_! b; } },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}{..} => { /* alloc disabled: cannot emit */ },`;
          default:
            return `            Operation::${v} => { ${hdr} },`;
        }
      })
      .join("\n");

    return `
/* This is GENERATED code by \`regen.ts\` */
use super::*;
// Note: alloc crate already imported in lib.rs, so we don't re-import it here
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use num_enum::{IntoPrimitive, TryFromPrimitive};

/// VM opcodes enum representing all possible operations
/// Each opcode has a unique numeric identifier matching the JavaScript implementation
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, IntoPrimitive, TryFromPrimitive)]
#[repr(u16)]
#[non_exhaustive]
pub enum Opcode {
${entries.map(([n,i])=>`    /// ${n} operation (id: ${i.id})
    ${pascal(n)}=${i.id}`).join(",\n")}
}
impl Opcode{
  pub const LEN: u16 = ${entries.length};
}

/// Rich operation type with operands automatically decoded from bytecode
/// 
/// This enum uses the two-variant operand approach where operands are automatically
/// decoded from raw u32 values into structured types:
/// - \`Operand\`: Handles LSB encoding for literal vs state reference distinction
/// - \`SignedOperand\`: Handles signed/unsigned encoding for numeric values
/// 
/// The parsing automatically handles the bitwise operations that were previously
/// done manually in JavaScript, providing a type-safe interface in Rust.
#[derive(Debug, Clone)]
pub enum Operation {
${enumVariants}
}

fn read_u16_le(buf: &[u8], off: usize) -> Option<(u16, usize)> {
    if off + 2 > buf.len() { return None; }
    let v = u16::from_le_bytes([buf[off], buf[off+1]]);
    Some((v, off+2))
}
fn read_u32_le(buf: &[u8], off: usize) -> Option<(u32, usize)> {
    if off + 4 > buf.len() { return None; }
    let v = u32::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3]]);
    Some((v, off+4))
}
fn read_i32_le(buf: &[u8], off: usize) -> Option<(i32, usize)> {
    if off + 4 > buf.len() { return None; }
    let v = i32::from_le_bytes([buf[off], buf[off+1], buf[off+2], buf[off+3]]);
    Some((v, off+4))
}

impl Operation {
  pub const LEN: u16 = ${entries.length};

  /// Parse an Operation from the start of ${'`'}buf${'`'}, returning the operation and the remaining slice.
  pub fn parse(buf: &[u8]) -> Option<(Operation, &[u8])> {
    let (op, mut off) = read_u16_le(buf, 0)?;
    match op as u16 {
${parseArms}
      _ => None
    }
  }
/// Emit returns an iterator over emitted bytes
  #[cfg(feature = "gen-blocks")]
  pub fn emit(&self) -> impl Iterator<Item=u8>{
    return crate::gen_block!{
    match self {
    ${genArms}
    }
    }    
  }
  /// Emit returns an iterator over emitted bytes; requires ${'`'}alloc${'`'} feature to allocate.
  #[cfg(all(feature = "alloc", not(feature = "gen-blocks")))]
  pub fn emit(&self) -> impl Iterator<Item=u8> {
    let mut wtr: Vec<u8> = Vec::new();
    match self {
${emitArms}
    }
    wtr.into_iter()
  }
}
`;
  })()
);

// ---------------------------------------------------------------------------
// Generate crates/jade-vm-core/src/dispatch.rs
//
// Two traits (State + Ops) + exec_op dispatch function.
// Covers all opcodes EXCEPT "src" (RET) and "src_dest" (AWAIT/YIELD/YIELDSTAR)
// which are handled at the interpreter loop level in jade-vm-wasm.
// ---------------------------------------------------------------------------
writeFileSync(
  `${__dirname}/../crates/jade-vm-core/src/dispatch.rs`,
  (() => {
    const entries = Object.entries(opcodes) as [string, any][];
    const pascal = (s: string) =>
      s.split("_").map((p) => p[0] + p.slice(1).toLowerCase()).join("");

    // Opcodes handled at loop level (not dispatched through exec_op)
    const loopLevel = new Set(["src", "src_dest"]);

    // ---- Ops trait method signatures ----------------------------------------
    // Each handler returns Self::Value (or Result<Self::Value, Self::Error>)
    // and takes NO dest parameter – exec_op calls State::set after the handler.
    const opsMethods = entries
      .filter(([, { args }]) => !loopLevel.has(args))
      .map(([name, { args }]) => {
        const mn = `op_${name.toLowerCase()}`;
        switch (args) {
          case "dest":
            return `    fn ${mn}(&self) -> Self::Value;`;
          case "lit32":
            return `    fn op_lit32(&self, val: u32) -> Self::Value;`;
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
      })
      .filter(Boolean)
      .join("\n");

    // ---- exec_op match arms -------------------------------------------------
    const execArms = entries
      .filter(([, { args }]) => !loopLevel.has(args))
      .map(([name, { args }]) => {
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
          case "fn":
            // exec_op flushes + snapshots parent state before calling op_fn, so
            // the handler does not need to touch state itself.
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
            // The key operand decides: Literal → assign, StateRef → defineProperties.
            // exec_op handles this split so op_litobj just builds and returns the object.
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
      })
      .filter(Boolean)
      .join("\n");

    const BT = "\x60"; // backtick character
    return `/* This is GENERATED code by ${BT}regen.ts${BT} */
use portal_solutions_jade_vm::{Operand, Operation};

/// State access for the Jade VM.
///
/// Provides slot read/write, cache flushing, and a snapshot of the underlying
/// state object needed by the FN opcode to capture the parent state in closures.
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
///
/// ${BT}Value${BT} must match the associated ${BT}State::Value${BT} for types that
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

    /// Apply property descriptors in ${BT}props${BT} to ${BT}target${BT} in place.
    /// Used by the LITOBJ opcode when its key operand is a state reference.
    fn define_properties(&self, target: &Self::Value, props: Self::Value);

    // Opcode handlers (one per non-loop opcode) ---------------------------
    // Each returns the computed value; exec_op writes it to the dest slot.
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

/// Dispatch a non-loop ${BT}Operation${BT} through the platform.
///
/// Resolves operands, calls the appropriate ${BT}Ops${BT} method, and writes the
/// result to the dest slot via ${BT}State::set${BT}.
/// Returns ${BT}Err${BT} only for unknown opcodes.
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
  })()
);


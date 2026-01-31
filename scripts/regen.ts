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
    const parameters = `[code,state,{ip:ip-2,globalThis,nt,tenant},...args]`;
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
/* This is GENERATED code by \`update.mjs\` */
import {type Tenant} from "./index.ts"
const {apply} = Reflect;
const {create,defineProperties} = Object;
const {fromCodePoint} = String;
type _globalThis = typeof globalThis;
${vmcode}`
);
writeFileSync(
  `${__dirname}/../crates/jade-vm/src/data.rs`,
  (() => {
    const entries = Object.entries(opcodes) as [string, any][];
    const pascal = (s: string) =>
      s.split("_").map((p) => p[0] + p.slice(1).toLowerCase()).join("");

    // Enhanced enum variants with proper operand types
    const enumVariants = entries
      .map(([name, info]) => {
        const v = pascal(name);
        const args = info.args;
        if (args === 0) return `    ${v},`;
        if (args === 1) return `    ${v}(crate::Operand),`;
        if (typeof args === "number")
          return `    ${v}([crate::Operand; ${args}]),`;
        if (args === "array")
          return `    #[cfg(feature = "alloc")]
    ${v}(Vec<crate::Operand>, crate::Operand),`;
        if (args === "object")
          return `    #[cfg(feature = "alloc")]
    ${v}{ c: crate::SignedOperand, pairs: Vec<(crate::Operand, crate::Operand)>, key: crate::Operand },`;
        return `    ${v},`;
      })
      .join("\n");

    // Enhanced parsing with automatic operand decoding
    const parseArms = entries
      .map(([name, info]) => {
        const id = info.id;
        const v = pascal(name);
        const args = info.args;
        if (args === 0)
          return `            ${id} => Some((Operation::${v}, &buf[off..])),`;
        if (args === 1)
          return `            ${id} => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::${v}(crate::Operand::decode(a)), &buf[off..])) },`;
        if (typeof args === "number")
          return `            ${id} => { let mut arr = [crate::Operand::decode(0); ${args}]; for i in 0..${args} { let (x,no) = read_u32_le(buf, off)?; off = no; arr[i] = crate::Operand::decode(x); } Some((Operation::${v}(arr), &buf[off..])) },`;
        if (args === "array")
          return `            #[cfg(feature = "alloc")]
            ${id} => { let (len,no) = read_u32_le(buf, off)?; off = no; let mut items = Vec::with_capacity(len as usize); for _ in 0..(len as usize) { let (x,no2) = read_u32_le(buf, off)?; off = no2; items.push(crate::Operand::decode(x)); } let (dest,no3) = read_u32_le(buf, off)?; off = no3; Some((Operation::${v}(items, crate::Operand::decode(dest)), &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            ${id} => { return None },`;
        if (args === "object")
          return `            #[cfg(feature = "alloc")]
            ${id} => { let (c,no) = read_i32_le(buf, off)?; off = no; let mut pairs = Vec::new(); let mut cnt = if c>=0 { c as usize } else { (-c) as usize }; while cnt>0 { let (k,no2) = read_u32_le(buf, off)?; off = no2; let (v,no3) = read_u32_le(buf, off)?; off = no3; pairs.push((crate::Operand::decode(k), crate::Operand::decode(v))); cnt-=1; } let (key,no4) = read_u32_le(buf, off)?; off = no4; Some((Operation::${v}{ c: crate::SignedOperand::decode(c as u32), pairs, key: crate::Operand::decode(key) }, &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            ${id} => { return None },`;
        return `            ${id} => Some((Operation::${v}, &buf[off..])),`;
      })
      .join("\n");

    // Enhanced emit arms with automatic operand encoding
    const emitArms = entries
      .map(([name, info]) => {
        const id = info.id;
        const v = pascal(name);
        const args = info.args;
        if (args === 0)
          return `            Operation::${v} => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); },`;
        if (args === 1)
          return `            Operation::${v}(a) => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); wtr.extend_from_slice(&a.encode().to_le_bytes()); },`;
        if (typeof args === "number")
          return `            Operation::${v}(arr) => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); for x in arr.iter() { wtr.extend_from_slice(&x.encode().to_le_bytes()); } },`;
        if (args === "array")
          return `            #[cfg(feature = "alloc")]
            Operation::${v}(items, dest) => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); wtr.extend_from_slice(&(items.len() as u32).to_le_bytes()); for x in items { wtr.extend_from_slice(&x.encode().to_le_bytes()); } wtr.extend_from_slice(&dest.encode().to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::${v} => { /* alloc disabled: cannot emit */ },`;
        if (args === "object")
          return `            #[cfg(feature = "alloc")]
            Operation::${v}{c, pairs, key} => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); wtr.extend_from_slice(&c.encode().to_le_bytes()); for (k, v) in pairs { wtr.extend_from_slice(&k.encode().to_le_bytes()); wtr.extend_from_slice(&v.encode().to_le_bytes()); } wtr.extend_from_slice(&key.encode().to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::${v} => { /* alloc disabled: cannot emit */ },`;
        return `            Operation::${v} => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); },`;
      })
      .join("\n");

    return `
/* This is GENERATED code by \`update.mjs\` */

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

  // Parse an Operation from the start of ${'`'}buf${'`'}, returning the operation and the remaining slice.
  pub fn parse(buf: &[u8]) -> Option<(Operation, &[u8])> {
    let (op, mut off) = read_u16_le(buf, 0)?;
    match op as u16 {
${parseArms}
      _ => None
    }
  }

  // Emit returns an iterator over emitted bytes; requires ${'`'}alloc${'`'} feature to allocate.
  #[cfg(feature = "alloc")]
  pub fn emit(&self) -> alloc::vec::IntoIter<u8> {
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


import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { writeFileSync } from "node:fs";

import { opcodes,handlers } from "../packages/jade-data/dist/index.js";

const __dirname = dirname(fileURLToPath(import.meta.url));



type VMOpt = { async?: boolean; gen?: boolean };

const vmcode = [{ async: true, gen: true }, { async: true }, { gen: true }, {}]
  .map((o: VMOpt) => {
    const ak = "async" in o;
    const gk = "gen" in o;
    const n = ({ ak: ak_ = ak, gk: gk_ = gk }: { ak?: boolean; gk?: boolean }) =>
      `runVirtualized${ak_ ? "A" : ""}${gk_ ? "G" : ""}`;
    const si = `[code,state,{ip:ip-2,globalThis,nt,tenant},...args]`;
    return `
export ${ak ? "async" : ""} function${gk ? "*" : ""} runVirtualized${
      ak ? "A" : ""
    }${
      gk ? "G" : ""
    }(code: () => DataView, state: {[a: number]: any},{ip=0,globalThis=(0,eval)('this'),nt=undefined,tenant}:{ip?:number,globalThis?: _globalThis,nt?: any,tenant:Tenant},...args: any[]): ${
      ak ? (gk ? `AsyncGenerator<any,any,any>` : `Promise<any>`) : `any`
    }{
    for(;;){
        const op = code().getUint16(ip,true);ip += 2;
        const arg = () => {
            const val = code().getUint32(ip,true);
            ip += 4;
            return val & 1 ? state[val >>> 1] : val >>> 1;
        }
        const val: any = (op === 0 || ${ak ? "op === 1" : "false"} || ${
      gk ? "op === 2 || op === 3 " : "false"
    }) ? arg() : undefined;
        switch(op){
            
            case ${opcodes.AWAIT.id}: ${
      ak
        ? `state[code().getUint32(ip,true)]=await val;ip += 4;break;`
        : `return apply(${n({
            ak: true,
          })},this,${si});`
    }
            case ${opcodes.YIELD.id}: ${
      gk
        ? `state[code().getUint32(ip,true)]=yield val;ip += 4;break;`
        : `return apply(${n({
            gk: true,
          })},this,${si});`
    }
            case ${opcodes.YIELDSTAR.id}: ${
      gk
        ? `state[code().getUint32(ip,true)]=yield* val;ip += 4;break;`
        : `return apply(${n({
            gk: true,
          })},this,${si});`
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

    const enumVariants = entries
      .map(([name, info]) => {
        const v = pascal(name);
        const args = info.args;
        if (args === 0) return `    ${v},`;
        if (args === 1) return `    ${v}(u32),`;
        if (typeof args === "number") return `    ${v}(Vec<u32>),`;
        if (args === "array") return `    ${v}(Vec<u32>, u32),`;
        if (args === "object")
          return `    ${v}{ c: i32, pairs: Vec<(u32,u32)>, key: u32 },`;
        return `    ${v},`;
      })
      .join("\n");

    const parseArms = entries
      .map(([name, info]) => {
        const id = info.id;
        const v = pascal(name);
        const args = info.args;
        if (args === 0)
          return `            ${id} => Some((Operation::${v}, &buf[off..])),`;
        if (args === 1)
          return `            ${id} => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::${v}(a), &buf[off..])) },`;
        if (typeof args === "number")
          return `            ${id} => { let mut vec = Vec::new(); for _ in 0..${args} { let (x,no) = read_u32_le(buf, off)?; off = no; vec.push(x); } Some((Operation::${v}(vec), &buf[off..])) },`;
        if (args === "array")
          return `            ${id} => { let (len,no) = read_u32_le(buf, off)?; off = no; let mut items = Vec::with_capacity(len as usize); for _ in 0..(len as usize) { let (x,no2) = read_u32_le(buf, off)?; off = no2; items.push(x); } let (dest,no3) = read_u32_le(buf, off)?; off = no3; Some((Operation::${v}(items, dest), &buf[off..])) },`;
        if (args === "object")
          return `            ${id} => { let (c,no) = read_i32_le(buf, off)?; off = no; let mut pairs = Vec::new(); let mut cnt = if c>=0 { c as usize } else { (-c) as usize }; while cnt>0 { let (k,no2) = read_u32_le(buf, off)?; off = no2; let (v,no3) = read_u32_le(buf, off)?; off = no3; pairs.push((k,v)); cnt-=1; } let (key,no4) = read_u32_le(buf, off)?; off = no4; Some((Operation::${v}{ c, pairs, key }, &buf[off..])) },`;
        return `            ${id} => Some((Operation::${v}, &buf[off..])),`;
      })
      .join("\n");

    const emitArms = entries
      .map(([name, info]) => {
        const id = info.id;
        const v = pascal(name);
        const args = info.args;
        if (args === 0)
          return `            Operation::${v} => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); },`;
        if (args === 1)
          return `            Operation::${v}(a) => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); wtr.extend_from_slice(&a.to_le_bytes()); },`;
        if (typeof args === "number")
          return `            Operation::${v}(vec) => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); for &x in vec { wtr.extend_from_slice(&x.to_le_bytes()); } },`;
        if (args === "array")
          return `            Operation::${v}(items,dest) => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); wtr.extend_from_slice(&(items.len() as u32).to_le_bytes()); for &x in items { wtr.extend_from_slice(&x.to_le_bytes()); } wtr.extend_from_slice(&dest.to_le_bytes()); },`;
        if (args === "object")
          return `            Operation::${v}{c,pairs,key} => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); wtr.extend_from_slice(&c.to_le_bytes()); for (k,v) in pairs { wtr.extend_from_slice(&k.to_le_bytes()); wtr.extend_from_slice(&v.to_le_bytes()); } wtr.extend_from_slice(&key.to_le_bytes()); },`;
        return `            Operation::${v} => { wtr.push(${id} as u8); wtr.push((${id}>>8) as u8); },`;
      })
      .join("\n");

    return `
/* This is GENERATED code by \`update.mjs\` */
use core::convert::TryFrom;
#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, IntoPrimitive, TryFromPrimitive)]
#[repr(u16)]
#[non_exhaustive]
pub enum Opcode {
${entries.map(([n,i])=>`${n}=${i.id}`).join(",\n    ")}
}
impl Opcode{
  pub const LEN: u16 = ${entries.length};
}

// Richer operation type with operands
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


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
          return `            ${id} => Some(Opcode::${v}),`;
        if (args === 1)
          return `            ${id} => { let a = rdr.read_u32::<LittleEndian>().ok()?; Some(Opcode::${v}(a)) },`;
        if (typeof args === "number")
          return `            ${id} => { let mut vec = Vec::new(); for _ in 0..${args} { vec.push(rdr.read_u32::<LittleEndian>().ok()?); } Some(Opcode::${v}(vec)) },`;
        if (args === "array")
          return `            ${id} => { let len = rdr.read_u32::<LittleEndian>().ok()? as usize; let mut items = Vec::with_capacity(len); for _ in 0..len { items.push(rdr.read_u32::<LittleEndian>().ok()?); } let dest = rdr.read_u32::<LittleEndian>().ok()?; Some(Opcode::${v}(items, dest)) },`;
        if (args === "object")
          return `            ${id} => { let c = rdr.read_i32::<LittleEndian>().ok()?; let mut pairs = Vec::new(); let mut cnt = if c>=0 { c as usize } else { (-c) as usize }; while cnt>0 { let k = rdr.read_u32::<LittleEndian>().ok()?; let v = rdr.read_u32::<LittleEndian>().ok()?; pairs.push((k,v)); cnt-=1; } let key = rdr.read_u32::<LittleEndian>().ok()?; Some(Opcode::${v}{ c, pairs, key }) },`;
        return `            ${id} => Some(Opcode::${v}),`;
      })
      .join("\n");

    const emitArms = entries
      .map(([name, info]) => {
        const id = info.id;
        const v = pascal(name);
        const args = info.args;
        if (args === 0)
          return `            Opcode::${v} => { wtr.write_u16::<LittleEndian>(${id}).unwrap(); },`;
        if (args === 1)
          return `            Opcode::${v}(a) => { wtr.write_u16::<LittleEndian>(${id}).unwrap(); wtr.write_u32::<LittleEndian>(*a).unwrap(); },`;
        if (typeof args === "number")
          return `            Opcode::${v}(vec) => { wtr.write_u16::<LittleEndian>(${id}).unwrap(); for &x in vec { wtr.write_u32::<LittleEndian>(x).unwrap(); } },`;
        if (args === "array")
          return `            Opcode::${v}(items,dest) => { wtr.write_u16::<LittleEndian>(${id}).unwrap(); wtr.write_u32::<LittleEndian>(items.len() as u32).unwrap(); for &x in items { wtr.write_u32::<LittleEndian>(x).unwrap(); } wtr.write_u32::<LittleEndian>(*dest).unwrap(); },`;
        if (args === "object")
          return `            Opcode::${v}{c,pairs,key} => { wtr.write_u16::<LittleEndian>(${id}).unwrap(); wtr.write_i32::<LittleEndian>(*c).unwrap(); for (k,v) in pairs { wtr.write_u32::<LittleEndian>(*k).unwrap(); wtr.write_u32::<LittleEndian>(*v).unwrap(); } wtr.write_u32::<LittleEndian>(*key).unwrap(); },`;
        return `            Opcode::${v} => { wtr.write_u16::<LittleEndian>(${id}).unwrap(); },`;
      })
      .join("\n");

    return `
/* This is GENERATED code by \`update.mjs\` */
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

#[derive(Debug, Clone)]
pub enum Opcode {
${enumVariants}
}

impl Opcode {
  pub const LEN: u16 = ${entries.length};

  pub fn parse(buf: &[u8]) -> Option<Opcode> {
    let mut rdr = Cursor::new(buf);
    let op = rdr.read_u16::<LittleEndian>().ok()?;
    match op as u16 {
${parseArms}
      _ => None
    }
  }

  pub fn emit(&self) -> Vec<u8> {
    let mut wtr = Vec::new();
    match self {
${emitArms}
    }
    wtr
  }
}
`;
  })()
);


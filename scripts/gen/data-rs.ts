import { pascal } from "./shared.ts";

// Emit two bytes (u16 LE) via Vec push
const O = (code: string) => `wtr.push(${code} as u8); wtr.push((${code}>>8) as u8);`;
// Emit two bytes via gen-block yield
const Oy = (code: string) => `yield_!(${code} as u8); yield_!((${code}>>8) as u8);`;

function enumVariant(name: string, info: any): string {
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
    case "bool":
      return `    ${v} { val: bool, dest: u32 },`;
    case "binop":
      return `    ${v} { a: crate::Operand, b: crate::Operand, dest: u32 },`;
    case "sel":
      return `    ${v} { cond: crate::Operand, then: crate::Operand, else_: crate::Operand, dest: u32 },`;
    case "member_get":
      return `    ${v} { obj: crate::Operand, key: crate::Operand, dest: u32 },`;
    case "member_set":
      return `    ${v} { obj: crate::Operand, key: crate::Operand, val: crate::Operand, dest: u32 },`;
    case "array":
      return `    #[cfg(feature = "alloc")]\n    ${v}(Vec<crate::Operand>, u32),`;
    case "object":
      return `    #[cfg(feature = "alloc")]\n    ${v}{ c: crate::SignedOperand, pairs: Vec<(crate::Operand, crate::Operand)>, key: crate::Operand },`;
    case "call":
      return `    #[cfg(feature = "alloc")]\n    ${v}{ fn_op: crate::Operand, args: Vec<crate::Operand>, dest: u32 },`;
    case "jmp":
      return `    ${v} { target: u32 },`;
    case "condjmp":
      return `    ${v} { cond: crate::Operand, if_true: u32, if_false: u32 },`;
    case "switch_jump":
      return `    #[cfg(feature = "alloc")]\n    ${v} { val: crate::Operand, cases: Vec<(u32, u32)>, default_target: u32 },`;
    default:
      return `    ${v},`;
  }
}

function parseArm(name: string, info: any): string {
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
    case "bool":
      return `            ${id} => { let (v,no)=read_u32_le(buf,off)?; off=no; let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ val: v != 0, dest }, &buf[off..])) },`;
    case "binop":
      return `            ${id} => { let (a,no)=read_u32_le(buf,off)?; off=no; let (b,no)=read_u32_le(buf,off)?; off=no; let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ a: crate::Operand::decode(a), b: crate::Operand::decode(b), dest }, &buf[off..])) },`;
    case "sel":
      return `            ${id} => { let (cond,no)=read_u32_le(buf,off)?; off=no; let (then,no)=read_u32_le(buf,off)?; off=no; let (else_r,no)=read_u32_le(buf,off)?; off=no; let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ cond: crate::Operand::decode(cond), then: crate::Operand::decode(then), else_: crate::Operand::decode(else_r), dest }, &buf[off..])) },`;
    case "member_get":
      return `            ${id} => { let (obj,no)=read_u32_le(buf,off)?; off=no; let (key,no)=read_u32_le(buf,off)?; off=no; let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ obj: crate::Operand::decode(obj), key: crate::Operand::decode(key), dest }, &buf[off..])) },`;
    case "member_set":
      return `            ${id} => { let (obj,no)=read_u32_le(buf,off)?; off=no; let (key,no)=read_u32_le(buf,off)?; off=no; let (val,no)=read_u32_le(buf,off)?; off=no; let (dest,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ obj: crate::Operand::decode(obj), key: crate::Operand::decode(key), val: crate::Operand::decode(val), dest }, &buf[off..])) },`;
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
    case "jmp":
      return `            ${id} => { let (target,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ target }, &buf[off..])) },`;
    case "condjmp":
      return `            ${id} => { let (cond_r,no)=read_u32_le(buf,off)?; off=no; let (if_true,no)=read_u32_le(buf,off)?; off=no; let (if_false,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ cond: crate::Operand::decode(cond_r), if_true, if_false }, &buf[off..])) },`;
    case "switch_jump":
      return `            #[cfg(feature = "alloc")]
            ${id} => { let (val_r,no)=read_u32_le(buf,off)?; off=no; let (n,no)=read_u32_le(buf,off)?; off=no; let mut cases=Vec::with_capacity(n as usize); for _ in 0..n { let (cv,no2)=read_u32_le(buf,off)?; off=no2; let (tgt,no3)=read_u32_le(buf,off)?; off=no3; cases.push((cv,tgt)); } let (default_target,no)=read_u32_le(buf,off)?; off=no; Some((Operation::${v}{ val: crate::Operand::decode(val_r), cases, default_target }, &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            ${id} => { return None },`;
    default:
      return `            ${id} => Some((Operation::${v}, &buf[off..])),`;
  }
}

function emitArm(name: string, info: any): string {
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
    case "bool":
      return `            Operation::${v}{val, dest} => { ${hdr} wtr.extend_from_slice(&(if *val { 1u32 } else { 0u32 }).to_le_bytes()); wtr.extend_from_slice(&dest.to_le_bytes()); },`;
    case "binop":
      return `            Operation::${v}{a, b, dest} => { ${hdr} wtr.extend_from_slice(&a.encode().to_le_bytes()); wtr.extend_from_slice(&b.encode().to_le_bytes()); wtr.extend_from_slice(&dest.to_le_bytes()); },`;
    case "sel":
      return `            Operation::${v}{cond, then, else_, dest} => { ${hdr} wtr.extend_from_slice(&cond.encode().to_le_bytes()); wtr.extend_from_slice(&then.encode().to_le_bytes()); wtr.extend_from_slice(&else_.encode().to_le_bytes()); wtr.extend_from_slice(&dest.to_le_bytes()); },`;
    case "member_get":
      return `            Operation::${v}{obj, key, dest} => { ${hdr} wtr.extend_from_slice(&obj.encode().to_le_bytes()); wtr.extend_from_slice(&key.encode().to_le_bytes()); wtr.extend_from_slice(&dest.to_le_bytes()); },`;
    case "member_set":
      return `            Operation::${v}{obj, key, val, dest} => { ${hdr} wtr.extend_from_slice(&obj.encode().to_le_bytes()); wtr.extend_from_slice(&key.encode().to_le_bytes()); wtr.extend_from_slice(&val.encode().to_le_bytes()); wtr.extend_from_slice(&dest.to_le_bytes()); },`;
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
    case "jmp":
      return `            Operation::${v}{target} => { ${hdr} wtr.extend_from_slice(&target.to_le_bytes()); },`;
    case "condjmp":
      return `            Operation::${v}{cond, if_true, if_false} => { ${hdr} wtr.extend_from_slice(&cond.encode().to_le_bytes()); wtr.extend_from_slice(&if_true.to_le_bytes()); wtr.extend_from_slice(&if_false.to_le_bytes()); },`;
    case "switch_jump":
      return `            #[cfg(feature = "alloc")]
            Operation::${v}{val, cases, default_target} => { ${hdr} wtr.extend_from_slice(&val.encode().to_le_bytes()); wtr.extend_from_slice(&(cases.len() as u32).to_le_bytes()); for (cv, tgt) in cases { wtr.extend_from_slice(&cv.to_le_bytes()); wtr.extend_from_slice(&tgt.to_le_bytes()); } wtr.extend_from_slice(&default_target.to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}{..} => { /* alloc disabled: cannot emit */ },`;
    default:
      return `            Operation::${v} => { ${hdr} },`;
  }
}

function genArm(name: string, info: any): string {
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
    case "bool":
      return `            Operation::${v}{val, dest} => { ${hdr} for b in (if val { 1u32 } else { 0u32 }).to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },`;
    case "binop":
      return `            Operation::${v}{a, b, dest} => { ${hdr} for b2 in a.encode().to_le_bytes() { yield_! b2; } for b2 in b.encode().to_le_bytes() { yield_! b2; } for b2 in dest.to_le_bytes() { yield_! b2; } },`;
    case "sel":
      return `            Operation::${v}{cond, then, else_, dest} => { ${hdr} for b in cond.encode().to_le_bytes() { yield_! b; } for b in then.encode().to_le_bytes() { yield_! b; } for b in else_.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },`;
    case "member_get":
      return `            Operation::${v}{obj, key, dest} => { ${hdr} for b in obj.encode().to_le_bytes() { yield_! b; } for b in key.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },`;
    case "member_set":
      return `            Operation::${v}{obj, key, val, dest} => { ${hdr} for b in obj.encode().to_le_bytes() { yield_! b; } for b in key.encode().to_le_bytes() { yield_! b; } for b in val.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },`;
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
    case "jmp":
      return `            Operation::${v}{target} => { ${hdr} for b in target.to_le_bytes() { yield_! b; } },`;
    case "condjmp":
      return `            Operation::${v}{cond, if_true, if_false} => { ${hdr} for b in cond.encode().to_le_bytes() { yield_! b; } for b in if_true.to_le_bytes() { yield_! b; } for b in if_false.to_le_bytes() { yield_! b; } },`;
    case "switch_jump":
      return `            #[cfg(feature = "alloc")]
            Operation::${v}{val, cases, default_target} => { ${hdr} for b in val.encode().to_le_bytes() { yield_! b; } for b in (cases.len() as u32).to_le_bytes() { yield_! b; } for (cv, tgt) in &cases { for b in cv.to_le_bytes() { yield_! b; } for b in tgt.to_le_bytes() { yield_! b; } } for b in default_target.to_le_bytes() { yield_! b; } },
            #[cfg(not(feature = "alloc"))]
            Operation::${v}{..} => { /* alloc disabled: cannot emit */ },`;
    default:
      return `            Operation::${v} => { ${hdr} },`;
  }
}

export function genDataRs(opcodes: Record<string, any>): string {
  const entries = Object.entries(opcodes) as [string, any][];

  const enumVariants = entries.map(([n, i]) => enumVariant(n, i)).join("\n");
  const parseArms = entries.map(([n, i]) => parseArm(n, i)).join("\n");
  const emitArms = entries.map(([n, i]) => emitArm(n, i)).join("\n");
  const genArms = entries.map(([n, i]) => genArm(n, i)).join("\n");

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
${entries.map(([n, i]) => `    /// ${n} operation (id: ${i.id})
    ${pascal(n)}=${i.id}`).join(",\n")}
}
impl Opcode{
  pub const LEN: u16 = ${entries.length};
}

/// Rich operation type with operands automatically decoded from bytecode
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

  /// Parse an Operation from the start of \`buf\`, returning the operation and the remaining slice.
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
  /// Emit returns an iterator over emitted bytes; requires \`alloc\` feature to allocate.
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
}


/* This is GENERATED code by `update.mjs` */
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
RET=0,
    AWAIT=1,
    YIELD=2,
    YIELDSTAR=3,
    GLOBAL=4,
    FN=5,
    LIT32=6,
    ARR=7,
    STR=8,
    LITOBJ=9,
    NEW_TARGET=10
}
impl Opcode{
  pub const LEN: u16 = 11;
}

// Richer operation type with operands
#[derive(Debug, Clone)]
pub enum Operation {
    Ret(u32),
    Await(u32),
    Yield(u32),
    Yieldstar(u32),
    Global,
    Fn(Vec<u32>),
    Lit32(u32),
    Arr(Vec<u32>, u32),
    Str(Vec<u32>, u32),
    Litobj{ c: i32, pairs: Vec<(u32,u32)>, key: u32 },
    NewTarget,
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
  pub const LEN: u16 = 11;

  // Parse an Operation from the start of `buf`, returning the operation and the remaining slice.
  pub fn parse(buf: &[u8]) -> Option<(Operation, &[u8])> {
    let (op, mut off) = read_u16_le(buf, 0)?;
    match op as u16 {
            0 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Ret(a), &buf[off..])) },
            1 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Await(a), &buf[off..])) },
            2 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Yield(a), &buf[off..])) },
            3 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Yieldstar(a), &buf[off..])) },
            4 => Some((Operation::Global, &buf[off..])),
            5 => { let mut vec = Vec::new(); for _ in 0..4 { let (x,no) = read_u32_le(buf, off)?; off = no; vec.push(x); } Some((Operation::Fn(vec), &buf[off..])) },
            6 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Lit32(a), &buf[off..])) },
            7 => { let (len,no) = read_u32_le(buf, off)?; off = no; let mut items = Vec::with_capacity(len as usize); for _ in 0..(len as usize) { let (x,no2) = read_u32_le(buf, off)?; off = no2; items.push(x); } let (dest,no3) = read_u32_le(buf, off)?; off = no3; Some((Operation::Arr(items, dest), &buf[off..])) },
            8 => { let (len,no) = read_u32_le(buf, off)?; off = no; let mut items = Vec::with_capacity(len as usize); for _ in 0..(len as usize) { let (x,no2) = read_u32_le(buf, off)?; off = no2; items.push(x); } let (dest,no3) = read_u32_le(buf, off)?; off = no3; Some((Operation::Str(items, dest), &buf[off..])) },
            9 => { let (c,no) = read_i32_le(buf, off)?; off = no; let mut pairs = Vec::new(); let mut cnt = if c>=0 { c as usize } else { (-c) as usize }; while cnt>0 { let (k,no2) = read_u32_le(buf, off)?; off = no2; let (v,no3) = read_u32_le(buf, off)?; off = no3; pairs.push((k,v)); cnt-=1; } let (key,no4) = read_u32_le(buf, off)?; off = no4; Some((Operation::Litobj{ c, pairs, key }, &buf[off..])) },
            10 => Some((Operation::NewTarget, &buf[off..])),
      _ => None
    }
  }

  // Emit returns an iterator over emitted bytes; requires `alloc` feature to allocate.
  #[cfg(feature = "alloc")]
  pub fn emit(&self) -> alloc::vec::IntoIter<u8> {
    let mut wtr: Vec<u8> = Vec::new();
    match self {
            Operation::Ret(a) => { wtr.push(0 as u8); wtr.push((0>>8) as u8); wtr.extend_from_slice(&a.to_le_bytes()); },
            Operation::Await(a) => { wtr.push(1 as u8); wtr.push((1>>8) as u8); wtr.extend_from_slice(&a.to_le_bytes()); },
            Operation::Yield(a) => { wtr.push(2 as u8); wtr.push((2>>8) as u8); wtr.extend_from_slice(&a.to_le_bytes()); },
            Operation::Yieldstar(a) => { wtr.push(3 as u8); wtr.push((3>>8) as u8); wtr.extend_from_slice(&a.to_le_bytes()); },
            Operation::Global => { wtr.push(4 as u8); wtr.push((4>>8) as u8); },
            Operation::Fn(vec) => { wtr.push(5 as u8); wtr.push((5>>8) as u8); for &x in vec { wtr.extend_from_slice(&x.to_le_bytes()); } },
            Operation::Lit32(a) => { wtr.push(6 as u8); wtr.push((6>>8) as u8); wtr.extend_from_slice(&a.to_le_bytes()); },
            Operation::Arr(items,dest) => { wtr.push(7 as u8); wtr.push((7>>8) as u8); wtr.extend_from_slice(&(items.len() as u32).to_le_bytes()); for &x in items { wtr.extend_from_slice(&x.to_le_bytes()); } wtr.extend_from_slice(&dest.to_le_bytes()); },
            Operation::Str(items,dest) => { wtr.push(8 as u8); wtr.push((8>>8) as u8); wtr.extend_from_slice(&(items.len() as u32).to_le_bytes()); for &x in items { wtr.extend_from_slice(&x.to_le_bytes()); } wtr.extend_from_slice(&dest.to_le_bytes()); },
            Operation::Litobj{c,pairs,key} => { wtr.push(9 as u8); wtr.push((9>>8) as u8); wtr.extend_from_slice(&c.to_le_bytes()); for (k,v) in pairs { wtr.extend_from_slice(&k.to_le_bytes()); wtr.extend_from_slice(&v.to_le_bytes()); } wtr.extend_from_slice(&key.to_le_bytes()); },
            Operation::NewTarget => { wtr.push(10 as u8); wtr.push((10>>8) as u8); },
    }
    wtr.into_iter()
  }
}

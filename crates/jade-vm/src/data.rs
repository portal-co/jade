
/* This is GENERATED code by `update.mjs` */

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
    /// RET operation (id: 0)
    Ret=0,
    /// AWAIT operation (id: 1)
    Await=1,
    /// YIELD operation (id: 2)
    Yield=2,
    /// YIELDSTAR operation (id: 3)
    Yieldstar=3,
    /// GLOBAL operation (id: 4)
    Global=4,
    /// FN operation (id: 5)
    Fn=5,
    /// LIT32 operation (id: 6)
    Lit32=6,
    /// ARR operation (id: 7)
    Arr=7,
    /// STR operation (id: 8)
    Str=8,
    /// LITOBJ operation (id: 9)
    Litobj=9,
    /// NEW_TARGET operation (id: 10)
    NewTarget=10
}
impl Opcode{
  pub const LEN: u16 = 11;
}

/// Rich operation type with operands automatically decoded from bytecode
/// 
/// This enum uses the two-variant operand approach where operands are automatically
/// decoded from raw u32 values into structured types:
/// - `Operand`: Handles LSB encoding for literal vs state reference distinction
/// - `SignedOperand`: Handles signed/unsigned encoding for numeric values
/// 
/// The parsing automatically handles the bitwise operations that were previously
/// done manually in JavaScript, providing a type-safe interface in Rust.
#[derive(Debug, Clone)]
pub enum Operation {
    Ret(crate::Operand),
    Await(crate::Operand),
    Yield(crate::Operand),
    Yieldstar(crate::Operand),
    Global,
    Fn([crate::Operand; 4]),
    Lit32(crate::Operand),
    #[cfg(feature = "alloc")]
    Arr(Vec<crate::Operand>, crate::Operand),
    #[cfg(feature = "alloc")]
    Str(Vec<crate::Operand>, crate::Operand),
    #[cfg(feature = "alloc")]
    Litobj{ c: crate::SignedOperand, pairs: Vec<(crate::Operand, crate::Operand)>, key: crate::Operand },
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
            0 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Ret(crate::Operand::decode(a)), &buf[off..])) },
            1 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Await(crate::Operand::decode(a)), &buf[off..])) },
            2 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Yield(crate::Operand::decode(a)), &buf[off..])) },
            3 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Yieldstar(crate::Operand::decode(a)), &buf[off..])) },
            4 => Some((Operation::Global, &buf[off..])),
            5 => { let mut arr = [crate::Operand::decode(0); 4]; for i in 0..4 { let (x,no) = read_u32_le(buf, off)?; off = no; arr[i] = crate::Operand::decode(x); } Some((Operation::Fn(arr), &buf[off..])) },
            6 => { let (a,no) = read_u32_le(buf, off)?; off = no; Some((Operation::Lit32(crate::Operand::decode(a)), &buf[off..])) },
            #[cfg(feature = "alloc")]
            7 => { let (len,no) = read_u32_le(buf, off)?; off = no; let mut items = Vec::with_capacity(len as usize); for _ in 0..(len as usize) { let (x,no2) = read_u32_le(buf, off)?; off = no2; items.push(crate::Operand::decode(x)); } let (dest,no3) = read_u32_le(buf, off)?; off = no3; Some((Operation::Arr(items, crate::Operand::decode(dest)), &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            7 => { return None },
            #[cfg(feature = "alloc")]
            8 => { let (len,no) = read_u32_le(buf, off)?; off = no; let mut items = Vec::with_capacity(len as usize); for _ in 0..(len as usize) { let (x,no2) = read_u32_le(buf, off)?; off = no2; items.push(crate::Operand::decode(x)); } let (dest,no3) = read_u32_le(buf, off)?; off = no3; Some((Operation::Str(items, crate::Operand::decode(dest)), &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            8 => { return None },
            #[cfg(feature = "alloc")]
            9 => { let (c,no) = read_i32_le(buf, off)?; off = no; let mut pairs = Vec::new(); let mut cnt = if c>=0 { c as usize } else { (-c) as usize }; while cnt>0 { let (k,no2) = read_u32_le(buf, off)?; off = no2; let (v,no3) = read_u32_le(buf, off)?; off = no3; pairs.push((crate::Operand::decode(k), crate::Operand::decode(v))); cnt-=1; } let (key,no4) = read_u32_le(buf, off)?; off = no4; Some((Operation::Litobj{ c: crate::SignedOperand::decode(c as u32), pairs, key: crate::Operand::decode(key) }, &buf[off..])) },
            #[cfg(not(feature = "alloc"))]
            9 => { return None },
            10 => Some((Operation::NewTarget, &buf[off..])),
      _ => None
    }
  }

  // Emit returns an iterator over emitted bytes; requires `alloc` feature to allocate.
  #[cfg(feature = "alloc")]
  pub fn emit(&self) -> alloc::vec::IntoIter<u8> {
    let mut wtr: Vec<u8> = Vec::new();
    match self {
            Operation::Ret(a) => { wtr.push(0 as u8); wtr.push((0>>8) as u8); wtr.extend_from_slice(&a.encode().to_le_bytes()); },
            Operation::Await(a) => { wtr.push(1 as u8); wtr.push((1>>8) as u8); wtr.extend_from_slice(&a.encode().to_le_bytes()); },
            Operation::Yield(a) => { wtr.push(2 as u8); wtr.push((2>>8) as u8); wtr.extend_from_slice(&a.encode().to_le_bytes()); },
            Operation::Yieldstar(a) => { wtr.push(3 as u8); wtr.push((3>>8) as u8); wtr.extend_from_slice(&a.encode().to_le_bytes()); },
            Operation::Global => { wtr.push(4 as u8); wtr.push((4>>8) as u8); },
            Operation::Fn(arr) => { wtr.push(5 as u8); wtr.push((5>>8) as u8); for x in arr.iter() { wtr.extend_from_slice(&x.encode().to_le_bytes()); } },
            Operation::Lit32(a) => { wtr.push(6 as u8); wtr.push((6>>8) as u8); wtr.extend_from_slice(&a.encode().to_le_bytes()); },
            #[cfg(feature = "alloc")]
            Operation::Arr(items, dest) => { wtr.push(7 as u8); wtr.push((7>>8) as u8); wtr.extend_from_slice(&(items.len() as u32).to_le_bytes()); for x in items { wtr.extend_from_slice(&x.encode().to_le_bytes()); } wtr.extend_from_slice(&dest.encode().to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::Arr => { /* alloc disabled: cannot emit */ },
            #[cfg(feature = "alloc")]
            Operation::Str(items, dest) => { wtr.push(8 as u8); wtr.push((8>>8) as u8); wtr.extend_from_slice(&(items.len() as u32).to_le_bytes()); for x in items { wtr.extend_from_slice(&x.encode().to_le_bytes()); } wtr.extend_from_slice(&dest.encode().to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::Str => { /* alloc disabled: cannot emit */ },
            #[cfg(feature = "alloc")]
            Operation::Litobj{c, pairs, key} => { wtr.push(9 as u8); wtr.push((9>>8) as u8); wtr.extend_from_slice(&c.encode().to_le_bytes()); for (k, v) in pairs { wtr.extend_from_slice(&k.encode().to_le_bytes()); wtr.extend_from_slice(&v.encode().to_le_bytes()); } wtr.extend_from_slice(&key.encode().to_le_bytes()); },
            #[cfg(not(feature = "alloc"))]
            Operation::Litobj => { /* alloc disabled: cannot emit */ },
            Operation::NewTarget => { wtr.push(10 as u8); wtr.push((10>>8) as u8); },
    }
    wtr.into_iter()
  }
}

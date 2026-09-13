/* This is GENERATED code by `regen.ts` */
use super::*;
// Note: alloc crate already imported in lib.rs, so we don't re-import it here
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use num_enum::{IntoPrimitive, TryFromPrimitive};

/// VM opcodes enum representing all possible operations
/// Each opcode has a unique numeric identifier matching the JavaScript implementation
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, IntoPrimitive, TryFromPrimitive,
)]
#[repr(u16)]
#[non_exhaustive]
pub enum Opcode {
    /// RET operation (id: 0)
    Ret = 0,
    /// AWAIT operation (id: 1)
    Await = 1,
    /// YIELD operation (id: 2)
    Yield = 2,
    /// YIELDSTAR operation (id: 3)
    Yieldstar = 3,
    /// GLOBAL operation (id: 4)
    Global = 4,
    /// FN operation (id: 5)
    Fn = 5,
    /// LIT32 operation (id: 6)
    Lit32 = 6,
    /// ARR operation (id: 7)
    Arr = 7,
    /// STR operation (id: 8)
    Str = 8,
    /// LITOBJ operation (id: 9)
    Litobj = 9,
    /// NEW_TARGET operation (id: 10)
    NewTarget = 10,
    /// CALL operation (id: 11)
    Call = 11,
    /// BOOL operation (id: 12)
    Bool = 12,
    /// EQ operation (id: 13)
    Eq = 13,
    /// NE operation (id: 14)
    Ne = 14,
    /// LT operation (id: 15)
    Lt = 15,
    /// LE operation (id: 16)
    Le = 16,
    /// GT operation (id: 17)
    Gt = 17,
    /// GE operation (id: 18)
    Ge = 18,
    /// SEL operation (id: 19)
    Sel = 19,
    /// JMP operation (id: 20)
    Jmp = 20,
    /// COND_JMP operation (id: 21)
    CondJmp = 21,
    /// SWITCH operation (id: 22)
    Switch = 22,
    /// GET operation (id: 23)
    Get = 23,
    /// SET operation (id: 24)
    Set = 24,
}
impl Opcode {
    pub const LEN: u16 = 25;
}

/// Rich operation type with operands automatically decoded from bytecode
#[derive(Debug, Clone)]
pub enum Operation {
    Ret(crate::Operand),
    Await {
        val: crate::Operand,
        dest: u32,
    },
    Yield {
        val: crate::Operand,
        dest: u32,
    },
    Yieldstar {
        val: crate::Operand,
        dest: u32,
    },
    Global(u32),
    Fn {
        variant: crate::Operand,
        closure_args: crate::Operand,
        spanner: crate::Operand,
        params: crate::Operand,
        j: u32,
        dest: u32,
    },
    Lit32 {
        dest: u32,
        val: u32,
    },
    #[cfg(feature = "alloc")]
    Arr(Vec<crate::Operand>, u32),
    #[cfg(feature = "alloc")]
    Str(Vec<crate::Operand>, u32),
    #[cfg(feature = "alloc")]
    Litobj {
        c: crate::SignedOperand,
        pairs: Vec<(crate::Operand, crate::Operand)>,
        key: crate::Operand,
    },
    NewTarget(u32),
    #[cfg(feature = "alloc")]
    Call {
        fn_op: crate::Operand,
        this_op: crate::Operand,
        args: Vec<crate::Operand>,
        dest: u32,
    },
    Bool {
        val: bool,
        dest: u32,
    },
    Eq {
        a: crate::Operand,
        b: crate::Operand,
        dest: u32,
    },
    Ne {
        a: crate::Operand,
        b: crate::Operand,
        dest: u32,
    },
    Lt {
        a: crate::Operand,
        b: crate::Operand,
        dest: u32,
    },
    Le {
        a: crate::Operand,
        b: crate::Operand,
        dest: u32,
    },
    Gt {
        a: crate::Operand,
        b: crate::Operand,
        dest: u32,
    },
    Ge {
        a: crate::Operand,
        b: crate::Operand,
        dest: u32,
    },
    Sel {
        cond: crate::Operand,
        then: crate::Operand,
        else_: crate::Operand,
        dest: u32,
    },
    Jmp {
        target: u32,
    },
    CondJmp {
        cond: crate::Operand,
        if_true: u32,
        if_false: u32,
    },
    #[cfg(feature = "alloc")]
    Switch {
        val: crate::Operand,
        cases: Vec<(u32, u32)>,
        default_target: u32,
    },
    Get {
        obj: crate::Operand,
        key: crate::Operand,
        dest: u32,
    },
    Set {
        obj: crate::Operand,
        key: crate::Operand,
        val: crate::Operand,
        dest: u32,
    },
}

fn read_u16_le(buf: &[u8], off: usize) -> Option<(u16, usize)> {
    if off + 2 > buf.len() {
        return None;
    }
    let v = u16::from_le_bytes([buf[off], buf[off + 1]]);
    Some((v, off + 2))
}
fn read_u32_le(buf: &[u8], off: usize) -> Option<(u32, usize)> {
    if off + 4 > buf.len() {
        return None;
    }
    let v = u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
    Some((v, off + 4))
}
fn read_i32_le(buf: &[u8], off: usize) -> Option<(i32, usize)> {
    if off + 4 > buf.len() {
        return None;
    }
    let v = i32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]);
    Some((v, off + 4))
}

impl Operation {
    pub const LEN: u16 = 25;

    /// Parse an Operation from the start of `buf`, returning the operation and the remaining slice.
    pub fn parse(buf: &[u8]) -> Option<(Operation, &[u8])> {
        let (op, mut off) = read_u16_le(buf, 0)?;
        match op as u16 {
            0 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::Ret(crate::Operand::decode(a)), &buf[off..]))
            }
            1 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Await {
                        val: crate::Operand::decode(a),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            2 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Yield {
                        val: crate::Operand::decode(a),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            3 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Yieldstar {
                        val: crate::Operand::decode(a),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            4 => {
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::Global(dest), &buf[off..]))
            }
            5 => {
                let (var_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (clos_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (span_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (par_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (j, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Fn {
                        variant: crate::Operand::decode(var_r),
                        closure_args: crate::Operand::decode(clos_r),
                        spanner: crate::Operand::decode(span_r),
                        params: crate::Operand::decode(par_r),
                        j,
                        dest,
                    },
                    &buf[off..],
                ))
            }
            6 => {
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                let (val, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::Lit32 { dest, val }, &buf[off..]))
            }
            #[cfg(feature = "alloc")]
            7 => {
                let (len, no) = read_u32_le(buf, off)?;
                off = no;
                let mut items = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let (x, no2) = read_u32_le(buf, off)?;
                    off = no2;
                    items.push(crate::Operand::decode(x));
                }
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::Arr(items, dest), &buf[off..]))
            }
            #[cfg(not(feature = "alloc"))]
            7 => return None,
            #[cfg(feature = "alloc")]
            8 => {
                let (len, no) = read_u32_le(buf, off)?;
                off = no;
                let mut items = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let (x, no2) = read_u32_le(buf, off)?;
                    off = no2;
                    items.push(crate::Operand::decode(x));
                }
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::Str(items, dest), &buf[off..]))
            }
            #[cfg(not(feature = "alloc"))]
            8 => return None,
            #[cfg(feature = "alloc")]
            9 => {
                let (c, no) = read_i32_le(buf, off)?;
                off = no;
                let mut pairs = Vec::new();
                let mut cnt = if c >= 0 { c as usize } else { (-c) as usize };
                if c < 0 {
                    let (sp, no2) = read_u32_le(buf, off)?;
                    off = no2;
                    pairs.push((crate::Operand::decode(sp), crate::Operand::decode(0)));
                }
                while cnt > 0 {
                    let (k, no2) = read_u32_le(buf, off)?;
                    off = no2;
                    let (v, no3) = read_u32_le(buf, off)?;
                    off = no3;
                    pairs.push((crate::Operand::decode(k), crate::Operand::decode(v)));
                    cnt -= 1;
                }
                let (key, no4) = read_u32_le(buf, off)?;
                off = no4;
                Some((
                    Operation::Litobj {
                        c: crate::SignedOperand::decode(c as u32),
                        pairs,
                        key: crate::Operand::decode(key),
                    },
                    &buf[off..],
                ))
            }
            #[cfg(not(feature = "alloc"))]
            9 => return None,
            10 => {
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::NewTarget(dest), &buf[off..]))
            }
            #[cfg(feature = "alloc")]
            11 => {
                let (fn_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (this_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (len, no) = read_u32_le(buf, off)?;
                off = no;
                let mut args = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    let (x, no2) = read_u32_le(buf, off)?;
                    off = no2;
                    args.push(crate::Operand::decode(x));
                }
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Call {
                        fn_op: crate::Operand::decode(fn_r),
                        this_op: crate::Operand::decode(this_r),
                        args,
                        dest,
                    },
                    &buf[off..],
                ))
            }
            #[cfg(not(feature = "alloc"))]
            11 => return None,
            12 => {
                let (v, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::Bool { val: v != 0, dest }, &buf[off..]))
            }
            13 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (b, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Eq {
                        a: crate::Operand::decode(a),
                        b: crate::Operand::decode(b),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            14 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (b, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Ne {
                        a: crate::Operand::decode(a),
                        b: crate::Operand::decode(b),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            15 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (b, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Lt {
                        a: crate::Operand::decode(a),
                        b: crate::Operand::decode(b),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            16 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (b, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Le {
                        a: crate::Operand::decode(a),
                        b: crate::Operand::decode(b),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            17 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (b, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Gt {
                        a: crate::Operand::decode(a),
                        b: crate::Operand::decode(b),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            18 => {
                let (a, no) = read_u32_le(buf, off)?;
                off = no;
                let (b, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Ge {
                        a: crate::Operand::decode(a),
                        b: crate::Operand::decode(b),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            19 => {
                let (cond, no) = read_u32_le(buf, off)?;
                off = no;
                let (then, no) = read_u32_le(buf, off)?;
                off = no;
                let (else_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Sel {
                        cond: crate::Operand::decode(cond),
                        then: crate::Operand::decode(then),
                        else_: crate::Operand::decode(else_r),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            20 => {
                let (target, no) = read_u32_le(buf, off)?;
                off = no;
                Some((Operation::Jmp { target }, &buf[off..]))
            }
            21 => {
                let (cond_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (if_true, no) = read_u32_le(buf, off)?;
                off = no;
                let (if_false, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::CondJmp {
                        cond: crate::Operand::decode(cond_r),
                        if_true,
                        if_false,
                    },
                    &buf[off..],
                ))
            }
            #[cfg(feature = "alloc")]
            22 => {
                let (val_r, no) = read_u32_le(buf, off)?;
                off = no;
                let (n, no) = read_u32_le(buf, off)?;
                off = no;
                let mut cases = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let (cv, no2) = read_u32_le(buf, off)?;
                    off = no2;
                    let (tgt, no3) = read_u32_le(buf, off)?;
                    off = no3;
                    cases.push((cv, tgt));
                }
                let (default_target, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Switch {
                        val: crate::Operand::decode(val_r),
                        cases,
                        default_target,
                    },
                    &buf[off..],
                ))
            }
            #[cfg(not(feature = "alloc"))]
            22 => return None,
            23 => {
                let (obj, no) = read_u32_le(buf, off)?;
                off = no;
                let (key, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Get {
                        obj: crate::Operand::decode(obj),
                        key: crate::Operand::decode(key),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            24 => {
                let (obj, no) = read_u32_le(buf, off)?;
                off = no;
                let (key, no) = read_u32_le(buf, off)?;
                off = no;
                let (val, no) = read_u32_le(buf, off)?;
                off = no;
                let (dest, no) = read_u32_le(buf, off)?;
                off = no;
                Some((
                    Operation::Set {
                        obj: crate::Operand::decode(obj),
                        key: crate::Operand::decode(key),
                        val: crate::Operand::decode(val),
                        dest,
                    },
                    &buf[off..],
                ))
            }
            _ => None,
        }
    }
    /// Emit returns an iterator over emitted bytes
    #[cfg(feature = "gen-blocks")]
    pub fn emit(&self) -> impl Iterator<Item = u8> {
        return crate::gen_block! {
        match self {
                    Operation::Ret(a) => { yield_!(0 as u8); yield_!((0>>8) as u8); for b in a.encode().to_le_bytes() { yield_! b; } },
                Operation::Await{val, dest} => { yield_!(1 as u8); yield_!((1>>8) as u8); for b in val.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Yield{val, dest} => { yield_!(2 as u8); yield_!((2>>8) as u8); for b in val.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Yieldstar{val, dest} => { yield_!(3 as u8); yield_!((3>>8) as u8); for b in val.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Global(dest) => { yield_!(4 as u8); yield_!((4>>8) as u8); for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Fn{variant, closure_args, spanner, params, j, dest} => { yield_!(5 as u8); yield_!((5>>8) as u8); for b in variant.encode().to_le_bytes() { yield_! b; } for b in closure_args.encode().to_le_bytes() { yield_! b; } for b in spanner.encode().to_le_bytes() { yield_! b; } for b in params.encode().to_le_bytes() { yield_! b; } for b in j.to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Lit32{dest, val} => { yield_!(6 as u8); yield_!((6>>8) as u8); for b in dest.to_le_bytes() { yield_! b; } for b in val.to_le_bytes() { yield_! b; } },
                #[cfg(feature = "alloc")]
                Operation::Arr(items, dest) => { yield_!(7 as u8); yield_!((7>>8) as u8); for b in (items.len() as u32).to_le_bytes() { yield_! b; } for x in items { for b in x.encode().to_le_bytes() { yield_! b; } } for b in dest.to_le_bytes() { yield_! b; } },
                #[cfg(not(feature = "alloc"))]
                Operation::Arr(..) => { /* alloc disabled: cannot emit */ },
                #[cfg(feature = "alloc")]
                Operation::Str(items, dest) => { yield_!(8 as u8); yield_!((8>>8) as u8); for b in (items.len() as u32).to_le_bytes() { yield_! b; } for x in items { for b in x.encode().to_le_bytes() { yield_! b; } } for b in dest.to_le_bytes() { yield_! b; } },
                #[cfg(not(feature = "alloc"))]
                Operation::Str(..) => { /* alloc disabled: cannot emit */ },
                #[cfg(feature = "alloc")]
                Operation::Litobj{c, pairs, key} => { yield_!(9 as u8); yield_!((9>>8) as u8); for b in c.encode().to_le_bytes() { yield_! b; } for (k, v) in pairs { for b in k.encode().to_le_bytes() { yield_! b; } for b in v.encode().to_le_bytes() { yield_! b; } } for b in key.encode().to_le_bytes() { yield_! b; } },
                #[cfg(not(feature = "alloc"))]
                Operation::Litobj{..} => { /* alloc disabled: cannot emit */ },
                Operation::NewTarget(dest) => { yield_!(10 as u8); yield_!((10>>8) as u8); for b in dest.to_le_bytes() { yield_! b; } },
                #[cfg(feature = "alloc")]
                Operation::Call{fn_op, this_op, args, dest} => { yield_!(11 as u8); yield_!((11>>8) as u8); for b in fn_op.encode().to_le_bytes() { yield_! b; } for b in this_op.encode().to_le_bytes() { yield_! b; } for b in (args.len() as u32).to_le_bytes() { yield_! b; } for x in args { for b in x.encode().to_le_bytes() { yield_! b; } } for b in dest.to_le_bytes() { yield_! b; } },
                #[cfg(not(feature = "alloc"))]
                Operation::Call{..} => { /* alloc disabled: cannot emit */ },
                Operation::Bool{val, dest} => { yield_!(12 as u8); yield_!((12>>8) as u8); for b in (if val { 1u32 } else { 0u32 }).to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Eq{a, b, dest} => { yield_!(13 as u8); yield_!((13>>8) as u8); for b2 in a.encode().to_le_bytes() { yield_! b2; } for b2 in b.encode().to_le_bytes() { yield_! b2; } for b2 in dest.to_le_bytes() { yield_! b2; } },
                Operation::Ne{a, b, dest} => { yield_!(14 as u8); yield_!((14>>8) as u8); for b2 in a.encode().to_le_bytes() { yield_! b2; } for b2 in b.encode().to_le_bytes() { yield_! b2; } for b2 in dest.to_le_bytes() { yield_! b2; } },
                Operation::Lt{a, b, dest} => { yield_!(15 as u8); yield_!((15>>8) as u8); for b2 in a.encode().to_le_bytes() { yield_! b2; } for b2 in b.encode().to_le_bytes() { yield_! b2; } for b2 in dest.to_le_bytes() { yield_! b2; } },
                Operation::Le{a, b, dest} => { yield_!(16 as u8); yield_!((16>>8) as u8); for b2 in a.encode().to_le_bytes() { yield_! b2; } for b2 in b.encode().to_le_bytes() { yield_! b2; } for b2 in dest.to_le_bytes() { yield_! b2; } },
                Operation::Gt{a, b, dest} => { yield_!(17 as u8); yield_!((17>>8) as u8); for b2 in a.encode().to_le_bytes() { yield_! b2; } for b2 in b.encode().to_le_bytes() { yield_! b2; } for b2 in dest.to_le_bytes() { yield_! b2; } },
                Operation::Ge{a, b, dest} => { yield_!(18 as u8); yield_!((18>>8) as u8); for b2 in a.encode().to_le_bytes() { yield_! b2; } for b2 in b.encode().to_le_bytes() { yield_! b2; } for b2 in dest.to_le_bytes() { yield_! b2; } },
                Operation::Sel{cond, then, else_, dest} => { yield_!(19 as u8); yield_!((19>>8) as u8); for b in cond.encode().to_le_bytes() { yield_! b; } for b in then.encode().to_le_bytes() { yield_! b; } for b in else_.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Jmp{target} => { yield_!(20 as u8); yield_!((20>>8) as u8); for b in target.to_le_bytes() { yield_! b; } },
                Operation::CondJmp{cond, if_true, if_false} => { yield_!(21 as u8); yield_!((21>>8) as u8); for b in cond.encode().to_le_bytes() { yield_! b; } for b in if_true.to_le_bytes() { yield_! b; } for b in if_false.to_le_bytes() { yield_! b; } },
                #[cfg(feature = "alloc")]
                Operation::Switch{val, cases, default_target} => { yield_!(22 as u8); yield_!((22>>8) as u8); for b in val.encode().to_le_bytes() { yield_! b; } for b in (cases.len() as u32).to_le_bytes() { yield_! b; } for (cv, tgt) in &cases { for b in cv.to_le_bytes() { yield_! b; } for b in tgt.to_le_bytes() { yield_! b; } } for b in default_target.to_le_bytes() { yield_! b; } },
                #[cfg(not(feature = "alloc"))]
                Operation::Switch{..} => { /* alloc disabled: cannot emit */ },
                Operation::Get{obj, key, dest} => { yield_!(23 as u8); yield_!((23>>8) as u8); for b in obj.encode().to_le_bytes() { yield_! b; } for b in key.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
                Operation::Set{obj, key, val, dest} => { yield_!(24 as u8); yield_!((24>>8) as u8); for b in obj.encode().to_le_bytes() { yield_! b; } for b in key.encode().to_le_bytes() { yield_! b; } for b in val.encode().to_le_bytes() { yield_! b; } for b in dest.to_le_bytes() { yield_! b; } },
        }
        };
    }
    /// Emit returns an iterator over emitted bytes; requires `alloc` feature to allocate.
    #[cfg(all(feature = "alloc", not(feature = "gen-blocks")))]
    pub fn emit(&self) -> impl Iterator<Item = u8> {
        let mut wtr: Vec<u8> = Vec::new();
        match self {
            Operation::Ret(a) => {
                wtr.push(0 as u8);
                wtr.push((0 >> 8) as u8);
                wtr.extend_from_slice(&a.encode().to_le_bytes());
            }
            Operation::Await { val, dest } => {
                wtr.push(1 as u8);
                wtr.push((1 >> 8) as u8);
                wtr.extend_from_slice(&val.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Yield { val, dest } => {
                wtr.push(2 as u8);
                wtr.push((2 >> 8) as u8);
                wtr.extend_from_slice(&val.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Yieldstar { val, dest } => {
                wtr.push(3 as u8);
                wtr.push((3 >> 8) as u8);
                wtr.extend_from_slice(&val.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Global(dest) => {
                wtr.push(4 as u8);
                wtr.push((4 >> 8) as u8);
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Fn {
                variant,
                closure_args,
                spanner,
                params,
                j,
                dest,
            } => {
                wtr.push(5 as u8);
                wtr.push((5 >> 8) as u8);
                wtr.extend_from_slice(&variant.encode().to_le_bytes());
                wtr.extend_from_slice(&closure_args.encode().to_le_bytes());
                wtr.extend_from_slice(&spanner.encode().to_le_bytes());
                wtr.extend_from_slice(&params.encode().to_le_bytes());
                wtr.extend_from_slice(&j.to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Lit32 { dest, val } => {
                wtr.push(6 as u8);
                wtr.push((6 >> 8) as u8);
                wtr.extend_from_slice(&dest.to_le_bytes());
                wtr.extend_from_slice(&val.to_le_bytes());
            }
            #[cfg(feature = "alloc")]
            Operation::Arr(items, dest) => {
                wtr.push(7 as u8);
                wtr.push((7 >> 8) as u8);
                wtr.extend_from_slice(&(items.len() as u32).to_le_bytes());
                for x in items {
                    wtr.extend_from_slice(&x.encode().to_le_bytes());
                }
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            #[cfg(not(feature = "alloc"))]
            Operation::Arr(..) => { /* alloc disabled: cannot emit */ }
            #[cfg(feature = "alloc")]
            Operation::Str(items, dest) => {
                wtr.push(8 as u8);
                wtr.push((8 >> 8) as u8);
                wtr.extend_from_slice(&(items.len() as u32).to_le_bytes());
                for x in items {
                    wtr.extend_from_slice(&x.encode().to_le_bytes());
                }
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            #[cfg(not(feature = "alloc"))]
            Operation::Str(..) => { /* alloc disabled: cannot emit */ }
            #[cfg(feature = "alloc")]
            Operation::Litobj { c, pairs, key } => {
                wtr.push(9 as u8);
                wtr.push((9 >> 8) as u8);
                wtr.extend_from_slice(&c.encode().to_le_bytes());
                for (k, v) in pairs {
                    wtr.extend_from_slice(&k.encode().to_le_bytes());
                    wtr.extend_from_slice(&v.encode().to_le_bytes());
                }
                wtr.extend_from_slice(&key.encode().to_le_bytes());
            }
            #[cfg(not(feature = "alloc"))]
            Operation::Litobj { .. } => { /* alloc disabled: cannot emit */ }
            Operation::NewTarget(dest) => {
                wtr.push(10 as u8);
                wtr.push((10 >> 8) as u8);
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            #[cfg(feature = "alloc")]
            Operation::Call {
                fn_op,
                this_op,
                args,
                dest,
            } => {
                wtr.push(11 as u8);
                wtr.push((11 >> 8) as u8);
                wtr.extend_from_slice(&fn_op.encode().to_le_bytes());
                wtr.extend_from_slice(&this_op.encode().to_le_bytes());
                wtr.extend_from_slice(&(args.len() as u32).to_le_bytes());
                for x in args {
                    wtr.extend_from_slice(&x.encode().to_le_bytes());
                }
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            #[cfg(not(feature = "alloc"))]
            Operation::Call { .. } => { /* alloc disabled: cannot emit */ }
            Operation::Bool { val, dest } => {
                wtr.push(12 as u8);
                wtr.push((12 >> 8) as u8);
                wtr.extend_from_slice(&(if *val { 1u32 } else { 0u32 }).to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Eq { a, b, dest } => {
                wtr.push(13 as u8);
                wtr.push((13 >> 8) as u8);
                wtr.extend_from_slice(&a.encode().to_le_bytes());
                wtr.extend_from_slice(&b.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Ne { a, b, dest } => {
                wtr.push(14 as u8);
                wtr.push((14 >> 8) as u8);
                wtr.extend_from_slice(&a.encode().to_le_bytes());
                wtr.extend_from_slice(&b.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Lt { a, b, dest } => {
                wtr.push(15 as u8);
                wtr.push((15 >> 8) as u8);
                wtr.extend_from_slice(&a.encode().to_le_bytes());
                wtr.extend_from_slice(&b.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Le { a, b, dest } => {
                wtr.push(16 as u8);
                wtr.push((16 >> 8) as u8);
                wtr.extend_from_slice(&a.encode().to_le_bytes());
                wtr.extend_from_slice(&b.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Gt { a, b, dest } => {
                wtr.push(17 as u8);
                wtr.push((17 >> 8) as u8);
                wtr.extend_from_slice(&a.encode().to_le_bytes());
                wtr.extend_from_slice(&b.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Ge { a, b, dest } => {
                wtr.push(18 as u8);
                wtr.push((18 >> 8) as u8);
                wtr.extend_from_slice(&a.encode().to_le_bytes());
                wtr.extend_from_slice(&b.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Sel {
                cond,
                then,
                else_,
                dest,
            } => {
                wtr.push(19 as u8);
                wtr.push((19 >> 8) as u8);
                wtr.extend_from_slice(&cond.encode().to_le_bytes());
                wtr.extend_from_slice(&then.encode().to_le_bytes());
                wtr.extend_from_slice(&else_.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Jmp { target } => {
                wtr.push(20 as u8);
                wtr.push((20 >> 8) as u8);
                wtr.extend_from_slice(&target.to_le_bytes());
            }
            Operation::CondJmp {
                cond,
                if_true,
                if_false,
            } => {
                wtr.push(21 as u8);
                wtr.push((21 >> 8) as u8);
                wtr.extend_from_slice(&cond.encode().to_le_bytes());
                wtr.extend_from_slice(&if_true.to_le_bytes());
                wtr.extend_from_slice(&if_false.to_le_bytes());
            }
            #[cfg(feature = "alloc")]
            Operation::Switch {
                val,
                cases,
                default_target,
            } => {
                wtr.push(22 as u8);
                wtr.push((22 >> 8) as u8);
                wtr.extend_from_slice(&val.encode().to_le_bytes());
                wtr.extend_from_slice(&(cases.len() as u32).to_le_bytes());
                for (cv, tgt) in cases {
                    wtr.extend_from_slice(&cv.to_le_bytes());
                    wtr.extend_from_slice(&tgt.to_le_bytes());
                }
                wtr.extend_from_slice(&default_target.to_le_bytes());
            }
            #[cfg(not(feature = "alloc"))]
            Operation::Switch { .. } => { /* alloc disabled: cannot emit */ }
            Operation::Get { obj, key, dest } => {
                wtr.push(23 as u8);
                wtr.push((23 >> 8) as u8);
                wtr.extend_from_slice(&obj.encode().to_le_bytes());
                wtr.extend_from_slice(&key.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
            Operation::Set {
                obj,
                key,
                val,
                dest,
            } => {
                wtr.push(24 as u8);
                wtr.push((24 >> 8) as u8);
                wtr.extend_from_slice(&obj.encode().to_le_bytes());
                wtr.extend_from_slice(&key.encode().to_le_bytes());
                wtr.extend_from_slice(&val.encode().to_le_bytes());
                wtr.extend_from_slice(&dest.to_le_bytes());
            }
        }
        wtr.into_iter()
    }
}

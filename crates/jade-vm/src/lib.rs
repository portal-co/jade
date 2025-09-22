use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, IntoPrimitive, TryFromPrimitive,
)]
#[repr(u16)]
#[non_exhaustive]
pub enum Opcode {
    RET = 0,
    AWAIT = 1,
    YEILD = 2,
    YIELDSTAR = 3,
    GLOBAL = 4,
    FN = 5,
    LIT32 = 6,
    ARR = 7,
    STR = 8,
    LITOBJ = 9,
}
pub fn encode_arg(a: u32, lit: bool) -> u32 {
    match lit {
        true => a << 1,
        false => (a << 1) | 1,
    }
}
pub fn encode_extension(a: u32, extended: bool) -> u32 {
    match extended {
        false => a,
        true => 0u32.wrapping_sub(a),
    }
}

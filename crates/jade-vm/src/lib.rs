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
    LITSTR = 8,
}

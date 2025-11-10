/* This is GENERATED code by `update.mjs` */
#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, IntoPrimitive, TryFromPrimitive,
)]
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

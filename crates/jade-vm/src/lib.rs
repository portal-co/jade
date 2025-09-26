#![no_std]
use num_enum::{IntoPrimitive, TryFromPrimitive};

include!("./data.rs");

pub fn encode_lsb(a: u32, val: bool) -> u32 {
    match val {
        true => a << 1,
        false => (a << 1) | 1,
    }
}
pub fn encode_signed(a: u32, val: bool) -> u32 {
    match val {
        false => a,
        true => 0u32.wrapping_sub(a),
    }
}

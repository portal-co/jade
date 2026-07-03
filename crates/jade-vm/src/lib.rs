#![no_std]
#![cfg_attr(feature = "gen-blocks", feature(gen_blocks))]
#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "gen-blocks")]
#[macro_export]
#[doc(hidden)]
macro_rules! gen_block {
    ($($a:expr)*) => {
        gen move{
            $($a)*
        }
    };
}

#[cfg(feature = "gen-blocks")]
#[macro_export]
#[doc(hidden)]
macro_rules! yield_ {
    ($a:expr) => {
        yield $a
    };
}

pub mod data;
pub use data::{Opcode, Operation};

/// Represents an operand that can be either a literal value or a state variable reference
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    /// A literal value
    Literal(u32),
    /// A reference to a state variable at the given index
    StateRef(u32),
}

impl Operand {
    /// Decode an operand from a raw u32 value using LSB encoding
    pub fn decode(val: u32) -> Self {
        if val & 1 != 0 {
            // LSB = 1: state reference
            Self::StateRef(val >> 1)
        } else {
            // LSB = 0: literal value
            Self::Literal(val >> 1)
        }
    }

    /// Encode an operand to a raw u32 value using LSB encoding
    pub fn encode(self) -> u32 {
        match self {
            Self::Literal(val) => val << 1,        // LSB = 0
            Self::StateRef(idx) => (idx << 1) | 1, // LSB = 1
        }
    }

    /// Get the inner value (either literal value or state index)
    pub fn value(self) -> u32 {
        match self {
            Self::Literal(val) | Self::StateRef(val) => val,
        }
    }

    /// Check if this is a literal operand
    pub fn is_literal(self) -> bool {
        matches!(self, Self::Literal(_))
    }

    /// Check if this is a state reference operand
    pub fn is_state_ref(self) -> bool {
        matches!(self, Self::StateRef(_))
    }
}

/// Represents a signed operand that can be positive or negative
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedOperand {
    /// A positive value
    Positive(u32),
    /// A negative value (stored as positive but semantically negative)
    Negative(u32),
}

impl SignedOperand {
    /// Decode a signed operand from a raw u32 value
    pub fn decode(val: u32) -> Self {
        // If the value is greater than i32::MAX, it represents a negative number
        if val > i32::MAX as u32 {
            Self::Negative(0u32.wrapping_sub(val))
        } else {
            Self::Positive(val)
        }
    }

    /// Encode a signed operand to a raw u32 value
    pub fn encode(self) -> u32 {
        match self {
            Self::Positive(val) => val,
            Self::Negative(val) => 0u32.wrapping_sub(val),
        }
    }

    /// Get the value as an i32
    pub fn as_i32(self) -> i32 {
        match self {
            Self::Positive(val) => val as i32,
            Self::Negative(val) => -(val as i32),
        }
    }

    /// Check if this is a positive operand
    pub fn is_positive(self) -> bool {
        matches!(self, Self::Positive(_))
    }

    /// Check if this is a negative operand
    pub fn is_negative(self) -> bool {
        matches!(self, Self::Negative(_))
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "alloc")]
    use alloc::vec;

    #[test]
    fn test_operand_encoding_decoding() {
        // Test literal operand
        let literal = Operand::Literal(42);
        let encoded = literal.encode();
        let decoded = Operand::decode(encoded);
        assert_eq!(literal, decoded);
        assert_eq!(encoded, 84); // 42 << 1 = 84
        assert!(decoded.is_literal());
        assert!(!decoded.is_state_ref());

        // Test state reference operand
        let state_ref = Operand::StateRef(15);
        let encoded = state_ref.encode();
        let decoded = Operand::decode(encoded);
        assert_eq!(state_ref, decoded);
        assert_eq!(encoded, 31); // (15 << 1) | 1 = 31
        assert!(!decoded.is_literal());
        assert!(decoded.is_state_ref());
    }

    #[test]
    fn test_signed_operand_encoding_decoding() {
        // Test positive operand
        let positive = SignedOperand::Positive(100);
        let encoded = positive.encode();
        let decoded = SignedOperand::decode(encoded);
        assert_eq!(positive, decoded);
        assert_eq!(encoded, 100);
        assert_eq!(decoded.as_i32(), 100);
        assert!(decoded.is_positive());
        assert!(!decoded.is_negative());

        // Test negative operand
        let negative = SignedOperand::Negative(50);
        let encoded = negative.encode();
        let decoded = SignedOperand::decode(encoded);
        assert_eq!(negative, decoded);
        assert_eq!(decoded.as_i32(), -50);
        assert!(!decoded.is_positive());
        assert!(decoded.is_negative());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn test_operation_parsing_and_emitting() {
        use alloc::vec::Vec;

        // RET: opcode (2) + LSB val (4) = 6 bytes
        let ret_op = Operation::Ret(Operand::StateRef(5));
        let bytes: Vec<u8> = ret_op.emit().collect();
        assert_eq!(bytes.len(), 6);
        let (parsed_op, remaining) = Operation::parse(&bytes).unwrap();
        assert_eq!(remaining.len(), 0);
        if let Operation::Ret(operand) = parsed_op {
            assert_eq!(operand, Operand::StateRef(5));
        } else {
            panic!("Expected Ret");
        }

        // GLOBAL: opcode (2) + raw dest (4) = 6 bytes
        let global_op = Operation::Global(7);
        let bytes: Vec<u8> = global_op.emit().collect();
        assert_eq!(bytes.len(), 6);
        let (parsed_op, _) = Operation::parse(&bytes).unwrap();
        if let Operation::Global(dest) = parsed_op {
            assert_eq!(dest, 7);
        } else {
            panic!("Expected Global");
        }

        // AWAIT: opcode (2) + LSB val (4) + raw dest (4) = 10 bytes
        let await_op = Operation::Await { val: Operand::StateRef(3), dest: 9 };
        let bytes: Vec<u8> = await_op.emit().collect();
        assert_eq!(bytes.len(), 10);
        let (parsed_op, _) = Operation::parse(&bytes).unwrap();
        if let Operation::Await { val, dest } = parsed_op {
            assert_eq!(val, Operand::StateRef(3));
            assert_eq!(dest, 9);
        } else {
            panic!("Expected Await");
        }

        // LIT32: opcode (2) + raw dest (4) + raw val (4) = 10 bytes
        let lit_op = Operation::Lit32 { dest: 2, val: 0xDEAD_BEEF };
        let bytes: Vec<u8> = lit_op.emit().collect();
        assert_eq!(bytes.len(), 10);
        let (parsed_op, _) = Operation::parse(&bytes).unwrap();
        if let Operation::Lit32 { dest, val } = parsed_op {
            assert_eq!(dest, 2);
            assert_eq!(val, 0xDEAD_BEEF);
        } else {
            panic!("Expected Lit32");
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn test_array_operation() {
        use alloc::vec::Vec;

        let items = vec![
            Operand::Literal(10),
            Operand::StateRef(2),
            Operand::Literal(20),
        ];
        // dest is now a raw u32 slot index, not an Operand
        let dest: u32 = 1;
        let arr_op = Operation::Arr(items.clone(), dest);

        let bytes: Vec<u8> = arr_op.emit().collect();
        // opcode(2) + len(4) + 3 items×4(12) + dest(4) = 22 bytes
        assert_eq!(bytes.len(), 22);

        let (parsed_op, _) = Operation::parse(&bytes).unwrap();
        if let Operation::Arr(parsed_items, parsed_dest) = parsed_op {
            assert_eq!(parsed_items, items);
            assert_eq!(parsed_dest, dest);
        } else {
            panic!("Expected Arr");
        }
    }

    #[test]
    fn test_jmp_operation() {
        use alloc::vec::Vec;

        let jmp_op = Operation::Jmp { target: 42 };
        let bytes: Vec<u8> = jmp_op.emit().collect();
        // opcode(2) + target(4) = 6 bytes
        assert_eq!(bytes.len(), 6);
        let (parsed_op, remaining) = Operation::parse(&bytes).unwrap();
        assert!(remaining.is_empty());
        if let Operation::Jmp { target } = parsed_op {
            assert_eq!(target, 42);
        } else {
            panic!("Expected Jmp");
        }
    }

    #[test]
    fn test_condjmp_operation() {
        use alloc::vec::Vec;

        let condjmp_op = Operation::CondJmp {
            cond: Operand::StateRef(0),
            if_true: 10,
            if_false: 20,
        };
        let bytes: Vec<u8> = condjmp_op.emit().collect();
        // opcode(2) + LSB cond(4) + if_true(4) + if_false(4) = 14 bytes
        assert_eq!(bytes.len(), 14);
        let (parsed_op, remaining) = Operation::parse(&bytes).unwrap();
        assert!(remaining.is_empty());
        if let Operation::CondJmp { cond, if_true, if_false } = parsed_op {
            assert_eq!(cond, Operand::StateRef(0));
            assert_eq!(if_true, 10);
            assert_eq!(if_false, 20);
        } else {
            panic!("Expected CondJmp");
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn test_switch_operation() {
        use alloc::vec;
        use alloc::vec::Vec;

        let switch_op = Operation::Switch {
            val: Operand::StateRef(1),
            cases: vec![(2, 100), (3, 200)],
            default_target: 300,
        };
        let bytes: Vec<u8> = switch_op.emit().collect();
        let (parsed_op, remaining) = Operation::parse(&bytes).unwrap();
        assert!(remaining.is_empty());
        if let Operation::Switch { val, cases, default_target } = parsed_op {
            assert_eq!(val, Operand::StateRef(1));
            assert_eq!(cases, vec![(2, 100), (3, 200)]);
            assert_eq!(default_target, 300);
        } else {
            panic!("Expected Switch");
        }
    }

    #[test]
    fn test_member_get_set_operations() {
        use alloc::vec::Vec;

        let get = Operation::Get {
            obj: Operand::StateRef(1),
            key: Operand::Literal(7),
            dest: 3,
        };
        let bytes: Vec<u8> = get.emit().collect();
        let (parsed, rest) = Operation::parse(&bytes).unwrap();
        assert!(rest.is_empty());
        match parsed {
            Operation::Get { obj, key, dest } => {
                assert_eq!(obj, Operand::StateRef(1));
                assert_eq!(key, Operand::Literal(7));
                assert_eq!(dest, 3);
            }
            _ => panic!("Expected Get"),
        }

        let set = Operation::Set {
            obj: Operand::StateRef(1),
            key: Operand::Literal(7),
            val: Operand::StateRef(2),
            dest: 4,
        };
        let bytes: Vec<u8> = set.emit().collect();
        let (parsed, rest) = Operation::parse(&bytes).unwrap();
        assert!(rest.is_empty());
        match parsed {
            Operation::Set { obj, key, val, dest } => {
                assert_eq!(obj, Operand::StateRef(1));
                assert_eq!(key, Operand::Literal(7));
                assert_eq!(val, Operand::StateRef(2));
                assert_eq!(dest, 4);
            }
            _ => panic!("Expected Set"),
        }
    }

    #[test]
    fn test_legacy_encode_functions() {
        // Test compatibility with existing encode_lsb function
        assert_eq!(encode_lsb(42, true), Operand::Literal(42).encode());
        assert_eq!(encode_lsb(42, false), Operand::StateRef(42).encode());

        // Test compatibility with existing encode_signed function
        assert_eq!(
            encode_signed(42, false),
            SignedOperand::Positive(42).encode()
        );
        assert_eq!(
            encode_signed(42, true),
            SignedOperand::Negative(42).encode()
        );
    }
}

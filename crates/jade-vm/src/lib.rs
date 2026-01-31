#![no_std]
#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

include!("./data.rs");

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

        // Test RET operation
        let ret_op = Operation::Ret(Operand::StateRef(5));
        let bytes: Vec<u8> = ret_op.emit().collect();
        
        // Should be: opcode (2 bytes) + encoded operand (4 bytes)
        assert_eq!(bytes.len(), 6);
        
        // Parse it back
        let (parsed_op, remaining) = Operation::parse(&bytes).unwrap();
        assert_eq!(remaining.len(), 0);
        
        if let Operation::Ret(operand) = parsed_op {
            assert_eq!(operand, Operand::StateRef(5));
        } else {
            panic!("Expected Ret operation");
        }
    }

    #[cfg(feature = "alloc")]
    #[test] 
    fn test_array_operation() {
        use alloc::vec::Vec;

        let arr_items = vec![
            Operand::Literal(10),
            Operand::StateRef(2),
            Operand::Literal(20)
        ];
        let dest = Operand::StateRef(1);
        let arr_op = Operation::Arr(arr_items.clone(), dest);
        
        let bytes: Vec<u8> = arr_op.emit().collect();
        let (parsed_op, _) = Operation::parse(&bytes).unwrap();
        
        if let Operation::Arr(parsed_items, parsed_dest) = parsed_op {
            assert_eq!(parsed_items, arr_items);
            assert_eq!(parsed_dest, dest);
        } else {
            panic!("Expected Arr operation");
        }
    }

    #[test]
    fn test_legacy_encode_functions() {
        // Test compatibility with existing encode_lsb function
        assert_eq!(encode_lsb(42, true), Operand::Literal(42).encode());
        assert_eq!(encode_lsb(42, false), Operand::StateRef(42).encode());

        // Test compatibility with existing encode_signed function
        assert_eq!(encode_signed(42, false), SignedOperand::Positive(42).encode());
        assert_eq!(encode_signed(42, true), SignedOperand::Negative(42).encode());
    }
}
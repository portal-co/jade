# Jade VM: Two-Variant Enum Operand System

This implementation replicates the JavaScript bitwise operand parsing system using Rust's type-safe two-variant enums.

## Overview

The JavaScript VM uses bitwise operations to encode operand flags:

```javascript
const arg = () => {
    const val = code().getUint32(ip,true);
    ip += 4;
    return val & 1 ? state[val >>> 1] : val >>> 1;
}
```

This Rust implementation provides the same functionality through two enum types that automatically handle encoding/decoding:

## Operand Types

### `Operand` - LSB Flag Encoding

Handles the distinction between literal values and state variable references:

- **LSB = 0**: Literal value (`Operand::Literal(val)`)
- **LSB = 1**: State variable reference (`Operand::StateRef(index)`)

```rust
pub enum Operand {
    Literal(u32),    // Direct value
    StateRef(u32),   // Reference to state[index]
}
```

**JavaScript equivalent**: `val & 1 ? state[val >>> 1] : val >>> 1`

### `SignedOperand` - Sign Encoding

Handles positive/negative number distinction:

```rust
pub enum SignedOperand {
    Positive(u32),   // Positive number
    Negative(u32),   // Negative number (stored as positive magnitude)
}
```

**JavaScript equivalent**: Uses `wrapping_sub` for negative encoding

## Automatic Encoding/Decoding

Each operand type provides automatic conversion methods:

```rust
// Decode from raw bytecode
let operand = Operand::decode(raw_u32);

// Encode back to bytecode  
let raw_u32 = operand.encode();
```

## Generated Operations

The VM operations are automatically generated with typed operands:

```rust
pub enum Operation {
    Ret(Operand),                    // Single operand
    Fn([Operand; 4]),               // Fixed array of operands
    Arr(Vec<Operand>, Operand),     // Dynamic array + destination
    Litobj{                         // Complex structure
        c: SignedOperand,           // Signed count
        pairs: Vec<(Operand, Operand)>, // Key-value pairs
        key: Operand                // Final key
    },
    // ...
}
```

## Benefits

1. **Type Safety**: Eliminates manual bitwise operations and potential errors
2. **Automatic Parsing**: Operands are decoded during bytecode parsing
3. **Clear Semantics**: Enum variants make the distinction explicit
4. **Round-trip Fidelity**: Perfect encoding/decoding compatibility with JavaScript
5. **Zero Overhead**: Compiles to the same bitwise operations

## Usage Examples

```rust
// Parse bytecode into typed operations
let (operation, remaining_bytes) = Operation::parse(bytecode)?;

match operation {
    Operation::Ret(operand) => {
        match operand {
            Operand::Literal(val) => println!("Return literal: {}", val),
            Operand::StateRef(idx) => println!("Return state[{}]", idx),
        }
    }
    Operation::Litobj { c, pairs, key } => {
        let count = c.as_i32(); // Automatic sign handling
        // Process object construction...
    }
    // ...
}

// Emit operations back to bytecode
let bytes: Vec<u8> = operation.emit().collect();
```

This system maintains full compatibility with the JavaScript implementation while providing Rust's compile-time safety guarantees.
//! Hand-written helpers that generated primordial code (`jade-primordial-rt`) calls for the
//! host-intrinsic mapping table's entries — see the "Host-intrinsic mapping table" section of
//! the primordial-IR plan. None of this is generated; it is the fixed, small vocabulary of
//! host builtins the surveyed `packages/jade-js/primordials/*.ts` sources actually use.

/// Mirrors `Number.isInteger(x)`.
pub fn is_integer(x: f64) -> bool {
    x.is_finite() && x.fract() == 0.0
}

/// Mirrors the fixed numeric-array-index-string test used throughout `typed-arrays.ts` and
/// `types.ts` (`/^(0|[1-9][0-9]*)$/.test(key)`): `"0"`, or a non-empty digit string with no
/// leading zero. Not general regex support — this one pattern is the only regex literal that
/// appears in the primordials.
pub fn is_array_index_string(key: &str) -> bool {
    if key == "0" {
        return true;
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(first) if first.is_ascii_digit() && first != '0' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_digit())
}

/// Parses an array-index string validated by [`is_array_index_string`] into its numeric value.
pub fn parse_array_index(key: &str) -> Option<usize> {
    if is_array_index_string(key) {
        key.parse().ok()
    } else {
        None
    }
}

/// Little-endian byte codec for one typed-array element kind — the Rust lowering of
/// `typed-arrays.ts`'s `Codec` table (`{ bytes, get(view, offset), set(view, offset, value) }`,
/// always constructed with the `littleEndian = true` `DataView` argument).
#[derive(Debug, Clone, Copy)]
pub struct Codec {
    pub bytes: usize,
    pub get: fn(&[u8]) -> f64,
    pub set: fn(&mut [u8], f64),
}

fn clamp_u8(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

pub const INT8: Codec = Codec {
    bytes: 1,
    get: |b| b[0] as i8 as f64,
    set: |b, v| b[0] = v as i64 as i8 as u8,
};
pub const UINT8: Codec = Codec {
    bytes: 1,
    get: |b| b[0] as f64,
    set: |b, v| b[0] = v as i64 as u8,
};
pub const UINT8_CLAMPED: Codec = Codec {
    bytes: 1,
    get: |b| b[0] as f64,
    set: |b, v| b[0] = clamp_u8(v),
};
pub const INT16: Codec = Codec {
    bytes: 2,
    get: |b| i16::from_le_bytes([b[0], b[1]]) as f64,
    set: |b, v| b[..2].copy_from_slice(&((v as i64) as i16).to_le_bytes()),
};
pub const UINT16: Codec = Codec {
    bytes: 2,
    get: |b| u16::from_le_bytes([b[0], b[1]]) as f64,
    set: |b, v| b[..2].copy_from_slice(&((v as i64) as u16).to_le_bytes()),
};
pub const INT32: Codec = Codec {
    bytes: 4,
    get: |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
    set: |b, v| b[..4].copy_from_slice(&((v as i64) as i32).to_le_bytes()),
};
pub const UINT32: Codec = Codec {
    bytes: 4,
    get: |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
    set: |b, v| b[..4].copy_from_slice(&((v as i64) as u32).to_le_bytes()),
};
pub const FLOAT32: Codec = Codec {
    bytes: 4,
    get: |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
    set: |b, v| b[..4].copy_from_slice(&(v as f32).to_le_bytes()),
};
pub const FLOAT64: Codec = Codec {
    bytes: 8,
    get: |b| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
    set: |b, v| b[..8].copy_from_slice(&v.to_le_bytes()),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_index_strings() {
        assert!(is_array_index_string("0"));
        assert!(is_array_index_string("1"));
        assert!(is_array_index_string("42"));
        assert!(!is_array_index_string("00"));
        assert!(!is_array_index_string("01"));
        assert!(!is_array_index_string(""));
        assert!(!is_array_index_string("-1"));
        assert!(!is_array_index_string("1.5"));
        assert!(!is_array_index_string("abc"));
    }

    #[test]
    fn integer_check() {
        assert!(is_integer(0.0));
        assert!(is_integer(-3.0));
        assert!(!is_integer(1.5));
        assert!(!is_integer(f64::NAN));
        assert!(!is_integer(f64::INFINITY));
    }

    #[test]
    fn codec_round_trips() {
        for codec in [
            INT8, UINT8, UINT8_CLAMPED, INT16, UINT16, INT32, UINT32, FLOAT32, FLOAT64,
        ] {
            let mut bytes = vec![0u8; codec.bytes];
            (codec.set)(&mut bytes, 5.0);
            assert_eq!((codec.get)(&bytes), 5.0);
        }
    }
}

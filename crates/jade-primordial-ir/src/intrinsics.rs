//! The host-intrinsic mapping table (see the primordial-IR plan's "Host-intrinsic mapping
//! table" section). `lower.rs` recognizes a fixed, curated set of global/member calls found in
//! the surveyed source and turns each into an `Expr::HostIntrinsic { name, .. }` node with one
//! of the names below; anything else calling a global is a hard lowering rejection. `emit_ts`
//! passes every recognized intrinsic straight through to its original JS spelling (trivial,
//! since that's exactly what was parsed); `emit_rust` maps each name to a concrete Rust
//! expression via [`rust_call`].

/// One entry in the table: the recognized intrinsic's IR name, and how `emit_rust` spells a
/// call to it. `{0}`, `{1}`, ... in `rust_template` are replaced with the emitted Rust source
/// of the corresponding argument.
pub struct IntrinsicSpec {
    pub name: &'static str,
    pub rust_template: &'static str,
}

pub const IS_ARRAY_INDEX_STRING: &str = "is_array_index_string";
pub const IS_STRING_KEY: &str = "is_string_key";
pub const TO_NUMBER: &str = "to_number";
pub const TO_STRING: &str = "to_string_intrinsic";
pub const ARRAY_FILTER: &str = "array_filter";
pub const ARRAY_SORT_BY: &str = "array_sort_by";
pub const ARRAY_PUSH: &str = "array_push";
pub const ARRAY_SLICE_FROM: &str = "array_slice_from";
pub const MAP_GET: &str = "map_get";
pub const MAP_SET: &str = "map_set";
pub const MAP_HAS: &str = "map_has";
pub const MAP_DELETE: &str = "map_delete";
pub const NUMBER_IS_INTEGER: &str = "number_is_integer";
pub const MATH_MIN: &str = "math_min";
pub const MATH_MAX: &str = "math_max";
pub const MATH_ROUND: &str = "math_round";

pub const TABLE: &[IntrinsicSpec] = &[
    IntrinsicSpec {
        name: IS_ARRAY_INDEX_STRING,
        rust_template: "portal_solutions_jade_tenant_rt::intrinsics::is_array_index_string(&{0})",
    },
    IntrinsicSpec {
        name: IS_STRING_KEY,
        rust_template: "matches!({0}, portal_solutions_jade_tenant_rt::PropertyKey::String(_))",
    },
    IntrinsicSpec {
        name: TO_NUMBER,
        rust_template: "({0}).parse::<f64>().unwrap_or(f64::NAN)",
    },
    IntrinsicSpec {
        name: TO_STRING,
        rust_template: "({0}).to_string()",
    },
    IntrinsicSpec {
        name: NUMBER_IS_INTEGER,
        rust_template: "portal_solutions_jade_tenant_rt::intrinsics::is_integer({0})",
    },
    IntrinsicSpec {
        name: MATH_MIN,
        rust_template: "f64::min({0}, {1})",
    },
    IntrinsicSpec {
        name: MATH_MAX,
        rust_template: "f64::max({0}, {1})",
    },
    IntrinsicSpec {
        name: MATH_ROUND,
        rust_template: "f64::round({0})",
    },
];

pub fn spec(name: &str) -> Option<&'static IntrinsicSpec> {
    TABLE.iter().find(|entry| entry.name == name)
}

//! SWC-based intermediate IR for `packages/jade-js/primordials/*.ts`. Parses that TypeScript
//! source (via `swc_ecma_parser`) into [`ir::Module`], and can re-emit it either back to
//! TypeScript (`emit_ts`, a losslessness/verification check — the hand-written TS source
//! remains the canonical single source of truth) or as genuine, checked-in Rust source
//! implementing the primordials against `jade-tenant-rt`'s `Tenant` trait (`emit_rust`, the new
//! artifact this crate exists to produce).
//!
//! See the primordial-IR plan for the full design and phased rollout.

pub mod cross_file;
pub mod emit_rust;
pub mod emit_ts;
pub mod intrinsics;
pub mod ir;
pub mod lower;
pub mod shims;

use std::fmt;

#[derive(Debug)]
pub enum IrError {
    Parse(String),
    /// A construct outside the surveyed IR node set (see `ir.rs`'s module doc comment). Names
    /// the exact file/construct — never a silent best-effort translation.
    Unsupported { file: String, construct: String },
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrError::Parse(message) => write!(f, "parse error: {message}"),
            IrError::Unsupported { file, construct } => {
                write!(f, "{file}: unsupported construct: {construct}")
            }
        }
    }
}

impl std::error::Error for IrError {}

/// Parse `source` (the contents of `file_name`, used only for diagnostics) into a [`ir::Module`].
pub fn translate_source(file_name: &str, source: &str) -> Result<ir::Module, IrError> {
    lower::lower_module(file_name, source)
}

pub fn emit_typescript(module: &ir::Module) -> String {
    emit_ts::emit_module(module)
}

pub fn emit_rust_source(module: &ir::Module, module_name: &str) -> String {
    emit_rust::emit_module(module, module_name)
}

//! GENERATED crate: Rust implementations of `packages/jade-js/primordials/*.ts`, produced by
//! `jade-primordial-ir`'s `gen-primordials` binary. Do not hand-edit files under `src/` other
//! than this placeholder — see the primordial-IR plan's "Package layout" section.
//!
//! Populated incrementally as the IR/emitter gains coverage of each source file. `types.ts` is
//! never a target of that generation — it's hand-shimmed instead; see `types_shim`'s module doc
//! comment.
//!
//! `object.rs` (from `object.ts`) and `function.rs` (from `function.ts`) are both fully
//! IR-lowered and generated, including `objectPrimordial`/`functionPrimordial` themselves —
//! `Object.keys`/`Reflect`-style `Vec<PropertyKey>`-returning methods now translate via
//! `Tenant::indexed_collection`/`property_key_value` (see `docs/array-primordial-gap-plan.md`,
//! which this closes).

pub mod types_shim;
pub mod object;
pub mod function;

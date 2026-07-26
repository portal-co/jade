//! GENERATED crate: Rust implementations of `packages/jade-js/primordials/*.ts`, produced by
//! `jade-primordial-ir`'s `gen-primordials` binary. Do not hand-edit files under `src/` other
//! than this placeholder — see the primordial-IR plan's "Package layout" section.
//!
//! Populated incrementally as the IR/emitter gains coverage of each source file. `types.ts` is
//! never a target of that generation — it's hand-shimmed instead; see `types_shim`'s module doc
//! comment.
//!
//! `object.rs` (from `object.ts`) is the first file lowered beyond `types.ts`'s helpers:
//! `install_method`/`lock` and the `ObjectPrimordial`/`ObjectPrimordialCache` types are fully
//! generated and tested. `objectPrimordial` itself is not yet — `Object.keys`'s closure returns
//! `tenant.ownKeys(...)`'s host-side `Vec<PropertyKey>` directly as a guest value, which has no
//! sound Rust translation without a guest Array primordial (out of scope per
//! `docs/primordials-plan.md`'s own non-goals). `gen-primordials` reports this on stderr and
//! omits just that one function, rather than embedding a `compile_error!` that would block the
//! whole crate from building — see `jade-primordial-ir::emit_rust`'s module doc comment.

pub mod types_shim;
pub mod object;

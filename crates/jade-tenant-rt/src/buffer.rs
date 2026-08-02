//! Hand-authored Rust mirror of `array-buffer.ts`'s `BufferHooks` interface — see
//! `docs/proxy-and-buffer-primordial-gap-plan.md`'s "real host JS builtins in
//! `nativeBufferHooks`" section for why this is hand-authored rather than IR-derived: the TS
//! interface's concrete implementation (`nativeBufferHooks`) is built directly from real host
//! `ArrayBuffer`/`SharedArrayBuffer`/`Uint8Array`, which have no `Tenant::Value` translation —
//! the same shape of gap already excluded for `promise.ts`'s host-`Promise` boundary. The
//! *factory* logic that only ever calls through this trait (`bufferPrimordial`/
//! `typedArraysPrimordial`) remains a real IR-lowered/generated target; only the concrete
//! adapter is hand-written, exactly as `types_shim.rs` is to `types.ts`.
//!
//! Deliberate simplification versus the TS interface: no `identity` field. Its one job in TS is
//! letting a per-tenant cache validate "is this the same `BufferHooks` capability as before"
//! without structurally comparing hooks that may contain closures — in Rust, a cache generic
//! over a concrete `H: BufferHooks` type already ties itself to one hooks *type* at compile
//! time, which is a stronger guarantee than the TS runtime check for the common case (a single
//! process using one hooks implementation). A caller juggling multiple *instances* of the same
//! `BufferHooks` type with one tenant is a real but narrow scenario this simplification doesn't
//! cover — revisit if that's ever needed.

use crate::TenantError;

/// Mirrors `array-buffer.ts`'s `BufferKind` (`"array-buffer" | "shared-array-buffer"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferKind {
    ArrayBuffer,
    SharedArrayBuffer,
}

/// Mirrors `array-buffer.ts`'s `BufferHooks` — an explicit embedder capability. No buffer/typed-
/// array primordial exists without one (see the TS doc comment on `BufferHooks`: "Explicit
/// opt-in adapter... never automatically selected").
pub trait BufferHooks {
    /// Opaque per-implementation handle to one allocated buffer. Mirrors TS's `BufferHandle =
    /// object` — never a `Tenant::Value`; guest code only ever observes the tenant-owned buffer
    /// *shell* the primordial wraps around a handle, never the handle itself.
    type Handle: Clone;

    fn supports_shared_array_buffer(&self) -> bool;
    fn allocate(&mut self, kind: BufferKind, byte_length: usize) -> Result<Self::Handle, TenantError>;
    /// Whether `value` is already a handle of this hooks implementation's own making — TS's
    /// version takes `value: unknown` (any value at all) since it's meant to gate whether some
    /// arbitrary input can be treated as a handle; no primordial factory currently IR-lowered
    /// calls this, so its exact contract is only exercised by hand-written call sites so far.
    fn is_handle(&self, value: &Self::Handle, kind: Option<BufferKind>) -> bool;
    fn byte_length(&mut self, handle: &Self::Handle) -> Result<usize, TenantError>;
    fn slice(&mut self, handle: &Self::Handle, begin: usize, end: usize) -> Result<Self::Handle, TenantError>;
    /// Exact bytes; implementations may perform asynchronous storage work in a real embedding
    /// (TS's `read`/`write` both return `TenantGenerator<_>` for exactly this reason) — this
    /// trait's own methods are synchronous/fallible like every other `jade-tenant-rt` trait (see
    /// this crate's module doc comment), so a genuinely async hooks implementation is out of
    /// scope for this first Rust port, same as everywhere else in this crate.
    fn read(&mut self, handle: &Self::Handle, byte_offset: usize, byte_length: usize) -> Result<Vec<u8>, TenantError>;
    fn write(&mut self, handle: &Self::Handle, byte_offset: usize, bytes: &[u8]) -> Result<(), TenantError>;
}

//! Hand-written Rust adapter implementing `jade-tenant-rt::BufferHooks` — the counterpart to
//! `array-buffer.ts`'s `nativeBufferHooks`, but **not a port of its internals**: there is no
//! native `ArrayBuffer`/`SharedArrayBuffer`/`DataView` in a pure-Rust embedding to wrap, so this
//! is a fresh, idiomatic Rust in-memory implementation of the same abstract contract instead —
//! see `docs/proxy-and-buffer-primordial-gap-plan.md`. Not generated; `array-buffer.ts`'s own
//! factory logic (`bufferPrimordial`) remains a real IR-lowered target that calls through this
//! trait generically, exactly as `types_shim.rs`'s hand-written helpers relate to `types.ts`.
//!
//! `NativeBufferHandle` wraps `Rc<RefCell<Vec<u8>>>` rather than a plain `Vec<u8>`: `slice`
//! yields an independent, freshly-copied buffer (an ordinary Rust `Vec` clone), matching
//! `ArrayBuffer.prototype.slice`'s copy semantics — this crate does not attempt `SharedArrayBuffer`'s
//! true aliasing-view semantics (`supports_shared_array_buffer` returns `false`; nothing surveyed
//! in the primordials requires it to return `true`, and doing so honestly would need `Arc<Mutex
//! <_>>` for real cross-thread sharing instead of `Rc<RefCell<_>>`).

use std::cell::RefCell;
use std::rc::Rc;

use portal_solutions_jade_tenant_rt::{BufferHooks, BufferKind, TenantError};

#[derive(Clone)]
pub struct NativeBufferHandle(Rc<RefCell<Vec<u8>>>);

/// The default in-memory `BufferHooks` adapter — always available, never implicitly selected
/// (mirrors `nativeBufferHooks`'s own "explicit opt-in, never automatic" contract).
#[derive(Default)]
pub struct NativeBufferHooks;

impl BufferHooks for NativeBufferHooks {
    type Handle = NativeBufferHandle;

    fn supports_shared_array_buffer(&self) -> bool {
        false
    }

    fn allocate(&mut self, _kind: BufferKind, byte_length: usize) -> Result<Self::Handle, TenantError> {
        Ok(NativeBufferHandle(Rc::new(RefCell::new(vec![0u8; byte_length]))))
    }

    fn is_handle(&self, _value: &Self::Handle, _kind: Option<BufferKind>) -> bool {
        true
    }

    fn byte_length(&mut self, handle: &Self::Handle) -> Result<usize, TenantError> {
        Ok(handle.0.borrow().len())
    }

    fn slice(&mut self, handle: &Self::Handle, begin: usize, end: usize) -> Result<Self::Handle, TenantError> {
        let buf = handle.0.borrow();
        let end = end.min(buf.len());
        let begin = begin.min(end);
        Ok(NativeBufferHandle(Rc::new(RefCell::new(buf[begin..end].to_vec()))))
    }

    fn read(&mut self, handle: &Self::Handle, byte_offset: usize, byte_length: usize) -> Result<Vec<u8>, TenantError> {
        let buf = handle.0.borrow();
        buf.get(byte_offset..byte_offset + byte_length)
            .map(|slice| slice.to_vec())
            .ok_or_else(|| TenantError::range_error("read out of bounds"))
    }

    fn write(&mut self, handle: &Self::Handle, byte_offset: usize, bytes: &[u8]) -> Result<(), TenantError> {
        let mut buf = handle.0.borrow_mut();
        let end = byte_offset.checked_add(bytes.len()).ok_or_else(|| TenantError::range_error("write out of bounds"))?;
        if end > buf.len() {
            return Err(TenantError::range_error("write out of bounds"));
        }
        buf[byte_offset..end].copy_from_slice(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_zero_fills_and_reports_byte_length() {
        let mut hooks = NativeBufferHooks;
        let handle = hooks.allocate(BufferKind::ArrayBuffer, 4).unwrap();
        assert_eq!(hooks.byte_length(&handle).unwrap(), 4);
        assert_eq!(hooks.read(&handle, 0, 4).unwrap(), vec![0, 0, 0, 0]);
    }

    #[test]
    fn write_then_read_round_trips() {
        let mut hooks = NativeBufferHooks;
        let handle = hooks.allocate(BufferKind::ArrayBuffer, 4).unwrap();
        hooks.write(&handle, 1, &[9, 8, 7]).unwrap();
        assert_eq!(hooks.read(&handle, 0, 4).unwrap(), vec![0, 9, 8, 7]);
    }

    #[test]
    fn write_out_of_bounds_is_a_range_error() {
        let mut hooks = NativeBufferHooks;
        let handle = hooks.allocate(BufferKind::ArrayBuffer, 2).unwrap();
        assert!(matches!(hooks.write(&handle, 1, &[1, 2]), Err(TenantError::RangeError(_))));
    }

    #[test]
    fn read_out_of_bounds_is_a_range_error() {
        let mut hooks = NativeBufferHooks;
        let handle = hooks.allocate(BufferKind::ArrayBuffer, 2).unwrap();
        assert!(matches!(hooks.read(&handle, 1, 5), Err(TenantError::RangeError(_))));
    }

    #[test]
    fn slice_copies_independent_bytes() {
        let mut hooks = NativeBufferHooks;
        let handle = hooks.allocate(BufferKind::ArrayBuffer, 4).unwrap();
        hooks.write(&handle, 0, &[1, 2, 3, 4]).unwrap();
        let sliced = hooks.slice(&handle, 1, 3).unwrap();
        assert_eq!(hooks.read(&sliced, 0, 2).unwrap(), vec![2, 3]);
        // Independent copy: writing to the original doesn't affect the slice.
        hooks.write(&handle, 1, &[9, 9]).unwrap();
        assert_eq!(hooks.read(&sliced, 0, 2).unwrap(), vec![2, 3]);
    }

    #[test]
    fn supports_shared_array_buffer_is_false() {
        assert!(!NativeBufferHooks.supports_shared_array_buffer());
    }
}

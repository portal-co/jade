# Jade VM – Goals

## JIT compilation

`Ops::fixpoint`, `if_op`, and `switch_op` take `&mut Ctx` as a method-level generic
so that a future JIT backend can pass its compilation context (e.g. a Cranelift
function builder) into the handler without it appearing in the trait bounds.

The interpreter passes `&mut ()` as `Ctx` and ignores it.  A JIT implementation
will pass a context that accumulates compiled code, allowing each block body to be
compiled on first entry and re-used on subsequent calls:

- **`fixpoint`** – the JIT compiles the body once, then spins until the output
  value stabilises (fixed point), re-executing the compiled form.
- **`if_op`** – the JIT compiles both branches eagerly so that branch-prediction
  and speculation are possible.
- **`switch_op`** – the JIT compiles all cases and the default, then dispatches
  via a jump table.

The body closures are `impl FnOnce/FnMut(&mut Self, &mut Ctx) -> Result<Self::Value, Self::Error>`
rather than raw `&[u8]` slices, so the `Ops` implementor is decoupled from the
bytecode format entirely.

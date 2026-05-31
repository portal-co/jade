# Jade VM – Goals

## Dispatch

A single `exec_op<P, Ctx>(op, code, platform, ctx)` (generated into
`jade-vm-core`) dispatches *every* non-loop opcode, including the control-flow
blocks (`WHILE`, `IF`, `SWITCH`). Block bodies are wrapped in closures that
recurse back into `exec_op`, so there is one entry point for both the
interpreter and the JIT. The four loop-level opcodes
(`RET`/`AWAIT`/`YIELD`/`YIELDSTAR`) are handled directly by the VM loop before
`exec_op` is reached.

`while_op`, `if_op`, and `switch_op` take `&mut Ctx` as a method-level generic so
a backend can thread a compilation context into the handler without it appearing
in the trait bounds. The interpreter passes `&mut ()` and ignores it.

The body closures are `impl FnOnce/FnMut(&mut Self, &mut Ctx) -> Result<Self::Value, Self::Error>`
rather than raw `&[u8]` slices, so the `Ops` implementor is decoupled from the
bytecode format entirely.

## `while`

`WHILE = [LSB cond][raw body_len][body…][LSB next]`. `cond` is the initial
condition; the body runs while the threaded condition value is truthy and, on
each iteration, returns the *next* condition value (the `next` operand,
re-resolved after the body runs). `Ops::while_op(ctx, init, body)` owns the loop
(`let mut c = init; while truthy(c) { c = body()?; }`).

## JIT compilation (`jade-vm-jit`)

`jade-vm-jit` is a backend that implements `State` + `Ops` and emits **textual
JavaScript**, runnable in any JS runtime (in-browser via a WASM host, or
remotely). It drives the shared `exec_op`.

- **`Value`s are JS variable IDs.** Each produced value is a `v{n}` temporary (or
  an inline leaf expression for literals / `state[idx]` reads). State is a real
  JS `state` object in the emitted code.
- **`if_op`** emits *both* branches (all branches always emitted).
- **`switch_op`** emits every case and the default.
- **`while_op`** emits a real JS `while`, with the body emitted exactly **once**.
- **`op_fn`** compiles the function body into its own source and delegates to a
  user-supplied `FnRegistry`; registered functions always lead with the implicit
  `tenant` and `nt` parameters, i.e. `function(tenant, nt, ...args){ … }`.

Entry point: `jade_vm_jit::compile(code, reg) -> Result<(String, R), String>`.

Follow-ups (not yet wired in the first backend): `FN` closure-slot capture, the
function variant (sync/async/generator) selecting the declaration form, and
decorator (`spanner`) application; and `AWAIT`/`YIELD`/`YIELDSTAR` support.

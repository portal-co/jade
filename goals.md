# Jade VM – Goals

## Tenants (object manager / isolation)

The `tenant` isolates a virtual environment by acting as an **object manager**:
the VM never touches object properties directly — it goes through
`tenant.{make,get,set,has,delete,ownKeys,define,assign}` (`packages/jade-js/index.ts`).
This decouples the property representation from the VM:

- **single-tenant** (`single_tenant.ts`) backs objects natively, mangling only
  "dirty" (polyfill/camo) keys.
- **multi-tenant** (`multi_tenant.ts`) keeps a private `WeakMap` shadow per
  tenant; `make()` returns a bare empty shell, so tenant properties are
  invisible to the host and other tenants ("foreign by nature") and are
  collected by the native GC (no manual mark/sweep). The legacy `gc.ts`
  `GCReactor` is retained only as dead code, slated to move to `semble`.

Property access uses the `GET` / `SET` opcodes (`op_get` / `op_set`), which
delegate to `tenant.get` / `tenant.set`; object literals (`LITOBJ`) build via
`tenant.make` + `tenant.set` (+ `tenant.assign` for spread), and the define path
uses `tenant.define`. Every backend (TS interpreter, Rust wasm, JIT) routes
object operations through the tenant.

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
  `tenant` and `nt` parameters, i.e. `function(tenant, nt, ...args){ … }`. The
  function's `variant` operand selects the declaration form passed to the
  registry — `function` / `async function` / `function*` / `async function*`
  (`FnVariant`).
- **`op_call`** threads the enclosing `tenant` and `nt` ahead of the user
  arguments, matching the registered `(tenant, nt, ...args)` calling convention.

Entry point: `jade_vm_jit::compile(code, reg) -> Result<(String, R), String>`.

Follow-ups (not yet wired in the first backend): `FN` closure-slot capture and
decorator (`spanner`) application; and `AWAIT`/`YIELD`/`YIELDSTAR` support.

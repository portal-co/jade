# Plan: free-variable capture analysis for nested closures

## Current state

`crates/jade-vm-frontend` now lowers nested function/closure literals
(`Item::Func`) to a real `FN` opcode (`compile_program`, `lib.rs`) — see the
git history around this doc's introduction for the two-phase (measure, then
emit) scheme that resolves a nested function's byte offset (`j`). This landed
**without** free-variable capture: every emitted `Fn` op has
`closure_args: Operand::Literal(0)` and `spanner: Operand::Literal(0)`. A
nested closure today can only observe values reachable through `tenant`/`nt`
(threaded into every function's parameters, same as any top-level function)
or its own locals — not a free variable from an enclosing scope. In practice
this means a JS closure like:

```js
function makeAdder(x) {
  return function (y) { return x + y; }; // `x` is a free variable here
}
```

does not yet work: `x` isn't reachable inside the nested function's own
state slots.

## Why this is a separate, deferred concern

Both bytecode operands this needs already exist and are already accepted by
every JIT tier's `op_fn` — they're just unused:

- **`closure_args`** (`Operation::Fn`'s second operand): intended to carry
  whatever values from the *enclosing* function's state slots the nested
  function needs to observe, threaded in alongside `tenant`/`nt` when the
  produced function is actually created/called. No opcode encodes *which*
  enclosing slots to capture yet — that's exactly the free-variable-capture
  analysis this doc is about.
- **`spanner`** (`Operation::Fn`'s third operand): a decorator hook applied
  to the produced function at creation time (see `jade-vm-jit`'s `op_fn` doc
  comment: "closure-slot capture and decorator (`spanner`) application are
  not yet wired in this first backend"). Not required for capture itself,
  but likely composes with it (e.g. a decorator that snapshots captured
  values, or that rebinds them by reference vs. by value).

Landing nested closures with `tenant`/`nt`-only threading first (no capture)
is a real, independently useful step — it already covers callback-shaped
code with no free variables (the two `jade-vm-jit-swc` doubleGen/nested-`Fn`
tests added alongside this doc use exactly that shape) — without blocking on
the harder, separate problem below.

## What a real implementation needs

1. **Identify free variables.** Walk the nested `TFunc`'s own `TCfg` (the
   same `Item`/`TStmt` shapes `FnLowering::lower_item`/`lower_stmt` already
   walk) and collect every `Ident` referenced that resolves to a slot in an
   *enclosing* function's `FnLowering::slots`, not the nested function's own.
   This has to happen during `compile_program`'s existing per-function
   lowering pass (phase 1 or a new pass before it), since it needs visibility
   into both the nested function's body *and* the enclosing function's slot
   table at the same time — today's `FnLowering` only ever sees one
   function's own `tcfg`/`slots` at once.
2. **Encode captured values.** At the `Item::Func` call site (in the
   *enclosing* function, where the captured values currently live in its own
   state slots), build a `closure_args` value — most naturally a Jade array
   (`Operation::Arr`) of the captured slots' current operands — and pass its
   operand as `Operation::Fn`'s `closure_args`.
3. **Bind captured values inside the nested function.** The nested function's
   own emitted parameter list (`tenant`, `nt`, `...args` today, per every JIT
   tier's `FnRegistry::register`) needs to also receive `closure_args`
   unpacked into its own fresh slots, allocated *before* `FnLowering` starts
   assigning slots for the nested body's own locals — i.e. `FnLowering`'s
   `next_slot` needs to start past however many captured values there are,
   with each capture's slot pre-populated from `closure_args[i]` rather than
   left to read as `undefined`.
4. **Decide capture semantics**: by-value (snapshot the enclosing slot's
   value at closure-creation time — simple, matches most real-world closure
   usage, but wrong for a closure that observes later mutations of the
   captured variable) vs. by-reference (the nested and enclosing function
   share the same mutable cell — correct JS semantics for `let`/`var` capture,
   but Jade bytecode has no existing "boxed/shared mutable cell" concept to
   build on, so this would need new plumbing). Real JS closures are
   by-reference; a from-scratch implementation should not silently ship
   by-value semantics as if it were the general case.
5. **Where `spanner` might compose in**: once capture exists, a `spanner`
   decorator could plausibly be used to implement by-reference semantics
   (wrapping the produced function so reads/writes of a captured variable
   route through a shared cell) rather than inventing a fifth bytecode
   concept — worth evaluating once both pieces are being designed together,
   rather than assuming they're unrelated.

## Status

Not started. `Item::Func` lowering (`crates/jade-vm-frontend/src/lib.rs`)
always emits `closure_args: Operand::Literal(0)` today; every JIT tier
already tolerates this (no tier currently reads `closure_args` at all — see
`op_fn` in `crates/jade-vm-jit`/`crates/jade-vm-jit-swc`, both of which
receive it as an ignored parameter), so this is a purely additive follow-on,
not a breaking change to any existing tier.

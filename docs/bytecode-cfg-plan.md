# Jade bytecode: CFG-native representation (shipped)

This document originally proposed moving Jade bytecode to a flat, jump-based
CFG representation. That migration has shipped; this document now describes
the design as built, so it stays useful as a map of the system rather than a
stale proposal.

## Wire format

Jade bytecode is a **flat array of basic blocks**, each block a sequence of
value-producing ops (`LIT32`, `BOOL`, `SEL`, `EQ`/`NE`/`LT`/`LE`/`GT`/`GE`,
`GET`/`SET`, `CALL`, `ARR`/`STR`/`LITOBJ`, `AWAIT`/`YIELD`/`YIELDSTAR`, `FN`,
`GLOBAL`, `NEW_TARGET`, …) ending in exactly one terminator:

- `JMP <target: u32>`
- `CONDJMP <cond: Operand> <if_true: u32> <if_false: u32>`
- `SWITCH <val: Operand> <n: u32> <n×(case_val, target)> <default_target>`
- `RET <val: Operand>`

Targets are **raw byte offsets into the same function's code buffer** — there
is no separate block-index/table concept, matching how every consumer already
addresses code (`FN`'s `j: u32`, every interpreter's `ip`). `RET` is valid in
*any* block, not just the last op of the function — the old nested/nested-body
format (`WHILE`/`IF`/`SWITCH` embedding a length-prefixed body blob, executed
by recursive descent) is gone. This is a breaking, unversioned change: the
format is never persisted (generated and consumed within one process), so
there is no legacy encoding to keep around.

Source of truth: `packages/jade-data/index.ts` (opcode spec), generated into
`crates/jade-vm/src/data.rs` and `crates/jade-vm-core/src/dispatch.rs` via
`scripts/gen/*.ts` / `scripts/regen.ts`. `jade-vm-core::dispatch::exec_op`
stays exactly the shared, generic-over-`P: State + Ops` dispatcher it always
was for value-producing ops — it never grew arms for `JMP`/`CONDJMP`/`SWITCH`,
since a jump is just "driver, please continue at this other offset." Each
consumer's own driver (`jade-vm-core`'s interpreter, `jade-vm-wasm`,
`jade-vm-jit`'s Tier 0) special-cases `RET`/`AWAIT`/`YIELD`/`YIELDSTAR`/
`JMP`/`CONDJMP`/`SWITCH` itself by updating its own `ip`.

## Frontend: `crates/jade-vm-frontend`

Pipeline: source text → `swc_ecma_ast::Function` (`swc_ecma_parser`) →
`TFunc`/`TCfg` (`portal-jsc-swc-tac`) → `jade-cfg-opt::optimize_tfunc` (see
below) → Jade bytecode. Because TAC's own blocks/terminators
(`TBlock`/`TTerm::{Jmp,CondJmp,Switch,Return}`) already match Jade bytecode's
shape almost 1:1, `FnLowering::compile_blocks` just walks `TCfg` directly: no
restructuring step, no flag-passing workarounds for `return` — every TAC
block becomes one Jade block at its own byte offset (two-pass: lower every
terminator once with placeholder `0` targets to learn each block's fixed
encoded length, compute prefix-sum offsets, then lower again for real), and
every TAC terminator becomes the matching Jade terminator. The previously
broken/rejected patterns (`return` inside a loop's own conditional; an
`if`/`else` where both arms `return` through a shared tail) just work now —
see `return_inside_while_loop_now_works_correctly`,
`if_else_return_values_are_distinguishable`.

Scope is deliberately narrow and explicit: only constructs with a direct Jade
opcode are lowered; anything else is `FrontendError::Unsupported`, never a
silent miscompile (no arithmetic ops besides the six comparisons, no nested
closures yet, no exception handling, no `switch` statement, etc. — see the
module doc comment in `crates/jade-vm-frontend/src/lib.rs` for the current
list).

## `crates/jade-cfg-opt`: shared TAC canonicalizer + SSA constant-fold/DCE

A small crate (`portal-jsc-swc-tac` + `portal-jsc-swc-ssa` only, no full SWC
AST/codegen stack) exposing `optimize_tfunc(&TFunc) -> Result<TFunc, Error>`:
round-trips the function through SSA form using `portal-jsc-swc-ssa`'s
existing `TFunc -> SFunc` / `SFunc -> TFunc` conversions, applying
`simplify_conditions` (folds a `CondJmp` with a known-boolean condition into a
plain `Jmp`, dropping the untaken branch entirely),
`simplify_loads` (redundant-load elimination) and `simplify_justs` (alias-chain
collapsing) in between — all pre-existing `portal-jsc-swc-ssa` passes, not
reimplemented here. `jade-vm-frontend` calls it unconditionally before
lowering; this is where "optimizing input JS during translation as a
byproduct" comes from (verified by `constant_condition_is_folded_away`, which
checks the untaken branch's literal never appears in the compiled output).

Because it depends on nothing beyond TAC/SSA, it's shared between the
frontend and Tier 2's JIT plugin (below) rather than duplicated.

One integration note: the SSA round-trip's own shim/entry-block mechanics
introduce explicit `Item::Undef` writes that direct AST→TAC lowering never
produces on its own; `jade-vm-frontend::lower_item` treats `Item::Undef` as a
no-op (an unwritten Jade state slot already reads as `undefined`, matching
the existing `undefined_operand` approach for `return;`).

## JIT backends: `crates/jade-vm-jit` (Tiers 0/1) and `crates/jade-vm-jit-swc` (Tier 2)

Three progressively heavier tiers, all consuming the same bytecode and
exposing compatible `compile(code, ..., Config)` entry points, so a caller can
pick based on what's available in its environment:

- **Tier 0** (always on, zero relooper dependency) — `jade-vm-jit`'s default
  `compile()`. A pre-pass (`discover_blocks`, `pub` so other tiers can reuse
  it) splits a function into blocks at every jump target, then emits
  `let __ip = 0; while (true) { switch (__ip) { case <offset>: { ...;
  __ip = <target>; continue; } ... } }`. Always correct; this is the baseline
  every other tier falls back to.
- **Tier 1** (`reloop` Cargo feature; `crates/jade-vm-jit/src/reloop.rs`) —
  deps: `cfg-traits` + `ssa-reloop2` + `arena-traits` only, deliberately no
  SWC/`jsaw-core`. A thin `cfg_traits::Func`/`Block`/`Term`/`Target` adapter
  wraps Tier 0's discovered blocks; `ssa_reloop2::go()` structures them into a
  `StructuredBlock`; a walker emits real `while`/`if`/`switch`/labeled
  `break`/`continue`/`return` instead of Tier 0's flat dispatch loop. Enabled
  via `Config.prefer_reloop`.
- **Tier 2** (`crates/jade-vm-jit-swc`, full `jsaw-core`) — reconstructs a
  real `portal_jsc_swc_cfg::Func` (each block's straight-line ops generated as
  JS text via `jade-vm-jit::ops_to_js` — the same per-op emission Tier 0/1
  use — then re-parsed into real `Stmt`s; terminator operands built directly
  as `Expr` nodes), runs it through `jade-cfg-opt::optimize_tfunc` (via the
  existing `Func -> TFunc -> Func` conversions in `portal-jsc-swc-tac`), then
  lets `swc-cfg`'s own `Cfg::process_block` + `ssa-reloop2` (the *same*
  Stackifier Tier 1 uses) produce genuinely "proper" structured JS, serialized
  via `swc_ecma_codegen`.

All three carry `Config.add_async`/`add_gen` (ambient capability upgrades —
tested end-to-end in Tier 2 via `add_gen_config_reaches_yield_ops`,
`add_async_config_reaches_await_ops`, and the doubleGen-combination test
`nested_gen_fn_runs_via_tier2_under_ambient_add_gen`) and `Config.tenant_methods`
(inlines `GET`/`SET` instead of calling through `tenant.get`/`tenant.set`, via
the same `ops_to_js` per-op emission all three tiers share).

### Nested closures recurse through the *active* tier

A nested `Fn` op's own body is compiled through whichever tier is compiling
the *enclosing* function, not silently downgraded to Tier 0. `op_fn`
(`crates/jade-vm-jit`) consults `Config.nested_body_compiler` — an injectable
hook, since Tier 2 (a separate crate depending on `jade-vm-jit`, not the
reverse) can't be called directly from `op_fn` without an illegal reverse
dependency; Tier 2's `compile()` sets it to recurse into its own CFG
reconstruction. Tier 1 doesn't need the hook: `Config.prefer_reloop`
propagates by `Clone` into every nested `JsJit`, and `op_fn` checks it
directly (both live in the same crate) — though `reloop::compile` itself
must force `prefer_reloop = true` on entry, since a caller reaching it
directly (bypassing `jade_vm_jit::compile`'s own dispatch, the only other
place that flag is normally set) would otherwise silently get Tier-0-emitted
nested closures. See `nested_fn_with_branch_is_reconstructed_by_tier1`/`_tier2`
for the tests locking this in (a nested function whose own body has real
control flow, proving genuine reconstruction — real `if`, no `switch (__ip)`
— rather than a downgrade that happens to still execute correctly).

Tier 2's `TFunc`/SSA round-trip (needed to reconstruct a nested body) has a
real bug worth knowing about: converting the *re-parsed* AST of Jade's own
per-op-emitted text hoists a bare `var <name>;` for **every** distinct
identifier referenced — not just Jade's own `v{n}`/`$v{n}`/`$k{n}p{n}`/`cff`
temporaries, but any free/external identifier too, including true JS globals
like `Reflect`/`Symbol`. Left uninitialized, that hoisted `var` shadows the
real outer binding with `undefined`, breaking e.g. every `Reflect.apply(...)`
call a `CALL` op emits the moment the reconstructed function actually runs —
previously unnoticed since no Tier 2 test combined a `CALL` op with actual
Node execution before nested-closure support needed one. Worked around at the
text level (`jade-vm-jit-swc`'s `strip_free_identifier_hoists`, applied to
every `compile_body` call) rather than in the vendored `portal-jsc-swc-tac`/
`-ssa` crates, which is out of scope here.

### Relooper bugs found and fixed at the source

Both Tier 1 and Tier 2 ultimately drive `ssa_reloop2::go()` (Tier 2 via
`swc-cfg`, which uses the identical algorithm). Getting their tests to
actually *execute* correctly (not just parse) surfaced three real,
reproducible Stackifier bugs, all fixed directly in the dependencies per an
explicit go-ahead to patch `ssa-reloop2`/`swc-cfg` rather than work around
them in the JIT layer:

1. **Missing self-gating in `swc-cfg::Cfg::process_block`'s `Simple` case.**
   It emitted a block's own statements/terminator unconditionally instead of
   gating them behind the `cff` control-flow-flag local it already threads
   through every branch resolution. Combined with bug 2 below, this let a
   block's content run when it shouldn't have — e.g. two independent
   `return`-terminated blocks chained as if sequential, where the first
   `return` silently prevented ever reaching the second. Fixed the same way
   `jade-vm-jit`'s Tier 1 already did: every block's own content wrapped in
   `if (cff === "<label>") { ... }`, `cff` seeded to the entry block's label
   in `From<Func> for Function`. (`jsaw-core/crates/swc-cfg/src/lib.rs`)
2. **Contiguous-rpo-position block partitioning in `ssa-reloop2::build`.**
   Both the loop body/rest split and `partition_branches`'s per-branch region
   used a positional range (`rpo_slice[..=last_owned_pos]`) to decide block
   membership. Postorder can freely interleave unrelated blocks between a
   loop's body and its own latch (the block whose back edge closes the loop),
   or between two branches' respective descendants — a positional cutoff then
   silently drops or misplaces blocks that dominance says truly belong
   somewhere else. Most seriously, a loop's own latch could end up excluded
   from the loop's body slice entirely, so its `continue`/`break` back into
   the loop got emitted as ordinary code *outside* the loop — a syntax error
   (`continue`/`break` referencing a label that doesn't lexically enclose the
   statement), not just a wrong result. Fixed by partitioning by actual
   dominance-based ownership instead of position.
   (`codegen-utils/crates/ssa-reloop2/src/lib.rs`)
3. **Trivial self-domination misclassifying shared reconverge points.** A
   block that's a *direct* successor of one branch but *also* reachable from
   another (e.g. code after a loop that both "skip the loop entirely" and
   "the loop's own exit" flow into) was still treated as exclusively owned by
   whichever branch named it directly, since a block trivially dominates
   itself. Consumers that lower a `Multiple` node to a `switch` then emit it
   as its own `case` sitting next to the loop's case — but a JS `switch`
   never re-dispatches after a case is chosen, so transitioning to that case's
   `cff` value from *inside* the loop's own case never reaches it. Fixed by
   additionally checking, for a block that IS one of the branch targets,
   whether every other predecessor it has is internal to its own dominated
   subtree; if not, it's a shared reconverge point and goes to the tail
   instead of becoming its own case. (`codegen-utils/crates/ssa-reloop2/src/lib.rs`)

Tier 2 additionally keeps a `validate_labels` defensive backstop (reject any
`break`/`continue` whose label doesn't lexically enclose it) so it fails
loudly rather than ever emitting broken JS, even though the known cause is
now fixed upstream — consistent with the "never a silent miscompile"
philosophy used throughout (see `jade-vm-frontend`'s `Unsupported` errors).

## Verification

- Unit tests per crate exercise `JMP`/`CONDJMP`/`SWITCH` directly (both raw,
  hand-encoded bytecode in `jade-vm-jit`'s `tests` module, and end-to-end via
  `jade-vm-frontend`'s real-Node-execution tests).
- `crates/jade-vm-e2e-tests`: real headless-Chrome `wasm-bindgen-test` suite.
- `packages/jade-js/trap.e2e.ts`: tenant trap invocation via Node.
- Every stage above is covered by `cargo test --workspace` both with and
  without the `reloop` feature; the sibling `jsaw-core`/`codegen-utils`
  patches are covered by those repos' own test suites (notably
  `jsaw-core`'s `swc-test-harness` roundtrip suite, which exercises the
  shared-reconverge case from bug 3 above independently of Jade).

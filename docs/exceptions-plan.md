# Exception support plan — native exceptions, bytecode exception regions

Status: draft (surveyed against the tree; nothing implemented yet).

Goal: guest `throw` / `try` / `catch` end-to-end on every tier, with the exception
*mechanism* riding the host's own channel — real JS exceptions in the JS backends
(TS interpreter, JIT tiers, WASM glue) and `Result::Err` in the Rust backends
(`jade-vm-native`) — and exception *regions* carried in the bytecode, derived from
the per-block catch metadata `jsaw-core` already computes.

Non-goals (explicitly deferred, each with a recorded owner):

- Guest-visible error *classes* (`TypeError` primordial, `instanceof TypeError`
  inside guest code, `Error#stack`). That is realm-primordial work (Phase 4 of
  `docs/test262-plan.md`), not exception-mechanism work.
- `try/finally` correctness on *exceptional* and *early-exit* paths (see
  "The jsaw-core finally gaps" — the upstream lowering currently only covers
  fall-through). Fixing that is external-repo work in jsaw-core, gated on user
  review like every jsaw-core push.
- Full structured `try/catch` reconstruction in Tier 1 (see "Tier 1" below —
  initial support falls back to Tier 0's dispatch shape for try-containing
  functions).

## 1. The metadata jsaw-core already provides

Exception regions are **not** new information we must invent in the frontend —
they are a re-encoding of metadata that exists at every stage of the jsaw-core
pipeline:

- **`swc-cfg`**: every block carries `end.catch: Catch`
  (`crates/swc-cfg/src/lib.rs:667`) — `Catch::Throw` (propagate to caller) or
  `Catch::Jump { pat, k }` (bind the exception to `pat`, jump to handler block
  `k`). The `Stmt::Try` lowering (`crates/swc-cfg/src/to_cfg.rs:427`) assigns the
  try body's ambient catch to *every* block genuinely inside the try
  (`ToCfgConversionCtx::new_block` → `trap_catch`, `to_cfg.rs:669-697`), plus an
  explicit `trap_catch` on the try body's inherited entry block. Nested `try`
  nests naturally (the ambient catch of the inner body's blocks is the inner
  handler).
- **`swc-tac`**: the same shape as `TPostecedent.catch: TCatch`
  (`crates/swc-tac/src/lib.rs:1316,1352`), plus `TTerm::Throw(I)` for explicit
  `throw` (`lib.rs:1400`).
- **`swc-ssa`**: the SSA round-trip *preserves* catch edges:
  `SCatch::Just { target }` passes the exception as the handler block's parameter
  (`crates/swc-ssa/src/lib.rs:760-790`), and `rew.rs:113-131` maps it back to
  `TCatch::Jump`, synthesizing a `$error` pat ident **inserted into `cfg.decls`**.
  So on the post-optimization TFunc the frontend lowers, the catch binding is an
  ordinary declared ident — `slot_for(pat)` Just Works.
- **Structured emission exists**: `swc-cfg`'s `Cfg::process_block`
  (`crates/swc-cfg/src/lib.rs:~308-338`) already turns `Catch::Jump` into a real
  `Stmt::Try`/`CatchClause` that assigns the caught value to `pat` and continues
  to `k`. Tier 2 therefore gets *native JS `try/catch` in its output* for free
  once catch edges reach its CFG (see §4.4).

### The jsaw-core finally gaps (honest scope boundary)

`Stmt::Try` lowering places the finalizer only on the fall-through join block:

- `try { throw x } finally { f() }` with no handler: the throwing block's catch is
  the *outer* ambient catch; `f()` never runs.
- `try { return 1 } finally { f() }`: `Stmt::Return` terminates the block
  directly; `f()` never runs. Same for `break`/`continue` out of a try.
- With both handler and finalizer: normal and *caught* paths run the finalizer,
  but an exception thrown *inside the catch handler* skips it.

Consequence: this plan delivers full `try/catch`/`throw`/rethrow semantics and
fall-through `try/finally` semantics. Finally-on-exceptional/early-exit paths
need an upstream jsaw-core change (catch-all trampoline + exit interception),
listed as Phase 5 below and gated on user review per `AGENTS.md`. Tests depending
on it fail honestly and are recorded in the expectations ratchet.

## 2. Bytecode encoding: `THROW`, `TRYPUSH`, `TRYPOP`

Three new opcodes in `packages/jade-data/index.ts` (ids continue from 24):

| Opcode    | Id | Layout                            | Kind        |
|-----------|----|-----------------------------------|-------------|
| `THROW`   | 25 | `[LSB val]`                       | loop-level  |
| `TRYPUSH` | 26 | `[raw catch_slot][raw handler_ip]`| loop-level  |
| `TRYPOP`  | 27 | (no operands)                     | loop-level  |

`TRYPUSH`/`TRYPOP` are the **exception regions**: inline, balanced delimiters in
the instruction stream. `TRYPUSH` enters the region whose handler starts at
absolute byte offset `handler_ip` (patched exactly like existing jump targets)
and whose catch binding is state slot `catch_slot`; `TRYPOP` exits it. Regions
may nest; the runtime handler stack is per-frame.

**Emission rules** (frontend, §3): for each lowered block whose `post.catch` is
`TCatch::Jump { pat, k }`:

1. emit `TRYPUSH(slot_for(pat), target(k))` at block entry (before the block's
   statement ops, and before the entry block's `GLOBAL` op — the region covers
   the whole block);
2. emit `TRYPOP` immediately before the block's terminator **unless the
   terminator is `TTerm::Throw`** — a `throw` must see its own region's handler
   (`try { throw x } catch (e)` catches), so the region is popped by the
   *dispatch* instead (see below). All other currently-supported terminators
   (`Jmp`/`CondJmp`/`Return`/`Default→Ret`) cannot throw, so popping first is
   correct region exit. Caveat recorded: when `TTerm::Tail` is ever lowered, its
   call can throw — its `TRYPOP` placement needs the same treatment as `Throw`.

**Runtime semantics** (interpretive tiers): each frame maintains a handler stack
of `(catch_slot, handler_ip)`. Any op failing — a tenant op raising a host
error, `THROW`, a guest call propagating — pops the top entry; if the stack is
empty the exception leaves the frame (propagating to the guest caller's frame,
whose own dispatch consults *its* stack); otherwise the driver writes the
exception value to `state[catch_slot]` and resumes at `handler_ip`. A handler
that catches is consumed (popped) by the dispatch; a handler region re-entered
normally is pushed again by its own `TRYPUSH`. Stack discipline is balanced by
construction: every region entered pushes once and exits either via its `TRYPOP`
(normal) or the dispatch pop (exceptional).

**Why inline markers and not a header region table.** The alternative — one
prologue op carrying `[raw n][n×(start, end, handler, slot)]` (JVM-style) — was
considered and rejected for now: it needs cross-block range coalescing (blocks of
one try body are not guaranteed contiguous in layout order), a measure-phase
fixpoint for the table's own size, and explicit threading through the tiers'
recursive `nested_body_compiler` (nested `FN` bodies are inline in the same flat
stream, so a global table must be re-sliced per function). Markers preserve the
flat-stream invariant: every consumer that can parse ops can parse regions, and
the recursive tier compilers see them with zero extra plumbing. Markers also
carry exactly the information a table would, so a later migration is a frontend
change only.

`LOOP_LEVEL` in `scripts/gen/shared.ts` gains the new args kinds (`"trypush"`,
`"none"`), so `jade-vm-core`'s generated `exec_op` keeps returning `Err` for all
three — every driving loop implements them, like the existing loop-level ops.
Regen per the established flow: `sh ./build.sh` + `rustfmt` on
`crates/jade-vm/src/data.rs` and `crates/jade-vm-core/src/dispatch.rs`.

## 3. Frontend (`crates/jade-vm-frontend`)

- `targets_of` (`lib.rs:511`): `TTerm::Throw(_)` moves from
  `unsupported("`throw` …")` to `vec![]` (it transfers to no block — the catch
  edge is *not* a terminator target).
- `emit_terminator` (`lib.rs:537`): `TTerm::Throw(id)` →
  `Operation::Throw(self.operand_for(id))`. (The current
  "internal invariant: unreachable TAC terminator" arm shrinks accordingly.)
- `discover_reachable` (`lib.rs:492`): the BFS must **also traverse catch
  edges** — `TCatch::Jump { k, .. }` pushes `k` onto the worklist. Without this,
  handler blocks are unreachable and never lowered (this is the easiest place to
  break the whole feature silently).
- The block loop's `try/catch` rejection (`lib.rs:455`) is removed; in its place
  the block loop applies the §2 emission rules, with `handler_ip` patched through
  the existing `offsets` map in phase 2 (TRYPUSH is fixed-size
  2 + 4 + 4 = 10 bytes, TRYPOP 2 bytes — the measure phase accounts for both).
- Already-correct pieces, verified during survey: `check_no_captures`'s
  `own_refs` already counts `TTerm::Throw` refs (`lib.rs:262`); the globals
  pass's `DeclCollector` already treats catch params as declared; the catch
  `pat` is a `cfg.decls` member post-optimization (§1), so `slot_for(pat)` needs
  no special-casing.
- Nested functions: their blocks carry their own `post.catch`; the recursive
  lowering emits their markers inline in their region of the stream. Region
  membership is by execution, not by byte range, so a caller's `CALL` op inside
  its own region is unaffected by the callee's markers.

Unit tests (mirroring the existing frontend suite): `throw` compiles and runs;
`try/catch` binds the thrown value; rethrow from a handler reaches an outer
handler; an uncaught throw propagates out of a nested `FN` call to the caller's
handler; `try/finally` fall-through runs the finalizer (and, documented, an
uncaught `throw` under the current upstream lowering does not — pinned as an
`#[ignore]`-style or expectations-recorded gap, not silently miscompiled… if the
miscompile risk is unacceptable the frontend keeps *rejecting* `TryStmt` with a
finalizer until Phase 5 — decide at implementation time, defaulting to:
**reject finalizers** for now, since "honest ratchet signal over lucky wrong
answers" is the established policy).

## 4. Backends

### 4.0 Exception representation, per backend (the "native exceptions" rule)

| Backend | Guest `throw v` | Tenant/host failure | Guest `catch (e)` receives |
|---|---|---|---|
| TS interp / JIT tiers | `throw v` (JS) | host JS exception | the value/error as-is |
| WASM | `wasm_bindgen::throw_val(v)` | JS exception, caught via `catch` imports | the JsValue as-is |
| native (`jade-vm-native`) | `Err(NativeError::GuestThrow(v))` | `Err(TenantError)` | guest value as-is for `GuestThrow`; for `TenantError`, a synthesized guest object `{ name: "TypeError"|"RangeError", message }` (documented approximation until error primordials exist) |

The last column is the only *semantic* divergence between backends, and it is
unavoidable without error primordials; it is recorded here and in the manifest
rather than smoothed over.

### 4.1 TS interpreter (`packages/jade-data` templates + `scripts/gen/vm-ts.ts`)

- The generated driving loop gains a per-frame handler stack declared with the
  loop state, and the `switch` is wrapped:
  `for(;;){ try { …switch… } catch (__e) { const h = __handlers.pop(); if (h === undefined) throw __e; state[h[0]] = __e; ip = h[1]; } }`.
- Handler templates: `THROW`: `{ const v=arg(); throw v; }` —
  `TRYPUSH`: `{ __handlers.push([code().getUint32(ip,true), code().getUint32(ip+4,true)]); ip+=8; break; }` —
  `TRYPOP`: `{ __handlers.pop(); break; }`.
- **Suspension**: the variant-handoff context object (`vm-ts.ts`'s `parameters`
  line) gains `handlers: __handlers` so a sync frame that suspends into the
  async/generator variant keeps its regions. An `await` rejection inside the
  async variant throws *inside the same loop* and is dispatched by the same
  `catch` — `try { await x } catch {}` works. (`yield` suspensions: the stack
  lives in the generator frame's locals and persists across `yield` for free.)

### 4.2 Tier 0 (`crates/jade-vm-jit`, block-dispatch)

- Per-op emission (the shared `ops_to_js` source): `op_throw` →
  `throw {val};`, `op_trypush` → `__handlers.push([{slot}, {handler_ip}]);`,
  `op_trypop` → `__handlers.pop();`.
- The emitted body prologue gains `const __handlers = [];` and the dispatch
  structure becomes `while(1){ try { switch(__ip){…} } catch(__e){ const h = __handlers.pop(); if(!h) throw __e; state[h[0]]=__e; __ip=h[1]; } }`.
- `__handlers` and `__e` don't collide with the temp-name families
  (`v{n}`, `$v{n}`, …) and `strip_free_identifier_hoists` only strips *hoisted
  var* declarations of free idents — a local `const` in the body is unaffected.

### 4.3 Tier 1 (`crates/jade-vm-jit`, reloop) — contained fallback

`ssa-reloop2` restructures the dispatch into real `if`/`while`; after that there
is no `__ip` to jump to, so the §4.2 dispatch-catch cannot be wrapped around
relooped code. Lifting catch edges into the relooper's own graph is external
work (`cfg-traits`/`ssa-reloop2` in codegen-utils — Phase 5). Until then:
**a function whose bytecode contains `TRYPUSH` falls back to Tier 0's dispatch
shape for that function only** (nested `FN` bodies decide independently, since
each recurses through the active tier). This is honest containment: Tier 1 keeps
its structured output for the (overwhelmingly common) try-free case and stays
semantically correct everywhere. Documented in the crate docs and the test262
README.

### 4.4 Tier 2 (`crates/jade-vm-jit-swc`) — contained fallback (same as Tier 1)

> **As-implemented update (phase 2):** the catch-edge lifting described below was
> built and validated for *minimal* regions, but real functions blow up inside
> `swc-ssa`'s catch shim: it carries the function's *entire* ident set as block
> params across every region edge (`conv.rs`'s `safe_to_carry` prune is a stub
> returning `false`), producing superlinear phi storms (a 25KB fixture → 17MB of
> emitted JS, ~60s compile) — and handler-body reads of the catch binding came out
> unbound because `to_cfg` never inserts the catch param into `cfg.decls` (jade
> patches around that one in the frontend: `declare_catch_pats`). So Tier 2 takes
> the same fallback as Tier 1: a function whose bytecode contains `TRYPUSH`
> compiles to Tier 0's dispatch shape for that function only (nested bodies decide
> independently via `with_nested_body_compiler`). A `THROW` with no regions still
> lifts to a real JS `throw` via `Term::Throw`. The lifting machinery landed in
> `build_cfg_func`'s git history and returns in phase 5 with the upstream fixes.
>
> Phase 2 also landed one upstream fix (in the locally-patched jsaw-core checkout,
> awaiting review): swc-cfg's `process_block` now emits a `Throw` *terminator*
> inside the block's try wrapper when the block has a catch edge — it previously
> landed outside, so `try { throw x } catch (e) {}` never caught its own throw.

Tier 2's output must keep real structure (the "clean code tier" rule), so it
does the inverse of the frontend: it *consumes* the markers as region metadata
and reconstitutes CFG catch edges.

- `discover_blocks`/`build_cfg_func` (`lib.rs:318`): while constructing
  `CBlock`s, maintain a region stack; `TRYPUSH`/`TRYPOP` ops are intercepted
  (they never reach `ops_to_stmts`), and each block inside a region gets
  `end.catch = Catch::Jump { pat: <synthetic ident per region>, k: <handler
  block id> }` instead of today's unconditional `Catch::Throw` (`lib.rs:347`).
  Handler ips are block boundaries already (they target lowered TAC blocks), so
  `offset_to_id` resolves them.
- `THROW` becomes `end.term = Term::Throw(…)` like `RET` becomes `Term::Return`.
- At each handler block's entry, synthesize one statement
  `state[slot] = <pat ident>` so the interpreter-model binding materializes in
  the structured world; the existing TFunc→optimize→codegen path then emits a
  **native JS `try { … } catch (e) { … }`** via `swc-cfg`'s `process_block`
  (§1), with no dispatch loop and no handler stack in the output.
- Nested `FN` bodies recurse through `with_nested_body_compiler` as today; the
  region stack is per-function.

### 4.5 WASM (`crates/jade-vm-wasm`)

- **Bug fix folded in**: `tenant_call`'s
  `Reflect::apply(…).unwrap_or(JsValue::UNDEFINED)` *silently swallows* every
  exception a tenant method raises (this is exactly the known
  `S7.8.3_A4.1_T{1,2,7,8}` wasm-interp delta class). Exception support requires
  a catch-aware call path: an imported JS helper
  (`#[wasm_bindgen(catch)] fn jade_try_apply(f, this, args) -> Result<JsValue, JsValue>`,
  or per-call `catch` on the tenant-method imports) so a host exception arrives
  in Rust as `Err(JsValue)` instead of vanishing.
- `run_sync` gains `handlers: Vec<(u32, u32)>`; `TRYPUSH`/`TRYPOP` maintain it;
  `Operation::Throw(v)` → `wasm_bindgen::throw_val(resolve(v))`; any `Err` from
  the tenant path pops a handler (`state[slot] = e; ip = handler_ip;`) or
  rethrows with `throw_val`.
- Async/generator variants (`run_async_internal`, gen paths): region dispatch on
  promise rejection is **deferred** (recorded gap; sync path is complete).
- Side effect: with exceptions no longer swallowed and `assert.throws` available
  (§5), the 4 known wasm-interp expectation deltas flip to match `interp`.

### 4.6 Native (`crates/jade-vm-native`)

- `NativeError` gains `GuestThrow(Value)`; `Operation::Throw` returns it from
  the driving loop; `run_sync` maintains `handlers: Vec<(u32, u32)>`.
- The `exec_op`/`pending` error path and `op_call`'s `Err` consult the handler
  stack *first*: pop → bind → resume, else propagate. A `GuestClosure::apply`
  that ends in an uncaught exception returns the same `Err`, so exceptions cross
  guest frames through `tenant.invoke` exactly like the JS backends.
- `TenantError` materialization on catch-binding per §4.0.

## 5. test262 integration

- **Harnesses**: `packages/jade-js/test262/harness.ts` and
  `crates/jade-test262/src/native.rs` both gain `assert.throws(expectedCtor, fn, msg)`:
  invoke `fn`; if no exception, `Test262Error`; on exception, check the
  constructor *approximately* without error primordials — host side, compare
  `e?.constructor === ctor` or `e?.name === ctor?.name`; native side, match the
  `TenantError` variant name or the thrown guest object's `name` own-property.
  The approximation is recorded in the manifest note. (`$DONE` stays deferred
  with async cells.)
- **Negative runtime-phase tests**: today they are uniformly
  `skip-unsupported-frontend`. With `throw`, a `negative: {phase: runtime}`
  test **passes when its cell threw a matching error**: `CellVerdict` gains an
  optional `errorName` (host `e.name`; `TenantError` variant name; absent for
  guest-thrown non-error values), and `run.rs`/`run.ts` reclassify
  `fail "threw during execution"` as `pass` iff `errorName === meta.negative.type`.
  Parse/early classification is unchanged.
- **Manifest**: remove `"try/catch (Jade bytecode has no exception-handling
  opcode)"` and the `` `throw` `` entry as each tier lands (single removal once
  the frontend stops emitting them; tier-specific failures surface as ordinary
  fails first).
- **New shard dirs**: `language/statements/throw` and `language/statements/try`
  become the Phase-4 target dirs across **all six environments**
  (`interp, wasm-interp, jit-t0, jit-t1, jit-t2, native`), with the same
  done criteria as Phase 2: zero `differential-mismatch`, per-env pass-rate
  parity, two consecutive stable `--check` runs, re-baselined expectation files
  (expect a large flip: most `try`-shaped tests currently skip).
- The smoke shard gains `try-catch.js` / `throw-rethrow.js` / `try-finally.js`
  raw fixtures in Phase 0 so every tier proves the mechanism before real
  test262 tests depend on it.

## 6. Invariants this preserves

- **Tenant ABI**: no change to `guestAbiMixin`, `TENANT_METHOD_NAMES`, or the
  driver protocol. Tenant ops keep failing the way they fail today (host
  exception / `Result::Err`); the *consumers* of that channel are what change.
  Spliced tenant methods inherit the caller's region for free — they are
  inlined into code the region already covers.
- **Single per-op emission**: Tier 0/1's `op_throw`/`op_trypush`/`op_trypop`
  live in `jade-vm-jit`'s per-op emission, the one place each op's JS exists;
  Tier 2 intercepts the markers structurally before per-op emission, which is
  the same relationship it already has with the other loop-level ops.
- **Tier 2 stays a clean-code tier**: its output contains native `try/catch`
  and no residual dispatch machinery.
- **Flat bytecode stream**: no container/header; regions are ordinary ops.
- **Generator-driver protocol**: `driveTenant` composition is unchanged; the
  exception channel is *above* it (a drive that raises is a failed op).

## 7. Phased rollout

Commit after each phase (`[AI]` convention).

**Phase 0 — bytecode + frontend + TS interpreter.**
Opcodes + regen; frontend lowering (incl. catch-edge reachability);
vm-ts.ts loop + templates + context handoff; smoke fixtures
(`try-catch.js`, `throw-rethrow.js`, `try-finally.js`); frontend unit tests.
*Done when:* smoke passes on `interp`; `compile --file` on a try script emits
the expected markers; full workspace builds and prior suites stay green.

**Phase 1 — Tier 0 + Tier 1 (fallback).**
Per-op emissions; Tier 0 dispatch wrapper; Tier 1 region detection + per-function
fallback; `jit-t0`/`jit-t1` smoke parity.
*Done when:* smoke passes on all three Node JIT envs with zero mismatches.

**Phase 2 — Tier 2 catch-edge lifting (landed as the contained fallback).**
`build_cfg_func` region stack, `Term::Throw`, handler-entry materialization;
`jit-swc` release tests incl. a try/catch execution test.
*Done when:* smoke passes on `jit-t2`; emitted output for a try script contains
a real `try`/`catch` and no handler stack (asserted in a test).
*Actual:* smoke passes on `jit-t2` via the Tier-0-shape fallback; the lifting
exists in git history and is gated off until the phase-5 upstream fixes
(swc-ssa catch-shim state pruning + entry-state snapshot semantics).

**Phase 3 — WASM + native.**
Catch-aware tenant calls (swallow fix); wasm `run_sync` regions; native
`GuestThrow` + handler stack + `TenantError` materialization.
*Done when:* smoke passes on `wasm-interp` and `native`; the
`S7.8.3` wasm delta flips to interp parity in a re-run (re-baseline).

**Phase 4 — harness + dirs + baseline.**
`assert.throws` in both harnesses; negative-runtime reclassification
(`errorName`); `language/statements/{throw,try}` on all six environments;
re-baseline every expectation file; remove the manifest entries.
*Done when:* both new dirs meet the Phase-2 parity criteria and
`--check` is stable twice on all six envs.

**Phase 5 — external follow-ups (user review required, separate repos).**
jsaw-core: finally-on-exceptional/early-exit lowering (catch-all trampoline +
exit interception); `swc-ssa` catch-shim state pruning (`safe_to_carry` is a
stub returning `false`, so the shim carries every ident — superlinear phi
storms, 17MB of JS for a 25KB fixture) and its entry-state snapshot semantics
(values written inside a protected block before a throwing op must be visible
to the handler; jade's frontend skips the SSA round-trip for try-containing
functions until then). codegen-utils: catch edges in `cfg-traits`/`ssa-reloop2`
so Tier 1 can restructure try regions instead of falling back. Land each behind
the usual review-and-pin flow; until then the plan's documented gaps stand.

## 8. Risks / open questions

- **Region/block-boundary alignment**: `handler_ip` must always land on a
  bytecode-block start in Tier 2's `discover_blocks`. True by construction
  (handlers are TAC blocks lowered at block starts), but an explicit debug
  assertion in `build_cfg_func` is cheap insurance against a future frontend
  layout change.
- **`TRYPOP`-before-terminator correctness** hinges on current terminators being
  non-throwing; the `Tail` caveat is recorded in §2, and the frontend should
  `debug_assert` it rather than rely on the comment.
- **Suspension mid-region** in WASM (deferred) means `try { await }` under
  `wasm-interp` async cells will mis-dispatch; async-flagged tests are already
  uniformly skipped, so no ratchet noise, but the gap must stay visible in the
  README until closed.
- **Handler-stack as `const` in Tier 0 bodies**: verify Tier 1's relooper and
  Tier 2's hoisting pass never see it (Tier 1 falls back, Tier 2 intercepts) —
  covered by construction, but the Phase 1/2 tests should pin it.

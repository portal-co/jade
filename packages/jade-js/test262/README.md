# Jade test262 runner

Implements `docs/test262-plan.md`. Reads: the plan first, then this file.

## Layout

- `manifest.json` — the capability manifest: the single source of truth for every
  `skip-feature`/`skip-flag`/`skip-unsupported-frontend` verdict the runners emit.
- `expectations/<env>.json` — the ratchet: only non-`pass` verdicts are named; anything
  unlisted is expected to pass.
- `fixtures/smoke/` — synthetic test262-format fixtures used by the `smoke` shard (raw
  tests needing no harness, so the pipeline is provable before harness primordials exist).
- `reports/` — run artifacts (gitignored).
- `exec.ts` / `driver.ts` — the single execution path: fresh tenant + primordial realm
  per cell, then the interpreter or a JIT body. `driver.ts` is the CLI the Rust
  orchestrator spawns per cell.
- `run.ts` — the TS orchestrator (delegates compilation to the Rust CLI).

## Commands

```sh
# Rust orchestrator (plan's Phase-0 criterion):
cargo run -p portal-solutions-jade-test262 -- --shard smoke

# Multi-env runs (jit-t*) belong in release mode — Tier 2's CFG/SSA round-trip is
# ~10x slower unoptimized:
cargo build --release -p portal-solutions-jade-test262
./target/release/jade-test262 --shard test262:language/literals

# TS orchestrator:
node --experimental-strip-types packages/jade-js/test262/run.ts --shard smoke

# Compile oracle for one file (frontmatter + bytecode + all JIT tiers, as JSON):
target/debug/jade-test262 compile --file vendor/test262/test/language/literals/boolean/S7.8.2_A1_T1.js

# Validate a report against the schema invariants:
target/debug/jade-test262 validate-report packages/jade-js/test262/reports/smoke.json

# Ratchet:
... --shard smoke --check                  # fail on drift vs expectations/
... --shard smoke --update-expectations    # re-baseline (review the diff!)
```

Shards: `smoke` (the fixtures) or `test262:<subdir>` (a vendored subtree under
`vendor/test262/test/`). Environments: `interp`, `wasm-interp`, `jit-t0`, `jit-t1`,
`jit-t2`, `native` (`--env a,b` to subset). Tenants: `--tenant multi` (default) or
`single`.

`wasm-interp` (Phase 3) executes the same bytecode through `jade-vm-wasm`'s generated
`run_virtualized` inside Node, against the same TS tenant + primordial realm as
`interp`. Its wasm-bindgen bundle lives at `pkg/jade_vm_wasm.js` (gitignored); the
runners build it on demand via `wasm-pack build --target nodejs crates/jade-vm-wasm`
when a run names the cell, and `--update-wasm` forces a rebuild after editing
`jade-vm-wasm`. Its ratchet file (`expectations/wasm-interp.json`) is seeded from the
Phase-1/2 subset. Since the exception-opcode phase-3 work (`docs/exceptions-plan.md`),
tenant calls are catch-aware rather than swallowing exceptions to `undefined`, so the
former `S7.8.3_A4.1_T{1,2,7,8}.js` delta class is gone: wasm-interp matches `interp`
verdict-for-verdict and value-for-value on the whole Phase-1 subset. Those four tests
then flipped to an honest uniform fail in phase 4 (`assert.throws: function did not
throw`): reading an *undeclared* global yields `undefined` via the globals pass
instead of throwing `ReferenceError` — a documented modeling gap in the globals
design, not an exception-mechanism issue.

`native` (the plan's native-Rust cell) executes the same bytecode entirely in-process:
`jade-vm-native`'s sync interpreter drives `jade-tenant-rt`'s ObjectManager with the
primordials `jade-primordial-rt` generates (no Node, no JS engine in the process). The
realm mirrors `createPrimordialRealm` minus Promise and the buffer family (not
generated; the interpreter is sync-only), and `crates/jade-test262/src/native.rs`
installs the same `sta.js`/`assert.js` harness surface as `harness.ts`. The one
deliberate divergence: the native harness *returns* `TenantError("Test262Error: …")`
from a failed assertion instead of throwing a host error (Rust has no host
exceptions), and the cell classifies the prefix back into `fail "assertion failed"`.
Its ratchet file (`expectations/native.json`) is seeded from the Phase-1/2 subset and
matches the `interp` rollup verdict-for-verdict.

## Invariants

- A skip the manifest cannot justify fails the run — update `manifest.json` in the same
  PR as the capability change.
- `pass → fail` or a new unexplained skip in `--check` blocks the PR; `fail → pass`
  moves freely.
- `exec.ts` is the only place guest code runs: never call `.next()` on a tenant
  generator, never add ambient host intrinsics to a cell's scope.

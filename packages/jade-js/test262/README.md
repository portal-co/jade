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
`vendor/test262/test/`). Environments: `interp`, `jit-t0`, `jit-t1`, `jit-t2`
(`--env a,b` to subset). Tenants: `--tenant multi` (default) or `single`.

## Invariants

- A skip the manifest cannot justify fails the run — update `manifest.json` in the same
  PR as the capability change.
- `pass → fail` or a new unexplained skip in `--check` blocks the PR; `fail → pass`
  moves freely.
- `exec.ts` is the only place guest code runs: never call `.next()` on a tenant
  generator, never add ambient host intrinsics to a cell's scope.

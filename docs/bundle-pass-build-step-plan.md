# Plan: bundle pass as a build-time TypeScript source pass

## Implementation status (Phases 1-3 complete, validated against real source)

- **Phase 2** (`jade-swc-tenant-exposure` hardening): done. Parser is `Syntax::Typescript`;
  `transform_program` now requires a `ResolvedProgram` (compile-time-enforced resolve
  precondition, via new `resolve()`); `HelperCaptureFinder` compares bindings by resolved `Id`
  instead of `Atom` spelling, closing a real latent bug (an arrow function's own parameter
  shadowing a captured `const` name was silently mis-rewritten before this — now fails closed,
  proven by a regression test). 10/10 tests pass.
- **Phase 1** (`emit_ast` in `jade-primordial-ir`): done for every file that currently fully
  lowers — `object.ts`/`function.ts`/`reflect.ts`/`array-buffer.ts` — each verified by a genuine
  round trip (parse -> lower -> `emit_ast` -> print -> re-lower -> same IR) against the real
  file via `include_str!`, not a synthetic fixture. Along the way, `lower_module`'s signature
  was solidified from `Result<Module, IrError>` to `Result<LoweredModule, IrError>` (`{ module,
  skipped }`) — it previously had no structured way to say "I only partially lowered this file,"
  only an `eprintln!`, which is fine for `gen-primordials`' Rust-emission use case (nothing calls
  an omitted item) but unsafe for a caller shipping real TypeScript (a dropped function is a
  silent runtime `ReferenceError`, not a caught build error). This immediately caught a real,
  previously invisible case: `array-buffer.ts`'s `nativeBufferHooks` (uses `instanceof`) has
  always been silently dropped — a documented, deliberate exclusion, not a bug, but the crate had
  no way to distinguish "known, accepted drop" from "new, unexpected gap" before. 12/12 tests
  pass.
- **Phase 3** (`crates/jade-bundle-build` orchestrator): done. One in-process pipeline — try IR
  (`translate_source` + `emit_ast`), full fallback to a direct TS parse on any partial coverage,
  then `resolve` -> `transform_program` -> `swc_ecma_codegen` print — written to
  `packages/jade-js/.generated/`. Runs clean against all 41 real `.ts` files in the package.
  **Correction to this doc's original "Concrete targets" list below**: `BufferPrimordialImpl` is
  not actually a valid target — it isn't a `Tenant` implementation (its methods are
  `record`/`shell`, never `TENANT_METHOD_NAMES`) — removed from the hardcoded config.
- **Scope extension, driven by a real finding**: the first real run against `Tenant`
  (`tenants/multi.ts`) and `MergedTenant` (`tenants/merged.ts`) produced **zero**
  transformations — both route every tenant-operation method's private-field access through a
  private *helper method* (`#shadowForKey`, `#requireOwner`/`#marshal`) rather than touching
  `this.#field` directly, exactly the shape this doc's source design
  (`docs/swc-tenant-accessors-and-exposed-functions-plan.md`) originally listed out of v1 scope.
  Extended `jade-swc-tenant-exposure` to generate a public mangled **forwarding method** for a
  private instance method a selected public method calls directly — `[MANGLED](...args) {
  return this.#helper(...args); }` — and rewrite that call site to use it, reusing the exact
  same mangled-key/collision-checking machinery the field accessors already had. The forwarder's
  own body stays inside the class and is never spliced, so `#helper`'s own internals (further
  private fields, further private method calls) never need tracing more than this one level.
  Verified both by unit tests (a synthetic fixture mirroring `multi.ts`'s real shape, plus a
  fail-closed case for a private method referenced without being called) and against the real
  generated output: `Tenant.get`/`.set`/`.ownKeys` end up fully `PrivateName`-free — multiple
  private fields *and* private method calls widened within a single method — while unselected
  methods (`has`, `delete`, `getOwnPropertyDescriptor`, ...) stay untouched. Private accessors
  (`get #x()`/`set #x()`) remain explicitly out of scope. 10/10 tests pass (including the two
  new ones).
- **Phases 4-6, all done**:
  - **Two more real bugs found and fixed** by actually running the orchestrator against every
    real file and type-checking/executing the result, rather than trusting per-file unit tests
    alone:
    1. `lower.rs` erased a method-shaped `interface` (any `TsMethodSignature` member) or an
       unrepresentable `type` alias entirely — a pre-existing gap shared by `emit_ts.rs` and
       `emit_ast.rs` alike (the missing information lived in the IR, not either emitter). This
       left dangling references in regenerated TypeScript: `primordials/proxy.ts`'s `class
       ProxyStateImpl implements ProxyState` with no `ProxyState` declared anywhere,
       `primordials/array-buffer.ts`'s `BufferHandle`/`BufferHooks`/`BufferPrimordial`
       similarly missing. Fixed with a new `Item::VerbatimTypeDecl { name, is_exported, source
       }` — `source` is sliced by byte-offset span directly from the original file (not
       re-derived), so both emitters can restate the declaration verbatim instead of dropping
       it; Rust emission (which never referenced these names directly — `rust_type`'s
       same-module-class fallback already resolves them) is unaffected, confirmed by diffing
       regenerated `array_buffer.rs` against the checked-in `jade-primordial-rt` output (only
       difference: an unrelated, pre-existing closure-capture-ordering nondeterminism).
    2. `Item::FnDecl`/`Item::ClassDef` never tracked the original source's `export` keyword, so
       no regenerated primordial function/class (`objectPrimordial`, `BufferPrimordialImpl`,
       ...) could actually be imported by another file — harmless for Rust emission (never
       needed it) but fatal for a real cross-file TypeScript bundle. Fixed by adding
       `is_exported: bool` to both variants, threaded from `lower_decl`'s existing call sites.
  - The private-helper-method forwarder's `(...args)` call also needed `tsc` to accept
    spreading `args` into `#name`'s fixed-arity parameter list (TS2556). Typing the *rest
    parameter* (`: any` or `: any[]`) does **not** work — verified empirically, both still
    trigger TS2556, since the check is on the spread expression's own declared type and no
    array type (even `any[]`) counts as a tuple. Casting the *callee* to `any` instead
    (`(this.#name as any)(...args)`) does work. Gated behind a new
    `TenantExposureConfig::type_forwarder_args_as_any` (default `false`): the crate's original
    runtime call site (`transform_bundle_source` against already-stripped JS from a live
    `toString()`) must never inject TypeScript-only syntax into its output; `jade-bundle-build`
    sets it `true` since its output is real TypeScript `tsc` needs to accept.
  - **Full regression proof, against the real codebase, not fixtures**: `npx tsc --noEmit -p
    tsconfig.generated-check.json` (new, checked-in scoped tsconfig) is **clean — zero errors**
    — across all 41 regenerated files. All 7 real e2e suites (`tenant.e2e.ts`,
    `primordials.e2e.ts`, `trap.e2e.ts`, `tenant-compose.e2e.ts`, `exotic-tenant.e2e.ts`,
    `rewrite.e2e.ts`, `promise.e2e.ts`) **pass running directly against `packages/jade-js/
    .generated/`** via `node`, identically to hand-written source.
  - Unrelated environment fix required to get any of this running at all: the npm-published
    `@portal-solutions/semble-weak-map.factory@0.1.3` is missing the `./gc-hooks` subpath
    `packages/jade-js/gc.ts` imports (and `@portal-solutions/semble-gc` isn't published at
    all) — pre-existing, unrelated to this plan, blocked `tsc`/e2e execution even against
    unmodified hand-written source. Fixed via root `package.json`'s new `overrides` field
    pointing `@portal-solutions/semble-common` and `@portal-solutions/semble-weak-map.factory`
    at the local `../semble` clone (which has real, built `dist/gc-hooks.*`), per the user's
    explicit instruction; `../semble`'s own `npm install` resolves its further transitive deps
    (`@portal-solutions/semble-gc` as a sibling workspace, `@portal-solutions/hooker-snap` from
    npm) normally.
  - `packages/jade-js/package.json`'s `zshy.exports` now point at `.generated/*.ts` instead of
    hand-written source paths — the actual "what ships" swap flagged as needing explicit review
    when this plan was first written. `npm run regen` (new script) regenerates
    `packages/jade-js/.generated/`, which is checked into git (matching `jade-primordial-rt`'s
    existing generated-and-committed convention) so drift is visible in a normal diff; this
    repo has no CI configured at all yet (no `.github/`), so an automated "regenerate and diff,
    fail on drift" *CI job* specifically is not wired up — that's a separate decision (which CI
    provider, etc.) left for whenever this repo gets CI at all.

## Problem and intended result

`crates/jade-swc-tenant-exposure` (`transform_bundle_source`/`transform_program`,
see `docs/swc-tenant-accessors-and-exposed-functions-plan.md`) is designed today as a
**dynamic, collection-time** pass: an embedder obtains a live tenant class's or helper
function's source via `Function.prototype.toString()` reflection at runtime and feeds that
text through the transform before the JIT/frontend collects it
(`crates/jade-vm-frontend/src/tenant_inline.rs`'s doc comment: "as returned by
`SomeTenantClass.toString()` in a real browser"). Consistent with that, the crate currently
parses its input as plain JS — `Syntax::Es(Default::default())` at
`crates/jade-swc-tenant-exposure/src/lib.rs:122` — because reflected-and-stripped source at
runtime has no type syntax left to parse.

Separately, `jade-primordial-ir` already has a **build-time** convention for a different
purpose (translating `packages/jade-js/primordials/*.ts` into native Rust): it parses real
`.ts` source with `Syntax::Typescript` (`crates/jade-primordial-ir/src/lower.rs:47`) and can
re-emit TypeScript text (`emit_ts.rs`) — but that emission path is explicitly a
losslessness/verification check today, not a generation step:

> "IR -> TypeScript re-emission, used as a losslessness/verification check (the hand-written
> `packages/jade-js/primordials/*.ts` remains the canonical source; this is never checked in
> as a replacement...)" — `crates/jade-primordial-ir/src/emit_ts.rs:1-3`

This plan turns the tenant-exposure transform into a second build-time stage that consumes
primordial-ir's TypeScript output — **deliberately reversing the "never checked in as a
replacement" statement above** — composes it with the rest of hand-written
`packages/jade-js`, and produces a shipped, already-transformed bundle. The existing dynamic
runtime path is kept as-is for classes/functions an embedder supplies at its own runtime,
outside Jade's own build.

## Decisions already settled with the user

1. **Target scope:** the whole `packages/jade-js` package, not just the files
   `jade-primordial-ir` already touches. Both `packages/jade-js/primordials/*.ts` and
   `packages/jade-js/tenants/*.ts` (real `Tenant` implementations — see "Concrete targets"
   below) are in scope.
2. **Pipeline order:** the bundle pass runs **after** primordial-ir's TypeScript emission.
   `emit_typescript` output is promoted from verification-only to a real generation stage
   that feeds the bundle pass.
3. **Runtime fallback:** kept. The build step covers known, checked-in `jade-js` source; the
   existing dynamic `transform_bundle_source` runtime path stays available for tenant
   classes/helpers an embedder supplies at its own runtime, not statically known at Jade's
   build time.
4. **Selection input:** hardcoded in the new build binary (matching `gen-primordials`'
   current `--file` ad hoc style), not a source annotation or a separate config file.

## Concrete targets found in the codebase

The private-field tenant-accessor transform's motivating shape already exists verbatim:

- `packages/jade-js/tenants/multi.ts:38` — `export class Tenant implements Tenant_` has
  `#shadow`, `#keys`, `#owned`, `#extensible`, `#exotics` — `#shadow` is the literal example
  used in the design doc's own illustration.
- `packages/jade-js/tenants/merged.ts:12` — `export class MergedTenant implements Tenant,
  TenantProvider` has `#bridges`, `#bridgeInfo`.
- `packages/jade-js/primordials/array-buffer.ts`'s `BufferPrimordialImpl` (landed via the
  primordial-IR class-lowering work) also has real `#private` fields, so it is a target that
  is *also* one of primordial-ir's own IR-lowered classes — direct evidence the two passes'
  inputs genuinely overlap, not just "compose" in the abstract.

The public methods to select per class are `TENANT_METHOD_NAMES` from
`crates/jade-vm-frontend/src/tenant_inline.rs:36` (`make`/`get`/`set`/`define`/`assign`/
`ownKeys`) — the same operation set the JIT already knows how to inline.

## Design

### Composability is in-process AST, not a file handoff — one new orchestration crate

`jade-swc-tenant-exposure` already exposes an AST-level entry point —
`pub fn transform_program(program: &mut Program, config: &TenantExposureConfig)` at
`crates/jade-swc-tenant-exposure/src/lib.rs:140` — separate from the string-level
`transform_bundle_source`. That means the tenant-exposure half of this plan needs **no
signature change** to compose in-process; it already accepts a live `swc_ecma_ast::Program`.

The missing half is on `jade-primordial-ir`'s side: `emit_ts.rs` currently only produces a
`String` (see its own doc comment, quoted above — text emission was a deliberate choice to
avoid rebuilding `swc_ecma_ast` nodes, since nothing downstream previously needed real AST).
This plan adds a new function that builds real AST instead:

```rust
// crates/jade-primordial-ir/src/emit_ts.rs (or a new emit_ast.rs)
pub fn emit_ast(module: &ir::Module) -> swc_ecma_ast::Module;
```

Both crates are already pinned to identical `swc_ecma_ast`/`swc_ecma_parser`/
`swc_ecma_codegen`/`swc_ecma_visit` versions (`docs/primordial-ir-plan.md`: "versions already
pinned"), which is what makes handing a `jade-primordial-ir`-built `swc_ecma_ast::Module`
straight into `jade-swc-tenant-exposure::transform_program` valid at all — same concrete
Rust types on both sides of the boundary, not just structurally similar ones.

**New crate: `crates/jade-bundle-build`** (bin-only orchestration crate, depending on both
`portal-solutions-jade-primordial-ir` and `portal-solutions-jade-swc-tenant-exposure`, plus
`swc_ecma_parser`/`swc_ecma_codegen` directly). Neither existing crate gains a dependency on
the other — matching the repo's existing "one small crate per concern," each independently
testable (`jade-cfg-opt`, `jade-vm-jit-swc`, `jade-swc-tenant-exposure` are called out as this
same pattern in `docs/primordial-ir-plan.md`'s "Package layout"). One binary, one pass, one
text-serialization point at the very end:

1. For each `packages/jade-js/primordials/*.ts`: `jade_primordial_ir::lower_module` -> IR ->
   `emit_ast(&ir_module)` -> an in-memory `swc_ecma_ast::Module`. No text yet.
2. For every other `packages/jade-js/**/*.ts` (`tenants/*.ts`, etc.): parse directly with
   `swc_ecma_parser::Syntax::Typescript` into a `swc_ecma_ast::Module`.
3. For every file in the combined set, wrap it as `Program::Module(...)`, call
   `jade_swc_tenant_exposure::resolve(program, true)` (see "Solidifying `transform_program`'s
   resolve precondition" below), then call
   `jade_swc_tenant_exposure::transform_program(&mut resolved, &config)` in-process, against a
   hardcoded `TenantExposureConfig` naming `Tenant` (`multi.ts`), `MergedTenant`
   (`merged.ts`), and `BufferPrimordialImpl` (`array-buffer.ts`, from step 1's AST) as
   `tenant_classes`, with `tenant_methods = TENANT_METHOD_NAMES`.
4. Print each transformed `Program` via `swc_ecma_codegen` — the single point in the whole
   pipeline where AST becomes text — into the generated, checked-in output location (below).

This avoids the lossy parse -> emit-text -> reparse -> transform -> emit-text chain a
file-handoff design would need, and avoids compounding `emit_ts.rs`'s existing
from-scratch-string-builder fidelity risk (comments/formatting not guaranteed to round-trip,
per its own doc comment) with a second serialization downstream of it.

### Migrating `emit_ts.rs` from text-first to AST-first (a call I'm making, flagging for override)

`emit_ts.rs`'s existing hand-rolled string builder is already verified (per
`docs/primordial-ir-plan.md`'s Progress notes) via `tsc --noEmit` round-trips for
`object.ts`/`function.ts`/`reflect.ts`/`array-buffer.ts`. Throwing that away in one shot to
build `emit_ast` from scratch would regress real, already-proven coverage. Recommended
default: **migrate file-by-file, keeping the old text emitter as a parity oracle during the
transition** — for each file, add a test asserting
`print(Program::Module(emit_ast(module))) == emit_module(module)` (the new AST path's printed
text matches the old text builder's output, byte-for-byte or after a documented allowlist of
cosmetic differences). Once a file's parity test passes, `gen-primordials`' TS output for that
file switches to the `emit_ast`-derived path, and the file's hand-rolled branch in
`emit_module`'s `match` can be deleted. `emit_ts.rs` (the text builder) disappears entirely
once every file has migrated. **Flag if you'd rather keep both emitters permanently** (e.g. as
an independent cross-check that never gets deleted) — the cost is two implementations of the
same IR->TS mapping that can silently drift against each other over time, which is the
opposite of this codebase's stated regeneration discipline.

### Where the orchestrator writes (a call I'm making, flagging for override)

`emit_ts.rs`'s current doc comment insists hand-written `primordials/*.ts` stays canonical
and generated TS is "never checked in as a replacement." Overwriting those files in place —
or overwriting `tenants/*.ts` — would contradict that and turn every hand-edit into an
edit/regenerate/reformat loop that can lose comments or formatting even after the migration
to `emit_ast` above (real AST construction fixes *structural* fidelity, e.g. it's no longer
possible to accidentally emit a syntactically different tree; it doesn't restore comments the
IR never captured, or guarantee the printer's formatting matches a human's).

Given that, and matching the repo's existing convention of keeping generated output in its
own checked-in location separate from hand-written source (`jade-primordial-rt` next to
`packages/jade-js/primordials`, `data.rs`/`dispatch.rs` next to the opcode spec), **the
orchestrator writes its final printed output to a new location** rather than overwriting
`primordials/*.ts`/`tenants/*.ts` in place — e.g. `packages/jade-js/.generated/*.ts`,
mirroring the package's existing directory layout underneath. This keeps every hand-written
file in `packages/jade-js` fully authored and canonical, exactly as today. **Flag if you'd
rather overwrite in place** — smaller change, but accepts the fidelity risk above on every
regen, and reintroduces exactly the file-handoff/edit-loop problem the AST-composability
change above was meant to avoid.

### What actually ships

`packages/jade-js/package.json`'s `zshy` config currently points `main`/`exports` directly at
hand-written `.ts` entry points (`"main": "index.ts"`, `"./tenants": "tenants/index.ts"`,
`"./primordials": "primordials/index.ts"`) — this package ships raw TypeScript source,
consumed via Node's type-stripping or a downstream bundler, not precompiled JS. For the build
step's output to actually be what a live `Tenant` class's `toString()` reflects at runtime
(the entire point of moving this to build time — see "Why this matters" below), `zshy`'s
`exports` need to point at the *generated* directory instead of the hand-written source
directory once this lands. This is a real, visible build-system change (affects what
`import "@portal-solutions/jade-js/tenants"` resolves to) and should be called out in review,
not silently swapped.

### Why "generated in place of hand source" actually matters here

The reason to do this as a build step at all, rather than leave the dynamic runtime pass as
the only mechanism: once the shipped `Tenant` classes' own defining source already contains
the mangled public accessors and exposed-function shims, a live class's
`Function.prototype.toString()` naturally returns already-transformed code with zero runtime
cost or dynamic dependency on `jade-swc-tenant-exposure` at all. `tenant_inline`'s extractor
(`crates/jade-vm-frontend/src/tenant_inline.rs`) is unaffected either way — it already expects
to reject `#private`-containing methods and accept the widened form; it doesn't care whether
the widening happened five minutes before collection (today) or at package build time
(this plan).

### Parser generalization: why the orchestrator's non-primordial files don't need primordial-ir's narrow grammar subset

`jade-primordial-ir`'s `lower.rs` intentionally only accepts a small, closed, surveyed subset
of TypeScript (`docs/primordial-ir-plan.md`'s "IR node set" section) because every accepted
construct must have an explicit Rust lowering — that constraint is unchanged by this plan and
still applies to `emit_ast`, since it walks the same IR. `jade-swc-tenant-exposure` has no
such constraint: it does targeted `VisitMut` rewrites (private-field access -> accessor
member expression; a function declaration -> shim + record) and re-emits everything else
through `swc_ecma_codegen` unchanged. `swc_ecma_parser`'s TS syntax mode is a real,
full-featured parser (generics, mapped/conditional types, enums, `satisfies`, etc. —
whatever's actually used across `packages/jade-js`, not just the 10 primordial files), so the
orchestrator can parse every non-primordial file directly (step 2 above) with no closed-subset
gate, and only the 10 primordial files need to survive `lower_module`'s stricter IR
translation. The pass's existing "hard reject, never silently miscompile" rule
(`ExposureSkipReason`) still applies, but only to the specific selected
classes/methods/functions it's asked to transform — not to the rest of each file's syntax.

### Solidifying `transform_program`'s resolve precondition (closing a pre-existing gap)

`transform_program`'s own doc comment currently says "this does not run resolver itself"
(`crates/jade-swc-tenant-exposure/src/lib.rs:136`) — an implicit, undocumented-as-a-contract
gap against the original design doc's explicit requirement: "must use SWC binding identity
(`Id`/syntax context after resolver), not spelling alone, for every free-variable decision"
(`docs/swc-tenant-accessors-and-exposed-functions-plan.md`). The new orchestrator is about to
become the first caller to hand this function real multi-method classes with repeated local
names across methods (`multi.ts`/`merged.ts` both have several), which is exactly the shape
that gap would miscompile silently. **Decision: solidify this as a compile-time-enforced
precondition, not a documented convention.**

```rust
// crates/jade-swc-tenant-exposure/src/lib.rs
pub struct ResolvedProgram(Program);

impl ResolvedProgram {
    pub fn into_inner(self) -> Program { self.0 }
}
impl std::ops::Deref for ResolvedProgram { /* ... */ }
impl std::ops::DerefMut for ResolvedProgram { /* ... */ }

pub fn resolve(mut program: Program, typescript: bool) -> ResolvedProgram {
    let unresolved_mark = swc_common::Mark::new();
    let top_level_mark = swc_common::Mark::new();
    program.visit_mut_with(&mut swc_ecma_transforms_base::resolver(
        unresolved_mark, top_level_mark, typescript,
    ));
    ResolvedProgram(program)
}

pub fn transform_program(program: &mut ResolvedProgram, config: &TenantExposureConfig) -> TenantExposureReport
```

`Program` -> `&mut ResolvedProgram` makes "call `resolve()` first" impossible to skip by
accident, matching this codebase's stated "hard reject, never silently miscompile" philosophy
more literally than a doc comment can. `swc_ecma_transforms_base` is already a pinned
workspace dependency (`^38.0.0`, `Cargo.toml:30`) but not yet listed in
`crates/jade-swc-tenant-exposure/Cargo.toml` — one line to add.

Blast radius is small: `transform_program` has exactly one existing call site today
(`lib.rs:131`, inside `transform_bundle_source` itself), and every test in the crate goes
through the string-level `transform_bundle_source` API rather than calling `transform_program`
directly. `transform_bundle_source` absorbs the new `resolve()` call internally, so its
signature and the runtime/embedder-facing surface (decision 3, the dynamic fallback path) are
unchanged. The only new caller that must call `resolve()` explicitly is the orchestrator built
in this plan — its step 3 (above) becomes "wrap it as `Program::Module(...)`, call `resolve()`,
then call `transform_program`."

### Unifying the parser mode instead of branching on caller (build-time vs. runtime)

TypeScript syntax is a superset of the plain-JS text a live `toString()` reflects at runtime
(after Node/browser type-stripping has already removed type annotations). `swc_ecma_parser`'s
TS mode parses plain JS input correctly, so `transform_bundle_source`/`transform_program` can
switch unconditionally to `Syntax::Typescript` rather than exposing a syntax-mode parameter
split between the build-time and runtime call sites — the runtime path's input just happens
to contain no type nodes to preserve, and its output is unaffected. This keeps the crate's
public API surface unchanged for the runtime fallback path (decision 3 above).

## Implementation phases

1. **`emit_ast` in `jade-primordial-ir`, migrated file-by-file against the parity oracle**
   described above, starting with `object.ts`/`function.ts`/`reflect.ts` (already fully
   IR-lowered and Rust-verified per `docs/primordial-ir-plan.md`'s Progress notes, so the IR
   side is trustworthy — only the new AST-shaped emission is unproven). `array-buffer.ts`
   (the file with a real `#private`-bearing class, `BufferPrimordialImpl`) is the file that
   actually exercises this plan's motivating case end-to-end, so it should land before
   declaring this phase done even though `typed-arrays.ts`/`proxy.ts`/`promise.ts`/`realm.ts`
   can migrate later.
2. **Harden `jade-swc-tenant-exposure`**, independent of phase 1 and landable in parallel:
   - Parser generalization: change `crates/jade-swc-tenant-exposure/src/lib.rs:122` (the main
     `transform_bundle_source` entry point) from `Syntax::Es` to `Syntax::Typescript`. Leave
     the internal `parse_script_program` helper (line ~788, used only for re-parsing small
     generated fragments the crate itself produces) as `Syntax::Es` — it never sees TS input.
     Add test fixtures with real type annotations (matching `multi.ts`/`merged.ts`'s actual
     field/method type shapes) to confirm codegen round-trips TS syntax correctly, not just
     plain JS.
   - Resolve solidification: add `swc_ecma_transforms_base` as a dependency, introduce
     `resolve`/`ResolvedProgram`, change `transform_program` to take `&mut ResolvedProgram`,
     update `transform_bundle_source`'s one internal call site to resolve first. Add a
     regression test that two same-named locals in different methods (matching
     `multi.ts`/`merged.ts`'s actual shape) are *not* conflated by the transform — the
     concrete failure mode this change closes.
3. **New `crates/jade-bundle-build` orchestration binary**, once phases 1-2 cover the
   concrete targets from "Concrete targets" above: builds the hardcoded
   `TenantExposureConfig`, runs the 4-step in-process pipeline (lower/emit_ast or parse ->
   `transform_program` -> `swc_ecma_codegen` print), writes to the new generated directory.
4. **Wire the build.** Add the new binary to whatever orchestrates the existing "regenerate
   and diff" CI-style check (`scripts/regen.ts`'s convention): regenerate, diff against
   checked-in output, fail CI on drift.
5. **Point `zshy` at generated output**, once the orchestrator's output is verified to
   round-trip through `tsc --noEmit` and the existing `primordials.e2e.ts`/tenant e2e suites
   (`docs/primordials-plan.md`'s prescribed regression command, pointed at generated output).
6. **Regression coverage**: confirm a `Tenant`/`MergedTenant`-shaped class's *shipped*
   (post-build-step) source, when reflected via `toString()` at runtime, already contains the
   mangled accessor and is accepted by `tenant_inline::extract_tenant_methods` — i.e. the
   runtime no longer needs to invoke `jade-swc-tenant-exposure` at all for this class, only
   for embedder-supplied ones (decision 3).

## Tests

- Round-trip: Stage 1 + Stage 2 output for `multi.ts`/`merged.ts`/`array-buffer.ts` compiles
  under `tsc --noEmit` against the project's `tsconfig.json`.
- Behavioral: existing `primordials.e2e.ts`/`trap.e2e.ts`/`tenant-compose.e2e.ts`/
  `exotic-tenant.e2e.ts` suites pass unmodified when pointed at the generated bundle
  directory instead of hand-written source (same regression command
  `docs/primordials-plan.md` already prescribes for primordial-ir's own output).
- `tenant_inline::extract_tenant_methods` accepts the post-build-step `Tenant`/`MergedTenant`
  source for every method named in `TENANT_METHOD_NAMES`, where it previously rejected the
  hand-written `#shadow`/`#bridges`-using bodies.
- Drift check: CI regenerates both stages and diffs against checked-in output, matching
  `scripts/regen.ts`'s existing discipline.
- Unselected classes/methods/files are byte-for-byte unchanged by Stage 2 (mirrors the
  existing unit-test style in `crates/jade-swc-tenant-exposure/src/lib.rs`'s test module).

## Open questions to resolve during implementation (not blocking a first draft)

- Exact generated-directory naming/layout (`packages/jade-js/.generated/...` used above as a
  placeholder).
- Whether `swc_ecma_codegen`'s default `Config` correctly re-emits every TS construct
  actually present in `packages/jade-js` (generics, `satisfies`, etc.) without a dedicated
  codegen feature flag — needs a small spike against real files from the package, not just
  the primordial subset, before phase 2 above is considered done.
- Whether `emit_ast` should adopt the existing `--rust` path's per-item resilience (an
  unlowerable top-level item is omitted + reported on stderr, per
  `docs/primordial-ir-plan.md`'s "Per-item emission" correction), or whether AST emission
  should hard-fail the whole file on any lowering gap since — unlike Rust emission — a
  partially-built `swc_ecma_module` has no meaningful "this one function is just missing"
  state that's still valid to feed into `transform_program`/`swc_ecma_codegen`. Leaning
  toward keeping per-item resilience (matches the already-established convention and lets the
  orchestrator ship every other item while one gap gets fixed), but worth confirming once
  phase 1 has a concrete case to look at.
- Whether the `emit_ast`/text-emitter parity check (phase 1) should be a one-time migration
  gate or a permanent CI test kept around as a cheap structural regression check even after
  the text emitter is deleted (e.g. snapshot-testing `emit_ast`'s printed output directly).
- Whether `packages/jade-js`'s two non-primordial-ir files with real class syntax beyond
  `multi.ts`/`merged.ts` (a full audit of `tenants/*.ts` and the rest of the package wasn't
  done as part of this plan) surface any additional selection targets or any TS construct
  `swc_ecma_codegen` doesn't round-trip cleanly.
- ~~Whether `transform_program` needs a resolver pass before it sees the AST~~ — resolved:
  see "Solidifying `transform_program`'s resolve precondition" above. Remaining detail to
  settle during implementation: exact `resolver()` argument values (`unresolved_mark`/
  `top_level_mark` freshly created per call vs. threaded through from wherever `GLOBALS::set`
  is scoped) and whether `typescript: bool` should be a `resolve()` parameter (as sketched
  above) or hardcoded `true` now that phase 2 always parses as `Syntax::Typescript`.

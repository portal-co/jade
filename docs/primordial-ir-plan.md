# Plan: SWC-based intermediate IR for tenant primordials

## Progress (updated as implementation lands)

- **Done:** `jade-tenant-rt` (hand-authored `Tenant`/`HostAsyncCapability` traits, extended
  during implementation with `ValueTag`/`typeof_tag`/`to_number`/`to_boolean`/`to_string_value`/
  `to_property_key`/`nullable`/`indexed_collection`/`property_key_value` and primitive
  constructors — needed once real primordial bodies, not just `types.ts`'s helpers, were
  lowered). `jade-primordial-ir` (parser/IR/`emit_ts`/`emit_rust`, `gen-primordials` binary).
  `jade-primordial-rt` (generated output crate).
- **`types.ts`:** shimmed, not IR-lowered — see "Shimmed modules" below (added after the initial
  plan; a deliberate correction, not part of the original design). Hand-ported to
  `jade-primordial-rt::types_shim`, covering `defineData`/`readGuestDescriptor`/
  `descriptorObject`/`guestArrayLike`/`assertObject`/`toIndex`/`makeBuiltin`. Fully tested.
- **`object.ts`:** fully IR-lowered and generated, `objectPrimordial` included — see
  `docs/array-primordial-gap-plan.md`, now closed: `Tenant` gained `indexed_collection`
  (constructs a tenant-owned indexed/`length` collection from a `Vec<Self::Value>`, deliberately
  not claiming `Array.prototype` fidelity) and `property_key_value` (the inverse of
  `to_property_key`), and `coerce_return_value`'s `ownKeys`/`ownPropertyKeys`-returning branch
  uses both instead of rejecting. Behavior-tested end to end (`object_primordial.rs`), including
  `Object.keys` itself.
- **`function.ts`:** fully IR-lowered, generated, and wired into the module tree (unblocked by
  the same gap closing, since it depends on `crate::object::object_primordial` for
  `ObjectPrototype`). Behavior-tested end to end (`function_primordial.rs`): `call`/`apply`/
  `bind` through the tenant invocation ABI. Emitter bugs found and fixed while landing it (beyond
  the ones already listed below from the first pass): `yield EXPR as TARGET` parses as `yield
  (EXPR as TARGET)`, not `(yield EXPR) as TARGET` (see `lower.rs`'s `lower_yield`);
  `ArgKind::OptionOwned`/`OptionRef` needed "an explicit `undefined` argument means `None`, not
  `Some(tenant.undefined_value())`" handling; a closure's `args[N]` fallback
  (`.unwrap_or_else(|| tenant.undefined_value())`) and `coerce_return_value`'s
  `tenant.boolean_value(tenant.<method>(...)?)` wrapping both inlined a *second* live mutable
  borrow of `tenant` as an argument to a call that already borrows it — fixed by pre-materializing
  a per-function/closure `__undefined` local (see `emit_member`'s doc comment) and by hoisting the
  inner call's result into a `let` before wrapping it, respectively; `.slice()` produced a
  borrowed `&[T::Value]` that couldn't be captured into a `'static` nested closure
  (`Function.prototype.bind`) — now produces an owned `Vec`; and the closure free-variable
  *capture* analysis was a text search over the emitted body, which false-positived on
  `TenantInvocation` struct-literal field labels (`this_arg:`, `args:`) that happen to spell the
  same word as a real outer-scope name — replaced with a precise IR walk (`free_idents_in_fn`)
  that skips key/label positions and resolvable call callees. Also added along the way:
  `TenantInvocation` object-literal recognition (`{ kind: "apply", thisArg, args }` / `{ kind:
  "construct", args, newTarget }`, `tenant.invoke`'s second argument), cross-file factory
  calls/destructuring (`cross_file.rs` — the generated-code counterpart to `shims.rs`, since
  `functionPrimordial` calls and destructures `objectPrimordial`'s result), and closures with
  fewer than 2 declared parameters (`Function`'s own apply/construct closures both ignore all
  their arguments, padded to the fixed 2-param Rust shape `make_builtin` requires with synthetic
  unused names).
- **`reflect.ts`:** fully IR-lowered, generated, and wired into the module tree
  (`reflect_primordial`, second cross-file consumer of `objectPrimordial`). Behavior-tested end
  to end (`reflect_primordial.rs`): `get`/`set`/`has`/`deleteProperty`/`ownKeys`/
  `getOwnPropertyDescriptor`/`defineProperty`/`apply`/`construct`. `Reflect.ownKeys` confirmed the
  `indexed_collection`/`property_key_value` path generalizes past `Object.keys`. One more emitter
  bug found and fixed while landing it, a *third* variant of the repeated "second live mutable
  borrow of `tenant` inline in an argument position" hazard (see `function.ts`'s entry above):
  `Reflect.defineProperty`'s `tenant.defineProperty(a, b, yield tenant.yieldTenant
  (readGuestDescriptor(tenant, c)))` — a tenant-method-call argument that itself makes a nested
  tenant/shim call. Rather than another point patch, this one got the systemic fix: `emit_call`'s
  `tenant.<method>(...)` branch and `emit_shim_call` now both hoist any argument whose rendered
  text contains `?` or the word `tenant` into its own `let` first (`maybe_hoist_arg` — excluding
  the bare `tenant` argument itself, and excluding closure-literal arguments, both of which
  needed their own carve-outs after the first attempt regressed `object.ts`/`function.ts`: hoisting
  `tenant` moves a non-`Copy` `&mut T`, and hoisting a closure into an unannotated `let` breaks
  its `Box<dyn FnMut>` unsizing coercion). Also extended `coerce_return_value` to wrap a bare
  `return true;`/`return false;` (`Reflect.set`/`deleteProperty`'s own literal boolean results,
  not derived from a `tenant.<method>()` call) the same way as a `bool`-returning tenant method.
- **The shared `TenantExoticHandler`-from-object-literal capability all three of `proxy.ts`/
  `array-buffer.ts`/`typed-arrays.ts` need is done.** `tenant.makeExotic(proto, { *get(...) {...},
  ... })` now generates a real struct + `impl TenantExoticHandler<T> for GeneratedStruct<T>`
  (`emit_exotic_handler_literal` in `emit_rust.rs`), confirmed to compile as real Rust and covered
  by 6 regression tests. Building and actually compiling it surfaced three real bugs in one pass —
  a stale assumption about `emit_expr`'s `Lit::Undefined`/`Null` mapping (needed a new
  `RETURN_COERCION` context so a trap's own return-type-specific coercion doesn't collide with
  `make_builtin`-closure coercion sharing the same `Stmt::Return` code path), a missing
  `self.<field>` shadow-clone prelude for captured free variables, and `PropertyKey` needing
  explicit `::from(...)` conversion against a string-literal comparison — see
  `docs/proxy-and-buffer-primordial-gap-plan.md`'s now-updated first section for the full story.
  Designing the follow-on work also surfaced a new, previously unflagged blocker: `array-buffer.ts`'s
  `shell`/`constructor` are self-referential local closures (a plain Rust closure can't reference
  itself), which needs restructuring the whole function into a local struct+impl, not just wider
  closure-signature support — also written up in that note.
- **`jade-tenant-rt::buffer::BufferHooks` + `jade-primordial-rt::buffer_shim::NativeBufferHooks`
  are done and unit-tested** (6 tests) — a genuinely native Rust in-memory buffer adapter
  (`Rc<RefCell<Vec<u8>>>`-backed), not a port of `nativeBufferHooks`'s host-`ArrayBuffer`
  internals. Also fixed: `lower_module` was all-or-nothing per file (one unlowerable top-level
  item — `nativeBufferHooks` itself, which uses `instanceof`, unmodeled — aborted lowering the
  *whole file*, silently blocking `bufferPrimordial` too); now per-item resilient at lowering,
  matching emission's already-established policy.
- **The four type-resolution gaps found by running `array-buffer.ts` through the pipeline are
  now closed**, all confirmed with no regressions (`object.ts`/`function.ts`/`reflect.ts` still
  generate, compile, pass all 33 tests, round-trip clean through `tsc`): `BufferKind` type-name
  shimming to the hand-written enum, `BufferHooks` as an added generic trait-bound parameter
  (`emit_fn_decl` conditionally emits `<T: Tenant, H: BufferHooks>`), its own method-shaped
  interface erased entirely rather than parsed, and the inline-`{ identity, primordial }`-wrapped
  per-tenant-cache value (`Item::PerTenantCache` gained an `identity_wrapped` flag; both its
  lookup and `.set(...)` peepholes now recognize the wrapped shape).
- **New finding from re-running after that:** `BufferPrimordial` (the interface `bufferPrimordial`
  returns) has function-typed members (`record`/`shell` are real methods, not data) — no
  interface-as-struct translation handles that yet. This broadens the class-refactor scope
  decided above: rather than only extracting `shell`/`constructor`'s *internal* state into a
  helper class, `bufferPrimordial`'s entire return value should become a class instance (data
  fields + `record`/`shell` as real methods) — same underlying capability, applied at the
  function's public boundary too.
- **`array-buffer.ts`'s TS-side class refactor is done and verified.** `bufferPrimordial` now
  returns a `BufferPrimordialImpl` class instance (`#private` state; `record`/`shell` gained an
  explicit `tenant` parameter, since a Rust port needs it even though the original closures
  didn't) — the whole return value, not just an internal `shell`/`constructor` helper, per the
  decision above. Cascaded into 6 call-site updates in `typed-arrays.ts`. Verified by running all
  7 `*.e2e.ts` suites (not just `tsc`, which turns out to be blind to this class of regression —
  see the gap note's new "TS-side class refactor" section for why: every `yield tenant.yieldTenant
  (...)` expression is typed `any` throughout this codebase, so a wrong-arity method call on a
  `yield`-derived value silently type-checks). **Not yet done:** the `class`-lowering IR/Rust
  capability itself (fields, constructor, methods, `Rc<RefCell<Inner>>` + inherent-`impl`
  codegen, method-call-on-instance emission) — this is now the single remaining piece for both
  `array-buffer.ts`/`typed-arrays.ts` and `proxy.ts`.
- **Not started:** `proxy.ts`, `array-buffer.ts`, `typed-arrays.ts`, `promise.ts`, `realm.ts`.
  Paused deliberately, same discipline: no closure-capture-model or `Tenant`/new-hand-authored-
  trait changes until the gap note's remaining items are acted on. `proxy.ts` will need
  `functionPrimordial` wired the same way `object.ts` was for `function.ts`.

### Shimmed modules (a correction to the original plan)

The original plan (below) assumed every primordial file would be fully IR-lowered. In practice,
`types.ts`'s helpers hit three IR-hostile shapes at once: `makeBuiltin`'s exotic-handler object
literal closes over `this` across its own methods (no clean Rust translation without a real
struct+impl, which is exactly what hand-writing gives for free); `readGuestDescriptor`/
`descriptorObject` do dynamic by-name field access over a fixed key set (`for (const key of
[...] as const) if (key in x)`) that would need real reflection machinery to derive
automatically; and `assertObject`/`toIndex` need `typeof`/`Number()` coercion on an opaque
`T::Value`, which doesn't have a natural IR shape either.

Rather than building a generic solution for all three, `types.ts` is now permanently hand-ported
(`jade-primordial-rt::types_shim`, hand-written despite living in the "generated" crate — see its
own module doc comment) and registered in a **shim registry** (`jade-primordial-ir::shims`): a
curated table of `(import source, export name) -> Rust path`, in the same spirit as
`tenant_inline.rs`'s `ALLOWED_GLOBAL_CALLEES`. TypeScript emission needs no special handling at
all (a call to an imported identifier is already emitted as an ordinary `Expr::Call`, and the
`import` statement is already re-emitted verbatim) — the regenerated file keeps calling the real
`types.ts`. Rust emission resolves the same call through the registry instead of attempting to
inline or re-derive the shimmed function's body. Any *other* file that turns out to need the same
treatment gets a new registry entry, not a one-off special case.

### Per-item emission (a correction to the original plan)

The original plan's "every phase's generated Rust must compile" test criterion turned out to
have a sharper edge than intended: a single `IrError` anywhere in one large factory function
(`Object.keys`'s Array gap, above) would make `emit_rust`'s straightforward approach — embed a
`compile_error!(...)` and move on — fail the *entire crate's* build, not just that one function's
call sites, since `compile_error!` is unconditional regardless of where it's textually placed.
That would force an all-or-nothing choice per file long before every construct in it is covered,
which defeats incremental testing. `gen-primordials` now emits each top-level item independently:
a failing item is *omitted* from the generated file and reported on stderr (`gen-primordials:
skipping fn \`objectPrimordial\`: ...`), rather than embedded as a build-breaking artifact. Still a
hard, loudly reported rejection at the point it happens, never a silent guess — it just surfaces
as a build-time diagnostic instead of a build-breaking one, since nothing calls the omitted item.

---

## Context

`packages/jade-js/primordials/` (10 files, ~1056 lines) implements Jade's tenant-scoped
standard-library values (`Object`, `Function`, `Reflect`, `Proxy`, `Promise`, `ArrayBuffer`,
typed arrays, plus `types.ts`'s shared helpers and `realm.ts`'s assembler) purely as
generator functions that call the `Tenant` interface (`packages/jade-js/tenants/types.ts`) —
never raw host property access, never host `Proxy`/`Function`. `docs/primordials-plan.md`
called this out as deliberate future-proofing (lines 123-125): "primordials must remain
lightweight consumers of their explicit API surface, so their behavior can be recompiled for
another backend without inheriting ambient host powers."

This plan cashes in that design goal: build a small, SWC-based intermediate IR that parses
the existing primordial TypeScript, and can re-emit it (a) back to TypeScript, as a
losslessness/verification check, and (b) as genuine, checked-in Rust source implementing the
same primordials against a new hand-authored Rust `Tenant` trait — so a pure-Rust embedding
of Jade (no JS/TS host at all) gets a native standard library without hand-porting ~1000 lines
of generator-heavy TypeScript by hand, and without that port silently drifting from the
canonical TS implementation over time.

The hand-written TypeScript in `packages/jade-js/primordials/` remains the single source of
truth, exactly as the opcode spec already is for `scripts/regen.ts`'s generated
`vm.ts`/`data.rs`/`dispatch.rs`. This plan adds a second, Rust-native generator alongside that
existing one, driven by real source parsing (via `swc_ecma_parser`) rather than data
templating, because the primordial files are executable code, not a structured spec.

**Decisions already settled with the user:**
- Rust emission produces a real, checked-in, callable native implementation (not a
  reference/spec-only rendering).
- V1 covers all ten primordial files, including `promise.ts` (async/microtask state machine)
  and `array-buffer.ts`/`typed-arrays.ts` (`DataView`-style byte codecs) — not just the
  "pure" `Object`/`Function`/`Reflect`/`Proxy` subset.

## Why this shape (research findings)

- **The existing TAC/SSA pipeline (`crates/jade-cfg-opt`, vendored `jsaw-core`) cannot be
  reused.** `optimize_tfunc` round-trips one function's straight-line/branching body through
  `portal_jsc_swc_tac`/`-ssa`'s `TFunc`/`SFunc`, and those types directly embed
  `swc_ecma_ast::TsType`/`Ident`/`Span` — they are not language-agnostic. The frontend that
  produces `TFunc` in the first place (`crates/jade-vm-frontend`) explicitly has "no nested
  closures yet [unimplemented per `AGENTS.md`], no exception handling, no switch statement."
  Primordials are whole-module (multiple co-defined generator functions, module-level
  `WeakMap` cache state, cross-file imports), closure-heavy, and use `try`/`catch`. Retrofitting
  the TAC/SSA pipeline to cover this would be riskier and larger than a purpose-built IR. The
  new IR is a **sibling** to `jade-cfg-opt`, not an extension of it — though it reuses the same
  `swc_ecma_parser`/`swc_ecma_ast`/`swc_ecma_codegen`/`swc_ecma_visit` workspace dependencies
  (versions already pinned: `swc_ecma_ast 21.0.0`, `swc_ecma_parser 35.0.0`,
  `swc_ecma_codegen 24.0.0`, `swc_ecma_visit 21.0.0`).
- **The closest working precedent is `crates/jade-vm-frontend/src/tenant_inline.rs`**: parse
  class source via `swc_ecma_parser`, extract/validate specific methods (hard-reject
  `#private` refs and free non-global callees, rewrite `this` via `VisitMut`, re-serialize via
  `swc_ecma_codegen`), gated by a hand-maintained allowlist of legal global callees
  (`ALLOWED_GLOBAL_CALLEES`). This plan follows the same philosophy — explicit allowlists,
  loud rejection over silent miscompilation — but is JS-in/**Rust**-out as well as JS-in/JS-out,
  which nothing in the repo does today (a grep of all of `crates/` for `quote!`/`proc_macro2`/
  `syn::` found zero matches: Rust-source emission is a green-field build here).
- **No Rust `Tenant` trait exists.** `crates/jade-vm-core/src/dispatch.rs`'s `State`/`Ops`
  traits are the low-level opcode-execution abstraction (`Result<Self::Value, Self::Error>`,
  fully synchronous). Nothing in Rust mirrors the TS `Tenant` interface's object-model surface
  (`get`/`set`/`has`/`delete`/`ownKeys`/descriptors/prototypes/exotics/`invoke`). Authoring this
  trait is a required, hand-written deliverable of this plan — the emitted Rust code's whole
  reason for existing is to call into it.
- **Key simplification: Rust doesn't need the TS generator/yield ceremony.** Every TS
  primordial operation is a `function*` composed via `yield tenant.yieldTenant(...)` solely to
  support the *optional* `addAsync`/`addGen` ambient-upgrade path through `driveTenant`. On the
  Rust side, tenant operations are already synchronous, fallible calls (matching `Ops`'s
  existing `Result<Value, Error>` convention) — there is no cooperative scheduler at this layer.
  So `yield tenant.yieldTenant(tenant.get(...))` is a **first-class, distinctly-lowered IR
  node**: TypeScript emission preserves the real generator/yield form verbatim (trivial, since
  it's exactly what was parsed); Rust emission strips it to a direct `tenant.get(...)?` call.
  Similarly the TS-only ABI/driver plumbing methods (`markGuestFn`, `createGuestGen`,
  `unpackGuestGen`, `yieldHostTask`, `yieldTenant`, `driveTenant`) have no Rust equivalent and
  are dropped from the Rust trait; `invoke`/`invokeGuestAware`/`invokeTrap` are kept since they
  matter for calling guest callbacks with correct ABI metadata.
- **Rust has no `WeakMap`, but doesn't need one.** Every primordial file's per-tenant cache is
  the identical `const cache = new WeakMap<Tenant, X>(); ... cache.get(tenant) ?? create...`
  idiom. In Rust, "one cache per tenant" is just "the cache lives inside whatever the embedder
  already keeps alive alongside that tenant" — no weak/GC-tied map needed at all. This becomes
  a dedicated `PerTenantCache` IR node (recognized specially, not a generic `WeakMap`
  intrinsic) that lowers to a `WeakMap` in TS and to a plain `Option<T>` field on a generated
  `PrimordialCache` struct in Rust, populated lazily and owned one-per-tenant by the caller.
  Non-tenant-keyed maps (`array-buffer.ts`'s buffer-handle records, `typed-arrays.ts`'s typed
  array records, `types.ts`'s per-builtin descriptor map) key off guest-object identity instead
  and need real map semantics — see the `Tenant::ObjectId` design below.

## Package layout

Three new Rust crates, following the existing one-small-crate-per-concern convention
(`jade-cfg-opt`, `jade-vm-jit-swc`, `jade-swc-tenant-exposure` are all similarly scoped):

```text
crates/
  jade-tenant-rt/            # hand-authored, NOT generated
    src/lib.rs                 # Tenant, HostAsyncCapability traits; PropertyKey,
                                # TenantError, TenantPropertyDescriptor, TenantInvocation,
                                # TenantExoticHandler / TenantCallableExoticHandler traits
    src/intrinsics.rs           # hand-written helpers the mapping table lowers to:
                                # DataView-style byte codecs, is-array-index-string, etc.

  jade-primordial-ir/        # hand-authored: the IR + both emitters
    src/ir.rs                   # IR node/type definitions
    src/lower.rs                 # swc AST -> IR, with hard-reject validation
    src/intrinsics.rs            # host-intrinsic mapping table (TS passthrough / Rust mapping)
    src/shims.rs                  # shimmed-module registry (see "Shimmed modules" above)
    src/emit_ts.rs                # IR -> TypeScript text
    src/emit_rust.rs              # IR -> Rust text (proc_macro2 + quote)
    src/lib.rs                    # translate_source/emit_typescript/emit_rust_source orchestration
    src/bin/gen-primordials.rs    # regeneration entrypoint (see Phase 8)

  jade-primordial-rt/        # GENERATED, checked in (like data.rs/dispatch.rs)
    src/types_shim.rs             # hand-written (see "Shimmed modules" above)
    src/object.rs / function.rs / reflect.rs / proxy.rs /
        array_buffer.rs / typed_arrays.rs / promise.rs / realm.rs
        # each begins "/* This is GENERATED code by gen-primordials */",
        # matching the existing data.rs/dispatch.rs header convention
```

`jade-primordial-ir` depends on `jade-tenant-rt` only to know the trait/type names it must
emit calls against, not the other way around. `jade-primordial-rt` depends on `jade-tenant-rt`
for its trait bounds and is otherwise pure generated output (plus the hand-written shim module).

Added `proc_macro2` and `quote` as new workspace dependencies for `jade-primordial-ir`'s Rust
emitter (chosen over hand-rolled string templating: hygienic token-stream construction avoids
a whole class of "forgot to escape/parenthesize" bugs that a from-scratch Rust codegen effort
is exactly where they'd show up first).

## IR node set

Derived from a construct survey of the actual 10 files (verified via grep — this is a closed,
already-observed subset, not a general TypeScript grammar to design against):

**Module level:** imports (type-only imports are tracked for signature purposes, erased from
both emission outputs), `interface` declarations (round-trip through the IR as a real
`Item::StructDef` — driving the generated Rust struct's field layout — rather than being erased
like a plain type alias), the `PerTenantCache` module-const pattern (special-cased, see above),
plain top-level generator function declarations.

**Statements/expressions actually present:** `function*`/`yield` (the primary control-flow
shape, pervasive), nested function expressions/closures (heavy — every `makeBuiltin` call,
every `{*get(){...}, *set(){...}, ...}` exotic-handler object literal; lowering must compute
each closure's captured-variable list explicitly, since Rust closures need it), `for (const x
of ...)` and one indexed `for (let i = 0; ...; i++)`, shallow object destructuring (`const {
ObjectPrototype } = yield ...` — no nested or array destructuring appears, so those are
rejected if ever seen), `try { } catch { }` (four occurrences, all in `promise.ts`), ternaries,
plain template-literal interpolation (six occurrences, no tagged templates), spread (array
literal and call-argument positions), object/array literals, `throw`, `if`, `return`, `continue`,
`const`/`let` declarations, comparison/arithmetic/logical operators (`+ - * %`, `&& || ??`,
`=== !==`, `in`), calls/`new`/member access/computed member access, `as` casts, the comma
operator, and literals.

**Explicitly absent from the source and therefore explicitly rejected, not silently
tolerated, if encountered:** classes, `#private` fields, `this` used as anything but an
ordinary `thisArg`/`_this` parameter, guest-visible `async`/`await` (host-only, confined to
`promise.ts`'s `toHostTask`), decorators, `switch`, optional chaining (`?.`), tagged templates,
nested/array destructuring, any bare `yield` not wrapped as `tenant.yieldTenant(...)`. A hard,
named rejection (file + construct) is the only accepted failure mode for anything outside the
surveyed set — matching `tenant_inline.rs`'s existing philosophy in this codebase.

## Host-intrinsic mapping table

A curated allowlist (in the spirit of `tenant_inline.rs`'s `ALLOWED_GLOBAL_CALLEES`), each
entry with an explicit per-target lowering. TypeScript emission always passes through
natively (trivial — it's exactly the construct that was parsed). Rust emission needs a real
mapping:

| Intrinsic (as used in source) | Rust lowering |
| --- | --- |
| `new WeakMap()`/`new Map()` + `.get/.set/.has/.delete` (non-tenant-keyed: buffer/typed-array/descriptor records) | `HashMap<Tenant::ObjectId, V>` or `HashMap<PropertyKey, V>`, owned by the enclosing closure/handler state |
| `DataView` codec family (`get/setInt8`, `get/setUint8`, `get/setInt16/Uint16` `{..., true}` = LE, `get/setInt32/Uint32`, `get/setFloat32/Float64`, all little-endian) | `i8/u8/i16/u16/i32/u32/f32/f64::{from,to}_le_bytes` on byte slices, via a small codec helper in `jade-tenant-rt::intrinsics` |
| `Number.isInteger(x)` | `x.fract() == 0.0 && x.is_finite()` helper |
| `Math.min/max/round` | `f64::min/max/round` |
| Array `.slice/.splice/.sort/.push`, `Array.from({length}, fn)` on host-native bookkeeping arrays (never guest values) | `Vec<T>` equivalents |
| `/^(0|[1-9][0-9]*)$/.test(key)` (fixed numeric-key pattern) | named "is-array-index-string" intrinsic → hand-written parser helper in `jade-tenant-rt::intrinsics`, not general regex support |
| Template-literal interpolation | `format!(...)` |
| `throw new TypeError(...)` / `throw new RangeError(...)` | `return Err(TenantError::TypeError(format!(...)))` / `RangeError(...)`, function signature becomes `Result<_, TenantError>`, call sites propagate via `?` |
| `Object.entries(...)` over a realm.ts-local literal (not a guest object) | hand-special-cased in `realm.ts`'s emitter path — iterate the equivalent generated Rust struct's fields directly, not a generic intrinsic |

Anything calling a global not in this table (or in the small always-allowed set — `TypeError`,
`RangeError`, `Symbol` used only as a private brand key in `async-host.ts`, not primordials)
is a hard lowering-time rejection naming the exact file and construct.

## Rust `Tenant` / `HostAsyncCapability` traits (`jade-tenant-rt`)

Mirrors `packages/jade-js/tenants/types.ts`'s `Tenant` interface, minus the TS-only
generator/ABI-driver plumbing (`markGuestFn`, `createGuestGen`, `unpackGuestGen`,
`yieldHostTask`, `yieldTenant`, `driveTenant` have no Rust equivalent — the ambient
generator-driving protocol they support doesn't exist on this side). Extended during
implementation (see "Progress" above) with value-introspection/construction primitives that
have no TS-interface counterpart at all — `typeof`/`Number()`/`ToString`/`ToPropertyKey` are
ordinary host operations on the TS side, but need an explicit tenant-mediated primitive once
`Value` is opaque:

```rust
pub trait Tenant {
    type Value: Clone + PartialEq;
    type ObjectId: Eq + std::hash::Hash + Clone;

    // Value introspection/construction (added during implementation — see above)
    fn typeof_tag(&self, value: &Self::Value) -> ValueTag;
    fn to_number(&self, value: &Self::Value) -> f64;
    fn to_boolean(&self, value: &Self::Value) -> bool;
    fn to_string_value(&self, value: &Self::Value) -> String;
    fn to_property_key(&self, value: &Self::Value) -> PropertyKey;
    fn nullable(&self, value: &Self::Value) -> Option<Self::Value>;
    fn boolean_value(&mut self, value: bool) -> Self::Value;
    fn string_value(&mut self, value: &str) -> Self::Value;
    fn number_value(&mut self, value: f64) -> Self::Value;
    fn undefined_value(&mut self) -> Self::Value;
    fn null_value(&mut self) -> Self::Value;

    fn make(&mut self, proto: Option<Self::Value>) -> Result<Self::Value, TenantError>;
    fn get(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<Self::Value, TenantError>;
    fn set(&mut self, obj: &Self::Value, key: &PropertyKey, value: Self::Value) -> Result<(), TenantError>;
    fn has(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<bool, TenantError>;
    fn delete(&mut self, obj: &Self::Value, key: &PropertyKey) -> Result<(), TenantError>;
    fn own_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError>;
    fn own_property_keys(&mut self, obj: &Self::Value) -> Result<Vec<PropertyKey>, TenantError>;
    fn get_own_property_descriptor(&mut self, obj: &Self::Value, key: &PropertyKey)
        -> Result<Option<TenantPropertyDescriptor<Self::Value>>, TenantError>;
    fn define_property(&mut self, obj: &Self::Value, key: &PropertyKey,
        descriptor: TenantPropertyDescriptor<Self::Value>) -> Result<bool, TenantError>;
    fn get_prototype_of(&mut self, obj: &Self::Value) -> Result<Option<Self::Value>, TenantError>;
    fn set_prototype_of(&mut self, obj: &Self::Value, prototype: Option<Self::Value>) -> Result<bool, TenantError>;
    fn is_extensible(&mut self, obj: &Self::Value) -> Result<bool, TenantError>;
    fn prevent_extensions(&mut self, obj: &Self::Value) -> Result<bool, TenantError>;
    fn define(&mut self, target: &Self::Value, descriptors: &Self::Value) -> Result<(), TenantError>;
    fn assign(&mut self, dst: &Self::Value, src: &Self::Value) -> Result<(), TenantError>;
    fn make_exotic(&mut self, proto: Option<Self::Value>,
        handler: Box<dyn TenantExoticHandler<Self::Value>>) -> Result<Self::Value, TenantError>;
    fn make_callable_exotic(&mut self, proto: Option<Self::Value>,
        handler: Box<dyn TenantCallableExoticHandler<Self::Value>>) -> Result<Self::Value, TenantError>;
    fn invoke(&mut self, callee: &Self::Value, invocation: TenantInvocation<Self::Value>)
        -> Result<Self::Value, TenantError>;
    fn invoke_trap(&mut self, f: &Self::Value, receiver: &Self::Value, args: &[Self::Value])
        -> Result<Self::Value, TenantError>;
    fn object_id(&self, value: &Self::Value) -> Self::ObjectId;
}

pub enum TenantError { TypeError(String), RangeError(String) }
pub enum PropertyKey { String(String), Symbol(SymbolId) }
pub enum ValueTag { Undefined, Null, Boolean, Number, String, Symbol, Object, Function }
```

`HostAsyncCapability` mirrors `async-host.ts`'s small explicit interface
(`enqueue_microtask`, `observe`, `on_unhandled_rejection`, `on_rejection_handled`) closely
enough that it's a plausible direct port; exact generic shape for its `Task<T>` (likely a GAT)
is flagged as an **open detail to finalize during implementation**, not committed here — Rust
async-trait ergonomics are genuinely fiddly and shouldn't be over-specified before Phase 6.

**Named exclusion, not an oversight:** `PromiseRuntime.toHostTask`/`.awaitHostTask` and
`async-host.ts`'s `hostTaskFromPromise`/`hostTaskPromise` construct/consume a real native host
`Promise` — a JS-host-interop boundary with no natural Rust translation. These are excluded
from IR translation entirely; a human hand-writes the Rust equivalent once, matching whatever
async bridge the specific Rust embedding actually has (a `Future`, a callback, etc.). The
generated `promise.rs` covers the state machine (pending/fulfilled/rejected, reaction queue,
thenable assimilation) but not this boundary.

**Newly discovered gap, design paused (not yet resolved):** `Object.keys`/`Reflect.ownKeys`'s
closures return `tenant.ownKeys(...)`'s host-side `Vec<PropertyKey>` directly as their own
guest-visible return value — sound in the current TS-hosted execution model (a host array *is*
already a valid guest-observable value there), unsound in Rust without a guest Array primordial.
This has turned out to be a cross-file blocker (see "Progress" above), not an isolated gap —
`docs/array-primordial-gap-plan.md` is the design note for what a minimal, honest fix needs
(two new `Tenant` primitives, deliberately *not* a full `Array.prototype`); no `Tenant` change
has landed from it yet. `gen-primordials` omits the enclosing function and reports why (see
"Per-item emission" above) in the meantime, rather than guessing at a primitive.

## Implementation phases

1. **IR core + TS round-trip on the simplest files** (`types.ts`, `object.ts` — smallest,
   only the `WeakMap`/`Map` intrinsics beyond core control flow). Build `ir.rs`, `lower.rs`,
   `emit_ts.rs`; prove losslessness before ever touching Rust emission. — **Done**, with the
   `types.ts` correction above: it's shimmed rather than IR-lowered.
2. **Extend IR coverage**: `function.ts`, `reflect.ts`, `proxy.ts` (adds `TenantExoticHandler`
   object-literal lowering, more closures, no new fundamentally new constructs). — **In
   progress**: `object.ts`'s `installMethod`/`lock`/struct/cache landed first (closures,
   computed member access, per-tenant cache codegen, interface→struct codegen, Option-narrowing
   peepholes); `objectPrimordial` itself is blocked on the Array-primordial gap above;
   `function.ts`/`reflect.ts`/`proxy.ts` not started.
3. **Author `jade-tenant-rt`** (`Tenant`, `HostAsyncCapability`, `TenantError`, `PropertyKey`,
   `TenantPropertyDescriptor`, exotic-handler traits) — hand-written, reviewed independently of
   the generator. — **Done**, extended per "Progress" above as real coverage demanded it.
4. **Rust emission checkpoint**: `emit_rust.rs` for `types`/`object`/`function`/`reflect`/
   `proxy`; generated `jade-primordial-rt` must compile and pass new unit tests against a
   minimal in-crate test-double `Tenant` impl before moving on. — **Partial**: `types` (shim)
   and part of `object` meet this bar; `function`/`reflect`/`proxy` remain.
5. **`array-buffer.ts`/`typed-arrays.ts` IR coverage + Rust emission**: add the `DataView`
   codec and is-array-index-string intrinsics; port `BufferHooks` as a second hand-authored
   trait in `jade-tenant-rt` (mirrors the TS capability interface exactly — `allocate`/
   `byte_length`/`slice`/`read`/`write`). — Not started.
6. **`promise.ts` IR coverage + Rust emission**: `try`/`catch` lowering, the mixed
   generator/direct-`driveTenant`-call style (in Rust this collapses uniformly to direct calls
   since the distinction was only ever a TS ABI artifact), reaction-queue state machine.
   `toHostTask`/`awaitHostTask` excluded per above. — Not started.
7. **`realm.ts` IR coverage + Rust emission**: generated `PrimordialCache` struct assembly,
   hand-specialized `Object.entries` lowering. — Not started.
8. **Wire up regeneration**: `gen-primordials` binary reads `packages/jade-js/primordials/*.ts`,
   emits `crates/jade-primordial-rt/src/*.rs`, checked in with a "regenerate and diff" CI-style
   check — same discipline `scripts/regen.ts` already established for the opcode-generated
   files. — Not started; currently invoked ad hoc with `--file <path> [--rust]`.

## Tests

- **TS round-trip (Phases 1-2 onward):** re-emit each file's IR back to TypeScript into a
  scratch location and run the existing `primordials.e2e.ts` (and `trap.e2e.ts`,
  `tenant-compose.e2e.ts`, `exotic-tenant.e2e.ts`) suites against the round-tripped source via
  Node's TS-stripping mode — the same regression command `docs/primordials-plan.md` already
  prescribes, just pointed at generated output instead of the hand-written file, proving the
  IR is lossless rather than merely "looks plausible." (So far verified via `tsc --noEmit`
  against the project's own `tsconfig.json` for `types.ts`/`object.ts`'s round-tripped output;
  the e2e suites themselves haven't been run against generated output yet.)
- **Rust (from Phase 4 onward):** new unit tests in `crates/jade-primordial-rt/tests/` against
  a minimal in-crate test-double `Tenant` (`tests/support/mod.rs`'s `TestTenant` — the Rust
  analogue of how the TS side has `MultiTenant` as its reference implementation) — cache
  identity, the full Object/Function/Reflect/Proxy operation surface, buffer/typed-array
  byte-level round-trips across every numeric kind, and Promise state-machine behavior
  (resolve/reject/then/catch/finally, thenable assimilation, unhandled-rejection reporting)
  driven by a synchronous test `HostAsyncCapability` that runs microtasks immediately for
  deterministic assertions. (So far: `types_shim`'s full surface, `object.ts`'s `install_method`/
  `lock`.)
- Every phase's generated Rust must compile under the workspace's existing `cargo build`/
  `cargo test` before moving to the next phase — no phase ships with a known-broken generated
  artifact checked in. (See "Per-item emission" above for how a partially-covered file still
  meets this bar.)

## Open questions carried into implementation (not blocking plan approval)

- Exact generic shape of `HostAsyncCapability::Task<T>` (GAT vs. a simpler non-generic handle
  type) — decide when Phase 6 is reached, informed by whatever the first real Rust embedding's
  own async model looks like.
- Whether `make_exotic`/`make_callable_exotic` should take `Box<dyn Trait>` (current sketch,
  simplest) or a generic parameter — revisit once Phase 4's generated code shows real call-site
  ergonomics. (Landed as `Box<dyn Trait>`; no real friction observed yet.)
- **New:** how (or whether) to represent a guest-visible Array well enough for `Object.keys`/
  `Reflect.ownKeys` to return one — needs its own design pass, likely as a prerequisite to
  finishing `object.ts`/`reflect.ts` rather than a blocking part of this plan's original scope.

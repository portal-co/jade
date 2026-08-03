# Design note: what's blocking `proxy.ts`/`array-buffer.ts`/`typed-arrays.ts`'s Rust ports

## Status

`object.ts`/`function.ts`/`reflect.ts` are done: fully IR-lowered, generated, wired into
`jade-primordial-rt`'s module tree, and behavior-tested. Items 1 (`TenantExoticHandler`-from-
object-literal), 2 (`BufferHooks` shim), and `class` lowering (below, under "Recommended order"
item 5) are now all **done and tested**, and `array-buffer.ts` generates real, compiling,
tested Rust end to end. `typed-arrays.ts` is also done: it fully IR-lowers, generates, and
compiles, including `codecs` (item 6, closed — see its own subsection below for the full list of
gaps this closed). **`proxy.ts` is now done too (item 7, closed)**: `ProxyState` refactored into
a real class (`ProxyStateImpl`, `#target`/`#handler`/`#revoked` private fields, `requireLive`/
`target`/`handler`/`revoke` methods), `trap` redesigned as a top-level function instead of a
`proxyExotic`-local closure, `TrapResult` resolves structurally to `Option<T::Value>` in Rust with
no generated enum needed. `jade-primordial-rt` builds clean with `proxy.rs` wired into `lib.rs`,
and all 48 existing tests across `jade-primordial-ir`/`jade-primordial-rt`/`jade-tenant-rt` still
pass, with `array-buffer.rs` regenerating byte-for-byte identical and `typed_arrays.rs`
regenerating identical modulo the same nondeterministic clone-prelude ordering as before —
confirmed via diff. See item 7's own subsection below for the full list of gaps this closed, most
of them general emitter bugs no prior file had exercised (a class method that itself throws by
calling another throwing method, an owned/`Option`-typed local reused after being consumed into an
array-literal call argument, a cross-file top-level function call needing borrow-hoisting for a
nested `tenant` call). A TS-side behavioral test for the new revocable-proxy path (`revoke()`/
`existingState`) was added to `primordials.e2e.ts`, since none existed before.

**Not yet done**: a dedicated Rust-side behavioral test for either `typed_arrays.rs` or
`proxy.rs`'s generated output — blocked on a real but separate gap, not specific to either file:
class-lowering never generates a public accessor for a TS class's own *public* (non-`#`) fields,
only the private `Inner` storage, so nothing outside the generating module can reach a specific
typed-array constructor (or, for `proxy.ts`, `ProxyPrimordial.Proxy` is already a plain public
field, so this gap doesn't block a `proxy.rs` test the same way — but no such test exists yet
regardless). `array-buffer.ts` has the identical public-field gap (`ArrayBuffer`/
`ArrayBufferPrototype` are public in TS but Rust-inaccessible externally) and was never held to
this bar either — worth fixing generally (one new accessor-per-public-field case in class
emission) before any of these three files' Rust output gets a real behavior-level test, but out of
scope for this pass. With `proxy.ts` closed, all ten primordial files' original scope from
`docs/primordial-ir-plan.md` items 1-2 (proxy/buffer gaps) is now done; `promise.ts`/`realm.ts`
remain the plan's own later phases, untouched by this doc.

## One capability all three files need: `TenantExoticHandler` from an object literal — DONE

**Implemented and tested** (`emit_exotic_handler_literal` in `crates/jade-primordial-ir/src/
emit_rust.rs`, plus 6 regression tests in that file's own `#[cfg(test)]` module). Recognizes
`tenant.makeExotic(proto, { *get(...) {...}, ... })` and generates a real struct + `impl
TenantExoticHandler<T> for GeneratedStruct<T>`, exactly as proposed below — confirmed to compile
as real Rust (verified by temporarily dropping generated output into `jade-primordial-rt` and
running `cargo build`, then removed once confirmed; the permanent regression coverage is the
text-assertion unit tests, since this crate has no downstream compilation target of its own).

Three real bugs surfaced only by actually compiling the output, all now fixed:
- **`emit_expr`'s `Lit::Undefined`/`Lit::Null` mapping had changed** (earlier in this same
  session) from `None` to `tenant.undefined_value()`/`tenant.null_value()` — correct for a
  `T::Value`-returning position, wrong for `getOwnPropertyDescriptor`'s real `Option<
  TenantPropertyDescriptor<T::Value>>` return type. Fixed via a new `RETURN_COERCION` thread-local
  context (`ReturnCoercion::Closure` vs `ReturnCoercion::Trap(TrapReturn)`) that `Stmt::Return`'s
  emission consults, since `coerce_return_value` (built for `make_builtin`-style closures, always
  `T::Value`) and each exotic trap's *own*, trap-specific return type are genuinely different
  coercion rules sharing the same `emit_block`/`emit_stmt` code path.
- **A captured free variable** (`proto` in the example below) **needs a `self.<field>`
  shadow-clone prelude**, the same idiom `emit_closure` already uses for its own captures —
  the original object-literal method body references it as a bare identifier (lexical capture,
  free in TS), but in the generated struct it only exists as `self.proto`.
- **`PropertyKey` has no `PartialEq<str>` impl** — `key === "byteLength"`-style comparisons
  (extremely common in these traps) need the string literal converted via `PropertyKey::from
  (...)` first. New `PROPERTY_KEY_TYPED_LOCALS` tracking (mirroring the existing
  `STRING_TYPED_LOCALS`), populated for each trap's own `key`-named parameter.

**Known simplification, unchanged from the original proposal below:** every captured field
defaults to `T::Value`. Fine for a captured `T::Value` (proven — see the smoke-test example in
`emit_rust.rs`'s test module); wrong for `array-buffer.ts`'s real `records`/`hooks` captures,
which are a map and an embedder-capability parameter respectively — real type inference this
emitter still doesn't have. A wrong field type is a loud, immediate compile error at the
generated struct-literal construction site, never a silent miscompile.

**Scoped to `tenant.makeExotic` only** (not `makeCallableExotic`) — no primordial's own object
literal needs a callable exotic; every callable exotic in the surveyed source already goes
through `make_builtin`'s hand-written `BuiltinHandler`.

### Original proposal (implemented as described)

`proxy.ts`'s `native`, `array-buffer.ts`'s buffer shell, and `typed-arrays.ts`'s typed-array
records all follow the exact same TS idiom:

```ts
const native: TenantExoticHandler = {
  *get(receiver, key) { ... },
  *set(receiver, key, value) { ... },
  *has(_receiver, key) { ... },
  // ... one generator method per TenantExoticHandler trap
};
const value = yield tenant.yieldTenant(tenant.makeExotic(proto, native));
```

This is a genuinely new emitter shape (an object literal whose *values* are generator methods,
not data — `ObjectProp::Method` with `func.is_generator: true` — already representable in the
IR, never emitted), but it's mechanical, not a design question: `jade-tenant-rt::TenantExoticHandler`
(`crates/jade-tenant-rt/src/lib.rs`) already exists with exactly matching trap names
(`get`/`set`/`has`/`delete`/`own_keys`/`own_property_keys`/`get_own_property_descriptor`/
`define_property`/`get_prototype_of`/`set_prototype_of`/`is_extensible`/`prevent_extensions`/
`define`/`assign`), because it was hand-authored against this exact TS interface in the first
place. The natural translation is a generated struct (holding whatever the handler's own closure
state captures — for `array-buffer.ts`'s buffer shell, `records`/`proto`/`hooks`) with an
`impl TenantExoticHandler<T::Value> for GeneratedHandler<T> { fn get(&mut self, ...) { ... } ... }`,
each trait method's body being the corresponding object-literal method's `emit_block`-ed body —
the same shape `crates/jade-primordial-rt/src/types_shim.rs`'s hand-written `BuiltinHandler`
(inside `make_builtin`) already demonstrates, just generated instead of hand-written.

**This is the single most tractable, broadly reusable next step** — it doesn't require deciding
anything about the host-builtin or shared-mutable-state problems below, and unblocks real
progress on all three remaining files at once (though not *completion* of any of them — see the
per-file blockers below).

Concretely it needs: a recognized IR shape (an object literal where every value is
`ObjectProp::Method` and the surrounding call is `tenant.makeExotic(proto, THIS_LITERAL)` or
`tenant.makeCallableExotic(...)`), a name-mapping table (TS trap name → Rust trait method,
already implicit in `TenantExoticHandler`'s existing method names — just needs codifying as a
lookup analogous to `tenant_method`), and codegen for the generated handler struct plus its
captured-state fields (the same free-variable analysis `free_idents_in_fn` already does for
closures, but producing struct fields instead of a clone-prelude).

## `proxy.ts`'s own blockers (beyond the shared one above)

`proxy.ts` is the single largest file survyed — three problems, not one:

1. **A shared, *mutable* closure-captured object.** `proxyExotic`'s `state: ProxyState` (and
   `Proxy.revocable`'s own copy) is read by every trap method and *mutated* by the separate
   `revoke` builtin (`state.revoked = true; state.target = undefined; state.handler = undefined;`).
   The current closure-capture mechanism (`emit_closure`'s clone-prelude, see
   `docs/primordial-ir-plan.md`'s `function.ts` entry) clones captured values into each closure —
   correct for the read-only captures every other file has used so far, but silently *wrong*
   here: `revoke()` would mutate its own cloned copy, invisible to every trap that captured a
   separate clone. The natural fix is representing a TS `let`/`const`-bound object that's
   captured by more than one closure *and* mutated by at least one of them as `Rc<RefCell<...>>`
   in Rust (clone the `Rc` handle, not the data) — but this needs real analysis (which locals are
   captured-and-mutated vs. captured-and-read-only) that doesn't exist yet, and changes the
   closure-capture model for every future file, not just this one.
   **Proposed direction (not yet built):** sidestep the analysis entirely by changing the *source*
   instead of building mutation inference — rewrite `ProxyState` in `proxy.ts` itself from a plain
   `type` alias into a real `class` with `#private` fields (`#target`, `#handler`, `#revoked`) and
   methods (`requireLive()`, `revoke()`) implementing a small interface. This makes the Rust
   translation mechanical rather than analytical: a class is always reference-shared in real JS
   (unlike a plain object literal treated as a value-like record so far), so *every* class
   instance becomes `Rc<RefCell<Inner>>` uniformly, with no per-field mutation analysis needed —
   `readonly` vs. plain fields (already exposed by `swc_ecma_ast`'s `ClassProp::readonly`) is
   enough to know which fields even need the `RefCell`, not whether the *instance itself* needs
   `Rc` sharing (it always does, once it's a class). This requires adding `class` lowering to the
   IR (fields, constructor, methods, `#private` — encoding the whole class as a unit is fine even
   though `#private` field *references from outside the class* are a hard rejection elsewhere in
   this codebase, per `tenant_inline.rs`'s precedent, because nothing here splices the class body
   out to a different scope) — a real, scoped new capability, but one with a mechanical target
   representation instead of an open-ended inference problem.
2. **A discriminated union type alias.** `type TrapResult = { found: false } | { found: true;
   value: unknown };`, returned by the local generator closure `trap(name, args)` and consumed via
   `result.found ? result.value : ...`. The natural Rust translation is an enum
   (`enum TrapResult<V> { NotFound, Found(V) }`), but this is a new *kind* of type-alias lowering
   (a union of two object-literal shapes sharing a discriminant field) — `object.ts`'s interfaces
   and `array-buffer.ts`'s/`typed-arrays.ts`'s plain-object type aliases are both single-shape;
   nothing surveyed so far needed a tagged union.
3. **Tuple types and array destructuring with holes.** `requireLive(): [object, object]` (a tuple
   return type) and `const [, guestHandler] = requireLive();` (array destructuring that skips its
   first element). Both are new IR shapes (`Pattern`/`TypeRef` currently only cover
   object-shallow destructuring and named/generic/optional/array types).

Additionally, `proxyExotic`/`trap`/`fallback`/`requireLive` are all **local closures with
arbitrary signatures**, called synchronously or via `TenantYield` like an ordinary function —
not the fixed 2-param apply/construct convention every closure handled so far has used
(`emit_closure` currently hard-assumes that shape). Generalizing closure emission to arbitrary
signatures is needed for `proxy.ts` regardless of the three problems above.

## `array-buffer.ts`/`typed-arrays.ts`'s own blocker: real host JS builtins in `nativeBufferHooks`

`BufferHooks` (`array-buffer.ts`) is an explicit embedder capability interface — the
`bufferPrimordial`/`typedArraysPrimordial` factory logic that *consumes* it is tenant-generic and
about as portable as `object.ts`'s own factories. But `nativeBufferHooks`, the one concrete
implementation in the file, is built directly from real host JS: `new ArrayBuffer(...)`, `new
SharedArrayBuffer(...)`, `new Uint8Array(...)`, `.slice()`/`instanceof` on them. `typed-arrays.ts`'s
`codecs` table goes further, calling real `DataView` methods (`view.getInt8(i)`, etc.) directly.
None of this has a `Self::Value` translation — `Tenant::Value` is the *tenant's own opaque guest
value*, not a raw host buffer, and there is no existing (nor should there be an implicit) bridge
between them.

This is the exact shape of gap `docs/primordial-ir-plan.md`'s original design already anticipated
and explicitly excluded for `promise.ts`'s host-`Promise` boundary (`toHostTask`/`awaitHostTask`,
"a JS-host-interop boundary with no natural Rust translation ... excluded from IR translation
entirely; a human hand-writes the Rust equivalent"). The natural resolution is the same: treat
`nativeBufferHooks` as a **fourth shimmed module** (alongside `types.ts`), hand-porting it against
a new hand-authored `jade-tenant-rt::BufferHooks` trait that mirrors the TS interface exactly
(`allocate`/`is_handle`/`byte_length`/`slice`/`read`/`write`, all `Result<_, TenantError>`), the
same relationship `types_shim.rs` already has to `types.ts`. The *factory* functions
(`bufferPrimordial`/`typedArraysPrimordial` themselves, generic over `BufferHooks`) remain real
IR-lowered/generated targets — only the concrete native adapter is hand-written. `typed-arrays.ts`'s
`codecs` table (a fixed, small, closed set of numeric encodings) is a plausible fifth thing to
hand-port the same way, or to fold into the same `BufferHooks`-adjacent shim module — a
naming/placement detail to decide when this is picked up, not a design blocker.

**`BufferHooks` trait + `NativeBufferHooks` shim: DONE.** `jade-tenant-rt::buffer::BufferHooks`
(a `BufferKind` enum + the six trait methods above, `identity` dropped per the simplification
already decided) and `jade-primordial-rt::buffer_shim::NativeBufferHooks` (a genuinely native
Rust in-memory adapter — `Rc<RefCell<Vec<u8>>>`-backed handles, not a port of
`nativeBufferHooks`'s JS internals, since there's no host `ArrayBuffer` to wrap in a pure-Rust
embedding) are both written and unit-tested (6 tests: allocate/read/write round-trip, both
out-of-bounds error cases, slice's independent-copy semantics, `supports_shared_array_buffer`).

**Also fixed while landing this:** `lower_module` was all-or-nothing — a single top-level item
that can never lower (`nativeBufferHooks` itself, which uses `instanceof`, a `BinOp` this crate
doesn't model) aborted lowering for the *entire file*, silently preventing `bufferPrimordial`
from ever reaching the (already-existing, per-item-resilient) emission stage at all. Now
per-item resilient at the lowering stage too, matching `emit_rust.rs`'s already-established
per-item emission policy exactly (`eprintln!` and skip, never a whole-file abort for one bad
item) — see `lower_module`'s updated doc comment.

### More granular blockers found by actually running `array-buffer.ts` through the pipeline

Running the current (unrefactored) file through `gen-primordials --rust` after the two fixes
above surfaces four more gaps, all needed *before* the self-referential-closure problem even
becomes reachable — `bufferPrimordial` itself doesn't lower yet, for reasons independent of its
own body:

1. **`BufferKind` is a string-literal-union type alias** (`type BufferKind = "array-buffer" |
   "shared-array-buffer"`), used both as a type annotation and compared against its own string
   literals (`kind === "shared-array-buffer"`) throughout the file. The natural Rust target is
   the `BufferKind` enum *already hand-written* in `jade-tenant-rt::buffer` (built for this exact
   purpose) — meaning `BufferKind` needs **type-name shimming**: recognizing the TS name
   `BufferKind` as an alias for `portal_solutions_jade_tenant_rt::BufferKind`, and lowering each
   `kind === "<literal>"` comparison to a variant comparison (`kind == BufferKind::SharedArrayBuffer`),
   the same shape of translation `value_tag_variant`/`typeof_tag` already does for the seven
   `typeof` tag strings, just for a second, smaller closed vocabulary. This is a new kind of
   shimming this codebase doesn't have yet — `shims.rs`/`cross_file.rs` both resolve *calls*, not
   *type names*.
2. **`BufferHooks` needs to become a generic trait-bound parameter, not a struct.** Every other
   interface handled so far (`ObjectPrimordial`, `BufferRecord`, ...) becomes a concrete
   per-primordial generated struct. `BufferHooks` is different: it's the TS interface the
   *hand-written* `jade-tenant-rt::BufferHooks` trait already mirrors, so `bufferPrimordial(tenant:
   Tenant, hooks: BufferHooks)` needs to become `pub fn buffer_primordial<T: Tenant, H:
   BufferHooks>(tenant: &mut T, hooks: &mut H) -> ...` — an *additional* generic parameter on the
   enclosing function, not a type substitution at the parameter position alone. `emit_fn_decl`
   currently hardcodes `<T: Tenant>` as the only generic parameter on every generated function;
   this needs a way to detect "this function has a `BufferHooks`-typed parameter" and extend its
   own generic parameter list accordingly.
3. **`BufferHooks`/`BufferRecord`'s interface members are method-shaped** (`allocate(kind,
   byteLength): TenantGenerator<BufferHandle>`, TS method-signature syntax), not the
   `name: Type` property-signature shape every interface lowered so far has used.
   `lower_decl`'s `TsInterface` handling only recognizes `TsPropertySignature` today — needs a
   `TsMethodSignature` arm too (matters most for the type-checking side of things here, since
   `BufferHooks` itself becomes a trait-bound shim per (2) rather than a generated struct with
   real fields — but `BufferRecord`, a plain data interface with no method members, hits the same
   parse gap for an unrelated reason: see (4) below).

   Correction while writing this up: re-checking, `BufferRecord`'s own members
   (`readonly kind: BufferKind; readonly handle: BufferHandle;`) are plain property signatures,
   not method-shaped — its actual lowering failure is `BufferKind` not resolving (1) transitively.
   The `TsMethodSignature` gap is real but only actually blocks `BufferHooks` itself.
4. **The per-tenant cache's value type is an inline anonymous object type**, not a named
   interface: `new WeakMap<Tenant, { identity: object; primordial: BufferPrimordial }>()`.
   `lower_module_var_decl`'s `PerTenantCache` recognition only extracts a *named* type reference
   (`TsTypeRef`) from the `WeakMap`'s second type argument today; an inline `TsTypeLit` needs
   either its own synthesized `StructDef` (giving it a made-up name, e.g. `BufferPrimordialCache
   Entry`) or a decision that this specific shape (cache entry = `{ identity, primordial }`) is
   common enough across future files to deserve first-class recognition rather than a generic
   fallback.

## Newly discovered: `array-buffer.ts`'s `shell`/`constructor` are self-referential local closures

Found while actually designing (1)'s follow-on — not previously flagged. `bufferPrimordial`'s
`shell`/`constructor` are `const`-bound local generator closures with **arbitrary signatures**
(`shell(kind, handle)`, not the fixed apply/construct 2-param convention `emit_closure` currently
hard-assumes), and — the real problem — `shell` references **itself** from within a nested
closure it constructs (the `"slice"` builtin installed inside its own `get` trap calls `shell(...)`
again). A plain Rust closure can't do this: `let shell = |...| { ...shell(...)... };` doesn't
compile — the closure captures at construction time, before its own binding exists.

The clean fix is **not** a self-referential-closure trick (`Rc<RefCell<Option<Box<dyn FnMut>>>>`
and friends) but a representational change: `bufferPrimordial`'s whole body — its captured
mutable state (`records`, `prototypes`, `result`) plus its two interacting local closures — is
structurally a local **struct with inherent methods** (`struct BufferPrimordialBuilder<T, H> {
hooks: H, records: HashMap<...>, prototypes: HashMap<BufferKind, T::Value> }`, with `shell`/
`constructor` as `fn shell(&mut self, tenant: &mut T, kind, handle) -> ... { ...
self.shell(tenant, ...) ... }`), since methods can call themselves/each other via `self.method
(...)` with none of a closure's self-reference problem.

**Decided (2026-08-02): use the same mechanism as `proxy.ts`'s `ProxyState` problem below, not a
bespoke "detect co-referencing local closures" heuristic.** Refactor the *source* — turn
`shell`/`constructor`'s shared captured state (`records`, `prototypes`, and `hooks` itself) into
a real TS `class` in `array-buffer.ts`, the same way `ProxyState` becomes a class. This means
building exactly one new IR capability (`class` lowering — fields with readonly/mutable and
`#private` detection, constructor, methods) and using it for *two* real call sites, which is
better evidence it's a genuinely general capability than building it for one. See "Recommended
order" below for how this reorders the remaining work, and the `ProxyState` entry below for the
class-instance-as-`Rc<RefCell<Inner>>` Rust translation this implies (uniformly for *every* class
instance, not conditionally — see that entry's own note on why no per-instance capture-sharing
analysis is needed once instances are always `Rc<RefCell<_>>`: `emit_closure`'s existing
clone-prelude mechanism already does the right thing for free, since `Rc::clone` is cheap and
`.clone()` is exactly what that mechanism already emits for every capture).

This affects the recommended order below: (2) (`BufferHooks` shim, done) plus the type-system
gaps just above are necessary but **not sufficient** to fully generate `array-buffer.ts` — the
class-lowering work is a separate, comparably-sized piece, worth scoping and building once, then
applied to both files. `typed-arrays.ts` should be checked against the same self-reference
pattern before assuming it's simpler (not yet re-verified since this finding).

### The TS-side class refactor itself: DONE and verified

`array-buffer.ts`'s `bufferPrimordial` now returns a `BufferPrimordialImpl` class instance
(`implements BufferPrimordial`) instead of a plain object with closure-valued fields — decided
per the discussion above: the *whole* return value becomes the class instance (data fields +
`record`/`shell` as real methods), not just an internal helper wrapping `shell`/`constructor`'s
state, since `BufferPrimordial`'s own interface has function-typed members (`record`/`shell`)
that need the same treatment regardless. `#hooks`/`#records`/`#prototypes` are `#private`;
`ArrayBuffer`/`ArrayBufferPrototype`/`SharedArrayBuffer?`/`SharedArrayBufferPrototype?` are
public data fields (the two optional ones use `?`, the two required ones use `!` — definite
assignment, populated by `bufferPrimordial` right after construction, not in the constructor
itself, since construction can't `yield`).

**Real signature change, not just an internal refactor:** `BufferPrimordial.record`/`.shell`
both gained an explicit leading `tenant: Tenant` parameter. Neither needed it in the original
closure-based design (both closed over `tenant` lexically), but a Rust port's `record`/`shell`
need it explicitly — `typeof`/identity-keyed `WeakMap` lookups are ambient host operations in TS,
tenant-mediated ones in Rust (`Tenant::typeof_tag`/`object_id`) — and this interface's shape is
meant to be shared verbatim by both ports rather than diverging per-language. This cascaded into
6 call-site updates in `typed-arrays.ts` (`buffers.record(...)`/`buffers.shell(...)`, all of
which already had `tenant` in scope as a captured free variable).

**Verification, and a real gap found in the process:** `npx tsc --noEmit -p tsconfig.json`
reports zero errors both before and after the `typed-arrays.ts` call-site fix — turns out this
tells you nothing here. `buffers` (`bufferPrimordial`'s result) is typed via `yield tenant
.yieldTenant(...)`, and `TenantGenerator<R> = Generator<any, R, any>` has `any` for both its
yield and next-value type parameters, so *every* `yield tenant.yieldTenant(X)` expression's own
value is typed `any` throughout this codebase — meaning `tsc` silently accepts a wrong-arity
method call on `buffers` instead of flagging it. Real verification came from actually *running*
the code: `packages/jade-js/primordials.e2e.ts` (and the other 6 `*.e2e.ts` suites, run via `node
--experimental-strip-types`, matching `docs/primordials-plan.md`'s own regression-testing
prescription) exercises exactly the changed paths — `ArrayBuffer` construction + `.byteLength`
read (`shell`'s `get` trap), and a `Uint8Array` constructed from a bare length (exercises
`buffers.shell(tenant, ...)`) with an indexed `set`/`get` (exercises `buffers.record(tenant,
...)` from *inside* `typedArraysPrimordial`'s own exotic-handler traps) — and all pass. Worth
remembering for the rest of this note's remaining work too: **`tsc` cannot be trusted alone to
catch a signature-change regression on anything that flows through a `yield tenant.yieldTenant
(...)` expression; only running the real `*.e2e.ts` suites (or new dedicated tests) actually
proves it.**

## Recommended order for whoever picks this up

1. ~~Build the shared `TenantExoticHandler`-from-object-literal codegen first~~ **Done.**
2. ~~Author `jade-tenant-rt::BufferHooks` + hand-port `nativeBufferHooks`/`codecs` as shims~~
   **Done** for `BufferHooks` (`codecs` not ported yet — not needed until `typed-arrays.ts`).
   Also landed: `lower_module`'s per-item resilience fix (was blocking `bufferPrimordial` from
   lowering at all because of `nativeBufferHooks` alone, unrelated to anything below).
3. ~~Close the type-system gaps found by actually running `array-buffer.ts` through the
   pipeline~~ **Done**: `BufferKind` type-name shimming to the hand-written enum (`TYPE_SHIMS` +
   `BUFFER_KIND_VARIANTS`/`BUFFER_KIND_TYPED_LOCALS` in `emit_rust.rs`), `BufferHooks` as an added
   generic trait-bound parameter (`emit_fn_decl` now conditionally emits `<T: Tenant, H:
   BufferHooks>`), `BufferHooks`'s own interface erased entirely (method-shaped, and already a
   type shim — no need to parse `TsMethodSignature` for it), and the inline-`{ identity,
   primordial }`-wrapped per-tenant-cache value (`Item::PerTenantCache` gained an
   `identity_wrapped` flag; both the lookup and `.set(...)` peepholes in `emit_rust.rs` now
   recognize the wrapped shape and drop the identity check, which has no Rust-side meaning once
   the cache's own generic parameter already ties it to one hooks type). No regressions —
   `object.ts`/`function.ts`/`reflect.ts` still generate, compile, pass all 33 tests, and
   round-trip clean through `tsc`.
4. **New finding, found by re-running `array-buffer.ts` after (3):** `BufferPrimordial` (the
   interface `bufferPrimordial` itself returns) has *function-typed* members — `record(value):
   BufferRecord | undefined` and `shell(kind, handle): TenantGenerator<object>` are real methods
   on the returned object, not data fields. No interface-as-struct translation built so far
   handles a function-typed field. This argues for a **broader** class refactor than originally
   scoped: rather than only extracting `shell`/`constructor`'s *internal* shared state into a
   helper class, `bufferPrimordial`'s entire return value should become a class instance —
   `ArrayBuffer`/`ArrayBufferPrototype`/etc. as data fields, `record`/`shell` as real methods.
   This is the same underlying capability (class lowering), just applied at the function's public
   boundary instead of only internally; TS callers (`typed-arrays.ts` calls `.shell(...)` on the
   result) are unaffected either way since method-call syntax on the result is identical.
5. ~~Refactor `array-buffer.ts` so `bufferPrimordial`'s return value is a class instance~~
   **Done and verified** (`BufferPrimordialImpl`, `#private` state, `record`/`shell` gained an
   explicit `tenant` parameter, `typed-arrays.ts`'s 6 call sites updated to match — see "The
   TS-side class refactor itself" above; all 7 `*.e2e.ts` suites pass, `tsc` clean).
   ~~Design and build `class` lowering~~ **Done.** `ir.rs`: `Item::ClassDef`/`ClassField`/
   `ClassMethod`, `MemberProp::Private`, `Expr::NonNull` (a real node now — see below). `lower.rs`:
   `lower_class` (constructor as a flat `this.#field = expr;` sequence, fields incl. `#private`/
   `!`/`?`, methods incl. generators; rejects `extends`/statics/accessors/decorators), plus a
   general rule erasing *any* method-shaped interface at lowering (not just the hand-carved
   `BufferHooks` case), trusting a same-module class to back it. `emit_rust.rs`: every class
   becomes an `Rc<RefCell<Inner>>` wrapper + inherent `impl` (hand-written `Clone` doing
   `Rc::clone`), every method takes `&self` (mutation goes through the shared `RefCell`, never
   `&mut self`), and `const that = this;` — the `self`-capture idiom nested closures use to call
   back into the instance — is just `let that = self.clone();`, a cheap `Rc::clone` that
   `emit_closure`'s *existing* capture mechanism already handles once told (via
   `class_instance_obj`) that a captured name is a class instance rather than a bare `T::Value`.
   Two new `emit_call` branches: method calls on a known class instance (`that.shell(...)`), and
   method calls on a `BufferHooks`-typed class field (`that.#hooks.byteLength(...)`, its own small
   arg/return-convention table — `BUFFER_HOOKS_METHODS` — since `BufferHooks`' byte offsets are
   `f64` in TS but `usize` on the hand-written trait's side, a conversion `shims::ArgKind` doesn't
   express). `array-buffer.ts` now generates real, compiling Rust end to end (verified by copying
   the output into `jade-primordial-rt/src/array_buffer.rs`, wiring it into `lib.rs`, and running
   `cargo build`/`cargo test` — all 48 tests across the touched crates pass, zero regressions).
   See `docs/primordial-ir-plan.md`'s own updated entry for the long list of pre-existing gaps this
   surfaced (mostly things no prior IR-lowered file had ever exercised, not new problems this work
   introduced) and the two small TS-source adjustments (`self` → `that`, `shell` takes
   `objectPrototype` explicitly) that kept the translation clean rather than fighting the emitter.
   `typed-arrays.ts` itself hasn't been checked yet for the same self-reference pattern in its own
   `create` closure — do that before assuming it's a trivial follow-on.
6. `typed-arrays.ts`'s TS-side class refactor: **done**. `create` had the exact same
   self-referential-closure problem `shell` did (the `"subarray"` builtin it installs calls
   `create` again) — `TypedArrayPrimordialImpl` backs `TypedArrayPrimordial` the same way
   `BufferPrimordialImpl` backs `BufferPrimordial`, `#buffers`/`#hooks`/`#records`/`#prototypes`
   private, `create`/`makeConstructor` real methods calling each other and `array-buffer.ts`'s own
   `record`/`shell` through the captured `that`. `TypedArrayPrimordial.constructors` changed from a
   plain `Record<TypedArrayKind, Function>` to `Map<TypedArrayKind, Function>` (the IR has full
   `Map`/`WeakMap` support, none for a plain-object lookup table) — updated the two consumers
   (`primordials.e2e.ts`, `realm.ts`). Verified via `tsc --noEmit` and all 7 `*.e2e.ts` suites.
   Committed separately from Rust generation, same discipline as `array-buffer.ts`.

   Attempting Rust generation (`gen-primordials --file typed-arrays.ts --rust`) initially surfaced
   a **materially larger** set of gaps than `array-buffer.ts` needed. All but one are now **done
   and tested** (48 tests across `jade-primordial-ir`/`jade-primordial-rt`/`jade-tenant-rt` pass;
   `array-buffer.rs` regenerates byte-for-byte identical modulo nondeterministic clone-prelude
   ordering, confirmed via diff — no regression from any of this):
   - `BinOp::Div` was missing from the IR entirely — added in all three backends (`ir.rs`,
     `lower.rs`, `emit_ts.rs`, `emit_rust.rs`), mirroring the existing `Mul`/`Mod` handling.
   - `ReadonlyMap<K, V>` now resolves exactly like `Map`/`WeakMap` in `rust_type` (one extra name
     in that match arm's guard).
   - **`TypedArrayKind` now has a real Rust type.** New `Item::StringEnumDef` IR item: a
     string-literal-union type alias lowers to a real Rust `enum` (`emit_rust.rs`'s
     `emit_string_enum_def`) with an `AsRef<str>` impl, since `makeBuiltin(tenant, kind, ...)`
     needs the enum back as a string. `TYPE_SHIMS`-registered names (`BufferKind`) are skipped —
     that table remains the authority for names with an earlier, hand-written counterpart.
     `make_builtin`'s own `name` parameter changed from `&str` to `impl AsRef<str>` to accept
     either a literal or an enum value with no call-site conversion needed (backward-compatible
     with every existing call site, verified by rebuilding `jade-primordial-rt`).
   - **`Record_` now has a real Rust type too.** A plain object-shaped type *alias* (not just
     `interface`) now also lowers to a `StructDef` when its right-hand side is a `TsTypeLit` —
     `lower_type_elements` factors the shared field-layout logic out of the `interface` case.
   - **Cross-file class/factory resolution now exists**, via two additions to `cross_file.rs`
     rather than a full multi-file compilation pass (Phase 8's eventual job): `CrossFileFactory`
     gained `extra_params` (positional arguments beyond the leading `tenant` — `bufferPrimordial`'s
     `hooks`) and `transitive_caches` (other factories' cache parameters this one's generated
     function *also* needs, because it calls them internally — `buffer_primordial` calls
     `objectPrimordial`, so a caller of `bufferPrimordial` needs `object_primordial_cache` too,
     which `objectPrimordial` itself never surfaced having no further dependencies of its own). A
     new `CrossFileClass`/`CLASS_TABLE` registers `BufferPrimordial`'s two cross-file-called
     methods (`record`, `shell`) by hand (their own `record`/`shell` param types), consulted by
     `type_needs_tenant`/`type_needs_buffer_hooks`, `rust_type`'s fallback chain, and a new
     `emit_call` branch (method call on a class field typed as a cross-file class's interface —
     mirrors the existing `BufferHooks`-field-method-call branch, calling straight through the
     `.inner.borrow()` since the field is already an `Rc<RefCell<...>>`-backed, reference-shared
     value with no clone needed). The self-import skip when generating a file's own prelude
     (`emit_rust.rs`'s cross-file `use` loop) now also checks `CLASS_DEFS`, not just `STRUCT_DEFS`
     — needed since `BufferPrimordialImpl` is a class, not a struct, and `array-buffer.ts` would
     otherwise self-import its own type when regenerated now that it's in the table.

   **`codecs` itself: done.** Rather than modeling `Record<TypedArrayKind, Codec>` as a real Rust
   value (a `HashMap` rebuilt on every access, or closures living in a `const`, neither necessary
   for a small fixed table), `try_emit_data_view_codec_table` recognizes the whole shape as a unit
   (the same philosophy as `Item::PerTenantCache`) and emits three inherent methods directly on
   `TypedArrayKind` — `codec_bytes`/`codec_get`/`codec_set` — extracting each entry's byte width and
   `DataView` method suffix structurally from the real lowered `Expr` tree (not a hand-duplicated
   parallel table, so it can't drift from the TS source), while lowering the setter's own value
   expression (`Math.max(0, Math.min(255, Math.round(x)))` for `Uint8ClampedArray`, bare `x`
   otherwise) through the ordinary `emit_expr` pipeline. `new DataView(X.buffer, ...)` is erased
   entirely at lowering time (`lower.rs`) to just `X` — Rust's byte-conversion functions
   (`f64::from_le_bytes` etc.) need no separate "view" wrapper. `Codec`'s own type alias (method
   signatures, not properties) still doesn't get a `StructDef` — expected and harmless, since
   nothing needs it once `codecs` is expressed as enum methods instead of real struct values.

   Chasing this from "lowers" to "actually compiles" surfaced a long tail of **real, general
   gaps** no prior generated file had exercised (typed-arrays.ts is the first file with real
   byte-level number/`usize` arithmetic, an early-return `Option` guard, and a "void" closure whose
   last statement isn't an explicit `return`) — all fixed in `emit_rust.rs`, none typed-arrays.ts-
   specific:
   - **A block-scoping bug in `Stmt::If`**: a `const`/`let` declared inside an `if`/`else` branch
     leaked into `LOCAL_REFNESS` for the rest of the enclosing function, so a *later*, unrelated
     closure redeclaring the same name (its own local, not a real capture) got wrongly treated as
     capturing an "outer" binding — `get`'s trap has two sibling branches that both declare their
     own `const bytes = ...`. Fixed by saving/restoring `LOCAL_REFNESS` around each branch.
   - **`Number(x)`/`is_array_index_string`/`typeof key === "string"`** previously assumed `x`/`key`
     was always a `T::Value` or always a `PropertyKey` respectively — both now branch on which one
     it actually is (`Number(key)` parses `PropertyKey`'s `Display` text; `Number(value)` calls
     `Tenant::to_number`; a `PropertyKey`-typed `typeof === "string"`/array-index check matches the
     `String` variant directly instead of calling `Tenant::typeof_tag`, which doesn't apply to a
     host-level key at all).
   - **`toIndex`'s Rust return type (`usize`) now gets cast to `f64` once, at the shim call site**,
     instead of leaking `usize` into arithmetic that's `f64` everywhere else it's used
     (`source.offset + i * codec.bytes`) — every existing caller either already cast explicitly at
     the point of use (harmless double-cast) or benefits from the fix directly.
   - **A native arithmetic expression (`record.length * codec.bytes`) or a `number`-typed struct
     field (`record.length`) returned directly from a trap** now gets `Tenant::number_value`
     wrapping, the same as a bare numeric literal already did — trap return coercion previously
     only recognized literals and two hand-picked call shapes.
   - **A bare arithmetic expression used as an `if` condition** (`if (offset % codec.bytes)`) now
     gets an explicit `!= 0.0` — TS's "truthy number" has no Rust equivalent.
   - **`if (x) { ... }` for an `Option`-typed `x`** (no negation — `const bufferRecord =
     that.#buffers.record(...); if (bufferRecord) { ...bufferRecord.handle... }`) now becomes a
     real `if let Some(x) = x.clone() { ... }`, alongside the existing `if (!x) continue;`/`if (!x)
     throw;` narrowing forms. A `Map.get(...)`/cross-file-class method registered
     `returns_option: true` (`BufferPrimordial.record`) now also marks its own local
     `Option`-typed, which those narrowing forms depend on to fire in the first place.
   - **A closure whose body falls off the end without an explicit `return`** (`typed-arrays.ts`'s
     `set` builtin — a "void" JS function whose last statement is a loop) now gets a trailing
     `Ok(tenant.undefined_value())` appended; every closure before this had happened to always end
     in an explicit `return`/`throw`.
   - **A `BufferKind`-typed argument fed a string literal directly** (`hooks.allocate("array-buffer",
     ...)`) now converts through `buffer_kind_literal` — previously only handled at one call site
     (`emit_call`'s class-instance-method-call branch), not `BufferHooks` method calls generally.
   - **A `BufferHooks`-typed parameter used more than once in the same function** (`typedArraysPrimordial`
     passes `hooks` to both `bufferPrimordial(tenant, hooks)` and `new
     TypedArrayPrimordialImpl(...)`) needed `H: Clone` added to the generated bound, and the
     cross-file-factory call site to clone rather than move it — `NativeBufferHooks` now derives
     `Clone` (trivial; it's a unit struct).
   - **A class constructor parameter typed `object`/`Function`/`unknown`** (`objectPrototype:
     object`) is `emit_param`'s reference-typed convention (`&T::Value`), but the field it's stored
     into is owned (`T::Value`) — constructor field-assignment now clones a reference-typed param
     when storing it, the same gap `BufferHooks`-typed constructor params never had (those are
     already owned on both sides).
   - **An interface a same-module class explicitly `implements`** now always resolves to that
     class in `rust_type`, even when the interface's own shape also happens to be plain-data
     enough to have gotten a `StructDef` of its own (`TypedArrayPrimordial { constructors: ... }` —
     unlike the method-shaped `BufferPrimordial`, which has no competing `StructDef` at all, so
     this ordering never mattered before). Was also a real bug in `typed-arrays.ts` itself: the
     class never declared `implements TypedArrayPrimordial` in the first place — fixed alongside.

   All fixed, all covered by the existing 48-test suite passing with zero regressions, confirmed
   by `array-buffer.rs` regenerating identically. `typed_arrays.rs` is wired into
   `jade-primordial-rt/src/lib.rs` and `jade-primordial-rt` builds clean end to end.
7. `proxy.ts` last: **done**. Reused the same class lowering `ProxyState` was always meant to
   use, but the TS source needed real adjustment first, not just IR coverage:
   - **`ProxyState` refactored from a plain mutable object into `ProxyStateImpl`**, a real class
     implementing a method-shaped `ProxyState` interface (`requireLive`/`target`/`handler`/
     `revoke`). `#target`/`#handler` stay non-`Option`-typed (unlike the original design's minor
     GC-hint of clearing them on `revoke()`) specifically to avoid needing truthiness narrowing on
     an `Option`-typed *class field*, which only works for a bare local variable so far (via
     `OPTION_LOCALS`) — `#revoked: bool` alone gates all access through `requireLive()`.
   - **`trap` redesigned as a top-level function**, not a closure local to `proxyExotic` — closure
     lowering only recognizes the fixed `makeBuiltin` apply/construct shape (`thisArg`/`args`,
     returning `T::Value`), which `trap`'s own shape (`name: string` first, returning `TrapResult`)
     doesn't match. `tenant`/`state` became explicit parameters instead of captured closure state,
     the same adjustment `array-buffer.ts`'s `shell`/`record` made earlier.
   - **`TrapResult` (`{found: false} | {found: true; value: unknown}`) resolves directly to
     `Option<T::Value>`** via a name-based special case in `rust_type`, not a generated enum — no
     general discriminated-union support was built (see "Explicitly not proposed here", still
     accurate: this stayed exactly as narrow as planned). `.found`/`.value` reads translate to
     `.is_some()`/`.clone().unwrap()` via a new `TRAP_RESULT_TYPED_LOCALS` local-tracking set.
   - A separately-declared `const native: TenantExoticHandler = {...}; tenant.makeExotic(null,
     native);` was inlined directly into the `makeExotic(...)` call — handler-literal recognition
     only fires when the object literal is inline, not referenced via a variable.
   - `existingState` became a required (not optional) parameter, with callers passing `undefined`
     explicitly, matching the parameter-registration machinery below rather than fighting it.

   Getting the resulting IR to lower and the Rust to actually compile surfaced a long tail of
   **general emitter gaps**, none `proxy.ts`-specific, all fixed in `emit_rust.rs` and covered by
   the existing 48-test suite passing with zero regressions:
   - **`new_class_instance_name`** now recognizes `existing ?? new ClassName(...)` (nullish-coalesce
     reusing a possibly-passed-in instance), not just a bare `new ClassName(...)`, so `const state =
     existingState ?? new ProxyStateImpl(...)` registers `state` in `CLASS_INSTANCE_LOCALS`.
   - **Class/interface-typed function *parameters*** now register into `CLASS_INSTANCE_LOCALS`
     during `emit_fn_decl` (previously only `Stmt::Let` bindings did) — needed for `trap`'s own
     `state: ProxyState` parameter, so `state.target()`/`state.handler()` resolve as method calls.
     `emit_fn_decl` also gained a `CLASS_INSTANCE_LOCALS.clear()` at its top, matching
     `emit_class_method`'s existing discipline (a latent gap: no top-level function had ever needed
     a class-typed parameter before, so nothing had ever exercised stale entries leaking in).
   - **Optional non-guest-value-typed parameters** (`existingState: ProxyState | undefined`) now
     register into `OPTION_LOCALS` during `emit_fn_decl`, routing through the ordinary
     `.unwrap_or(...)` translation instead of the bare-ident nullish-check heuristic added earlier
     for `Number(value)`-style cases (which assumed any non-`OPTION_LOCALS` bare ident was a guest
     `T::Value`).
   - **A private class field read directly returned** (`return this.#target;`) needed an explicit
     `.clone()` neither `coerce_return_value` nor `coerce_literal_to_value` added before —
     `emit_member`'s class-field branch deliberately returns a *borrow* through
     `self.inner.borrow().<field>` (correct for its only previously-observed use, being the
     receiver of a further call, which bypasses this coercion entirely via `emit_call`'s own
     class-field-aware branches), but a bare `return` of that borrow tries to move out of a
     temporary `Ref` that doesn't outlive the statement. Fixed in both return-coercion paths.
   - **A class method that itself throws only by calling another throwing method**
     (`ProxyStateImpl::target`/`::handler` calling `require_live()`, with no `throw` of their own)
     wasn't detected as throwing at all — `block_throws`/`expr_throws` only looked for a syntactic
     `throw`, not a transitive call. Fixed with a new one-hop case in `expr_throws` (a call to
     another same-class method throws if that method's own body does) plus a new
     `class_method_throws(class_name, method_name)` helper — needed because naively re-running
     `block_throws` on another method's body from an unrelated caller's context resolves `this`
     against whatever class is *currently* being emitted (`CURRENT_CLASS`), not the target method's
     own class, unless `CURRENT_CLASS` is temporarily pointed at it first.
   - **A non-generator class method call site now appends `?` when the target method throws** —
     previously assumed "never `?`-suffixed: every generator method call is `TenantYield`-wrapped
     (which appends its own `?`), a non-generator method is never throwing" — true until
     `target`/`handler` became the first non-generator *and* throwing methods observed.
   - **`keys as PropertyKey[]`** (an array-typed TS cast, `ownKeys`/`ownPropertyKeys`'s own
     `guestArrayLike(...)` result) needed per-element `Tenant::to_property_key` conversion —
     `emit_cast` only had a single-value `PropertyKey` case before, no `TypeRef::Array` case.
   - **`getOwnPropertyDescriptor`/`getPrototypeOf`'s `coerce_trap_return_value`** double-wrapped an
     already-`Option`-shaped trap-return expression (`return yield tenant.yieldTenant(tenant
     .getOwnPropertyDescriptor(...))`) in an extra `Some(...)` — a pre-existing bug in the
     `OptionValue`/`OptionDescriptor` branch, never exercised before because no prior trap had ever
     forwarded straight through to the corresponding `Tenant` method's own already-`Option`-typed
     result. Fixed by reusing `returns_option` (which already recognized this exact tenant-call
     shape for a different purpose) as an extra "don't re-wrap" check.
   - **Guest-value `!x` truthy-checks** (`!result.value`, `TrapResult`'s unwrapped payload) needed
     `Tenant::to_boolean` — Rust's `!` only applies to `bool`, but TS's `!`/`!!` truthy-checks any
     value. Also fixed a name collision: `result.value === undefined` was being mis-recognized as a
     `TenantPropertyDescriptor.value` field check (`emit_eq_cmp`'s `DESCRIPTOR_FIELDS` heuristic),
     needing an explicit `TRAP_RESULT_TYPED_LOCALS` exclusion guard.
   - **A same-module top-level function call's own array-literal argument
     (`emit_value_slice_array_literal`, `trap`'s `args: readonly unknown[]`)** needed several
     per-element fixes beyond the original `PropertyKey`-conversion case: a `&T::Value`-typed bare
     local (`receiver`/`descriptors`/`source`) needs `.clone()` for this owned-`T::Value` slot; an
     `Option<T::Value>`-typed local (`setPrototypeOf`'s `prototype`) needs
     `.clone().unwrap_or_else(|| tenant.null_value())`, cloned first since it's reused after the
     call in the trap's own fallback branch; and a plain owned `T::Value` local (`set`'s own
     `value` parameter) also needs `.clone()` for the same reused-after-the-call reason — settled
     on cloning *every* bare-ident element unconditionally rather than trying to prove which ones
     are single-use, since a guest `T::Value`/class-instance clone is always cheap and correct here.
   - **The same call's own leading arguments** needed two more general fixes mirroring
     `emit_shim_call`'s existing ones: `maybe_hoist_arg` (hoisting a nested-`tenant`-borrowing
     argument into its own `let` before the call, to avoid `tenant` being borrowed twice in one
     statement) wasn't wired into the local-function-call branch at all; and a class-instance-typed
     argument passed by value (`trap(tenant, state, ...)`, `state: ProxyState`) was being *moved*
     rather than cloned, breaking every later reuse of the same local in the same trap body
     (sometimes even later in the very same call's own array-literal argument) — now cloned like
     any other class-instance value, matching the `Rc<RefCell<...>>` cheap-clone convention used
     everywhere else in this codebase.
   - **A `ClassInterface | undefined`-typed call argument** (`proxyExotic(tenant, target, handler,
     existingState)`/`proxyExotic(..., undefined)`) needed the same `None`/`Some((...).clone())`
     handling `ArgKind::OptionalValue` already gives a guest-value `Option`, but for a class
     instance instead — no `TypeRef::Optional` case existed in the local-function-call argument
     loop at all before this.
   - **A new `functionPrimordial` cross-file factory entry** in `cross_file.rs` (`FunctionPrimordial`
     for `FunctionPrototype`, transitively depending on `ObjectPrimordial`'s own cache) — the same
     kind of entry `bufferPrimordial` already had, just for `function.ts` instead of
     `array-buffer.ts`.

   All fixed, all covered by the existing 48-test suite passing with zero regressions;
   `array-buffer.rs` regenerates byte-for-byte identical and `typed_arrays.rs` regenerates identical
   modulo the same nondeterministic clone-prelude ordering as before. `proxy.rs` is wired into
   `jade-primordial-rt/src/lib.rs` and `jade-primordial-rt` builds clean end to end.

## Explicitly not proposed here

- A general discriminated-union-to-Rust-enum transpiler for arbitrary TS unions — only the one
  `TrapResult` shape is needed; whatever's built should be scoped to what `proxy.ts` actually
  uses, not generalized ahead of a second real example.
- General tuple-type/arbitrary-arity destructuring support — same reasoning, scope to `[T, T]`
  2-tuples and single-hole array patterns, the only shapes `proxy.ts` uses.
- Any change to `promise.ts`'s already-decided host-`Promise`-boundary exclusion — this note's
  `nativeBufferHooks` recommendation is explicitly modeled on that decision, not a reopening of it.

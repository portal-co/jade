# Design note: what's blocking `proxy.ts`/`array-buffer.ts`/`typed-arrays.ts`'s Rust ports

## Status

Design only — no code changes. Written after actually reading all three files in full and
tracing exactly which new IR/emitter constructs each would need, the same way
`docs/array-primordial-gap-plan.md` was written before that gap was closed (see
`docs/primordial-ir-plan.md`'s "Progress" section for the object.ts→reflect.ts run that produced
this note). `object.ts`/`function.ts`/`reflect.ts` are done: fully IR-lowered, generated, wired
into `jade-primordial-rt`'s module tree, and behavior-tested. This note covers the next three
files, which are qualitatively harder than anything covered so far — do not start extending
`jade-tenant-rt::Tenant` or the closure-capture model until a follow-up session picks a scope
from this note and it's signed off, same discipline as the previous gap note.

## One capability all three files need: `TenantExoticHandler` from an object literal

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

## Recommended order for whoever picks this up

1. Build the shared `TenantExoticHandler`-from-object-literal codegen first (tractable, no open
   design questions, unblocks real (partial) progress on all three files).
2. Author `jade-tenant-rt::BufferHooks` + hand-port `nativeBufferHooks`/`codecs` as shims, the
   same relationship `types_shim.rs` has to `types.ts`. This plus (1) should be enough to fully
   generate `array-buffer.ts` and `typed-arrays.ts`.
3. `proxy.ts` last: needs (1) plus generalized arbitrary-signature closure emission, tuple
   types/holey array destructuring, discriminated-union-as-enum type aliases, and a real decision
   on `Rc<RefCell<...>>`-style shared-mutable capture detection — likely worth its own follow-up
   note once (1) and (2) have proven out the object-literal-handler codegen in practice.

## Explicitly not proposed here

- A general discriminated-union-to-Rust-enum transpiler for arbitrary TS unions — only the one
  `TrapResult` shape is needed; whatever's built should be scoped to what `proxy.ts` actually
  uses, not generalized ahead of a second real example.
- General tuple-type/arbitrary-arity destructuring support — same reasoning, scope to `[T, T]`
  2-tuples and single-hole array patterns, the only shapes `proxy.ts` uses.
- Any change to `promise.ts`'s already-decided host-`Promise`-boundary exclusion — this note's
  `nativeBufferHooks` recommendation is explicitly modeled on that decision, not a reopening of it.

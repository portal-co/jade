# Design note: the guest-Array gap blocking the primordial-IR Rust port

## Status

Design only — no code changes. Written because `objectPrimordial`'s Rust translation
(`crates/jade-primordial-rt/src/object.rs`, see `docs/primordial-ir-plan.md`'s "Progress"
section) is blocked on this, and every other primordial file transitively calls
`objectPrimordial` for `ObjectPrototype` — so this is the actual critical-path blocker for
`function.ts`/`reflect.ts`/`proxy.ts`/`array-buffer.ts`/`typed-arrays.ts`'s Rust ports too, not
an isolated `Object.keys` edge case. Do not extend `jade-tenant-rt::Tenant` until a follow-up
session picks a scope from this note and someone signs off on it.

## The gap, precisely

`Object.keys(obj)` / `Reflect.ownKeys(obj)` (and, by inheritance, `Object.prototype
.hasOwnProperty`'s sibling methods that don't hit this — only the two that *directly return* a
key list do) call `tenant.ownKeys`/`tenant.ownPropertyKeys`, whose TS signature is
`(obj: object) => TenantGenerator<PropertyKey[]>` (`packages/jade-js/tenants/types.ts:64,133`).
Checking what a **real** implementation returns there (not a hypothetical): `single.ts:115`
returns `Reflect.ownKeys(obj).filter(...)` and `multi.ts:195` returns whatever a native exotic
handler's own `ownKeys` returns — in both cases, a genuine **host-native JS `Array`**.
`object.ts`'s `Object.keys` closure returns that array *directly* as its own result:

```ts
installMethod(tenant, ObjectFn, "keys", function* (_thisArg, args) {
  assertObject(args[0]);
  return yield tenant.yieldTenant(tenant.ownKeys(args[0]));
});
```

This is **not a bug or a shortcut in the TS source** — it's simply how the current
TS-hosted-in-a-real-JS-engine implementation works: there is one heap, "host value" and "guest
value" are the same representation for anything that isn't tenant-managed, and a native `Array`
returned from a builtin's `apply` is already a perfectly ordinary guest-observable value with
full `Array.prototype` behavior, no marshaling required. Nothing needs designing on the TS side.

The gap is entirely on the Rust-port side: `jade-tenant-rt::Tenant::Value` is opaque, has no
built-in concept of "array," and `own_keys`/`own_property_keys` return a plain host-side
`Vec<PropertyKey>` — a Rust collection, not a `Self::Value`. There is currently no way to turn
that `Vec<PropertyKey>` into a `Self::Value` at all, so the generated Rust closure for
`Object.keys` has no sound return statement — see `crates/jade-primordial-ir/src/emit_rust.rs`'s
`coerce_return_value`, which currently hard-rejects this shape by name rather than guessing.

## What "real" fidelity would require, and why that's a separate, much bigger project

A byte-for-byte faithful Rust equivalent of what `Object.keys` returns today would need a real
guest-visible Array: `.length`, numeric indexed get/set, `Array.prototype` methods
(`.map`/`.filter`/`.forEach`/`.push`/`.slice`/...), `Array.isArray`, array literal/spread/
destructuring support at the bytecode level, and iteration protocol (`for...of`, spread) which
**Jade doesn't have at all yet for any guest value** — there's no `Symbol.iterator` concept
anywhere in `packages/jade-js/tenants/types.ts`'s `Tenant` interface today. Building that is a
project on the scale of the original `Object`/`Function`/`Reflect`/`Proxy` milestone
(`docs/primordials-plan.md`), not a one-off fix to unblock a Rust port — it would need its own
plan doc, its own TS primordial file (`packages/jade-js/primordials/array.ts`, following the
same factory/cache/interface pattern every other file already does), and its own phased rollout.
**This note does not propose building that.**

## What's actually needed to unblock the Rust port specifically

The narrow question is: what's the smallest, honest primitive that lets `Object.keys`'s Rust
translation produce a `Self::Value` at all, without claiming more behavior than it delivers?

Two conversions are missing, not one:

1. **`Vec<PropertyKey>` → some kind of guest-observable collection value.** `PropertyKey` itself
   is a host-side Rust type (`String`/`SymbolId`), not a `Self::Value` — so this also needs...
2. **A single `PropertyKey` → `Self::Value`.** There's no existing primitive for this — the
   existing `to_property_key`/`nullable`/`typeof_tag` family all go the *other* direction (guest
   value → host fact). A `String` key converts trivially via the existing `string_value`; a
   `Symbol` key has no guest-value constructor at all yet (`SymbolId` is purely an opaque
   embedder-allocated id today — see `jade-tenant-rt::SymbolId`'s doc comment). Real own-key
   lists usually are all-string in the primordials' own internal usage, but "usually" is exactly
   the kind of gap this codebase's stated philosophy (`tenant_inline.rs`'s precedent, this
   session's own `coerce_return_value`) says shouldn't be silently approximated.

### Proposed shape (for a future session to actually build, not decided here)

A single new `Tenant` method, deliberately not named "array" to avoid implying `Array.prototype`
fidelity it doesn't have:

```rust
/// Constructs a tenant-owned, guest-observable ordered collection from `values` — indexed
/// numeric access (`0..values.len()`) and a `length` property, and nothing else. Does **not**
/// claim `Array.prototype` behavior (no `.map`/`.push`/`Array.isArray`/spread/iteration
/// support) — see `docs/array-primordial-gap-plan.md`. A `Tenant` implementation is free to
/// back this with a full guest `Array` if one exists in its embedding; the contract this trait
/// makes is only the two properties named above.
fn indexed_collection(&mut self, values: Vec<Self::Value>) -> Result<Self::Value, TenantError>;
```

Built on the *existing* primordial machinery, not raw host state: a real implementation would
naturally express this as `tenant.make_exotic(None, handler)` with a handler whose `get` trap
recognizes `"length"` and numeric-string keys — precisely the same shape `array-buffer.ts`/
`typed-arrays.ts` already use for their own indexed exotics (`array-buffer.ts`'s buffer shell,
`typed-arrays.ts`'s typed-array records), so this isn't a new pattern for the codebase, just a
new named instance of an existing one.

Paired with a key-to-value conversion:

```rust
/// The inverse of `to_property_key`: the guest value a given key would compare `===` to. A
/// string key converts via `string_value`; a symbol key has no guest-value representation yet
/// (see this note's "still needed" list) — an implementation may return `Err` or a
/// deliberately-inert placeholder value for that case until symbol values are designed, but
/// must not silently stringify a symbol (that would be observably wrong, not just incomplete).
fn property_key_value(&mut self, key: &PropertyKey) -> Result<Self::Value, TenantError>;
```

`emit_rust.rs`'s `coerce_return_value` would then translate `tenant.ownKeys(...)`-as-a-direct-
return into:

```rust
let keys = tenant.own_keys(&obj)?;
let values = keys.iter().map(|k| tenant.property_key_value(k)).collect::<Result<Vec<_>, _>>()?;
tenant.indexed_collection(values)
```

### Explicitly still out of scope after this

- `Array.prototype` methods, `Array.isArray`, `new Array()`/`Array.of`/`Array.from` — none of
  these are needed by any current primordial's own internals, and adding them means designing a
  real `array.ts` primordial (TS-side work, not a Rust-port concern).
- Symbol-valued guest keys — `property_key_value`'s `Symbol` arm has no real answer until guest
  `Symbol` values themselves are designed (out of scope for every primordial file surveyed so
  far; none of them construct a guest-visible symbol).
- Iteration protocol (`for...of`, spread of a guest value into host code) — `indexed_collection`
  only promises `length` + indexed get, which is enough for `Object.keys`'s own current TS
  behavior's *consumers within the primordials* (none of which iterate the result — `Object.keys`
  is a leaf call in every surveyed usage) but not enough for arbitrary guest code that expects
  real iterability.
- Mutation (`.push`, indexed `set`) — `Object.keys`'s result is conventionally not mutated by
  its own callers; no primordial file needs a writable result here.

## Where this plugs into the existing plan

- `jade-tenant-rt/src/lib.rs`: the two new `Tenant` methods, next to `nullable`/`to_property_key`
  in the "value introspection/construction" group.
- `jade-primordial-ir/src/emit_rust.rs`: `coerce_return_value`'s `KEY_LIST_RETURNING` branch
  (currently a named rejection) becomes the translation shown above instead.
- Test-double coverage: `crates/jade-primordial-rt/tests/support/mod.rs`'s `TestTenant` needs a
  real (if simple) `indexed_collection`/`property_key_value` implementation before any test can
  exercise `Object.keys`/`Reflect.ownKeys` end-to-end.
- Once this lands, `objectPrimordial` (`object.ts`) should generate and compile in full, which
  in turn unblocks `functionPrimordial` (`function.ts`, already IR-covered and generated as of
  this note — see `docs/primordial-ir-plan.md` — but not yet wired into `jade-primordial-rt`'s
  module tree because it references `crate::object::object_primordial`, which doesn't exist
  until this gap closes) and, transitively, `reflectPrimordial`/`proxyPrimordial`.

## Open question for whoever picks this up

Is `indexed_collection`'s all-or-nothing `Vec<Self::Value>` construction sufficient, or does any
later primordial need to build one up incrementally (indexed `set` during construction, before
the collection is "finished" and exposed)? Nothing surveyed so far needs that, but it's worth
checking against `typed-arrays.ts`'s own construction patterns before committing to the
signature above.

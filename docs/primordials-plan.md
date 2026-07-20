# Plan: tenant-scoped JavaScript primordials

## Summary

Jade currently receives a host `globalThis`, while guest-visible object behavior is
already mediated by a `Tenant`.  This plan adds a tenant-scoped primordial layer:
standard-library values are *guest values owned by that tenant*, rather than raw
host intrinsics accidentally shared by every guest.

The initial set is deliberately small:

- `Object`
- `Function`, **without** dynamic source compilation (`Function(...)` and
  `new Function(...)`)
- `Reflect`
- `Proxy`, implemented with Jade's native tenant-exotic system rather than a
  host `Proxy`
- `ArrayBuffer`, plus an optional `SharedArrayBuffer`
- typed-array constructors and prototypes, backed by the same abstract buffer
  interface as `ArrayBuffer`

Each primordial is implemented in its own file under
`packages/jade-js/primordials/`.  Each file exports a factory which accepts a
`Tenant` and retains one result per tenant in a private `WeakMap`.  A small realm
assembler creates the guest global object and installs the requested primordial
values.

This is a design and implementation plan.  It does not itself move files or
change runtime behavior.

## Why tenant-scoped primordials

Giving guest code raw `globalThis.Object`, `globalThis.Reflect`, or native typed
arrays defeats several existing boundaries:

- guest object-property accesses must use `tenant.get/set/has/delete/ownKeys/
  define/assign`, not host property access;
- a function visible to guest code must use `tenant.invoke`, which supplies the
  registered Jade ABI and the exact `new.target` value;
- a tenant may be a `MergedTenant`, where values crossing provider boundaries
  are bridge exotics rather than raw provider objects;
- a buffer may be local native memory in one embedding and an asynchronous
  shared-store facade in another.

Primordials therefore cannot be module-global singleton host objects.  Their
function shells, prototypes, instances, properties, and internal state must all
be allocated through the tenant supplied to their factory.

## Goals

1. Make the initial standard values tenant-owned and cache their identity per
   tenant.
2. Preserve the existing generator-driver contract: any primordial operation
   which touches tenant state or a remote/shared buffer is a tenant generator
   and is driven only by `tenant.driveTenant` at a guest boundary.
3. Make `Proxy` compose with the existing `TenantExoticHandler` /
   `TenantCallableExoticHandler` API.  Do **not** implement guest `Proxy` by
   exposing host `Proxy` objects.
4. Let each environment choose binary-buffer storage independently.  In
   particular, an environment can provide a native `SharedArrayBuffer` adapter
   or an asynchronous adapter backed by a shared store.
5. Keep `new Function`, `Function(...)`, `eval`-like compilation, and any JIT
   capability out of this milestone.  Compilation policy is use-case-specific:
   it depends on the guest's tenant, global realm, available host powers, and
   desired backend.
6. Retain a clear package boundary: tenant implementations live under
   `packages/jade-js/tenants/`; primordial implementations live under
   `packages/jade-js/primordials/`.
7. Update `AGENTS.md` when implementation starts so future changes preserve
   primordial cache, ABI, exotic, and buffer-hook invariants.

## Non-goals

- A complete SES realm, a full browser/Node global, or an exhaustive ES
  standard library.
- Full ECMAScript `Proxy` internal methods and invariants.  Jade currently
  exposes a smaller tenant operation surface; the first `Proxy` maps precisely
  to that surface.
- Making raw host reflection (`obj[key]`, `Object.keys(obj)`, host
  `Reflect.apply`) work on tenant-managed values.  Host callers use the tenant
  API or explicit adapters.
- Dynamic source evaluation: no `Function` constructor, `new Function`, direct
  `eval`, or JIT/compiler capability is exposed by the `Function` primordial.
- `DataView`, `Atomics`, resizable/growable buffers, transfer/detach, primitive
  boxing, `BigInt` typed arrays, and every typed-array method in the first patch
  unless the selected buffer hook can state their semantics.  The hook boundary
  is designed so these can be added without changing the representation.
- Replacing tenant merging or cross-provider marshalling.  Primordials are
  created for the execution-facing tenant (which may itself be a
  `MergedTenant`) and use that tenant for every operation.

## Existing constraints to preserve

The following are load-bearing existing rules, not optional implementation
preferences:

- Every `Tenant` operation is a generator.  Nested tenant work inside a
  primordial or exotic handler uses
  `yield this.yieldTenant(...)`/`yield tenant.yieldTenant(...)`; no code calls
  `.next()` on a tenant operation.
- VM/JIT/WASM guest calls route through
  `tenant.invoke(callee, invocation)`.  An apply is
  `{ kind: "apply", thisArg, args }`; a construction is
  `{ kind: "construct", args, newTarget }`.
- `nt` means exactly `new.target`: apply supplies `undefined`, and construction
  supplies the invoked construction's actual new target.  A surrounding
  function's `nt` must never leak into an ordinary apply.
- `makeFunction` owns an adopted native function but preserves whether that
  native implementation is constructible.  `makeCallableExotic` creates a
  constructible shell and routes its apply/construct behavior through tenant
  generators.
- Missing **native exotic** traps fail closed with `TypeError`.  The `Proxy`
  primordial will install every native exotic trap itself; it can then implement
  JavaScript-style target fallback when a *guest Proxy handler property* is
  absent.  These are distinct levels and must not be conflated.
- Cross-provider values must be marshalled by `MergedTenant`, not stored as raw
  foreign objects by primordial closures or buffer hooks.
- **No host fallback:** a guest-visible primordial operation either uses its
  specified tenant/hook operation or rejects as unsupported.  It must not
  quietly read a guest value with raw host property access, call a guest value
  through host `Reflect.apply`, or substitute a host intrinsic because the
  tenant API lacks an operation.  This is both a present isolation rule and a
  future recompilation rule: primordials must remain lightweight consumers of
  their explicit API surface, so their behavior can be recompiled for another
  backend without inheriting ambient host powers.
- ABI/driver helpers remain `this`-derived tenant methods and must not be added
  to `TENANT_METHOD_NAMES` for JIT tenant-method inlining.

## Package reorganization

### Target layout

```text
packages/jade-js/
  index.ts                         # stable public barrel only
  tenants/
    index.ts                       # Tenant types, public tenant exports
    types.ts                       # Tenant/TenantInvocation/exotic interfaces
    multi.ts                       # current multi_tenant.ts implementation
    single.ts                      # current single_tenant.ts implementation
    merged.ts                      # current merged_tenant.ts implementation
    narrow.ts                      # validation + guest ABI mixin
    driver.ts                      # tenant generator driver
    shims.ts                       # guest generator shims
    rewrite.ts                     # host/guest conversion types and helpers
    wsdom-runtime.ts               # WSDOM provider runtime
  primordials/
    types.ts                       # shared primordial-only types and helpers
    realm.ts                       # explicit global/installation assembler
    object.ts
    function.ts
    reflect.ts
    proxy.ts
    array-buffer.ts             # ArrayBuffer + optional SharedArrayBuffer
    typed-arrays.ts
  gc.ts
  ses_compat.ts
  vm.ts
```

`array-buffer.ts` names the ArrayBuffer/SharedArrayBuffer *primordial family*;
`typed-arrays.ts` names the typed-array family.  The named initial primordials
remain individually isolated from tenant code and one another.  If the project
requires literal one-constructor-per-file later, `typed-arrays.ts` can be split
without changing the hook or factory interfaces.

### Compatibility imports and package exports

The move should not require every consumer to change in one atomic patch:

1. Move the implementation files first.
2. Leave thin deprecated forwarding modules at the current paths for one
   release (`multi_tenant.ts`, `single_tenant.ts`, `merged_tenant.ts`,
   `narrow.ts`, `driver.ts`, `shims.ts`, `rewrite.ts`, and
   `wsdom-tenant-runtime.ts`).  Each only re-exports its new canonical module;
   it must not duplicate state or create another mixin/registry.
3. Make `packages/jade-js/index.ts` a stable barrel which re-exports the tenant
   API and primordial realm/factories.
4. Update internal imports to use canonical `./tenants/...` or
   `./primordials/...` paths, then migrate downstream import sites gradually.
5. Update `packages/jade-js/package.json` exports with explicit subpaths for
   `./primordials`, `./tenants`, and `./wsdom-tenant-runtime`; retain the
   current root and WSDOM paths.

The `GUEST_FN_REGISTRY`, exotic metadata, and tenant-operation symbol must
remain singleton module state.  Compatibility modules must re-export—not copy
or reinitialize—those definitions.

## Common primordial model

### Factory contract and caching

Every primordial file owns a private `WeakMap` cache:

```ts
export type ObjectPrimordial = {
  Object: Function;
  ObjectPrototype: object;
};

const cache = new WeakMap<Tenant, ObjectPrimordial>();

export function objectPrimordial(tenant: Tenant): ObjectPrimordial {
  const existing = cache.get(tenant);
  if (existing) return existing;
  const created = createObjectPrimordial(tenant);
  cache.set(tenant, created);
  return created;
}
```

Creation may need tenant generator work, so the actual public spelling should
be generator-safe, for example:

```ts
export function objectPrimordial(tenant: Tenant): TenantGenerator<ObjectPrimordial>;
```

The cache stores a completed primordial only.  It must not cache a partially
initialised object, a raw generator, or a rejected promise.  Because initial
construction uses synchronous tenant allocations in the first implementations,
a per-tenant re-entrancy guard is sufficient; once construction may await a
remote provider, use a per-tenant state record (`initializing`/`ready`) so a
recursive lookup is deterministic rather than publishing an incomplete realm.

The cache is intentionally keyed by the exact tenant object, not by a provider
inside a `MergedTenant`: a primordial closing over router `R` must continue to
operate through `R`, even if R's primary provider is the same as another
router's.  A `WeakMap` lets both the tenant and the primordial collection be
garbage-collected together.

### Function shells and intrinsic properties

All guest-callable builtins are created with `tenant.makeCallableExotic` unless
their implementation is provably synchronous and never performs tenant work.
This gives their implementation a generator-capable apply/construct handler and
makes their `length`, `name`, `prototype`, and static methods tenant-owned
properties.  Do not attach guest-visible properties directly to a native shell.

The shared primordial helper module will provide generator helpers to:

- allocate a tenant-owned ordinary object/function shell;
- install enumerable/non-enumerable data properties through tenant descriptors;
- read a guest argument/receiver through tenant APIs;
- perform brand checks using private `WeakMap`s rather than raw host properties;
- invoke a guest-supplied callback through `tenant.invoke`;
- report standard `TypeError`/`RangeError` errors consistently.

The helpers must not provide a global mutable “current tenant”.  Closures made
by a factory capture their exact tenant.

### Realm assembly and global injection

`primordials/realm.ts` exposes a deliberately explicit bootstrap API:

```ts
interface PrimordialRealmOptions {
  buffers?: BufferHooks;
  includeSharedArrayBuffer?: boolean;
  typedArrays?: readonly TypedArrayKind[];
}

interface PrimordialRealm {
  globalThis: object;                  // tenant-owned guest global
  Object: Function;
  Function: Function;
  Reflect: object;
  Proxy: Function;
  ArrayBuffer: Function;
  SharedArrayBuffer?: Function;
  typedArrays: Readonly<Record<TypedArrayKind, Function>>;
}

function* createPrimordialRealm(
  tenant: Tenant,
  options?: PrimordialRealmOptions,
): TenantGenerator<PrimordialRealm>;
```

The assembler calls each cached factory, creates a tenant-owned global object,
and defines the selected names.  It is the one place that determines which
primordials a given guest receives.  VM embedder APIs will accept either an
explicit guest global object or a `PrimordialRealm`; they must never silently
replace a host `globalThis` with a partially configured realm.

Initial property graphs are created in dependency order:

1. `Object.prototype` and `Object`;
2. `Function.prototype` and `Function` (using the Object values);
3. `Reflect`;
4. buffer constructors/prototypes and typed arrays;
5. `Proxy`, which depends on the tenant exotic machinery and function helpers;
6. the tenant-owned global object.

Factories may depend on earlier factories explicitly.  They must not import the
realm assembler, which prevents cyclic module initialization.

## Tenant descriptor and prototype surface

Descriptors and prototypes are in scope for this milestone.  Before the
Object/Reflect/Proxy factories are implemented, extend the `Tenant` and exotic
interfaces with explicit generator operations; do not emulate them with host
reflection or a magic tenant property:

```ts
export interface TenantPropertyDescriptor {
  value?: unknown;
  writable?: boolean;
  get?: Function;
  set?: Function;
  enumerable?: boolean;
  configurable?: boolean;
}

interface Tenant {
  getPrototypeOf(target: object): TenantGenerator<object | null>;
  setPrototypeOf(target: object, prototype: object | null): TenantGenerator<boolean>;
  getOwnPropertyDescriptor(
    target: object,
    key: PropertyKey,
  ): TenantGenerator<TenantPropertyDescriptor | undefined>;
  defineProperty(
    target: object,
    key: PropertyKey,
    descriptor: TenantPropertyDescriptor,
  ): TenantGenerator<boolean>;
  isExtensible(target: object): TenantGenerator<boolean>;
  preventExtensions(target: object): TenantGenerator<boolean>;
  /** All own keys, including non-enumerable keys; unlike current ownKeys. */
  ownPropertyKeys(target: object): TenantGenerator<PropertyKey[]>;
}
```

`TenantExoticHandler` grows the matching optional operations.  Native exotic
missing traps remain fail-closed.  `define` and `ownKeys` retain their existing
Jade meanings (bulk definition and own **enumerable** keys respectively), so
this is additive rather than a silent change in bytecode behavior.

`TenantPropertyDescriptor` is a host-side control record returned across the
Tenant API, not a guest-visible descriptor object.  Implementations must keep
its values/getters/setters tenant-correct and must never inspect guest-owned
source descriptor objects with raw property access.  The existing bulk
`define(target, descriptors)` remains useful to the bytecode object-literal
path; it should be implemented in terms of the same descriptor machinery where
possible.

`MultiTenant` stores prototype, descriptor, extensibility, and complete-key
metadata in its shadow representation.  `single_tenant` delegates the same
operations to native storage after applying its key-cleaning policy.
`MergedTenant` routes them exclusively to the owner and marshals descriptor
values, prototype values, and descriptor accessors through its canonical bridge
path.  A WSDOM provider adds these names to its operation protocol.  Each
backend must use the new operations at the one corresponding emission site once
the bytecode surface gains the relevant opcodes.

This extension makes the following standard-facing operations real rather than
approximated: `Object.getPrototypeOf`, `Object.setPrototypeOf`,
`Object.getOwnPropertyDescriptor`, `Object.defineProperty`,
`Reflect.getPrototypeOf`, `Reflect.setPrototypeOf`,
`Reflect.getOwnPropertyDescriptor`, `Reflect.defineProperty`,
`Reflect.isExtensible`, and `Reflect.preventExtensions`.

## `Object` primordial (`primordials/object.ts`)

The factory creates and caches:

- `Object.prototype` as a tenant-owned object;
- a constructible/callable `Object` function shell;
- the initial supported static operations; and
- prototype operations that can be represented correctly through the current
  tenant surface.

### First supported behavior

- `Object(value)` / `new Object(value)`:
  - `null`/`undefined` allocate a tenant-owned object with `Object.prototype`;
  - an object/function returns the value unchanged;
  - primitive boxing is deferred until primitive wrapper primordials exist.
- `Object.create(proto)` allocates through `tenant.make(proto)` after validating
  `proto` is an object or null.
- `Object.keys(obj)` delegates to `tenant.ownKeys(obj)`.
- `Object.assign(target, ...sources)` delegates to `tenant.assign`; it skips
  `null`/`undefined` sources and uses the guest-visible enumerable-key rules.
- `Object.defineProperty(target, key, descriptor)` converts the guest descriptor
  through the descriptor helper and delegates to `tenant.defineProperty`.
- `Object.defineProperties(target, descriptors)` delegates to `tenant.define`.
- `Object.getOwnPropertyDescriptor(target, key)` materializes a tenant-owned
  guest descriptor object from `tenant.getOwnPropertyDescriptor`'s control
  record, or returns `undefined`.
- `Object.getPrototypeOf(target)` and `Object.setPrototypeOf(target, proto)`
  delegate to their explicit tenant operations.
- `Object.isExtensible`, `Object.preventExtensions`, `Object.seal`, and
  `Object.freeze` use the explicit extensibility/descriptor operations.  The
  first implementation may defer `seal`/`freeze` only if the descriptor-update
  semantics are not yet fully specified; it must not substitute host behavior.
- `Object.hasOwn(obj, key)` delegates to `tenant.has`.
- `Object.prototype.hasOwnProperty` delegates to `tenant.has` for its receiver.

Primitive boxing and inherited lookup remain deferred until primitive wrapper
primordials and the corresponding full property-lookup semantics are designed.
In particular, `tenant.has` currently means own-property presence, so it must
not be presented as general JavaScript `in` semantics.

## `Function` primordial (`primordials/function.ts`)

The Function factory creates a tenant-owned `Function.prototype` and a
`Function` constructor-shaped value.  It provides operations that use the
canonical call boundary:

- `Function.prototype.call(thisArg, ...args)` invokes the receiver through
  `tenant.invoke(receiver, { kind: "apply", thisArg, args })`.
- `Function.prototype.apply(thisArg, argsArray)` first reads the array-like
  values through tenant operations, then invokes through `tenant.invoke`.
  Initial support may require a real guest Array/typed array; arbitrary host
  iterables are not inspected directly.
- `Function.prototype.bind(thisArg, ...prefix)` returns a
  `makeCallableExotic` bound-function shell.  Its apply combines prefix and
  invocation arguments; its construct forwards construction with the caller's
  `newTarget` according to the chosen bound-constructor policy.  Its own
  guest-visible properties remain tenant-managed.

The `Function` value itself deliberately has no compilation path.  Both apply
and construct throw a deterministic `TypeError` such as
`"dynamic Function construction is not available in this Jade realm"`.
`Function.prototype` is still useful for function identity and the safe
`call`/`apply`/`bind` methods, but neither it nor `Function` may expose a way to
turn strings into executable guest source.

A future compilation capability is a separately reviewed API, not a flag on
this constructor.  It must specify source ownership, parser/compiler backend,
bytecode/JIT selection, global/tenant bindings, source maps, auditing, and
whether compilation is allowed in the embedding.

## `Reflect` primordial (`primordials/reflect.ts`)

`Reflect` is a tenant-owned ordinary object whose methods are tenant-owned
callable shells.  Initial methods map one-to-one to current semantics:

| Method | Tenant operation |
| --- | --- |
| `Reflect.get(target, key)` | `tenant.get(target, key)` |
| `Reflect.set(target, key, value)` | `tenant.set(target, key, value)` then returns `true` |
| `Reflect.has(target, key)` | `tenant.has(target, key)` |
| `Reflect.deleteProperty(target, key)` | `tenant.delete(target, key)` then returns `true` |
| `Reflect.ownKeys(target)` | `tenant.ownPropertyKeys(target)` |
| `Reflect.getOwnPropertyDescriptor(target, key)` | `tenant.getOwnPropertyDescriptor(target, key)` then materialize a guest descriptor object |
| `Reflect.defineProperty(target, key, descriptor)` | convert descriptor and call `tenant.defineProperty`; returns its boolean |
| `Reflect.getPrototypeOf(target)` | `tenant.getPrototypeOf(target)` |
| `Reflect.setPrototypeOf(target, proto)` | `tenant.setPrototypeOf(target, proto)` |
| `Reflect.isExtensible(target)` | `tenant.isExtensible(target)` |
| `Reflect.preventExtensions(target)` | `tenant.preventExtensions(target)` |
| `Reflect.apply(target, thisArg, argsList)` | `tenant.invoke(target, apply invocation)` |
| `Reflect.construct(target, argsList, newTarget?)` | `tenant.invoke(target, construct invocation)` |

Argument validation and argument-list extraction occur through the tenant.  A
method must never use bare `Reflect.apply` to call a guest function or raw
property indexing to inspect a guest argument array.

The descriptor/prototype and extensibility methods are in scope because they
now have exact Tenant operations.  Methods outside that surface remain withheld
rather than approximated, and may only be added with defined behavior for
exotics and merged values.

## `Proxy` primordial (`primordials/proxy.ts`)

### Representation

`Proxy` itself is a tenant-owned constructible callable exotic.  Its construct
handler accepts `(target, handler)`, validates that both are guest objects, and
returns `tenant.makeExotic(null, proxyHandler)`.  Calling `Proxy(...)` without
`new` throws, matching the standard constructor form.  No host `Proxy` is
created.

`proxyHandler` implements **all** native `TenantExoticHandler` operations,
including the descriptor/prototype/extensibility surface introduced above.  This
satisfies the fail-closed native exotic rule.  For each operation it:

1. reads the corresponding trap property from the guest handler through
   `tenant.get(handler, trapName)`;
2. if the property is `undefined`, falls back to the same operation on target;
3. otherwise validates it is callable and invokes it through `tenant.invoke`
   with `handler` as `thisArg` and the appropriate target/operation arguments;
4. validates/coerces the result where Jade's operation requires it (notably
   boolean `has`); and
5. composes the work with `yield tenant.yieldTenant(...)`, allowing guest traps
   to be async or generator-shaped under the normal driver protocol.

The proxy map is intentionally Jade-specific at first:

| Tenant operation | Guest handler name | Guest trap arguments |
| --- | --- | --- |
| `get(proxy, key)` | `get` | `(target, key, receiver)` |
| `set(proxy, key, value)` | `set` | `(target, key, value, receiver)` |
| `has(proxy, key)` | `has` | `(target, key)` |
| `delete(proxy, key)` | `deleteProperty` | `(target, key)` |
| `ownKeys(proxy)` | `ownKeys` | `(target)` |
| `ownPropertyKeys(proxy)` | `ownKeys` | `(target)` |
| `getOwnPropertyDescriptor(proxy, key)` | `getOwnPropertyDescriptor` | `(target, key)` |
| `defineProperty(proxy, key, descriptor)` | `defineProperty` | `(target, key, descriptor)` |
| `getPrototypeOf(proxy)` | `getPrototypeOf` | `(target)` |
| `setPrototypeOf(proxy, proto)` | `setPrototypeOf` | `(target, proto)` |
| `isExtensible(proxy)` | `isExtensible` | `(target)` |
| `preventExtensions(proxy)` | `preventExtensions` | `(target)` |
| `define(proxy, descriptors)` | `defineProperties` | `(target, descriptors)` |
| `assign(proxy, source)` | `assign` | `(target, source)` |

The existing `define` and `assign` operations are bulk Jade operations, while
`defineProperty` is the explicit descriptor operation.  The guest trap names
above make that distinction visible.  `ownKeys` maps to
`ownPropertyKeys` because standard Proxy's `ownKeys` includes non-enumerable
keys; the existing Jade `ownKeys` operation remains the filtered enumerable
view.  The initial implementation enforces the tenant-defined proxy invariants
for descriptors, prototypes, and extensibility; it makes no claim to expose
host-proxy behavior or direct host access.

### Revocation and identity

`Proxy.revocable(target, handler)` is in scope.  It constructs the same proxy
record plus a tenant-owned revoker function.  The record contains private
`revoked` state; every exotic trap, including descriptor/prototype/extensibility
traps, checks it first and throws `TypeError` after revocation.  The revoker is
idempotent and drops the target/handler references when it revokes so ordinary
native garbage collection can reclaim them.  The result is a tenant-owned
object `{ proxy, revoke }` with its two entries defined through the tenant.

Tenant identity is preserved: the proxy is one owned exotic shell, and any
`MergedTenant` boundary will bridge it like other tenant-owned values.  No host
`Proxy`, host revocable record, or host fallback is involved.

## Generic buffer hook system (`primordials/array-buffer.ts`)

### Motivation

`ArrayBuffer` cannot always be a native `ArrayBuffer`.  A local embedding can
use native memory; an embedding with shared storage may need a wrapper whose
operations await remote/shared-store work.  Typed arrays must work over either
without knowing which representation was selected.

### Hook contract

The primordial layer defines representation-neutral opaque buffer handles and
uses tenant generators for every potentially asynchronous operation:

```ts
export type BufferKind = "array-buffer" | "shared-array-buffer";
export type BufferHandle = object;

export interface BufferHooks {
  /** Identity for cache validation; one immutable hook set per tenant realm. */
  readonly identity: object;
  readonly supportsSharedArrayBuffer: boolean;

  allocate(kind: BufferKind, byteLength: number): TenantGenerator<BufferHandle>;
  isHandle(value: unknown, kind?: BufferKind): boolean;
  byteLength(handle: BufferHandle): TenantGenerator<number>;
  slice(handle: BufferHandle, begin: number, end: number): TenantGenerator<BufferHandle>;

  /** Exact bytes; implementations may yield asynchronous storage work. */
  read(handle: BufferHandle, byteOffset: number, byteLength: number): TenantGenerator<Uint8Array>;
  write(handle: BufferHandle, byteOffset: number, bytes: Uint8Array): TenantGenerator<void>;
}
```

A hook implementation owns storage and is responsible for validating bounds,
sharing semantics, and stable handle identity.  The primordial code owns guest
buffer shells and maps each shell to `{ kind, handle }` in a private `WeakMap`.
It does not expose the raw `BufferHandle` as a guest property.

`BufferHooks` is intentionally object-shaped and has an `identity`, so buffer
factory caching is unambiguous.  `arrayBufferPrimordial(tenant, hooks)` keeps a
private `WeakMap<Tenant, WeakMap<object, BufferPrimordial>>` (or a per-tenant
record that rejects a different `identity`).  It must fail loudly if callers
attempt to install two incompatible binary representations in one tenant realm.

### Built-in adapters

Ship a synchronous `nativeBufferHooks` adapter which uses native `ArrayBuffer`
and, when available and requested, native `SharedArrayBuffer`.  It is one
implementation of the interface, not a special path in typed-array logic.

An embedding may provide `sharedStoreBufferHooks` (or its own implementation)
where a shared-buffer handle names a shared store.  `read`, `write`, `slice`,
and `byteLength` can yield promises through the normal tenant driver.  This is
how an async shared-store wrapper is supported without claiming that a remote
buffer is a real host `SharedArrayBuffer`.

### ArrayBuffer/SharedArrayBuffer shells

The factory creates tenant-owned constructor shells, prototype objects, and
buffer instance exotics.  Their handlers expose the initial required state:

- getter-like `byteLength`;
- `slice(begin, end)`;
- constructor allocation with checked non-negative integral byte lengths;
- an optional `SharedArrayBuffer` constructor only when
  `hooks.supportsSharedArrayBuffer` is true.

The host-facing native buffer adapter may retain native buffers internally, but
guest code observes a tenant-owned buffer shell.  This keeps a shared-store
adapter and a native adapter behaviorally interchangeable at the tenant
boundary.  Direct host APIs such as `new Uint8Array(guestBuffer)` are not a
supported way to inspect a guest buffer.

## Typed arrays (`primordials/typed-arrays.ts`)

The typed-array factory is parameterized by the same `BufferHooks` instance and
creates tenant-owned constructors/prototypes for the chosen initial kinds:

```ts
type TypedArrayKind =
  | "Int8Array" | "Uint8Array" | "Uint8ClampedArray"
  | "Int16Array" | "Uint16Array" | "Int32Array" | "Uint32Array"
  | "Float32Array" | "Float64Array";
```

Each kind records its element byte width and codec.  A typed-array instance is a
tenant exotic with private metadata:

```ts
type TypedArrayRecord = {
  kind: TypedArrayKind;
  buffer: object;             // the guest buffer shell, never raw handle
  byteOffset: number;
  length: number;
};
```

The implementation obtains the underlying handle only through the buffer
factory's private brand/accessor.  It validates alignment, range, and kind
compatibility at construction.  This prevents a typed array from accidentally
accepting a native host buffer outside the selected hook system.

Initial exotic behavior supports:

- numeric-index reads/writes through `hooks.read`/`hooks.write` and the kind
  codec;
- `length`, `byteLength`, `byteOffset`, and `buffer` reads;
- `set(source, offset?)` and `subarray(begin, end?)` methods;
- constructors from a guest buffer plus byte offset/length, and from a numeric
  element count;
- own enumerable numeric keys through the existing `ownKeys` contract.

The first implementation must define an explicit choice for construction from
an array-like guest object.  Recommended first scope: support typed-array and
buffer inputs plus numeric lengths; defer arbitrary array-like/iterable inputs
until a tenant-aware Array/iterator primordial exists.  Do not iterate a guest
object natively.

All numeric decoding/encoding lives in this module and uses temporary native
`DataView`/`Uint8Array` values only on bytes returned from the hook.  `BigInt`
typed arrays are deliberately deferred until BigInt semantics are supported by
every intended guest backend and transport.  The codec table is shared by
native and shared-store backends.

Async hooks mean an indexed typed-array read can be asynchronous under an
async-capable guest boundary.  The same operation correctly throws under a
plain synchronous boundary, exactly as any tenant operation yielding a promise
does today.  This is intentional: an embedding must select `addAsync` / an
async-capable execution path before exposing a shared-store buffer to code that
performs reads or writes.

## Ownership, merging, and lifecycle

- A primordial cache is owned by a `WeakMap<Tenant, ...>`; it must not hold a
  tenant strongly from a global array or registry.
- A factory created for a `MergedTenant` captures that router.  Its calls,
  proxy target accesses, and buffer values therefore take the canonical router
  marshalling path.
- JavaScript resource lifecycle is ordinary native garbage collection: private
  `WeakMap`/`WeakSet` records disappear with their tenant-owned shells and a
  revocable Proxy clears its own references when revoked.  Primordials do not
  introduce guest-visible `close`/manual-release APIs or rely on
  `FinalizationRegistry` for normal JavaScript object management.
- A storage embedding with resources outside the JS heap must manage those
  resources at its own explicit host boundary.  Such bookkeeping is not a
  primordial API and must not cause a guest operation to fall back to raw host
  object behavior.
- No primordial closure may capture a raw value from a foreign tenant provider.
  Receive and retain only values returned through the execution-facing tenant.

## Error and security behavior

- Validate target, handler, constructor argument, buffer length, byte offset,
  and typed-array range before creating observable partial state.
- Throw `TypeError` for wrong brands/callability and `RangeError` for invalid
  lengths, offsets, and out-of-bounds index/range requests.
- Do not expose host `Function`, `eval`, native `Proxy`, raw buffer handles, or
  hooks as tenant-visible properties.
- Do not derive a capability from the existence of a host global.  In
  particular, native `SharedArrayBuffer` availability does not grant it to a
  guest unless the selected `BufferHooks` and realm options both enable it.
- Guest Proxy traps are invoked through `tenant.invoke`, not via `Reflect.apply`.
  Guest handler properties and descriptor maps are read only through the
  tenant.
- Treat symbols deliberately.  Existing WSDOM transport does not support
  symbols except as opaque objects, so the first cross-provider primordial
  behavior either rejects unsupported symbol keys at the transport boundary or
  preserves them as opaque handles; it must never stringify them.

## Tests

### Reorganization/regression tests

- Run `npx tsc --noEmit` after the move.
- Run the existing `tenant.e2e.ts`, `trap.e2e.ts`, `tenant-compose.e2e.ts`,
  `exotic-tenant.e2e.ts`, and `rewrite.e2e.ts` through Node's TypeScript
  stripping mode.
- Verify both current import paths and the canonical subfolder exports resolve
  during the compatibility period, and prove the ABI registry is shared across
  them.

### Cache/realm tests

- Each individual factory returns identical identities for repeated calls with
  the same tenant and distinct identities for two tenants.
- A realm installs tenant-owned `Object`, `Function`, `Reflect`, `Proxy`,
  buffers, and selected typed arrays on a tenant-owned global object.
- A primordial created for a `MergedTenant` routes object operations through
  the router; it does not leak a secondary provider object.
- A failed/reentrant initialization neither publishes a partial primordial nor
  poisons future successful construction.

### Object/Function/Reflect tests

- The descriptor/prototype Tenant operations work for `MultiTenant`,
  `single_tenant`, an exotic, and a `MergedTenant` bridge; descriptors preserve
  accessor ABI and cross-provider descriptor values are marshalled.
- `Object.create`, `Object.keys`, `Object.assign`, `Object.defineProperties`,
  `Object.defineProperty`, `Object.getOwnPropertyDescriptor`,
  `Object.getPrototypeOf`, `Object.setPrototypeOf`, and `Object.hasOwn` operate
  solely through tenant methods, including a remote asynchronous exotic where
  appropriate.
- `Reflect.ownKeys` returns non-enumerable keys through `ownPropertyKeys`, and
  descriptor/prototype/extensibility operations use their exact tenant methods.
- `Function.prototype.call`, `apply`, and `bind` call a registered
  leading-tenant-nt guest function with tenant correctly injected; ordinary
  apply observes `nt === undefined`.
- Bound construction observes its actual `newTarget`.
- `Function(...)` and `new Function(...)` fail without compiling or invoking a
  backend.
- `Reflect.apply` and `Reflect.construct` route through `tenant.invoke`, and
  the construct test verifies actual `newTarget` forwarding.

### Proxy tests

- Proxy creation uses `makeExotic`, not host `Proxy`; raw host inspection sees
  no proxy trap properties.
- Each supported operation calls a supplied guest trap with `handler` as its
  receiver and expected arguments.
- An absent guest trap falls back to target behavior, while an absent native
  exotic trap still fails closed in a direct exotic test.
- Async guest traps compose under `addAsync`; generator-shaped traps compose
  under `addGen`.
- `Proxy.revocable` returns a tenant-owned `{ proxy, revoke }` record; revoke
  is idempotent, clears retained target/handler references, and every operation
  after revocation throws `TypeError`.
- A Proxy target/handler crossing a `MergedTenant` boundary remains a bridge
  exotic and preserves source identity/call ABI.

### Buffers and typed arrays tests

- Native hooks: allocate, `byteLength`, `slice`, typed indexed reads/writes,
  `set`, `subarray`, and range errors for signed/unsigned/float kinds.
  `BigInt64Array`/`BigUint64Array` are absent and attempts to obtain them do not
  expose a host fallback.
- Shared native adapter: `SharedArrayBuffer` is exposed only when explicitly
  enabled, and views share writes.
- Async shared-store hook: each `byteLength`/read/write path composes through
  the async tenant driver; the same operation rejects under a sync-only
  boundary.
- A typed array created from a buffer uses the factory's exact hook instance;
  incompatible/wrong-brand buffers are rejected.
- Merged-tenant tests ensure buffer shells/view values are bridged normally and
  no raw native handle crosses a provider boundary.

## Implementation phases

1. **Document invariants.** Update `AGENTS.md` with the tenant layout, per-file
   WeakMap caching requirement, no-dynamic-Function and no-host-fallback rules,
   canonical invocation rule, Proxy's two trap layers and revocation behavior,
   descriptor/prototype routing, and shared `BufferHooks` ownership/async
   rules.  Add this plan to the linked docs.
2. **Move tenant modules without behavior changes.** Create `tenants/`, move
   canonical implementations, add forwarding modules and barrel exports, and
   run the existing test suite.
3. **Extend the Tenant surface.** Add descriptor, prototype, full-own-key, and
   extensibility operations to every provider, exotic handler, merged router,
   WSDOM protocol, and relevant backend emission path.  Land exhaustive routing
   and bridge tests before a primordial relies on them.
4. **Add primordial infrastructure.** Add primordial shared types/helpers and
   `createPrimordialRealm`; establish generator-safe cache initialization and
   tests before adding standard behavior.
5. **Implement Object, Function, and Reflect.** Use exact descriptor/prototype
   tenant operations and test ABI/driver behavior; leave primitive boxing out.
6. **Implement Proxy and `Proxy.revocable`.** Build them exclusively on
   `makeExotic`/`makeCallableExotic`; test fallback, trap invocation,
   revocation, and merged routing.
7. **Add buffer hooks and ArrayBuffer.** Implement native hooks first, then
   test a deliberately asynchronous fake shared-store hook before choosing a
   production shared-store backend.
8. **Implement non-BigInt typed arrays.** Start with constructors from
   length/buffer, indexed access, metadata, `set`, and `subarray`; add remaining
   constructors and methods only after tenant-aware Array/iterator support is
   available.
9. **Integrate realm bootstrap.** Offer explicit VM/embedder wiring and retain
   the current host-global injection path for compatibility until callers opt
   into a primordial realm.

Each phase should land with its own tests and no change should silently widen
host powers available to a guest.

## Settled design decisions

- `BufferHooks` is an **explicit embedder capability**.  A primordial realm does
  not install `ArrayBuffer`, `SharedArrayBuffer`, or typed arrays unless the
  caller supplies a hook set; `nativeBufferHooks` is available as an explicit
  opt-in, never an implicit host-global fallback.  Until Number/String/Boolean wrapper
  primordials are designed, `Object(value)`/`new Object(value)` only supports
  `null`/`undefined` allocation and object/function identity; unsupported
  primitive inputs reject rather than boxing through the host.
- `BigInt` and `BigInt64Array`/`BigUint64Array` are deferred.  The first typed
  array set is the non-BigInt numeric constructors listed in this plan.
- `Proxy.revocable` is in scope and uses private revocation state plus native
  garbage collection; it is not implemented with host `Proxy.revocable`.
- Tenant descriptors, prototype access, complete own-key enumeration, and
  extensibility operations are in scope and precede the primordials that use
  them.
- JavaScript object lifetime is native GC.  Primordials use weak private state
  and do not expose manual resource management; external stores remain an
  embedder concern.
- The no-host-fallback rule is mandatory now and for future primordial
  recompilation.  Primordial code remains a lightweight client of explicit
  Tenant/BufferHooks APIs; unsupported behavior rejects rather than importing
  ambient host semantics.

## Remaining open decisions

`BufferHooks` capability exposure and `Object.seal`/`Object.freeze` are settled;
there are no remaining design decisions blocking the implementation.  The
seal/freeze phase includes exact descriptor updates and extensibility checks,
with dedicated exotic and merged-provider tests.
# Plan: guest `Promise` primordials and explicit host/guest async separation

## Summary

Jade currently has one accidental, unsafe notion of a promise.  Native host
`Promise`s are used both as:

1. the trusted suspension mechanism for tenant operations, VM runners, WSDOM
   requests, and WASM futures; and
2. the value a guest is meant to receive from an `async` function or eventually
   from a `Promise` global.

Those are not interchangeable.  A guest promise must be a tenant-owned,
realm-specific guest value.  A host promise is a host scheduling/resource
capability.  Treating either as the other leaks ambient host behavior, lets the
host driver inspect guest `then` properties directly, breaks multi-tenant
ownership, and makes an eventual non-JS recompilation of primordials
impossible.

This plan has two coupled deliverables:

1. add `packages/jade-js/primordials/promise.ts`, a cached per-tenant guest
   `Promise` constructor/prototype/runtime and install it from
   `primordials/realm.ts`; and
2. replace every implicit `Promise`/thenable assumption at a guest/host
   boundary with explicit, nominally distinct host-task and guest-promise
   protocols.  This includes TypeScript rewrite types and adapters, the tenant
   driver and ABI helpers, interpreter/JIT/WASM `AWAIT` behavior, and WSDOM
   transport/composability.

The implementation must not merely wrap host promises at the global name
`Promise`.  It must make async execution and conversion use the exact guest
realm's promise runtime, so values have correct ownership, `then` is invoked
through `tenant`, and the host scheduler is only reached through an explicit
embedder capability.

## Goals

1. A guest realm created by `createPrimordialRealm` receives a tenant-owned
   `Promise`, `Promise.prototype`, and guest promise instances.  Each has one
   identity per exact execution-facing `Tenant`, cached with a private
   `WeakMap` like all current primordials.
2. A guest promise is **not** a native host `Promise`, a native `PromiseLike`,
   or an unowned thenable.  Its visible properties and methods are accessed via
   the tenant interface and its state/reactions stay private to the primordial
   factory.
3. Host asynchronous work is represented by a distinct, explicit host-only
   capability.  Tenant drivers may suspend for that capability, but may never
   inspect an arbitrary yielded value's `.then` property.
4. `HostToGuest`, `GuestToHost`, `FnResult`, and the runtime adapters express
   the direction of promise conversion rather than preserving `Promise<T>`
   unchanged on both sides.
5. `AWAIT` performs guest Promise Resolution using the active realm runtime;
   it must not use JavaScript's bare `await value` or `Promise.resolve(value)`
   against a guest value.
6. TS interpreter, JIT tiers 0/1/2, and WASM follow the same promise boundary
   and propagate it through nested functions without silently using a host
   promise path.
7. WSDOM distinguishes a transport request task from a browser guest promise
   value.  A promise returned by browser guest code remains an opaque remote
   guest value; it is not reclassified as the host request promise merely
   because both are thenable in browser JavaScript.
8. Preserve the existing no-host-fallback rule: guest semantics either use the
   selected tenant/promise-runtime operation or reject.  They never obtain
   semantics by reading/calling a guest value with host reflection.

## Non-goals

- Replacing the host JavaScript event loop or implementing a custom scheduler.
  The embedder still supplies the scheduling capability; Jade only makes that
  authority explicit.
- Exposing native `Promise`, native `queueMicrotask`, or a host scheduler object
  to guest code.
- A complete Stage-3/ES-next Promise surface in the first patch.  In
  particular, `Promise.withResolvers`, `Promise.try`, cancellation, and custom
  species/subclassing are deferred.
- Reworking the guest generator protocol, except where its async branch must
  consume an explicit host task rather than a raw thenable.
- Supporting arbitrary host thenables as an accidental compatibility fallback.
  A host integration must wrap/adapt its asynchronous work explicitly.
- Treating all foreign tenant promise shells as locally branded promises.  They
  travel through `MergedTenant` bridge exotics and are operated on through the
  execution-facing tenant, like every other guest value.

## Terminology and the non-unification invariant

The implementation uses four intentionally different concepts.

| Name | Meaning | Guest-visible? | May the driver await it directly? |
| --- | --- | --- | --- |
| **Guest promise** | Tenant-owned Promise instance from `promisePrimordial`. | Yes | No |
| **Guest thenable** | Any guest object with a guest-visible `then`; it is observed only with `tenant.get` + `tenant.invoke`. | Yes | No |
| **Host task** | Nominal/opaque host asynchronous work supplied by an explicit capability. It may be backed by a native `Promise`. | No | Yes, through its capability |
| **Host promise** | The host-facing JavaScript `Promise` returned by an adapter/API after it observes a guest promise. | Host only | Not unless explicitly wrapped as a host task |

A host native promise can be the *implementation* of a `HostTask`, but it is
not thereby a guest promise.  Conversely, a guest promise can be implemented
using host scheduling internally, but neither its shell nor its `.then` method
may be passed to bare native `await`, `Promise.resolve`, `Reflect.apply`, or a
host async adapter.

The hard rule is:

> No generic code may decide that a value is awaitable by looking for `.then`,
> using `instanceof Promise`, or accepting `PromiseLike`.  A host-task brand is
> the only driver suspension signal.  Guest Promise Resolution is a separate,
> tenant-aware operation.

This rule applies to types, runtime adapters, the tenant driver, VM/JIT/WASM
execution, WSDOM, tests, and generated code.

## Current incorrect unification inventory

The plan starts by making these assumptions explicit so none are missed during
migration.

### Type system and adapters

`packages/jade-js/tenants/rewrite.ts` currently:

- represents ambient async results as `Promise<R>` in `FnResult`;
- recognizes `R extends Promise<infer U>` for both directions;
- maps `HostToGuest<Promise<U>>` to `Promise<HostToGuest<U>>`; and
- maps `GuestToHost<Promise<U>>` to `Promise<GuestToHost<U>>`.

That claims the same `Promise<T>` exists on both sides of the boundary.  The
compile fixture `packages/jade-js/rewrite.typecheck.ts` reinforces the same
assumption with native `Promise.resolve` guest stubs.  Runtime `hostToGuest`
and `guestToHost` have no Promise conversion branch, so values covered by an
`any`/object/function conversion can pass through as raw host promises.

### Tenant composition and ABI helpers

`packages/jade-js/tenants/driver.ts` defines `isPromiseLike` by host-reading
`.then`.  It uses that for all four driver variants: sync rejects it, async
`await`s it, and generator variants yield it.  `TenantYield` also names
`PromiseLike<any>`.

`packages/jade-js/tenants/narrow.ts` does the same after a function invocation:
`invokeGuestAware` detects a raw `.then` and yields it to the driver.  Thus a
plain guest object with a guest `then` can be host-inspected/called by native
Promise machinery, and a host promise can silently become a guest result.

Existing composition tests deliberately use `Promise.resolve(...)` and function
thenables as tenant trap results.  They prove today's behavior, but they must
be rewritten as explicit host-task tests rather than preserved as public
semantics.

### Execution backends

- `packages/jade-js/vm.ts` emits/uses bare `await val` for opcode `AWAIT` in
  the async interpreter variants.  Nested async closures call
  `runVirtualizedA`, which returns a native promise directly.
- `crates/jade-vm-jit/src/lib.rs` emits `state[dest] = await value;`.  Tier 2
  (`jade-vm-jit-swc`) reuses this per-op emission, so it inherits the bug.
  JIT async function declarations also natively return host promises.
- `crates/jade-vm-wasm/src/lib.rs` uses `js_sys::Promise::resolve` and
  `JsFuture::from` at each async `AWAIT`/delegation site.  Its async entry and
  async generator helper paths produce native JS promises directly.

A raw language-level `await` has thenable assimilation semantics.  Therefore
all four paths can invoke a guest `then` outside the tenant boundary.

### Transport

`packages/jade-js/tenants/wsdom-runtime.ts` uses native `new Promise` for a
request/reply transport task.  That is legitimate host-side transport work,
but it is currently unbranded and indistinguishable from a guest async result.
The Rust WSDOM `RemotePromise` wrapper similarly names a remote async invocation
result without declaring whether it is a transport task or an opaque guest
promise value.

## Proposed public model

### Nominal host async capability

Add `packages/jade-js/async-host.ts` (host-side only; it is not installed on a
guest global).  It defines opaque nominal values, with module-private or
`unique symbol` brands that guest code cannot forge:

```ts
export interface HostTask<T> {
  readonly [HOST_TASK]: HostTask<T>; // nominal; no guest-facing `.then` contract
}

export interface HostAsyncCapability {
  readonly identity: object;
  enqueueMicrotask(job: () => void): void;
  observe<T>(task: HostTask<T>, onFulfilled: (value: T) => void,
             onRejected: (reason: unknown) => void): void;
}

/** Host-only native adapter. Explicitly wraps a native promise, never guesses. */
function hostTaskFromPromise<T>(
  capability: HostAsyncCapability,
  promise: Promise<T>,
): HostTask<T>;
```

`nativeHostAsyncCapability` is an explicit host adapter built with native
`Promise`/`queueMicrotask`; it never appears as a guest property.  A WSDOM or
Rust embedding can supply another implementation backed by callbacks, a Rust
future, or a remote request queue.  Its `identity` allows realm configuration
validation just as `BufferHooks` does.  A standard guest realm **requires** an
explicit `HostAsyncCapability`; there is no default capability and no
`createNativeAsyncPrimordialRealm` convenience constructor.  This keeps the
normal realm-creation API minimal and prevents a bare realm from gaining host
scheduling authority accidentally.

Only code holding the capability can observe the task.  The driver does not
call `then`; it delegates to a helper that recognizes the private host-task
brand and asks the capability to observe it.  A task produced by a different
capability is rejected rather than accidentally awaited through host behavior.

The exported host-facing API is `HostTask<T>` **exclusively**.  No Jade
runtime, tenant API, conversion API, or VM entry point uses native `Promise<T>`
as its public async type.  The one opt-in ergonomic conversion is a separately
exported adapter, kept outside the normal execution API:

```ts
/** Explicit host-only interoperability adapter; never used internally. */
function hostTaskToPromise<T>(
  capability: HostAsyncCapability,
  task: HostTask<T>,
): Promise<T>;
```

This is the only intentional conversion to a host native promise.  It is for an
embedder/user that explicitly requests native-promise interop; it is never the
representation of a guest value or an implicit result of a Jade API.

### Guest promise types and realm runtime

`packages/jade-js/primordials/promise.ts` exports nominal types for TypeScript
boundary descriptions, not a raw host `Promise` alias:

```ts
declare const GUEST_PROMISE: unique symbol;
export interface GuestPromise<T> {
  readonly [GUEST_PROMISE]: T; // compile-time direction/brand only
}

export interface PromiseRuntime {
  readonly tenant: Tenant;
  readonly async: HostAsyncCapability;
  readonly Promise: Function;
  readonly PromisePrototype: object;

  isLocalPromise(value: unknown): value is GuestPromise<unknown>;
  resolve(value: unknown): TenantGenerator<GuestPromise<unknown>>;
  reject(reason: unknown): TenantGenerator<GuestPromise<never>>;
  fromHostTask<T>(task: HostTask<T>): TenantGenerator<GuestPromise<T>>;
  toHostTask<T>(value: GuestPromise<T> | unknown): HostTask<T>;
  awaitGuest(value: unknown): HostTask<unknown>;
}
```

`toHostTask`/`awaitGuest` are host-runtime operations; they are deliberately
not methods placed on a guest promise shell.  They may recognize a local
private record directly.  For an unbranded guest thenable they perform Promise
Resolution by reading `then` via `tenant.get` and invoking it via
`tenant.invoke`; they never let native `Promise.resolve` observe it.

`PromiseRuntime` is owned by an exact **realm**, not merely by a tenant.  The
same Tenant can deliberately host two guest globals with different capabilities
or policy, so storing a mutable “current promise runtime” on `Tenant` would be
wrong.  The execution context carries the exact runtime alongside `tenant`,
`globalThis`, and `nt`; nested functions capture/thread it exactly as they do
the effective async/generator variant.

### Realm assembly

Extend the explicit primordial realm configuration:

```ts
interface PrimordialRealmOptions {
  async: HostAsyncCapability; // explicit; required to install Promise
  // existing buffers/options...
}

interface PrimordialRealm {
  // existing fields...
  Promise: Function;
  promiseRuntime: PromiseRuntime; // host/embedder control capability, not global data
}
```

`async` is required.  `createPrimordialRealm` never supplies a native default,
and it never silently omits Promise from a standard realm.  A deliberately
Promise-less bootstrap remains a separate lower-level API with a distinct type,
not a partially configured call to `createPrimordialRealm`.

The assembler installs only `realm.Promise` on `realm.globalThis`.  It does not
install `promiseRuntime`, `HostAsyncCapability`, `HostTask`, or host adapters.
It passes the same runtime to any VM/JIT/WASM execution entry that evaluates
that global.

## Guest `Promise` primordial

### Factory, identity, and state

`promisePrimordial(tenant, async)` uses a private completed-result cache keyed
by the exact tenant and the exact `HostAsyncCapability.identity`.  A second
incompatible async capability for the same tenant is a deterministic
configuration error, matching the BufferHooks rule.  The cache record contains:

- tenant-owned `Promise.prototype`;
- constructible tenant-owned `Promise` shell;
- private `WeakMap<object, PromiseRecord>` keyed by guest promise shells;
- its `PromiseRuntime`;
- the selected host scheduler capability.

`PromiseRecord` is private implementation state:

```ts
type PromiseState = "pending" | "fulfilled" | "rejected";
type PromiseRecord = {
  state: PromiseState;
  result: unknown;
  reactions: Reaction[];
  alreadyResolved: boolean;
};
```

No record, resolver, reaction list, host task, host promise, or capability is a
guest own property.  The constructor/prototype/static method shells use
`makeCallableExotic`, and all visible descriptors are installed with the
existing primordial helper/tenant descriptor path.

A guest promise is an object owned by the execution-facing tenant.  In a
`MergedTenant`, it follows canonical bridge/exotic routing; a primordial must
not retain a raw provider object to “recognize” it across a router boundary.

### Initial surface

The initial implementation is intentionally limited to the constructor and
chaining core:

- `new Promise(executor)`; calling `Promise(...)` without construction throws;
- `Promise.prototype.then`, `.catch`, and `.finally`;
- `Promise.resolve` and `Promise.reject`;
- getter-free identity/brand behavior implemented by private records, with
  wrong receivers throwing `TypeError`.

`Promise.all`, `Promise.race`, `Promise.allSettled`, `Promise.any`,
`Promise.withResolvers`, subclasses, `Symbol.species`, host-native iterable
support, and other static APIs are deliberately deferred.  Later additions
must extend this plan's resolution, ownership, type-boundary, driver, and test
rules rather than reintroduce a raw host iterable/Promise fallback.

### Resolution procedure

Implement Promise Resolution in one central runtime function and reuse it from
constructor resolve functions, `Promise.resolve`, and chained reactions.  When
the deferred static APIs are implemented, they must use this same function;
they do not get a native-Promise shortcut.  It must implement the relevant ES
orderings:

1. Ignore repeated resolve/reject calls after the first observable settlement.
2. Reject self-resolution with `TypeError`.
3. Adopt a same-runtime, same-record guest promise without exposing native
   state.
4. For an object/function guest thenable, read `then` with `tenant.get`; if it
   is callable, schedule/invoke it through `tenant.invoke` with the thenable as
   `thisArg` and fresh once-only resolve/reject callbacks.
5. For an explicit `HostTask`, attach an explicit host observer and settle the
   guest shell from its result.  This is the only host async adoption path.
6. Otherwise fulfill with the value.

Getter errors, invocation errors, thenable cycles, and reentrancy must reject
rather than escape through a host microtask.  A reaction is always scheduled
through `HostAsyncCapability.enqueueMicrotask`, including reactions attached
to an already-settled promise.  It is never run inline merely because the
promise is already settled.

`HostAsyncCapability` has a required host-only unhandled-rejection hook.  The
runtime calls it once when a rejection survives the specified observation point
(for the initial surface: after the scheduled reaction/checkpoint that confirms
there is no rejection handler), and may call an optional handled-later hook if
a handler is attached after reporting:

```ts
interface HostAsyncCapability {
  readonly identity: object;
  enqueueMicrotask(job: () => void): void;
  observe<T>(task: HostTask<T>, onFulfilled: (value: T) => void,
             onRejected: (reason: unknown) => void): void;
  onUnhandledRejection(reason: unknown, promise: GuestPromise<unknown>): void;
  onRejectionHandled?(promise: GuestPromise<unknown>): void;
}
```

These callbacks are host capabilities, never guest properties.  They must not
throw into promise-reaction processing; an embedding either contains/reports a
hook failure itself or Jade treats it as a host integration failure outside
normal guest settlement semantics.  Tests establish reporting order, exactly
once reporting, and late-handler notification.

A reaction invokes its guest callback through `tenant.invoke`.  Its return is
resolved through the same procedure, so a guest callback may return a guest
promise, guest thenable, ordinary value, or explicitly adapted host task.
Thrown guest errors reject the chained promise.  The reaction runner drives
nested tenant work through the normal driver using the runtime’s explicitly
selected async-capable host task pathway; it never calls a tenant generator’s
`.next()` itself.

### Constructor timing and the missing construction opcode

The existing tenant ABI supports `{ kind: "construct", newTarget }`, but the
current bytecode survey shows CALL and no general Jade construction opcode.  A
useful guest Promise constructor therefore requires a construction work item:

1. add/verify frontend parsing and bytecode representation for `new`;
2. add a CONSTRUCT execution path in `packages/jade-data/index.ts`/generated
   `vm.ts`, JIT tiers, WASM, and the operation crate;
3. emit `tenant.invoke(callee, { kind: "construct", args, newTarget })`, with
   the invoked constructor’s actual `newTarget`, never the surrounding `nt`;
4. test native callable exotics and normal guest constructors before Promise
   uses the path.

`new Promise(executor)` invokes its executor synchronously as required by the
language.  The construct handler is still a tenant generator because the
executor is a guest callable.  It allocates/registers the shell and resolving
functions first, invokes the executor through `tenant.invoke`, and converts a
throw into rejection.  If an executor requires remote asynchronous tenant work
before returning, that work is not silently awaited by construction; it follows
the active execution variant and must settle through its provided resolve/
reject functions.  The detailed implementation must preserve observable
synchronous executor ordering while permitting the shell’s later reactions to
use scheduled jobs.

## Explicit conversion at the host/guest boundary

### Type rewrite replacements

Replace raw native `Promise` conditionals with directional nominal types.
The exact exported spelling is reviewable, but the intended split is:

```ts
export type HostFnResult<R, AA, AG> = /* ambient async => HostTask only */;
export type GuestFnResult<R, AA, AG> = /* ambient async => GuestPromise */;

export type HostToGuest<T, AA = false, AG = false> =
  // HostTask<T>/declared host async result -> GuestPromise<HostToGuest<T>>
  // and a host function's async result -> GuestPromise<...>

export type GuestToHost<T, AA = false, AG = false> =
  // GuestPromise<T>/declared guest async result -> HostTask<GuestToHost<T>>
  // and a guest function's async result -> HostTask<...>
```

`FnResult` must no longer be a direction-free alias that returns native
`Promise<R>`.  Either replace it with the two directional result types or make
its direction an explicit required type parameter.  `TenantOpResult` likewise
uses the host-task result type because a tenant operation is host-side control
flow, not a guest value.

`HostToGuest<HostTask<U>>` recursively becomes `GuestPromise<HostToGuest<U>>`.
`GuestToHost<GuestPromise<U>>` recursively becomes `HostTask<GuestToHost<U>>`.
Native `Promise<U>` and `PromiseLike<U>` are not generic cross-boundary cases.
A host API wanting native-promise interoperability must first expose a
`HostTask` and then let its user call the separately exported
`hostTaskToPromise`; no core Jade signature returns native `Promise`.  A type
fixture must prove that an unwrapped `Promise<U>` cannot be passed as a guest
promise.

Update `rewrite.typecheck.ts` so an async guest stub returns `GuestPromise<T>`,
not `Promise<T>`, and a host wrapper accepts/returns the appropriate host task
adapter.  Add negative `@ts-expect-error` cases for both wrong directions and
for raw native Promise interchange.

### Runtime adapters

Make conversion asynchronous where it needs to materialize/observe guest
state.  The recommended APIs separate immediate values from generator work:

```ts
function* hostToGuest<T>(
  spec: NarrowSpec<T>, value: T, context: GuestConversionContext,
): TenantGenerator<HostToGuest<T>>;

function* guestToHostTask<T>(
  spec: NarrowSpec<T>, value: unknown, context: HostConversionContext,
): TenantGenerator<GuestToHost<T>>;
```

Contexts contain the exact tenant and `PromiseRuntime`; host conversion also
contains the explicit `HostAsyncCapability`.  Existing convenience APIs may
remain as sync-only wrappers for specs that cannot contain guest/host async
values, but they must throw a deterministic error if asked to cross a Promise
boundary.  They must not inspect `.then` to guess which route is needed.

For host-to-guest conversion, a declared `HostTask` creates a guest promise via
`runtime.fromHostTask`.  For guest-to-host conversion, a declared guest promise
is observed through `runtime.toHostTask`, then exposed as a native host promise
only by an explicit outer adapter.  Rejection reason and fulfillment value use
the same directional conversion recursively; a host `Error` is not silently
made a guest object by native promise plumbing.

`NarrowSpec` gains explicit `hostTask` and `guestPromise` forms (or an
orthogonal async-wrapper representation), so object-field conversion has a
stated rule.  `kind: "any"` remains an untyped escape hatch but is documented
as not authorizing raw async crossing; boundary APIs reject/require an explicit
adapter for async values under `any`.

### Function adapters

A host function exposed to guest code that declares a host task result is
wrapped so its result becomes a guest promise.  A guest function exposed to a
host receives a plain host callable whose async result is **always a
`HostTask`**, produced by `runtime.toHostTask`.  Only the separately exported
`hostTaskToPromise` interoperability adapter can turn that result into a native
host promise.  Function adapters retain the existing ABI rule (`tenant`, exact
`nt`, ordinary apply vs construct), but must not call `invokeGuestAware` and
then hand its raw native async result through.

This fixes the current `guestToHost` leading-tenant adapter: it remains a
plain host function, but its async return is adapted through the supplied
runtime rather than treated as a guest/native promise by coincidence.

## Tenant driver and composability changes

### Tagged yields only

Replace this current conceptual union:

```ts
type TenantYield = TenantOp<any> | PromiseLike<any> | Iterator<any, any, any>;
```

with a closed host-control union such as:

```ts
type TenantYield = TenantOp<any> | HostTaskYield<any> | HostIteratorYield<any>;
```

`HostTaskYield` carries an unforgeable task/capability brand.  A host tenant
method, WSDOM request, buffer hook, or embedder trap that needs suspension
returns/yields this wrapper.  A guest Promise or arbitrary guest thenable is
ordinary guest data to the driver and is returned to its caller unchanged.

The sync driver rejects a `HostTaskYield` with the existing precise
`addAsync`-style error.  Async and async-generator drivers observe the wrapped
host task through the capability.  Generator drivers preserve their existing
host-scheduler protocol, but do not treat guest promises as yielded scheduler
values.  The implementation should tighten `TenantGenerator` from its current
`Generator<any, R, any>` back to the closed yield union once all existing
operations are migrated, so future accidental raw promise yields fail at type
check.

### ABI helper behavior

Remove raw thenable detection from `invokeGuestAware`.  It returns the result
of `Reflect.apply`/`Reflect.construct` (which are only used to call trusted
host/native function implementations after ABI selection) as a value unless
the host function explicitly returns a `HostTaskYield`.  It must not read
`raw.then`, and it must not yield a guest promise.

Likewise, `invokeTrap` simply composes `invokeGuestAware`; descriptor getters,
setters, proxy traps, and primordial reaction callbacks follow the same tagged
host-task rule.  Existing host trap providers migrate from:

```ts
() => Promise.resolve(value)
```

to an explicit host capability helper that yields/returns a `HostTaskYield`.
Tests retain async coverage but prove the new explicit contract instead of the
old thenable heuristic.

### WSDOM request tasks

`installServerTenantRuntime` wraps `bridge.request(...)` in the WSDOM
`HostAsyncCapability` before yielding it.  It does not expose the raw native
request promise to tenant composition.  Its operation facade must be extended
with the descriptor/prototype methods added by the primordial work as well;
that omission is an existing follow-up needed before Promise/proxy values can
be used through the WSDOM tenant surface.

A server/browser guest promise is represented as an owned/remote guest value
(or a bridge exotic), not as a transport task.  Host code that wants to await
that value uses the realm-specific `PromiseRuntime.toHostTask` route, which
performs tenant-aware observation on the correct side.  Rename/split Rust
`RemotePromise` into names that distinguish *transport invocation task* from
*remote guest Promise value*, and update `IntoFuture` only for the former or
for an explicit guest-promise adapter.

## `AWAIT`, interpreter, JIT, and WASM

### One promise-runtime operation per `AWAIT`

All backends use the same conceptual operation:

```ts
const task = promiseRuntime.awaitGuest(value);
const resolved = awaitHostTask(task); // backend host control flow only
```

`awaitGuest` implements Guest Promise Resolution and returns a **trusted host
task**, never a guest shell/native thenable.  The host `await` in the second
line is therefore legal only inside an implementation of the selected backend
or host capability.

This preserves native JavaScript `await` language semantics for guest values
without granting native JavaScript the right to assimilate them directly.

### Generated TypeScript VM: retain names, make results `HostTask`

The TypeScript VM is generated, not hand-maintained.  The source of truth is
`packages/jade-data/index.ts`; `scripts/gen/vm-ts.ts` emits the four runner
variants; `scripts/regen.ts` imports the built `packages/jade-data/dist/index.js`
and writes `packages/jade-js/vm.ts` as well as the Rust opcode/data dispatch
outputs.  The implementation changes this complete pipeline, then runs
`scripts/regen.ts` and reviews every generated diff.  It must also reconcile
any generator/template changes made since `vm.ts` was first produced—especially
canonical relocated imports (`./tenants/shims.ts`, `./tenants/narrow.ts`) and
current tenant invocation/driver conventions—rather than preserving stale
output by hand-editing `vm.ts`.

The exported names remain `runVirtualized`, `runVirtualizedA`,
`runVirtualizedG`, and `runVirtualizedAG`; they do not acquire a new public
name or silently switch to native `Promise` results.  Their execution context
adds the exact `promiseRuntime` required by the realm.  Their host-facing
results use `HostTask` exclusively:

- `runVirtualizedA` returns `HostTask<unknown>`, not `Promise<unknown>`.
- `runVirtualizedAG` remains an explicit host async-iterator/task protocol;
  each async step uses a `HostTask`, never a bare native promise.
- `runVirtualized` and `runVirtualizedG` preserve their synchronous/generator
  contracts, and upgrade/reject through the explicitly selected runtime rather
  than a host fallback.
- A separate exported `hostTaskToPromise` adapter is the only way a user turns
  `runVirtualizedA(...)` or an async iterator step into a native `Promise`.

At generation time, carry `promiseRuntime` in every context literal and nested
`FN` closure (`globalThis`, `nt`, `tenant`, effective flags, and
`promiseRuntime` all remain aligned).  `AWAIT` code emitted by the template
becomes the runtime's trusted task-observation operation rather than
`await val`.  The template must also make an async nested guest function return
`GuestPromise` to guest callers by wrapping its internal `HostTask`; it must
not expose its host runner task/native implementation result.

Update the generated VM type imports to canonical tenant/primordial/async-host
modules and make `scripts/regen.ts` fail early with a clear instruction if the
jade-data build output is stale or unavailable.  The implementation command
sequence is: build `packages/jade-data` (`build.sh`/its package build step),
run `scripts/regen.ts`, typecheck, and review the generated JS/Rust files.  Do
not directly edit `packages/jade-js/vm.ts` except as an immediately regenerated
verification artifact.

### JIT tiers 0, 1, and 2

Add the runtime explicitly to generated wrapper/function ABI or the generated
execution context.  It must be independently threaded through a nested `FN`
body just as `tenant`, `nt`, and effective variant bits are threaded.  It is
not a new ambient global and must not silently be read from host `globalThis`.

At the one existing per-op emission site in `crates/jade-vm-jit` (`Operation::Await`),
emit `await promiseRuntime.awaitGuest(value)` (or the resolved host-name
resolver equivalent), not `await value`.  Tier 2 continues to reuse this exact
emission through `ops_to_js`; it must not recreate Promise behavior itself.

The JIT function registry needs a guest-visible async wrapper: native compiled
async implementation results are host tasks internally, while the registered
guest function shell converts them to a guest promise using its captured
`promiseRuntime`.  The wrapper retains the existing `leading-tenant-nt` ABI
and exact `new.target` rule.  The top-level host compile API exposes a
`HostTask`, not a native promise; only `hostTaskToPromise` offers optional user
interoperability.  Do not mark a native async implementation itself as a guest
promise-producing value.

Tests must cover Tier 0, reloop Tier 1 when enabled, and Tier 2 nested async
functions.  In particular, prove a guest promise returned by a nested async
function is not `instanceof` the host `Promise` constructor and that `AWAIT`
settles it through the runtime without host-reading its guest `then`.

### WASM backend

Replace each `js_sys::Promise::resolve(&value)` / `JsFuture::from(...)` use at
a guest `AWAIT` or guest generator delegation boundary with calls to an
explicit JavaScript `PromiseRuntime`/host-task bridge binding.  Rust may await
the resulting trusted transport/host task, but it may not invoke native
`Promise::resolve` on a guest value.

Thread the runtime through `WasmPlatform`, async run functions, async generator
machines, closures, nested functions, and exported entry points.  Keep
`future_to_promise` only inside the separately exported host interoperability
adapter that implements `HostTask`/`hostTaskToPromise`; convert its result to a
guest promise before exposing it to guest code.  Audit all `Promise::resolve`,
`Promise::reject`, `JsFuture::from`, and JS `then` bindings in
`crates/jade-vm-wasm` individually—some are legitimate host adapter code, but
none may be an implicit guest-resolution path.

## Construction and metadata prerequisites

1. Complete the construction opcode work described above before advertising
   guest `new Promise` source support.
2. Extend guest-function metadata only as needed to distinguish an internal
   native async implementation from its guest-visible promise-wrapping shell.
   Do not infer this distinction from `fn.constructor`, `instanceof Promise`,
   or function source text.
3. Keep `promiseRuntime` out of `TENANT_METHOD_NAMES` and out of tenant method
   inlining.  It is a realm/execution capability, not a tenant helper that a
   splice site can invent.
4. Update the SWC tenant-exposure allow-list only if a selected tenant helper
   legitimately needs a `Promise` binding.  It must refer to the guest realm
   global through explicit parameters/tenant access, never a free native host
   `Promise` import.

## Error, ordering, ownership, and security rules

- The guest `Promise` constructor rejects non-callable executors; `.then`,
  `.catch`, `.finally`, and static methods validate receivers/arguments through
  tenant operations.
- User callback/then getter/invocation errors become rejection reasons.  They
  must not strand host tasks or throw from a host scheduler callback.
- Resolver functions are once-only.  A thenable can call resolve/reject more
  than once, throw after a call, or resolve with itself; all are handled by the
  central resolution procedure.
- Promise reaction callbacks execute in capability-scheduled microtasks, not
  inline.  Tests must establish constructor executor vs reaction ordering.
- No raw host promise, host scheduler, resolver callback, task brand, or
  PromiseRecord appears in a tenant property map, WSDOM descriptor, bridge
  payload, or generated guest global.
- Promise identities are tenant/realm identities.  Values move through
  `MergedTenant` and WSDOM only via their normal bridge/opaque-handle paths.
  A bridge is not rebranded by a destination factory merely because it exposes
  a `then` property.
- Native host `Promise.resolve(guestValue)`, bare `await guestValue`, raw
  `PromiseLike`, and `instanceof Promise` are prohibited in Jade guest/runtime
  code.  A narrowly documented host adapter may use native promises only after
  it has received an explicit `HostTask`.

## Tests

### Guest Promise primordial

Add `packages/jade-js/promise.e2e.ts` (or extend the primordial E2E suite) for
both `MultiTenant` and `single_tenant`:

- cache identity is stable for the same exact tenant/capability and distinct
  across tenants; incompatible capability installation fails;
- `Promise`, prototype, constructor, instance, resolver functions, and methods
  are tenant-owned (not host own-property storage);
- constructor calls executor synchronously; `.then` reactions run later in
  deterministic microtask order;
- fulfill, reject, catch, finally, throw propagation, chained returned guest
  promise, thenable adoption, self-resolution, and multiple resolver calls;
- `resolve`, `reject`, and the initial chaining surface/error cases; deferred
  static APIs have an explicit unsupported/missing-property test that points to
  this plan;
- a guest promise shell is not a native host promise and cannot be passed to
  an unguarded host `await` test path;
- `Proxy` around a promise/thenable has its `then` getter and call observed
  through tenant operations, proving no host property fallback;
- `Proxy.revocable` thenable/rejection behavior after revocation;
- `MergedTenant` promise values bridge correctly without a raw foreign object
  being retained by the promise runtime.

### Driver and adapter separation

Replace/add tests in `tenant-compose.e2e.ts`, `rewrite.e2e.ts`, and
`rewrite.typecheck.ts`:

- raw native Promise and arbitrary `.then` objects are not accepted as driver
  yields;
- explicitly wrapped host tasks suspend/resolve only when `addAsync` is set;
- a guest promise returned by a trap is ordinary guest data, not eagerly
  unwrapped by `driveTenant`;
- host-to-guest host-task conversion produces a guest promise; guest-to-host
  conversion produces an explicit host task only; `hostTaskToPromise` is tested
  separately as user-requested interop;
- type tests reject `Promise<T>` where `GuestPromise<T>` or `HostTask<T>` is
  required in either direction;
- guest async function adapters produce guest promises; every host adapter and
  the retained `runVirtualizedA` entry point produces `HostTask`; only the
  separately exported user adapter produces a native promise;

### Backend conformance

For the same bytecode program and realm runtime, test TS VM, JIT Tier 0, Tier
1 (feature-gated), Tier 2, and WASM where available:

- `AWAIT` a fulfilled/rejected guest promise;
- `AWAIT` a guest thenable whose getter/invocation is tenant-observable;
- nested async functions return guest promises and preserve exact `nt`;
- sync variants reject required host task suspension deterministically;
- no generated source contains bare `await state[...]` at an `AWAIT` operation
  or `Promise.resolve(guestValue)`;
- the exported TypeScript VM runners retain their names and return the declared
  `HostTask` contracts; `hostTaskToPromise` alone returns an ordinary native
  promise when an embedder opts in.

### WSDOM and lifecycle

- WSDOM request/reply uses a tagged transport host task.
- Browser guest Promise results stay remote guest values; host observation uses
  an explicit runtime adapter.
- Settlement/rejection releases callbacks and no task/reaction keeps a
  tenant/remote handle alive after both shell and subscriptions become
  unreachable.  Normal lifetime remains native GC/weak state; do not add a
  guest-visible `close` API or depend on FinalizationRegistry for correctness.

## Implementation phases

1. **Document and type the settled split.** Add `async-host.ts`, nominal
   host/guest async types, the required unhandled-rejection hook, type tests,
   and lint/search guards.  Make `HostTask` the exclusive host contract and
   expose `hostTaskToPromise` only as separately exported user interop.  Change
   no guest global behavior until explicit conversion APIs compile.
2. **Migrate composability.** Replace `PromiseLike`/`.then` detection in tenant
   types, driver, narrow ABI helpers, tenant tests, and WSDOM request facade
   with tagged host tasks.  Preserve existing host async integrations only via
   explicit adapters.
3. **Build the guest Promise runtime.** Implement `primordials/promise.ts`,
   central resolution/reactions and required unhandled-rejection reporting,
   cache/error rules, and required-capability realm assembly.  The initial
   static surface is constructor plus `resolve`/`reject`; record each omitted
   static API as a follow-up to this plan rather than adding a fallback.
4. **Convert boundary adapters.** Implement explicit host-task ↔ guest-promise
   conversion in rewrite types/runtime and migrate consumers.  Make sync-only
   helpers fail clearly on async specs.
5. **Add/finish construction.** Implement the general construct bytecode path
   across TS/JIT/WASM and prove the `new.target` ABI before enabling `new
   Promise` in guest source.
6. **Regenerate and thread the TypeScript VM runtime.** Update
   `packages/jade-data/index.ts` and `scripts/gen/vm-ts.ts`, rebuild jade-data,
   run `scripts/regen.ts`, and review the regenerated `vm.ts`, VM data, and
   dispatch files.  Preserve the four runner names, make their async host
   contracts `HostTask`, thread `promiseRuntime` into nested `FN` contexts, and
   reconcile generator output with all current canonical tenant imports/API
   changes since the original VM generator output.
7. **Thread runtime through the other backends.** Update the shared JIT
   per-op emission/Tier 2 recursion and then WASM.  Wrap guest-visible async
   function results and replace every guest `AWAIT` route.
8. **Transport and regression audit.** Split WSDOM names/protocols, audit all
   native Promise/thenable uses, run full type/E2E/Rust suites, and update
   `AGENTS.md` with the host-task/guest-promise invariant at implementation
   time.

Each phase lands with a repository-wide search for `PromiseLike`,
`instanceof Promise`, `.then`, `Promise.resolve`, `await `, `JsFuture::from`,
and `future_to_promise`.  Each remaining occurrence is classified as either a
narrow host adapter with an explicit `HostTask` or a bug; it is never left
unreviewed because it merely appears harmless.

## Settled design decisions

1. **Required realm capability.** `createPrimordialRealm` requires an explicit
   `HostAsyncCapability`.  There is no default native capability or convenience
   realm constructor: default API usage remains minimal and a bare realm never
   gains host scheduling authority accidentally.
2. **Host async type.** Jade's host-facing runtime and adapter APIs use
   `HostTask<T>` exclusively.  Native `Promise<T>` is available only through
   the separately exported, explicitly requested `hostTaskToPromise` user
   adapter; it is never a core Jade boundary type.
3. **Initial static surface.** Ship only constructor, `Promise.resolve`, and
   `Promise.reject` with chaining.  Deferred Promise static methods remain
   documented follow-ups in this plan and must reuse its explicit resolution,
   ownership, and host-task rules when added.
4. **Unhandled rejections.** `HostAsyncCapability` has a required host-level
   unhandled-rejection hook and an optional handled-later hook.  Both remain
   host-only; reporting is tested as part of Promise reaction scheduling.
5. **Generated TypeScript VM API.** Keep `runVirtualized`, `runVirtualizedA`,
   `runVirtualizedG`, and `runVirtualizedAG` names.  Update their generated
   implementation and types through `packages/jade-data`, `scripts/gen`, and
   `scripts/regen.ts`; async host-facing results are `HostTask`, and native
   Promise conversion is only the separate user adapter.

## No remaining blocking design decisions

The implementation may choose internal symbol names and exact helper module
partitioning consistent with the contracts above, but it must not reopen these
five settled boundary decisions without an explicit design review.
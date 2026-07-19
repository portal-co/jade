# Plan: exotic tenant objects and merged tenant routing

## Summary

Jade already treats a `Tenant` as the owner of all guest-visible object-property
operations.  This plan extends that boundary in two related directions:

1. **Exotic objects:** a tenant can manufacture an object whose Jade operations
   are implemented by a handler, much like the relevant subset of JavaScript
   `Proxy` traps.  Exotics can also be callable: their public value is a native
   function carrying the tenant call ABI while its own properties continue to
   live in the tenant representation.  A tenant implementation can create
   these facades for its own use, and an embedder/caller can ask a tenant to
   create one through the public tenant API.
2. **Merged tenants:** one runtime-facing `Tenant` composes a primary tenant
   with additional object providers.  Each object has exactly one provider;
   operations route to that provider rather than being broadcast.  Values which
   cross from one provider into another are represented at the destination as
   exotic bridge objects, never as the source provider's raw object.

The first concrete target is a browser runtime with a local `MultiTenant` and a
WSDOM server tenant.  A local object remains local; a server value observed by
local guest code is an exotic local facade; and a local object passed to the
server is an exotic server-side facade.  This is a single runtime with several
providers, not a switch from one tenant to another per invocation.

This document is a design plan.  It deliberately does not change the existing
`Tenant` ABI yet.

## Existing constraints

The design must preserve these established contracts:

- The VM, JIT, and WASM backend access guest-visible properties only through
  `tenant.make/get/set/has/delete/ownKeys/define/assign` and must route guest
  calls through the tenant callable ABI; no implementation may bypass the
  tenant by using direct property access for guest objects or a bare
  `Reflect.apply` for a tenant-managed callable.
- Every tenant operation is a generator.  Call sites compose it only through
  `tenant.driveTenant(gen, addAsync, addGen)`.  Nested tenant work is expressed
  through `yield this.yieldTenant(...)`, never through `.next()`.
- Ambient async/generator capability is threaded once per backend emission
  site.  A merged or exotic operation that reaches the WSDOM server may yield a
  `Promise`, so it must participate in the existing `addAsync`/`addGen`
  protocol rather than inventing a second promise bridge.
- The ABI helpers (`markGuestFn`, `invokeGuestAware`, `invokeTrap`,
  `createGuestGen`, `unpackGuestGen`, `yieldTenant`, and `driveTenant`) stay
  real tenant methods injected by `guestAbiMixin`.  Do not add them to
  `TENANT_METHOD_NAMES` or inline them away.
- **`nt` is `new.target`, not an ambient capability or an inherited call
  context.** The callable ABI is `(tenant, nt, ...args)`: ordinary apply sets
  `nt` to `undefined`; construction sets it to that invocation's `newTarget`.
  `NEW_TARGET` observes this exact value. Any new `CALL`/construct routing,
  native function shell, tenant helper, bridge, or backend must use the same
  discriminated invocation form—never forward an enclosing `nt` through an
  apply.
- Tenant-method source inlining is optional.  Dynamic router/bridge methods
  must remain correct when they cannot be inlined (and initially should not be
  offered as inlinable source).
- The current tenant API's `ownKeys` means **own enumerable keys**, and `set`
  returns no boolean.  The initial exotic API must model these Jade semantics,
  not claim full ECMAScript `Proxy` invariants which Jade does not presently
  expose.

Relevant current code is `packages/jade-js/index.ts`, `multi_tenant.ts`,
`single_tenant.ts`, `driver.ts`, `narrow.ts`, `rewrite.ts`, and
`wsdom-tenant-runtime.ts`; backend call sites are generated from
`packages/jade-data/index.ts` and emitted/interpreted by the JS, JIT, and WASM
backends.

## Goals and non-goals

### Goals

- Make an exotic object's behavior explicit, typed, generator-composed, and
  usable by every current tenant operation.
- Let a tenant construct an exotic object internally and let an embedder create
  one through the same public API and driver protocol.
- Provide a `MergedTenant` (name provisional) with a primary provider and any
  number of secondary providers.
- Preserve object identity: repeatedly marshalling the same source object to
  the same destination returns the same destination facade while it is live.
- Prevent raw foreign objects from leaking into another provider's object
  namespace.
- Support both directions of the `MultiTenant` + WSDOM-server case, including
  recursive values returned by property reads and supplied to property writes.
- Make ownership, lifecycle, error propagation, async behavior, and callable
  ABI behavior observable and testable.
- Make every standard tenant representation capable of returning a native
  function shell when a created object is callable, while keeping that
  function's own guest-visible properties in the tenant rather than on the
  shell.

### Non-goals for the first implementation

- Implementing native JavaScript `Proxy` objects or every ES internal method.
  The interface is *Proxy-trap-like*, but is defined by Jade's tenant surface.
  A future, separately designed `Proxy` capability may expose exotic creation to
  Jade source; this plan keeps exotic construction host-facing.
- Fan-out or conflict resolution where two providers simultaneously store
  properties for the same object identity.  An object has one provider at a
  time.
- Transparently virtualizing arbitrary host code that directly reads
  `obj[key]`, uses `Object.keys`, or calls a guest function.  Jade bytecode uses
  the tenant surface; host consumers must use tenant conversion/boundary APIs.
- Cross-provider callable/function virtualization is **not** deferred.
  Callable objects must cross a provider boundary as callable bridge exotics;
  they retain normal native-function callability, use the tenant ABI, and keep
  their own property operations tenant-managed.  Arbitrary direct host calls
  outside Jade's tenant-aware boundary remain out of scope unless the embedder
  explicitly uses the documented callable adapter.
- Changing closure-capture support or weakening the JIT tenant-inlining
  scanner's rejection rules.

## Terms and ownership model

- **Provider:** an ordinary `Tenant` implementation such as `MultiTenant`,
  `single_tenant`, or the WSDOM server facade.
- **Primary provider:** the provider used by bytecode-level object creation
  (`make`, `FN` shells, and any current bytecode allocation).  It is not a
  global default/fallback owner for foreign values.  In the local/server
  example, this is normally `MultiTenant`.
- **Owner:** the one provider responsible for an object's real storage and
  operations.
- **Exotic:** a tenant-managed facade with a handler implementing Jade's object
  operations and, optionally, its call operation.  A callable exotic is exposed
  as a native function shell, so it remains callable where a normal Jade
  function is callable; its own guest-visible properties nevertheless remain
  exclusively tenant-managed.
- **Bridge exotic:** an exotic created in destination provider `B` to represent
  a source-provider `A` object or callable.  Its handler routes operations and
  calls to `A` and recursively marshals values across the `A`/`B` boundary.
- **Router:** the `MergedTenant` instance exposed to the VM/JIT.  It selects an
  object owner and drives every selected provider operation under the router's
  ambient execution boundary.

Ownership is exclusive and stable:

1. An allocation is registered with exactly one owner before it is returned.
2. The router dispatches a property operation only to that object’s owner.
3. Unknown non-primitive objects are not probed by every provider: probing can
   run traps, produce async work, and create observable side effects.  They are
   either registered explicitly, treated as documented primary-native input, or
   rejected.
4. Passing an object from owner `A` to code operating in `B` creates or reuses
   an `A -> B` bridge exotic.  The source object itself does not cross.
5. Passing the bridge back to `A` unwraps it to the original source identity;
   passing it to a third provider produces/reuses an `A -> C` facade rather
   than stacking `A -> B -> C` bridges.

The router's routing metadata is separate from user-visible tenant properties.
Each provider independently implements a pure, non-trapping
`ownsObject(value)` predicate for values it owns.  A `MergedTenant` queries
only its explicitly enrolled providers and may maintain router-local weak
caches/owner hints for bridges and allocations it observes; it is not the
exclusive registration authority.  This allows a provider to first create and
operate on its objects independently, then join one or more `MergedTenant`s;
merged routers may themselves participate in another `MergedTenant`.  Do not
discover ownership by calling `get`, `has`, or `ownKeys`, and do not let router
membership mutate a provider's independent ownership record.

## Proposed public and internal interfaces

### 1. Jade exotic-handler and callable-allocation surface

Add an explicit handler type alongside `Tenant` in `packages/jade-js/index.ts`.
The exact spelling is provisional, but it should resemble the following:

```ts
interface TenantExoticHandler {
  get?(receiver: object, key: PropertyKey): TenantGenerator<unknown>;
  set?(receiver: object, key: PropertyKey, value: unknown): TenantGenerator<void>;
  has?(receiver: object, key: PropertyKey): TenantGenerator<boolean>;
  delete?(receiver: object, key: PropertyKey): TenantGenerator<void>;
  ownKeys?(receiver: object): TenantGenerator<PropertyKey[]>;
  define?(receiver: object, descriptors: object): TenantGenerator<void>;
  assign?(receiver: object, source: object): TenantGenerator<void>;
}

/** `nt` is derived from this discriminant; callers never supply it ad hoc. */
type TenantInvocation =
  | { kind: "apply"; thisArg: unknown; args: readonly unknown[] }
  | { kind: "construct"; args: readonly unknown[]; newTarget: Function };

/** The native function shell and call semantics for a callable exotic. */
interface TenantCallableExoticHandler extends TenantExoticHandler {
  /** Execute an ordinary call; its guest ABI `nt` is always `undefined`. */
  apply?(
    receiver: Function,
    thisArg: unknown,
    args: readonly unknown[],
  ): TenantGenerator<unknown>;
  /** Execute construction; its guest ABI `nt` is exactly `newTarget`. */
  construct?(
    receiver: Function,
    newTarget: Function,
    args: readonly unknown[],
  ): TenantGenerator<object>;
}

interface Tenant {
  // Existing methods ...
  makeExotic(
    proto: object | null | undefined,
    handler: TenantExoticHandler,
  ): TenantGenerator<object>;
  /**
   * Adopt a native function as a tenant-owned callable shell.  Its own
   * guest-visible properties are stored through this tenant, and `invoke`
   * dispatches it through ordinary guest/host ABI handling.  `guestMeta` is
   * supplied for a Jade function created by `FN`; omit it for a plain native
   * host function.
   */
  makeFunction(
    implementation: Function,
    options?: { proto?: object | null; guestMeta?: GuestFnMeta },
  ): TenantGenerator<Function>;
  /** Create an exotic whose public identity is a real native function. */
  makeCallableExotic(
    proto: object | null | undefined,
    handler: TenantCallableExoticHandler,
  ): TenantGenerator<Function>;
  /**
   * The only tenant-aware VM/backend call boundary.  `apply` always sets the
   * guest ABI's `nt` (`new.target`) to `undefined`; `construct` sets it to the
   * supplied `newTarget`.  It owns normal Jade ABI dispatch, callable-exotic
   * dispatch, and the declared/ambient result shape.
   */
  invoke(callee: Function, invocation: TenantInvocation): TenantGenerator<unknown>;
}
```

This is intentionally based on current Jade operations plus its existing
`CALL` semantics, rather than on every standard `Proxy` trap signature.
`invoke` is the one call-routing boundary: it receives the real native function
shell and a `TenantInvocation`, then applies either the callable-exotic handler
or the ordinary Jade/host function ABI.  In particular, it preserves the
existing rule that a registered JIT guest function is invoked as
`(tenant, nt, ...args)`, while an unregistered native host function is invoked
with its ordinary arguments.  `nt` is *not* inherited from the caller: it is
`undefined` for an apply and the current `newTarget` for construction.  The
implementation must delegate ABI selection to `invokeGuestAware` (extended to
receive the invocation/new-target information); it is not a generic host
`apply` helper.

A callable exotic must be represented by an actual function, not an ordinary
object with a faux `call` property.  That keeps normal function detection and
existing native call machinery viable.  A callable-exotic shell must be a
constructible native function shell as well, so it can receive both `apply` and
`construct`; if its selected construct handler/fallback is absent, that
construct path throws `TypeError`.  `makeFunction` instead preserves the native
implementation's own constructibility (an adopted arrow/async/generator
function remains non-constructible).  Its properties must still be stored in
the provider's shadow/exotic metadata rather than placed on the native function
shell, so `tenant.get/set/ownKeys/define/assign` work identically for callable
and non-callable tenant objects.  The representation therefore expands the
existing `make` result model from “object shell plus tenant metadata” to
“object-or-function shell plus tenant metadata.”

`makeFunction` is the corresponding non-exotic allocation path.  It adopts a
native implementation function while making it an owned tenant value with the
same tenant-side own-property representation.  Every existing path which
creates a guest-visible function must use it (or a common lower-level shell
registrar): in particular the TS interpreter's `FN` handler, JIT function
registry/wrapper installation, WASM function creation, generator helper
methods installed on tenant-created generator objects, and any server/browser
function shim.  `markGuestFn` records ABI metadata but is not enough by itself:
it does not establish router ownership or ensure that function properties are
kept in the tenant representation.  `make` remains the ordinary object-shell
allocator; callers choose `makeFunction` when the public value must be
callable.

Define missing-trap behavior before coding: **exotics fail closed.** Every
missing property trap (`get`, `set`, `has`, `delete`, `ownKeys`, `define`, or
`assign`) throws a descriptive `TypeError`; a missing `apply` or `construct`
handler does the same.  An exotic has no implicit target or fallback object,
because many virtual/bridge values have no meaningful ordinary-object behavior
to delegate to.  If a provider needs partly ordinary behavior, it must install
explicit handler methods that delegate to a provider-owned target itself.  This
policy must be encoded in types, documentation, and tests; never make it an
accidental per-method behavior.

The handler is an implementation-level capability, not a guest object that is
read through `tenant.get`.  It may contain host functions or guest functions.
When a property handler function is invoked, use `this.invokeTrap`; when a
callable handler forwards to a normal guest/host function, use the
`TenantInvocation`-aware `this.invoke`/`this.invokeGuestAware` path; and
compose all of it with `yield this.yieldTenant(...)`.  This retains guest ABI
metadata, `new.target`, and async/generator results instead of using a bare
native `.call`/`.apply`.

### 2. Callable/constructible ABI — `nt` is exactly `new.target`

This is a hard semantic rule, not an implementation convenience.  Every
Jade-callable native function has the leading ABI
`(tenant, nt, ...args)`, where **`nt` is only the JavaScript `new.target` of
that invocation**:

| Invocation kind | Native operation | Value passed as ABI `nt` | Result semantics |
| --- | --- | --- | --- |
| `apply` | `Reflect.apply` / ordinary Jade `CALL` | `undefined` | Ordinary call result. |
| `construct` | `Reflect.construct` | The invocation's `newTarget` (normally the callee) | JavaScript construction result. |

An apply inside a constructor is still an apply.  It must pass `undefined`,
**not the enclosing function's `nt`**.  Conversely, construction must not
invent `undefined` or reuse an unrelated outer `nt`: it passes its actual
`newTarget`.  `NEW_TARGET` reads precisely this per-invocation ABI value.

Use the `TenantInvocation` discriminated union at every boundary so a caller
cannot accidentally thread an ambient/outer `nt` into an apply.  `invoke` must
branch on `kind`, rather than treating construction as `apply` with a
truthy-looking extra argument.  This rule applies uniformly to the generated
TS interpreter, Tier 0/Tier 1/Tier 2 JIT output, the WASM backend, tenant
helpers, callable exotics, bridge callables, WSDOM request descriptors, and
host/embedder adapters.

For an ordinary tenant-owned function, `apply` uses the existing guest ABI
adapter: registered `leading-tenant-nt` functions receive
`(tenant, undefined, ...args)` and plain host functions receive their ordinary
arguments.  `construct` must use `Reflect.construct`, not `Reflect.apply`, so
the native function shell receives JavaScript construction semantics (`this`,
prototype selection, and `new.target`); if it is a registered Jade function,
its virtual body receives `(tenant, newTarget, ...args)`.  The construction
result is returned with normal JavaScript constructor replacement semantics.

A callable exotic must define both operations deliberately. A missing `apply` or
`construct` fails closed with `TypeError`; construction cannot be silently
emulated by applying the handler. A non-constructible native function likewise
throws `TypeError`, matching native JavaScript behavior. Current Jade bytecode
has `CALL` but no explicit construction opcode; nonetheless the tenant/embedder ABI must support both now, and a later construction opcode
must use this exact contract.

### 3. Creation paths

`makeExotic`, `makeFunction`, and `makeCallableExotic` have two equally
supported entry points:

1. **Inside a provider:** provider methods can call
   `yield this.yieldTenant(this.makeExotic(...))`,
   `yield this.yieldTenant(this.makeFunction(...))`, or
   `yield this.yieldTenant(this.makeCallableExotic(...))` when they need to
   expose a facade (for example, a bridge or a provider-specific virtual
   object).
2. **By an embedder/caller:** host code calls
   `tenant.driveTenant(tenant.makeExotic(proto, handler), addAsync, addGen)`,
   `tenant.makeFunction(...)`, or their callable-exotic counterpart.  These are
   the supported ways to create tenant-owned values outside a tenant and are
   subject to the same ambient variant rules as `make`.

If “caller” is also intended to include Jade source/bytecode: it is **not** in
this plan.  Jade has no exotic-construction opcode or intrinsic, and only host
embedders/providers may supply trusted handlers through the APIs above.  Future
work may add a deliberate `Proxy` capability and its front-end lowering after
its capability/security model is designed; it must not expose a raw handler
object to current guest code merely because JavaScript has `Proxy`.

### 3. Provider registration and marshalling

Keep routing facilities distinct from guest property operations.  A candidate
internal interface is:

```ts
interface TenantProvider extends Tenant {
  /**
   * Pure, non-trapping and independent ownership predicate. A provider keeps
   * this record whether or not it currently belongs to any MergedTenant.
   */
  ownsObject(value: object): boolean;
}

interface TenantMarshaller {
  marshal(value: unknown, from: TenantProvider, to: TenantProvider):
    TenantGenerator<unknown>;
}
```

`ownsObject` is the required trusted registration/ownership mechanism.  Every
provider records values it allocates or imports from a trusted transport in its
own independent ownership store; a router consults only that pure predicate for
its enrolled providers.  Objects do **not** need to be allocated through a
`MergedTenant`: a secondary tenant can be operated independently, create its
needed values, and then be added to one or more routers.  A `MergedTenant` can
also be an enrolled provider in an outer router.  The router must still reject
unowned values rather than probe tenant operations, and must detect ambiguous
claims from two enrolled providers as a configuration/ownership error.

`marshal` has the following rules:

- Primitives pass unchanged.
- `null` and `undefined` pass unchanged.
- An object owned by the destination passes unchanged.
- A source bridge returning to its original source unwraps to its canonical
  source object.
- Any other object returns the cached or newly-created bridge exotic for
  `(canonical source value, source provider, destination provider)`.  If the
  source is callable, the bridge is made through `makeCallableExotic` and is a
  native function in the destination too.
- The bridge handler converts its arguments destination-to-source before
  forwarding; it converts results source-to-destination before returning.
  This applies recursively to `get`, `set`, `define`, `assign`, property-trap
  results, and `invoke` arguments/results—not just the top-level object passed
  to a method.
- Unowned objects, revoked/released remote values, and incompatible capability
  domains fail closed with a descriptive error.  Do not reject a value merely
  because it is callable: callable ownership is resolved and marshalled by the
  same canonical-source rules as object ownership.

Use weak, direction-aware caches so identity is stable without retaining an
object graph forever.  A cache key must include both provider identities and
canonical source identity; keying only by raw object is insufficient when three
or more providers are present.

### 4. `MergedTenant`

Expose one router with an API conceptually like:

```ts
new MergedTenant({ primary, providers: [primary, server, ...] })
```

It implements the complete `Tenant` interface and receives the VM/JIT’s
`tenant` parameter.  Its behavior is:

- `make`, `makeFunction`, and caller-created `makeExotic`/`makeCallableExotic`
  allocate in the primary provider unless an explicit provider-selection API is
  used. `makeFunction` preserves the adopted implementation's native
  constructibility; `makeCallableExotic` creates a constructible forwarding
  shell, whose missing construct handler fails closed with `TypeError`.
- Every object method resolves the receiver’s owner from router metadata and
  delegates only there.
- Inputs such as `value` in `set`, `source` in `assign`, and descriptors in
  `define` are marshalled to the receiver owner before delegation.
- Results such as `get` values and created objects are marshalled from the
  receiver owner to the active caller/provider context.  During ordinary
  bytecode execution, this context is unambiguously the router's **primary**
  provider: it is the provider used by bytecode-level object creation.  An
  explicit host/provider API is required to select a different destination
  context; no operation may silently treat a secondary provider as primary.
- `define` must marshal the descriptor object and all descriptor fields; a
  shallow wrapper is not enough because getters/setters and descriptor values
  can themselves cross a provider boundary.
- `ownKeys`, `has`, and `delete` operate only on the receiver owner and never
  fan out.
- `invoke` resolves the callee owner, marshals its invocation arguments to that
  owner, delegates to the owner’s `invoke`, and marshals the result back to the
  active context.  The invocation kind and its `newTarget` are preserved
  verbatim—routing must never turn `construct` into `apply` or forward an
  outer `nt` through an apply.  `invoke` is the sole implementation path for
  normal native functions, registered Jade functions, and callable exotics.
- Router methods are generator methods.  They delegate by yielding the selected
  provider generator into the router driver; they never invoke a participant’s
  `.next()` or use a second, nested `driveTenant` boundary.

The phrase “multiple tenants handle objects simultaneously” therefore means
that their independently owned objects coexist in one guest execution.  It does
not mean that an object’s operation is broadcast to multiple tenant shadows.

## Bridge behavior in detail

For a bridge representing `sourceObject` from provider `A` inside provider `B`:

| Operation on the B bridge | Required behavior |
| --- | --- |
| `get(bridge, key)` | Call `A.get(sourceObject, marshal(B→A, key))`; marshal the result `A→B`. |
| `set(bridge, key, value)` | Marshal key/value `B→A`, call `A.set`, return Jade’s usual `void`. |
| `has` / `delete` | Marshal the key, delegate to `A`, preserve the normal return shape. |
| `ownKeys` | Delegate to `A.ownKeys`. WSDOM has no symbol transport in this plan: any symbol key/result at a WSDOM boundary is represented as an opaque, provider-owned object capability, never serialized by description. It must round-trip by identity within its capability domain or fail closed; raw symbols are not accepted on the wire. |
| `define` | Marshal the complete descriptor-map object `B→A`, then delegate to `A.define`. |
| `assign` | Marshal the source `B→A`, then delegate to `A.assign`. |
| `invoke(bridge, invocation)` | Marshal apply arguments and `thisArg` `B→A`; for construct marshal both arguments and `newTarget` `B→A` (so `new bridge(...)` maps the bridge new target back to `sourceCallable`); preserve `kind`; call `A.invoke(sourceCallable, marshalledInvocation)`; then marshal the result `A→B`. The destination bridge remains a native function and never exposes its forwarding state as ordinary properties. |

The bridge must be created by (and registered as an object or callable of) `B`;
therefore `B` can recognize it and use its normal object-management and call
path.  A callable bridge is a native forwarding function whose ordinary call body
creates `{ kind: "apply", thisArg, args }`, and whose construction body creates
`{ kind: "construct", newTarget, args }`; each enters `B.invoke`/the bridge
handler.  It is **not** a direct closure that calls `A` without tenant ABI
handling, nor may it pass its enclosing `new.target` to an ordinary apply.  Its handler closes over only trusted
router/provider metadata, never guest-controlled source properties.  Bridge
trap and call code must use the same generator driver as ordinary tenant
methods, so server promises propagate through the caller’s existing
async/generator shape.

The initial implementation may define `proto` as opaque metadata because Jade
currently has no tenant `getPrototypeOf`/`setPrototypeOf` operation.  If a later
opcode exposes prototype behavior, add corresponding handler methods and
bridge forwarding at that time; do not rely on native shell prototypes as an
untracked side channel.

## WSDOM/server integration

`packages/jade-js/wsdom-tenant-runtime.ts` currently creates private local shims
for `ServerDescriptor` values and forwards all tenant operations through an
async callback.  Merging changes the boundary as follows:

1. Keep the server descriptor capability and lifecycle protocol. A server
descriptor remains an opaque server-owned identity, never a plain object copied
into `MultiTenant`. On the TypeScript side, use `FinalizationRegistry` to issue
best-effort release of destination bridge/descriptor capabilities; explicit
release remains available where deterministic disposal is required. On the
Rust side, scope ownership and release to the borrow checker: no JS-facing
remote reference may outlive the Rust borrow/owner that authorizes it, and
borrowed descriptors must never be retained as `'static` bridge state.
2. Install the server facade as a `TenantProvider` participant and make its
   imported shims explicitly registerable with the router.
3. Replace the current “browser values stay real values” shortcut at a
   cross-provider request boundary with router marshalling.  Browser/local
   objects become server-owned bridge exotics/descriptors; browser/local
   functions become server-owned callable bridge descriptors; and server
   objects or callables delivered to local code become local exotic facades.
   The local callable facade is a native function that re-enters the merged
   tenant `invoke` path, so it gets the same tenant ABI as a local function.
4. Include provider identity and the connection capability in every transport
   descriptor, including callable invocation descriptors and a construct
   invocation's `newTarget`.  Marshal `newTarget` through the same canonical
   bridge cache as any other callable so a server sees the source-side target,
   not a browser bridge shell. Symbols are not serialized: represent them as
   opaque object capabilities in the same provider/capability domain, or reject
   a raw symbol that has no such capability. Reject descriptors belonging to a
   different connection or merged-router instance; an object ID without its
   capability domain is not a valid identity.
5. Define release ownership precisely: a TypeScript `FinalizationRegistry`
   finalizer only releases a destination-side remote reference on a
   best-effort basis; it must not invalidate a still-live canonical source
   object or another destination’s bridge. Rust-side disposal follows the
   owning borrow/lifetime, with explicit release where the protocol needs a
   deterministic boundary.
6. Preserve current error settlement exactly once per WSDOM request.  Errors
   thrown by bridge forwarding reject the same request/promise and must not
   leave an unobserved generator or callback outstanding.

A server participant necessarily introduces async work.  Calls through a
merged router must fail with the current clear “requires `addAsync`” behavior
when executed synchronously, and succeed without a special merge-only calling
convention when `addAsync` is present.  Generator and async-generator callers
must retain the existing driver behavior as well.

## Backend and JIT work

### Tenant API consumers

Once `makeExotic`, `makeFunction`, `makeCallableExotic`, or `invoke` is
bytecode-reachable, update the entire semantic surface together:

- `packages/jade-js/index.ts` types and all tenant implementations;
- `packages/jade-data/index.ts` handlers plus `scripts/regen.ts` and generated
  `packages/jade-js/vm.ts`;
- `crates/jade-vm-core` operation dispatch, if a new opcode is selected;
- `crates/jade-vm-jit` (`JadeTenantMethod`, host-name validation, property,
  `CALL`, and future construct emission, plus Tier 0/1 nested compilation).
  Its current `op_call` emits a direct `Reflect.apply`; replace that with a
  driven `tenant.invoke(callee, { kind: "apply", thisArg: undefined, args })`
  expression.  It must **not** pass the surrounding `nt` through that apply.
  A future construct emission instead passes `{ kind: "construct", args,
  newTarget }`; all three JIT tiers must preserve that discriminant and bind
  `NEW_TARGET` to the invoked function's `nt`, not an outer wrapper's value;
- `crates/jade-vm-jit-swc`, which reuses the Tier 0 per-operation emission and
  therefore inherits the `invoke` change, but must receive declared and
  ambient variant bits correctly for nested bodies;
- `crates/jade-vm-wasm`, where the direct registry fast path and fallback
  `Reflect::apply` must be made tenant-aware (or proven equivalent) and then
  composed through `driveTenant`, never direct `.next()`.  Its apply path must
  supply `nt = undefined`; its eventual `Reflect::construct` path must supply
  the actual `newTarget`;
- WSDOM fixtures and runtime setup.

Even if exotic creation remains embedder-only in the first milestone,
`invoke` is not optional: all backend `CALL` paths must be routed through it
with the explicit `apply` invocation form to recognize native callable shells
and avoid leaking an outer `new.target`.  A future construction operation must
use the explicit `construct` form.  Document that boundary and implement the
new methods uniformly in every concrete `Tenant` type.

### Inlining and host names

- Add a JIT host-name enum entry only for a genuinely emitted Jade operation.
  Do not expand `TENANT_METHOD_NAMES` with router bookkeeping or ABI/driver
  helpers.
- `MergedTenant` and bridge methods should initially use normal member calls.
  Their dispatch depends on private WeakMaps, dynamic provider metadata, and
  closures, so the tenant-inlining scanner will correctly reject many of them.
  That is an optimization fallback, not a correctness issue.
- If any new real operation is considered for source inlining, extend the Rust
  frontend extractor, JIT arity checks, all three JIT tiers, and tests in one
  change.  Preserve the hard rejection of `#private` references and captured
  free callees.

### Callable exotics and normal tenant functions

`invoke` is part of the first implementation, not a later optional extension.
It must preserve normal behavior as well as add bridge behavior:

1. If `callee` is a callable exotic owned by this provider, dispatch its
   `apply` or `construct` handler according to `invocation.kind`.
2. Otherwise use an invocation-aware `invokeGuestAware`.  For `apply`, a
   registered `leading-tenant-nt` guest function receives
   `(tenant, undefined, ...args)`; for `construct`, it receives
   `(tenant, newTarget, ...args)` through a native `Reflect.construct` path.
   Native/plain functions receive their ordinary apply or construction
   semantics.  Extend the helper or add a strictly equivalent tenant-owned ABI
   helper—do not leave call sites to guess or carry `nt` separately.
3. Compose the result through the current tenant driver exactly once.  Do not
   pre-drive it inside `invoke`, then drive it again at the VM/JIT/WASM call
   site.

The standard providers must expand their representation accordingly.  Today
`MultiTenant.make` always returns `Object.create(null)`, and `single_tenant`
assumes an object shell.  Introduce shared shell allocation/metadata so both
can allocate either a null-prototype object shell or a real native function
shell while their descriptors remain in the same tenant shadow/native mapping.
WSDOM descriptor/shim representations need the same callable bit and a native
function shim on the browser side.  This keeps the object model consistent:
functions can have tenant-owned own properties and can cross providers without
being flattened into raw host functions.

A function created by a provider must be registered with `markGuestFn` when it
implements Jade's `leading-tenant-nt` ABI.  A forwarding native bridge is not
itself an arbitrary guest function: it must enter `tenant.invoke` with its
stored canonical source and an explicit apply or construct invocation.  Do not
independently misregister it in a way that causes the JIT to prepend
`tenant`/`nt` twice, and do not accidentally capture an outer constructor's
`new.target` for ordinary calls.

## Implementation phases

### Phase 0 — settle contracts and create focused fixtures

- Record the fixed fail-closed missing-trap behavior and document the required
  `TypeError` shape for absent property, apply, and construct traps.
- Confirm exotic construction is host/embedder-facing only; record the future
  `Proxy` capability as separate work, not as a bytecode opcode in this plan.
- Define the exact `TenantExoticHandler`, callable-handler, and
  `TenantInvocation` signatures, including receiver, `thisArg`, `newTarget`,
  descriptor-map handling, errors, prototype metadata, and whether the
  provider or handler owns normal-call ABI adaptation.
- Cement and test the `nt` rule: `apply` always supplies `undefined`,
  `construct` supplies exactly its `newTarget`, and `NEW_TARGET` observes that
  same value.  No call path may inherit an outer `nt`.
- Specify the `invokeGuestAware` migration so existing guest calls preserve
  their leading `(tenant, nt, ...args)` ABI with the invocation-derived `nt`.
- Adopt `ownsObject` as the independent, non-trapping provider ownership
  predicate; test a secondary provider creating values before it joins a
  router, one provider participating in multiple routers, nested routers, and
  conflicting ownership claims.
- Add small test-only providers that record every call and can produce sync,
  promise, generator, and async-generator trap results.  These make routing
  behavior testable without WSDOM.

**Exit criteria:** agreed API document and tests that would fail under
broadcasting, raw-object leakage, or an accidental direct `.next()`.

### Phase 1 — implement exotic objects in one local provider

- Add the public types, `makeExotic`, `makeFunction`, `makeCallableExotic`,
  and `invoke` to `Tenant`.
- Implement a shared exotic-storage helper or a `MultiTenant` implementation
  backed by weak metadata.  Keep exotic metadata separate from ordinary
  descriptor storage and invisible to guest `ownKeys`; expand its shell creator
  to produce native function shells for callable values, and migrate all
  existing guest-visible function creation to `makeFunction`/the shared
  registrar.
- Implement all current tenant operations and both invocation forms against the
  handler.  Missing traps must fail closed with `TypeError`; providers that
  want ordinary behavior must implement it explicitly.  Preserve
  `invokeTrap`/`invokeGuestAware` and nested generator composition.
- Add the same methods and function-shell representation to `single_tenant`,
  so the public interface remains complete.  Avoid a native `Proxy` shortcut
  that would cause VM and host semantics to diverge.
- Verify host callers use `driveTenant(tenant.makeExotic(...), flags)` or the
  callable form, and that provider-internal creation works through
  `yieldTenant`.

**Exit criteria:** local sync/async/generator tests prove each Jade operation
and call routes through a handler; native callable shells receive the expected
tenant ABI and retain tenant-owned own properties; ordinary objects and normal
functions retain their existing behavior; and an exotic’s metadata cannot be
read or overwritten through tenant properties.

### Phase 2 — implement router ownership and local merging

- Introduce `MergedTenant`, provider-local `ownsObject` records, router-local
  direction-aware weak bridge caches, and ambiguity detection.  Do not make a
  router-owned registrar the source of provider ownership.
- Make bytecode allocation use the primary context; permit independently
  created secondary values only after their provider is enrolled and claims
  them through `ownsObject`. Test one provider in multiple routers and a
  nested `MergedTenant` as a provider; reject unowned or multiply-owned foreign
  objects rather than guessing.
- Implement bridge exotics and callable bridge exotics plus recursive
  marshalling for every operation/call input and output, including descriptor
  maps.
- Route `CALL` through `MergedTenant.invoke` as an explicit apply; verify one
  source invocation, `nt === undefined` even when the caller was constructed,
  correct tenant ABI forwarding, and exactly one driver composition.
- Exercise the router's construct form through the embedder-level API before a
  bytecode construct opcode exists; verify it preserves an explicit `newTarget`
  across A→B bridges and reaches `NEW_TARGET` in a registered Jade callee.
- Ensure all selected provider generators run under the merged driver, with
  `this` receivers preserved for participant methods and ABI helpers.
- Add tests using two local instrumented providers: primary dispatch, secondary
  dispatch, no broadcast, round-trip unwrapping, A→B→A identity, A→B→C
  canonicalization, cyclic values, native callable bridge identity, and
  tenant-owned function property behavior.

**Exit criteria:** objects and functions owned by two providers can participate
in one Jade execution; every cross-provider object is an exotic facade and
every cross-provider callable is a native callable exotic; neither provider
observes the other’s raw value.

### Phase 3 — integrate the WSDOM server provider

- Extend wire descriptors and the browser runtime with provider/capability
  identity and router registration hooks.
- Marshal request arguments and resolution values through the router rather
  than retaining the current raw browser-value pass-through across a provider
  boundary.
- Wire bridge release/finalization to the existing `release` callback: use
  `FinalizationRegistry` for best-effort TypeScript disposal and Rust
  borrow-checked ownership for Rust-side lifetime/release, without releasing
  canonical source identities prematurely.
- Add end-to-end browser/WSDOM tests with local primary + server secondary in
  both directions, including server-owned nested values returned from a local
  facade, local-owned values stored through a server facade, ordinary apply
  from a constructed caller (`nt` remains `undefined`), and construction with
  a marshalled cross-provider `newTarget`.

**Exit criteria:** no raw browser object is treated as a server object (or vice
versa); capability mismatches are rejected; release and errors are deterministic;
server-bound operations obey ambient async requirements.

### Phase 4 — performance and future bytecode/`Proxy` work

- Keep exotic construction host-facing in this plan: do **not** add a Jade
  exotic-allocation opcode or intrinsic.  A later `Proxy` design may introduce
  one with a separately reviewed capability model.
- Add host-name/JIT support for `invoke` and Tier 0, Tier 1, Tier 2 coverage
  in lockstep when the call-boundary change is emitted.
- When Jade eventually gains a construction opcode, route it through the
  existing `TenantInvocation { kind: "construct", newTarget, args }` contract
  in every backend; add `NEW_TARGET` tests rather than creating a second ABI.
- Profile router, bridge, and normal-call overhead.  Optimize lookup/cache
  paths only after measuring, and retain the non-inlined normal-call fallback.
- Do not add an ad-hoc construct API: the constructible ABI above is already
  authoritative.

## Test matrix

At minimum, add coverage for:

- Each handler operation plus both invocation forms: fail-closed missing traps,
  thrown traps, guest-function traps, normal native-function calls,
  `Reflect.construct` behavior, `NEW_TARGET`, and
  sync/async/generator/async-generator results. Include an apply made from
  within a constructed callee to prove its `nt` is still `undefined`.
- Creation of ordinary, native-function, and callable exotics from an embedder
  and from inside a provider method; callable own properties must be invisible
  to raw shell property inspection but reachable through tenant operations.
  Cover every migrated function source (`FN`, generator helpers, and WSDOM
  shims) so none bypasses ownership registration.
- Two local providers: receiver/callee-owned routing, no broadcast, property
  changes visible only through the correct owner, and a primary object plus a
  secondary object/function used in one VM program. Include independently
  pre-created secondary values, one provider enrolled in multiple routers,
  nested `MergedTenant`s, and rejection of ambiguous `ownsObject` claims.
- Cross-provider `get`, `set`, `has`, `delete`, `ownKeys`, `define`, `assign`,
  and both `invoke` forms, including a descriptor containing a cross-provider
  callable.  Assert that apply and construct preserve their distinct `nt`
  values over an A→B bridge.
- Bridge identity cache behavior, native function identity/callability,
  back-edge unwrapping, cyclic object graphs, unowned input rejection, and
  provider/capability mismatch rejection.
- Existing `MultiTenant` isolation: shells remain empty to host raw property
  inspection, and new exotic metadata is likewise invisible.
- Driver and callable-ABI invariants: no tenant operation is manually stepped;
  no call bypasses `tenant.invoke`; sync rejection of async server work; correct
  promise/generator composition under all four ambient flag combinations; no
  double prepending of `tenant`/`nt` or double driving of a call result; apply
  always delivers `nt === undefined`; and construct delivers precisely its
  `newTarget`.
- Type tests for the extended public `Tenant` interface, callable/exotic
  handler types, and marshalling helper types, while retaining the
  `HostToGuest`/`GuestToHost` no-double-wrap properties.
- JIT structural tests and Tier-2 end-to-end tests if a new bytecode operation
  is emitted; nested functions must continue to compile through the active
  tier, not silently fall back to Tier 0.
- WSDOM callback settlement, capability isolation, opaque-symbol capability
  round trips/rejection of raw wire symbols, remote release, TypeScript
  `FinalizationRegistry` best-effort cleanup, Rust borrow-checked lifetime
  enforcement, and error propagation tests.

## Documentation and migration checklist

- Update `packages/jade-js/index.ts` API comments, `narrow.ts` ABI helpers,
  `rewrite.ts` call conversions, the generated interpreter's `FN` and `CALL`
  paths, JIT/WASM function registries, and `docs/tenant-generator-driver.md`
  for `makeFunction`, `TenantInvocation`, `invoke`, `new.target`, and
  callable-exotic driver behavior.
- Update `goals.md` to describe exclusive provider ownership, independently
  owned provider values, primary bytecode-allocation context, exotic facades,
  and merged/nested routing once implemented.
- Update `docs/pluggable-tenant-interface-plan.md` and JIT documentation if a
  JIT-visible operation or inlining decision changes.
- Preserve the invariants in `AGENTS.md`: generator composition, ABI helpers as
  `this` methods, no driver-helper inlining, and active-tier nested compilation.
- Do not change `packages/jade-data/index.ts` without running
  `scripts/regen.ts` and reviewing the generated `packages/jade-js/vm.ts` diff.

## Settled design decisions

- Exotic construction is host/embedder/provider-facing only.  Jade source gets
  no exotic constructor, intrinsic, or raw handler object in this plan; a
  future `Proxy` capability is separate work.
- Missing exotic traps fail closed with `TypeError`; no implicit fallback target
  exists.
- `ownsObject` is the independent, pure ownership predicate. Providers create
  and operate their own values before or after enrollment; a provider may join
  multiple `MergedTenant`s, and merged routers may be nested.
- Bytecode-level object creation uses the router primary provider as its active
  context. Other destination contexts require an explicit host/provider API.
- WSDOM does not transport raw symbols. It either carries an opaque
  provider-owned object capability with domain-scoped identity or rejects the
  value.
- TypeScript bridge lifecycle uses `FinalizationRegistry` for best-effort
  release (plus explicit release where required); Rust-side lifetime/release is
  governed by the borrow checker and cannot outlive the authorizing owner.

## Remaining open decisions

1. Should `invokeGuestAware` accept a `TenantInvocation` directly, or should a
   new tenant-owned ABI helper do so while preserving its existing trap-facing
   API?
2. What exact shared shell/registrar API allocates native callable
   representations in `MultiTenant`, `single_tenant`, JIT/WASM `FN` paths,
   generator helpers, and the WSDOM facade without exposing native own
   properties to guest operations?
3. For an explicit embedder construction API, how is a non-default
   `newTarget` validated, registered, and marshalled across providers before a
   Jade construction opcode exists?
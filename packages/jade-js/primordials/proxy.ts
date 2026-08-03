import type {
  Tenant, TenantGenerator, TenantPropertyDescriptor,
} from "../tenants/types.ts";
import { assertObject, defineData, descriptorObject, guestArrayLike, makeBuiltin, readGuestDescriptor } from "./types.ts";
import { functionPrimordial } from "./function.ts";

export interface ProxyPrimordial { Proxy: Function; }

/**
 * `proxyExotic`'s shared revocable state — `#target`/`#handler`/`#revoked` are private (only ever
 * touched by this class's own methods) so a Rust port can translate mutability directly: every
 * field here is genuinely mutable (`revoke()` flips `#revoked`), and the `Rc<RefCell<Inner>>`
 * translation every class gets handles that uniformly with no separate mutation analysis needed.
 * `#revoked` alone gates all access (`requireLive()`); `#target`/`#handler` stay set after
 * `revoke()` rather than also being cleared (the original design's minor GC hint, dropped here —
 * both are already kept alive for as long as `revoke`/the proxy itself are reachable regardless,
 * via this same state object, so nothing observable changes) — that in turn means neither field
 * needs `Option`-typed storage, sidestepping a truthiness check on an `Option`-typed *class field*
 * class lowering doesn't yet narrow the way it already does for a plain local variable. Exposed
 * only through `target()`/`handler()` (both re-validate via `requireLive()`), never as raw fields
 * — this also means nothing needs a tuple return type purely for convenience, since no real call
 * site in this file ever needs both values at once. See
 * `docs/proxy-and-buffer-primordial-gap-plan.md`.
 */
export interface ProxyState {
  requireLive(): void;
  target(): object;
  handler(): object;
  revoke(): void;
}

class ProxyStateImpl implements ProxyState {
  #target: object;
  #handler: object;
  #revoked: boolean = false;

  constructor(target: object, handler: object) {
    this.#target = target;
    this.#handler = handler;
  }

  requireLive(): void {
    if (this.#revoked) throw new TypeError("Cannot perform operation on a revoked Proxy");
  }
  target(): object {
    this.requireLive();
    return this.#target;
  }
  handler(): object {
    this.requireLive();
    return this.#handler;
  }
  revoke(): void {
    this.#revoked = true;
  }
}

/**
 * `found: false` vs `found: true` distinguishes "no guest trap installed at all" from "the trap
 * ran and returned the guest `undefined` value" — a real distinction that must survive even
 * though `value` itself can legitimately *be* that same guest `undefined`, so it can't collapse
 * to a plain `value | undefined` alias. A Rust port recognizes this exact two-variant shape
 * (`found: false` alone vs `found: true` plus one more field) and resolves it directly to
 * `Option<T::Value>` — `None` for `{ found: false }`, `Some(value)` for `{ found: true, value }`
 * (`Some` of the guest `undefined` value is still distinct from `None` at the Rust level, exactly
 * preserving this distinction) — not a hand-rolled enum. See
 * `docs/proxy-and-buffer-primordial-gap-plan.md`.
 */
type TrapResult = { found: false } | { found: true; value: unknown };
const cache = new WeakMap<Tenant, ProxyPrimordial>();

/**
 * A real top-level function, not a closure local to `proxyExotic` — closure lowering targets the
 * fixed `makeBuiltin` apply/construct convention (`thisArg`/`args`, returning a guest value)
 * specifically, which this doesn't match (its own first parameter is a trap name, and it returns
 * `TrapResult`, not `T::Value`); `tenant`/`state` become explicit parameters instead of captured
 * closure state, the same adjustment `array-buffer.ts`'s `shell`/`record` made.
 */
function* trap(tenant: Tenant, state: ProxyState, name: string, args: readonly unknown[]): TenantGenerator<TrapResult> {
  const guestHandler = state.handler();
  const value = yield tenant.yieldTenant(tenant.get(guestHandler, name));
  if (value === undefined) return { found: false };
  if (typeof value !== "function") throw new TypeError(`Proxy trap ${name} is not callable`);
  return { found: true, value: yield tenant.yieldTenant(tenant.invoke(value, {
    kind: "apply", thisArg: guestHandler, args,
  })) };
}

function* proxyExotic(
  tenant: Tenant,
  target: object,
  handler: object,
  existingState: ProxyState | undefined,
): TenantGenerator<object> {
  const state: ProxyState = existingState ?? new ProxyStateImpl(target, handler);
  return yield tenant.yieldTenant(tenant.makeExotic(null, {
    *get(receiver, key) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "get", [state.target(), key, receiver]));
      return result.found ? result.value : yield tenant.yieldTenant(tenant.get(state.target(), key));
    },
    *set(receiver, key, value) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "set", [state.target(), key, value, receiver]));
      if (!result.found) yield tenant.yieldTenant(tenant.set(state.target(), key, value));
      else if (!result.value) throw new TypeError("Proxy set trap returned false");
    },
    *has(_receiver, key) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "has", [state.target(), key]));
      return !result.found ? yield tenant.yieldTenant(tenant.has(state.target(), key)) : !!result.value;
    },
    *delete(_receiver, key) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "deleteProperty", [state.target(), key]));
      if (!result.found) yield tenant.yieldTenant(tenant.delete(state.target(), key));
      else if (!result.value) throw new TypeError("Proxy deleteProperty trap returned false");
    },
    *ownKeys() {
      const result = yield tenant.yieldTenant(trap(tenant, state, "ownKeys", [state.target()]));
      if (!result.found) return yield tenant.yieldTenant(tenant.ownKeys(state.target()));
      assertObject(result.value, "Proxy ownKeys trap must return an object");
      const keys = yield tenant.yieldTenant(guestArrayLike(tenant, result.value));
      return keys as PropertyKey[];
    },
    *ownPropertyKeys() {
      const result = yield tenant.yieldTenant(trap(tenant, state, "ownKeys", [state.target()]));
      if (!result.found) return yield tenant.yieldTenant(tenant.ownPropertyKeys(state.target()));
      assertObject(result.value, "Proxy ownKeys trap must return an object");
      const keys = yield tenant.yieldTenant(guestArrayLike(tenant, result.value));
      return keys as PropertyKey[];
    },
    *getOwnPropertyDescriptor(_receiver, key) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "getOwnPropertyDescriptor", [state.target(), key]));
      if (!result.found) return yield tenant.yieldTenant(tenant.getOwnPropertyDescriptor(state.target(), key));
      if (result.value === undefined) return undefined;
      assertObject(result.value, "Proxy descriptor trap must return an object");
      return yield tenant.yieldTenant(readGuestDescriptor(tenant, result.value));
    },
    *defineProperty(_receiver, key, descriptor) {
      const descriptorArg = yield tenant.yieldTenant(descriptorObject(tenant, descriptor));
      const result = yield tenant.yieldTenant(trap(tenant, state, "defineProperty", [state.target(), key, descriptorArg]));
      return !result.found
        ? yield tenant.yieldTenant(tenant.defineProperty(state.target(), key, descriptor))
        : !!result.value;
    },
    *getPrototypeOf() {
      const result = yield tenant.yieldTenant(trap(tenant, state, "getPrototypeOf", [state.target()]));
      if (!result.found) return yield tenant.yieldTenant(tenant.getPrototypeOf(state.target()));
      if (result.value !== null) assertObject(result.value, "Proxy getPrototypeOf trap must return object or null");
      return result.value as object | null;
    },
    *setPrototypeOf(_receiver, prototype) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "setPrototypeOf", [state.target(), prototype]));
      return !result.found ? yield tenant.yieldTenant(tenant.setPrototypeOf(state.target(), prototype)) : !!result.value;
    },
    *isExtensible() {
      const result = yield tenant.yieldTenant(trap(tenant, state, "isExtensible", [state.target()]));
      return !result.found ? yield tenant.yieldTenant(tenant.isExtensible(state.target())) : !!result.value;
    },
    *preventExtensions() {
      const result = yield tenant.yieldTenant(trap(tenant, state, "preventExtensions", [state.target()]));
      return !result.found ? yield tenant.yieldTenant(tenant.preventExtensions(state.target())) : !!result.value;
    },
    *define(_receiver, descriptors) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "defineProperties", [state.target(), descriptors]));
      if (!result.found) yield tenant.yieldTenant(tenant.define(state.target(), descriptors));
    },
    *assign(_receiver, source) {
      const result = yield tenant.yieldTenant(trap(tenant, state, "assign", [state.target(), source]));
      if (!result.found) yield tenant.yieldTenant(tenant.assign(state.target(), source));
    },
  }));
}

export function* proxyPrimordial(tenant: Tenant): TenantGenerator<ProxyPrimordial> {
  const existing = cache.get(tenant);
  if (existing) return existing;
  const { FunctionPrototype } = yield tenant.yieldTenant(functionPrimordial(tenant));
  const ProxyFn = yield tenant.yieldTenant(makeBuiltin(
    tenant, "Proxy",
    function* () { throw new TypeError("Constructor Proxy requires 'new'"); },
    function* (_newTarget, args) {
      assertObject(args[0], "Proxy target must be an object");
      assertObject(args[1], "Proxy handler must be an object");
      return yield tenant.yieldTenant(proxyExotic(tenant, args[0], args[1], undefined));
    }, FunctionPrototype,
  ));
  const result = { Proxy: ProxyFn };
  cache.set(tenant, result);
  const revocable = yield tenant.yieldTenant(makeBuiltin(tenant, "revocable", function* (_this, args) {
    assertObject(args[0], "Proxy target must be an object"); assertObject(args[1], "Proxy handler must be an object");
    // Separate state is represented by a forwarding exotic so revoke can clear target/handler.
    const state = new ProxyStateImpl(args[0], args[1]);
    const proxy = yield tenant.yieldTenant(proxyExotic(tenant, args[0], args[1], state));
    const revoke = yield tenant.yieldTenant(makeBuiltin(tenant, "revoke", function* () {
      state.revoke();
    }, undefined, FunctionPrototype));
    const record = yield tenant.yieldTenant(tenant.make(null));
    yield tenant.yieldTenant(defineData(tenant, record, "proxy", proxy));
    yield tenant.yieldTenant(defineData(tenant, record, "revoke", revoke));
    return record;
  }, undefined, FunctionPrototype));
  yield tenant.yieldTenant(defineData(tenant, ProxyFn, "revocable", revocable));
  return result;
}

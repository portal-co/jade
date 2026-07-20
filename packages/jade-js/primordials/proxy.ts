import type {
  Tenant, TenantExoticHandler, TenantGenerator, TenantPropertyDescriptor,
} from "../tenants/types.ts";
import { assertObject, defineData, descriptorObject, guestArrayLike, makeBuiltin, readGuestDescriptor } from "./types.ts";
import { functionPrimordial } from "./function.ts";

export interface ProxyPrimordial { Proxy: Function; }
type ProxyState = { target?: object; handler?: object; revoked: boolean };
const cache = new WeakMap<Tenant, ProxyPrimordial>();

function* proxyExotic(
  tenant: Tenant,
  target: object,
  handler: object,
  existingState?: ProxyState,
): TenantGenerator<object> {
  const state: ProxyState = existingState ?? { target, handler, revoked: false };
  const requireLive = (): [object, object] => {
    if (state.revoked || !state.target || !state.handler) throw new TypeError("Cannot perform operation on a revoked Proxy");
    return [state.target, state.handler];
  };
  type TrapResult = { found: false } | { found: true; value: unknown };
  const trap = function* (name: string, args: readonly unknown[]): TenantGenerator<TrapResult> {
    const [, guestHandler] = requireLive();
    const value = yield tenant.yieldTenant(tenant.get(guestHandler, name));
    if (value === undefined) return { found: false };
    if (typeof value !== "function") throw new TypeError(`Proxy trap ${name} is not callable`);
    return { found: true, value: yield tenant.yieldTenant(tenant.invoke(value, {
      kind: "apply", thisArg: guestHandler, args,
    })) };
  };
  const fallback = () => requireLive()[0];
  const native: TenantExoticHandler = {
    *get(receiver, key) {
      const result = yield tenant.yieldTenant(trap("get", [fallback(), key, receiver]));
      return result.found ? result.value : yield tenant.yieldTenant(tenant.get(fallback(), key));
    },
    *set(receiver, key, value) {
      const result = yield tenant.yieldTenant(trap("set", [fallback(), key, value, receiver]));
      if (!result.found) yield tenant.yieldTenant(tenant.set(fallback(), key, value));
      else if (!result.value) throw new TypeError("Proxy set trap returned false");
    },
    *has(_receiver, key) {
      const result = yield tenant.yieldTenant(trap("has", [fallback(), key]));
      return !result.found ? yield tenant.yieldTenant(tenant.has(fallback(), key)) : !!result.value;
    },
    *delete(_receiver, key) {
      const result = yield tenant.yieldTenant(trap("deleteProperty", [fallback(), key]));
      if (!result.found) yield tenant.yieldTenant(tenant.delete(fallback(), key));
      else if (!result.value) throw new TypeError("Proxy deleteProperty trap returned false");
    },
    *ownKeys() {
      const result = yield tenant.yieldTenant(trap("ownKeys", [fallback()]));
      if (!result.found) return yield tenant.yieldTenant(tenant.ownKeys(fallback()));
      assertObject(result.value, "Proxy ownKeys trap must return an object");
      const keys = yield tenant.yieldTenant(guestArrayLike(tenant, result.value));
      return keys as PropertyKey[];
    },
    *ownPropertyKeys() {
      const result = yield tenant.yieldTenant(trap("ownKeys", [fallback()]));
      if (!result.found) return yield tenant.yieldTenant(tenant.ownPropertyKeys(fallback()));
      assertObject(result.value, "Proxy ownKeys trap must return an object");
      const keys = yield tenant.yieldTenant(guestArrayLike(tenant, result.value));
      return keys as PropertyKey[];
    },
    *getOwnPropertyDescriptor(_receiver, key) {
      const result = yield tenant.yieldTenant(trap("getOwnPropertyDescriptor", [fallback(), key]));
      if (!result.found) return yield tenant.yieldTenant(tenant.getOwnPropertyDescriptor(fallback(), key));
      if (result.value === undefined) return undefined;
      assertObject(result.value, "Proxy descriptor trap must return an object");
      return yield tenant.yieldTenant(readGuestDescriptor(tenant, result.value));
    },
    *defineProperty(_receiver, key, descriptor) {
      const descriptorArg = yield tenant.yieldTenant(descriptorObject(tenant, descriptor));
      const result = yield tenant.yieldTenant(trap("defineProperty", [fallback(), key, descriptorArg]));
      return !result.found
        ? yield tenant.yieldTenant(tenant.defineProperty(fallback(), key, descriptor))
        : !!result.value;
    },
    *getPrototypeOf() {
      const result = yield tenant.yieldTenant(trap("getPrototypeOf", [fallback()]));
      if (!result.found) return yield tenant.yieldTenant(tenant.getPrototypeOf(fallback()));
      if (result.value !== null) assertObject(result.value, "Proxy getPrototypeOf trap must return object or null");
      return result.value as object | null;
    },
    *setPrototypeOf(_receiver, prototype) {
      const result = yield tenant.yieldTenant(trap("setPrototypeOf", [fallback(), prototype]));
      return !result.found ? yield tenant.yieldTenant(tenant.setPrototypeOf(fallback(), prototype)) : !!result.value;
    },
    *isExtensible() {
      const result = yield tenant.yieldTenant(trap("isExtensible", [fallback()]));
      return !result.found ? yield tenant.yieldTenant(tenant.isExtensible(fallback())) : !!result.value;
    },
    *preventExtensions() {
      const result = yield tenant.yieldTenant(trap("preventExtensions", [fallback()]));
      return !result.found ? yield tenant.yieldTenant(tenant.preventExtensions(fallback())) : !!result.value;
    },
    *define(_receiver, descriptors) {
      const result = yield tenant.yieldTenant(trap("defineProperties", [fallback(), descriptors]));
      if (!result.found) yield tenant.yieldTenant(tenant.define(fallback(), descriptors));
    },
    *assign(_receiver, source) {
      const result = yield tenant.yieldTenant(trap("assign", [fallback(), source]));
      if (!result.found) yield tenant.yieldTenant(tenant.assign(fallback(), source));
    },
  };
  return yield tenant.yieldTenant(tenant.makeExotic(null, native));
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
      return yield tenant.yieldTenant(proxyExotic(tenant, args[0], args[1]));
    }, FunctionPrototype,
  ));
  const result = { Proxy: ProxyFn };
  cache.set(tenant, result);
  const revocable = yield tenant.yieldTenant(makeBuiltin(tenant, "revocable", function* (_this, args) {
    assertObject(args[0], "Proxy target must be an object"); assertObject(args[1], "Proxy handler must be an object");
    // Separate state is represented by a forwarding exotic so revoke can clear target/handler.
    const state: ProxyState = { target: args[0], handler: args[1], revoked: false };
    const proxy = yield tenant.yieldTenant(proxyExotic(tenant, state.target, state.handler, state));
    const revoke = yield tenant.yieldTenant(makeBuiltin(tenant, "revoke", function* () {
      state.revoked = true; state.target = undefined; state.handler = undefined;
    }, undefined, FunctionPrototype));
    const record = yield tenant.yieldTenant(tenant.make(null));
    yield tenant.yieldTenant(defineData(tenant, record, "proxy", proxy));
    yield tenant.yieldTenant(defineData(tenant, record, "revoke", revoke));
    return record;
  }, undefined, FunctionPrototype));
  yield tenant.yieldTenant(defineData(tenant, ProxyFn, "revocable", revocable));
  return result;
}
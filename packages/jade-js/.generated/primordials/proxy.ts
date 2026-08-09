import type { Tenant, TenantGenerator, TenantPropertyDescriptor } from "../tenants/types.ts";
import { assertObject, defineData, descriptorObject, guestArrayLike, makeBuiltin, readGuestDescriptor } from "./types.ts";
import { functionPrimordial } from "./function.ts";
export interface ProxyPrimordial {
    Proxy: Function;
}
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
    constructor(target: object, handler: object){
        this.#target = target;
        this.#handler = handler;
    }
    requireLive(): void {
        if (this.#revoked) {
            throw new TypeError("Cannot perform operation on a revoked Proxy");
        }
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
type TrapResult = {
    found: false;
} | {
    found: true;
    value: unknown;
};
const cache = new WeakMap<Tenant, ProxyPrimordial>();
function* trap(tenant: Tenant, state: ProxyState, name: string, args: unknown[]): TenantGenerator<TrapResult> {
    const guestHandler = state.handler();
    const value = yield tenant.yieldTenant(tenant.get(guestHandler, name));
    if (value === undefined) {
        return {
            found: false
        };
    }
    if (typeof value !== "function") {
        throw new TypeError(`Proxy trap ${name} is not callable`);
    }
    return {
        found: true,
        value: yield tenant.yieldTenant(tenant.invoke(value, {
            kind: "apply",
            thisArg: guestHandler,
            args: args
        }))
    };
}
function* proxyExotic(tenant: Tenant, target: object, handler: object, existingState: ProxyState | null): TenantGenerator<object> {
    const state = existingState ?? new ProxyStateImpl(target, handler);
    return yield tenant.yieldTenant(tenant.makeExotic(null, {
        *get (receiver, key) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "get", [
                state.target(),
                key,
                receiver
            ]));
            return result.found ? result.value : yield tenant.yieldTenant(tenant.get(state.target(), key));
        },
        *set (receiver, key, value) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "set", [
                state.target(),
                key,
                value,
                receiver
            ]));
            if (!result.found) {
                yield tenant.yieldTenant(tenant.set(state.target(), key, value));
            } else {
                if (!result.value) {
                    throw new TypeError("Proxy set trap returned false");
                }
            }
        },
        *has (_receiver, key) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "has", [
                state.target(),
                key
            ]));
            return !result.found ? yield tenant.yieldTenant(tenant.has(state.target(), key)) : !!result.value;
        },
        *delete (_receiver, key) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "deleteProperty", [
                state.target(),
                key
            ]));
            if (!result.found) {
                yield tenant.yieldTenant(tenant.delete(state.target(), key));
            } else {
                if (!result.value) {
                    throw new TypeError("Proxy deleteProperty trap returned false");
                }
            }
        },
        *ownKeys () {
            const result = yield tenant.yieldTenant(trap(tenant, state, "ownKeys", [
                state.target()
            ]));
            if (!result.found) {
                return yield tenant.yieldTenant(tenant.ownKeys(state.target()));
            }
            assertObject(result.value, "Proxy ownKeys trap must return an object");
            const keys = yield tenant.yieldTenant(guestArrayLike(tenant, result.value));
            return keys as PropertyKey[];
        },
        *ownPropertyKeys () {
            const result = yield tenant.yieldTenant(trap(tenant, state, "ownKeys", [
                state.target()
            ]));
            if (!result.found) {
                return yield tenant.yieldTenant(tenant.ownPropertyKeys(state.target()));
            }
            assertObject(result.value, "Proxy ownKeys trap must return an object");
            const keys = yield tenant.yieldTenant(guestArrayLike(tenant, result.value));
            return keys as PropertyKey[];
        },
        *getOwnPropertyDescriptor (_receiver, key) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "getOwnPropertyDescriptor", [
                state.target(),
                key
            ]));
            if (!result.found) {
                return yield tenant.yieldTenant(tenant.getOwnPropertyDescriptor(state.target(), key));
            }
            if (result.value === undefined) {
                return undefined;
            }
            assertObject(result.value, "Proxy descriptor trap must return an object");
            return yield tenant.yieldTenant(readGuestDescriptor(tenant, result.value));
        },
        *defineProperty (_receiver, key, descriptor) {
            const descriptorArg = yield tenant.yieldTenant(descriptorObject(tenant, descriptor));
            const result = yield tenant.yieldTenant(trap(tenant, state, "defineProperty", [
                state.target(),
                key,
                descriptorArg
            ]));
            return !result.found ? yield tenant.yieldTenant(tenant.defineProperty(state.target(), key, descriptor)) : !!result.value;
        },
        *getPrototypeOf () {
            const result = yield tenant.yieldTenant(trap(tenant, state, "getPrototypeOf", [
                state.target()
            ]));
            if (!result.found) {
                return yield tenant.yieldTenant(tenant.getPrototypeOf(state.target()));
            }
            if (result.value !== null) {
                assertObject(result.value, "Proxy getPrototypeOf trap must return object or null");
            }
            return result.value as object | null;
        },
        *setPrototypeOf (_receiver, prototype) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "setPrototypeOf", [
                state.target(),
                prototype
            ]));
            return !result.found ? yield tenant.yieldTenant(tenant.setPrototypeOf(state.target(), prototype)) : !!result.value;
        },
        *isExtensible () {
            const result = yield tenant.yieldTenant(trap(tenant, state, "isExtensible", [
                state.target()
            ]));
            return !result.found ? yield tenant.yieldTenant(tenant.isExtensible(state.target())) : !!result.value;
        },
        *preventExtensions () {
            const result = yield tenant.yieldTenant(trap(tenant, state, "preventExtensions", [
                state.target()
            ]));
            return !result.found ? yield tenant.yieldTenant(tenant.preventExtensions(state.target())) : !!result.value;
        },
        *define (_receiver, descriptors) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "defineProperties", [
                state.target(),
                descriptors
            ]));
            if (!result.found) {
                yield tenant.yieldTenant(tenant.define(state.target(), descriptors));
            }
        },
        *assign (_receiver, source) {
            const result = yield tenant.yieldTenant(trap(tenant, state, "assign", [
                state.target(),
                source
            ]));
            if (!result.found) {
                yield tenant.yieldTenant(tenant.assign(state.target(), source));
            }
        }
    }));
}
export function* proxyPrimordial(tenant: Tenant): TenantGenerator<ProxyPrimordial> {
    const existing = cache.get(tenant);
    if (existing) {
        return existing;
    }
    const { FunctionPrototype } = yield tenant.yieldTenant(functionPrimordial(tenant));
    const ProxyFn = yield tenant.yieldTenant(makeBuiltin(tenant, "Proxy", function*() {
        throw new TypeError("Constructor Proxy requires 'new'");
    }, function*(_newTarget: Function, args: unknown[]) {
        assertObject(args[0], "Proxy target must be an object");
        assertObject(args[1], "Proxy handler must be an object");
        return yield tenant.yieldTenant(proxyExotic(tenant, args[0], args[1], undefined));
    }, FunctionPrototype));
    const result = {
        Proxy: ProxyFn
    };
    cache.set(tenant, result);
    const revocable = yield tenant.yieldTenant(makeBuiltin(tenant, "revocable", function*(_this: unknown, args: unknown[]) {
        assertObject(args[0], "Proxy target must be an object");
        assertObject(args[1], "Proxy handler must be an object");
        const state = new ProxyStateImpl(args[0], args[1]);
        const proxy = yield tenant.yieldTenant(proxyExotic(tenant, args[0], args[1], state));
        const revoke = yield tenant.yieldTenant(makeBuiltin(tenant, "revoke", function*() {
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

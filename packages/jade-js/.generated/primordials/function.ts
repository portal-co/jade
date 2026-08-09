import type { Tenant, TenantGenerator } from "../tenants/types.ts";
import { assertObject, defineData, guestArrayLike, makeBuiltin } from "./types.ts";
import { objectPrimordial } from "./object.ts";
export interface FunctionPrimordial {
    Function: Function;
    FunctionPrototype: object;
}
const cache = new WeakMap<Tenant, FunctionPrimordial>();
export function* functionPrimordial(tenant: Tenant): TenantGenerator<FunctionPrimordial> {
    const existing = cache.get(tenant);
    if (existing) {
        return existing;
    }
    const { ObjectPrototype } = yield tenant.yieldTenant(objectPrimordial(tenant));
    const FunctionPrototype = yield tenant.yieldTenant(tenant.make(ObjectPrototype));
    const FunctionFn = yield tenant.yieldTenant(makeBuiltin(tenant, "Function", function*() {
        throw new TypeError("dynamic Function construction is not available in this Jade realm");
    }, function*() {
        throw new TypeError("dynamic Function construction is not available in this Jade realm");
    }, FunctionPrototype));
    const result = {
        Function: FunctionFn,
        FunctionPrototype: FunctionPrototype
    };
    cache.set(tenant, result);
    yield tenant.yieldTenant(tenant.setPrototypeOf(FunctionFn, FunctionPrototype));
    yield tenant.yieldTenant(defineData(tenant, FunctionFn, "prototype", FunctionPrototype, {
        writable: false,
        configurable: false
    }));
    const call = yield tenant.yieldTenant(makeBuiltin(tenant, "call", function*(thisArg: unknown, args: unknown[]) {
        if (typeof thisArg !== "function") {
            throw new TypeError("Function.prototype.call called on non-function");
        }
        return yield tenant.yieldTenant(tenant.invoke(thisArg, {
            kind: "apply",
            thisArg: args[0],
            args: args.slice(1)
        }));
    }, undefined, FunctionPrototype));
    const apply = yield tenant.yieldTenant(makeBuiltin(tenant, "apply", function*(thisArg: unknown, args: unknown[]) {
        if (typeof thisArg !== "function") {
            throw new TypeError("Function.prototype.apply called on non-function");
        }
        const list = args[1];
        const values = list === null || list === undefined ? [] : (assertObject(list, "Function.prototype.apply arguments must be an object"), yield tenant.yieldTenant(guestArrayLike(tenant, list)));
        return yield tenant.yieldTenant(tenant.invoke(thisArg, {
            kind: "apply",
            thisArg: args[0],
            args: values
        }));
    }, undefined, FunctionPrototype));
    const bind = yield tenant.yieldTenant(makeBuiltin(tenant, "bind", function*(thisArg: unknown, args: unknown[]) {
        if (typeof thisArg !== "function") {
            throw new TypeError("Function.prototype.bind called on non-function");
        }
        const target = thisArg;
        const boundThis = args[0];
        const prefix = args.slice(1);
        return yield tenant.yieldTenant(makeBuiltin(tenant, "bound", function*(_ignored: unknown, callArgs: unknown[]) {
            return yield tenant.yieldTenant(tenant.invoke(target, {
                kind: "apply",
                thisArg: boundThis,
                args: [
                    ...prefix,
                    ...callArgs
                ]
            }));
        }, function*(newTarget: Function, callArgs: unknown[]) {
            return yield tenant.yieldTenant(tenant.invoke(target, {
                kind: "construct",
                newTarget: newTarget,
                args: [
                    ...prefix,
                    ...callArgs
                ]
            })) as object;
        }, FunctionPrototype));
    }, undefined, FunctionPrototype));
    yield tenant.yieldTenant(defineData(tenant, FunctionPrototype, "call", call));
    yield tenant.yieldTenant(defineData(tenant, FunctionPrototype, "apply", apply));
    yield tenant.yieldTenant(defineData(tenant, FunctionPrototype, "bind", bind));
    return result;
}

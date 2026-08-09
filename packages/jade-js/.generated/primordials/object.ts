import type { Tenant, TenantGenerator } from "../tenants/types.ts";
import { defineData, descriptorObject, makeBuiltin, readGuestDescriptor, assertObject } from "./types.ts";
export interface ObjectPrimordial {
    Object: Function;
    ObjectPrototype: object;
}
const cache = new WeakMap<Tenant, ObjectPrimordial>();
function* installMethod(tenant: Tenant, target: object, name: string, apply: (thisArg: unknown, args: unknown[]) => TenantGenerator<unknown>): TenantGenerator<Function> {
    const fn = yield tenant.yieldTenant(makeBuiltin(tenant, name, apply));
    yield tenant.yieldTenant(defineData(tenant, target, name, fn, {
        writable: true,
        configurable: true
    }));
    return fn;
}
function* lock(tenant: Tenant, object: object, freeze: boolean): TenantGenerator<object> {
    for (const key of yield tenant.yieldTenant(tenant.ownPropertyKeys(object))){
        const descriptor = yield tenant.yieldTenant(tenant.getOwnPropertyDescriptor(object, key));
        if (!descriptor) {
            continue;
        }
        const next = {
            ...descriptor,
            configurable: false
        };
        if (freeze && ("value" in next || next.writable !== undefined)) {
            next.writable = false;
        }
        if (!(yield tenant.yieldTenant(tenant.defineProperty(object, key, next)))) {
            throw new TypeError("cannot update object descriptor");
        }
    }
    if (!(yield tenant.yieldTenant(tenant.preventExtensions(object)))) {
        throw new TypeError("cannot prevent extensions");
    }
    return object;
}
export function* objectPrimordial(tenant: Tenant): TenantGenerator<ObjectPrimordial> {
    const existing = cache.get(tenant);
    if (existing) {
        return existing;
    }
    const ObjectPrototype = yield tenant.yieldTenant(tenant.make(null));
    const ObjectFn = yield tenant.yieldTenant(makeBuiltin(tenant, "Object", function*(_thisArg: unknown, args: unknown[]) {
        const value = args[0];
        if (value === null || value === undefined) {
            return yield tenant.yieldTenant(tenant.make(ObjectPrototype));
        }
        if (typeof value === "object" || typeof value === "function") {
            return value;
        }
        throw new TypeError("primitive Object boxing is not available in this Jade realm");
    }, function*(_newTarget: Function, args: unknown[]) {
        const value = args[0];
        if (value === null || value === undefined) {
            return yield tenant.yieldTenant(tenant.make(ObjectPrototype));
        }
        if (typeof value === "object" || typeof value === "function") {
            return value as object;
        }
        throw new TypeError("primitive Object boxing is not available in this Jade realm");
    }));
    const result = {
        Object: ObjectFn,
        ObjectPrototype: ObjectPrototype
    };
    cache.set(tenant, result);
    yield tenant.yieldTenant(defineData(tenant, ObjectFn, "prototype", ObjectPrototype, {
        writable: false,
        configurable: false
    }));
    yield tenant.yieldTenant(tenant.setPrototypeOf(ObjectFn, ObjectPrototype));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "create", function*(_thisArg: unknown, args: unknown[]) {
        const proto = args[0];
        if (proto !== null) {
            assertObject(proto, "Object.create prototype must be object or null");
        }
        return yield tenant.yieldTenant(tenant.make(proto as object | null));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "keys", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        return yield tenant.yieldTenant(tenant.ownKeys(args[0]));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "hasOwn", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        return yield tenant.yieldTenant(tenant.has(args[0], args[1] as PropertyKey));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "assign", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        for (const source of args.slice(1)){
            if (source !== null && source !== undefined) {
                assertObject(source);
                yield tenant.yieldTenant(tenant.assign(args[0], source));
            }
        }
        return args[0];
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "defineProperty", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        assertObject(args[2], "property descriptor must be an object");
        const descriptor = yield tenant.yieldTenant(readGuestDescriptor(tenant, args[2]));
        if (!(yield tenant.yieldTenant(tenant.defineProperty(args[0], args[1] as PropertyKey, descriptor)))) {
            throw new TypeError("cannot define property");
        }
        return args[0];
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "defineProperties", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        assertObject(args[1], "descriptors must be an object");
        yield tenant.yieldTenant(tenant.define(args[0], args[1]));
        return args[0];
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "getOwnPropertyDescriptor", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        const descriptor = yield tenant.yieldTenant(tenant.getOwnPropertyDescriptor(args[0], args[1] as PropertyKey));
        return descriptor === undefined ? undefined : yield tenant.yieldTenant(descriptorObject(tenant, descriptor));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "getPrototypeOf", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        return yield tenant.yieldTenant(tenant.getPrototypeOf(args[0]));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "setPrototypeOf", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        if (args[1] !== null) {
            assertObject(args[1], "prototype must be object or null");
        }
        if (!(yield tenant.yieldTenant(tenant.setPrototypeOf(args[0], args[1] as object | null)))) {
            throw new TypeError("cannot set prototype");
        }
        return args[0];
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "isExtensible", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        return yield tenant.yieldTenant(tenant.isExtensible(args[0]));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "preventExtensions", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        if (!(yield tenant.yieldTenant(tenant.preventExtensions(args[0])))) {
            throw new TypeError("cannot prevent extensions");
        }
        return args[0];
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "seal", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        return yield tenant.yieldTenant(lock(tenant, args[0], false));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectFn, "freeze", function*(_thisArg: unknown, args: unknown[]) {
        assertObject(args[0]);
        return yield tenant.yieldTenant(lock(tenant, args[0], true));
    }));
    yield tenant.yieldTenant(installMethod(tenant, ObjectPrototype, "hasOwnProperty", function*(thisArg: unknown, args: unknown[]) {
        assertObject(thisArg, "Object.prototype.hasOwnProperty called on non-object");
        return yield tenant.yieldTenant(tenant.has(thisArg, args[0] as PropertyKey));
    }));
    return result;
}

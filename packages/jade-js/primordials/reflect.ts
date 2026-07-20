import type { Tenant, TenantGenerator } from "../tenants/types.ts";
import { assertObject, defineData, descriptorObject, guestArrayLike, makeBuiltin, readGuestDescriptor } from "./types.ts";
import { objectPrimordial } from "./object.ts";

export interface ReflectPrimordial { Reflect: object; }
const cache = new WeakMap<Tenant, ReflectPrimordial>();

function* method(
  tenant: Tenant, target: object, name: string,
  apply: (thisArg: unknown, args: readonly unknown[]) => TenantGenerator<unknown>,
): TenantGenerator<void> {
  yield tenant.yieldTenant(defineData(tenant, target, name,
    yield tenant.yieldTenant(makeBuiltin(tenant, name, apply))));
}

export function* reflectPrimordial(tenant: Tenant): TenantGenerator<ReflectPrimordial> {
  const existing = cache.get(tenant);
  if (existing) return existing;
  const { ObjectPrototype } = yield tenant.yieldTenant(objectPrimordial(tenant));
  const ReflectObject = yield tenant.yieldTenant(tenant.make(ObjectPrototype));
  const result = { Reflect: ReflectObject };
  cache.set(tenant, result);

  yield tenant.yieldTenant(method(tenant, ReflectObject, "get", function* (_this, args) {
    assertObject(args[0]); return yield tenant.yieldTenant(tenant.get(args[0], args[1] as PropertyKey));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "set", function* (_this, args) {
    assertObject(args[0]); yield tenant.yieldTenant(tenant.set(args[0], args[1] as PropertyKey, args[2])); return true;
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "has", function* (_this, args) {
    assertObject(args[0]); return yield tenant.yieldTenant(tenant.has(args[0], args[1] as PropertyKey));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "deleteProperty", function* (_this, args) {
    assertObject(args[0]); yield tenant.yieldTenant(tenant.delete(args[0], args[1] as PropertyKey)); return true;
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "ownKeys", function* (_this, args) {
    assertObject(args[0]); return yield tenant.yieldTenant(tenant.ownPropertyKeys(args[0]));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "getOwnPropertyDescriptor", function* (_this, args) {
    assertObject(args[0]);
    const descriptor = yield tenant.yieldTenant(tenant.getOwnPropertyDescriptor(args[0], args[1] as PropertyKey));
    return descriptor === undefined ? undefined : yield tenant.yieldTenant(descriptorObject(tenant, descriptor));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "defineProperty", function* (_this, args) {
    assertObject(args[0]); assertObject(args[2], "descriptor must be an object");
    return yield tenant.yieldTenant(tenant.defineProperty(args[0], args[1] as PropertyKey,
      yield tenant.yieldTenant(readGuestDescriptor(tenant, args[2]))));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "getPrototypeOf", function* (_this, args) {
    assertObject(args[0]); return yield tenant.yieldTenant(tenant.getPrototypeOf(args[0]));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "setPrototypeOf", function* (_this, args) {
    assertObject(args[0]); if (args[1] !== null) assertObject(args[1]);
    return yield tenant.yieldTenant(tenant.setPrototypeOf(args[0], args[1] as object | null));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "isExtensible", function* (_this, args) {
    assertObject(args[0]); return yield tenant.yieldTenant(tenant.isExtensible(args[0]));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "preventExtensions", function* (_this, args) {
    assertObject(args[0]); return yield tenant.yieldTenant(tenant.preventExtensions(args[0]));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "apply", function* (_this, args) {
    if (typeof args[0] !== "function") throw new TypeError("Reflect.apply target must be a function");
    assertObject(args[2], "Reflect.apply arguments must be an object");
    return yield tenant.yieldTenant(tenant.invoke(args[0], {
      kind: "apply", thisArg: args[1], args: yield tenant.yieldTenant(guestArrayLike(tenant, args[2])),
    }));
  }));
  yield tenant.yieldTenant(method(tenant, ReflectObject, "construct", function* (_this, args) {
    if (typeof args[0] !== "function") throw new TypeError("Reflect.construct target must be a function");
    assertObject(args[1], "Reflect.construct arguments must be an object");
    const newTarget = args[2] === undefined ? args[0] : args[2];
    if (typeof newTarget !== "function") throw new TypeError("Reflect.construct newTarget must be a function");
    return yield tenant.yieldTenant(tenant.invoke(args[0], {
      kind: "construct", newTarget, args: yield tenant.yieldTenant(guestArrayLike(tenant, args[1])),
    }));
  }));
  return result;
}
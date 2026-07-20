import { MultiTenant, single_tenant } from "./index.ts";
import { objectPrimordial } from "./primordials/object.ts";
import { functionPrimordial } from "./primordials/function.ts";
import { reflectPrimordial } from "./primordials/reflect.ts";
import { proxyPrimordial } from "./primordials/proxy.ts";
import { bufferPrimordial, nativeBufferHooks } from "./primordials/array-buffer.ts";
import { typedArraysPrimordial } from "./primordials/typed-arrays.ts";
import { createNativeHostAsyncCapability } from "./async-host.ts";
import { createPrimordialRealm } from "./primordials/realm.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`FAIL: ${message}`);
}
function drive<T>(tenant: MultiTenant | typeof single_tenant, gen: Generator<any, T, any>): T {
  return tenant.driveTenant(gen, false, false) as T;
}

for (const tenant of [new MultiTenant(), single_tenant] as const) {
  const objects = drive(tenant, objectPrimordial(tenant));
  assert(drive(tenant, objectPrimordial(tenant)) === objects, "Object cache must preserve exact tenant identity");
  const value = drive(tenant, tenant.make(objects.ObjectPrototype));
  drive(tenant, tenant.set(value, "x", 1));
  const freeze = drive(tenant, tenant.get(objects.Object, "freeze")) as Function;
  drive(tenant, tenant.invoke(freeze, { kind: "apply", thisArg: objects.Object, args: [value] }));
  assert(drive(tenant, tenant.isExtensible(value)) === false, "Object.freeze must prevent extensions");
  assert(drive(tenant, tenant.getOwnPropertyDescriptor(value, "x"))?.writable === false,
    "Object.freeze must make data descriptors non-writable");

  const functions = drive(tenant, functionPrimordial(tenant));
  let dynamicRejected = false;
  try { drive(tenant, tenant.invoke(functions.Function, { kind: "apply", thisArg: undefined, args: [] })); }
  catch (error) { dynamicRejected = error instanceof TypeError; }
  assert(dynamicRejected, "Function must reject dynamic source compilation");

  const reflect = drive(tenant, reflectPrimordial(tenant));
  const ownKeys = drive(tenant, tenant.get(reflect.Reflect, "ownKeys")) as Function;
  assert((drive(tenant, tenant.invoke(ownKeys, { kind: "apply", thisArg: reflect.Reflect, args: [value] })) as PropertyKey[]).includes("x"),
    "Reflect.ownKeys must expose non-enumerable keys through tenant API");

  const proxy = drive(tenant, proxyPrimordial(tenant));
  const handler = drive(tenant, tenant.make(null));
  const proxyValue = drive(tenant, tenant.invoke(proxy.Proxy, { kind: "construct", newTarget: proxy.Proxy, args: [value, handler] })) as object;
  assert(drive(tenant, tenant.get(proxyValue, "x")) === 1, "Proxy must fall back to target when guest trap is absent");

  const buffers = drive(tenant, bufferPrimordial(tenant, nativeBufferHooks));
  const buffer = drive(tenant, tenant.invoke(buffers.ArrayBuffer, {
    kind: "construct", newTarget: buffers.ArrayBuffer, args: [8],
  })) as object;
  assert(drive(tenant, tenant.get(buffer, "byteLength")) === 8, "ArrayBuffer shell uses explicit native hook capability");
  const typed = drive(tenant, typedArraysPrimordial(tenant, nativeBufferHooks, ["Uint8Array"]));
  const Uint8ArrayCtor = typed.constructors.Uint8Array;
  const array = drive(tenant, tenant.invoke(Uint8ArrayCtor, { kind: "construct", newTarget: Uint8ArrayCtor, args: [2] })) as object;
  drive(tenant, tenant.set(array, "0", 255));
  assert(drive(tenant, tenant.get(array, "0")) === 255, "typed array indexes go through BufferHooks");

  const realm = drive(tenant, createPrimordialRealm(tenant, {
    async: createNativeHostAsyncCapability(),
  }));
  assert(drive(tenant, tenant.get(realm.globalThis, "Object")) === realm.Object, "realm installs Object");
  assert(drive(tenant, tenant.get(realm.globalThis, "ArrayBuffer")) === undefined,
    "BufferHooks must be explicit: bare realm has no ArrayBuffer");
}
console.log("PASS: tenant-scoped primordials");
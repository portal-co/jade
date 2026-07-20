import type { Tenant, TenantGenerator } from "../tenants/types.ts";
import type { BufferHooks } from "./array-buffer.ts";
import { bufferPrimordial } from "./array-buffer.ts";
import { defineData } from "./types.ts";
import { objectPrimordial } from "./object.ts";
import { functionPrimordial } from "./function.ts";
import { reflectPrimordial } from "./reflect.ts";
import { proxyPrimordial } from "./proxy.ts";
import { type TypedArrayKind, typedArraysPrimordial } from "./typed-arrays.ts";

export interface PrimordialRealmOptions {
  /** Explicit binary-memory authority. Omit it to expose no buffer constructors. */
  buffers?: BufferHooks;
  includeSharedArrayBuffer?: boolean;
  typedArrays?: readonly TypedArrayKind[];
}
export interface PrimordialRealm {
  globalThis: object;
  Object: Function;
  Function: Function;
  Reflect: object;
  Proxy: Function;
  ArrayBuffer?: Function;
  SharedArrayBuffer?: Function;
  typedArrays: Readonly<Partial<Record<TypedArrayKind, Function>>>;
}

/** Assemble a tenant-owned global explicitly; no host global is consulted. */
export function* createPrimordialRealm(
  tenant: Tenant,
  options: PrimordialRealmOptions = {},
): TenantGenerator<PrimordialRealm> {
  const object = yield tenant.yieldTenant(objectPrimordial(tenant));
  const functions = yield tenant.yieldTenant(functionPrimordial(tenant));
  const reflect = yield tenant.yieldTenant(reflectPrimordial(tenant));
  const proxy = yield tenant.yieldTenant(proxyPrimordial(tenant));
  const globalThis = yield tenant.yieldTenant(tenant.make(object.ObjectPrototype));
  const realm: PrimordialRealm = {
    globalThis, Object: object.Object, Function: functions.Function, Reflect: reflect.Reflect,
    Proxy: proxy.Proxy, typedArrays: {},
  };
  for (const [key, value] of Object.entries({
    Object: realm.Object, Function: realm.Function, Reflect: realm.Reflect, Proxy: realm.Proxy,
  })) yield tenant.yieldTenant(defineData(tenant, globalThis, key, value));
  if (options.buffers) {
    const buffers = yield tenant.yieldTenant(bufferPrimordial(tenant, options.buffers));
    realm.ArrayBuffer = buffers.ArrayBuffer;
    yield tenant.yieldTenant(defineData(tenant, globalThis, "ArrayBuffer", buffers.ArrayBuffer));
    if (options.includeSharedArrayBuffer && buffers.SharedArrayBuffer) {
      realm.SharedArrayBuffer = buffers.SharedArrayBuffer;
      yield tenant.yieldTenant(defineData(tenant, globalThis, "SharedArrayBuffer", buffers.SharedArrayBuffer));
    }
    const typed = yield tenant.yieldTenant(typedArraysPrimordial(tenant, options.buffers, options.typedArrays));
    realm.typedArrays = typed.constructors;
    for (const [key, value] of Object.entries(typed.constructors)) {
      yield tenant.yieldTenant(defineData(tenant, globalThis, key, value));
    }
  }
  return realm;
}
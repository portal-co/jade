import type { Tenant, TenantGenerator } from "../tenants/types.ts";
import { assertObject, defineData, makeBuiltin, toIndex } from "./types.ts";
import { objectPrimordial } from "./object.ts";

export type BufferKind = "array-buffer" | "shared-array-buffer";
export type BufferHandle = object;

/** Explicit embedder capability. No buffer primordial exists without one. */
export interface BufferHooks {
  readonly identity: object;
  readonly supportsSharedArrayBuffer: boolean;
  allocate(kind: BufferKind, byteLength: number): TenantGenerator<BufferHandle>;
  isHandle(value: unknown, kind?: BufferKind): boolean;
  byteLength(handle: BufferHandle): TenantGenerator<number>;
  slice(handle: BufferHandle, begin: number, end: number): TenantGenerator<BufferHandle>;
  read(handle: BufferHandle, byteOffset: number, byteLength: number): TenantGenerator<Uint8Array>;
  write(handle: BufferHandle, byteOffset: number, bytes: Uint8Array): TenantGenerator<void>;
}

export interface BufferRecord { readonly kind: BufferKind; readonly handle: BufferHandle; }
export interface BufferPrimordial {
  ArrayBuffer: Function;
  ArrayBufferPrototype: object;
  SharedArrayBuffer?: Function;
  SharedArrayBufferPrototype?: object;
  record(value: unknown): BufferRecord | undefined;
  shell(kind: BufferKind, handle: BufferHandle): TenantGenerator<object>;
}

const caches = new WeakMap<Tenant, { identity: object; primordial: BufferPrimordial }>();

/** Explicit opt-in adapter around native buffers; never automatically selected. */
export const nativeBufferHooks: BufferHooks = {
  identity: {},
  supportsSharedArrayBuffer: typeof SharedArrayBuffer === "function",
  *allocate(kind, byteLength) {
    return (kind === "shared-array-buffer"
      ? new SharedArrayBuffer(byteLength)
      : new ArrayBuffer(byteLength)) as unknown as BufferHandle;
  },
  isHandle(value, kind) {
    if (kind === "shared-array-buffer") {
      return typeof SharedArrayBuffer === "function" && value instanceof SharedArrayBuffer;
    }
    return value instanceof ArrayBuffer;
  },
  *byteLength(handle) { return (handle as unknown as ArrayBuffer).byteLength; },
  *slice(handle, begin, end) { return (handle as unknown as ArrayBuffer).slice(begin, end) as unknown as BufferHandle; },
  *read(handle, offset, length) { return new Uint8Array((handle as unknown as ArrayBuffer).slice(offset, offset + length)); },
  *write(handle, offset, bytes) { new Uint8Array(handle as unknown as ArrayBuffer, offset, bytes.byteLength).set(bytes); },
};

export function* bufferPrimordial(tenant: Tenant, hooks: BufferHooks): TenantGenerator<BufferPrimordial> {
  const cached = caches.get(tenant);
  if (cached) {
    if (cached.identity !== hooks.identity) throw new TypeError("a tenant realm may have only one BufferHooks capability");
    return cached.primordial;
  }
  const { ObjectPrototype } = yield tenant.yieldTenant(objectPrimordial(tenant));
  const records = new WeakMap<object, BufferRecord>();
  const prototypes = new Map<BufferKind, object>();
  let result!: BufferPrimordial;
  const shell = function* (kind: BufferKind, handle: BufferHandle): TenantGenerator<object> {
    const proto = prototypes.get(kind)!;
    const value = yield tenant.yieldTenant(tenant.makeExotic(proto, {
      *get(receiver, key) {
        const record = records.get(receiver)!;
        if (key === "byteLength") return yield tenant.yieldTenant(hooks.byteLength(record.handle));
        if (key === "slice") return yield tenant.yieldTenant(makeBuiltin(tenant, "slice", function* (_this, args) {
          const length = yield tenant.yieldTenant(hooks.byteLength(record.handle));
          const begin = Math.min(toIndex(args[0] ?? 0), length);
          const end = Math.min(toIndex(args[1] ?? length), length);
          return yield tenant.yieldTenant(shell(record.kind, yield tenant.yieldTenant(hooks.slice(record.handle, begin, end))));
        }, undefined, ObjectPrototype));
        return undefined;
      },
      *set() {}, *has(_receiver, key) { return key === "byteLength" || key === "slice"; }, *delete() {},
      *ownKeys() { return []; }, *ownPropertyKeys() { return []; },
      *getOwnPropertyDescriptor() { return undefined; }, *defineProperty() { return false; },
      *getPrototypeOf() { return proto; }, *setPrototypeOf() { return false; },
      *isExtensible() { return true; }, *preventExtensions() { return true; }, *define() {}, *assign() {},
    }));
    records.set(value, { kind, handle });
    return value;
  };
  const constructor = function* (kind: BufferKind, name: string): TenantGenerator<Function> {
    const proto = yield tenant.yieldTenant(tenant.make(ObjectPrototype));
    prototypes.set(kind, proto);
    const ctor = yield tenant.yieldTenant(makeBuiltin(tenant, name,
      function* () { throw new TypeError(`Constructor ${name} requires 'new'`); },
      function* (_target, args) { return yield tenant.yieldTenant(shell(kind, yield tenant.yieldTenant(hooks.allocate(kind, toIndex(args[0] ?? 0))))); },
      ObjectPrototype));
    yield tenant.yieldTenant(defineData(tenant, ctor, "prototype", proto, { writable: false, configurable: false }));
    return ctor;
  };
  const ArrayBuffer = yield tenant.yieldTenant(constructor("array-buffer", "ArrayBuffer"));
  const SharedArrayBuffer = hooks.supportsSharedArrayBuffer
    ? yield tenant.yieldTenant(constructor("shared-array-buffer", "SharedArrayBuffer")) : undefined;
  result = { ArrayBuffer, ArrayBufferPrototype: prototypes.get("array-buffer")!, SharedArrayBuffer,
    SharedArrayBufferPrototype: prototypes.get("shared-array-buffer"), record: (value) =>
      value !== null && (typeof value === "object" || typeof value === "function") ? records.get(value as object) : undefined,
    shell };
  caches.set(tenant, { identity: hooks.identity, primordial: result });
  return result;
}
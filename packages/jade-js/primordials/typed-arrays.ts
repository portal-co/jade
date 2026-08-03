import type { Tenant, TenantGenerator } from "../tenants/types.ts";
import type { BufferHooks, BufferPrimordial } from "./array-buffer.ts";
import { bufferPrimordial } from "./array-buffer.ts";
import { defineData, makeBuiltin, toIndex } from "./types.ts";
import { objectPrimordial } from "./object.ts";

export type TypedArrayKind =
  | "Int8Array" | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
  | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array";
type Codec = { bytes: number; get(view: DataView, offset: number): number; set(view: DataView, offset: number, value: number): void };
type Record_ = { kind: TypedArrayKind; buffer: object; offset: number; length: number };

const codecs: Record<TypedArrayKind, Codec> = {
  Int8Array: { bytes: 1, get: (v, i) => v.getInt8(i), set: (v, i, x) => v.setInt8(i, x) },
  Uint8Array: { bytes: 1, get: (v, i) => v.getUint8(i), set: (v, i, x) => v.setUint8(i, x) },
  Uint8ClampedArray: { bytes: 1, get: (v, i) => v.getUint8(i), set: (v, i, x) => v.setUint8(i, Math.max(0, Math.min(255, Math.round(x)))) },
  Int16Array: { bytes: 2, get: (v, i) => v.getInt16(i, true), set: (v, i, x) => v.setInt16(i, x, true) },
  Uint16Array: { bytes: 2, get: (v, i) => v.getUint16(i, true), set: (v, i, x) => v.setUint16(i, x, true) },
  Int32Array: { bytes: 4, get: (v, i) => v.getInt32(i, true), set: (v, i, x) => v.setInt32(i, x, true) },
  Uint32Array: { bytes: 4, get: (v, i) => v.getUint32(i, true), set: (v, i, x) => v.setUint32(i, x, true) },
  Float32Array: { bytes: 4, get: (v, i) => v.getFloat32(i, true), set: (v, i, x) => v.setFloat32(i, x, true) },
  Float64Array: { bytes: 8, get: (v, i) => v.getFloat64(i, true), set: (v, i, x) => v.setFloat64(i, x, true) },
};
/**
 * `constructors` is a `Map`, not a plain `Record<TypedArrayKind, Function>`: the Rust IR/emitter
 * already has full `Map`/`WeakMap` support (`HostIntrinsic` get/set/has/delete + `rust_type`'s
 * generic case) but no translation for a plain object used as a string-keyed lookup table, so this
 * is the same "shape the TS source so the mechanical translation is clean" adjustment
 * `array-buffer.ts` made a few times — see `docs/proxy-and-buffer-primordial-gap-plan.md`.
 */
export interface TypedArrayPrimordial { constructors: ReadonlyMap<TypedArrayKind, Function>; }
const caches = new WeakMap<Tenant, { identity: object; primordial: TypedArrayPrimordial }>();

/**
 * Backs `TypedArrayPrimordial`, mirroring `array-buffer.ts`'s `BufferPrimordialImpl`: `create`
 * recurses into itself (the `"subarray"` builtin it installs on a typed array calls `create`
 * again) — the same self-referential-closure problem `shell` had, needing a real `class` rather
 * than a plain closure. `#hooks`/`#buffers`/`#records`/`#prototypes` are private, touched only by
 * this class's own methods, including the nested exotic-handler/builtin closures lexically inside
 * them, which reach back in via the captured `that` (not `self` — see `BufferPrimordialImpl`'s own
 * doc comment for why).
 */
class TypedArrayPrimordialImpl implements TypedArrayPrimordial {
  #buffers: BufferPrimordial;
  #hooks: BufferHooks;
  #objectPrototype: object;
  #records: WeakMap<object, Record_> = new WeakMap();
  #prototypes: Map<TypedArrayKind, object> = new Map();
  constructors: Map<TypedArrayKind, Function> = new Map();

  constructor(buffers: BufferPrimordial, hooks: BufferHooks, objectPrototype: object) {
    this.#buffers = buffers;
    this.#hooks = hooks;
    this.#objectPrototype = objectPrototype;
  }

  *create(tenant: Tenant, kind: TypedArrayKind, buffer: object, offset: number, length: number): TenantGenerator<object> {
    const that = this;
    const codec = codecs[kind];
    const prototype = this.#prototypes.get(kind)!;
    const value = yield tenant.yieldTenant(tenant.makeExotic(prototype, {
      *get(receiver, key) {
        const record = that.#records.get(receiver)!;
        if (key === "length") return record.length;
        if (key === "byteLength") return record.length * codec.bytes;
        if (key === "byteOffset") return record.offset;
        if (key === "buffer") return record.buffer;
        if (typeof key === "string" && /^(0|[1-9][0-9]*)$/.test(key)) {
          const index = Number(key); if (index >= record.length) return undefined;
          const handle = that.#buffers.record(tenant, record.buffer)!.handle;
          const bytes = yield tenant.yieldTenant(that.#hooks.read(handle, record.offset + index * codec.bytes, codec.bytes));
          return codec.get(new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), 0);
        }
        if (key === "subarray") return yield tenant.yieldTenant(makeBuiltin(tenant, "subarray", function* (_this, args) {
          const begin = Math.min(toIndex(args[0] ?? 0), record.length);
          const end = Math.min(toIndex(args[1] ?? record.length), record.length);
          return yield tenant.yieldTenant(that.create(tenant, record.kind, record.buffer, record.offset + begin * codec.bytes, Math.max(0, end - begin)));
        }, undefined, that.#objectPrototype));
        if (key === "set") return yield tenant.yieldTenant(makeBuiltin(tenant, "set", function* (_this, args) {
          const source = that.#records.get(args[0] as object); if (!source) throw new TypeError("TypedArray.set requires a Jade typed array");
          const start = toIndex(args[1] ?? 0); if (start + source.length > record.length) throw new RangeError("source is too large");
          for (let i = 0; i < source.length; i++) {
            const sourceCodec = codecs[source.kind];
            const sourceHandle = that.#buffers.record(tenant, source.buffer)!.handle;
            const bytes = yield tenant.yieldTenant(that.#hooks.read(sourceHandle, source.offset + i * sourceCodec.bytes, sourceCodec.bytes));
            const number = sourceCodec.get(new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), 0);
            const target = new Uint8Array(codec.bytes); codec.set(new DataView(target.buffer), 0, number);
            yield tenant.yieldTenant(that.#hooks.write(that.#buffers.record(tenant, record.buffer)!.handle, record.offset + (start + i) * codec.bytes, target));
          }
        }, undefined, that.#objectPrototype));
        return undefined;
      },
      *set(receiver, key, value) {
        if (typeof key !== "string" || !/^(0|[1-9][0-9]*)$/.test(key)) return;
        const record = that.#records.get(receiver)!; const index = Number(key); if (index >= record.length) return;
        const bytes = new Uint8Array(codec.bytes); codec.set(new DataView(bytes.buffer), 0, Number(value));
        yield tenant.yieldTenant(that.#hooks.write(that.#buffers.record(tenant, record.buffer)!.handle, record.offset + index * codec.bytes, bytes));
      },
      *has(receiver, key) { const record = that.#records.get(receiver)!; return typeof key === "string" && /^(0|[1-9][0-9]*)$/.test(key) && Number(key) < record.length; },
      *delete() {}, *ownKeys(receiver) { const record = that.#records.get(receiver)!; return Array.from({ length: record.length }, (_, i) => String(i)); },
      *ownPropertyKeys(receiver) { const record = that.#records.get(receiver)!; return Array.from({ length: record.length }, (_, i) => String(i)); },
      *getOwnPropertyDescriptor() { return undefined; }, *defineProperty() { return false; }, *getPrototypeOf() { return prototype; },
      *setPrototypeOf() { return false; }, *isExtensible() { return true; }, *preventExtensions() { return true; }, *define() {}, *assign() {},
    }));
    this.#records.set(value, { kind, buffer, offset, length });
    return value;
  }

  *makeConstructor(tenant: Tenant, kind: TypedArrayKind): TenantGenerator<Function> {
    const that = this;
    const codec = codecs[kind];
    const prototype = yield tenant.yieldTenant(tenant.make(this.#objectPrototype));
    this.#prototypes.set(kind, prototype);
    const ctor = yield tenant.yieldTenant(makeBuiltin(tenant, kind,
      function* () { throw new TypeError(`Constructor ${kind} requires 'new'`); },
      function* (_target, args) {
        const first = args[0];
        const bufferRecord = that.#buffers.record(tenant, first);
        if (bufferRecord) {
          const offset = toIndex(args[1] ?? 0); if (offset % codec.bytes) throw new RangeError("unaligned byteOffset");
          const total = yield tenant.yieldTenant(that.#hooks.byteLength(bufferRecord.handle));
          const length = args[2] === undefined ? (total - offset) / codec.bytes : toIndex(args[2]);
          if (!Number.isInteger(length) || offset + length * codec.bytes > total) throw new RangeError("typed array is out of bounds");
          return yield tenant.yieldTenant(that.create(tenant, kind, first as object, offset, length));
        }
        const length = toIndex(first ?? 0); const handle = yield tenant.yieldTenant(that.#hooks.allocate("array-buffer", length * codec.bytes));
        return yield tenant.yieldTenant(
          that.create(tenant, kind, yield tenant.yieldTenant(that.#buffers.shell(tenant, "array-buffer", handle, that.#objectPrototype)), 0, length),
        );
      }, this.#objectPrototype));
    yield tenant.yieldTenant(defineData(tenant, ctor, "prototype", prototype, { writable: false, configurable: false }));
    return ctor;
  }
}

export function* typedArraysPrimordial(
  tenant: Tenant, hooks: BufferHooks, kinds: readonly TypedArrayKind[] = Object.keys(codecs) as TypedArrayKind[],
): TenantGenerator<TypedArrayPrimordial> {
  const cached = caches.get(tenant);
  if (cached) {
    if (cached.identity !== hooks.identity) throw new TypeError("typed arrays require the realm BufferHooks capability");
    return cached.primordial;
  }
  const buffers = yield tenant.yieldTenant(bufferPrimordial(tenant, hooks));
  const { ObjectPrototype } = yield tenant.yieldTenant(objectPrimordial(tenant));
  const impl = new TypedArrayPrimordialImpl(buffers, hooks, ObjectPrototype);
  for (const kind of kinds) {
    impl.constructors.set(kind, yield tenant.yieldTenant(impl.makeConstructor(tenant, kind)));
  }
  const primordial: TypedArrayPrimordial = impl;
  caches.set(tenant, { identity: hooks.identity, primordial });
  return primordial;
}

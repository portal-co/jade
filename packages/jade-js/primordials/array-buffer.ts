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
/**
 * `shell`/`record` take `tenant` explicitly, unlike every other reader-facing method in this
 * codebase's TS surface: a Rust port's `record`/`shell` need it too (`typeof`/identity-keyed
 * lookups are ambient host operations in TS but tenant-mediated operations in Rust), and this
 * interface's shape is shared verbatim by both ports rather than diverging — see
 * `docs/proxy-and-buffer-primordial-gap-plan.md`.
 */
export interface BufferPrimordial {
  ArrayBuffer: Function;
  ArrayBufferPrototype: object;
  SharedArrayBuffer?: Function;
  SharedArrayBufferPrototype?: object;
  record(tenant: Tenant, value: unknown): BufferRecord | undefined;
  shell(tenant: Tenant, kind: BufferKind, handle: BufferHandle): TenantGenerator<object>;
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

/**
 * Backs `BufferPrimordial`. A real `class` (not the closures-plus-shared-state idiom every other
 * primordial factory uses) specifically so a Rust port can translate it directly: `shell`
 * recurses into itself (the `"slice"` builtin it installs on a buffer shell calls `shell` again),
 * which a plain Rust closure can't do — a struct with `self`-calling inherent methods has none of
 * that problem. `#hooks`/`#records`/`#prototypes` are private (only ever touched by this class's
 * own methods, including the nested exotic-handler/builtin closures lexically inside them, which
 * reach back in via the captured `self`); nothing outside this class needs them. See
 * `docs/proxy-and-buffer-primordial-gap-plan.md`.
 *
 * Deliberately does **not** store `tenant` as a field, unlike `#hooks`: every other tenant-owned
 * value in this codebase treats `tenant` as a fresh per-call argument, never something held
 * across calls (see e.g. `tenants/types.ts`'s own doc comments) — `#hooks` is a plain, owned
 * embedder capability with no such lifetime, so it *can* be captured once at construction.
 */
class BufferPrimordialImpl implements BufferPrimordial {
  #hooks: BufferHooks;
  #records = new WeakMap<object, BufferRecord>();
  #prototypes = new Map<BufferKind, object>();
  ArrayBuffer!: Function;
  ArrayBufferPrototype!: object;
  SharedArrayBuffer?: Function;
  SharedArrayBufferPrototype?: object;

  constructor(hooks: BufferHooks) {
    this.#hooks = hooks;
  }

  record(_tenant: Tenant, value: unknown): BufferRecord | undefined {
    return value !== null && (typeof value === "object" || typeof value === "function")
      ? this.#records.get(value as object)
      : undefined;
  }

  /** Only ever called by this class's own methods and `bufferPrimordial` — not part of the
   * public `BufferPrimordial` interface. */
  prototypeOf(kind: BufferKind): object {
    return this.#prototypes.get(kind)!;
  }

  *shell(tenant: Tenant, kind: BufferKind, handle: BufferHandle): TenantGenerator<object> {
    const hooks = this.#hooks;
    const self = this;
    const { ObjectPrototype } = yield tenant.yieldTenant(objectPrimordial(tenant));
    const proto = this.#prototypes.get(kind)!;
    const value = yield tenant.yieldTenant(tenant.makeExotic(proto, {
      *get(receiver, key) {
        const record = self.record(tenant, receiver)!;
        if (key === "byteLength") return yield tenant.yieldTenant(hooks.byteLength(record.handle));
        if (key === "slice") return yield tenant.yieldTenant(makeBuiltin(tenant, "slice", function* (_this, args) {
          const length = yield tenant.yieldTenant(hooks.byteLength(record.handle));
          const begin = Math.min(toIndex(args[0] ?? 0), length);
          const end = Math.min(toIndex(args[1] ?? length), length);
          return yield tenant.yieldTenant(self.shell(tenant, record.kind, yield tenant.yieldTenant(hooks.slice(record.handle, begin, end))));
        }, undefined, ObjectPrototype));
        return undefined;
      },
      *set() {}, *has(_receiver, key) { return key === "byteLength" || key === "slice"; }, *delete() {},
      *ownKeys() { return []; }, *ownPropertyKeys() { return []; },
      *getOwnPropertyDescriptor() { return undefined; }, *defineProperty() { return false; },
      *getPrototypeOf() { return proto; }, *setPrototypeOf() { return false; },
      *isExtensible() { return true; }, *preventExtensions() { return true; }, *define() {}, *assign() {},
    }));
    this.#records.set(value, { kind, handle });
    return value;
  }

  *makeConstructor(tenant: Tenant, kind: BufferKind, name: string): TenantGenerator<Function> {
    const hooks = this.#hooks;
    const self = this;
    const { ObjectPrototype } = yield tenant.yieldTenant(objectPrimordial(tenant));
    const proto = yield tenant.yieldTenant(tenant.make(ObjectPrototype));
    this.#prototypes.set(kind, proto);
    const ctor = yield tenant.yieldTenant(makeBuiltin(tenant, name,
      function* () { throw new TypeError(`Constructor ${name} requires 'new'`); },
      function* (_target, args) { return yield tenant.yieldTenant(self.shell(tenant, kind, yield tenant.yieldTenant(hooks.allocate(kind, toIndex(args[0] ?? 0))))); },
      ObjectPrototype));
    yield tenant.yieldTenant(defineData(tenant, ctor, "prototype", proto, { writable: false, configurable: false }));
    return ctor;
  }
}

export function* bufferPrimordial(tenant: Tenant, hooks: BufferHooks): TenantGenerator<BufferPrimordial> {
  const cached = caches.get(tenant);
  if (cached) {
    if (cached.identity !== hooks.identity) throw new TypeError("a tenant realm may have only one BufferHooks capability");
    return cached.primordial;
  }
  const impl = new BufferPrimordialImpl(hooks);
  impl.ArrayBuffer = yield tenant.yieldTenant(impl.makeConstructor(tenant, "array-buffer", "ArrayBuffer"));
  impl.ArrayBufferPrototype = impl.prototypeOf("array-buffer");
  if (hooks.supportsSharedArrayBuffer) {
    impl.SharedArrayBuffer = yield tenant.yieldTenant(impl.makeConstructor(tenant, "shared-array-buffer", "SharedArrayBuffer"));
    impl.SharedArrayBufferPrototype = impl.prototypeOf("shared-array-buffer");
  }
  const result: BufferPrimordial = impl;
  caches.set(tenant, { identity: hooks.identity, primordial: result });
  return result;
}
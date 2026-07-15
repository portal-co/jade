import type { Tenant as Tenant_ } from "./index.ts";
import { narrow, type NarrowSpec, guestAbiMixin } from "./narrow.ts";

/** The shape of a descriptor as stored in the shadow / read from a guest-built LITOBJ. */
type GuestDescriptor = {
  value: unknown;
  writable: boolean;
  enumerable: boolean;
  configurable: boolean;
  get: (() => unknown) | undefined;
  set: ((v: unknown) => void) | undefined;
};

// Getter/setter traps are validated as functions here; their *actual* calling convention
// (if any — they might just be plain host functions) is looked up from the guest-function
// registry at invocation time (`invokeTrap`), never guessed while narrowing.
const DESCRIPTOR_SPEC: NarrowSpec<GuestDescriptor> = {
  kind: "object",
  fields: {
    value: { kind: "any" },
    // Guest-authored descriptors commonly omit these (e.g. accessor descriptors have no
    // `writable`), so they default to `false` rather than rejecting the whole descriptor.
    writable: { kind: "defaulted", inner: { kind: "typeof", tag: "boolean" }, default: false },
    enumerable: { kind: "defaulted", inner: { kind: "typeof", tag: "boolean" }, default: false },
    configurable: { kind: "defaulted", inner: { kind: "typeof", tag: "boolean" }, default: false },
    get: { kind: "optional", inner: { kind: "guestFn" } },
    set: { kind: "optional", inner: { kind: "guestFn" } },
  },
};

// Multi-tenant object manager: each tenant keeps a private WeakMap "shadow" of
// every object it creates, mapping property keys to descriptors. The object the
// outside world sees is a bare, empty shell — it is foreign by nature: host code
// and other tenants observe no properties, and the shadow is collected together
// with the object by the native GC (no manual bookkeeping required).

export class Tenant implements Tenant_ {
  // Generator method bodies yield `this.yieldTenant(...)` sentinels, so every
  // tenant implementation needs the driver helpers available on `this`.
  // Injected onto the prototype via `Object.assign(Tenant.prototype, guestAbiMixin)`
  // below (single shared implementation, not duplicated here) — `declare` tells
  // TypeScript these exist on every instance without re-initializing them per-instance.
  declare markGuestFn: Tenant_["markGuestFn"];
  declare invokeGuestAware: Tenant_["invokeGuestAware"];
  declare invokeTrap: Tenant_["invokeTrap"];
  declare createGuestGen: Tenant_["createGuestGen"];
  declare unpackGuestGen: Tenant_["unpackGuestGen"];
  declare yieldTenant: Tenant_["yieldTenant"];
  declare driveTenant: Tenant_["driveTenant"];

  #shadow: Record<`$${string}` | number | symbol, WeakMap<object, PropertyDescriptor>> = Object.create(null);

  #shadowForKey(key: PropertyKey): WeakMap<object, PropertyDescriptor> {
    return (this.#shadow[(typeof key === 'string' ? key === `${+key}` ? +key : `$${key}` : key) as any] ??= new WeakMap());
  }

  *make(proto: object | null = null) {
    const newObject = Object.create(null);
    this.#shadowForKey('__proto__').set(newObject, {
      value: proto,
      writable: true,
      enumerable: false,
      configurable: false,
    });
    return newObject;
  }

  *get<R = unknown>(obj: object, key: PropertyKey) {
    const d = this.#shadowForKey(key).get(obj);
    if (!d) return undefined as R;
    if (!("get" in d)) return d.value as R;
    // `d.get` may be a guest function (created via the FN opcode) or a plain host
    // function; invoke it respecting whichever ABI it actually has.
    if (!d.get) return undefined as R;
    return (yield this.yieldTenant(this.invokeTrap(d.get as Function, obj, []))) as R;
  }

  *set<V = unknown>(obj: object, key: PropertyKey, value: V) {
    const shadow = this.#shadowForKey(key);
    const existing = shadow.get(obj);
    // Symmetric with `get()`: if there's an existing accessor descriptor for this
    // (obj, key), route through its setter trap instead of silently clobbering it
    // with a plain data descriptor.
    if (existing && "set" in existing) {
      if (existing.set) {
        yield this.yieldTenant(this.invokeTrap(existing.set as Function, obj, [value]));
      }
      return;
    }
    shadow.set(obj, {
      value,
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }

  *has(obj: object, key: PropertyKey) {
    return this.#shadowForKey(key).has(obj);
  }

  *delete(obj: object, key: PropertyKey) {
    this.#shadowForKey(key).delete(obj);
  }

  *ownKeys(obj: object) {
    // `Object.keys(this.#shadow)` yields *internal* storage keys (non-numeric keys are
    // `$`-prefixed by `#shadowForKey`). Undo exactly that one level of prefixing before
    // looking the descriptor up again, which would otherwise double-prefix it and never
    // match anything. Only *enumerable* descriptors count as "own keys" — e.g. `make()`'s
    // internal `__proto__` bookkeeping entry is deliberately non-enumerable and must stay
    // invisible here, matching `single_tenant.ts`'s equivalent filter.
    const keys: PropertyKey[] = [];
    for (const internalKey of Object.keys(this.#shadow)) {
      const originalKey = internalKey.startsWith("$") ? internalKey.slice(1) : internalKey;
      const d = this.#shadowForKey(originalKey).get(obj);
      if (d?.enumerable) keys.push(originalKey);
    }
    return keys;
  }

  *define(target: object, descriptors: object) {
    // `descriptors` is itself a tenant-managed object whose values are
    // descriptor objects; read it through this tenant.
    const keys = yield this.yieldTenant(this.ownKeys(descriptors));
    for (const k of keys as PropertyKey[]) {
      const raw = yield this.yieldTenant(this.get(descriptors, k));
      // Validate + convert the guest-controlled descriptor shape instead of `as`-casting
      // fields sight-unseen; also registers any get/set guest-function ABI along the way.
      const n = narrow<GuestDescriptor>(DESCRIPTOR_SPEC, raw, this);
      if (n) this.#shadowForKey(k).set(target, n.value as PropertyDescriptor);
    }
  }

  *assign(dst: object, src: object) {
    const keys = yield this.yieldTenant(this.ownKeys(src));
    for (const k of keys as PropertyKey[]) {
      yield this.yieldTenant(
        this.set(dst, k, (yield this.yieldTenant(this.get(src, k))) as PropertyDescriptor),
      );
    }
  }

}
Object.assign(Tenant.prototype, guestAbiMixin);

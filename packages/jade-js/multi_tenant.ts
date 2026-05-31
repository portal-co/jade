import type { Tenant as Tenant_ } from "./index.ts";

// Multi-tenant object manager: each tenant keeps a private WeakMap "shadow" of
// every object it creates, mapping property keys to descriptors. The object the
// outside world sees is a bare, empty shell — it is foreign by nature: host code
// and other tenants observe no properties, and the shadow is collected together
// with the object by the native GC (no manual bookkeeping required).
type Shadow = Map<PropertyKey, PropertyDescriptor>;

export class Tenant implements Tenant_ {
  #shadow = new WeakMap<object, Shadow>();

  #store(obj: object): Shadow {
    let m = this.#shadow.get(obj);
    if (!m) {
      m = new Map();
      this.#shadow.set(obj, m);
    }
    return m;
  }

  make(proto: object | null = null): object {
    return Object.create(proto);
  }

  get(obj: object, key: PropertyKey): unknown {
    const d = this.#shadow.get(obj)?.get(key);
    if (!d) return undefined;
    return "get" in d ? d.get?.call(obj) : d.value;
  }

  set(obj: object, key: PropertyKey, value: unknown): void {
    this.#store(obj).set(key, {
      value,
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }

  has(obj: object, key: PropertyKey): boolean {
    return this.#shadow.get(obj)?.has(key) ?? false;
  }

  delete(obj: object, key: PropertyKey): void {
    this.#shadow.get(obj)?.delete(key);
  }

  ownKeys(obj: object): PropertyKey[] {
    const m = this.#shadow.get(obj);
    if (!m) return [];
    return [...m.keys()].filter((k) => m.get(k)!.enumerable);
  }

  define(target: object, descriptors: object): void {
    const store = this.#store(target);
    // `descriptors` is itself a tenant-managed object whose values are
    // descriptor objects; read it through this tenant.
    for (const k of this.ownKeys(descriptors)) {
      store.set(k, this.get(descriptors, k) as PropertyDescriptor);
    }
  }

  assign(dst: object, src: object): void {
    const m = this.#shadow.get(src);
    if (m) {
      for (const k of this.ownKeys(src)) this.set(dst, k, this.get(src, k));
    } else {
      // Foreign / native source (e.g. a host object or array spread).
      for (const k of Object.keys(src)) this.set(dst, k, (src as any)[k]);
    }
  }

}

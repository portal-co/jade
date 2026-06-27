import type { Tenant as Tenant_ } from "./index.ts";

// Multi-tenant object manager: each tenant keeps a private WeakMap "shadow" of
// every object it creates, mapping property keys to descriptors. The object the
// outside world sees is a bare, empty shell — it is foreign by nature: host code
// and other tenants observe no properties, and the shadow is collected together
// with the object by the native GC (no manual bookkeeping required).

export class Tenant implements Tenant_ {
  #shadow: Record<`$${string}` | number | symbol, WeakMap<object, PropertyDescriptor>> = Object.create(null);

  #shadowForKey(key: PropertyKey): WeakMap<object, PropertyDescriptor> {
    return (this.#shadow[(typeof key === 'string' ? key === `${+key}` ? +key : `$${key}` : key) as any] ??= new WeakMap());
  }

  make(proto: object | null = null): object {
    const newObject = Object.create(null);
    this.#shadowForKey('__proto__').set(newObject, {
      value: proto,
      writable: true,
      enumerable: false,
      configurable: false,
    });
    return newObject;
  }

  get(obj: object, key: PropertyKey): unknown {
    const d = this.#shadowForKey(key).get(obj);
    if (!d) return undefined;
    return "get" in d ? d.get?.call(obj) : d.value;
  }

  set(obj: object, key: PropertyKey, value: unknown): void {
    this.#shadowForKey(key).set(obj, {
      value,
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }

  has(obj: object, key: PropertyKey): boolean {
    return this.#shadowForKey(key).has(obj);
  }

  delete(obj: object, key: PropertyKey): void {
    this.#shadowForKey(key).delete(obj);
  }

  ownKeys(obj: object): PropertyKey[] {
    return Object.keys(this.#shadow).filter((k) => this.has(obj, k));
  }

  define(target: object, descriptors: object): void {

    // `descriptors` is itself a tenant-managed object whose values are
    // descriptor objects; read it through this tenant.
    for (const k of this.ownKeys(descriptors)) {
      const d = this.get(descriptors, k);
      if(typeof d === 'object' && d !== null) this.#shadowForKey(k).set(target, {
        value: this.get(d, 'value'),
        writable: (this.get(d, 'writable') ?? false) as boolean,
        enumerable: (this.get(d, 'enumerable') ?? false) as boolean,
        configurable: (this.get(d, 'configurable') ?? false) as boolean,
        get: (this.get(d, 'get') ?? undefined) as (() => unknown) | undefined,
        set: (this.get(d, 'set') ?? undefined) as ((v: unknown) => void) | undefined,
      });
    }
  }

  assign(dst: object, src: object): void {
    for (const k of this.ownKeys(src)) {
      this.set(dst, k, this.get(src, k) as PropertyDescriptor);
    }
  }

}

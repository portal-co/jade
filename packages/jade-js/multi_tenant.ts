import { Tenant as Tenant_ } from "./index.ts";
type Kache = { [a: string | symbol]: string | symbol };
const sym = typeof Symbol === "undefined" ? undefined : Symbol?.dispose;
const { keys } = Object;
export class Tenant implements Tenant_ {
  #kache_obj: Kache = {};
  static #needsGCHooks = false;
  static get needsGCHooks(): boolean {
    return Tenant.#needsGCHooks;
  }
  static #destructionList: { [a: string | symbol]: {} } = {};
  static #destroyKache(a: Kache) {
    for (const k in a) Tenant.#destructionList[a[k]] ??= {};
  }
  static wipe<T>(a: T): T {
    if (typeof a === "object" || typeof a === "function")
      if (a)
        for (const k in Tenant.#destructionList)
          if (k in a) delete (k as any)[k];
    return a;
  }
  #randomUUID: typeof crypto.randomUUID;
  static #finalizer: FinalizationRegistry<Kache> | undefined =
    typeof FinalizationRegistry === "undefined"
      ? undefined
      : new FinalizationRegistry(Tenant.#destroyKache.bind(Tenant));

  constructor({
    randomUUID = crypto.randomUUID.bind(crypto),
  }: { randomUUID?: typeof crypto.randomUUID } = {}) {
    Tenant.#needsGCHooks = true;
    this.#randomUUID = randomUUID;
    Tenant.#finalizer?.register(this, this.#kache_obj);
    const dispose = () => Tenant.#destroyKache(this.#kache_obj);
    if (sym) (this as any)[sym] = dispose;
    (this as any).dispose = dispose;
  }
  clean<T extends object>(object: T, a: keyof T): keyof T {
    if (typeof a === "string") {
      if (a === `${+a}`) return a;
    }
    return (this.#kache_obj[a] ??=
      typeof a === "string"
        ? this.#randomUUID()
        : Symbol(this.#randomUUID())) as any;
  }
}

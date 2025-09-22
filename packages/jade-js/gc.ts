import { MultiTenant } from "./index.ts";

const weakmapInternal = (a: object, WeakMap: typeof globalThis.WeakMap) =>
  isPolyfillWeakMap(WeakMap)
    ? ((a as any)[WeakMap.__symbol] ??=
        "__create" in WeakMap && typeof WeakMap.__create === "function"
          ? WeakMap.__create()
          : {})
    : undefined;
const isPolyfillWeakMap = (
  WeakMap: typeof globalThis.WeakMap
): WeakMap is {
  __symbol: string | symbol;
  prototype: { __id: string };
} & typeof globalThis.WeakMap =>
  ("__symbol" in WeakMap &&
    (typeof WeakMap.__symbol === "string" ||
      typeof WeakMap.__symbol === "symbol")) as any;
type GcState = {
  [a: string]: { map: boolean; objs: any[]; objSet: WeakMap<any, {}> };
};
export type MTFlags = { needsGCHooks: boolean };
export class GCReactor {
  #weakMap: typeof WeakMap;
  #object: typeof Object;
  #roots: any[] = [];
  #rootSet: WeakMap<any, {}> | undefined = undefined;
  #markSet: WeakMap<any, {}> | undefined = undefined;
  #markObjs: any[] = [];
  #wipe: <T>(a: T) => T;
  #mtFlags: MTFlags;
  constructor({
    WeakMap = globalThis.WeakMap,
    Object = { ...globalThis.Object } as any,
    wipe = MultiTenant.wipe.bind(MultiTenant),
    flags = MultiTenant,
  }: {
    WeakMap?: typeof globalThis.WeakMap;
    Object?: typeof globalThis.Object;
    wipe?: <T>(a: T) => T;
    flags?: MTFlags;
  } = {}) {
    this.#weakMap = WeakMap;
    this.#object = Object;
    this.#wipe = wipe;
    this.#mtFlags = flags;
  }
  get gc() {
    return () => this.#gc();
  }
  get root() {
    return (a: any) => this.#root(a);
  }
  get unroot() {
    return (a: any) => this.#unroot(a);
  }
  #clean(): boolean {
    if (isPolyfillWeakMap(this.#weakMap)) return false;
    if (this.#mtFlags.needsGCHooks) return false;
    return true;
  }
  #root(a: any) {
    if (typeof a !== "object" && typeof a !== "function") return;
    if (this.#clean()) return;
    const rootSet = (this.#rootSet ??= new this.#weakMap());
    if (rootSet.has(a)) return;
    rootSet.set(a, {});
    this.#roots = [a, ...this.#roots];
  }
  #unroot(a: any) {
    if (typeof a !== "object" && typeof a !== "function") return;
    if (this.#clean()) return;
    const rootSet = (this.#rootSet ??= new this.#weakMap());
    rootSet.delete(a);
    this.#roots = this.#roots.filter((a) => rootSet.has(a));
  }
  #gc() {
    if (this.#clean()) return;
    const rootSet = (this.#rootSet ??= new this.#weakMap());
    const roots = (this.#roots = this.#roots.filter((a) => rootSet.has(a)));
    const state = {};
    for (const rk in roots) this.#mark(roots[rk], state);
    this.#sweep(state);
  }
  #mark(a: any, state: GcState) {
    if (this.#clean()) return;
    const markSet = (this.#markSet ??= new this.#weakMap());
    if (typeof a !== "object" && typeof a !== "function") {
      const proto = this.#object.getPrototypeOf(a);
      if (proto) this.#mark(proto, state);
      return;
    }
    if (markSet.has(a)) return;
    markSet.set(a, {});
    this.#wipe(a);
    this.#markObjs = [a, ...this.#markObjs];
    const proto = this.#object.getPrototypeOf(a);
    if (proto) this.#mark(proto, state);
    if (isPolyfillWeakMap(this.#weakMap) && proto === this.#weakMap.prototype) {
      const r = (state[(a as any).__id] ??= {
        map: false,
        objs: [],
        objSet: new this.#weakMap(),
      });
      r.map = true;
      for (const obj in r.objs) {
        this.#mark(r.objs[obj], state);
      }
    }
    const d = this.#object.getOwnPropertyDescriptors(a);
    for (const key in d) {
      if (isPolyfillWeakMap(this.#weakMap) && key === this.#weakMap.__symbol) {
        for (var k in d.value)
          if (k !== (markSet as any).__id) {
            const r = (state[k] ??= {
              map: false,
              objs: [],
              objSet: new this.#weakMap(),
            });
            if (!r.objSet.has(a)) {
              r.objSet.set(a, {});
              r.objs = [a, ...r.objs];
            }
            if (r.map) {
              this.#mark((d.value as any)[k], state);
            }
          }
      } else {
        for (const k2 in { value: true, get: true, set: true })
          if (k2 in d[key]) {
            this.#mark((d[key] as any)[k2], state);
          }
      }
    }
  }
  #sweep(state: GcState) {
    if (this.#clean()) return;
    const markSet = (this.#markSet ??= new this.#weakMap());
    for (var key in state) {
      if (!state[key].map) {
        for (const obj_key in state[key].objs) {
          const obj = state[key].objs[obj_key];
          if (!markSet.has(obj)) {
            const val = weakmapInternal(obj, this.#weakMap);
            if (val) {
              delete val[key];
              delete val[(state[key].objSet as any).__id];
            }
          }
        }
      }
    }
    for (const x of this.#markObjs) {
      markSet.delete(x);
    }
    this.#markObjs = [];
  }
}

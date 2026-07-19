import type {
  Tenant as Tenant_, TenantCallableExoticHandler, TenantExoticHandler,
  TenantInvocation,
} from "./index.ts";
import { narrow, type NarrowSpec, guestAbiMixin, type GuestFnMeta } from "./narrow.ts";

type GuestDescriptor = {
  value: unknown;
  writable: boolean;
  enumerable: boolean;
  configurable: boolean;
  get: (() => unknown) | undefined;
  set: ((v: unknown) => void) | undefined;
};

const DESCRIPTOR_SPEC: NarrowSpec<GuestDescriptor> = {
  kind: "object",
  fields: {
    value: { kind: "any" },
    writable: { kind: "defaulted", inner: { kind: "typeof", tag: "boolean" }, default: false },
    enumerable: { kind: "defaulted", inner: { kind: "typeof", tag: "boolean" }, default: false },
    configurable: { kind: "defaulted", inner: { kind: "typeof", tag: "boolean" }, default: false },
    get: { kind: "optional", inner: { kind: "guestFn" } },
    set: { kind: "optional", inner: { kind: "guestFn" } },
  },
};

type ExoticMeta = {
  handler: TenantExoticHandler;
  callable?: TenantCallableExoticHandler;
};

/**
 * Isolated tenant representation.  Guest-visible own properties stay in private
 * weak shadow maps for both object and function shells.  Exotic metadata is a
 * separate WeakMap and is never visible to guest ownKeys or raw host inspection.
 */
export class Tenant implements Tenant_ {
  declare markGuestFn: Tenant_["markGuestFn"];
  declare invokeGuestAware: Tenant_["invokeGuestAware"];
  declare invokeTrap: Tenant_["invokeTrap"];
  declare createGuestGen: Tenant_["createGuestGen"];
  declare unpackGuestGen: Tenant_["unpackGuestGen"];
  declare yieldTenant: Tenant_["yieldTenant"];
  declare driveTenant: Tenant_["driveTenant"];

  #shadow: Record<`$${string}` | number | symbol, WeakMap<object, PropertyDescriptor>> = Object.create(null);
  #keys = new Set<PropertyKey>();
  #owned = new WeakSet<object>();
  #exotics = new WeakMap<object, ExoticMeta>();

  #shadowForKey(key: PropertyKey): WeakMap<object, PropertyDescriptor> {
    this.#keys.add(key);
    return (this.#shadow[(typeof key === "string" ? key === `${+key}` ? +key : `$${key}` : key) as any] ??= new WeakMap());
  }

  #missing(operation: string): never {
    throw new TypeError(`tenant exotic has no ${operation} trap`);
  }

  ownsObject(value: object): boolean {
    return this.#owned.has(value);
  }

  *make(proto: object | null = null) {
    const out = Object.create(null);
    this.#owned.add(out);
    this.#shadowForKey("__proto__").set(out, {
      value: proto, writable: true, enumerable: false, configurable: false,
    });
    return out;
  }

  *makeFunction(
    implementation: Function,
    options: { proto?: object | null; guestMeta?: GuestFnMeta } = {},
  ) {
    this.#owned.add(implementation);
    if (options.guestMeta) this.markGuestFn(implementation, options.guestMeta);
    this.#shadowForKey("__proto__").set(implementation, {
      value: options.proto ?? null, writable: true, enumerable: false, configurable: false,
    });
    return implementation;
  }

  *makeExotic(proto: object | null | undefined, handler: TenantExoticHandler) {
    const out = (yield this.yieldTenant(this.make(proto ?? null))) as object;
    this.#exotics.set(out, { handler });
    return out;
  }

  *makeCallableExotic(proto: object | null | undefined, handler: TenantCallableExoticHandler) {
    const tenant = this;
    let shell!: Function;
    // This shell exists for host function identity/callability. VM calls go through
    // invoke(), while direct host calls use the same trap under the sync driver.
    shell = function (this: unknown, ...args: unknown[]) {
      if (new.target) {
        return tenant.driveTenant(
          tenant.#callExotic(shell, { kind: "construct", args, newTarget: new.target }), false, false,
        );
      }
      return tenant.driveTenant(
        tenant.#callExotic(shell, { kind: "apply", thisArg: this, args }), false, false,
      );
    };
    yield this.yieldTenant(this.makeFunction(shell, { proto: proto ?? null }));
    this.#exotics.set(shell, { handler, callable: handler });
    return shell;
  }

  *#callExotic(callee: Function, invocation: TenantInvocation): Generator<any, unknown, any> {
    const meta = this.#exotics.get(callee);
    const handler = meta?.callable;
    if (!handler) return this.#missing("call");
    if (invocation.kind === "apply") {
      if (!handler.apply) return this.#missing("apply");
      return yield this.yieldTenant(handler.apply(callee, invocation.thisArg, invocation.args));
    }
    if (!handler.construct) return this.#missing("construct");
    return yield this.yieldTenant(handler.construct(callee, invocation.newTarget, invocation.args));
  }

  *invoke(callee: Function, invocation: TenantInvocation) {
    if (this.#exotics.get(callee)?.callable) {
      return yield this.yieldTenant(this.#callExotic(callee, invocation));
    }
    const args = invocation.args;
    return yield this.yieldTenant(this.invokeGuestAware(
      callee,
      invocation.kind === "apply" ? invocation.thisArg : undefined,
      args,
      invocation,
    ));
  }

  *get<R = unknown>(obj: object, key: PropertyKey) {
    const exotic = this.#exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.get) return this.#missing("get");
      return (yield this.yieldTenant(exotic.handler.get(obj, key))) as R;
    }
    const d = this.#shadowForKey(key).get(obj);
    if (!d) return undefined as R;
    if (!("get" in d)) return d.value as R;
    if (!d.get) return undefined as R;
    return (yield this.yieldTenant(this.invokeTrap(d.get as Function, obj, []))) as R;
  }

  *set<V = unknown>(obj: object, key: PropertyKey, value: V) {
    const exotic = this.#exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.set) return this.#missing("set");
      return yield this.yieldTenant(exotic.handler.set(obj, key, value));
    }
    const shadow = this.#shadowForKey(key);
    const existing = shadow.get(obj);
    if (existing && "set" in existing) {
      if (existing.set) yield this.yieldTenant(this.invokeTrap(existing.set as Function, obj, [value]));
      return;
    }
    shadow.set(obj, { value, writable: true, enumerable: true, configurable: true });
  }

  *has(obj: object, key: PropertyKey) {
    const exotic = this.#exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.has) return this.#missing("has");
      return yield this.yieldTenant(exotic.handler.has(obj, key));
    }
    return this.#shadowForKey(key).has(obj);
  }

  *delete(obj: object, key: PropertyKey) {
    const exotic = this.#exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.delete) return this.#missing("delete");
      return yield this.yieldTenant(exotic.handler.delete(obj, key));
    }
    this.#shadowForKey(key).delete(obj);
  }

  *ownKeys(obj: object) {
    const exotic = this.#exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.ownKeys) return this.#missing("ownKeys");
      return yield this.yieldTenant(exotic.handler.ownKeys(obj));
    }
    const keys: PropertyKey[] = [];
    for (const key of this.#keys) {
      if (this.#shadowForKey(key).get(obj)?.enumerable) keys.push(key);
    }
    return keys;
  }

  *define(target: object, descriptors: object) {
    const exotic = this.#exotics.get(target);
    if (exotic) {
      if (!exotic.handler.define) return this.#missing("define");
      return yield this.yieldTenant(exotic.handler.define(target, descriptors));
    }
    const keys = yield this.yieldTenant(this.ownKeys(descriptors));
    for (const k of keys as PropertyKey[]) {
      const raw = yield this.yieldTenant(this.get(descriptors, k));
      const n = narrow<GuestDescriptor>(DESCRIPTOR_SPEC, raw, this);
      if (n) this.#shadowForKey(k).set(target, n.value as PropertyDescriptor);
    }
  }

  *assign(dst: object, src: object) {
    const exotic = this.#exotics.get(dst);
    if (exotic) {
      if (!exotic.handler.assign) return this.#missing("assign");
      return yield this.yieldTenant(exotic.handler.assign(dst, src));
    }
    const keys = yield this.yieldTenant(this.ownKeys(src));
    for (const k of keys as PropertyKey[]) {
      yield this.yieldTenant(this.set(dst, k, yield this.yieldTenant(this.get(src, k))));
    }
  }
}
Object.assign(Tenant.prototype, guestAbiMixin);
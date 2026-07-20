import { isPolyfillKey } from "@portal-solutions/semble-common";
import {
  isCamoKey, type Tenant, type TenantCallableExoticHandler,
  type TenantExoticHandler, type TenantInvocation, type TenantPropertyDescriptor,
} from "./types.ts";
import { guestAbiMixin, type GuestFnMeta } from "./narrow.ts";

export function dirty(a: any): boolean {
  return isPolyfillKey(a) || isCamoKey(a);
}
export function clean<T extends object>(object: T, a: keyof T): keyof T {
  if (!dirty(a)) return a;
  if (typeof a === "string") {
    if (isPolyfillKey(a) || (object === globalThis && isCamoKey(a))) return `$$jade_js$$${a}` as any;
  }
  return a;
}

type ExoticMeta = { handler: TenantExoticHandler; callable?: TenantCallableExoticHandler };
const owned = new WeakSet<object>();
const exotics = new WeakMap<object, ExoticMeta>();
const missing = (operation: string): never => {
  throw new TypeError(`tenant exotic has no ${operation} trap`);
};

/** Native single-tenant representation with fail-closed exotic dispatch. */
export const single_tenant: Tenant & { ownsObject(value: object): boolean } = Object.assign({
  *make(proto: object | null = null) {
    const out = Object.create(proto);
    owned.add(out);
    return out;
  },
  *makeFunction(implementation: Function, options: { proto?: object | null; guestMeta?: GuestFnMeta } = {}) {
    owned.add(implementation);
    if (options.guestMeta) this.markGuestFn(implementation, options.guestMeta);
    if (options.proto !== undefined) Object.setPrototypeOf(implementation, options.proto);
    return implementation;
  },
  *makeExotic(proto: object | null | undefined, handler: TenantExoticHandler) {
    const out = Object.create(proto ?? null);
    owned.add(out);
    exotics.set(out, { handler });
    return out;
  },
  *makeCallableExotic(proto: object | null | undefined, handler: TenantCallableExoticHandler) {
    const tenant = this as Tenant;
    let shell!: Function;
    shell = function (this: unknown, ...args: unknown[]) {
      const invocation: TenantInvocation = new.target
        ? { kind: "construct", args, newTarget: new.target }
        : { kind: "apply", thisArg: this, args };
      return tenant.driveTenant(tenant.invoke(shell, invocation), false, false);
    };
    if (proto !== undefined) Object.setPrototypeOf(shell, proto);
    owned.add(shell);
    exotics.set(shell, { handler, callable: handler });
    return shell;
  },
  *invoke(callee: Function, invocation: TenantInvocation) {
    const exotic = exotics.get(callee)?.callable;
    if (exotic) {
      if (invocation.kind === "apply") {
        if (!exotic.apply) return missing("apply");
        return yield this.yieldTenant(exotic.apply(callee, invocation.thisArg, invocation.args));
      }
      if (!exotic.construct) return missing("construct");
      return yield this.yieldTenant(exotic.construct(callee, invocation.newTarget, invocation.args));
    }
    return yield this.yieldTenant(this.invokeGuestAware(
      callee,
      invocation.kind === "apply" ? invocation.thisArg : undefined,
      invocation.args,
      invocation,
    ));
  },
  ownsObject(value: object) { return owned.has(value); },
  *get<R = unknown>(obj: object, key: PropertyKey) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.get) return missing("get");
      return (yield this.yieldTenant(exotic.handler.get(obj, key))) as R;
    }
    return (obj as any)[clean(obj as any, key as any)] as R;
  },
  *set<V = unknown>(obj: object, key: PropertyKey, value: V) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.set) return missing("set");
      return yield this.yieldTenant(exotic.handler.set(obj, key, value));
    }
    (obj as any)[clean(obj as any, key as any)] = value;
  },
  *has(obj: object, key: PropertyKey) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.has) return missing("has");
      return yield this.yieldTenant(exotic.handler.has(obj, key));
    }
    return clean(obj as any, key as any) in obj;
  },
  *delete(obj: object, key: PropertyKey) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.delete) return missing("delete");
      return yield this.yieldTenant(exotic.handler.delete(obj, key));
    }
    delete (obj as any)[clean(obj as any, key as any)];
  },
  *ownKeys(obj: object) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.ownKeys) return missing("ownKeys");
      return yield this.yieldTenant(exotic.handler.ownKeys(obj));
    }
    return Reflect.ownKeys(obj).filter((k) => Object.getOwnPropertyDescriptor(obj, k)?.enumerable);
  },
  *ownPropertyKeys(obj: object) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.ownPropertyKeys) return missing("ownPropertyKeys");
      return yield this.yieldTenant(exotic.handler.ownPropertyKeys(obj));
    }
    return Reflect.ownKeys(obj).map((key) => clean(obj as any, key as any));
  },
  *getOwnPropertyDescriptor(obj: object, key: PropertyKey) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.getOwnPropertyDescriptor) return missing("getOwnPropertyDescriptor");
      return yield this.yieldTenant(exotic.handler.getOwnPropertyDescriptor(obj, key));
    }
    const descriptor = Object.getOwnPropertyDescriptor(obj, clean(obj as any, key as any));
    return descriptor && ({ ...descriptor } as TenantPropertyDescriptor);
  },
  *defineProperty(obj: object, key: PropertyKey, descriptor: TenantPropertyDescriptor) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.defineProperty) return missing("defineProperty");
      return yield this.yieldTenant(exotic.handler.defineProperty(obj, key, descriptor));
    }
    try {
      const out: PropertyDescriptor = {
        enumerable: descriptor.enumerable ?? false,
        configurable: descriptor.configurable ?? false,
      };
      if (descriptor.get !== undefined || descriptor.set !== undefined) {
        if (descriptor.get !== undefined) out.get = descriptor.get as () => unknown;
        if (descriptor.set !== undefined) out.set = descriptor.set as (value: unknown) => void;
      } else {
        out.value = descriptor.value;
        out.writable = descriptor.writable ?? false;
      }
      Object.defineProperty(
        obj,
        clean(obj as any, key as any) as any,
        out,
      );
      return true;
    } catch (error) {
      if (error instanceof TypeError) return false;
      throw error;
    }
  },
  *getPrototypeOf(obj: object) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.getPrototypeOf) return missing("getPrototypeOf");
      return yield this.yieldTenant(exotic.handler.getPrototypeOf(obj));
    }
    return Object.getPrototypeOf(obj);
  },
  *setPrototypeOf(obj: object, prototype: object | null) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.setPrototypeOf) return missing("setPrototypeOf");
      return yield this.yieldTenant(exotic.handler.setPrototypeOf(obj, prototype));
    }
    try {
      Object.setPrototypeOf(obj, prototype);
      return true;
    } catch (error) {
      if (error instanceof TypeError) return false;
      throw error;
    }
  },
  *isExtensible(obj: object) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.isExtensible) return missing("isExtensible");
      return yield this.yieldTenant(exotic.handler.isExtensible(obj));
    }
    return Object.isExtensible(obj);
  },
  *preventExtensions(obj: object) {
    const exotic = exotics.get(obj);
    if (exotic) {
      if (!exotic.handler.preventExtensions) return missing("preventExtensions");
      return yield this.yieldTenant(exotic.handler.preventExtensions(obj));
    }
    return Object.preventExtensions(obj) === obj;
  },
  *define(target: object, descriptors: object) {
    const exotic = exotics.get(target);
    if (exotic) {
      if (!exotic.handler.define) return missing("define");
      return yield this.yieldTenant(exotic.handler.define(target, descriptors));
    }
    const keys = yield this.yieldTenant(this.ownKeys(descriptors));
    for (const k of keys as PropertyKey[]) {
      Object.defineProperty(target, clean(target as any, k as any) as any,
        (yield this.yieldTenant(this.get(descriptors, k))) as PropertyDescriptor);
    }
  },
  *assign(dst: object, src: object) {
    const exotic = exotics.get(dst);
    if (exotic) {
      if (!exotic.handler.assign) return missing("assign");
      return yield this.yieldTenant(exotic.handler.assign(dst, src));
    }
    const keys = yield this.yieldTenant(this.ownKeys(src));
    for (const k of keys as PropertyKey[]) {
      yield this.yieldTenant(this.set(dst, k, yield this.yieldTenant(this.get(src, k))));
    }
  },
}, guestAbiMixin);
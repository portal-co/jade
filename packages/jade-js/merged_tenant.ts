import type {
  Tenant, TenantCallableExoticHandler, TenantExoticHandler, TenantInvocation,
  TenantProvider,
} from "./index.ts";
import { guestAbiMixin } from "./narrow.ts";

/**
 * Routes independently-owned tenant objects through one execution-facing tenant.
 * The primary is the bytecode allocation/result context; ownership remains with
 * each provider and is discovered only by its pure ownsObject predicate.
 */
export class MergedTenant implements Tenant, TenantProvider {
  declare markGuestFn: Tenant["markGuestFn"];
  declare invokeGuestAware: Tenant["invokeGuestAware"];
  declare invokeTrap: Tenant["invokeTrap"];
  declare createGuestGen: Tenant["createGuestGen"];
  declare unpackGuestGen: Tenant["unpackGuestGen"];
  declare yieldTenant: Tenant["yieldTenant"];
  declare driveTenant: Tenant["driveTenant"];

  readonly primary: TenantProvider;
  readonly providers: readonly TenantProvider[];
  #bridges = new WeakMap<object, WeakMap<object, WeakMap<object, object>>>();
  #bridgeInfo = new WeakMap<object, { source: object; owner: TenantProvider }>();

  constructor({ primary, providers }: { primary: TenantProvider; providers: readonly TenantProvider[] }) {
    if (!providers.includes(primary)) {
      throw new TypeError("MergedTenant providers must include primary");
    }
    if (new Set(providers).size !== providers.length) {
      throw new TypeError("MergedTenant providers must be unique");
    }
    this.primary = primary;
    this.providers = providers;
  }

  /** Allows this router itself to join an outer MergedTenant. */
  ownsObject(value: object): boolean {
    return this.#owner(value) !== undefined;
  }

  #owner(value: object): TenantProvider | undefined {
    let owner: TenantProvider | undefined;
    for (const provider of this.providers) {
      if (!provider.ownsObject(value)) continue;
      if (owner && owner !== provider) {
        throw new TypeError("object is claimed by multiple merged tenant providers");
      }
      owner = provider;
    }
    return owner;
  }

  #requireOwner(value: object): TenantProvider {
    return this.#owner(value) ?? (() => { throw new TypeError("object is not owned by this MergedTenant"); })();
  }

  *#marshal(value: unknown, from: TenantProvider, to: TenantProvider): Generator<any, unknown, any> {
    if (value === null || (typeof value !== "object" && typeof value !== "function")) return value;
    const raw = value as object;
    const known = this.#bridgeInfo.get(raw);
    const object = known?.source ?? raw;
    const canonicalOwner = known?.owner ?? this.#requireOwner(raw);
    // A returned bridge is already known by its destination provider. Its owner
    // is that destination; preserving the canonical source through nested router
    // use is supplied by bridge caching rather than a raw foreign-value leak.
    if (canonicalOwner === to) return object;
    const source = object;
    let bySourceProvider = this.#bridges.get(source);
    if (!bySourceProvider) this.#bridges.set(source, bySourceProvider = new WeakMap());
    let byDestination = bySourceProvider.get(canonicalOwner as object);
    if (!byDestination) bySourceProvider.set(canonicalOwner as object, byDestination = new WeakMap());
    const cached = byDestination.get(to as object);
    if (cached) return cached;

    const bridge = this.#bridgeHandler(source, canonicalOwner, to);
    const made = typeof source === "function"
      ? yield to.yieldTenant(to.makeCallableExotic(null, bridge as TenantCallableExoticHandler))
      : yield to.yieldTenant(to.makeExotic(null, bridge));
    byDestination.set(to as object, made as object);
    this.#bridgeInfo.set(made as object, { source, owner: canonicalOwner });
    return made;
  }

  #bridgeHandler(source: object, sourceOwner: TenantProvider, destination: TenantProvider): TenantExoticHandler {
    const marshalIn = function* (this: MergedTenant, value: unknown) {
      return yield this.yieldTenant(this.#marshal(value, destination, sourceOwner));
    };
    const marshalOut = function* (this: MergedTenant, value: unknown) {
      return yield this.yieldTenant(this.#marshal(value, sourceOwner, destination));
    };
    const router = this;
    const handler: TenantExoticHandler = {
      *get(_receiver, key) {
        const inKey = yield router.yieldTenant(marshalIn.call(router, key));
        const result = yield sourceOwner.yieldTenant(sourceOwner.get(source, inKey as PropertyKey));
        return yield router.yieldTenant(marshalOut.call(router, result));
      },
      *set(_receiver, key, value) {
        const inKey = yield router.yieldTenant(marshalIn.call(router, key));
        const inValue = yield router.yieldTenant(marshalIn.call(router, value));
        yield sourceOwner.yieldTenant(sourceOwner.set(source, inKey as PropertyKey, inValue));
      },
      *has(_receiver, key) {
        const inKey = yield router.yieldTenant(marshalIn.call(router, key));
        return yield sourceOwner.yieldTenant(sourceOwner.has(source, inKey as PropertyKey));
      },
      *delete(_receiver, key) {
        const inKey = yield router.yieldTenant(marshalIn.call(router, key));
        yield sourceOwner.yieldTenant(sourceOwner.delete(source, inKey as PropertyKey));
      },
      *ownKeys() {
        return yield sourceOwner.yieldTenant(sourceOwner.ownKeys(source));
      },
      *define(_receiver, descriptors) {
        const incoming = yield router.yieldTenant(marshalIn.call(router, descriptors));
        yield sourceOwner.yieldTenant(sourceOwner.define(source, incoming as object));
      },
      *assign(_receiver, value) {
        const incoming = yield router.yieldTenant(marshalIn.call(router, value));
        yield sourceOwner.yieldTenant(sourceOwner.assign(source, incoming as object));
      },
    };
    if (typeof source !== "function") return handler;
    return Object.assign(handler, {
      *apply(_receiver: Function, thisArg: unknown, args: readonly unknown[]) {
        const marshalledThis = yield router.yieldTenant(marshalIn.call(router, thisArg));
        const marshalledArgs: unknown[] = [];
        for (const arg of args) marshalledArgs.push(yield router.yieldTenant(marshalIn.call(router, arg)));
        const result = yield sourceOwner.yieldTenant(sourceOwner.invoke(source as Function, {
          kind: "apply", thisArg: marshalledThis, args: marshalledArgs,
        }));
        return yield router.yieldTenant(marshalOut.call(router, result));
      },
      *construct(_receiver: Function, newTarget: Function, args: readonly unknown[]) {
        const marshalledArgs: unknown[] = [];
        for (const arg of args) marshalledArgs.push(yield router.yieldTenant(marshalIn.call(router, arg)));
        const target = yield router.yieldTenant(marshalIn.call(router, newTarget));
        const result = yield sourceOwner.yieldTenant(sourceOwner.invoke(source as Function, {
          kind: "construct", args: marshalledArgs, newTarget: target as Function,
        }));
        return yield router.yieldTenant(marshalOut.call(router, result));
      },
    });
  }

  *make(proto: object | null = null) { return yield this.primary.yieldTenant(this.primary.make(proto)); }
  *makeFunction(implementation: Function, options?: { proto?: object | null; guestMeta?: any }) {
    return yield this.primary.yieldTenant(this.primary.makeFunction(implementation, options));
  }
  *makeExotic(proto: object | null | undefined, handler: TenantExoticHandler) {
    return yield this.primary.yieldTenant(this.primary.makeExotic(proto, handler));
  }
  *makeCallableExotic(proto: object | null | undefined, handler: TenantCallableExoticHandler) {
    return yield this.primary.yieldTenant(this.primary.makeCallableExotic(proto, handler));
  }

  *get<R = unknown>(obj: object, key: PropertyKey) {
    const owner = this.#requireOwner(obj);
    const value = yield owner.yieldTenant(owner.get(obj, key));
    return (yield this.yieldTenant(this.#marshal(value, owner, this.primary))) as R;
  }
  *set(obj: object, key: PropertyKey, value: unknown) {
    const owner = this.#requireOwner(obj);
    const incoming = yield this.yieldTenant(this.#marshal(value, this.primary, owner));
    yield owner.yieldTenant(owner.set(obj, key, incoming));
  }
  *has(obj: object, key: PropertyKey) {
    const owner = this.#requireOwner(obj);
    return yield owner.yieldTenant(owner.has(obj, key));
  }
  *delete(obj: object, key: PropertyKey) {
    const owner = this.#requireOwner(obj);
    return yield owner.yieldTenant(owner.delete(obj, key));
  }
  *ownKeys(obj: object) {
    const owner = this.#requireOwner(obj);
    return yield owner.yieldTenant(owner.ownKeys(obj));
  }
  *define(target: object, descriptors: object) {
    const owner = this.#requireOwner(target);
    const incoming = yield this.yieldTenant(this.#marshal(descriptors, this.primary, owner));
    yield owner.yieldTenant(owner.define(target, incoming as object));
  }
  *assign(dst: object, src: object) {
    const owner = this.#requireOwner(dst);
    const incoming = yield this.yieldTenant(this.#marshal(src, this.primary, owner));
    yield owner.yieldTenant(owner.assign(dst, incoming as object));
  }
  *invoke(callee: Function, invocation: TenantInvocation) {
    const owner = this.#requireOwner(callee);
    const args: unknown[] = [];
    for (const arg of invocation.args) args.push(yield this.yieldTenant(this.#marshal(arg, this.primary, owner)));
    const forwarded: TenantInvocation = invocation.kind === "apply"
      ? { kind: "apply", thisArg: yield this.yieldTenant(this.#marshal(invocation.thisArg, this.primary, owner)), args }
      : { kind: "construct", newTarget: (yield this.yieldTenant(this.#marshal(invocation.newTarget, this.primary, owner))) as unknown as Function, args };
    const result = yield owner.yieldTenant(owner.invoke(callee, forwarded));
    return yield this.yieldTenant(this.#marshal(result, owner, this.primary));
  }
}
Object.assign(MergedTenant.prototype, guestAbiMixin);
import { isPolyfillKey } from "@portal-solutions/semble-common";
import type { GuestFnMeta } from "./narrow.ts";
import type { FnResult } from "./rewrite.ts";

export { isPolyfillKey };

export function isCamoKey(a: any): boolean {
  return (
    a === "location" ||
    a === "eval" ||
    a === "parent" ||
    a === "top" ||
    a === "ActiveXObject"
  );
}

/**
 * Symbol tag on a value yielded by a tenant method to ask the driver to
 * recursively compose an inner tenant operation.
 */
export const TENANT_OP: unique symbol = Symbol.for("jade.tenantOp");

/** A value yielded by a tenant method to ask the driver to recursively drive an
 * inner tenant operation with the same ambient `addAsync`/`addGen` flags. */
export interface TenantOp<T> {
  readonly [TENANT_OP]: Generator<any, T, any>;
}

type TenantYield = TenantOp<any> | PromiseLike<any> | Iterator<any, any, any>;

/** Generator shape returned by every tenant operation method. */
export type TenantGenerator<R> = Generator<any, R, any>;

/** The boundary type produced by `tenant.driveTenant(...)` for a given operation
 *  result `R` and the ambient `addAsync`/`addGen` flags — the same declared-bits
 *  OR ambient-bits rule used by `FnResult`. */
export type TenantOpResult<R, AA extends boolean, AG extends boolean> = FnResult<
  R,
  AA,
  AG
>;

export type TenantInvocation =
  | { kind: "apply"; thisArg: unknown; args: readonly unknown[] }
  | { kind: "construct"; args: readonly unknown[]; newTarget: Function };

export interface TenantPropertyDescriptor {
  value?: unknown;
  writable?: boolean;
  get?: Function;
  set?: Function;
  enumerable?: boolean;
  configurable?: boolean;
}

/** Proxy-trap-like behavior for a tenant-managed exotic. Missing traps fail closed. */
export interface TenantExoticHandler {
  get?(receiver: object, key: PropertyKey): TenantGenerator<unknown>;
  set?(receiver: object, key: PropertyKey, value: unknown): TenantGenerator<void>;
  has?(receiver: object, key: PropertyKey): TenantGenerator<boolean>;
  delete?(receiver: object, key: PropertyKey): TenantGenerator<void>;
  ownKeys?(receiver: object): TenantGenerator<PropertyKey[]>;
  /** All own keys, including non-enumerable keys. */
  ownPropertyKeys?(receiver: object): TenantGenerator<PropertyKey[]>;
  getOwnPropertyDescriptor?(
    receiver: object,
    key: PropertyKey,
  ): TenantGenerator<TenantPropertyDescriptor | undefined>;
  defineProperty?(
    receiver: object,
    key: PropertyKey,
    descriptor: TenantPropertyDescriptor,
  ): TenantGenerator<boolean>;
  getPrototypeOf?(receiver: object): TenantGenerator<object | null>;
  setPrototypeOf?(receiver: object, prototype: object | null): TenantGenerator<boolean>;
  isExtensible?(receiver: object): TenantGenerator<boolean>;
  preventExtensions?(receiver: object): TenantGenerator<boolean>;
  define?(receiver: object, descriptors: object): TenantGenerator<void>;
  assign?(receiver: object, source: object): TenantGenerator<void>;
}

/** Call/construct traps for an exotic represented by a real native function. */
export interface TenantCallableExoticHandler extends TenantExoticHandler {
  apply?(receiver: Function, thisArg: unknown, args: readonly unknown[]): TenantGenerator<unknown>;
  construct?(receiver: Function, newTarget: Function, args: readonly unknown[]): TenantGenerator<object>;
}

/** A Tenant with independent, non-trapping ownership knowledge for merging. */
export interface TenantProvider extends Tenant {
  ownsObject(value: object): boolean;
}

/**
 * A tenant is an abstract object manager: it isolates a virtual environment by
 * owning the representation of the objects it creates. Implementations are free
 * to back objects natively (single-tenant) or out-of-band in a `WeakMap`
 * shadow (multi-tenant) — the VM only ever manipulates objects through these
 * methods, never via direct property access.
 *
 * **Every tenant operation is a generator.**  Call sites (the interpreter, the JIT,
 * embedder host code) must compose them through `tenant.driveTenant(gen, addAsync,
 * addGen)`, never by calling `.next()` on a raw tenant method.  Inside tenant methods
 * nested operations are composed with `yield this.yieldTenant(innerGen)`.
 *
 * `markGuestFn` / `invokeGuestAware` / `invokeTrap` / `createGuestGen` /
 * `unpackGuestGen` / `yieldTenant` / `driveTenant` are the guest/host ABI + driver
 * helpers (see `narrow.ts`, `shims.ts`, `driver.ts` for their single shared
 * implementations, injected onto every `Tenant` implementation via `guestAbiMixin`
 * rather than duplicated).  They are attached here, rather than left as free
 * module-level imports, so that a tenant method body referencing them via `this.<method>`
 * remains inlinable by the JIT's tenant-method-inlining feature (see
 * `docs/pluggable-tenant-interface-plan.md`): the inlining scanner
 * (`crates/jade-vm-frontend/src/tenant_inline.rs`) rejects a method for referencing any
 * free (non-parameter, non-`this`-derived, non-global) identifier, since such a reference
 * can't resolve at the JIT's arbitrary splice site — putting every non-builtin dependency
 * a tenant method could need onto `this` closes that gap generally, not just for these
 * helpers specifically.
 */
export interface Tenant {
  /** Create a fresh isolated object with the given prototype (null by default). */
  make(proto?: object | null): TenantGenerator<object>;
  /** Read a property. */
  get<R = unknown>(obj: object, key: PropertyKey): TenantGenerator<R>;
  /** Write a (data) property. */
  set<V = unknown>(obj: object, key: PropertyKey, value: V): TenantGenerator<void>;
  /** Whether the object has an own property `key`. */
  has(obj: object, key: PropertyKey): TenantGenerator<boolean>;
  /** Delete an own property. */
  delete(obj: object, key: PropertyKey): TenantGenerator<void>;
  /** Own enumerable keys. */
  ownKeys(obj: object): TenantGenerator<PropertyKey[]>;
  /** All own keys, including non-enumerable keys. */
  ownPropertyKeys(obj: object): TenantGenerator<PropertyKey[]>;
  /** Read one own property descriptor without exposing host representation. */
  getOwnPropertyDescriptor(
    obj: object,
    key: PropertyKey,
  ): TenantGenerator<TenantPropertyDescriptor | undefined>;
  /** Define one own property from a tenant control descriptor. */
  defineProperty(
    obj: object,
    key: PropertyKey,
    descriptor: TenantPropertyDescriptor,
  ): TenantGenerator<boolean>;
  /** Read or update the object's prototype. */
  getPrototypeOf(obj: object): TenantGenerator<object | null>;
  setPrototypeOf(obj: object, prototype: object | null): TenantGenerator<boolean>;
  /** Extensibility operations used by Object/Reflect seal and freeze. */
  isExtensible(obj: object): TenantGenerator<boolean>;
  preventExtensions(obj: object): TenantGenerator<boolean>;
  /**
   * Apply property descriptors to `target`. `descriptors` is itself a
   * tenant-managed object mapping keys to descriptor objects (as produced by the
   * LITOBJ "define" path), so it is read through this tenant.
   */
  define(target: object, descriptors: object): TenantGenerator<void>;
  /** Copy own enumerable properties from `src` into `dst` (object spread). */
  assign(dst: object, src: object): TenantGenerator<void>;
  /** Register `fn`'s guest calling convention. See `narrow.ts`. */
  markGuestFn<F extends Function>(fn: F, meta: GuestFnMeta): F;
  /** Create a fail-closed exotic object. This is host/provider-facing only. */
  makeExotic(proto: object | null | undefined, handler: TenantExoticHandler): TenantGenerator<object>;
  /** Adopt a native callable as an owned, non-exotic tenant value. */
  makeFunction(
    implementation: Function,
    options?: { proto?: object | null; guestMeta?: GuestFnMeta },
  ): TenantGenerator<Function>;
  /** Create a callable exotic represented by a constructible native function. */
  makeCallableExotic(
    proto: object | null | undefined,
    handler: TenantCallableExoticHandler,
  ): TenantGenerator<Function>;
  /** Route normal apply or construction through the tenant callable ABI. */
  invoke(callee: Function, invocation: TenantInvocation): TenantGenerator<unknown>;
  /** Invoke `fn` respecting its actual registered ABI. See `narrow.ts`. */
  invokeGuestAware<Args extends readonly unknown[], R = unknown>(
    fn: Function,
    thisArg: unknown,
    args: Args,
    invocation?: TenantInvocation,
  ): TenantGenerator<R>;
  /** Invoke a property-descriptor getter/setter ("trap"). See `narrow.ts`. */
  invokeTrap<Args extends readonly unknown[], R = unknown>(
    fn: Function,
    receiver: object,
    args: Args,
  ): TenantGenerator<R>;
  /** Wrap a native generator as a guest-side generator object. See `shims.ts`. */
  createGuestGen<Y, Ret = unknown>(
    nativeGen: Generator<Y, Ret, unknown>,
  ): TenantGenerator<unknown>;
  /** Adapt a guest-side generator object back to the native generator protocol. See `shims.ts`. */
  unpackGuestGen(g: unknown): Generator;

  /**
   * Create the sentinel used by tenant methods to compose nested tenant ops:
   * `yield this.yieldTenant(this.get(...))`.  See `driver.ts`.
   */
  yieldTenant<T>(gen: Generator<any, T, any>): TenantOp<T>;
  /**
   * Drive a tenant-operation generator according to the effective variant, returning
   * a plain value / Promise / Generator / AsyncGenerator.  This is the single entry
   * point every backend and embedder uses.  See `driver.ts`.
   */
  driveTenant<T>(
    gen: Generator<any, T, any>,
    addAsync: boolean,
    addGen: boolean,
  ): any;
}

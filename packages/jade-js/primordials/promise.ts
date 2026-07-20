import {
  hostTaskFromPromise, hostTaskPromise, isHostTask,
  type HostAsyncCapability, type HostTask,
} from "../async-host.ts";
import type { Tenant, TenantGenerator } from "../tenants/types.ts";
import { defineData, makeBuiltin } from "./types.ts";

declare const GUEST_PROMISE: unique symbol;
/** Nominal guest value. It is never a host native Promise or thenable contract. */
export interface GuestPromise<T> { readonly [GUEST_PROMISE]: T; }

export interface PromiseRuntime {
  readonly tenant: Tenant;
  readonly async: HostAsyncCapability;
  readonly Promise: Function;
  readonly PromisePrototype: object;
  isLocalPromise(value: unknown): value is GuestPromise<unknown>;
  resolve(value: unknown): TenantGenerator<GuestPromise<unknown>>;
  reject(reason: unknown): TenantGenerator<GuestPromise<never>>;
  fromHostTask<T>(task: HostTask<T>): TenantGenerator<GuestPromise<T>>;
  toHostTask<T>(value: GuestPromise<T> | unknown): HostTask<T>;
  awaitGuest(value: unknown): HostTask<unknown>;
  /** Host-runtime-only task observation used by generated backends. */
  awaitHostTask<T>(task: HostTask<T>): Promise<T>;
}

type State = "pending" | "fulfilled" | "rejected";
type Reaction = { fulfilled?: Function; rejected?: Function; child: object };
type Record_ = { state: State; value: unknown; reactions: Reaction[]; handled: boolean; reported: boolean };
export interface PromisePrimordial { Promise: Function; PromisePrototype: object; promiseRuntime: PromiseRuntime; }

const cache = new WeakMap<Tenant, { capability: HostAsyncCapability; result: PromisePrimordial }>();

/** Tenant/realm-owned guest Promise. Host native promises remain internal HostTasks only. */
export function* promisePrimordial(tenant: Tenant, async: HostAsyncCapability): TenantGenerator<PromisePrimordial> {
  const prior = cache.get(tenant);
  if (prior) {
    if (prior.capability !== async) throw new TypeError("tenant already has an incompatible HostAsyncCapability");
    return prior.result;
  }
  const records = new WeakMap<object, Record_>();
  let PromisePrototype!: object;
  let PromiseFn!: Function;

  const isLocal = (value: unknown): value is GuestPromise<unknown> =>
    value !== null && typeof value === "object" && records.has(value as object);
  const create = (): object => {
    // Allocation still uses the Tenant representation. This synchronous runtime
    // helper composes it through driveTenant rather than touching a tenant
    // generator directly; Promise allocation itself cannot suspend.
    const shell = tenant.driveTenant(tenant.make(PromisePrototype), false, false) as object;
    records.set(shell, { state: "pending", value: undefined, reactions: [], handled: false, reported: false });
    for (const [key, value] of [["then", then], ["catch", catcher], ["finally", finally_]] as const) {
      tenant.driveTenant(defineData(tenant, shell, key, value), false, false);
    }
    return shell;
  };
  const scheduleUnhandled = (shell: object, record: Record_) => async.enqueueMicrotask(() => {
    if (record.state === "rejected" && !record.handled && !record.reported) {
      record.reported = true;
      async.onUnhandledRejection(record.value, shell);
    }
  });
  const settle = (shell: object, state: Exclude<State, "pending">, value: unknown) => {
    const record = records.get(shell)!;
    if (record.state !== "pending") return;
    record.state = state; record.value = value;
    if (state === "rejected") scheduleUnhandled(shell, record);
    for (const reaction of record.reactions.splice(0)) enqueueReaction(shell, reaction);
  };
  const resolveValue = (shell: object, value: unknown) => {
    if (value === shell) return settle(shell, "rejected", new TypeError("Promise cannot resolve itself"));
    if (isLocal(value)) {
      const source = records.get(value as object)!;
      if (source.state === "pending") {
        source.reactions.push({ child: shell });
      } else settle(shell, source.state, source.value);
      return;
    }
    if (isHostTask(async, value)) {
      async.observe(value, (v) => settle(shell, "fulfilled", v), (e) => settle(shell, "rejected", e));
      return;
    }
    if (value !== null && (typeof value === "object" || typeof value === "function")) {
      // Guest thenable assimilation is tenant-mediated. Native Promise.resolve and
      // raw property access never get to observe a guest-controlled `then`.
      let then: unknown;
      try { then = tenant.driveTenant(tenant.get(value as object, "then"), false, false); }
      catch (error) { settle(shell, "rejected", error); return; }
      if (typeof then === "function") {
        let called = false;
        const resolve = tenant.driveTenant(tenant.makeFunction((next: unknown) => {
          if (!called) { called = true; resolveValue(shell, next); }
        }), false, false);
        const reject = tenant.driveTenant(tenant.makeFunction((reason: unknown) => {
          if (!called) { called = true; settle(shell, "rejected", reason); }
        }), false, false);
        try {
          tenant.driveTenant(tenant.invoke(then, {
            kind: "apply", thisArg: value, args: [resolve, reject],
          }), false, false);
        } catch (error) { if (!called) settle(shell, "rejected", error); }
        return;
      }
    }
    settle(shell, "fulfilled", value);
  };
  const enqueueReaction = (parent: object, reaction: Reaction) => async.enqueueMicrotask(() => {
    const parentRecord = records.get(parent)!;
    const callback = parentRecord.state === "fulfilled" ? reaction.fulfilled : reaction.rejected;
    if (!callback) return settle(reaction.child, parentRecord.state as Exclude<State, "pending">, parentRecord.value);
    try {
      const result = tenant.driveTenant(tenant.invoke(callback, {
        kind: "apply", thisArg: undefined, args: [parentRecord.value],
      }), false, false);
      resolveValue(reaction.child, result);
    } catch (error) { settle(reaction.child, "rejected", error); }
  });

  PromisePrototype = yield tenant.yieldTenant(tenant.make(null));
  const then = yield tenant.yieldTenant(makeBuiltin(tenant, "then", function* (thisArg, args) {
    if (!isLocal(thisArg)) throw new TypeError("Promise.prototype.then called on incompatible receiver");
    const parent = thisArg as object, record = records.get(parent)!;
    const child = create();
    const reaction: Reaction = {
      fulfilled: typeof args[0] === "function" ? args[0] as Function : undefined,
      rejected: typeof args[1] === "function" ? args[1] as Function : undefined,
      child,
    };
    if (reaction.rejected) {
      record.handled = true;
      if (record.reported) async.onRejectionHandled?.(parent);
    }
    if (record.state === "pending") record.reactions.push(reaction); else enqueueReaction(parent, reaction);
    return child as GuestPromise<unknown>;
  }, undefined, PromisePrototype));
  const catcher = yield tenant.yieldTenant(makeBuiltin(tenant, "catch", function* (thisArg, args) {
    return yield tenant.yieldTenant(tenant.invoke(then, { kind: "apply", thisArg, args: [undefined, args[0]] }));
  }, undefined, PromisePrototype));
  const finally_ = yield tenant.yieldTenant(makeBuiltin(tenant, "finally", function* (thisArg, args) {
    const f = args[0];
    if (typeof f !== "function") return yield tenant.yieldTenant(tenant.invoke(then, { kind: "apply", thisArg, args: [undefined, undefined] }));
    const onFulfilled = yield tenant.yieldTenant(makeBuiltin(tenant, "finallyFulfilled", function* (_unused, values) {
      yield tenant.yieldTenant(tenant.invoke(f, { kind: "apply", thisArg: undefined, args: [] })); return values[0];
    }));
    const onRejected = yield tenant.yieldTenant(makeBuiltin(tenant, "finallyRejected", function* (_unused, values) {
      yield tenant.yieldTenant(tenant.invoke(f, { kind: "apply", thisArg: undefined, args: [] })); throw values[0];
    }));
    return yield tenant.yieldTenant(tenant.invoke(then, { kind: "apply", thisArg, args: [onFulfilled, onRejected] }));
  }, undefined, PromisePrototype));
  yield tenant.yieldTenant(defineData(tenant, PromisePrototype, "then", then));
  yield tenant.yieldTenant(defineData(tenant, PromisePrototype, "catch", catcher));
  yield tenant.yieldTenant(defineData(tenant, PromisePrototype, "finally", finally_));

  PromiseFn = yield tenant.yieldTenant(makeBuiltin(tenant, "Promise", function* () {
    throw new TypeError("Promise constructor requires 'new'");
  }, function* (_newTarget, args) {
    const executor = args[0];
    if (typeof executor !== "function") throw new TypeError("Promise executor is not callable");
    const shell = create();
    const resolve = yield tenant.yieldTenant(makeBuiltin(tenant, "resolve", function* (_unused, values) { resolveValue(shell, values[0]); }));
    const reject = yield tenant.yieldTenant(makeBuiltin(tenant, "reject", function* (_unused, values) { settle(shell, "rejected", values[0]); }));
    try { yield tenant.yieldTenant(tenant.invoke(executor, { kind: "apply", thisArg: undefined, args: [resolve, reject] })); }
    catch (error) { settle(shell, "rejected", error); }
    return shell;
  }, PromisePrototype));
  yield tenant.yieldTenant(defineData(tenant, PromiseFn, "prototype", PromisePrototype, { writable: false, configurable: false }));
  const resolve = yield tenant.yieldTenant(makeBuiltin(tenant, "resolve", function* (_unused, args) { const shell = create(); resolveValue(shell, args[0]); return shell; }));
  const reject = yield tenant.yieldTenant(makeBuiltin(tenant, "reject", function* (_unused, args) { const shell = create(); settle(shell, "rejected", args[0]); return shell; }));
  yield tenant.yieldTenant(defineData(tenant, PromiseFn, "resolve", resolve));
  yield tenant.yieldTenant(defineData(tenant, PromiseFn, "reject", reject));

  const runtime: PromiseRuntime = {
    tenant, async, Promise: PromiseFn, PromisePrototype,
    isLocalPromise: isLocal,
    *resolve(value) { const shell = create(); resolveValue(shell, value); return shell as GuestPromise<unknown>; },
    *reject(reason) { const shell = create(); settle(shell, "rejected", reason); return shell as GuestPromise<never>; },
    *fromHostTask<T>(task) { const shell = create(); resolveValue(shell, task); return shell as GuestPromise<T>; },
    toHostTask<T>(value: GuestPromise<T> | unknown): HostTask<T> {
      const promise = new Promise<unknown>((resolveHost, rejectHost) => {
        if (!isLocal(value)) return resolveHost(value);
        const record = records.get(value as object)!;
        const done = () => record.state === "fulfilled" ? resolveHost(record.value) : rejectHost(record.value);
        if (record.state === "pending") record.reactions.push({ child: create(), fulfilled: undefined, rejected: undefined });
        if (record.state !== "pending") done();
        else {
          const poll = () => record.state === "pending" ? async.enqueueMicrotask(poll) : done();
          async.enqueueMicrotask(poll);
        }
      });
      return hostTaskFromPromise(async, promise) as HostTask<T>;
    },
    awaitGuest(value) { return this.toHostTask(value); },
    awaitHostTask(task) { return hostTaskPromise(async, task); },
  };
  const result = { Promise: PromiseFn, PromisePrototype, promiseRuntime: runtime };
  cache.set(tenant, { capability: async, result });
  return result;
}
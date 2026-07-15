import type { Tenant, TenantOp } from "./index.ts";
import { isGuestGen } from "./shims.ts";
import { TENANT_OP } from "./index.ts";

export { TENANT_OP };

function isTenantOp(value: unknown): value is TenantOp<unknown> {
  return (
    value !== null &&
    typeof value === "object" &&
    (value as any)[TENANT_OP] !== undefined
  );
}

function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return (
    value !== null && typeof value === "object" && typeof (value as any).then === "function"
  );
}

function isNativeIterator(value: unknown): value is Iterator<unknown, any, any> {
  return (
    value !== null &&
    typeof value === "object" &&
    typeof (value as any).next === "function" &&
    !isGuestGen(value)
  );
}

/** Create the sentinel used by tenant methods to compose nested tenant ops:
 * `yield this.yieldTenant(this.get(...))`. */
export function yieldTenant<T>(this: Tenant, gen: Generator<any, T, any>): TenantOp<T> {
  const op: TenantOp<T> = {} as TenantOp<T>;
  (op as any)[TENANT_OP] = gen;
  return op;
}

/** Synchronous driver: nested tenant ops run synchronously; everything else is
 * passed back to the generator unchanged. */
function driveTenantSync<T>(this: Tenant, gen: Generator<any, T, any>): T {
  let step = gen.next();
  while (!step.done) {
    const v = step.value;
    if (isTenantOp(v)) {
      const r = this.driveTenant(v[TENANT_OP], false, false);
      step = gen.next(r);
    } else {
      step = gen.next(v);
    }
  }
  return step.value;
}

/** Asynchronous driver: awaits promises, recursively drives nested tenant ops. */
async function driveTenantAsync<T>(this: Tenant, gen: Generator<any, T, any>): Promise<T> {
  let step = gen.next();
  while (!step.done) {
    const v = step.value;
    if (isTenantOp(v)) {
      const r = await this.driveTenant(v[TENANT_OP], true, false);
      step = gen.next(r);
    } else if (isPromiseLike(v)) {
      const r = await v;
      step = gen.next(r);
    } else {
      step = gen.next(v);
    }
  }
  return step.value;
}

/** Generator driver: `yield*`s through nested ops and traps returning native
 * generators, wrapping those native generators with `createGuestGen` when
 * `addGen` is active. */
function* driveTenantGen<T>(this: Tenant, gen: Generator<any, T, any>): Generator<any, T, any> {
  let step = gen.next();
  while (!step.done) {
    const v = step.value;
    if (isTenantOp(v)) {
      let r: any = (yield* (this.driveTenant(v[TENANT_OP], false, true) as Generator<any, any, any>)) as any;
      if (isNativeIterator(r)) {
        r = (yield* (this.driveTenant(
          this.createGuestGen(r as Generator),
          false,
          true,
        ) as Generator<any, any, any>)) as any;
      }
      step = gen.next(r);
    } else if (isNativeIterator(v)) {
      const r = (yield* (this.driveTenant(
        this.createGuestGen(v as Generator),
        false,
        true,
      ) as Generator<any, any, any>)) as any;
      step = gen.next(r);
    } else if (isPromiseLike(v)) {
      const r = yield v;
      step = gen.next(r);
    } else {
      const r = yield v;
      step = gen.next(r);
    }
  }
  return step.value;
}

/** Async-generator driver: composes both `await` and `yield*`. */
async function* driveTenantAsyncGen<T>(
  this: Tenant,
  gen: Generator<any, T, any>,
): AsyncGenerator<any, T, any> {
  let step = gen.next();
  while (!step.done) {
    const v = step.value;
    if (isTenantOp(v)) {
      let r: any = (yield* (this.driveTenant(v[TENANT_OP], true, true) as AsyncGenerator<any, any, any>)) as any;
      if (isNativeIterator(r)) {
        r = (yield* (this.driveTenant(
          this.createGuestGen(r as Generator),
          true,
          true,
        ) as AsyncGenerator<any, any, any>)) as any;
      }
      step = gen.next(r);
    } else if (isPromiseLike(v)) {
      const r = await v;
      step = gen.next(r);
    } else if (isNativeIterator(v)) {
      const r = (yield* (this.driveTenant(
        this.createGuestGen(v as Generator),
        true,
        true,
      ) as AsyncGenerator<any, any, any>)) as any;
      step = gen.next(r);
    } else {
      const r = yield v;
      step = gen.next(r);
    }
  }
  return step.value;
}

/** Drive a tenant-operation generator according to the effective variant.  This
 * is the single entry point every backend and embedder uses -- never call
 * `.next()` on a tenant method directly. */
export function driveTenant<T>(
  this: Tenant,
  gen: Generator<any, T, any>,
  addAsync: boolean,
  addGen: boolean,
): any {
  if (addGen) {
    if (addAsync) return driveTenantAsyncGen.call(this, gen);
    return driveTenantGen.call(this, gen);
  }
  if (addAsync) return driveTenantAsync.call(this, gen);
  return driveTenantSync.call(this, gen);
}
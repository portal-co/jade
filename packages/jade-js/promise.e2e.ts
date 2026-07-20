import {
  createNativeHostAsyncCapability, hostTaskFromPromise, hostTaskToPromise,
  MultiTenant, single_tenant,
} from "./index.ts";
import { promisePrimordial } from "./primordials/promise.ts";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(`FAIL: ${message}`);
}
function drive<T>(tenant: MultiTenant | typeof single_tenant, gen: Generator<any, T, any>): T {
  return tenant.driveTenant(gen, false, false) as T;
}

for (const tenant of [new MultiTenant(), single_tenant] as const) {
  const reports: unknown[] = [];
  const async = createNativeHostAsyncCapability({ onUnhandledRejection(reason) { reports.push(reason); } });
  const primordial = drive(tenant, promisePrimordial(tenant, async));
  assert(drive(tenant, promisePrimordial(tenant, async)) === primordial, "Promise cache must preserve tenant/capability identity");
  let rejected = false;
  try { drive(tenant, promisePrimordial(tenant, createNativeHostAsyncCapability())); }
  catch (error) { rejected = error instanceof TypeError; }
  assert(rejected, "incompatible capability must be rejected");

  const resolve = drive(tenant, tenant.get(primordial.Promise, "resolve")) as Function;
  const promise = drive(tenant, tenant.invoke(resolve, { kind: "apply", thisArg: primordial.Promise, args: [42] }));
  assert(primordial.promiseRuntime.isLocalPromise(promise), "Promise.resolve must create a guest promise");
  assert(!(promise instanceof Promise), "guest promise must not be a host Promise");
  assert(await hostTaskToPromise(async, primordial.promiseRuntime.toHostTask(promise)) === 42,
    "explicit guest-to-host HostTask conversion must settle");

  const then = drive(tenant, tenant.get(promise as object, "then")) as Function;
  let observed: unknown;
  const callback = drive(tenant, tenant.makeFunction((value: unknown) => { observed = value; return value; }));
  drive(tenant, tenant.invoke(then, { kind: "apply", thisArg: promise, args: [callback] }));
  await Promise.resolve();
  assert(observed === 42, "guest reactions must be microtask-scheduled");

  const adopted = drive(tenant, primordial.promiseRuntime.fromHostTask(hostTaskFromPromise(async, Promise.resolve("host task"))));
  assert(await hostTaskToPromise(async, primordial.promiseRuntime.toHostTask(adopted)) === "host task",
    "only explicit HostTask adoption crosses into guest Promise");

  const reject = drive(tenant, tenant.get(primordial.Promise, "reject")) as Function;
  drive(tenant, tenant.invoke(reject, { kind: "apply", thisArg: primordial.Promise, args: ["unhandled"] }));
  await Promise.resolve();
  assert(reports.includes("unhandled"), "host capability receives unhandled rejection reports");
}
console.log("PASS: guest Promise and HostTask separation");
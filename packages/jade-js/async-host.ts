/**
 * Host-only asynchronous control values.  A HostTask is deliberately not a
 * thenable: guest values must never reach native Promise assimilation merely
 * because host control flow needs to suspend.
 */
const HOST_TASK = Symbol("jade.hostTask");

export interface HostTask<T> {
  /** Nominal/private brand; no guest-facing `.then` contract exists. */
  readonly [HOST_TASK]: T;
}

type TaskRecord<T> = { capability: HostAsyncCapability; promise: Promise<T> };
const tasks = new WeakMap<object, TaskRecord<unknown>>();

export interface HostAsyncCapability {
  /** Stable identity used to reject accidental cross-embedder task use. */
  readonly identity: object;
  enqueueMicrotask(job: () => void): void;
  observe<T>(task: HostTask<T>, onFulfilled: (value: T) => void, onRejected: (reason: unknown) => void): void;
  /** Host-only diagnostics; never installed in a guest realm. */
  onUnhandledRejection(reason: unknown, promise: unknown): void;
  onRejectionHandled?(promise: unknown): void;
}

function taskRecord<T>(capability: HostAsyncCapability, task: HostTask<T>): TaskRecord<T> {
  const record = tasks.get(task as object);
  if (!record || record.capability !== capability) {
    throw new TypeError("HostTask does not belong to this HostAsyncCapability");
  }
  return record as TaskRecord<T>;
}

export interface HostTaskYield<T> {
  readonly task: HostTask<T>;
  readonly capability: HostAsyncCapability;
  readonly [HOST_TASK]: true;
}

/** Tagged suspension request for a tenant generator; raw promises are never accepted. */
export function hostTaskYield<T>(capability: HostAsyncCapability, task: HostTask<T>): HostTaskYield<T> {
  taskRecord(capability, task);
  return Object.freeze({ task, capability, [HOST_TASK]: true }) as HostTaskYield<T>;
}

export function isHostTaskYield(value: unknown): value is HostTaskYield<unknown> {
  return value !== null && typeof value === "object" && (value as { [HOST_TASK]?: unknown })[HOST_TASK] === true;
}

/** Make a trusted task from a host promise at an explicit host integration boundary. */
export function hostTaskFromPromise<T>(capability: HostAsyncCapability, promise: Promise<T>): HostTask<T> {
  const task = Object.create(null) as HostTask<T>;
  tasks.set(task as object, { capability, promise });
  return task;
}

/** Whether value is a task owned by exactly this capability. */
export function isHostTask<T>(capability: HostAsyncCapability, value: unknown): value is HostTask<T> {
  return value !== null && typeof value === "object" && tasks.get(value as object)?.capability === capability;
}

/** Observe a task without exposing its backing native promise. */
export function observeHostTask<T>(capability: HostAsyncCapability, task: HostTask<T>): void {
  const record = taskRecord(capability, task);
  capability.observe(task, () => {}, () => {});
  void record;
}

/** Explicit user/embedder interoperability adapter. Core Jade APIs never call this. */
export function hostTaskToPromise<T>(capability: HostAsyncCapability, task: HostTask<T>): Promise<T> {
  return taskRecord(capability, task).promise;
}

/** A deliberately explicit native host adapter, not a realm default. */
export function createNativeHostAsyncCapability(options: {
  onUnhandledRejection?: (reason: unknown, promise: unknown) => void;
  onRejectionHandled?: (promise: unknown) => void;
} = {}): HostAsyncCapability {
  const capability: HostAsyncCapability = {
    identity: Object.create(null),
    enqueueMicrotask(job) { queueMicrotask(job); },
    observe(task, onFulfilled, onRejected) {
      const record = taskRecord(this, task);
      record.promise.then(onFulfilled, onRejected);
    },
    onUnhandledRejection(reason, promise) { options.onUnhandledRejection?.(reason, promise); },
    onRejectionHandled: options.onRejectionHandled,
  };
  return capability;
}

/** Internal utility for implementations that must await only a known HostTask. */
export function hostTaskPromise<T>(capability: HostAsyncCapability, task: HostTask<T>): Promise<T> {
  return taskRecord(capability, task).promise;
}
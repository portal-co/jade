const HOST_TASK = Symbol("jade.hostTask");
export interface HostTask<T> {
    readonly [HOST_TASK]: T;
}
type TaskRecord<T> = {
    capability: HostAsyncCapability;
    promise: Promise<T>;
};
const tasks = new WeakMap<object, TaskRecord<unknown>>();
export interface HostAsyncCapability {
    readonly identity: object;
    enqueueMicrotask(job: () => void): void;
    observe<T>(task: HostTask<T>, onFulfilled: (value: T) => void, onRejected: (reason: unknown) => void): void;
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
export function hostTaskYield<T>(capability: HostAsyncCapability, task: HostTask<T>): HostTaskYield<T> {
    taskRecord(capability, task);
    return Object.freeze({
        task,
        capability,
        [HOST_TASK]: true
    }) as HostTaskYield<T>;
}
export function isHostTaskYield(value: unknown): value is HostTaskYield<unknown> {
    return value !== null && typeof value === "object" && (value as {
        [HOST_TASK]?: unknown;
    })[HOST_TASK] === true;
}
export function hostTaskFromPromise<T>(capability: HostAsyncCapability, promise: Promise<T>): HostTask<T> {
    const task = Object.create(null) as HostTask<T>;
    tasks.set(task as object, {
        capability,
        promise
    });
    return task;
}
export function isHostTask<T>(capability: HostAsyncCapability, value: unknown): value is HostTask<T> {
    return value !== null && typeof value === "object" && tasks.get(value as object)?.capability === capability;
}
export function observeHostTask<T>(capability: HostAsyncCapability, task: HostTask<T>): void {
    const record = taskRecord(capability, task);
    capability.observe(task, ()=>{}, ()=>{});
    void record;
}
export function hostTaskToPromise<T>(capability: HostAsyncCapability, task: HostTask<T>): Promise<T> {
    return taskRecord(capability, task).promise;
}
export function createNativeHostAsyncCapability(options: {
    onUnhandledRejection?: (reason: unknown, promise: unknown) => void;
    onRejectionHandled?: (promise: unknown) => void;
} = {}): HostAsyncCapability {
    const capability: HostAsyncCapability = {
        identity: Object.create(null),
        enqueueMicrotask (job) {
            queueMicrotask(job);
        },
        observe (task, onFulfilled, onRejected) {
            const record = taskRecord(this, task);
            record.promise.then(onFulfilled, onRejected);
        },
        onUnhandledRejection (reason, promise) {
            options.onUnhandledRejection?.(reason, promise);
        },
        onRejectionHandled: options.onRejectionHandled
    };
    return capability;
}
export function hostTaskPromise<T>(capability: HostAsyncCapability, task: HostTask<T>): Promise<T> {
    return taskRecord(capability, task).promise;
}

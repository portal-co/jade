import { guestAbiMixin } from "./narrow.ts";
import type { Tenant } from "./index.ts";

/**
 * Browser half of Jade's callback-driven WSDOM server tenant.
 *
 * This module is deliberately small and stateless with respect to tenant data:
 * values owned by Rust are represented only by private WeakMap-backed shims.
 * Add it to the WSDOM generator's import module list under the exact package
 * name `@portal-solutions/jade-js/wsdom-tenant-runtime`.
 */
export const WSDOM_TENANT_RUNTIME_IMPORT = "@portal-solutions/jade-js/wsdom-tenant-runtime";

type Callback = (value: unknown) => unknown;
type ServerDescriptor = { kind: "server"; capability: string; id: number };

type Request = {
  capability: string;
  requestId: number;
  operation: string;
  args: unknown[];
  resolve: Callback;
  reject: Callback;
};

function isServerDescriptor(value: unknown): value is ServerDescriptor {
  return (
    value !== null &&
    typeof value === "object" &&
    (value as { kind?: unknown }).kind === "server" &&
    typeof (value as { capability?: unknown }).capability === "string" &&
    typeof (value as { id?: unknown }).id === "number"
  );
}

/** Install one tenant facade for one WSDOM connection.
 *
 * `request` and `release` are WSDOM callback functions. Rust receives their
 * calls through `callback::new_callback` streams. Resolver/rejecter callbacks
 * are carried on each individual request, which prevents cross-request
 * settlement and does not require a second reverse-RPC protocol.
 */
export function installServerTenantRuntime(
  request: Callback,
  release: Callback,
  capability: string,
): Tenant {
  let nextRequestId = 1;
  const serverShims = new WeakMap<object, ServerDescriptor>();
  const releaseQueue: number[] = [];
  let releaseScheduled = false;

  const flushRelease = () => {
    releaseScheduled = false;
    if (releaseQueue.length !== 0) {
      const ids = releaseQueue.splice(0);
      release({ capability, serverValueIds: ids });
    }
  };
  const finalizer = typeof FinalizationRegistry === "undefined"
    ? undefined
    : new FinalizationRegistry<number>((id) => {
        releaseQueue.push(id);
        if (!releaseScheduled) {
          releaseScheduled = true;
          queueMicrotask(flushRelease);
        }
      });

  const shim = (descriptor: ServerDescriptor): object => {
    const out = Object.freeze(Object.create(null));
    serverShims.set(out, descriptor);
    finalizer?.register(out, descriptor.id);
    return out;
  };

  const toWire = (value: unknown): unknown => {
    if (value !== null && (typeof value === "object" || typeof value === "function")) {
      const descriptor = serverShims.get(value as object);
      if (descriptor !== undefined) return descriptor;
    }
    // Browser values intentionally stay real values. WSDOM's callback mechanism
    // preserves them as remote handles on Rust rather than JSON-cloning them.
    return value;
  };

  const fromWire = (value: unknown): unknown =>
    isServerDescriptor(value) && value.capability === capability ? shim(value) : value;

  const bridge = {
    request(operation: string, args: unknown[]): Promise<unknown> {
      const requestId = nextRequestId++;
      return new Promise((resolve, reject) => {
        const resolveCallback: Callback = (value) => resolve(fromWire(value));
        const rejectCallback: Callback = (error) => reject(error);
        request({
          capability,
          requestId,
          operation,
          args: args.map(toWire),
          resolve: resolveCallback,
          reject: rejectCallback,
        } satisfies Request);
      });
    },
  };

  const op = (name: string) => function* (...args: unknown[]) {
    return yield bridge.request(name, args);
  };

  // The full normal tenant object-manager surface is present. The ABI helpers
  // below come from the current Jade package, never a Rust source snapshot.
  return Object.assign(
    Object.create(null),
    {
      make: op("make"),
      get: op("get"),
      set: op("set"),
      has: op("has"),
      delete: op("delete"),
      ownKeys: op("ownKeys"),
      define: op("define"),
      assign: op("assign"),
    },
    guestAbiMixin,
  ) as Tenant;
}
/**
 * Shared test262 cell execution — the one place guest code actually runs.
 *
 * Used both by the CLI driver (`driver.ts`, spawned per-cell by the Rust orchestrator)
 * and by the TS orchestrator (`run.ts`) in-process. Every execution gets a fresh tenant
 * and a fresh primordial realm, and every tenant interaction goes through the driver's
 * `driveTenant` — never raw `.next()`, never host reflection.
 *
 * Job JSON shape (produced by `jade-test262` or `run.ts`):
 *   { test, env, tenant, bytecode?: number[], body?: string, prelude?: string }
 * Verdict JSON shape is the report schema's CellVerdict (see `crates/jade-test262/src/model.rs`).
 */
import {
  MultiTenant,
  single_tenant,
  createPrimordialRealm,
  createNativeHostAsyncCapability,
  markGuestFn,
  vm,
  type Tenant,
  type PrimordialRealm,
} from "../index.ts";
import { Test262Error, installHarness } from "./harness.ts";

export interface Job {
  test: string;
  /** `interp` | `jit-t0` | `jit-t1` | `jit-t2` */
  env: string;
  tenant?: string;
  bytecode?: number[];
  body?: string;
  prelude?: string;
  /** Install the test262 harness primordials (harness.ts) before running. */
  harness?: boolean;
}

export interface CellVerdict {
  kind:
    | "pass" | "fail" | "differential-mismatch"
    | "skip-unsupported-frontend" | "skip-parse" | "skip-feature" | "skip-flag"
    | "crash" | "timeout";
  reason?: string;
  /** JSON-encoded completion value snapshot, when one was produced. */
  value?: string;
  error?: string;
}

export interface CellContext {
  tenant: Tenant;
  realm: PrimordialRealm;
}

/** Build a fresh tenant + realm for one cell. Never shared across cells. */
export function freshContext(tenantName: string = "multi"): CellContext {
  const tenant: Tenant = tenantName === "single" ? single_tenant : new MultiTenant();
  const hostAsync = createNativeHostAsyncCapability();
  const realm = (tenant.driveTenant(
    createPrimordialRealm(tenant, { async: hostAsync }),
    false,
    false,
  ) as unknown) as PrimordialRealm;
  return { tenant, realm };
}

/** Deterministic, allocation-light snapshot of a completion value for differential
 *  comparison. Guest objects get no deep inspection here (that would need tenant reads);
 *  primitives are the differential signal at this layer. */
export function snapshot(v: unknown): string {
  try {
    const seen = new Set<object>();
    const out = JSON.stringify(v, (_k, x) => {
      if (typeof x === "bigint") return `bigint:${x}`;
      if (typeof x === "function") return `[function ${x.name || "anonymous"}]`;
      if (typeof x === "symbol") return String(x);
      if (typeof x === "undefined") return "[undefined]";
      if (typeof x === "object" && x !== null) {
        if (seen.has(x)) return "[circular]";
        seen.add(x);
      }
      return x;
    });
    return out === undefined ? "[undefined]" : out;
  } catch {
    return Object.prototype.toString.call(v);
  }
}

function runInterpreter(job: Job, ctx: CellContext): unknown {
  if (!job.bytecode) throw new Error("interp cell without bytecode");
  const bytes = new Uint8Array(job.bytecode);
  const code = () => new DataView(bytes.buffer);
  return vm.runVirtualized(code, {}, {
    tenant: ctx.tenant,
    promiseRuntime: ctx.realm.promiseRuntime,
    globalThis: ctx.realm.globalThis as typeof globalThis,
  });
}

function runJit(job: Job, ctx: CellContext): unknown {
  if (job.body === undefined || job.prelude === undefined) {
    throw new Error("jit cell without body/prelude");
  }
  // The emitted body's ambient references are exactly these parameters — `globalThis`
  // and `promiseRuntime` are shadowed so guest code only ever sees the realm.
  const fn = new Function(
    "tenant",
    "nt",
    "state",
    "globalThis",
    "promiseRuntime",
    "markGuestFn",
    job.prelude + "\n" + job.body,
  );
  return fn(
    ctx.tenant,
    undefined,
    {},
    ctx.realm.globalThis,
    ctx.realm.promiseRuntime,
    markGuestFn,
  );
}

/** Execute one cell; never throws — any exception becomes a `fail` verdict. */
export async function execute(job: Job): Promise<CellVerdict> {
  let ctx: CellContext;
  try {
    ctx = freshContext(job.tenant);
    if (job.harness) {
      installHarness(ctx);
    }
  } catch (error) {
    return { kind: "crash", error: `context setup: ${String(error)}` };
  }
  try {
    const value = job.env === "interp" ? runInterpreter(job, ctx) : runJit(job, ctx);
    return { kind: "pass", value: snapshot(value) };
  } catch (error) {
    return {
      kind: "fail",
      reason: error instanceof Test262Error ? "assertion failed" : "threw during execution",
      error: error instanceof Error ? (error.stack ?? error.message) : String(error),
    };
  }
}

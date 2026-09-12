/**
 * TS test262 orchestrator:
 *
 *   node --experimental-strip-types packages/jade-js/test262/run.ts \
 *     --shard smoke [--tenant multi] [--env interp,wasm-interp,jit-t0,jit-t1,jit-t2] \
 *     [--report <path>] [--check] [--update-expectations]
 *
 * Uses the Rust CLI (`jade-test262 compile --file`) as the compile oracle, executes each
 * cell in-process via exec.ts, then applies the same skip-justification, differential,
 * validation, and expectation rules as the Rust orchestrator. Emits the shared report
 * schema from report.ts.
 */
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, relative } from "node:path";
import { execute, type CellVerdict } from "./exec.ts";
import {
  REPORT_VERSION,
  checkExpectations,
  expectationsFromReport,
  loadExpectations,
  rollup,
  validateReport,
  writeJson,
  type Report,
  type TestOutcome,
} from "./report.ts";

const ROOT = fileURLToPath(new URL("../../..", import.meta.url));
const T262_DIR = fileURLToPath(new URL(".", import.meta.url));
const RUST_BIN = join(ROOT, "target/debug/jade-test262");
const WASM_PKG = join(T262_DIR, "pkg", "jade_vm_wasm.js");
const NODE_ENVS = ["interp", "jit-t0", "jit-t1", "jit-t2"];
const INTERP_ENVS = new Set(["interp", "wasm-interp"]);

function ensureWasmBundle(): void {
  if (existsSync(WASM_PKG)) return;
  console.error("run: building jade-vm-wasm bundle (wasm-pack build)…");
  execFileSync("wasm-pack", [
    "build",
    "--target", "nodejs",
    "--out-dir", "../../packages/jade-js/test262/pkg",
    "--out-name", "jade_vm_wasm",
    "--no-typescript",
    "crates/jade-vm-wasm",
  ], { cwd: ROOT, stdio: "inherit" });
}

interface Manifest {
  version: number;
  features: Record<string, { status: string; note?: string }>;
  harnessIncludes: Record<string, { status: string; note?: string }>;
  unsupportedConstructs: Record<string, { status: string; note?: string }>;
  flags: Record<string, { status: string; note?: string }>;
}

function opt(args: string[], name: string): string | undefined {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
}

function discover(shard: string): string[] {
  const root = shard === "smoke"
    ? join(T262_DIR, "fixtures/smoke")
    : shard.startsWith("test262:")
      ? join(ROOT, "vendor/test262/test", shard.slice("test262:".length))
      : die(`unknown shard ${JSON.stringify(shard)} (want smoke or test262:<subdir>)`);
  const out: string[] = [];
  const walk = (dir: string): void => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (name.endsWith(".js") && !name.includes("_FIXTURE")) out.push(p);
    }
  };
  walk(root);
  return out.sort();
}

function die(msg: string): never {
  console.error(`run: ${msg}`);
  process.exit(1);
}

function ensureRustBin(): void {
  if (existsSync(RUST_BIN)) return;
  console.error("run: building jade-test262 (cargo build)…");
  execFileSync("cargo", ["build", "-p", "portal-solutions-jade-test262"], {
    cwd: ROOT,
    stdio: "inherit",
  });
}

interface CompileArtifact {
  ok: boolean;
  meta?: { flags?: string[]; includes?: string[]; negative?: { phase: string; type?: string } };
  bytecode?: number[];
  tiers?: Record<string, { Ok?: { body: string; prelude: string }; Err?: string } | { body: string; prelude: string }>;
  error_kind?: string;
  message?: string;
}

function compile(file: string): CompileArtifact {
  let stdout: string;
  try {
    stdout = execFileSync(RUST_BIN, ["compile", "--file", file], { cwd: ROOT, encoding: "utf8" });
  } catch (error) {
    // The compile subcommand exits nonzero for parse/unsupported verdicts, with the
    // artifact still on stdout.
    stdout = (error as { stdout?: string }).stdout ?? "";
    if (!stdout.trim()) throw error;
  }
  return JSON.parse(stdout);
}

/** Manifest skip-justification — the TS port of `manifest.rs::justify_skip`. */
function justifySkip(manifest: Manifest, cell: CellVerdict, meta: CompileArtifact["meta"]): string | null {
  const reason = cell.reason ?? "";
  switch (cell.kind) {
    case "skip-unsupported-frontend":
      return reason in manifest.unsupportedConstructs
        ? null
        : `Unsupported message not in manifest.unsupportedConstructs: ${JSON.stringify(reason)}`;
    case "skip-feature": {
      const known = manifest.features[reason] ?? manifest.harnessIncludes[reason];
      if (known) return known.status !== "supported" ? null : `skip-feature ${reason} but manifest says supported`;
      const includes = meta?.includes ?? [];
      const required = [...(meta?.flags?.includes("raw") ? [] : ["sta.js", "assert.js"]), ...includes];
      return required.includes(reason) ? null : `skip-feature reason names nothing: ${reason}`;
    }
    case "skip-flag": {
      const known = manifest.flags[reason];
      if (!known) return `skip-flag ${reason} missing from manifest.flags`;
      return known.status !== "supported" ? null : `skip-flag ${reason} but manifest says supported`;
    }
    case "skip-parse":
      return null;
    default:
      return `not a skip verdict: ${cell.kind}`;
  }
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  const shard = opt(args, "--shard") ?? die("missing --shard");
  const tenant = opt(args, "--tenant") ?? "multi";
  const envs = opt(args, "--env")?.split(",") ?? NODE_ENVS;
  const check = args.includes("--check");
  const update = args.includes("--update-expectations");
  const reportPath = opt(args, "--report") ?? join(T262_DIR, "reports", `${shard}.json`);
  const manifest = JSON.parse(readFileSync(join(T262_DIR, "manifest.json"), "utf8")) as Manifest;

  ensureRustBin();
  if (envs.includes("wasm-interp")) ensureWasmBundle();
  const files = discover(shard);
  if (files.length === 0) die(`shard ${shard} discovered no tests`);

  const results: TestOutcome[] = [];
  for (const file of files) {
    const path = shard === "smoke"
      ? `fixtures/smoke/${file.split("/").pop()}`
      : relative(join(ROOT, "vendor/test262/test"), file);
    const artifact = compile(file);
    const cells: Record<string, CellVerdict> = {};

    // Uniform pre-compilation gates — mirror of run.rs::uniform_skip.
    const flags = artifact.meta?.flags ?? [];
    const includes = artifact.meta?.includes ?? [];
    const requiredIncludes = [...(flags.includes("raw") ? [] : ["sta.js", "assert.js"]), ...includes];
    let uniform: CellVerdict | undefined;
    for (const f of flags) {
      if (manifest.flags[f]?.status === "unsupported") {
        uniform = { kind: "skip-flag", reason: f };
        break;
      }
    }
    if (!uniform) {
      for (const inc of requiredIncludes) {
        if (manifest.harnessIncludes[inc]?.status !== "supported") {
          uniform = { kind: "skip-feature", reason: inc };
          break;
        }
      }
    }
    if (!uniform) {
      for (const feat of (artifact.meta as { features?: string[] } | undefined)?.features ?? []) {
        if (manifest.features[feat]?.status === "unsupported") {
          uniform = { kind: "skip-feature", reason: feat };
          break;
        }
      }
    }
    if (!uniform && artifact.meta?.negative?.phase === "runtime") {
      uniform = {
        kind: "skip-unsupported-frontend",
        reason: "exceptions: no bytecode opcode for throw/try yet",
      };
    }
    if (!uniform && flags.includes("async")) {
      uniform = { kind: "skip-flag", reason: "async" };
    }

    let baseline: CellVerdict | undefined;
    for (const env of envs) {
      let cell: CellVerdict;
      if (uniform) {
        cell = uniform;
      } else if (!artifact.ok) {
        const neg = artifact.meta?.negative;
        const compileRejectionExpected = neg && (neg.phase === "parse" || neg.phase === "early");
        if (compileRejectionExpected) {
          cell = { kind: "pass", reason: `expected compile rejection: ${artifact.message}` };
        } else if (artifact.error_kind === "unsupported") {
          cell = { kind: "skip-unsupported-frontend", reason: artifact.message };
        } else if (artifact.error_kind === "parse") {
          cell = { kind: "skip-parse", reason: artifact.message };
        } else {
          cell = { kind: "fail", reason: `compile: ${artifact.error_kind}: ${artifact.message}` };
        }
      } else if (artifact.meta?.negative && ["parse", "early"].includes(artifact.meta.negative.phase)) {
        // Compiled but demands a parse/early rejection: parser-strictness failure.
        cell = { kind: "fail", reason: "compiled successfully but the test demands a parse/early rejection" };
      } else if (INTERP_ENVS.has(env)) {
        cell = await execute({ test: path, env, tenant, bytecode: artifact.bytecode, harness: !flags.includes("raw") });
      } else {
        const tier = artifact.tiers?.[env] as
          | { Ok?: { body: string; prelude: string }; Err?: string }
          | { body: string; prelude: string }
          | undefined;
        const out = tier && ("Ok" in tier ? tier.Ok : tier as { body: string; prelude: string });
        const err = tier && "Err" in tier ? tier.Err : undefined;
        if (out?.body !== undefined) {
          cell = await execute({ test: path, env, tenant, body: out.body, prelude: out.prelude, harness: !flags.includes("raw") });
        } else {
          cell = { kind: "crash", error: `JIT compile (${env}): ${err ?? "no tier output"}` };
        }
      }
      // Differential: every passing tier must agree with the interpreter's value.
      if (env === "interp") baseline = cell;
      else if (baseline?.kind === "pass" && cell.kind === "pass" && cell.value !== baseline.value) {
        cell = {
          kind: "differential-mismatch",
          reason: `baseline interp produced ${JSON.stringify(baseline.value)}`,
          error: `this cell produced ${JSON.stringify(cell.value)}`,
        };
      }
      cells[env] = cell;
    }
    results.push({ path, meta: artifact.meta ?? {}, cells });
  }

  const report: Report = { version: REPORT_VERSION, shard, tenant, results };

  const problems = validateReport(report);
  if (problems.length) die(`report failed self-validation:\n${problems.join("\n")}`);

  const unjustified: string[] = [];
  for (const r of results) {
    for (const [env, cell] of Object.entries(r.cells)) {
      if (cell.kind.startsWith("skip-")) {
        const problem = justifySkip(manifest, cell, r.meta as CompileArtifact["meta"]);
        if (problem) unjustified.push(`${r.path}[${env}]: ${problem}`);
      }
    }
  }
  if (unjustified.length) die(`skips without manifest justification:\n${unjustified.join("\n")}`);

  writeJson(reportPath, report);

  const expectationsDir = join(T262_DIR, "expectations");
  if (update) {
    const exps = expectationsFromReport(report, loadExpectations(expectationsDir));
    for (const [env, exp] of Object.entries(exps)) {
      writeJson(join(expectationsDir, `${env}.json`), exp);
    }
  }
  if (check) {
    const drift = checkExpectations(report, loadExpectations(expectationsDir));
    if (drift.length) die(`expectation drift:\n${drift.join("\n")}`);
  }

  console.log(`shard ${shard}:`);
  for (const [env, kinds] of Object.entries(rollup(report))) {
    console.log(`  ${env}: ${Object.entries(kinds).map(([k, n]) => `${k}=${n}`).join(" ")}`);
  }
}

await main();

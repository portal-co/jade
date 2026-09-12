/**
 * Report schema + validation + expectations — the TS mirror of
 * `crates/jade-test262/src/model.rs`. The invariants here must match `Report::validate`
 * 1:1; both runners' reports are checked by both validators.
 */
import { readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import type { CellVerdict } from "./exec.ts";

export const REPORT_VERSION = 1;

export interface TestOutcome {
  path: string;
  meta: unknown;
  cells: Record<string, CellVerdict>;
}

export interface Report {
  version: number;
  shard: string;
  tenant: string;
  results: TestOutcome[];
}

const SKIP_KINDS = new Set([
  "skip-unsupported-frontend",
  "skip-parse",
  "skip-feature",
  "skip-flag",
]);

/** Structural invariants — mirror of `Report::validate`. Returns violations found. */
export function validateReport(report: Report): string[] {
  const problems: string[] = [];
  if (report.version !== REPORT_VERSION) {
    problems.push(`version ${report.version} != ${REPORT_VERSION}`);
  }
  const seen = new Set<string>();
  for (const r of report.results) {
    if (seen.has(r.path)) problems.push(`duplicate test path ${r.path}`);
    seen.add(r.path);
    for (const [env, cell] of Object.entries(r.cells)) {
      if (SKIP_KINDS.has(cell.kind) && !cell.reason) {
        problems.push(`${r.path}[${env}]: skip verdict without a reason`);
      }
      if (cell.kind === "differential-mismatch" && !cell.reason) {
        problems.push(`${r.path}[${env}]: differential-mismatch without a reason`);
      }
    }
  }
  return problems;
}

export function rollup(report: Report): Record<string, Record<string, number>> {
  const out: Record<string, Record<string, number>> = {};
  for (const r of report.results) {
    for (const [env, cell] of Object.entries(r.cells)) {
      (out[env] ??= {})[cell.kind] = (out[env]?.[cell.kind] ?? 0) + 1;
    }
  }
  return out;
}

export interface Expectation { verdict: string; note?: string }
export interface Expectations { version: number; tests: Record<string, Expectation> }

export function loadExpectations(dir: string): Record<string, Expectations> {
  const out: Record<string, Expectations> = {};
  if (!existsSync(dir)) return out;
  for (const f of readdirSync(dir)) {
    if (f.endsWith(".json")) {
      out[f.replace(/\.json$/, "")] = JSON.parse(readFileSync(join(dir, f), "utf8"));
    }
  }
  return out;
}

/** Drift lines (`path[env]: expected X, got Y`); empty = ratchet holds. */
export function checkExpectations(
  report: Report,
  expectations: Record<string, Expectations>,
): string[] {
  const drift: string[] = [];
  for (const r of report.results) {
    for (const [env, cell] of Object.entries(r.cells)) {
      const expected = expectations[env]?.tests[r.path]?.verdict ?? "pass";
      if (cell.kind !== expected) {
        drift.push(`${r.path}[${env}]: expected ${expected}, got ${cell.kind}`);
      }
    }
  }
  return drift;
}

export function writeJson(path: string, value: unknown): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(value, null, 2) + "\n");
}

/** Re-baseline: expectations from a report (only non-pass verdicts are named). */
export function expectationsFromReport(report: Report): Record<string, Expectations> {
  const out: Record<string, Expectations> = {};
  for (const r of report.results) {
    for (const [env, cell] of Object.entries(r.cells)) {
      if (cell.kind !== "pass") {
        (out[env] ??= { version: 1, tests: {} }).tests[r.path] = { verdict: cell.kind };
      }
    }
  }
  return out;
}

/**
 * Per-cell execution driver, spawned by the Rust orchestrator:
 *
 *   node --experimental-strip-types packages/jade-js/test262/driver.ts <job.json>
 *
 * Reads the job, executes it (see exec.ts), prints the CellVerdict JSON on a
 * marker-prefixed line (the LAST line in single-job mode). Anything a test prints (via the harness `print`, later phases) lands on
 * earlier lines and is ignored by the orchestrator.
 *
 * Batch mode: if the file contains `{"jobs": [...]}` instead of a single job, every job
 * runs sequentially (each in its own fresh tenant + realm) and the driver prints one
 * verdict line per job, in order. This amortizes process+import startup across a shard;
 * the orchestrator falls back to single-job mode for any batch it can't complete.
 */
import { readFileSync } from "node:fs";
import { execute, type CellVerdict, type Job } from "./exec.ts";

/** Marker prefix for verdict lines: everything else on stdout is test/log output. */
const VERDICT_PREFIX = "__JADE_T262_VERDICT__ ";

async function main(): Promise<void> {
  const jobPath = process.argv[2];
  if (!jobPath) {
    const verdict: CellVerdict = { kind: "crash", error: "driver: missing <job.json>" };
    console.log(VERDICT_PREFIX + JSON.stringify(verdict));
    process.exitCode = 1;
    return;
  }
  let payload: Job | { jobs: Job[] };
  try {
    payload = JSON.parse(readFileSync(jobPath, "utf8")) as Job;
  } catch (error) {
    console.log(VERDICT_PREFIX + JSON.stringify({ kind: "crash", error: `driver: bad job json: ${String(error)}` }));
    process.exitCode = 1;
    return;
  }
  const jobs = "jobs" in payload ? payload.jobs : [payload];
  for (const job of jobs) {
    let verdict: CellVerdict;
    try {
      verdict = await execute(job);
    } catch (error) {
      // execute() already never throws; this guards against bugs in the harness itself.
      verdict = { kind: "crash", error: `driver: uncaught: ${String(error)}` };
    }
    console.log(VERDICT_PREFIX + JSON.stringify(verdict));
  }
}

await main();

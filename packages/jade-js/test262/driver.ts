/**
 * Per-cell execution driver, spawned by the Rust orchestrator:
 *
 *   node --experimental-strip-types packages/jade-js/test262/driver.ts <job.json>
 *
 * Reads the job, executes it (see exec.ts), prints the CellVerdict JSON as the LAST
 * stdout line. Anything a test prints (via the harness `print`, later phases) lands on
 * earlier lines and is ignored by the orchestrator.
 */
import { readFileSync } from "node:fs";
import { execute, type CellVerdict, type Job } from "./exec.ts";

async function main(): Promise<void> {
  const jobPath = process.argv[2];
  if (!jobPath) {
    const verdict: CellVerdict = { kind: "crash", error: "driver: missing <job.json>" };
    console.log(JSON.stringify(verdict));
    process.exitCode = 1;
    return;
  }
  let job: Job;
  try {
    job = JSON.parse(readFileSync(jobPath, "utf8")) as Job;
  } catch (error) {
    console.log(JSON.stringify({ kind: "crash", error: `driver: bad job json: ${String(error)}` }));
    process.exitCode = 1;
    return;
  }
  let verdict: CellVerdict;
  try {
    verdict = await execute(job);
  } catch (error) {
    // execute() already never throws; this guards against bugs in the harness itself.
    verdict = { kind: "crash", error: `driver: uncaught: ${String(error)}` };
  }
  console.log(JSON.stringify(verdict));
}

await main();

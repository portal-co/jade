//! Bridge to the Node execution driver (`packages/jade-js/test262/driver.ts`).
//!
//! All JS execution — interpreter and every JIT tier — goes through that one TS driver,
//! so the Rust orchestrator and the TS orchestrator observe identical semantics. This
//! module owns the subprocess mechanics: job JSON to a temp file, `node
//! --experimental-strip-types` with a wall-clock timeout, verdict JSON from
//! marker-prefixed stdout lines.
//!
//! Execution is **batched**: process+import startup dwarfs per-test runtime, so jobs go
//! out in chunks (`BATCH_SIZE`) with a per-chunk deadline derived from the per-cell
//! timeout. A chunk that can't complete (timeout, crash, short output) falls back to
//! single-job runs so one wedged test can't take its chunk-mates down with it.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::model::{CellVerdict, VerdictKind};

/// Repo-root-relative driver entry point. Resolved against `CARGO_MANIFEST_DIR` so the
/// binary works from any working directory.
pub fn driver_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/jade-js/test262/driver.ts")
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Jobs per driver process. Sized so a fully-passing chunk is one import cost + N fast
/// executions, and a pathological chunk costs at most `batch deadline` + N single-job
/// fallbacks.
pub const BATCH_SIZE: usize = 32;
const VERDICT_PREFIX: &str = "__JADE_T262_VERDICT__ ";

/// Run a batch of jobs; returns one verdict per job, in order.
pub fn run_cells(jobs: &[serde_json::Value], per_cell: Duration) -> Vec<CellVerdict> {
    let mut out = Vec::with_capacity(jobs.len());
    for chunk in jobs.chunks(BATCH_SIZE.max(1)) {
        run_chunk(chunk, per_cell, &mut out);
    }
    out
}

fn run_chunk(chunk: &[serde_json::Value], per_cell: Duration, out: &mut Vec<CellVerdict>) {
    if chunk.len() == 1 {
        out.push(run_cell(&chunk[0], per_cell));
        return;
    }
    // A chunk's deadline: every cell could individually time out, plus startup.
    let deadline = per_cell * chunk.len() as u32 + Duration::from_secs(15);
    let payload = serde_json::json!({ "jobs": chunk });
    match spawn_and_collect(&payload, deadline) {
        Ok(lines) if lines.len() == chunk.len() => {
            out.extend(lines.into_iter().map(|l| parse_verdict(&l)));
        }
        _ => {
            // Fallback: isolate each cell so one bad test doesn't sink the chunk.
            out.extend(chunk.iter().map(|job| run_cell(job, per_cell)));
        }
    }
}

/// Run one job in its own driver process. Never fails to return a verdict: IO trouble
/// becomes `crash`, exceeding `timeout` becomes `timeout`.
pub fn run_cell(job: &serde_json::Value, timeout: Duration) -> CellVerdict {
    let started = Instant::now();
    match spawn_and_collect(job, timeout) {
        Ok(lines) if !lines.is_empty() => {
            let mut cell = parse_verdict(lines.last().unwrap());
            cell.duration_ms = started.elapsed().as_millis() as u64;
            cell
        }
        Ok(_) => {
            let mut c = CellVerdict::new(VerdictKind::Crash);
            c.error = Some("driver exited without a verdict line".into());
            c
        }
        Err(kind) => kind,
    }
}

type SpawnResult = Result<Vec<String>, CellVerdict>;

fn spawn_and_collect(payload: &serde_json::Value, deadline: Duration) -> SpawnResult {
    let started = Instant::now();
    let tmp = tempfile_path("job");
    let write = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(payload.to_string().as_bytes()));
    if let Err(e) = write {
        return Err(crash(format!("write job file: {e}")));
    }
    let mut child = match Command::new("node")
        .arg("--experimental-strip-types")
        .arg(driver_path())
        .arg(&tmp)
        .current_dir(repo_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(crash(format!("spawn node: {e}")));
        }
    };
    let result = poll(&mut child, deadline, started);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn poll(child: &mut Child, deadline: Duration, started: Instant) -> SpawnResult {
    use std::io::Read;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut h) = child.stdout.take() {
                    let _ = h.read_to_string(&mut stdout);
                }
                if let Some(mut h) = child.stderr.take() {
                    let _ = h.read_to_string(&mut stderr);
                }
                let lines: Vec<String> = stdout
                    .lines()
                    .filter_map(|l| l.strip_prefix(VERDICT_PREFIX).map(str::to_string))
                    .collect();
                if lines.is_empty() && !status.success() {
                    return Err(crash(format!(
                        "driver exited {status} with no verdicts: {}",
                        stderr.trim()
                    )));
                }
                return Ok(lines);
            }
            Ok(None) => {
                if started.elapsed() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let mut c = CellVerdict::new(VerdictKind::Timeout);
                    c.error = Some(format!("exceeded {deadline:?}"));
                    return Err(c);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(crash(format!("wait: {e}"))),
        }
    }
}

fn parse_verdict(line: &str) -> CellVerdict {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        if let Some(kind) = v.get("kind").and_then(|k| k.as_str()) {
            let mut cell = CellVerdict::new(parse_kind(kind).unwrap_or(VerdictKind::Crash));
            cell.reason = v.get("reason").and_then(|r| r.as_str()).map(String::from);
            cell.value = v.get("value").and_then(|r| r.as_str()).map(String::from);
            cell.error = v.get("error").and_then(|r| r.as_str()).map(String::from);
            return cell;
        }
    }
    crash(format!("unparseable verdict line: {line:?}"))
}

fn parse_kind(s: &str) -> Option<VerdictKind> {
    Some(match s {
        "pass" => VerdictKind::Pass,
        "fail" => VerdictKind::Fail,
        "differential-mismatch" => VerdictKind::DifferentialMismatch,
        "skip-unsupported-frontend" => VerdictKind::SkipUnsupportedFrontend,
        "skip-parse" => VerdictKind::SkipParse,
        "skip-feature" => VerdictKind::SkipFeature,
        "skip-flag" => VerdictKind::SkipFlag,
        "crash" => VerdictKind::Crash,
        "timeout" => VerdictKind::Timeout,
        _ => return None,
    })
}

fn crash(msg: String) -> CellVerdict {
    let mut c = CellVerdict::new(VerdictKind::Crash);
    c.error = Some(msg);
    c
}

fn tempfile_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jade-test262-{tag}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

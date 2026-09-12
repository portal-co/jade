//! Bridge to the Node execution driver (`packages/jade-js/test262/driver.ts`).
//!
//! All JS execution — interpreter and every JIT tier — goes through that one TS driver,
//! so the Rust orchestrator and the TS orchestrator observe identical semantics. This
//! module owns the subprocess mechanics: job JSON to a temp file, `node
//! --experimental-strip-types` with a wall-clock timeout, verdict JSON from stdout.

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

/// Run one test × one environment cell. `job` is the driver job JSON (see
/// `packages/jade-js/test262/exec.ts` for the shape). Never fails to return a verdict:
/// process/IO trouble becomes `crash`, exceeding `timeout` becomes `timeout`.
pub fn run_cell(job: &serde_json::Value, timeout: Duration) -> CellVerdict {
    let started = Instant::now();
    let tmp = tempfile_path("job");
    let mut f = match std::fs::File::create(&tmp) {
        Ok(f) => f,
        Err(e) => return crash(format!("create job file: {e}"), started),
    };
    if let Err(e) = f.write_all(job.to_string().as_bytes()) {
        return crash(format!("write job file: {e}"), started);
    }
    drop(f);

    let child = Command::new("node")
        .arg("--experimental-strip-types")
        .arg(driver_path())
        .arg(&tmp)
        .current_dir(repo_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return crash(format!("spawn node: {e}"), started),
    };

    let mut cell = poll(&mut child, timeout, started);
    cell.duration_ms = started.elapsed().as_millis() as u64;
    let _ = std::fs::remove_file(&tmp);
    cell
}

fn poll(child: &mut Child, timeout: Duration, started: Instant) -> CellVerdict {
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
                return finish(status.success(), &stdout, &stderr);
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return CellVerdict::new(VerdictKind::Timeout);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return crash(format!("wait: {e}"), started),
        }
    }
}

/// The driver prints its verdict as the last non-empty stdout line; anything before it
/// is test `print` output. A nonzero exit without a parseable verdict is a crash.
fn finish(success: bool, stdout: &str, stderr: &str) -> CellVerdict {
    if let Some(line) = stdout.lines().rev().find(|l| !l.trim().is_empty()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(kind) = v.get("kind").and_then(|k| k.as_str()) {
                let mut cell = CellVerdict::new(parse_kind(kind).unwrap_or(VerdictKind::Crash));
                cell.reason = v.get("reason").and_then(|r| r.as_str()).map(String::from);
                cell.value = v.get("value").and_then(|r| r.as_str()).map(String::from);
                cell.error = v.get("error").and_then(|r| r.as_str()).map(String::from);
                if cell.kind == VerdictKind::Crash && cell.error.is_none() && !success {
                    cell.error = Some(stderr.trim().to_string());
                }
                return cell;
            }
        }
    }
    crash(format!(
        "driver produced no verdict line (exit ok={success}): {}",
        stderr.trim()
    ), Instant::now())
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

fn crash(msg: String, _started: Instant) -> CellVerdict {
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

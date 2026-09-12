//! Orchestration: discover tests, compile, dispatch to the Node driver per environment
//! cell, differential-compare, justify skips against the manifest, check expectations,
//! emit the report.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::manifest::Manifest;
use crate::meta::{self, TestMeta};
use crate::model::{
    self, CellVerdict, Expectations, Report, TestOutcome, VerdictKind, REPORT_VERSION,
};
use crate::node;
use crate::pipeline::{self, CompileVerdict, ENV_INTERP, INTERP_ENVS, NODE_ENVS};

pub struct RunConfig {
    /// `smoke` (the synthetic fixtures) or `test262:<subdir>` (a vendored subtree).
    pub shard: String,
    /// Run at most this many tests (discovery order) — for bisecting wedges.
    pub limit: Option<usize>,
    pub tenant: String,
    pub envs: Vec<String>,
    pub report_path: PathBuf,
    pub manifest_path: PathBuf,
    pub expectations_dir: PathBuf,
    /// Compare against checked-in expectations and fail on drift.
    pub check: bool,
    /// Rewrite expectation files from this run's report.
    pub update_expectations: bool,
    pub timeout: std::time::Duration,
}

pub fn default_manifest_path() -> PathBuf {
    node::repo_root().join("packages/jade-js/test262/manifest.json")
}

pub fn default_expectations_dir() -> PathBuf {
    node::repo_root().join("packages/jade-js/test262/expectations")
}

/// Tests for a shard: absolute file paths plus the display path used in reports.
pub fn discover(shard: &str) -> Result<Vec<(String, PathBuf)>, String> {
    let root = if shard == "smoke" {
        node::repo_root().join("packages/jade-js/test262/fixtures/smoke")
    } else if let Some(sub) = shard.strip_prefix("test262:") {
        node::repo_root().join("vendor/test262/test").join(sub)
    } else {
        return Err(format!(
            "unknown shard {shard:?} (want `smoke` or `test262:<subdir>`)"
        ));
    };
    let mut out = Vec::new();
    walk(&root, &root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?;
    for e in entries {
        let e = e.map_err(|e| e.to_string())?;
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("js")
            // test262 `_FIXTURE.js` files are includes, not tests.
            && !p.file_name().unwrap_or_default().to_string_lossy().contains("_FIXTURE")
        {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, p.clone()));
        }
    }
    Ok(())
}

pub fn run(cfg: &RunConfig) -> Result<Report, String> {
    let manifest = Manifest::load(&cfg.manifest_path)?;
    let mut tests = discover(&cfg.shard)?;
    if let Some(n) = cfg.limit {
        tests.truncate(n);
    }
    if tests.is_empty() {
        return Err(format!("shard {:?} discovered no tests", cfg.shard));
    }

    // Phase A: plan every cell (metadata gates + compilation) without executing.
    let mut planned = Vec::new();
    for (rel, abs) in &tests {
        let src = std::fs::read_to_string(abs)
            .map_err(|e| format!("read {}: {e}", abs.display()))?;
        planned.push(plan_one(cfg, &manifest, rel, &src));
    }

    // Phase B: batch-execute every cell that needs it, then assemble outcomes.
    let mut job_refs: Vec<(usize, String)> = Vec::new();
    let mut jobs: Vec<serde_json::Value> = Vec::new();
    for (ti, t) in planned.iter().enumerate() {
        for (env, cell) in &t.cells {
            if let Planned::Exec(job) = cell {
                job_refs.push((ti, env.clone()));
                jobs.push(job.clone());
            }
        }
    }
    // `wasm-interp` needs the Node-side WASM bundle (`packages/jade-js/test262/pkg`);
    // build it once per run that requests the cell rather than checking artifacts in.
    if cfg.envs.iter().any(|e| e == pipeline::ENV_WASM_INTERP) {
        node::ensure_wasm_bundle()?;
    }
    let verdicts = node::run_cells(&jobs, cfg.timeout);
    for ((ti, env), verdict) in job_refs.into_iter().zip(verdicts) {
        planned[ti].cells.insert(env, Planned::Done(verdict));
    }
    let results = planned.into_iter().map(assemble).collect();

    let report = Report {
        version: REPORT_VERSION,
        shard: cfg.shard.clone(),
        tenant: cfg.tenant.clone(),
        results,
    };

    // Schema invariants.
    let problems = report.validate();
    if !problems.is_empty() {
        return Err(format!("report failed self-validation:\n{}", problems.join("\n")));
    }

    // Every skip must be manifest-justified.
    let mut unjustified = Vec::new();
    for r in &report.results {
        for (env, cell) in &r.cells {
            if cell.kind.is_skip() {
                if let Err(e) = manifest.justify_skip(cell, &r.meta) {
                    unjustified.push(format!("{}[{env}]: {e}", r.path));
                }
            }
        }
    }
    if !unjustified.is_empty() {
        return Err(format!(
            "skips without manifest justification:\n{}",
            unjustified.join("\n")
        ));
    }

    write_json(&cfg.report_path, &report)?;

    if cfg.update_expectations {
        let dir = &cfg.expectations_dir;
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
        let existing = load_expectations(dir)?;
        for (env, exp) in model::expectations_from_report(&report, &existing) {
            write_json(&dir.join(format!("{env}.json")), &exp)?;
        }
    }

    if cfg.check {
        let expectations = load_expectations(&cfg.expectations_dir)?;
        let drift = model::check_expectations(&report, &expectations);
        if !drift.is_empty() {
            return Err(format!("expectation drift:\n{}", drift.join("\n")));
        }
    }

    Ok(report)
}

fn load_expectations(dir: &Path) -> Result<BTreeMap<String, Expectations>, String> {
    let mut out = BTreeMap::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json") {
            let env = p.file_stem().unwrap().to_string_lossy().to_string();
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            out.insert(env, serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?);
        }
    }
    Ok(out)
}

fn write_json<T: serde::Serialize>(path: &Path, v: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| format!("write {}: {e}", path.display()))
}

/// A cell that either already has a verdict (uniform gate / compile outcome) or is a
/// job awaiting execution.
enum Planned {
    Done(CellVerdict),
    Exec(serde_json::Value),
}

struct PlannedTest {
    path: String,
    meta: TestMeta,
    cells: BTreeMap<String, Planned>,
}

/// Plan one test across all configured environment cells — everything except execution.
fn plan_one(cfg: &RunConfig, manifest: &Manifest, rel: &str, src: &str) -> PlannedTest {
    let meta = match meta::parse_frontmatter(src) {
        Ok(m) => m,
        Err(e) => {
            // A test we cannot even read metadata from fails every cell uniformly.
            let mut cells = BTreeMap::new();
            for env in &cfg.envs {
                cells.insert(env.clone(), Planned::Done(CellVerdict::fail(format!("frontmatter: {e}"))));
            }
            return PlannedTest { path: rel.to_string(), meta: TestMeta::default(), cells };
        }
    };

    // Pre-compilation gates, applied uniformly to every cell.
    let uniform_skip = uniform_skip(&meta, manifest, cfg);
    let state = if uniform_skip.is_none() {
        Some(compile_state(&meta, src, &cfg.envs))
    } else {
        None
    };

    let mut cells: BTreeMap<String, Planned> = BTreeMap::new();
    for env in &cfg.envs {
        if let Some(skip) = &uniform_skip {
            cells.insert(env.clone(), Planned::Done(skip.clone()));
            continue;
        }
        cells.insert(
            env.clone(),
            plan_cell(cfg, state.as_ref().expect("state when not skipped"), rel, env, &meta),
        );
    }

    PlannedTest { path: rel.to_string(), meta, cells }
}

/// Assemble a planned test into its outcome, applying the differential check against
/// the interpreter baseline (kind + completion value) across executed cells.
fn assemble(t: PlannedTest) -> TestOutcome {
    let PlannedTest { path, meta, cells } = t;
    let mut baseline: Option<(VerdictKind, Option<String>)> = None;
    let mut out: BTreeMap<String, CellVerdict> = BTreeMap::new();
    for (env, planned) in cells {
        let Planned::Done(cell) = planned else {
            unreachable!("all cells executed before assemble")
        };
        if env == ENV_INTERP {
            baseline = Some((cell.kind, cell.value.clone()));
        } else if let Some((bkind, bval)) = &baseline {
            if cell.kind == VerdictKind::Pass
                && *bkind == VerdictKind::Pass
                && cell.value != *bval
            {
                let got = cell.value.clone().unwrap_or_default();
                out.insert(
                    env,
                    CellVerdict {
                        kind: VerdictKind::DifferentialMismatch,
                        reason: Some(format!(
                            "baseline {ENV_INTERP} produced {:?}",
                            bval.clone().unwrap_or_default()
                        )),
                        value: None,
                        error: Some(format!("this cell produced {got:?}")),
                        duration_ms: cell.duration_ms,
                    },
                );
                continue;
            }
        }
        out.insert(env, cell);
    }
    TestOutcome { path, meta, cells: out }
}

/// Skips that apply to every cell identically: manifest flags, harness includes with no
/// primordial equivalent, and negative-runtime tests (no exception opcodes yet).
fn uniform_skip(meta: &TestMeta, manifest: &Manifest, cfg: &RunConfig) -> Option<CellVerdict> {
    if let Some(flag) = manifest.flag_skip(meta) {
        return Some(CellVerdict::skip(VerdictKind::SkipFlag, flag));
    }
    for inc in meta.required_includes() {
        match manifest.harness_includes.get(&inc) {
            Some(s) if s.status == crate::manifest::Status::Supported => {}
            // Absent includes mean "no primordial equivalent written yet".
            _ => return Some(CellVerdict::skip(VerdictKind::SkipFeature, inc.clone())),
        }
    }
    // Capability-manifest feature gate: only features the manifest explicitly calls
    // `unsupported` skip. Absent = not yet classified = run; if the test then fails,
    // the failure is real signal the ratchet records (and a manifest entry can later
    // turn a genuinely-missing feature into a clean skip).
    for feat in &meta.features {
        if let Some(s) = manifest.features.get(feat) {
            if s.status == crate::manifest::Status::Unsupported {
                return Some(CellVerdict::skip(VerdictKind::SkipFeature, feat.clone()));
            }
        }
    }
    if let Some(neg) = &meta.negative {
        if neg.phase == "runtime" {
            return Some(CellVerdict::skip(
                VerdictKind::SkipUnsupportedFrontend,
                "exceptions: no bytecode opcode for throw/try yet".to_string(),
            ));
        }
    }
    // Async tests need `$DONE` + an async-capable cell — Phase 5. Until then every
    // current cell is sync and skips uniformly (justified by manifest.flags.async,
    // which stays `partial` until async cells exist).
    if meta.has_flag("async") {
        return Some(CellVerdict::skip(VerdictKind::SkipFlag, "async"));
    }
    let _ = cfg;
    None
}

/// The compile-time verdict for one test, computed once and shared by every cell.
enum CompileState {
    Bytecode { bytes: Vec<u8>, tiers: BTreeMap<String, Result<pipeline::TierOut, String>> },
    Uniform(CellVerdict),
}

/// Wall-clock budget for compiling *all* JIT tiers of one test. Tier 2's CFG/SSA
/// round-trip has a known superlinear blowup on long straight-line tests (see
/// `docs/test262-plan.md` and the `TFunc -> Function` relooper in vendored jsaw-core) —
/// the deadline turns "shard hangs for minutes" into a visible per-test `timeout`.
const TIER_COMPILE_DEADLINE: Duration = Duration::from_secs(60);

fn compile_state(meta: &TestMeta, src: &str, envs: &[String]) -> CompileState {
    let compile_rejection = |msg: String, kind: VerdictKind| {
        // A `negative` test expecting a parse/early error PASSES when compilation
        // rejects it — that is the rejection it asked for.
        if let Some(neg) = &meta.negative {
            if neg.phase == "parse" || neg.phase == "early" {
                let mut c = CellVerdict::new(VerdictKind::Pass);
                c.reason = Some(format!("expected compile rejection: {msg}"));
                return c;
            }
        }
        // Skip reasons are manifest keys: normalize parameterized message families to
        // their stable form and keep the raw message for debugging.
        let mut c = CellVerdict::skip(kind, crate::normalize::unsupported(&msg));
        if c.reason.as_deref() != Some(msg.as_str()) {
            c.error = Some(msg);
        }
        c
    };
    match pipeline::compile_test(src) {
        CompileVerdict::Unsupported(msg) => {
            CompileState::Uniform(compile_rejection(msg, VerdictKind::SkipUnsupportedFrontend))
        }
        CompileVerdict::ParseError(msg) => {
            CompileState::Uniform(compile_rejection(msg, VerdictKind::SkipParse))
        }
        CompileVerdict::Bytecode(bytes)
            if matches!(&meta.negative, Some(n) if n.phase == "parse" || n.phase == "early") =>
        {
            // A negative parse/early test that *compiles*: our parser is more lenient
            // than the spec demands (e.g. SWC accepts some numeric-separator and
            // Annex-B forms). Deterministic parser-strictness failure — no execution.
            let _ = bytes;
            CompileState::Uniform(CellVerdict::fail(
                "compiled successfully but the test demands a parse/early rejection",
            ))
        }
        CompileVerdict::Bytecode(bytes) => {
            // Tier compilation (Tier 2's CFG/SSA round-trip in particular) is the
            // runner's most expensive per-test step — only pay it when a JIT cell is
            // actually configured for this run, and bound it by TIER_COMPILE_DEADLINE:
            // an overrun yields per-tier `timeout` verdicts instead of hanging the
            // shard. The worker thread is detached; the relooper blowup is CPU-bound
            // and does terminate, so the leak is bounded in practice.
            let jit_envs: Vec<&str> = envs
                .iter()
                .map(String::as_str)
                .filter(|e| NODE_ENVS.contains(e) && *e != ENV_INTERP)
                .collect();
            let tiers = if jit_envs.is_empty() {
                BTreeMap::new()
            } else {
                let owned: Vec<String> = jit_envs.iter().map(|s| s.to_string()).collect();
                let bytes_clone = bytes.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
                    let _ = tx.send(pipeline::compile_tier_selection(&bytes_clone, &refs));
                });
                match rx.recv_timeout(TIER_COMPILE_DEADLINE) {
                    Ok(tiers) => tiers,
                    Err(_) => jit_envs
                        .into_iter()
                        .map(|e| {
                            (
                                e.to_string(),
                                Err("TIMEOUT: tier compile exceeded deadline".to_string()),
                            )
                        })
                        .collect(),
                }
            };
            CompileState::Bytecode { bytes, tiers }
        }
    }
}

fn plan_cell(
    cfg: &RunConfig,
    state: &CompileState,
    rel: &str,
    env: &str,
    meta: &TestMeta,
) -> Planned {
    let (bytes, tiers) = match state {
        CompileState::Uniform(v) => return Planned::Done(v.clone()),
        CompileState::Bytecode { bytes, tiers } => (bytes, tiers),
    };

    let mut job = serde_json::json!({
        "test": rel,
        "env": env,
        "tenant": cfg.tenant,
        "harness": !meta.has_flag("raw"),
    });
    if INTERP_ENVS.contains(&env) {
        job["bytecode"] = serde_json::json!(bytes);
    } else if NODE_ENVS.contains(&env) {
        match tiers.get(env) {
            Some(Ok(t)) => {
                job["body"] = serde_json::json!(t.body);
                job["prelude"] = serde_json::json!(t.prelude);
            }
            Some(Err(e)) => {
                if let Some(msg) = e.strip_prefix("TIMEOUT: ") {
                    let mut c = CellVerdict::new(VerdictKind::Timeout);
                    c.error = Some(format!("JIT compile ({env}): {msg}"));
                    return Planned::Done(c);
                }
                let mut c = CellVerdict::new(VerdictKind::Crash);
                c.error = Some(format!("JIT compile ({env}): {e}"));
                return Planned::Done(c);
            }
            None => unreachable!("NODE_ENVS contains env but tiers lacks it"),
        }
    } else {
        return Planned::Done(CellVerdict::fail(format!("unknown environment cell {env:?}")));
    }

    Planned::Exec(job)
}

//! Orchestration: discover tests, compile, dispatch to the Node driver per environment
//! cell, differential-compare, justify skips against the manifest, check expectations,
//! emit the report.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::manifest::Manifest;
use crate::meta::{self, TestMeta};
use crate::model::{
    self, CellVerdict, Expectations, Report, TestOutcome, VerdictKind, REPORT_VERSION,
};
use crate::node;
use crate::pipeline::{self, CompileVerdict, ENV_INTERP, NODE_ENVS};

pub struct RunConfig {
    /// `smoke` (the synthetic fixtures) or `test262:<subdir>` (a vendored subtree).
    pub shard: String,
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
    let tests = discover(&cfg.shard)?;
    if tests.is_empty() {
        return Err(format!("shard {:?} discovered no tests", cfg.shard));
    }

    let mut results = Vec::new();
    for (rel, abs) in &tests {
        let src = std::fs::read_to_string(abs)
            .map_err(|e| format!("read {}: {e}", abs.display()))?;
        results.push(run_one(cfg, &manifest, rel, &src));
    }

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
        for (env, exp) in model::expectations_from_report(&report) {
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

/// Run one test across all configured environment cells.
fn run_one(cfg: &RunConfig, manifest: &Manifest, rel: &str, src: &str) -> TestOutcome {
    let meta = match meta::parse_frontmatter(src) {
        Ok(m) => m,
        Err(e) => {
            // A test we cannot even read metadata from fails every cell uniformly.
            let mut cells = BTreeMap::new();
            for env in &cfg.envs {
                cells.insert(env.clone(), CellVerdict::fail(format!("frontmatter: {e}")));
            }
            return TestOutcome { path: rel.to_string(), meta: TestMeta::default(), cells };
        }
    };

    // Pre-compilation gates, applied uniformly to every cell.
    let uniform_skip = uniform_skip(&meta, manifest, cfg);
    let state = if uniform_skip.is_none() {
        Some(compile_state(&meta, src))
    } else {
        None
    };

    let mut cells: BTreeMap<String, CellVerdict> = BTreeMap::new();
    let mut baseline: Option<(VerdictKind, Option<String>)> = None;

    for env in &cfg.envs {
        if let Some(skip) = &uniform_skip {
            cells.insert(env.clone(), skip.clone());
            continue;
        }
        let cell = run_cell(cfg, state.as_ref().expect("state when not skipped"), rel, env);
        // Differential check against the interpreter baseline (kind + completion value).
        if env == ENV_INTERP {
            baseline = Some((cell.kind, cell.value.clone()));
        } else if let Some((bkind, bval)) = &baseline {
            if cell.kind == VerdictKind::Pass
                && *bkind == VerdictKind::Pass
                && cell.value != *bval
            {
                let got = cell.value.clone().unwrap_or_default();
                cells.insert(
                    env.clone(),
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
        cells.insert(env.clone(), cell);
    }

    TestOutcome { path: rel.to_string(), meta, cells }
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
    if let Some(neg) = &meta.negative {
        if neg.phase == "runtime" {
            return Some(CellVerdict::skip(
                VerdictKind::SkipUnsupportedFrontend,
                "exceptions: no bytecode opcode for throw/try yet".to_string(),
            ));
        }
    }
    let _ = cfg;
    None
}

/// The compile-time verdict for one test, computed once and shared by every cell.
enum CompileState {
    Bytecode { bytes: Vec<u8>, tiers: BTreeMap<String, Result<pipeline::TierOut, String>> },
    Uniform(CellVerdict),
}

fn compile_state(meta: &TestMeta, src: &str) -> CompileState {
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
        CellVerdict::skip(kind, msg)
    };
    match pipeline::compile_test(src) {
        CompileVerdict::Unsupported(msg) => {
            CompileState::Uniform(compile_rejection(msg, VerdictKind::SkipUnsupportedFrontend))
        }
        CompileVerdict::ParseError(msg) => {
            CompileState::Uniform(compile_rejection(msg, VerdictKind::SkipParse))
        }
        CompileVerdict::Bytecode(bytes) => {
            let tiers = pipeline::compile_tiers(&bytes);
            CompileState::Bytecode { bytes, tiers }
        }
    }
}

fn run_cell(
    cfg: &RunConfig,
    state: &CompileState,
    rel: &str,
    env: &str,
) -> CellVerdict {
    let (bytes, tiers) = match state {
        CompileState::Uniform(v) => return v.clone(),
        CompileState::Bytecode { bytes, tiers } => (bytes, tiers),
    };

    let mut job = serde_json::json!({
        "test": rel,
        "env": env,
        "tenant": cfg.tenant,
    });
    if env == ENV_INTERP {
        job["bytecode"] = serde_json::json!(bytes);
    } else if NODE_ENVS.contains(&env) {
        match tiers.get(env) {
            Some(Ok(t)) => {
                job["body"] = serde_json::json!(t.body);
                job["prelude"] = serde_json::json!(t.prelude);
            }
            Some(Err(e)) => {
                let mut c = CellVerdict::new(VerdictKind::Crash);
                c.error = Some(format!("JIT compile ({env}): {e}"));
                return c;
            }
            None => unreachable!("NODE_ENVS contains env but tiers lacks it"),
        }
    } else {
        return CellVerdict::fail(format!("unknown environment cell {env:?}"));
    }

    node::run_cell(&job, cfg.timeout)
}

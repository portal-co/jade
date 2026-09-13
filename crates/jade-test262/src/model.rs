//! Verdict taxonomy and the JSON report schema shared by the Rust and TS runners.
//!
//! The taxonomy is the one from `docs/test262-plan.md`: a test × environment cell
//! produces exactly one [`VerdictKind`], and every skip/flaky class carries a machine-
//! checkable `reason`. `Report::validate` enforces the invariants that make a report
//! "schema-valid" in the plan's Phase-0 sense — the TS runner mirrors these checks in
//! `packages/jade-js/test262/report.ts`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::meta::TestMeta;

/// Every verdict a single test × environment cell can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerdictKind {
    /// Compiled, ran, assertions held (or `negative` expectations met).
    Pass,
    /// Ran and violated the test, or failed in a way the test did not allow.
    Fail,
    /// This environment disagreed with the interpreter baseline on the same bytecode —
    /// a bug regardless of what test262 would call the outcome.
    DifferentialMismatch,
    /// `FrontendError::Unsupported`; `reason` carries the exact message.
    SkipUnsupportedFrontend,
    /// `FrontendError::Parse`/TAC on a non-negative test.
    SkipParse,
    /// The capability manifest says this environment lacks the feature; `reason` names it.
    SkipFeature,
    /// Environment-level flag mismatch (`module`, `onlyStrict`, `async` on a sync cell…).
    SkipFlag,
    /// The host process crashed or the harness wedged. Always a failure.
    Crash,
    /// Exceeded the per-test wall-clock limit. Always a failure.
    Timeout,
}

impl VerdictKind {
    pub fn is_skip(self) -> bool {
        matches!(
            self,
            Self::SkipUnsupportedFrontend | Self::SkipParse | Self::SkipFeature | Self::SkipFlag
        )
    }

    /// Verdicts that block a run when expectations say otherwise.
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Fail | Self::DifferentialMismatch | Self::Crash | Self::Timeout)
    }
}

/// One cell's outcome. `value`/`error` are JSON-encoded snapshots used for differential
/// comparison and reporting; they are never parsed back into semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellVerdict {
    pub kind: VerdictKind,
    /// Machine-checkable justification, required for every skip and for
    /// `differential-mismatch` (which names the baseline cell disagreed with).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// JSON-encoded completion value, when the test produced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Error text when the cell failed/crashed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The thrown error's `name` when a cell threw one (host `Error.name` on JS cells,
    /// `TenantError` variant name or the thrown guest object's `name` own-property on
    /// native) — the machine-checkable channel for `negative: {phase: runtime}`
    /// classification (docs/exceptions-plan.md §5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_name: Option<String>,
    pub duration_ms: u64,
}

impl CellVerdict {
    pub fn new(kind: VerdictKind) -> Self {
        Self { kind, reason: None, value: None, error: None, error_name: None, duration_ms: 0 }
    }

    pub fn skip(kind: VerdictKind, reason: impl Into<String>) -> Self {
        debug_assert!(kind.is_skip());
        Self { reason: Some(reason.into()), ..Self::new(kind) }
    }

    pub fn fail(reason: impl Into<String>) -> Self {
        Self { reason: Some(reason.into()), ..Self::new(VerdictKind::Fail) }
    }
}

/// All environment cells for one test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestOutcome {
    /// test262-style path relative to the tests root, e.g.
    /// `language/literals/numeric/binary.js` or `fixtures/smoke/literal-number.js`.
    pub path: String,
    pub meta: TestMeta,
    pub cells: BTreeMap<String, CellVerdict>,
}

/// The report both runners emit. `version` is the schema version; bump on any
/// incompatible change and update both runners in the same commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub shard: String,
    /// Tenant column the run used (e.g. `multi`, `single`).
    pub tenant: String,
    pub results: Vec<TestOutcome>,
}

pub const REPORT_VERSION: u32 = 1;

impl Report {
    /// Structural invariants of a schema-valid report. Returns every violation found
    /// (empty = valid). Kept deliberately mechanical so the TS port matches 1:1.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.version != REPORT_VERSION {
            problems.push(format!("version {} != {REPORT_VERSION}", self.version));
        }
        let mut seen = BTreeSet::new();
        for r in &self.results {
            if !seen.insert(r.path.as_str()) {
                problems.push(format!("duplicate test path {}", r.path));
            }
            for (env, cell) in &r.cells {
                if cell.kind.is_skip() && cell.reason.as_deref().unwrap_or("").is_empty() {
                    problems.push(format!("{}[{env}]: skip verdict without a reason", r.path));
                }
                if cell.kind == VerdictKind::DifferentialMismatch
                    && cell.reason.as_deref().unwrap_or("").is_empty()
                {
                    problems.push(format!(
                        "{}[{env}]: differential-mismatch without a reason",
                        r.path
                    ));
                }
            }
        }
        problems
    }

    /// Rollup counts per (environment, verdict kind), for summary printing.
    pub fn rollup(&self) -> BTreeMap<String, BTreeMap<VerdictKind, usize>> {
        let mut out: BTreeMap<String, BTreeMap<VerdictKind, usize>> = BTreeMap::new();
        for r in &self.results {
            for (env, cell) in &r.cells {
                *out.entry(env.clone()).or_default().entry(cell.kind).or_default() += 1;
            }
        }
        out
    }
}

/// The per-environment expectation files (`expectations/<environment>.json`). Maps a
/// test path to the verdict it is expected to produce; anything not listed is expected
/// to `pass` — the ratchet only ever has to name the exceptions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Expectations {
    pub version: u32,
    #[serde(default)]
    pub tests: BTreeMap<String, Expectation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expectation {
    pub verdict: VerdictKind,
    /// Optional link to the issue/notes explaining a non-pass expectation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Compare a report against per-environment expectations. Returns drift lines
/// (`path[env]: expected X, got Y`); empty means the ratchet holds.
pub fn check_expectations(
    report: &Report,
    expectations: &BTreeMap<String, Expectations>,
) -> Vec<String> {
    let mut drift = Vec::new();
    for r in &report.results {
        for (env, cell) in &r.cells {
            let expected = expectations
                .get(env)
                .and_then(|e| e.tests.get(&r.path))
                .map(|e| e.verdict)
                .unwrap_or(VerdictKind::Pass);
            if cell.kind != expected {
                drift.push(format!(
                    "{}[{env}]: expected {}, got {}",
                    r.path,
                    kind_name(expected),
                    kind_name(cell.kind)
                ));
            }
        }
    }
    drift
}

/// Re-baseline: derive expectations from a report (used by `--update-expectations`).
/// `existing` (per env) is merged in: tests covered by this report take their new
/// verdict (or drop out if they now pass); tests from *other* shards keep their entry,
/// so successive per-shard updates don't clobber one another.
pub fn expectations_from_report(
    report: &Report,
    existing: &BTreeMap<String, Expectations>,
) -> BTreeMap<String, Expectations> {
    let mut out: BTreeMap<String, Expectations> = existing.clone();
    for r in &report.results {
        for (env, cell) in &r.cells {
            let env_exp = out.entry(env.clone()).or_default();
            if cell.kind != VerdictKind::Pass {
                env_exp.tests.insert(
                    r.path.clone(),
                    Expectation { verdict: cell.kind, note: None },
                );
            } else {
                env_exp.tests.remove(&r.path);
            }
        }
    }
    for e in out.values_mut() {
        e.version = 1;
    }
    out
}

pub fn kind_name(k: VerdictKind) -> &'static str {
    match k {
        VerdictKind::Pass => "pass",
        VerdictKind::Fail => "fail",
        VerdictKind::DifferentialMismatch => "differential-mismatch",
        VerdictKind::SkipUnsupportedFrontend => "skip-unsupported-frontend",
        VerdictKind::SkipParse => "skip-parse",
        VerdictKind::SkipFeature => "skip-feature",
        VerdictKind::SkipFlag => "skip-flag",
        VerdictKind::Crash => "crash",
        VerdictKind::Timeout => "timeout",
    }
}

//! The compile oracle: test source → frontend bytecode → per-tier JIT output.
//!
//! This is the Rust half of the pipeline under test. Compilation *errors* are verdict
//! inputs (a `FrontendError::Unsupported` is a skip class, not a failure), so nothing
//! here panics on bad input — every error is returned for the orchestrator to classify.

use std::collections::BTreeMap;

use portal_solutions_jade_vm_frontend::{FrontendError, compile_to_bytecode};
use portal_solutions_jade_vm_jit::{Config, VecRegistry};
use portal_solutions_jade_vm_jit_swc as jit_swc;

/// Environment cell ids executed by the Node driver. Kept as plain strings in reports
/// so later rows (`wasm-interp`, `native-rust`, …) slot in without a schema change.
pub const ENV_INTERP: &str = "interp";
pub const ENV_JIT_T0: &str = "jit-t0";
pub const ENV_JIT_T1: &str = "jit-t1";
pub const ENV_JIT_T2: &str = "jit-t2";
/// The WASM interpreter (`jade-vm-wasm`'s generated `run_virtualized`) executing in
/// Node against the same TS tenant + primordial realm as `interp`.
pub const ENV_WASM_INTERP: &str = "wasm-interp";

/// The Node cells the Phase-1/2 ratchet is pinned on. `wasm-interp` is deliberately
/// *not* part of this default matrix while its Phase-3 rollout is in flight — run it
/// explicitly via `--env wasm-interp` or one of the Phase-3 opt-in sets below.
pub const NODE_ENVS: &[&str] = &[ENV_INTERP, ENV_JIT_T0, ENV_JIT_T1, ENV_JIT_T2];

/// Every interpreter environment (TS + WASM). Used by the runner to know which cells
/// run bytecode directly versus needing a JIT `TierOut`.
pub const INTERP_ENVS: &[&str] = &[ENV_INTERP, ENV_WASM_INTERP];

/// What the frontend made of one test's source.
pub enum CompileVerdict {
    /// Bytecode, ready to execute or JIT.
    Bytecode(Vec<u8>),
    /// `FrontendError::Parse`/`Tac` — a parse-level gap (or an expected `negative` test).
    ParseError(String),
    /// `FrontendError::Unsupported` — a known-class bytecode gap; message preserved.
    Unsupported(String),
}

pub fn compile_test(src: &str) -> CompileVerdict {
    match compile_to_bytecode(src) {
        Ok(bytes) => CompileVerdict::Bytecode(bytes),
        Err(FrontendError::Parse(m)) | Err(FrontendError::Tac(m)) => {
            CompileVerdict::ParseError(m)
        }
        Err(FrontendError::Unsupported(u)) => CompileVerdict::Unsupported(u.0),
        Err(FrontendError::Opt(m)) => CompileVerdict::ParseError(format!("opt: {m}")),
    }
}

/// JIT output for one tier: the function body plus the registry prelude that declares
/// any nested functions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TierOut {
    pub body: String,
    pub prelude: String,
}

/// Compile `bytes` through all three JIT tiers. Each tier is independent; a tier error
/// is recorded as a string, not propagated, so one tier's gap doesn't mask another's.
pub fn compile_tiers(bytes: &[u8]) -> BTreeMap<String, Result<TierOut, String>> {
    compile_tier_selection(bytes, &[ENV_JIT_T0, ENV_JIT_T1, ENV_JIT_T2])
}

/// Compile only the named tiers (e.g. `[ENV_JIT_T2]`) — Tier 2's CFG/SSA round-trip is
/// the runner's most expensive per-test step, so callers skip tiers they don't need.
pub fn compile_tier_selection(
    bytes: &[u8],
    tiers: &[&str],
) -> BTreeMap<String, Result<TierOut, String>> {
    let mut out = BTreeMap::new();
    for tier in tiers {
        let r = match *tier {
            ENV_JIT_T0 => {
                portal_solutions_jade_vm_jit::compile(bytes, VecRegistry::new(), Config::default())
                    .map(|(body, reg)| TierOut { body, prelude: reg.prelude() })
            }
            ENV_JIT_T1 => {
                let mut cfg1 = Config::default();
                cfg1.prefer_reloop = true;
                portal_solutions_jade_vm_jit::compile(bytes, VecRegistry::new(), cfg1)
                    .map(|(body, reg)| TierOut { body, prelude: reg.prelude() })
            }
            ENV_JIT_T2 => jit_swc::compile(bytes, Config::default())
                .map(|(body, reg)| TierOut { body, prelude: reg.prelude() }),
            other => Err(format!("unknown tier {other:?}")),
        };
        out.insert(tier.to_string(), r);
    }
    out
}

/// The JSON artifact emitted by `jade-test262 compile --file`, consumed by the TS
/// orchestrator (`run.ts`) so it never needs its own frontmatter parser.
#[derive(Debug, serde::Serialize)]
pub struct CompileArtifact {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<crate::meta::TestMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytecode: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<BTreeMap<String, Result<TierOut, String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn compile_artifact(src: &str) -> CompileArtifact {
    let meta = match crate::meta::parse_frontmatter(src) {
        Ok(m) => m,
        Err(e) => {
            return CompileArtifact {
                ok: false,
                meta: None,
                bytecode: None,
                tiers: None,
                error_kind: Some("frontmatter".into()),
                message: Some(e.to_string()),
            };
        }
    };
    match compile_test(src) {
        CompileVerdict::Bytecode(bytes) => {
            // Diagnostic escape hatch: JADE_T262_ONLY_TIERS=t0,t1 restricts which tiers
            // the artifact compiles (empty = bytecode only). Used to isolate per-tier
            // compile-time behavior.
            let only: Option<Vec<String>> = std::env::var("JADE_T262_ONLY_TIERS")
                .ok()
                .map(|v| v.split(',').map(|s| format!("jit-{}", s.trim())).collect());
            let tiers = match &only {
                Some(keep) => compile_tier_selection(
                    &bytes,
                    &keep.iter().map(String::as_str).collect::<Vec<_>>(),
                ),
                None => compile_tiers(&bytes),
            };
            CompileArtifact {
                ok: true,
                meta: Some(meta),
                tiers: Some(tiers),
                bytecode: Some(bytes),
                error_kind: None,
                message: None,
            }
        }
        CompileVerdict::ParseError(m) => CompileArtifact {
            ok: false,
            meta: Some(meta),
            bytecode: None,
            tiers: None,
            error_kind: Some("parse".into()),
            message: Some(m),
        },
        CompileVerdict::Unsupported(m) => CompileArtifact {
            ok: false,
            meta: Some(meta),
            bytecode: None,
            tiers: None,
            error_kind: Some("unsupported".into()),
            message: Some(m),
        },
    }
}

//! The capability manifest (`packages/jade-js/test262/manifest.json`) — the single source
//! of truth for what each environment supports, and the mechanical justification for
//! every skip verdict the runner emits. A skip the manifest cannot justify fails the run.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::meta::TestMeta;
use crate::model::{CellVerdict, VerdictKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Supported,
    Unsupported,
    /// Runs but expected to fail; the note names the missing sub-behavior.
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Support {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// test262 `features:` tags → support level.
    #[serde(default)]
    pub features: BTreeMap<String, Support>,
    /// Harness include files (`assert.js`, `propertyHelper.js`, …) → support level.
    /// An include absent here means "no primordial equivalent exists yet".
    #[serde(default, rename = "harnessIncludes")]
    pub harness_includes: BTreeMap<String, Support>,
    /// Normalized `FrontendError::Unsupported` messages the project acknowledges as
    /// known bytecode/frontend gaps, each with a note. The runner's observed messages
    /// must be a subset of these keys (checked 1:1 in the Phase 1 criterion).
    #[serde(default, rename = "unsupportedConstructs")]
    pub unsupported_constructs: BTreeMap<String, Support>,
    /// test262 `flags:` → support level.
    #[serde(default)]
    pub flags: BTreeMap<String, Support>,
}

impl Manifest {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
    }

    /// Whether a skip verdict is justified by this manifest. `Ok(())` = justified,
    /// `Err` = the reason the runner must treat the skip as a runner/manifest bug.
    pub fn justify_skip(&self, cell: &CellVerdict, meta: &TestMeta) -> Result<(), String> {
        let reason = cell.reason.as_deref().unwrap_or("");
        match cell.kind {
            VerdictKind::SkipUnsupportedFrontend => {
                if self.unsupported_constructs.contains_key(reason) {
                    Ok(())
                } else {
                    Err(format!(
                        "observed Unsupported message not in manifest.unsupportedConstructs: {reason:?}"
                    ))
                }
            }
            VerdictKind::SkipFeature => {
                // The reason names either a `features:` tag or a harness include.
                let known = self
                    .features
                    .get(reason)
                    .or_else(|| self.harness_includes.get(reason));
                match known {
                    Some(s) if s.status != Status::Supported => Ok(()),
                    Some(_) => Err(format!(
                        "skip-feature {reason:?} but manifest marks it supported"
                    )),
                    None => {
                        // A harness include with no entry is implicitly unsupported:
                        // no primordial equivalent has been written. Require the test to
                        // actually name it so the reason is checkable.
                        if meta.required_includes().iter().any(|i| i == reason)
                            && !self.harness_includes.contains_key(reason)
                        {
                            Ok(())
                        } else {
                            Err(format!("skip-feature reason {reason:?} names nothing in the manifest and no unlisted include of the test"))
                        }
                    }
                }
            }
            VerdictKind::SkipFlag => match self.flags.get(reason) {
                Some(s) if s.status != Status::Supported => Ok(()),
                Some(_) => Err(format!("skip-flag {reason:?} but manifest marks it supported")),
                None => Err(format!("skip-flag {reason:?} missing from manifest.flags")),
            },
            // A parse gap on a test that didn't ask for it is always "justified" as a
            // skip class — it means SWC/test262 syntax drift, which the report surfaces
            // directly; no manifest key could name it more precisely than the message.
            VerdictKind::SkipParse => Ok(()),
            other => Err(format!("not a skip verdict: {other:?}")),
        }
    }

    /// Pre-execution classification: should this test be skipped on flag grounds before
    /// compilation is even attempted? Returns the flag name to record as the reason.
    pub fn flag_skip(&self, meta: &TestMeta) -> Option<String> {
        for f in &meta.flags {
            if let Some(s) = self.flags.get(f) {
                if s.status == Status::Unsupported {
                    return Some(f.clone());
                }
            }
        }
        None
    }
}

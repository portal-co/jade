//! Shared TAC-level canonicalizer + SSA constant-fold/DCE pass, usable by both
//! `crates/jade-vm-frontend` (before lowering to Jade bytecode) and an optional JIT
//! plugin that reconstructs a real `swc-cfg` (Tier 2, `crates/jade-vm-jit-swc`) — see
//! `docs/bytecode-cfg-plan.md`. Depends only on `portal-jsc-swc-tac` +
//! `portal-jsc-swc-ssa`, not the full `jsaw-core` stack (no SWC AST/codegen).
//!
//! [`optimize_tfunc`] round-trips a [`TFunc`] through SSA form
//! (`TFunc -> SFunc -> TFunc`, both directions already provided by `portal-jsc-swc-ssa`)
//! and applies `simplify_conditions` (constant-folds a `CondJmp` whose condition is a
//! known boolean literal into a plain `Jmp`, dropping the untaken branch), `simplify_loads`
//! (forwards a `LoadId` to its most recent same-block `StoreId`, an SSA-level redundant-load
//! elimination), and `simplify_justs` (collapses `Item::Just` alias chains) in between —
//! all existing `portal-jsc-swc-ssa` passes, not reimplemented here. `discover_reachable`
//! (in `crates/jade-vm-frontend`) already drops any TAC block left unreachable by this
//! folding, so no separate block-level DCE step is needed here.

use portal_jsc_swc_ssa::{Error, SFunc};
use portal_jsc_swc_tac::TFunc;

/// Canonicalize and constant-fold/DCE `tfunc` via an SSA round-trip. Returns a
/// semantically-equivalent [`TFunc`] — never smaller in *structure* (may allocate extra
/// blocks/renamed variables as an artifact of the round-trip), but with constant branches
/// resolved and redundant loads/aliases collapsed, which is what shrinks the bytecode
/// `crates/jade-vm-frontend` ultimately emits from it.
pub fn optimize_tfunc(tfunc: &TFunc) -> Result<TFunc, Error> {
    let mut sfunc = SFunc::try_from(tfunc)?;
    sfunc.cfg.simplify_conditions();
    sfunc.cfg.simplify_loads();
    sfunc.cfg.simplify_justs();
    TFunc::try_from(&sfunc)
}

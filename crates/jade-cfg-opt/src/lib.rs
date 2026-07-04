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
//! elimination), `simplify_justs` (collapses `Item::Just` alias chains), and `inline_iifes`
//! (inlines a direct call to an immediately-invoked function expression whose body is a
//! straight-line chain of blocks -- see `docs/pluggable-tenant-interface-plan.md`'s addendum
//! on the tenant-method `this`-rewrite, which is exactly the shape this eliminates) in
//! between -- all existing `portal-jsc-swc-ssa` passes, not reimplemented here.
//! `discover_reachable` (in `crates/jade-vm-frontend`) already drops any TAC block left
//! unreachable by this folding, so no separate block-level DCE step is needed here.
//!
//! Deliberately **not** calling `SCfg::strip_useless()` here: it has a pre-existing gap
//! (vendored, out of scope to patch blindly right now) -- it only scans `Item`/`Jmp`/
//! `CondJmp`/`Switch`-target references when deciding whether an `Item::Func`/`Undef`/`Lit`
//! value is unused, not a bare `Return`/`Throw` terminator's own referenced value -- so it
//! can strip a value's computation while a terminator still names it. `inline_iifes` may
//! leave a now-unreferenced `Item::Func` value (the inlined-away callee literal) behind;
//! that's harmless dead code (an unused function-expression assignment in the emitted JS),
//! not a correctness issue, so it's fine to skip cleaning it up here.

use portal_jsc_swc_ssa::{Error, SFunc};
use portal_jsc_swc_tac::TFunc;

/// Canonicalize and constant-fold/DCE `tfunc` via an SSA round-trip. Returns a
/// semantically-equivalent [`TFunc`] — never smaller in *structure* (may allocate extra
/// blocks/renamed variables as an artifact of the round-trip), but with constant branches
/// resolved, redundant loads/aliases collapsed, and single-block IIFEs inlined, which is what
/// shrinks the bytecode `crates/jade-vm-frontend` ultimately emits from it.
pub fn optimize_tfunc(tfunc: &TFunc) -> Result<TFunc, Error> {
    let mut sfunc = SFunc::try_from(tfunc)?;
    sfunc.cfg.simplify_conditions();
    sfunc.cfg.simplify_loads();
    sfunc.cfg.simplify_justs();
    sfunc.cfg.inline_iifes();
    TFunc::try_from(&sfunc)
}

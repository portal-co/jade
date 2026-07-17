//! Tier 1 (see `docs/bytecode-cfg-plan.md`): restructure the Tier-0 block-dispatch
//! loop's discovered blocks back into native JS `while`/`if`/`switch`/`return`/labeled
//! `break`/`continue`, via `ssa-reloop2` — the *same* Stackifier algorithm
//! `crates/jade-vm-frontend` used to use, and the one `jsaw-core`'s own `swc-cfg` crate
//! uses for its `Cfg -> Function` conversion (see Tier 2, `crates/jade-vm-jit-swc`) — but
//! wired here with zero SWC dependency: just `cfg-traits` + `ssa-reloop2` +
//! `arena-traits`, matching the "loop-switching... independent of the whole jsaw-core
//! machinery" fallback path.
//!
//! Unlike lowering to Jade bytecode (which has no `goto`, forcing flag-based workarounds
//! for early `return`/`break`/`continue` in `crates/jade-vm-frontend`'s pre-CFG-native
//! history), JS has real non-local exits (`return`/labeled `break`/`continue`), so none of
//! the old `not_returned_flag`/`reject_return_inside_loop` workarounds are needed here.
//!
//! **However**, `ssa-reloop2`'s reconverge heuristic (`find_reconverge`'s "all blocks
//! owned" fallback) can still place an unrelated block into a shared `.next`/tail
//! position purely due to postorder-traversal coincidence, even when it's only reachable
//! from one specific branch with no true reconvergence — e.g. two independent
//! `return`-terminated leaves chained as if sequential (confirmed by a real test case:
//! `return_inside_loop_body_executes_correctly`, which reliably reproduced exactly this).
//! Contrary to the original hope that a bare `return` is always safe regardless of
//! hoisting, chaining TWO such leaves sequentially means the first one's `return`
//! prevents ever reaching the second, silently changing behavior. The fix used here:
//! every emitted block is unconditionally gated behind its own `__cff` ("control-flow
//! flag") check (see `emit_block`'s `Simple` arm and `emit_branch_arm`'s doc comment) — a
//! real JS local recording which successor was actually taken, so each block only runs
//! when it's truly the one reached, regardless of where `ssa-reloop2` places it
//! structurally. This is the same `cff` technique `swc-cfg`'s own `Cfg::process_block`
//! uses (with a JS string switch instead of a plain local) for the same `Multiple` node
//! shape — Tier 2 should be watched for the same failure mode.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::iter::{empty, once};

use portal_solutions_jade_vm::{Operand, Operation};
use ssa_reloop2::{BranchMode, StructuredBlock};

use crate::{Block, Config, FnRegistry, JsJit, discover_blocks, emit_op, resolve};

/// A block index in the [`CfgFunc`] adapter — opaque, unrelated to the real bytecode's
/// byte offsets (erased once `ssa-reloop2` only needs abstract block identity).
type BlockIdx = usize;

/// One block's terminator, translated from `Operation`'s `Jmp`/`CondJmp`/`Switch`/`Ret`
/// into `BlockIdx`-addressed targets.
enum Term {
    Ret(Operand),
    Jmp(BlockIdx),
    CondJmp {
        cond: Operand,
        if_true: BlockIdx,
        if_false: BlockIdx,
    },
    Switch {
        val: Operand,
        cases: Vec<(u32, BlockIdx)>,
        default: BlockIdx,
    },
}

struct CfgBlock {
    ops: Vec<Operation>,
    term: Term,
}

/// Minimal `usize`-indexed arena — just enough to satisfy `arena_traits::Arena` for
/// `ssa_reloop2::go`.
struct VecArena(Vec<CfgBlock>);

impl core::ops::Index<BlockIdx> for VecArena {
    type Output = CfgBlock;
    fn index(&self, i: BlockIdx) -> &CfgBlock {
        &self.0[i]
    }
}
impl core::ops::IndexMut<BlockIdx> for VecArena {
    fn index_mut(&mut self, i: BlockIdx) -> &mut CfgBlock {
        &mut self.0[i]
    }
}
impl arena_traits::IndexAlloc<BlockIdx> for VecArena {
    fn alloc(&mut self, v: CfgBlock) -> BlockIdx {
        self.0.push(v);
        self.0.len() - 1
    }
}
impl arena_traits::IndexIter<BlockIdx> for VecArena {
    fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = BlockIdx> + 'a> {
        Box::new(0..self.0.len())
    }
}

struct CfgFunc {
    blocks: VecArena,
    entry: BlockIdx,
}

impl cfg_traits::Func for CfgFunc {
    type Block = BlockIdx;
    type Blocks = VecArena;
    fn blocks(&self) -> &VecArena {
        &self.blocks
    }
    fn blocks_mut(&mut self) -> &mut VecArena {
        &mut self.blocks
    }
    fn entry(&self) -> BlockIdx {
        self.entry
    }
}
impl cfg_traits::Block<CfgFunc> for CfgBlock {
    type Terminator = Term;
    fn term(&self) -> &Term {
        &self.term
    }
    fn term_mut(&mut self) -> &mut Term {
        &mut self.term
    }
}
// `Target`'s own `Target = Self` bound (see `cfg_traits::Target`) requires that `BlockIdx`
// trivially implement `Term<CfgFunc>` over itself (a single-target "terminator" of one
// block) — mirroring `swc-cfg`'s `impl cfg_traits::Term<Func> for BlockId`.
impl cfg_traits::Term<CfgFunc> for BlockIdx {
    type Target = BlockIdx;
    fn targets<'a>(&'a self) -> Box<dyn Iterator<Item = &'a BlockIdx> + 'a>
    where
        CfgFunc: 'a,
    {
        Box::new(once(self))
    }
    fn targets_mut<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut BlockIdx> + 'a>
    where
        CfgFunc: 'a,
    {
        Box::new(once(self))
    }
}
impl cfg_traits::Target<CfgFunc> for BlockIdx {
    fn block(&self) -> BlockIdx {
        *self
    }
    fn block_mut(&mut self) -> &mut BlockIdx {
        self
    }
}
impl cfg_traits::Term<CfgFunc> for Term {
    type Target = BlockIdx;
    fn targets<'a>(&'a self) -> Box<dyn Iterator<Item = &'a BlockIdx> + 'a>
    where
        CfgFunc: 'a,
    {
        match self {
            Term::Ret(_) => Box::new(empty()),
            Term::Jmp(t) => Box::new(once(t)),
            Term::CondJmp {
                if_true, if_false, ..
            } => Box::new([if_true, if_false].into_iter()),
            Term::Switch { cases, default, .. } => {
                Box::new(cases.iter().map(|(_, t)| t).chain(once(default)))
            }
        }
    }
    fn targets_mut<'a>(&'a mut self) -> Box<dyn Iterator<Item = &'a mut BlockIdx> + 'a>
    where
        CfgFunc: 'a,
    {
        match self {
            Term::Ret(_) => Box::new(empty()),
            Term::Jmp(t) => Box::new(once(t)),
            Term::CondJmp {
                if_true, if_false, ..
            } => Box::new([if_true, if_false].into_iter()),
            Term::Switch { cases, default, .. } => {
                Box::new(cases.iter_mut().map(|(_, t)| t).chain(once(default)))
            }
        }
    }
}

/// Build a [`CfgFunc`] from Tier 0's discovered blocks (keyed by byte offset), re-indexed
/// to plain `usize`s (`entry_offset` always lands at index `0`).
fn build_cfg_func(mut blocks: BTreeMap<usize, Block>, entry_offset: usize) -> CfgFunc {
    let mut offsets: Vec<usize> = blocks.keys().copied().collect();
    offsets.sort_unstable();
    if let Some(pos) = offsets.iter().position(|&o| o == entry_offset) {
        offsets.swap(0, pos);
    }
    let offset_to_idx: BTreeMap<usize, BlockIdx> =
        offsets.iter().enumerate().map(|(i, &o)| (o, i)).collect();
    let idx = |offset: u32| offset_to_idx[&(offset as usize)];

    let vec_blocks: Vec<CfgBlock> = offsets
        .iter()
        .map(|o| {
            let block = blocks.remove(o).expect("offset came from blocks.keys()");
            let term = match block.term {
                Operation::Ret(val) => Term::Ret(val),
                Operation::Jmp { target } => Term::Jmp(idx(target)),
                Operation::CondJmp {
                    cond,
                    if_true,
                    if_false,
                } => Term::CondJmp {
                    cond,
                    if_true: idx(if_true),
                    if_false: idx(if_false),
                },
                Operation::Switch {
                    val,
                    cases,
                    default_target,
                } => Term::Switch {
                    val,
                    cases: cases.into_iter().map(|(cv, t)| (cv, idx(t))).collect(),
                    default: idx(default_target),
                },
                _ => unreachable!("Tier 0 only ever discovers Ret/Jmp/CondJmp/Switch terminators"),
            };
            CfgBlock {
                ops: block.ops,
                term,
            }
        })
        .collect();
    CfgFunc {
        blocks: VecArena(vec_blocks),
        entry: 0,
    }
}

fn label_for(loop_id: u32) -> String {
    format!("L{loop_id}")
}

/// Emit `sb`'s ops/terminator, then whatever structurally follows.
fn emit_block<R: FnRegistry, N: portal_jit_host_names::HostMethodNames<crate::JadeTenantMethod>>(
    jit: &mut JsJit<R, N>,
    code: &[u8],
    cfg: &CfgFunc,
    sb: &StructuredBlock<BlockIdx>,
    loops: &mut Vec<(u32, String)>,
) -> Result<(), String> {
    match sb {
        StructuredBlock::Simple(s) => {
            // `ssa-reloop2`'s reconverge heuristic (`find_reconverge`'s "all blocks owned"
            // fallback) can place a block into a shared `.next` tail purely due to
            // postorder-traversal position, even when it's actually only reachable from
            // one specific branch with no true reconvergence (e.g. two independent
            // `return`-terminated leaves chained as if sequential) — the same class of
            // bug `docs/bytecode-cfg-plan.md` documents for the old bytecode-facing
            // design. Since a real JS local (`__cff`) is available here (unlike
            // targeting Jade bytecode), the fix is simply to gate *every* block's own
            // content behind its own `__cff` check — sound regardless of where
            // `ssa-reloop2` places it structurally, at the cost of a redundant check in
            // the common (correctly-placed) case.
            jit.line(format!("if (__cff === {}) {{", s.label));
            {
                let block = &cfg.blocks.0[s.label];
                let ops = block.ops.clone();
                for op in ops {
                    emit_op(jit, code, op)?;
                }
            }
            let term = &cfg.blocks.0[s.label].term;
            emit_terminator(jit, code, cfg, term, &s.branches, &s.immediate, loops)?;
            jit.line("}");
            if let Some(nx) = &s.next {
                emit_block(jit, code, cfg, nx, loops)?;
            }
            Ok(())
        }
        StructuredBlock::Loop(l) => {
            let label = label_for(l.loop_id);
            loops.push((l.loop_id, label.clone()));
            jit.line(format!("{label}: while (true) {{"));
            emit_block(jit, code, cfg, &l.inner, loops)?;
            jit.line("}");
            loops.pop();
            if let Some(nx) = &l.next {
                emit_block(jit, code, cfg, nx, loops)?;
            }
            Ok(())
        }
        StructuredBlock::Multiple(_) => {
            Err("jit(reloop): Multiple block reached outside of terminator dispatch (internal invariant)".into())
        }
    }
}

/// One target during terminator dispatch: `Ok(true)` if a real, non-local JS exit
/// (`return`/labeled `break`/`continue`) was emitted for it — the caller must not also
/// treat it as a normal fallthrough/dispatch case.
///
/// Every resolved branch (including a plain `MergedBranch`) first records `__cff =
/// <target>;` — a real JS variable standing in for "which successor was actually taken".
/// This is exactly the `cff` (control-flow-flag) technique `jsaw-core`'s own `swc-cfg`
/// uses to turn `ssa-reloop2`'s `Multiple` nodes into real conditionals (see
/// `emit_dispatch`): unlike targeting Jade bytecode (which has no local variables to flag
/// with), a real JS local makes this completely sound, not just a same-shaped heuristic.
fn emit_branch_arm<
    R: FnRegistry,
    N: portal_jit_host_names::HostMethodNames<crate::JadeTenantMethod>,
>(
    jit: &mut JsJit<R, N>,
    branches: &BTreeMap<BlockIdx, BranchMode>,
    target: BlockIdx,
    loops: &[(u32, String)],
) -> Result<bool, String> {
    match branches.get(&target) {
        None => Ok(false),
        Some(BranchMode::MergedBranch) => {
            jit.line(format!("__cff = {target};"));
            Ok(false)
        }
        Some(BranchMode::LoopContinue(id)) => {
            jit.line(format!("__cff = {target};"));
            let label = find_label(loops, *id)?;
            jit.line(format!("continue {label};"));
            Ok(true)
        }
        Some(BranchMode::LoopBreak(id)) => {
            jit.line(format!("__cff = {target};"));
            let label = find_label(loops, *id)?;
            jit.line(format!("break {label};"));
            Ok(true)
        }
        Some(other) => Err(format!("jit(reloop): unsupported branch mode {other:?}")),
    }
}

fn find_label(loops: &[(u32, String)], id: u32) -> Result<String, String> {
    loops
        .iter()
        .rev()
        .find(|(lid, _)| *lid == id)
        .map(|(_, label)| label.clone())
        .ok_or_else(|| "jit(reloop): loop break/continue with no matching enclosing loop".into())
}

/// Emit `term`. A `Return` always becomes a real `return` — correct from any nesting
/// depth, unlike the old bytecode-targeting design (see the module doc comment).
fn emit_terminator<
    R: FnRegistry,
    N: portal_jit_host_names::HostMethodNames<crate::JadeTenantMethod>,
>(
    jit: &mut JsJit<R, N>,
    code: &[u8],
    cfg: &CfgFunc,
    term: &Term,
    branches: &BTreeMap<BlockIdx, BranchMode>,
    immediate: &Option<Box<StructuredBlock<BlockIdx>>>,
    loops: &mut Vec<(u32, String)>,
) -> Result<(), String> {
    match term {
        Term::Ret(val) => {
            let v = resolve(*val, jit);
            jit.line(format!("return {v};"));
        }
        Term::Jmp(target) => {
            if !emit_branch_arm(jit, branches, *target, loops)? {
                if let Some(im) = immediate {
                    emit_dispatch(jit, code, cfg, im, loops)?;
                }
            }
        }
        Term::CondJmp {
            cond,
            if_true,
            if_false,
        } => {
            let cond_v = resolve(*cond, jit);
            // Both arms are emitted as real `if`/`else`; each arm either performs a real
            // exit (return/break/continue) or falls through to whatever the shared
            // dispatch/`next` represents.
            jit.line(format!("if ({cond_v}) {{"));
            let then_exited = emit_branch_arm(jit, branches, *if_true, loops)?;
            jit.line("} else {");
            let else_exited = emit_branch_arm(jit, branches, *if_false, loops)?;
            jit.line("}");
            if !then_exited || !else_exited {
                if let Some(im) = immediate {
                    emit_dispatch(jit, code, cfg, im, loops)?;
                }
            }
        }
        Term::Switch {
            val,
            cases,
            default,
        } => {
            let val_v = resolve(*val, jit);
            jit.line(format!("switch ({val_v}) {{"));
            let mut any_fallthrough = false;
            for (cv, target) in cases {
                jit.line(format!("case {cv}: {{"));
                if !emit_branch_arm(jit, branches, *target, loops)? {
                    any_fallthrough = true;
                }
                jit.line("break; }");
            }
            jit.line("default: {");
            if !emit_branch_arm(jit, branches, *default, loops)? {
                any_fallthrough = true;
            }
            jit.line("}");
            jit.line("}");
            if any_fallthrough {
                if let Some(im) = immediate {
                    emit_dispatch(jit, code, cfg, im, loops)?;
                }
            }
        }
    }
    Ok(())
}

/// Lower the `Multiple` dispatch reached via `immediate`: gate each handled arm behind an
/// `if (__cff === <one of h.labels>)` check, so exactly the arm whose target was actually
/// taken (recorded into `__cff` by [`emit_branch_arm`]) runs — the same `cff`-flag
/// technique `swc-cfg`'s own `Cfg::process_block` uses for the same `ssa-reloop2`
/// `Multiple` node shape, just with a plain JS local instead of a JS string literal
/// switch.
fn emit_dispatch<
    R: FnRegistry,
    N: portal_jit_host_names::HostMethodNames<crate::JadeTenantMethod>,
>(
    jit: &mut JsJit<R, N>,
    code: &[u8],
    cfg: &CfgFunc,
    im: &StructuredBlock<BlockIdx>,
    loops: &mut Vec<(u32, String)>,
) -> Result<(), String> {
    let StructuredBlock::Multiple(m) = im else {
        return Err(
            "jit(reloop): terminator's `immediate` was not a Multiple block (internal invariant)"
                .into(),
        );
    };
    for h in &m.handled {
        let cond = h
            .labels
            .iter()
            .map(|l| format!("__cff === {l}"))
            .collect::<Vec<_>>()
            .join(" || ");
        jit.line(format!("if ({cond}) {{"));
        emit_block(jit, code, cfg, &h.inner, loops)?;
        jit.line("}");
    }
    Ok(())
}

/// Tier 1 entry point: compile Jade bytecode into JS by restructuring Tier 0's
/// discovered blocks via `ssa-reloop2` into native `while`/`if`/`switch`/`return`/labeled
/// `break`/`continue`, instead of Tier 0's flat block-dispatch loop.
pub fn compile<R: FnRegistry, N>(
    code: &[u8],
    reg: R,
    mut cfg: Config<N>,
) -> Result<(String, R), String>
where
    N: portal_jit_host_names::HostMethodNames<crate::JadeTenantMethod>,
{
    // Force this on regardless of what the caller passed: calling *this* function is
    // itself the choice of Tier 1, and `op_fn` (`crates/jade-vm-jit/src/lib.rs`) reads
    // `Config.prefer_reloop` — propagated by `Clone` into every nested `JsJit` — to decide
    // whether a nested closure recurses through Tier 1 too, rather than downgrading to
    // Tier 0. Without this, a caller reaching this function directly (bypassing
    // `jade_vm_jit::compile`'s own `if cfg.prefer_reloop { return reloop::compile(...) }`
    // dispatch, which is the only other place this flag is normally set) would silently
    // get Tier-0-emitted nested closures despite asking for Tier 1 at the top level.
    cfg.prefer_reloop = true;
    let cell = alloc::rc::Rc::new(RefCell::new(reg));
    let body = {
        let mut jit = JsJit::with_config(cell.clone(), cfg, false, false, false);
        emit_reloop_program(&mut jit, code, 0)?;
        jit.emit.into_inner().buf
    };
    let reg = alloc::rc::Rc::try_unwrap(cell)
        .map_err(|_| "jit(reloop): internal invariant: FnRegistry Rc had lingering clones after compile finished".to_string())?
        .into_inner();
    Ok((body, reg))
}

/// Restructure the bytecode at `code[start_ip..]` into `jit`'s own emitted output — public
/// to this crate (not the whole crate's public API) so `op_fn` (in `lib.rs`) can recurse
/// into Tier 1's own pipeline for a nested closure when `Config.prefer_reloop` is set,
/// instead of downgrading to Tier 0's `emit_program`. Nested `Config.prefer_reloop`
/// already propagates correctly via `op_fn`'s `self.cfg.clone()`, so no
/// `Config::nested_body_compiler` override is needed for this tier — that hook exists only
/// for Tier 2, which cannot be called directly from `jade-vm-jit` without an illegal
/// reverse crate dependency.
pub(crate) fn emit_reloop_program<
    R: FnRegistry,
    N: portal_jit_host_names::HostMethodNames<crate::JadeTenantMethod>,
>(
    jit: &mut JsJit<R, N>,
    code: &[u8],
    start_ip: usize,
) -> Result<(), String> {
    let blocks = discover_blocks(code, start_ip)?;
    let cfg_func = build_cfg_func(blocks, start_ip);
    let sb = ssa_reloop2::go(&cfg_func);
    // `__cff` ("control-flow flag"): records which successor a branch actually took, so
    // `emit_dispatch`'s `Multiple` handling and every `Simple` block's own self-gate (see
    // `emit_block`) can tell whether they're actually the one that was reached. Seeded to
    // the entry block's own label so its self-gate is satisfied on first entry.
    jit.line("let __cff;");
    jit.line(format!("__cff = {};", cfg_func.entry));
    let mut loops = Vec::new();
    emit_block(jit, code, &cfg_func, &sb, &mut loops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VecRegistry;
    use portal_solutions_jade_vm::Operand;

    fn chunk(ops: &[Operation]) -> Vec<u8> {
        ops.iter().flat_map(|o| o.emit()).collect()
    }

    fn op_len(op: &Operation) -> u32 {
        op.emit().count() as u32
    }

    fn run_js(body: &str) -> String {
        let script = format!(
            "const fn = new Function('tenant','nt','state', {:?}); console.log(JSON.stringify(fn(undefined,undefined,[])));",
            body
        );
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(&script)
            .output()
            .expect("node failed");
        assert!(
            output.status.success(),
            "node stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn condjmp_emits_native_if_and_executes_correctly() {
        let then_body = chunk(&[Operation::Lit32 { dest: 1, val: 10 }]);
        let else_body = chunk(&[Operation::Lit32 { dest: 1, val: 20 }]);
        let ret_body = chunk(&[Operation::Ret(Operand::StateRef(1))]);
        let condjmp_len = op_len(&Operation::CondJmp {
            cond: Operand::StateRef(0),
            if_true: 0,
            if_false: 0,
        });
        let jmp_len = op_len(&Operation::Jmp { target: 0 });
        let then_offset = condjmp_len;
        let jmp_offset = then_offset + then_body.len() as u32;
        let else_offset = jmp_offset + jmp_len;
        let ret_offset = else_offset + else_body.len() as u32;

        for cond_val in [true, false] {
            let mut code = chunk(&[Operation::Bool {
                val: cond_val,
                dest: 0,
            }]);
            code.extend(
                Operation::CondJmp {
                    cond: Operand::StateRef(0),
                    if_true: then_offset + 10,
                    if_false: else_offset + 10,
                }
                .emit(),
            );
            code.extend(then_body.clone());
            code.extend(
                Operation::Jmp {
                    target: ret_offset + 10,
                }
                .emit(),
            );
            code.extend(else_body.clone());
            code.extend(ret_body.clone());

            let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
            assert!(js.contains("if ("), "got:\n{js}");
            assert!(
                !js.contains("__ip"),
                "should not fall back to Tier 0 dispatch, got:\n{js}"
            );

            let result = run_js(&js);
            let expected = if cond_val { "10" } else { "20" };
            assert_eq!(result, expected, "cond={cond_val}, js:\n{js}");
        }
    }

    #[test]
    fn loop_emits_native_while_and_executes_correctly() {
        let ret_body = chunk(&[Operation::Ret(Operand::StateRef(0))]);
        let body_ops = chunk(&[Operation::Bool {
            val: false,
            dest: 0,
        }]);
        let header_len = op_len(&Operation::CondJmp {
            cond: Operand::StateRef(0),
            if_true: 0,
            if_false: 0,
        });
        let jmp_len = op_len(&Operation::Jmp { target: 0 });

        let mut code = chunk(&[Operation::Bool { val: true, dest: 0 }]);
        let header_offset = code.len() as u32;
        let body_offset = header_offset + header_len;
        let exit_offset = body_offset + body_ops.len() as u32 + jmp_len;
        code.extend(
            Operation::CondJmp {
                cond: Operand::StateRef(0),
                if_true: body_offset,
                if_false: exit_offset,
            }
            .emit(),
        );
        code.extend(body_ops);
        code.extend(
            Operation::Jmp {
                target: header_offset,
            }
            .emit(),
        );
        code.extend(ret_body);

        let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        assert!(js.contains("while (true) {"), "got:\n{js}");
        assert!(
            !js.contains("__ip"),
            "should not fall back to Tier 0 dispatch, got:\n{js}"
        );

        let result = run_js(&js);
        assert_eq!(result, "false", "js:\n{js}");
    }

    /// The correctness case that motivated the whole jump-based bytecode redesign: a
    /// `return` reached from inside a conditional nested in a loop body must actually
    /// execute — mirrors `jade-vm-frontend`'s
    /// `return_inside_while_loop_now_works_correctly` test.
    ///
    /// `var x = true; while (x) { if (true) { return 7; } else { x = false; } } return 99;`
    #[test]
    fn return_inside_loop_body_executes_correctly() {
        let entry_ops = chunk(&[Operation::Bool { val: true, dest: 0 }]);
        let header_len = op_len(&Operation::CondJmp {
            cond: Operand::StateRef(0),
            if_true: 0,
            if_false: 0,
        });
        let body_ops = chunk(&[Operation::Bool { val: true, dest: 1 }]);
        let body_condjmp_len = op_len(&Operation::CondJmp {
            cond: Operand::StateRef(1),
            if_true: 0,
            if_false: 0,
        });
        let ret7 = chunk(&[Operation::Ret(Operand::Literal(7))]);
        let else_ops = chunk(&[Operation::Bool {
            val: false,
            dest: 0,
        }]);
        let jmp_len = op_len(&Operation::Jmp { target: 0 });
        let exit_ops = chunk(&[Operation::Ret(Operand::Literal(99))]);

        let header_offset = entry_ops.len() as u32;
        let body_offset = header_offset + header_len;
        let retblk_offset = body_offset + body_ops.len() as u32 + body_condjmp_len;
        let elseblk_offset = retblk_offset + ret7.len() as u32;
        let exit_offset = elseblk_offset + else_ops.len() as u32 + jmp_len;

        let mut code = entry_ops;
        code.extend(
            Operation::CondJmp {
                cond: Operand::StateRef(0),
                if_true: body_offset,
                if_false: exit_offset,
            }
            .emit(),
        );
        code.extend(body_ops);
        code.extend(
            Operation::CondJmp {
                cond: Operand::StateRef(1),
                if_true: retblk_offset,
                if_false: elseblk_offset,
            }
            .emit(),
        );
        code.extend(ret7);
        code.extend(else_ops);
        code.extend(
            Operation::Jmp {
                target: header_offset,
            }
            .emit(),
        );
        code.extend(exit_ops);

        let (js, _reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        let result = run_js(&js);
        assert_eq!(result, "7", "js:\n{js}");
    }

    /// Run compiled JS that references `tenant`/`nt`/a registered nested function — unlike
    /// `run_js`, wraps `body` with a `markGuestFn` stub and `prelude` (nested function
    /// declarations from `VecRegistry::prelude()`) in scope.
    fn run_js_with_prelude(prelude: &str, body: &str) -> String {
        let script = format!(
            "function markGuestFn(f, m) {{ return f; }}\n{prelude}\nconst fn = new Function('tenant', 'nt', 'state', {body:?});\nconsole.log(JSON.stringify(fn(undefined, undefined, [])));",
        );
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(&script)
            .output()
            .expect("node failed");
        assert!(
            output.status.success(),
            "node stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// A nested `Fn` op whose own body has real control flow (an `if`/`else`), compiled
    /// via Tier 1 (this module's own `compile` entry point) — proves the nested body is
    /// itself restructured via `ssa-reloop2` (a real `if`, no `switch (__ip)` block-
    /// dispatch loop), not silently downgraded to Tier 0, now that `op_fn`
    /// (`crates/jade-vm-jit/src/lib.rs`) recurses through whichever tier is driving the
    /// *current* compilation — for Tier 1 specifically, `Config.prefer_reloop` just
    /// propagates by `Clone` into every nested `JsJit`, so no `nested_body_compiler`
    /// override is needed (that hook exists only for Tier 2, a separate crate). Mirrors
    /// `jade-vm-jit-swc`'s `nested_fn_with_branch_is_reconstructed_by_tier2`.
    #[test]
    fn nested_fn_with_branch_is_reconstructed_by_tier1() {
        let mk = |j: u32| Operation::Fn {
            variant: Operand::Literal(0),
            closure_args: Operand::Literal(0),
            spanner: Operand::Literal(0),
            j,
            dest: 0,
        };
        let call_op = Operation::Call {
            fn_op: Operand::StateRef(0),
            args: vec![],
            dest: 1,
        };
        let ret_op = Operation::Ret(Operand::StateRef(1));
        // The nested body must start *after* the entire top-level block (Fn + Call +
        // Ret), not right after the `Fn` op alone — see the identical note in
        // `jade-vm-jit-swc`'s equivalent test.
        let j = op_len(&mk(0)) + op_len(&call_op) + op_len(&ret_op);

        let bool_len = op_len(&Operation::Bool { val: true, dest: 0 });
        let cond_len = op_len(&Operation::CondJmp {
            cond: Operand::StateRef(0),
            if_true: 0,
            if_false: 0,
        });
        let jmp_len = op_len(&Operation::Jmp { target: 0 });
        let then_body = chunk(&[Operation::Lit32 { dest: 1, val: 10 }]);
        let else_body = chunk(&[Operation::Lit32 { dest: 1, val: 20 }]);
        let ret_body = chunk(&[Operation::Ret(Operand::StateRef(1))]);

        let then_offset = j + bool_len + cond_len;
        let jmp_offset = then_offset + then_body.len() as u32;
        let else_offset = jmp_offset + jmp_len;
        let ret_offset = else_offset + else_body.len() as u32;

        let mut fn_body = chunk(&[Operation::Bool { val: true, dest: 0 }]);
        fn_body.extend(
            Operation::CondJmp {
                cond: Operand::StateRef(0),
                if_true: then_offset,
                if_false: else_offset,
            }
            .emit(),
        );
        fn_body.extend(then_body);
        fn_body.extend(Operation::Jmp { target: ret_offset }.emit());
        fn_body.extend(else_body);
        fn_body.extend(ret_body);

        let mut code: Vec<u8> = mk(j).emit().collect();
        code.extend(call_op.emit());
        code.extend(ret_op.emit());
        assert_eq!(
            code.len() as u32,
            j,
            "internal test invariant: top-level block length must match the precomputed `j`"
        );
        code.extend_from_slice(&fn_body);

        let (js, reg) = compile(&code, VecRegistry::new(), Config::default()).unwrap();
        let prelude = reg.prelude();
        assert!(
            prelude.contains("if ("),
            "expected the nested body's branch reconstructed as a real `if`, got:\n{prelude}"
        );
        assert!(
            !prelude.contains("__ip"),
            "did not expect a Tier 0 block-dispatch loop, got:\n{prelude}"
        );

        assert_eq!(
            run_js_with_prelude(&prelude, &js),
            "10",
            "prelude:\n{prelude}\njs:\n{js}"
        );
    }
}

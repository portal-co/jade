//! JS-source-to-Jade-bytecode frontend.
//!
//! Pipeline: source text -> SWC AST (`swc_ecma_parser`) -> TAC (`portal_jsc_swc_tac`,
//! which internally lowers through `portal_jsc_swc_cfg`) -> Jade `Operation`s
//! (`portal_solutions_jade_vm`).
//!
//! Jade bytecode is now itself a flat, jump-based CFG (`JMP`/`CONDJMP`/`SWITCH` name
//! byte-offset targets directly; `RET` is valid in any block) — see
//! `docs/bytecode-cfg-plan.md`. TAC's own blocks/terminators
//! (`TBlock`/`TTerm::{Jmp,CondJmp,Switch,Return}`) map almost 1:1 onto Jade's, so this
//! frontend just walks `TCfg` directly: no restructuring step, no flag-passing
//! workarounds for `return` — every TAC block becomes one Jade block at its own byte
//! offset, and every TAC terminator becomes the matching Jade terminator.
//!
//! **Scope**: only lowers constructs that have a direct Jade bytecode opcode — see
//! `packages/jade-data/index.ts` for the authoritative opcode list. Notably, Jade bytecode
//! currently has no arithmetic opcodes (only the six comparisons), no way to bind function
//! parameters to state slots, no exception handling, and no opcode for `undefined` as a
//! produced value. Everything without a direct mapping is an explicit
//! [`FrontendError::Unsupported`], never a silent miscompile.

use std::collections::HashMap;

use portal_jsc_swc_tac::{Item, LId, TBlock, TBlockId, TCallee, TCatch, TCfg, TFunc, TStmt, TTerm};
use portal_solutions_jade_vm::{Operand, Operation, SignedOperand};
use swc_common::sync::Lrc;
use swc_common::{FileName, SourceMap};
use swc_ecma_ast::{BinaryOp, BlockStmt, Function, Lit, MetaPropKind};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, StringInput, Syntax};

type Ident = swc_ecma_ast::Id;

mod globals;
pub mod tenant_inline;

/// A JS construct with no direct Jade bytecode equivalent (yet). Carries a short
/// human-readable description of what wasn't supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported construct: {}", self.0)
    }
}
impl std::error::Error for Unsupported {}

#[derive(Debug)]
pub enum FrontendError {
    Parse(String),
    Tac(String),
    Opt(String),
    Unsupported(Unsupported),
}

impl std::fmt::Display for FrontendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrontendError::Parse(s) => write!(f, "parse error: {s}"),
            FrontendError::Tac(s) => write!(f, "AST->TAC lowering error: {s}"),
            FrontendError::Opt(s) => write!(f, "jade-cfg-opt error: {s}"),
            FrontendError::Unsupported(u) => write!(f, "{u}"),
        }
    }
}
impl std::error::Error for FrontendError {}

fn unsupported<T>(msg: impl Into<String>) -> Result<T, FrontendError> {
    Err(FrontendError::Unsupported(Unsupported(msg.into())))
}

/// Parse `src` as a top-level script and compile it into Jade bytecode.
///
/// Returns the compiled byte buffer; execution should start at offset `0`. Any nested
/// functions (`FN` opcode bodies) are appended after the top-level code, referenced by
/// byte offset exactly like hand-encoded bytecode.
///
/// Equivalent to [`compile_to_bytecode_with_variant`] with `is_generator`/`is_async` both
/// `false` — the top-level program is declared sync; a caller wanting to exercise real
/// `yield`/`yield*`/`await` syntax at the top level (e.g. to combine with a JIT's ambient
/// `Config.add_gen`/`add_async` upgrade for a genuine doubleGen program) needs
/// [`compile_to_bytecode_with_variant`] instead.
pub fn compile_to_bytecode(src: &str) -> Result<Vec<u8>, FrontendError> {
    compile_to_bytecode_with_variant(src, false, false)
}

/// Like [`compile_to_bytecode`], but lets the top-level program itself be declared a
/// generator and/or async function — which is what actually makes `yield`/`yield*`/
/// `await` syntax legal at the top level (SWC's parser only recognizes `yield`/`await`
/// expressions inside a function/script declared as such). Without this, those keywords
/// can only be exercised via a JIT tier's *ambient* `Config.add_gen`/`add_async` upgrade
/// applied to bytecode that was never really declared a generator/async to begin with.
pub fn compile_to_bytecode_with_variant(
    src: &str,
    is_generator: bool,
    is_async: bool,
) -> Result<Vec<u8>, FrontendError> {
    swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let mut stmts = parse_script(src)?;
        // Resolve free-identifier reads into realm-global member reads *before* TAC
        // conversion — afterwards, names are gone (see globals.rs).
        let global_ctxt = globals::resolve_globals(&mut stmts).map(|g| g.ctxt);
        let synthetic = Function {
            params: vec![],
            decorators: vec![],
            span: swc_common::DUMMY_SP,
            ctxt: Default::default(),
            body: Some(BlockStmt {
                span: swc_common::DUMMY_SP,
                ctxt: Default::default(),
                stmts,
            }),
            is_generator,
            is_async,
            type_params: None,
            return_type: None,
        };
        let tfunc =
            TFunc::try_from(&synthetic).map_err(|e| FrontendError::Tac(format!("{e:?}")))?;
        // Reject closure captures while names and decls are still accurate — after
        // `optimize_tfunc`'s SSA round-trip, its `$k…p…` block-param materializations
        // look identical to real captures.
        check_no_captures(&tfunc, global_ctxt)?;
        // Shared TAC canonicalizer + SSA constant-fold/DCE pass (see
        // `docs/bytecode-cfg-plan.md`); the same `jade-cfg-opt::optimize_tfunc` an
        // optional JIT plugin (Tier 2) can also apply to its own reconstructed CFG.
        let tfunc = portal_solutions_jade_cfg_opt::optimize_tfunc(&tfunc)
            .map_err(|e| FrontendError::Opt(format!("{e:?}")))?;
        compile_program(tfunc, global_ctxt)
    })
}

/// Compile `entry` (and everything nested inside it, transitively — see
/// `docs/closure-capture-plan.md` for the closure-capture-analysis follow-on this doesn't
/// yet do) into one flat Jade bytecode buffer.
///
/// Every jump target in Jade bytecode — including a nested function's own `FN` opcode's
/// `j` operand — is an absolute byte offset into *one shared buffer*; no backend rebases
/// them (e.g. `jade-vm-jit`'s `op_fn` slices the very same `code` it was given, at `j`).
/// That means a nested function's `j` can only be assigned once the byte length of
/// everything *before* it in the final buffer is known — which, for a function whose own
/// body might itself contain further nested functions, isn't known until that function's
/// own length is measured too. This applies the same trick `FnLowering::compile_blocks`
/// already uses for its own block offsets (measure with placeholder targets first, since
/// every opcode's operands are fixed-width regardless of value; emit for real once real
/// offsets are known) one level up, across the whole nested-function tree instead of just
/// one function's own blocks:
///
/// 1. **Discover**: `entry` plus everything nested inside it, transitively, in a stable
///    traversal order — measuring each function's own byte length with placeholder
///    (`j = 0`) nested-function operands.
/// 2. **Lay out**: assign each discovered function a region (a byte range) in the final
///    buffer — `entry` first (so it lands at offset `0`, matching this module's existing
///    documented invariant), then every other function in discovery order.
/// 3. **Emit for real**: re-lower every function, now that every function's region start
///    is known, so both its own block offsets (region start + local block offset) and any
///    nested `Fn` op's `j` (the referenced function's region start) resolve correctly.
fn compile_program(
    entry: TFunc,
    global_ctxt: Option<swc_common::SyntaxContext>,
) -> Result<Vec<u8>, FrontendError> {
    let mut funcs: Vec<TFunc> = vec![entry];
    let mut region_lens: Vec<u32> = Vec::new();
    let mut children_base: Vec<u32> = Vec::new();

    // Phase 1: discover + measure. Appending to `funcs` while iterating it by index is
    // exactly how transitively-nested functions get discovered; `children_base[k]` is the
    // global index the first of function `k`'s own (immediate) nested functions lands at.
    let mut i = 0;
    while i < funcs.len() {
        let mut lowering = FnLowering::new(&funcs[i].cfg, &funcs[i].params, global_ctxt);
        let mut discovered = Vec::new();
        let mut phase = FnPhase::Measure {
            discovered: &mut discovered,
        };
        let len = lowering
            .compile_blocks(funcs[i].entry, 0, &mut phase)?
            .len() as u32;
        region_lens.push(len);
        children_base.push(funcs.len() as u32);
        funcs.extend(discovered);
        i += 1;
    }

    // Phase 2: lay out regions as a simple prefix sum over discovery order.
    let mut region_offsets: Vec<u32> = Vec::with_capacity(funcs.len());
    let mut cursor = 0u32;
    for &len in &region_lens {
        region_offsets.push(cursor);
        cursor += len;
    }

    // Phase 3: emit for real.
    let mut out = vec![0u8; cursor as usize];
    for (k, tfunc) in funcs.iter().enumerate() {
        let mut lowering = FnLowering::new(&tfunc.cfg, &tfunc.params, global_ctxt);
        let mut phase = FnPhase::Emit {
            children_base: children_base[k],
            region_offsets: &region_offsets,
        };
        let bytes = lowering.compile_blocks(tfunc.entry, region_offsets[k], &mut phase)?;
        debug_assert_eq!(
            bytes.len() as u32,
            region_lens[k],
            "measured (phase 1) and real (phase 3) lengths must match: every opcode's \
             operands are fixed-width regardless of value"
        );
        let start = region_offsets[k] as usize;
        out[start..start + bytes.len()].copy_from_slice(&bytes);
    }
    Ok(out)
}

fn parse_script(src: &str) -> Result<Vec<swc_ecma_ast::Stmt>, FrontendError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input.js".into())),
        src.to_string(),
    );
    let lexer = Lexer::new(
        Syntax::Es(Default::default()),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let script = parser
        .parse_script()
        .map_err(|e| FrontendError::Parse(format!("{e:?}")))?;
    Ok(script.body)
}

/// Reject (transitively) nested functions that reference enclosing-scope names.
///
/// A nested function may only see its own declarations and parameters (plus the marked
/// global object ident — global reads are member ops after `globals::resolve_globals`,
/// not captures). Anything else is an upvalue the `FN` opcode's not-yet-wired
/// `closure_args` would have to supply, and compiling it would silently produce
/// `undefined` where JS has a shared binding — an explicit `Unsupported` instead.
fn check_no_captures(
    tfunc: &TFunc,
    global_ctxt: Option<swc_common::SyntaxContext>,
) -> Result<(), FrontendError> {
    /// The function's *own* references — everything `TCfg::refs()` yields except the
    /// transitive `Item::Func` externs it also folds in. Those externs can't be used
    /// here: `TCfg::externs()` filters only by the nested cfg's `decls` (its `params`
    /// live on `TFunc`, which `TCfg` can't see), so a nested function's *parameter*
    /// would show up in this function's `refs()` as a false "capture". The walk
    /// recurses into nested functions itself, checking each against its own
    /// decls ∪ params, so descending here would also double-report.
    fn own_refs(func: &TFunc) -> Vec<Ident> {
        let mut out: Vec<Ident> = Vec::new();
        for block in func.cfg.blocks.iter() {
            for stmt in &block.1.stmts {
                out.extend(stmt.left.as_ref().refs().cloned());
                // `Item::refs()`'s `Func` arm is empty — nested bodies never leak here.
                out.extend(stmt.right.refs().cloned());
            }
            match &block.1.post.term {
                TTerm::Return(r) => out.extend(r.iter().cloned()),
                TTerm::Throw(t) => out.push(t.clone()),
                TTerm::Jmp(_) | TTerm::Default => {}
                TTerm::CondJmp { cond, .. } => out.push(cond.clone()),
                TTerm::Switch { x, blocks, .. } => {
                    out.push(x.clone());
                    out.extend(blocks.iter().map(|case| case.0.clone()));
                }
                TTerm::Tail { callee, args } => {
                    match callee {
                        TCallee::Val(f) | TCallee::PrivateMember { func: f, .. } => out.push(f.clone()),
                        TCallee::Member { func, member } => {
                            out.push(func.clone());
                            out.push(member.clone());
                        }
                        TCallee::Import | TCallee::Super | TCallee::SuperMember { .. } | TCallee::Eval => {}
                        _ => {}
                    }
                    out.extend(args.iter().map(|a| a.value.clone()));
                }
                _ => {}
            }
        }
        out
    }
    fn walk(func: &TFunc, global_ctxt: Option<swc_common::SyntaxContext>) -> Result<(), FrontendError> {
        let mut names = Vec::new();
        for id in own_refs(func) {
            if Some(id.1) == global_ctxt || func.cfg.decls.contains(&id) || func.params.contains(&id) {
                continue;
            }
            if !names.contains(&id) {
                names.push(id);
            }
        }
        if !names.is_empty() {
            let shown: Vec<String> = names
                .iter()
                .take(3)
                .map(|id| format!("`{}`", id.0))
                .collect();
            return unsupported(format!(
                "closure capture of {}{} (see docs/closure-capture-plan.md)",
                shown.join(", "),
                if names.len() > 3 { ", …" } else { "" },
            ));
        }
        for block in func.cfg.blocks.iter() {
            for stmt in &block.1.stmts {
                if let Item::Func { func: nested, .. } = &stmt.right {
                    walk(nested, global_ctxt)?;
                }
            }
        }
        Ok(())
    }
    walk(tfunc, global_ctxt)
}

/// Which phase of [`compile_program`]'s two-phase (measure-then-emit) nested-function
/// offset resolution scheme is driving a `lower_item` call. See `compile_program`'s doc
/// comment for the full scheme; this only concerns the `Item::Func` arm of `lower_item`.
enum FnPhase<'a> {
    /// Phase 1: discover nested functions and measure this function's own byte length.
    /// Nested-function `j` operands are written as `0` (safe: fixed-width regardless of
    /// value) and the nested `TFunc`s themselves are collected in traversal order.
    Measure { discovered: &'a mut Vec<TFunc> },
    /// Phase 3: real emission, now that every function's region offset is known. Maps the
    /// Nth nested `Item::Func` encountered (0-indexed, same traversal order phase 1 used
    /// — see `FnLowering::nested_fn_counter`) to its global function index via
    /// `children_base + N`, then to its real byte offset via `region_offsets`.
    Emit {
        children_base: u32,
        region_offsets: &'a [u32],
    },
}

struct FnLowering<'a> {
    tcfg: &'a TCfg,
    /// The mark `globals::resolve_globals` put on "the realm global object" idents, if
    /// the program references the global at all. Survives the SSA name mangle.
    global_ctxt: Option<swc_common::SyntaxContext>,
    /// The slot holding this region's `GLOBAL` result. Allocated up-front (slot 0)
    /// whenever `global_ctxt` is set, so the measure and emit phases agree.
    global_slot: Option<u32>,
    slots: HashMap<Ident, u32>,
    next_slot: u32,
    /// Lazily-allocated slot holding a value that is never written — reading it
    /// therefore yields `undefined`, used for e.g. `return;` (Jade has no dedicated
    /// "produce undefined" opcode).
    undefined_slot: Option<u32>,
    /// How many `Item::Func` nodes this function's own body has encountered so far in
    /// this traversal — see [`FnPhase::Emit`].
    nested_fn_counter: u32,
}

impl<'a> FnLowering<'a> {
    fn new(
        tcfg: &'a TCfg,
        params: &'a [Ident],
        global_ctxt: Option<swc_common::SyntaxContext>,
    ) -> Self {
        let mut lowering = Self {
            tcfg,
            global_ctxt,
            global_slot: None,
            slots: HashMap::new(),
            next_slot: 0,
            undefined_slot: None,
            nested_fn_counter: 0,
        };
        if global_ctxt.is_some() {
            // Reserve this region's global slot deterministically (always the first
            // slot), and make `compile_blocks` emit `GLOBAL` into it at the entry.
            lowering.global_slot = Some(lowering.fresh_slot());
        }
        // Reserve one slot per declared parameter, deterministically and before any
        // local. The enclosing function's `Item::Func` arm advertises exactly this
        // list (via `param_slot_ids`, the same deterministic assignment) in the `FN`
        // op's `params` operand, and every backend's closure binds `args[i]` into
        // `state[param_slots[i]]` at call time.
        for param in params {
            lowering.slot_for(param);
        }
        lowering
    }

    /// The slot ids `FnLowering::new` assigns to `params`, in argument order —
    /// deterministic per (`params`, `global_ctxt`) so the measure and emit phases (and
    /// the *enclosing* function's `FN` emission, which never builds the nested
    /// function's own `FnLowering`) all agree on it byte-for-byte.
    fn param_slot_ids(
        params: &[Ident],
        global_ctxt: Option<swc_common::SyntaxContext>,
    ) -> Vec<u32> {
        let mut next = if global_ctxt.is_some() { 1 } else { 0 };
        let mut seen: HashMap<Ident, u32> = HashMap::new();
        params
            .iter()
            .map(|p| {
                if let Some(&s) = seen.get(p) {
                    s
                } else {
                    let s = next;
                    next += 1;
                    seen.insert(p.clone(), s);
                    s
                }
            })
            .collect()
    }
}

/// One TAC block, lowered to Jade bytecode: the straight-line ops as bytes, plus its
/// terminator lowered *twice* — once with placeholder (`0`) jump targets purely to learn
/// its encoded byte length (`Jmp`/`CondJmp`/`Switch`'s target fields are fixed-width
/// regardless of value), and once for real once every block's offset is known. See
/// `FnLowering::compile_blocks`.
struct LoweredBlock {
    ops_bytes: Vec<u8>,
    term: TTerm,
}

impl<'a> FnLowering<'a> {
    /// Discover every TAC block reachable from `entry`, lower each one's straight-line
    /// ops, assign each a stable *absolute* byte offset (`region_offset` plus a simple
    /// local prefix sum over emission order — `entry` always first, so it always lands at
    /// exactly `region_offset`), then lower every block's terminator with the now-known
    /// real target offsets and concatenate. `region_offset` is this function's own region
    /// start in the final whole-program buffer (see [`compile_program`]) — `0` is fine
    /// during phase-1 measuring, when only the returned `Vec`'s *length* is consulted, not
    /// any jump-target value actually written into it.
    fn compile_blocks(
        &mut self,
        entry: TBlockId,
        region_offset: u32,
        phase: &mut FnPhase,
    ) -> Result<Vec<u8>, FrontendError> {
        let order = self.discover_reachable(entry)?;
        let mut lowered: Vec<(TBlockId, LoweredBlock)> = Vec::with_capacity(order.len());
        for (position, id) in order.iter().enumerate() {
            let block: &TBlock = &self.tcfg.blocks[*id];
            let mut ops_bytes = Vec::new();
            if position == 0 {
                if let Some(slot) = self.global_slot {
                    // The entry block initializes the realm-global slot ahead of any
                    // member reads against it (see globals.rs).
                    ops_bytes.extend(Operation::Global(slot).emit());
                }
            }
            for stmt in &block.stmts {
                self.lower_stmt(stmt, phase, &mut ops_bytes)?;
            }
            if !matches!(block.post.catch, TCatch::Throw) {
                return unsupported("try/catch (Jade bytecode has no exception-handling opcode)");
            }
            lowered.push((
                *id,
                LoweredBlock {
                    ops_bytes,
                    term: block.post.term.clone(),
                },
            ));
        }

        // Placeholder-target terminator lengths, to compute each block's byte offset.
        // `offsets` holds *absolute* (whole-buffer) byte offsets — `region_offset` (this
        // function's own region start, `0` during phase 1 measuring, when only lengths
        // matter and no real jump-target value is observed) plus the local prefix-sum
        // `cursor` — since every jump target in Jade bytecode, including a nested `Fn`'s
        // `j`, is absolute into the one shared buffer, never rebased by any backend.
        let mut offsets: HashMap<TBlockId, u32> = HashMap::with_capacity(lowered.len());
        let mut cursor = 0u32;
        for (id, block) in &lowered {
            offsets.insert(*id, region_offset + cursor);
            let term_len = self.emit_terminator(&block.term, &offsets, true)?.len() as u32;
            cursor += block.ops_bytes.len() as u32 + term_len;
        }

        let mut out = Vec::with_capacity(cursor as usize);
        for (_, block) in &lowered {
            out.extend_from_slice(&block.ops_bytes);
            out.extend(self.emit_terminator(&block.term, &offsets, false)?);
        }
        Ok(out)
    }

    /// BFS over `TTerm`'s jump targets, starting at `entry`. `entry` is always first in
    /// the returned order (so it always ends up at byte offset `0`); the rest follow in
    /// discovery order. Unreachable TAC blocks (dead code from AST/TAC lowering) are
    /// never visited, matching how a well-formed compiler naturally drops them.
    fn discover_reachable(&self, entry: TBlockId) -> Result<Vec<TBlockId>, FrontendError> {
        let mut order = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut worklist = std::collections::VecDeque::new();
        worklist.push_back(entry);
        visited.insert(entry);
        while let Some(id) = worklist.pop_front() {
            order.push(id);
            let block = &self.tcfg.blocks[id];
            for target in Self::targets_of(&block.post.term)? {
                if visited.insert(target) {
                    worklist.push_back(target);
                }
            }
        }
        Ok(order)
    }

    /// The set of blocks a terminator can jump to (empty for `Return`/error terminators).
    fn targets_of(term: &TTerm) -> Result<Vec<TBlockId>, FrontendError> {
        Ok(match term {
            TTerm::Return(_) => vec![],
            TTerm::Jmp(id) => vec![*id],
            TTerm::CondJmp {
                if_true, if_false, ..
            } => vec![*if_true, *if_false],
            TTerm::Switch { .. } => {
                return unsupported("`switch` statement (JS `switch`, not yet lowered)");
            }
            TTerm::Throw(_) => {
                return unsupported("`throw` (Jade bytecode has no exception-handling opcode)");
            }
            TTerm::Tail { .. } => return unsupported("tail call"),
            // Reachable `Default` = falls off the end of the program/function. It's a
            // valid final terminator; only *unreachable* Default blocks (placeholder
            // artifacts of AST/TAC lowering) are garbage, and `discover_reachable`'s BFS
            // never visits those.
            TTerm::Default => vec![],
        })
    }

    /// Lower `term` to Jade bytecode. When `placeholder` is set, every jump target is
    /// encoded as `0` — used only to measure the terminator's byte length (fixed-width
    /// regardless of target value) before real offsets are known; `offsets` is ignored
    /// in that mode (may be incomplete).
    fn emit_terminator(
        &mut self,
        term: &TTerm,
        offsets: &HashMap<TBlockId, u32>,
        placeholder: bool,
    ) -> Result<Vec<u8>, FrontendError> {
        let target = |id: &TBlockId| -> u32 { if placeholder { 0 } else { offsets[id] } };
        Ok(match term {
            TTerm::Return(val) => {
                let op = match val {
                    Some(id) => self.operand_for(id),
                    None => self.undefined_operand(),
                };
                Operation::Ret(op).emit().collect()
            }
            TTerm::Jmp(id) => Operation::Jmp { target: target(id) }.emit().collect(),
            TTerm::CondJmp {
                cond,
                if_true,
                if_false,
            } => {
                let cond_op = self.operand_for(cond);
                Operation::CondJmp {
                    cond: cond_op,
                    if_true: target(if_true),
                    if_false: target(if_false),
                }
                .emit()
                .collect()
            }
            // Reachable `Default` = falls off the end: complete with `undefined`,
            // exactly like `return;` with no value (JS completion semantics).
            TTerm::Default => Operation::Ret(self.undefined_operand()).emit().collect(),
            TTerm::Switch { .. } | TTerm::Throw(_) | TTerm::Tail { .. } => {
                // `discover_reachable` already rejects these before we ever get here.
                return unsupported(
                    "internal invariant: unreachable TAC terminator reached emit_terminator",
                );
            }
        })
    }

    fn slot_for(&mut self, id: &Ident) -> u32 {
        // An ident carrying the global-resolution mark is the realm global object
        // itself — read the reserved slot, never an ordinary (unwritten) one. Compare
        // by full SyntaxContext, not text: the mark is the binding's identity.
        if Some(id.1) == self.global_ctxt {
            return self.global_slot.expect("allocated in FnLowering::new");
        }
        if let Some(&s) = self.slots.get(id) {
            return s;
        }
        let s = self.next_slot;
        self.next_slot += 1;
        self.slots.insert(id.clone(), s);
        s
    }

    fn fresh_slot(&mut self) -> u32 {
        let s = self.next_slot;
        self.next_slot += 1;
        s
    }

    fn operand_for(&mut self, id: &Ident) -> Operand {
        Operand::StateRef(self.slot_for(id))
    }

    fn undefined_operand(&mut self) -> Operand {
        let slot = *self.undefined_slot.get_or_insert_with(|| {
            // NB: allocated but deliberately never written.
            let s = self.next_slot;
            s
        });
        if self.undefined_slot == Some(slot) && self.next_slot == slot {
            self.next_slot += 1;
        }
        Operand::StateRef(slot)
    }

    /// Lower a single TAC statement, materializing its value into a slot determined by
    /// its `LId` (a plain identifier gets its own slot; a member write also emits the
    /// Jade `SET` and discards the write's own result slot). `phase` is only consulted by
    /// `lower_item`'s `Item::Func` arm; see [`FnPhase`].
    fn lower_stmt(
        &mut self,
        stmt: &TStmt,
        phase: &mut FnPhase,
        out: &mut Vec<u8>,
    ) -> Result<(), FrontendError> {
        match &stmt.left {
            LId::Id { id } => {
                let dest = self.slot_for(id);
                self.lower_item(&stmt.right, dest, phase, out)
            }
            LId::Member { obj, mem } => {
                let val_slot = self.fresh_slot();
                self.lower_item(&stmt.right, val_slot, phase, out)?;
                let obj_op = self.operand_for(obj);
                let key_op = self.operand_for(&mem[0]);
                let dest = self.fresh_slot();
                out.extend(
                    Operation::Set {
                        obj: obj_op,
                        key: key_op,
                        val: Operand::StateRef(val_slot),
                        dest,
                    }
                    .emit(),
                );
                Ok(())
            }
            LId::Private { .. } => unsupported("private field assignment (`obj.#x = ...`)"),
            _ => unsupported("unknown LId variant"),
        }
    }

    fn lower_item(
        &mut self,
        item: &Item<Ident, TFunc>,
        dest: u32,
        phase: &mut FnPhase,
        out: &mut Vec<u8>,
    ) -> Result<(), FrontendError> {
        match item {
            Item::Undef => {
                // No-op: Jade has no "produce undefined" opcode (see `undefined_operand`),
                // but an unwritten state slot already reads as `undefined`, so simply
                // never writing to `dest` here is correct. `jade-cfg-opt`'s SSA round-trip
                // (see `optimize_tfunc`) introduces explicit `Item::Undef` writes (its own
                // shim-block sentinel) that wouldn't otherwise appear from direct AST->TAC
                // lowering.
                Ok(())
            }
            Item::Just { id } => {
                // Alias: copy the source slot's value into `dest` via a no-op SEL
                // (`cond ? a : a` always evaluates to `a`), since there's no plain
                // "move" opcode.
                let src = self.operand_for(id);
                out.extend(
                    Operation::Sel {
                        cond: Operand::Literal(1),
                        then: src,
                        else_: src,
                        dest,
                    }
                    .emit(),
                );
                Ok(())
            }
            Item::Lit { lit } => self.lower_lit(lit, dest, out),
            Item::Bin { left, right, op } => self.lower_bin(*op, left, right, dest, out),
            Item::Mem { obj, mem } => {
                let obj_op = self.operand_for(obj);
                let key_op = self.operand_for(mem);
                out.extend(
                    Operation::Get {
                        obj: obj_op,
                        key: key_op,
                        dest,
                    }
                    .emit(),
                );
                Ok(())
            }
            Item::Select {
                cond,
                then,
                otherwise,
            } => {
                let cond_op = self.operand_for(cond);
                let then_op = self.operand_for(then);
                let else_op = self.operand_for(otherwise);
                out.extend(
                    Operation::Sel {
                        cond: cond_op,
                        then: then_op,
                        else_: else_op,
                        dest,
                    }
                    .emit(),
                );
                Ok(())
            }
            Item::Call { callee, args } => {
                let mut arg_ops = Vec::with_capacity(args.len());
                for a in args {
                    if a.is_spread {
                        return unsupported("spread arguments in a call");
                    }
                    arg_ops.push(self.operand_for(&a.value));
                }
                let (fn_op, this_op) = match callee {
                    TCallee::Val(f) => (self.operand_for(f), self.undefined_operand()),
                    // `obj.member(...)`: resolve the callee through the tenant (GET)
                    // and keep the receiver as the call's `this` — the CALL opcode's
                    // dedicated operand preserves member-call `this` semantics.
                    TCallee::Member { func, member } => {
                        let this_op = self.operand_for(func);
                        let key_op = self.operand_for(member);
                        let fn_slot = self.fresh_slot();
                        out.extend(
                            Operation::Get {
                                obj: this_op,
                                key: key_op,
                                dest: fn_slot,
                            }
                            .emit(),
                        );
                        (Operand::StateRef(fn_slot), this_op)
                    }
                    _ => {
                        return unsupported(
                            "call target other than a plain value or member call (`super`, `import()`, `eval`)",
                        );
                    }
                };
                out.extend(
                    Operation::Call {
                        fn_op,
                        this_op,
                        args: arg_ops,
                        dest,
                    }
                    .emit(),
                );
                Ok(())
            }
            Item::Arr { members } => {
                let mut ops = Vec::with_capacity(members.len());
                for m in members {
                    if m.is_spread {
                        return unsupported("spread element in an array literal");
                    }
                    ops.push(self.operand_for(&m.value));
                }
                out.extend(Operation::Arr(ops, dest).emit());
                Ok(())
            }
            Item::Obj { members } => {
                let mut pairs = Vec::with_capacity(members.len());
                for (key, val) in members {
                    let portal_jsc_swc_tac::PropVal::Item(val_id) = val else {
                        return unsupported("getter/setter/method in an object literal");
                    };
                    let key_op = match key {
                        portal_jsc_swc_tac::PropKey::Lit(sym) => {
                            self.lower_string_into_fresh_slot(sym.sym.as_str(), out)
                        }
                        portal_jsc_swc_tac::PropKey::Computed(id) => self.operand_for(id),
                        _ => return unsupported("unknown PropKey variant"),
                    };
                    let val_op = self.operand_for(val_id);
                    pairs.push((key_op, val_op));
                }
                out.extend(
                    Operation::Litobj {
                        c: SignedOperand::Positive(pairs.len() as u32),
                        pairs,
                        key: Operand::Literal(dest),
                    }
                    .emit(),
                );
                Ok(())
            }
            Item::Await { value } => {
                let val = self.operand_for(value);
                out.extend(Operation::Await { val, dest }.emit());
                Ok(())
            }
            Item::Yield { value, delegate } => {
                let val = match value {
                    Some(v) => self.operand_for(v),
                    None => self.undefined_operand(),
                };
                if *delegate {
                    out.extend(Operation::Yieldstar { val, dest }.emit());
                } else {
                    out.extend(Operation::Yield { val, dest }.emit());
                }
                Ok(())
            }
            Item::Meta {
                prop: MetaPropKind::NewTarget,
            } => {
                out.extend(Operation::NewTarget(dest).emit());
                Ok(())
            }
            Item::Func { func, .. } => {
                // `closure_args`/`spanner` below are always literal `0` — free-variable
                // capture isn't wired up yet; see `docs/closure-capture-plan.md`.
                //
                // The Nth nested `Item::Func` this function's body encounters (0-indexed,
                // traversal order) — must match exactly between `compile_program`'s phase 1
                // (measure/discover) and phase 3 (real emission) passes for `j` to resolve
                // to the right function. See `FnPhase`/`compile_program`.
                let n = self.nested_fn_counter;
                self.nested_fn_counter += 1;
                let j = match phase {
                    FnPhase::Measure { discovered } => {
                        discovered.push(func.clone());
                        0
                    }
                    FnPhase::Emit {
                        children_base,
                        region_offsets,
                    } => region_offsets[(*children_base + n) as usize],
                };
                // 0=sync, 1=async, 2=sync generator, 3=async generator — the same 2-bit
                // encoding every JIT tier's `fn_variant`/`effectiveVariant` decodes.
                let variant_val = (func.is_async as u32) | ((func.is_generator as u32) << 1);
                // The nested function's declared-parameter slots (its own state space),
                // advertised as an array value so every backend's closure binds
                // `args[i]` into `state[param_slots[i]]` at call time. Literal(0) for
                // a parameterless function keeps the common case op-free.
                let param_slots = Self::param_slot_ids(&func.params, self.global_ctxt);
                let params_op = if param_slots.is_empty() {
                    Operand::Literal(0)
                } else {
                    let mut item_slots = Vec::with_capacity(param_slots.len());
                    for slot_id in param_slots {
                        let tmp = self.fresh_slot();
                        out.extend(
                            Operation::Lit32 {
                                dest: tmp,
                                val: slot_id,
                            }
                            .emit(),
                        );
                        item_slots.push(Operand::StateRef(tmp));
                    }
                    let arr_slot = self.fresh_slot();
                    out.extend(Operation::Arr(item_slots, arr_slot).emit());
                    Operand::StateRef(arr_slot)
                };
                out.extend(
                    Operation::Fn {
                        variant: Operand::Literal(variant_val),
                        // Free-variable capture (`closure_args`) and the decorator hook
                        // (`spanner`) aren't wired up yet — see `docs/closure-capture-plan.md`.
                        closure_args: Operand::Literal(0),
                        spanner: Operand::Literal(0),
                        params: params_op,
                        j,
                        dest,
                    }
                    .emit(),
                );
                Ok(())
            }
            other => unsupported(format!("{other:?}")),
        }
    }

    fn lower_lit(&mut self, lit: &Lit, dest: u32, out: &mut Vec<u8>) -> Result<(), FrontendError> {
        match lit {
            Lit::Num(n) => {
                if n.value.fract() != 0.0 || n.value < 0.0 || n.value > u32::MAX as f64 {
                    return unsupported(format!(
                        "numeric literal {} (Jade's LIT32 only represents non-negative integers up to u32::MAX)",
                        n.value
                    ));
                }
                out.extend(
                    Operation::Lit32 {
                        dest,
                        val: n.value as u32,
                    }
                    .emit(),
                );
                Ok(())
            }
            Lit::Bool(b) => {
                out.extend(Operation::Bool { val: b.value, dest }.emit());
                Ok(())
            }
            Lit::Str(s) => {
                let Some(str_val) = s.value.as_str() else {
                    return unsupported("string literal containing an unpaired UTF-16 surrogate");
                };
                let op = self.lower_string_into_fresh_slot(str_val, out);
                // Alias into `dest` (see `Item::Just` for why a SEL is used as a move).
                out.extend(
                    Operation::Sel {
                        cond: Operand::Literal(1),
                        then: op,
                        else_: op,
                        dest,
                    }
                    .emit(),
                );
                Ok(())
            }
            Lit::Null(_) => unsupported("`null` literal (no Jade opcode produces it)"),
            Lit::Regex(_) => unsupported("regex literal"),
            Lit::BigInt(_) => unsupported("BigInt literal"),
            Lit::JSXText(_) => unsupported("JSX text"),
        }
    }

    /// Emit a Jade `STR` building `s` from its UTF-32 code points into a fresh slot,
    /// returning an operand referencing that slot.
    fn lower_string_into_fresh_slot(&mut self, s: &str, out: &mut Vec<u8>) -> Operand {
        let dest = self.fresh_slot();
        let items: Vec<Operand> = s.chars().map(|c| Operand::Literal(c as u32)).collect();
        out.extend(Operation::Str(items, dest).emit());
        Operand::StateRef(dest)
    }

    fn lower_bin(
        &mut self,
        op: BinaryOp,
        left: &Ident,
        right: &Ident,
        dest: u32,
        out: &mut Vec<u8>,
    ) -> Result<(), FrontendError> {
        let a = self.operand_for(left);
        let b = self.operand_for(right);
        let bytes = match op {
            BinaryOp::EqEqEq => Operation::Eq { a, b, dest }.emit().collect::<Vec<_>>(),
            BinaryOp::NotEqEq => Operation::Ne { a, b, dest }.emit().collect(),
            BinaryOp::Lt => Operation::Lt { a, b, dest }.emit().collect(),
            BinaryOp::LtEq => Operation::Le { a, b, dest }.emit().collect(),
            BinaryOp::Gt => Operation::Gt { a, b, dest }.emit().collect(),
            BinaryOp::GtEq => Operation::Ge { a, b, dest }.emit().collect(),
            other => {
                return unsupported(format!(
                    "binary operator {other:?} (only ===, !==, <, <=, >, >= have a Jade opcode)"
                ));
            }
        };
        out.extend(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portal_solutions_jade_vm_jit::{Config, VecRegistry, compile};

    /// Compile `src` end-to-end (parse -> TAC -> bytecode -> JIT) and return the emitted
    /// JS source. Structural assertions on this string are the same style already used by
    /// `jade-vm-jit`'s own unit tests; full runtime semantics are covered by the
    /// real-Node-execution tests below and the browser end-to-end test in
    /// `jade-vm-e2e-tests`.
    fn jit_js(src: &str) -> String {
        let bytecode = compile_to_bytecode(src).expect("frontend compile failed");
        let (js, _reg) =
            compile(&bytecode, VecRegistry::new(), Config::default()).expect("JIT compile failed");
        js
    }

    /// Actually *run* the JIT-emitted JS via Node (rather than just checking its shape),
    /// wrapped as a bare function body. Only exercises programs with no `tenant`/`nt`
    /// object or call ops in scope.
    fn run_js(src: &str) -> String {
        let js = jit_js(src);
        let script = format!(
            "const fn = new Function('state', {body}); console.log(JSON.stringify(fn([])));",
            body = serde_json_escape(&js),
        );
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(&script)
            .output()
            .expect("failed to run node");
        assert!(
            output.status.success(),
            "node failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// Minimal JS string-literal escaper sufficient for JIT-emitted code (no control
    /// characters or unpaired surrogates are ever produced by this frontend).
    fn serde_json_escape(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    #[test]
    fn compiles_return_literal() {
        assert_eq!(run_js("return 42;"), "42");
    }

    #[test]
    fn compiles_if_else() {
        assert_eq!(run_js("if (true) { return 1; } else { return 2; }"), "1");
        assert_eq!(run_js("if (false) { return 1; } else { return 2; }"), "2");
    }

    #[test]
    fn compiles_while_loop() {
        assert_eq!(
            run_js("var x = true; while (x) { x = false; } return x;"),
            "false"
        );
    }

    #[test]
    fn compiles_nested_if_in_while() {
        let js = jit_js(
            "var x = true; while (x) { x = false; if (true) { var y = 1; } else { var y = 2; } } return x;",
        );
        assert!(
            js.contains("__ip"),
            "expected a jump-based dispatch loop, got:\n{js}"
        );
        assert_eq!(
            run_js(
                "var x = true; while (x) { x = false; if (true) { var y = 1; } else { var y = 2; } } return x;"
            ),
            "false"
        );
    }

    #[test]
    fn rejects_unsupported_switch_statement() {
        let err =
            compile_to_bytecode("switch (1) { case 1: return 1; default: return 0; }").unwrap_err();
        assert!(matches!(err, FrontendError::Unsupported(_)), "got: {err:?}");
    }

    #[test]
    fn return_inside_while_loop_now_works_correctly() {
        // Previously rejected outright (`rejects_return_inside_loop_body`): the old
        // structured (`ssa-reloop2`-based) lowering silently dropped this pattern's
        // content. Jade bytecode is jump-based now, so `return` is valid in any block —
        // this compiles *and* executes correctly, with no special-casing at all.
        assert_eq!(
            run_js(
                "var x = true; while (x) { if (true) { return 7; } else { x = false; } } return 99;"
            ),
            "7"
        );
    }

    #[test]
    fn if_else_return_values_are_distinguishable() {
        assert_eq!(run_js("if (true) { return 1; } else { return 2; }"), "1");
        assert_eq!(run_js("if (false) { return 1; } else { return 2; }"), "2");
    }

    /// Run compiled JS that references `tenant`/`nt` (i.e. contains at least one nested
    /// closure and/or a `CALL` op) — unlike `run_js`, wraps `body` with a `markGuestFn`
    /// stub and the JIT's `prelude` (nested function declarations) in scope, and threads
    /// `undefined` for both `tenant` and `nt`.
    fn run_js_with_tenant(src: &str) -> String {
        let bytecode = compile_to_bytecode(src).expect("frontend compile failed");
        let (body, reg) =
            compile(&bytecode, VecRegistry::new(), Config::default()).expect("JIT compile failed");
        // The emitted code drives the real tenant ABI (`tenant.driveTenant(...)`,
        // `tenant.makeFunction(...)`, `tenant.invoke(...)` with the markGuestFn-registered
        // calling convention), so the stub must be ABI-faithful, not `undefined`.
        let script = format!(
            "const __guestFns = new WeakMap();\n\
             function markGuestFn(f, m) {{ __guestFns.set(f, m); return f; }}\n\
             const tenant = {{\n\
               makeFunction: (f) => f,\n\
               make: (proto) => Object.create(proto ?? null),\n\
               set: (o, k, v) => {{ o[k] = v; }},\n\
               get: (o, k) => o[k],\n\
               assign: (dst, src) => Object.assign(dst, src),\n\
               define: (t, d) => {{}},\n\
               driveTenant: (g) => g,\n\
               invoke(fn, inv) {{\n\
                 const meta = __guestFns.get(fn);\n\
                 const args = meta && meta.abi === 'leading-tenant-nt' ? [tenant, undefined, ...inv.args] : inv.args;\n\
                 return Reflect.apply(fn, inv.thisArg, args);\n\
               }},\n\
             }};\n\
             {prelude}\n\
             const fn = new Function('tenant', 'nt', 'state', {body});\n\
             console.log(JSON.stringify(fn(tenant, undefined, [])));",
            prelude = reg.prelude(),
            body = serde_json_escape(&body),
        );
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(&script)
            .output()
            .expect("failed to run node");
        assert!(
            output.status.success(),
            "node failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// `Item::Func` now actually lowers to a real `FN` opcode (previously an explicit
    /// `Unsupported` rejection) — the simplest possible case: a nested closure with no
    /// control flow of its own, called once.
    #[test]
    fn compiles_and_calls_a_nested_closure() {
        assert_eq!(
            run_js_with_tenant("var inner = function () { return 42; }; return inner();"),
            "42"
        );
    }

    /// Declared parameters bind call arguments: the FN op's `params` operand advertises
    /// the nested function's parameter slots and every backend's closure writes
    /// `args[i]` into `state[params[i]]` at call time.
    #[test]
    fn nested_closure_parameter_receives_its_argument() {
        assert_eq!(
            run_js_with_tenant("var id = function (x) { return x; }; return id(42);"),
            "42"
        );
    }

    #[test]
    fn nested_closure_two_parameters_bind_in_order() {
        assert_eq!(
            run_js_with_tenant(
                "var eq = function (a, b) { return a === b; }; return eq(7, 7);"
            ),
            "true"
        );
        assert_eq!(
            run_js_with_tenant(
                "var eq = function (a, b) { return a === b; }; return eq(7, 8);"
            ),
            "false"
        );
    }

    /// A missing argument binds `undefined`; extras are dropped — ordinary JS
    /// parameter semantics.
    #[test]
    fn nested_closure_missing_and_extra_arguments() {
        assert_eq!(
            run_js_with_tenant("var second = function (a, b) { return b; }; return second(1);"),
            "undefined"
        );
        assert_eq!(
            run_js_with_tenant("var id = function (x) { return x; }; return id(11, 22, 33);"),
            "11"
        );
    }

    /// Parameters are per-function slots: two calls see their own arguments, and a
    /// parameterless callee's encoding stays `Literal(0)` (no binding work at all).
    #[test]
    fn nested_closure_parameters_do_not_leak_across_calls() {
        assert_eq!(
            run_js_with_tenant(
                "var id = function (x) { return x; }; var a = id(1); var b = id(2); return a === b;"
            ),
            "false"
        );
        assert_eq!(
            run_js_with_tenant(
                "var id = function (x) { return x; }; id(5); return id(6);"
            ),
            "6"
        );
    }

    /// The nested closure's own body has its own multi-block control flow (an `if`/`else`)
    /// — exercises `compile_program`'s per-function `discover_reachable`/block-offset
    /// machinery recursively, not just a single straight-line nested block.
    #[test]
    fn nested_closure_with_its_own_branch_executes_correctly() {
        assert_eq!(
            run_js_with_tenant(
                "var inner = function () { if (true) { return 1; } else { return 2; } }; return inner();"
            ),
            "1"
        );
    }

    /// Static member *writes* (`o.x = v`): swc-tac's assignment-target lowering used to
    /// leak the key as a synthetic variable reference (rejected here as a bogus "closure
    /// capture of `x`"); fixed upstream in jsaw-core's conv.rs, this pins the jade-side
    /// end-to-end behavior.
    #[test]
    fn static_member_assignment_executes_correctly() {
        assert_eq!(run_js_with_tenant("var o = {}; o.x = 5; return o.x;"), "5");
        assert_eq!(
            run_js_with_tenant("var o = {}; o.x = 1; o.y = 2; return o.x === 1 ? o.y : 4;"),
            "2"
        );
    }

    /// Two independent nested closures in the same outer function: proves
    /// `compile_program`'s region layout (each function's own `j` computed from its own
    /// `region_offsets` entry, via `children_base + nested_fn_counter`) doesn't confuse
    /// sibling functions with each other.
    #[test]
    fn two_sibling_nested_closures_do_not_collide() {
        assert_eq!(
            run_js_with_tenant(
                "var a = function () { return 10; }; var b = function () { return 20; }; return a() === b();"
            ),
            "false"
        );
        assert_eq!(
            run_js_with_tenant(
                "var a = function () { return 10; }; var b = function () { return 10; }; return a() === b();"
            ),
            "true"
        );
    }

    /// A closure nested inside another nested closure — exercises `compile_program`'s
    /// discovery loop actually being transitive (a function discovered while measuring
    /// function `k` can itself contain further nested functions, appended and measured in
    /// the same pass), not just one level deep.
    ///
    /// Note: `inner`'s call is deliberately *not* in tail position (`var r = inner();
    /// return r;`, not `return inner();`) — a tail-position call lowers to TAC's
    /// `TTerm::Tail`, which this frontend already explicitly rejects as unsupported
    /// (`targets_of`) independent of closures entirely; tail-call lowering is a separate,
    /// unrelated gap, not something nested-closure support needs to (or does) fix.
    #[test]
    fn doubly_nested_closure_executes_correctly() {
        assert_eq!(
            run_js_with_tenant(
                "var outer = function () { \
                     var inner = function () { return 7; }; \
                     var r = inner(); \
                     return r; \
                 }; \
                 var r2 = outer(); \
                 return r2;"
            ),
            "7"
        );
    }

    #[test]
    fn constant_condition_is_folded_away() {
        let js = jit_js("if (true) { return 111; } else { return 222; }");
        assert!(js.contains("111"), "got:\n{js}");
        assert!(
            !js.contains("222"),
            "expected the untaken branch to be folded away, got:\n{js}"
        );

        let js = jit_js("if (false) { return 111; } else { return 222; }");
        assert!(
            !js.contains("111"),
            "expected the untaken branch to be folded away, got:\n{js}"
        );
        assert!(js.contains("222"), "got:\n{js}");
    }
}



#[cfg(test)]
mod capture_probe2 {
    #[test]
    fn scope_lex_close_refs() {
        let src = "var probe;\n{ let x = 'inside'; probe = function() { return x; }; }\nlet x = 'outside';\nassert.sameValue(probe(), 'inside');";
        let (mut stmts, g) = swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
            let mut s = crate::parse_script(src).unwrap();
            let g = crate::globals::resolve_globals(&mut s).map(|r| r.ctxt);
            (s, g)
        });
        let synthetic = swc_ecma_ast::Function {
            params: vec![], decorators: vec![], span: swc_common::DUMMY_SP,
            ctxt: Default::default(),
            body: Some(swc_ecma_ast::BlockStmt { span: swc_common::DUMMY_SP, ctxt: Default::default(), stmts }),
            is_generator: false, is_async: false, type_params: None, return_type: None,
        };
        swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
            let tfunc = portal_jsc_swc_tac::TFunc::try_from(&synthetic).unwrap();
            fn walk(f: &portal_jsc_swc_tac::TFunc, d: usize) {
                eprintln!("{}refs: {:?}", " ".repeat(d), f.cfg.refs().collect::<Vec<_>>());
                eprintln!("{}decls: {:?}", " ".repeat(d), f.cfg.decls);
                for b in f.cfg.blocks.iter() {
                    for s in &b.1.stmts {
                        if let portal_jsc_swc_tac::Item::Func { func, .. } = &s.right {
                            walk(func, d + 2);
                        }
                    }
                }
            }
            walk(&tfunc, 0);
        });
    }
}

#[cfg(test)]
mod capture_check_test {
    #[test]
    fn rejects_let_capture() {
        let err = crate::compile_to_bytecode(
            "var probe;\n{ let x = 'inside'; probe = function() { return x; }; }\nlet x = 'outside';",
        );
        assert!(matches!(err, Err(crate::FrontendError::Unsupported(_))), "got: {err:?}");
    }

    #[test]
    fn rejects_real_scope_lex_close_body() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/test262/test/language/statements/block/scope-lex-close.js"),
        )
        .unwrap();
        let err = crate::compile_to_bytecode(&src);
        assert!(matches!(err, Err(crate::FrontendError::Unsupported(_))), "got: {err:?}");
    }
}

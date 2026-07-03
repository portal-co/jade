//! JS-source-to-Jade-bytecode frontend.
//!
//! Pipeline: source text -> SWC AST (`swc_ecma_parser`) -> TAC (`portal_jsc_swc_tac`,
//! which internally lowers through `portal_jsc_swc_cfg`) -> structured control flow
//! (`ssa_reloop2`, a from-scratch Stackifier that — unlike the older `ssa-reloop` crate —
//! doesn't wrap the external `relooper` crate, whose original design assumed a bytecode
//! target rather than a generic CFG) -> Jade `Operation`s (`portal_solutions_jade_vm`).
//!
//! **Scope**: only lowers constructs that have a direct Jade bytecode opcode — see
//! `packages/jade-data/index.ts` for the authoritative opcode list. Notably, Jade bytecode
//! currently has no arithmetic opcodes (only the six comparisons), no way to bind function
//! parameters to state slots, no exception handling, and no opcode for `undefined` as a
//! produced value. Everything without a direct mapping is an explicit
//! [`FrontendError::Unsupported`], never a silent miscompile. See
//! `docs/bytecode-cfg-plan.md` for future plans to move the bytecode format itself to a
//! CFG-native representation, which would remove the need for the structuring step here.

use std::collections::HashMap;

use portal_jsc_swc_tac::{Item, LId, TBlock, TCallee, TCatch, TCfg, TFunc, TPostecedent, TStmt, TTerm};
use portal_solutions_jade_vm::{Operand, Operation, SignedOperand};
use ssa_reloop2::{BranchMode, StructuredBlock};
use swc_common::sync::Lrc;
use swc_common::{FileName, SourceMap};
use swc_ecma_ast::{BinaryOp, BlockStmt, Function, Lit, MetaPropKind};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, StringInput, Syntax};

type Ident = swc_ecma_ast::Id;

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
    Unsupported(Unsupported),
}

impl std::fmt::Display for FrontendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrontendError::Parse(s) => write!(f, "parse error: {s}"),
            FrontendError::Tac(s) => write!(f, "AST->TAC lowering error: {s}"),
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
pub fn compile_to_bytecode(src: &str) -> Result<Vec<u8>, FrontendError> {
    swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let stmts = parse_script(src)?;
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
            is_generator: false,
            is_async: false,
            type_params: None,
            return_type: None,
        };
        let tfunc = TFunc::try_from(&synthetic).map_err(|e| FrontendError::Tac(format!("{e:?}")))?;
        let mut compiler = Compiler::default();
        let (main, trailer) = compiler.compile_function(&tfunc)?;
        let mut code = main;
        code.extend(trailer);
        Ok(code)
    })
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

/// Per-function lowering state: a fresh Jade state-slot numbering (parameters and
/// temporaries alike), plus the loop-nesting context needed to translate
/// `ssa_reloop2::BranchMode::{LoopBreak,LoopContinue}` into Jade's structured (no
/// break/continue) `WHILE`.
#[derive(Default)]
struct Compiler {
    /// Byte-encoded bodies of nested functions, appended after the top-level code.
    /// Each entry's start offset (relative to the *final* buffer, after the caller
    /// concatenates `main ++ trailer`) is threaded back into the `FN` opcode that
    /// references it.
    trailer: Vec<u8>,
}

struct FnLowering<'a> {
    tcfg: &'a TCfg,
    slots: HashMap<Ident, u32>,
    next_slot: u32,
    /// Lazily-allocated slot holding a value that is never written — reading it
    /// therefore yields `undefined`, used for e.g. `return;` (Jade has no dedicated
    /// "produce undefined" opcode).
    undefined_slot: Option<u32>,
    /// Lazily-allocated "control-flow flag" slot, used to encode which branch of an
    /// `if`/`Multiple` dispatch was taken so a Jade `SWITCH` can pick the matching arm.
    cff_slot: Option<u32>,
    /// Innermost-first stack of `(loop_id, break_flag_slot)` for currently-open loops.
    loop_stack: Vec<(u32, u32)>,
    /// Lazily-allocated slot holding the function's eventual return value. Jade's `RET`
    /// opcode is only valid as the very last op of the *whole* function body — nested
    /// `IF`/`WHILE`/`SWITCH` bodies dispatch through `exec_op`, which has no arm for it
    /// (see `jade-vm-core::dispatch::exec_op`'s catch-all `"unexpected opcode"`). So a
    /// `return` reached from inside any nested body writes here and sets
    /// `not_returned_flag` instead of emitting `RET` directly; the real `RET` is emitted
    /// once, by `compile_function`, after the whole structured tree has been lowered.
    return_slot: Option<u32>,
    /// Eagerly-allocated bool flag, `true` until an early return sets it `false`.
    ///
    /// This is needed even for a plain, non-loop `if (c) { return a; } else { return b; }`:
    /// `ssa-reloop2`'s reconverge heuristic (`find_reconverge`), when every branch is a
    /// dead end (no further targets, as any `return`-terminated block is), can only own
    /// one branch's region and hoists the *other* branch's content into the Multiple's
    /// shared, unconditionally-run `next` — so without this flag that branch's value would
    /// be silently overwritten after the switch. Every loop's `WHILE` condition is also
    /// gated by its own break flag (see `loop_stack` above), and the code following a loop
    /// is additionally gated by this flag.
    not_returned_flag: u32,
}

impl Compiler {
    /// Compile `tfunc`'s body. Returns `(main_bytes, trailer_bytes)`; the caller
    /// concatenates them (nested-function offsets in `main_bytes` are already relative
    /// to the concatenation, i.e. `main_bytes.len() + offset_within_trailer`).
    fn compile_function(&mut self, tfunc: &TFunc) -> Result<(Vec<u8>, Vec<u8>), FrontendError> {
        let mut lowering = FnLowering {
            tcfg: &tfunc.cfg,
            slots: HashMap::new(),
            next_slot: 0,
            undefined_slot: None,
            cff_slot: None,
            loop_stack: Vec::new(),
            return_slot: None,
            not_returned_flag: 0,
        };
        lowering.not_returned_flag = lowering.fresh_slot();
        // `compile_to_bytecode` always synthesizes a zero-parameter function (see
        // `compile_to_bytecode`), so `tfunc.params` is always empty here — nothing to bind.
        let structured = ssa_reloop2::go(tfunc);
        reject_return_inside_loop(&structured, &tfunc.cfg)?;
        let mut main = Operation::Bool { val: true, dest: lowering.not_returned_flag }.emit().collect::<Vec<_>>();
        lowering.lower_block(&structured, &mut self.trailer, &mut main)?;
        let ret_op = match lowering.return_slot {
            Some(slot) => Operand::StateRef(slot),
            None => lowering.undefined_operand(),
        };
        main.extend(Operation::Ret(ret_op).emit());
        Ok((main, std::mem::take(&mut self.trailer)))
    }
}

/// Conservative pre-check: reject a function outright if it contains any loop *and* more
/// than one distinct `return`-terminated block.
///
/// `ssa-reloop2`'s reconverge heuristic (`find_reconverge` in the `ssa-reloop2` crate)
/// assumes every edge leaving a loop's structural slice converges on that loop's single
/// shared `next`. A `return` reached via a branch nested inside a loop body breaks that
/// assumption — its content ends up neither in its own arm nor in `next`, so it's silently
/// never emitted at all. Detecting the exact unsafe shape precisely would need the same
/// dominance analysis `ssa-reloop2` does internally; this over-approximates instead
/// (rejecting some loops that would actually be fine, e.g. an early return positioned
/// entirely before a loop starts) until the bytecode format itself moves to a CFG — see
/// `docs/bytecode-cfg-plan.md`.
fn reject_return_inside_loop(
    sb: &StructuredBlock<portal_jsc_swc_tac::TBlockId>,
    cfg: &TCfg,
) -> Result<(), FrontendError> {
    let mut has_loop = false;
    let mut return_labels = std::collections::BTreeSet::new();
    walk(sb, &mut has_loop, &mut return_labels, cfg);

    fn walk(
        sb: &StructuredBlock<portal_jsc_swc_tac::TBlockId>,
        has_loop: &mut bool,
        return_labels: &mut std::collections::BTreeSet<portal_jsc_swc_tac::TBlockId>,
        cfg: &TCfg,
    ) {
        match sb {
            StructuredBlock::Simple(s) => {
                if matches!(cfg.blocks[s.label].post.term, TTerm::Return(_)) {
                    return_labels.insert(s.label);
                }
                if let Some(im) = &s.immediate {
                    walk(im, has_loop, return_labels, cfg);
                }
                if let Some(nx) = &s.next {
                    walk(nx, has_loop, return_labels, cfg);
                }
            }
            StructuredBlock::Loop(l) => {
                *has_loop = true;
                walk(&l.inner, has_loop, return_labels, cfg);
                if let Some(nx) = &l.next {
                    walk(nx, has_loop, return_labels, cfg);
                }
            }
            StructuredBlock::Multiple(m) => {
                for h in &m.handled {
                    walk(&h.inner, has_loop, return_labels, cfg);
                }
            }
        }
    }

    if has_loop && return_labels.len() > 1 {
        return unsupported(
            "return from inside a loop body, alongside another return elsewhere in the \
             same function (not yet supported by this frontend — see docs/bytecode-cfg-plan.md)",
        );
    }
    Ok(())
}

impl<'a> FnLowering<'a> {
    fn slot_for(&mut self, id: &Ident) -> u32 {
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

    fn cff_slot(&mut self) -> u32 {
        if let Some(s) = self.cff_slot {
            return s;
        }
        let s = self.fresh_slot();
        self.cff_slot = Some(s);
        s
    }

    fn return_slot(&mut self) -> u32 {
        if let Some(s) = self.return_slot {
            return s;
        }
        let s = self.fresh_slot();
        self.return_slot = Some(s);
        s
    }

    /// Lower `next` (the code that structurally follows a block/loop), guarded by
    /// `not_returned_flag` — see the field's doc comment for why this guard is required
    /// even outside of loops.
    fn lower_continuation(
        &mut self,
        next: &Option<Box<StructuredBlock<portal_jsc_swc_tac::TBlockId>>>,
        trailer: &mut Vec<u8>,
        out: &mut Vec<u8>,
    ) -> Result<(), FrontendError> {
        let Some(nx) = next else { return Ok(()) };
        let mut body = Vec::new();
        self.lower_block(nx, trailer, &mut body)?;
        out.extend(
            Operation::If { cond: Operand::StateRef(self.not_returned_flag), then_body: body, else_body: Vec::new() }
                .emit(),
        );
        Ok(())
    }

    /// Lower a single TAC statement, materializing its value into a slot determined by
    /// its `LId` (a plain identifier gets its own slot; a member write also emits the
    /// Jade `SET` and discards the write's own result slot).
    fn lower_stmt(&mut self, stmt: &TStmt, out: &mut Vec<u8>) -> Result<(), FrontendError> {
        match &stmt.left {
            LId::Id { id } => {
                let dest = self.slot_for(id);
                self.lower_item(&stmt.right, dest, out)
            }
            LId::Member { obj, mem } => {
                let val_slot = self.fresh_slot();
                self.lower_item(&stmt.right, val_slot, out)?;
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

    fn lower_item(&mut self, item: &Item<Ident, TFunc>, dest: u32, out: &mut Vec<u8>) -> Result<(), FrontendError> {
        match item {
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
                out.extend(Operation::Get { obj: obj_op, key: key_op, dest }.emit());
                Ok(())
            }
            Item::Select { cond, then, otherwise } => {
                let cond_op = self.operand_for(cond);
                let then_op = self.operand_for(then);
                let else_op = self.operand_for(otherwise);
                out.extend(
                    Operation::Sel { cond: cond_op, then: then_op, else_: else_op, dest }.emit(),
                );
                Ok(())
            }
            Item::Call { callee, args } => {
                let TCallee::Val(f) = callee else {
                    return unsupported("call target other than a plain value (method calls, `super`, `import()`, `eval`)");
                };
                let mut arg_ops = Vec::with_capacity(args.len());
                for a in args {
                    if a.is_spread {
                        return unsupported("spread arguments in a call");
                    }
                    arg_ops.push(self.operand_for(&a.value));
                }
                let fn_op = self.operand_for(f);
                out.extend(Operation::Call { fn_op, args: arg_ops, dest }.emit());
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
            Item::Meta { prop: MetaPropKind::NewTarget } => {
                out.extend(Operation::NewTarget(dest).emit());
                Ok(())
            }
            Item::Func { .. } => unsupported("nested function/closure capture (not yet wired in the frontend)"),
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
                out.extend(Operation::Lit32 { dest, val: n.value as u32 }.emit());
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
                out.extend(Operation::Sel { cond: Operand::Literal(1), then: op, else_: op, dest }.emit());
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
            other => return unsupported(format!("binary operator {other:?} (only ===, !==, <, <=, >, >= have a Jade opcode)")),
        };
        out.extend(bytes);
        Ok(())
    }

    /// Lower a structured-control-flow node into Jade bytecode, appending nested-function
    /// bodies to `trailer` and emitting the current function's own code into `out`.
    fn lower_block(
        &mut self,
        sb: &StructuredBlock<portal_jsc_swc_tac::TBlockId>,
        trailer: &mut Vec<u8>,
        out: &mut Vec<u8>,
    ) -> Result<(), FrontendError> {
        match sb {
            StructuredBlock::Simple(s) => {
                let block: &TBlock = &self.tcfg.blocks[s.label];
                for stmt in &block.stmts {
                    self.lower_stmt(stmt, out)?;
                }
                if !matches!(block.post.catch, TCatch::Throw) {
                    return unsupported("try/catch (Jade bytecode has no exception-handling opcode)");
                }
                // `lower_terminator` fully handles `s.immediate` itself (it is always either
                // `None` or `Some(Multiple)` — ssa-reloop2 never puts a plain fallthrough
                // block there, only the reconvergence-via-switch case — and the switch is
                // built directly from `branches`/`immediate` inside `lower_terminator`).
                let terminated = self.lower_terminator(&block.post, &s.branches, &s.immediate, trailer, out)?;
                if !terminated {
                    self.lower_continuation(&s.next, trailer, out)?;
                }
                Ok(())
            }
            StructuredBlock::Loop(l) => {
                let break_flag = self.fresh_slot();
                out.extend(Operation::Bool { val: true, dest: break_flag }.emit());
                self.loop_stack.push((l.loop_id, break_flag));
                let mut body = Vec::new();
                self.lower_block(&l.inner, trailer, &mut body)?;
                self.loop_stack.pop();
                out.extend(
                    Operation::While {
                        cond: Operand::StateRef(break_flag),
                        body,
                        next: Operand::StateRef(break_flag),
                    }
                    .emit(),
                );
                self.lower_continuation(&l.next, trailer, out)?;
                Ok(())
            }
            StructuredBlock::Multiple(_) => {
                unsupported("Multiple block reached outside of terminator dispatch (internal invariant)")
            }
        }
    }

    /// Handle one `branches`-mapped target during terminator lowering: `None` output
    /// means "nothing to emit here, the continuation lives in `next`/the `Multiple`
    /// dispatch"; `Some` early-returns from the current arm.
    fn lower_branch_arm(&mut self, target: portal_jsc_swc_tac::TBlockId, branches: &std::collections::BTreeMap<portal_jsc_swc_tac::TBlockId, BranchMode>) -> Result<Vec<u8>, FrontendError> {
        match branches.get(&target) {
            None | Some(BranchMode::MergedBranch) => Ok(Vec::new()),
            Some(BranchMode::LoopContinue(_)) => Ok(Vec::new()),
            Some(BranchMode::LoopBreak(id)) => {
                let flag = self
                    .loop_stack
                    .iter()
                    .rev()
                    .find(|(lid, _)| lid == id)
                    .map(|(_, slot)| *slot)
                    .ok_or_else(|| FrontendError::Tac("LoopBreak with no matching enclosing loop".into()))?;
                Ok(Operation::Bool { val: false, dest: flag }.emit().collect())
            }
            Some(other) => unsupported(format!("{other:?} branch mode")),
        }
    }

    /// Handle one target during a *Multiple*-dispatch terminator: as above, but on the
    /// `MergedBranch`/unlisted path we additionally set the `cff` slot so the following
    /// `SWITCH` picks the right arm.
    fn lower_branch_arm_with_cff(
        &mut self,
        target: portal_jsc_swc_tac::TBlockId,
        branches: &std::collections::BTreeMap<portal_jsc_swc_tac::TBlockId, BranchMode>,
    ) -> Result<Vec<u8>, FrontendError> {
        match branches.get(&target) {
            None | Some(BranchMode::MergedBranch) => {
                let cff = self.cff_slot();
                Ok(Operation::Lit32 { dest: cff, val: target.index() as u32 }.emit().collect())
            }
            _ => self.lower_branch_arm(target, branches),
        }
    }

    /// Lower `post`'s terminator. Returns `Ok(true)` if it fully terminates this arm of
    /// control flow — currently only a bare `Return` (so the caller must not additionally
    /// process `next`, which would be unreachable dead code after it anyway).
    fn lower_terminator(
        &mut self,
        post: &TPostecedent,
        branches: &std::collections::BTreeMap<portal_jsc_swc_tac::TBlockId, BranchMode>,
        immediate: &Option<Box<StructuredBlock<portal_jsc_swc_tac::TBlockId>>>,
        trailer: &mut Vec<u8>,
        out: &mut Vec<u8>,
    ) -> Result<bool, FrontendError> {
        match &post.term {
            TTerm::Return(val) => {
                if !self.loop_stack.is_empty() {
                    // `ssa-reloop2`'s reconverge heuristic doesn't own a `return`'s content
                    // as a distinct exit target when it's nested inside a loop's own
                    // conditional (unlike the non-loop case, `not_returned_flag` gating
                    // alone isn't enough here — the content silently never gets emitted
                    // at all). Reject rather than silently miscompile; see
                    // `docs/bytecode-cfg-plan.md`.
                    return unsupported(
                        "return from inside a loop body (not yet supported by this frontend)",
                    );
                }
                // Can't emit `RET` here — see the doc comment on `FnLowering::return_slot`.
                if let Some(id) = val {
                    let val_op = self.operand_for(id);
                    let slot = self.return_slot();
                    out.extend(Operation::Sel { cond: Operand::Literal(1), then: val_op, else_: val_op, dest: slot }.emit());
                }
                out.extend(Operation::Bool { val: false, dest: self.not_returned_flag }.emit());
                Ok(true)
            }
            TTerm::Default => Ok(false),
            TTerm::Jmp(target) => {
                let uses_multi = matches!(immediate.as_deref(), Some(StructuredBlock::Multiple(_)));
                let bytes = if uses_multi {
                    self.lower_branch_arm_with_cff(*target, branches)?
                } else {
                    self.lower_branch_arm(*target, branches)?
                };
                out.extend(bytes);
                if let Some(im) = immediate {
                    self.lower_dispatch(im, trailer, out)?;
                }
                // Every switch case `break_after`s, so control always falls through past
                // the dispatch to whatever `next` represents — the caller must still run it.
                Ok(false)
            }
            TTerm::CondJmp { cond, if_true, if_false } => {
                let cond_op = self.operand_for(cond);
                let uses_multi = matches!(immediate.as_deref(), Some(StructuredBlock::Multiple(_)));
                let (then_body, else_body) = if uses_multi {
                    (
                        self.lower_branch_arm_with_cff(*if_true, branches)?,
                        self.lower_branch_arm_with_cff(*if_false, branches)?,
                    )
                } else {
                    (
                        self.lower_branch_arm(*if_true, branches)?,
                        self.lower_branch_arm(*if_false, branches)?,
                    )
                };
                out.extend(Operation::If { cond: cond_op, then_body, else_body }.emit());
                if let Some(im) = immediate {
                    self.lower_dispatch(im, trailer, out)?;
                }
                Ok(false)
            }
            TTerm::Switch { .. } => unsupported("`switch` statement (structural Multiple-dispatch for it is not yet wired)"),
            TTerm::Throw(_) => unsupported("`throw` (Jade bytecode has no exception-handling opcode)"),
            TTerm::Tail { .. } => unsupported("tail call"),
        }
    }

    /// Lower the `Multiple` dispatch reached via `immediate`, as a Jade `SWITCH` on the
    /// `cff` slot (one case per `HandledBlock`, matched on its (sole) label's index).
    fn lower_dispatch(
        &mut self,
        im: &StructuredBlock<portal_jsc_swc_tac::TBlockId>,
        trailer: &mut Vec<u8>,
        out: &mut Vec<u8>,
    ) -> Result<(), FrontendError> {
        let StructuredBlock::Multiple(m) = im else {
            return unsupported("terminator's `immediate` was not a Multiple block (internal invariant)");
        };
        let cff = self.cff_slot();
        let mut cases = Vec::with_capacity(m.handled.len());
        for h in &m.handled {
            let &[label] = h.labels.as_slice() else {
                return unsupported("Multiple arm with zero or multiple labels (not produced by ssa-reloop2 today, but not handled here)");
            };
            let mut body = Vec::new();
            self.lower_block(&h.inner, trailer, &mut body)?;
            cases.push((label.index() as u32, body));
        }
        out.extend(
            Operation::Switch { val: Operand::StateRef(cff), cases, default_body: Vec::new() }.emit(),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portal_solutions_jade_vm_jit::{compile, Config, VecRegistry};

    /// Compile `src` end-to-end (parse -> TAC -> reloop -> bytecode -> JIT) and return the
    /// emitted JS source. Structural assertions on this string are the same style already
    /// used by `jade-vm-jit`'s own unit tests; full runtime semantics are covered by the
    /// browser end-to-end test in `jade-vm-e2e-tests`.
    fn jit_js(src: &str) -> String {
        let bytecode = compile_to_bytecode(src).expect("frontend compile failed");
        let (js, _reg) = compile(&bytecode, VecRegistry::new(), Config::default()).expect("JIT compile failed");
        js
    }

    #[test]
    fn compiles_return_literal() {
        let js = jit_js("return 42;");
        assert!(js.contains("return state["), "got:\n{js}");
    }

    #[test]
    fn compiles_if_else_via_cff_switch() {
        // Two non-trivial, reconverging arms force the structural Multiple/cff dispatch.
        let js = jit_js("if (true) { var x = 1; } else { var x = 2; } return x;");
        assert!(js.contains("switch"), "expected a structural switch dispatch, got:\n{js}");
    }

    #[test]
    fn compiles_while_loop() {
        // A literal `true` condition is compile-time-foldable and never produces a genuine
        // back-edge (the CFG proves there's no second iteration); a re-assigned variable
        // condition forces a real merge point at the loop header instead.
        let js = jit_js("var x = true; while (x) { x = false; } return x;");
        assert!(js.contains("while (v"), "got:\n{js}");
    }

    #[test]
    fn compiles_nested_if_in_while() {
        let js = jit_js("var x = true; while (x) { x = false; if (true) { var y = 1; } else { var y = 2; } } return x;");
        assert!(js.contains("while (v"), "got:\n{js}");
        assert!(js.contains("switch"), "got:\n{js}");
    }

    #[test]
    fn rejects_unsupported_switch_statement() {
        let err = compile_to_bytecode("switch (1) { case 1: return 1; default: return 0; }").unwrap_err();
        assert!(matches!(err, FrontendError::Unsupported(_)), "got: {err:?}");
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
        assert!(output.status.success(), "node failed: {}", String::from_utf8_lossy(&output.stderr));
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
    fn rejects_return_inside_loop_body() {
        // See `reject_return_inside_loop`'s doc comment: `ssa-reloop2`'s reconverge
        // heuristic silently drops this specific shape's content instead of miscompiling
        // predictably, so it's rejected outright at compile time.
        let err = compile_to_bytecode(
            "var x = true; while (x) { if (true) { return 7; } else { x = false; } } return 99;",
        )
        .unwrap_err();
        assert!(matches!(err, FrontendError::Unsupported(_)), "got: {err:?}");
    }

    #[test]
    fn if_else_return_values_are_distinguishable() {
        assert_eq!(run_js("if (true) { return 1; } else { return 2; }"), "1");
        assert_eq!(run_js("if (false) { return 1; } else { return 2; }"), "2");
    }
}

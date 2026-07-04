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
        // Shared TAC canonicalizer + SSA constant-fold/DCE pass (see
        // `docs/bytecode-cfg-plan.md`); the same `jade-cfg-opt::optimize_tfunc` an
        // optional JIT plugin (Tier 2) can also apply to its own reconstructed CFG.
        let tfunc = portal_solutions_jade_cfg_opt::optimize_tfunc(&tfunc)
            .map_err(|e| FrontendError::Opt(format!("{e:?}")))?;
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

#[derive(Default)]
struct Compiler {
    /// Byte-encoded bodies of nested functions, appended after the top-level code.
    /// Each entry's start offset (relative to the *final* buffer, after the caller
    /// concatenates `main ++ trailer`) is threaded back into the `FN` opcode that
    /// references it. Unused for now — nested closures (`Item::Func`) aren't wired up
    /// yet (see `lower_item`), so this is always empty in practice.
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
        };
        // `compile_to_bytecode` always synthesizes a zero-parameter function (see
        // `compile_to_bytecode`), so `tfunc.params` is always empty here — nothing to bind.
        let main = lowering.compile_blocks(tfunc.entry, &mut self.trailer)?;
        Ok((main, std::mem::take(&mut self.trailer)))
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
    /// ops, assign each a stable byte offset (a simple prefix sum over emission order —
    /// `entry` always first, so it always lands at offset `0`), then lower every block's
    /// terminator with the now-known real target offsets and concatenate.
    fn compile_blocks(&mut self, entry: TBlockId, trailer: &mut Vec<u8>) -> Result<Vec<u8>, FrontendError> {
        let order = self.discover_reachable(entry)?;
        let mut lowered: Vec<(TBlockId, LoweredBlock)> = Vec::with_capacity(order.len());
        for id in &order {
            let block: &TBlock = &self.tcfg.blocks[*id];
            let mut ops_bytes = Vec::new();
            for stmt in &block.stmts {
                self.lower_stmt(stmt, &mut ops_bytes)?;
            }
            if !matches!(block.post.catch, TCatch::Throw) {
                return unsupported("try/catch (Jade bytecode has no exception-handling opcode)");
            }
            lowered.push((*id, LoweredBlock { ops_bytes, term: block.post.term.clone() }));
        }

        // Placeholder-target terminator lengths, to compute each block's byte offset.
        let mut offsets: HashMap<TBlockId, u32> = HashMap::with_capacity(lowered.len());
        let mut cursor = 0u32;
        for (id, block) in &lowered {
            offsets.insert(*id, cursor);
            let term_len = self.emit_terminator(&block.term, &offsets, true)?.len() as u32;
            cursor += block.ops_bytes.len() as u32 + term_len;
        }

        let mut out = Vec::with_capacity(cursor as usize);
        for (_, block) in &lowered {
            out.extend_from_slice(&block.ops_bytes);
            out.extend(self.emit_terminator(&block.term, &offsets, false)?);
        }
        let _ = trailer; // nested functions aren't wired up yet; see `Compiler::trailer`.
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
            TTerm::CondJmp { if_true, if_false, .. } => vec![*if_true, *if_false],
            TTerm::Switch { .. } => return unsupported("`switch` statement (JS `switch`, not yet lowered)"),
            TTerm::Throw(_) => return unsupported("`throw` (Jade bytecode has no exception-handling opcode)"),
            TTerm::Tail { .. } => return unsupported("tail call"),
            TTerm::Default => return unsupported("internal invariant: unreachable TAC terminator"),
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
        let target = |id: &TBlockId| -> u32 {
            if placeholder { 0 } else { offsets[id] }
        };
        Ok(match term {
            TTerm::Return(val) => {
                let op = match val {
                    Some(id) => self.operand_for(id),
                    None => self.undefined_operand(),
                };
                Operation::Ret(op).emit().collect()
            }
            TTerm::Jmp(id) => Operation::Jmp { target: target(id) }.emit().collect(),
            TTerm::CondJmp { cond, if_true, if_false } => {
                let cond_op = self.operand_for(cond);
                Operation::CondJmp { cond: cond_op, if_true: target(if_true), if_false: target(if_false) }
                    .emit()
                    .collect()
            }
            TTerm::Switch { .. } | TTerm::Throw(_) | TTerm::Tail { .. } | TTerm::Default => {
                // `discover_reachable` already rejects these before we ever get here.
                return unsupported("internal invariant: unreachable TAC terminator reached emit_terminator");
            }
        })
    }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use portal_solutions_jade_vm_jit::{compile, Config, VecRegistry};

    /// Compile `src` end-to-end (parse -> TAC -> bytecode -> JIT) and return the emitted
    /// JS source. Structural assertions on this string are the same style already used by
    /// `jade-vm-jit`'s own unit tests; full runtime semantics are covered by the
    /// real-Node-execution tests below and the browser end-to-end test in
    /// `jade-vm-e2e-tests`.
    fn jit_js(src: &str) -> String {
        let bytecode = compile_to_bytecode(src).expect("frontend compile failed");
        let (js, _reg) = compile(&bytecode, VecRegistry::new(), Config::default()).expect("JIT compile failed");
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
        assert_eq!(run_js("var x = true; while (x) { x = false; } return x;"), "false");
    }

    #[test]
    fn compiles_nested_if_in_while() {
        let js = jit_js(
            "var x = true; while (x) { x = false; if (true) { var y = 1; } else { var y = 2; } } return x;",
        );
        assert!(js.contains("__ip"), "expected a jump-based dispatch loop, got:\n{js}");
        assert_eq!(
            run_js("var x = true; while (x) { x = false; if (true) { var y = 1; } else { var y = 2; } } return x;"),
            "false"
        );
    }

    #[test]
    fn rejects_unsupported_switch_statement() {
        let err = compile_to_bytecode("switch (1) { case 1: return 1; default: return 0; }").unwrap_err();
        assert!(matches!(err, FrontendError::Unsupported(_)), "got: {err:?}");
    }

    #[test]
    fn return_inside_while_loop_now_works_correctly() {
        // Previously rejected outright (`rejects_return_inside_loop_body`): the old
        // structured (`ssa-reloop2`-based) lowering silently dropped this pattern's
        // content. Jade bytecode is jump-based now, so `return` is valid in any block —
        // this compiles *and* executes correctly, with no special-casing at all.
        assert_eq!(
            run_js("var x = true; while (x) { if (true) { return 7; } else { x = false; } } return 99;"),
            "7"
        );
    }

    #[test]
    fn if_else_return_values_are_distinguishable() {
        assert_eq!(run_js("if (true) { return 1; } else { return 2; }"), "1");
        assert_eq!(run_js("if (false) { return 1; } else { return 2; }"), "2");
    }

    /// `jade-cfg-opt::optimize_tfunc`'s `simplify_conditions` pass folds a `CondJmp` whose
    /// condition is a known boolean literal into a plain `Jmp`, dropping the untaken
    /// branch entirely — so the untaken branch's own literal should never even be
    /// discovered as reachable bytecode, proving the optimization pass is actually wired
    /// in (not just a no-op round-trip).
    #[test]
    fn constant_condition_is_folded_away() {
        let js = jit_js("if (true) { return 111; } else { return 222; }");
        assert!(js.contains("111"), "got:\n{js}");
        assert!(!js.contains("222"), "expected the untaken branch to be folded away, got:\n{js}");

        let js = jit_js("if (false) { return 111; } else { return 222; }");
        assert!(!js.contains("111"), "expected the untaken branch to be folded away, got:\n{js}");
        assert!(js.contains("222"), "got:\n{js}");
    }
}

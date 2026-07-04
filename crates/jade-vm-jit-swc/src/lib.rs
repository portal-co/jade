//! Tier 2 (see `docs/bytecode-cfg-plan.md`): reconstruct Jade bytecode as a real
//! `swc-cfg` `Func` (each op's straight-line ops become real `swc_ecma_ast::Stmt`s), run
//! it through the shared `jade-cfg-opt` canonicalizer/constant-folder, then let
//! `swc-cfg`'s own `Cfg::process_block` (patched — see below) + `ssa-reloop2` produce
//! genuinely "proper" structured JS (`while`/`if`/`switch`/`return`, not a block-dispatch
//! loop), serialized via `swc_ecma_codegen`. This is the full-`jsaw-core`-dependency tier;
//! Tier 0 (always on) and Tier 1 (`reloop` feature) in `crates/jade-vm-jit` are the
//! progressively lighter-weight fallbacks this tier is optional on top of.
//!
//! Each discovered block's straight-line ops are turned into statements by generating
//! their JS text via `jade-vm-jit`'s own `ops_to_js` (the exact same per-op emission Tier
//! 0/1 use) and re-parsing that text with `swc_ecma_parser` — reusing already-tested
//! per-op logic instead of hand-building every `Operation` variant's AST shape a second
//! time. Terminator operands (`Ret`'s value, `CondJmp`'s condition, `Switch`'s
//! discriminant/case values) are built directly as trivial `state[i]`/literal `Expr`s, no
//! parsing needed for those.
//!
//! **Relooper bugs found and fixed directly** (per the explicit go-ahead to patch
//! `swc-cfg`/`ssa-reloop2` if a real bug surfaced): `swc-cfg`'s own relooper is literally
//! `ssa_reloop2::go()`, the same Stackifier `crates/jade-vm-jit`'s Tier 1 uses. Three
//! separate correctness bugs surfaced while getting this tier's tests to actually execute
//! correctly (not just parse):
//! 1. `Cfg::process_block`'s `Simple` case didn't self-gate its own content, so
//!    `ssa-reloop2`'s reconverge heuristic could chain an unrelated block into a shared
//!    tail purely from postorder-traversal coincidence. Fixed the same way as Tier 1: every
//!    block's own content gated behind `if (cff === "<label>")`, seeded via a `cff` local
//!    in `From<Func> for Function`.
//! 2. `ssa-reloop2::build`'s loop body/rest split, and `partition_branches`'s per-branch
//!    region, both used a *contiguous rpo-position range* to decide membership — unsound
//!    whenever postorder interleaves unrelated blocks between a loop's body and its own
//!    latch (or between two branches' respective descendants), silently dropping/misplacing
//!    blocks that dominance says truly belong there. Fixed by partitioning by actual
//!    dominance-based ownership instead of position (see `codegen-utils/crates/ssa-reloop2`).
//! 3. A block that's a *direct* successor of one branch but *also* reachable from another
//!    (a genuine shared reconverge point, e.g. code after a loop that both "skip the loop"
//!    and "the loop's own exit" flow into) was still treated as exclusively owned by
//!    whichever branch named it directly (dominance is trivially reflexive), turning it
//!    into its own `switch` case sitting next to the loop's case — but a JS `switch` never
//!    re-dispatches after committing to a case, so transitioning to that case's `cff` value
//!    from *inside* the loop's own case never reaches it. Fixed by additionally checking,
//!    for a block that IS one of the branch targets, whether every other predecessor it has
//!    is internal to its own dominated subtree; if not, it's a shared reconverge and goes
//!    to the tail instead of becoming its own case.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use portal_jsc_swc_cfg::{Block as CBlock, BlockId, Catch, Cfg, End, Func, Term};
use portal_jsc_swc_tac::TFunc;
use portal_solutions_jade_vm::{Operand, Operation};
use portal_solutions_jade_vm_jit::{Config, VecRegistry, discover_blocks, ops_to_js};
use swc_common::sync::Lrc;
use swc_common::{DUMMY_SP, FileName, SourceMap};
use swc_ecma_ast::{
    ComputedPropName, Expr, Ident, Lit, MemberExpr, MemberProp, Number, Script, Stmt,
};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, StringInput, Syntax};

/// Compile Jade bytecode into JavaScript via the Tier 2 swc-cfg-backed pipeline. The
/// returned text is a bare statement list (reads/writes a `state` object; ends in
/// `return`), matching the same contract as `portal_solutions_jade_vm_jit::compile`'s body
/// output — the caller wraps it in a `function(tenant, nt, ...args){ ... }` shell exactly
/// as Tier 0/1 do. `cfg.add_async`/`cfg.add_gen` upgrade the compiled function's own
/// declared capability exactly as they do for Tier 0/1 (jade bytecode produced by
/// `jade-vm-frontend` never contains the `FN` opcode yet, so there are no nested functions
/// to propagate them into here); `cfg.tenant_methods` inlines `GET`/`SET` the same way too,
/// via the shared `ops_to_js` per-op emission.
pub fn compile(code: &[u8], cfg: Config) -> Result<String, String> {
    swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let cfg_func = build_cfg_func(code, 0, &cfg)?;
        let tfunc =
            TFunc::try_from(&cfg_func).map_err(|e| format!("jit-swc: Func -> TFunc: {e:?}"))?;
        let tfunc = portal_solutions_jade_cfg_opt::optimize_tfunc(&tfunc)
            .map_err(|e| format!("jit-swc: jade-cfg-opt: {e:?}"))?;
        let function: swc_ecma_ast::Function = (&tfunc)
            .try_into()
            .map_err(|e: portal_jsc_swc_tac::Error| format!("jit-swc: TFunc -> Function: {e:?}"))?;
        let stmts = function.body.map(|b| b.stmts).unwrap_or_default();
        validate_labels(&stmts)?;
        codegen_stmts(&stmts)
    })
}

/// Defensive backstop: reject output where a labeled `break`/`continue` doesn't lexically
/// nest inside its target `LabeledStmt`. This exact shape was a real, reproducible
/// `ssa-reloop2` bug (see the module doc comment) that's now fixed at the source; this
/// check stays as a never-silent-miscompile guard against any *other* shape the fix
/// doesn't happen to cover, so the caller can fall back to Tier 0/1 instead of receiving
/// broken JS.
fn validate_labels(stmts: &[Stmt]) -> Result<(), String> {
    let mut labels: Vec<swc_atoms::Atom> = Vec::new();
    walk_stmts(stmts, &mut labels)
}

fn walk_stmts(stmts: &[Stmt], labels: &mut Vec<swc_atoms::Atom>) -> Result<(), String> {
    for stmt in stmts {
        walk_stmt(stmt, labels)?;
    }
    Ok(())
}

fn walk_stmt(stmt: &Stmt, labels: &mut Vec<swc_atoms::Atom>) -> Result<(), String> {
    match stmt {
        Stmt::Break(b) => check_label(b.label.as_ref(), labels, "break"),
        Stmt::Continue(c) => check_label(c.label.as_ref(), labels, "continue"),
        Stmt::Block(b) => walk_stmts(&b.stmts, labels),
        Stmt::If(i) => {
            walk_stmt(&i.cons, labels)?;
            if let Some(alt) = &i.alt {
                walk_stmt(alt, labels)?;
            }
            Ok(())
        }
        Stmt::Labeled(l) => {
            labels.push(l.label.sym.clone());
            let result = walk_stmt(&l.body, labels);
            labels.pop();
            result
        }
        Stmt::For(f) => walk_stmt(&f.body, labels),
        Stmt::While(w) => walk_stmt(&w.body, labels),
        Stmt::DoWhile(d) => walk_stmt(&d.body, labels),
        Stmt::Switch(s) => {
            for case in &s.cases {
                walk_stmts(&case.cons, labels)?;
            }
            Ok(())
        }
        Stmt::Try(t) => {
            walk_stmts(&t.block.stmts, labels)?;
            if let Some(h) = &t.handler {
                walk_stmts(&h.body.stmts, labels)?;
            }
            if let Some(f) = &t.finalizer {
                walk_stmts(&f.stmts, labels)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_label(label: Option<&Ident>, labels: &[swc_atoms::Atom], kind: &str) -> Result<(), String> {
    if let Some(id) = label
        && !labels.contains(&id.sym)
    {
        return Err(format!(
            "jit-swc: {kind} references label {:?} outside its lexical scope \
             (ssa-reloop2 reconverge-heuristic placement bug; fall back to Tier 0/1 for this input)",
            id.sym
        ));
    }
    Ok(())
}

/// Reconstruct a `swc_cfg::Func` from Jade bytecode: reuses Tier 0's block discovery,
/// then turns each block's straight-line ops into real `Stmt`s (see module docs) and its
/// terminator into a real `Term`.
fn build_cfg_func(code: &[u8], start_ip: usize, cfg: &Config) -> Result<Func, String> {
    let blocks = discover_blocks(code, start_ip)?;
    let mut cfg_out = Cfg::default();
    let mut offset_to_id: BTreeMap<usize, BlockId> = BTreeMap::new();
    for &offset in blocks.keys() {
        offset_to_id.insert(offset, cfg_out.blocks.alloc(CBlock::default()));
    }
    let entry = *offset_to_id
        .get(&start_ip)
        .ok_or("jit-swc: entry offset was not among discovered blocks (internal invariant)")?;

    // Jade bytecode produced by `jade-vm-frontend` never contains the `FN` opcode yet
    // (nested closures are unsupported there), so a placeholder registry that's never
    // actually invoked is sufficient here.
    let reg = RefCell::new(VecRegistry::new());
    for (offset, block) in &blocks {
        let id = offset_to_id[offset];
        let stmts = ops_to_stmts(&reg, cfg, &block.ops, code)?;
        let term = lower_terminator(&block.term, &offset_to_id)?;
        cfg_out.blocks[id] = CBlock { stmts, end: End { catch: Catch::Throw, term, orig_span: None } };
    }
    Ok(Func { cfg: cfg_out, entry, params: vec![], is_generator: cfg.add_gen, is_async: cfg.add_async })
}

/// Emit `ops`' JS text (via `jade-vm-jit`'s own per-op emission) and re-parse it into real
/// `Stmt`s.
fn ops_to_stmts(
    reg: &RefCell<VecRegistry>,
    cfg: &Config,
    ops: &[Operation],
    code: &[u8],
) -> Result<Vec<Stmt>, String> {
    let js = ops_to_js(reg, cfg.clone(), cfg.add_gen, cfg.add_async, ops, code)?;
    parse_block_js(&js, cfg.add_gen, cfg.add_async)
}

/// Parse `src` (one block's straight-line statements) back into real `Stmt`s. `yield`/
/// `await` are only syntactically legal inside a generator/async function body, so when
/// `is_gen`/`is_async` is set, `src` is wrapped in a matching synthetic function
/// expression before parsing and the wrapper's body is unwrapped afterward — parsing it
/// as a bare top-level script (as a plain, non-generator/async block would be) would
/// otherwise reject those keywords as a syntax error.
fn parse_block_js(src: &str, is_gen: bool, is_async: bool) -> Result<Vec<Stmt>, String> {
    if !is_gen && !is_async {
        return parse_script(src).map_err(|e| format!("jit-swc: failed to parse generated per-block JS: {e}"));
    }
    let keyword = match (is_async, is_gen) {
        (true, true) => "async function*",
        (true, false) => "async function",
        (false, true) => "function*",
        (false, false) => unreachable!(),
    };
    let wrapped = format!("{keyword} __jit_swc_block(){{\n{src}\n}}");
    let mut stmts = parse_script(&wrapped)
        .map_err(|e| format!("jit-swc: failed to parse generated per-block JS: {e}"))?;
    let Some(Stmt::Decl(swc_ecma_ast::Decl::Fn(fn_decl))) = stmts.pop() else {
        return Err("jit-swc: internal invariant: wrapped block JS did not parse to a single function declaration".to_string());
    };
    Ok(fn_decl.function.body.map(|b| b.stmts).unwrap_or_default())
}

fn parse_script(src: &str) -> Result<Vec<Stmt>, String> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Custom("jit-swc-block.js".into())), src.to_string());
    let lexer = Lexer::new(Syntax::Es(Default::default()), Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let script = parser.parse_script().map_err(|e| format!("{e:?}"))?;
    Ok(script.body)
}

/// `state[idx]`.
fn state_member(idx: u32) -> Expr {
    Expr::Member(MemberExpr {
        span: DUMMY_SP,
        obj: Box::new(Expr::Ident(Ident::new("state".into(), DUMMY_SP, Default::default()))),
        prop: MemberProp::Computed(ComputedPropName {
            span: DUMMY_SP,
            expr: Box::new(num_lit(idx as f64)),
        }),
    })
}

fn num_lit(value: f64) -> Expr {
    Expr::Lit(Lit::Num(Number { span: DUMMY_SP, value, raw: None }))
}

fn operand_expr(op: Operand) -> Expr {
    match op {
        Operand::Literal(v) => num_lit(v as f64),
        Operand::StateRef(idx) => state_member(idx),
    }
}

fn lower_terminator(op: &Operation, offset_to_id: &BTreeMap<usize, BlockId>) -> Result<Term, String> {
    let target = |offset: u32| -> Result<BlockId, String> {
        offset_to_id
            .get(&(offset as usize))
            .copied()
            .ok_or_else(|| format!("jit-swc: jump target {offset} is not a discovered block start (internal invariant)"))
    };
    Ok(match op {
        Operation::Ret(val) => Term::Return(Some(operand_expr(*val))),
        Operation::Jmp { target: t } => Term::Jmp(target(*t)?),
        Operation::CondJmp { cond, if_true, if_false } => Term::CondJmp {
            cond: operand_expr(*cond),
            if_true: target(*if_true)?,
            if_false: target(*if_false)?,
        },
        Operation::Switch { val, cases, default_target } => {
            let mut m: HashMap<Expr, BlockId> = HashMap::with_capacity(cases.len());
            for (case_val, case_target) in cases {
                m.insert(num_lit(*case_val as f64), target(*case_target)?);
            }
            Term::Switch { x: operand_expr(*val), blocks: m, default: target(*default_target)? }
        }
        _ => return Err("jit-swc: block did not end in a terminator (internal invariant)".to_string()),
    })
}

fn codegen_stmts(stmts: &[Stmt]) -> Result<String, String> {
    use swc_ecma_codegen::text_writer::JsWriter;
    use swc_ecma_codegen::{Config as CgConfig, Emitter};
    let cm: Lrc<SourceMap> = Default::default();
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut emitter = Emitter {
            cfg: CgConfig::default(),
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm.clone(), "\n", &mut buf, None),
        };
        emitter
            .emit_script(&Script { span: DUMMY_SP, body: stmts.to_vec(), shebang: None })
            .map_err(|e| format!("jit-swc: codegen: {e}"))?;
    }
    String::from_utf8(buf).map_err(|e| format!("jit-swc: codegen produced invalid utf8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use portal_solutions_jade_vm::Operand;

    fn chunk(ops: &[Operation]) -> Vec<u8> {
        ops.iter().flat_map(|o| o.emit()).collect()
    }

    fn op_len(op: &Operation) -> u32 {
        op.emit().count() as u32
    }

    /// Actually *run* the compiled JS via Node, same pattern used by Tier 0/1's tests.
    fn run_js(body: &str) -> String {
        let script = format!(
            "const fn = new Function('tenant','nt','state', {:?}); console.log(JSON.stringify(fn(undefined,undefined,[])));",
            body
        );
        let output = std::process::Command::new("node").arg("-e").arg(&script).output().expect("node failed");
        assert!(output.status.success(), "node stderr: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn compiles_and_executes_return_literal() {
        let code = chunk(&[Operation::Ret(Operand::Literal(42))]);
        let js = compile(&code, Config::default()).unwrap();
        assert_eq!(run_js(&js), "42", "js:\n{js}");
    }

    #[test]
    fn condjmp_emits_native_if_and_executes_correctly() {
        let then_body = chunk(&[Operation::Lit32 { dest: 1, val: 10 }]);
        let else_body = chunk(&[Operation::Lit32 { dest: 1, val: 20 }]);
        let ret_body = chunk(&[Operation::Ret(Operand::StateRef(1))]);
        let condjmp_len = op_len(&Operation::CondJmp { cond: Operand::StateRef(0), if_true: 0, if_false: 0 });
        let jmp_len = op_len(&Operation::Jmp { target: 0 });
        let then_offset = condjmp_len;
        let jmp_offset = then_offset + then_body.len() as u32;
        let else_offset = jmp_offset + jmp_len;
        let ret_offset = else_offset + else_body.len() as u32;

        for cond_val in [true, false] {
            let mut code = chunk(&[Operation::Bool { val: cond_val, dest: 0 }]);
            code.extend(
                Operation::CondJmp {
                    cond: Operand::StateRef(0),
                    if_true: then_offset + 10,
                    if_false: else_offset + 10,
                }
                .emit(),
            );
            code.extend(then_body.clone());
            code.extend(Operation::Jmp { target: ret_offset + 10 }.emit());
            code.extend(else_body.clone());
            code.extend(ret_body.clone());

            let js = compile(&code, Config::default()).unwrap();
            assert!(js.contains("if"), "got:\n{js}");

            let result = run_js(&js);
            let expected = if cond_val { "10" } else { "20" };
            assert_eq!(result, expected, "cond={cond_val}, js:\n{js}");
        }
    }

    #[test]
    fn loop_executes_correctly() {
        let ret_body = chunk(&[Operation::Ret(Operand::StateRef(0))]);
        let body_ops = chunk(&[Operation::Bool { val: false, dest: 0 }]);
        let header_len = op_len(&Operation::CondJmp { cond: Operand::StateRef(0), if_true: 0, if_false: 0 });
        let jmp_len = op_len(&Operation::Jmp { target: 0 });

        let mut code = chunk(&[Operation::Bool { val: true, dest: 0 }]);
        let header_offset = code.len() as u32;
        let body_offset = header_offset + header_len;
        let exit_offset = body_offset + body_ops.len() as u32 + jmp_len;
        code.extend(
            Operation::CondJmp { cond: Operand::StateRef(0), if_true: body_offset, if_false: exit_offset }.emit(),
        );
        code.extend(body_ops);
        code.extend(Operation::Jmp { target: header_offset }.emit());
        code.extend(ret_body);

        let js = compile(&code, Config::default()).unwrap();
        let result = run_js(&js);
        assert_eq!(result, "false", "js:\n{js}");
    }

    /// The correctness case that motivated the whole jump-based bytecode redesign, and
    /// that found the reconverge-heuristic double-emission bug in `ssa-reloop2`/`swc-cfg`
    /// (see the module doc comment and `crates/jade-vm-jit/src/reloop.rs`): a `return`
    /// reached from inside a conditional nested in a loop body must actually execute.
    ///
    /// `var x = true; while (x) { if (true) { return 7; } else { x = false; } } return 99;`
    #[test]
    fn return_inside_loop_body_executes_correctly() {
        let entry_ops = chunk(&[Operation::Bool { val: true, dest: 0 }]);
        let header_len = op_len(&Operation::CondJmp { cond: Operand::StateRef(0), if_true: 0, if_false: 0 });
        let body_ops = chunk(&[Operation::Bool { val: true, dest: 1 }]);
        let body_condjmp_len = op_len(&Operation::CondJmp { cond: Operand::StateRef(1), if_true: 0, if_false: 0 });
        let ret7 = chunk(&[Operation::Ret(Operand::Literal(7))]);
        let else_ops = chunk(&[Operation::Bool { val: false, dest: 0 }]);
        let jmp_len = op_len(&Operation::Jmp { target: 0 });
        let exit_ops = chunk(&[Operation::Ret(Operand::Literal(99))]);

        let header_offset = entry_ops.len() as u32;
        let body_offset = header_offset + header_len;
        let retblk_offset = body_offset + body_ops.len() as u32 + body_condjmp_len;
        let elseblk_offset = retblk_offset + ret7.len() as u32;
        let exit_offset = elseblk_offset + else_ops.len() as u32 + jmp_len;

        let mut code = entry_ops;
        code.extend(
            Operation::CondJmp { cond: Operand::StateRef(0), if_true: body_offset, if_false: exit_offset }.emit(),
        );
        code.extend(body_ops);
        code.extend(
            Operation::CondJmp { cond: Operand::StateRef(1), if_true: retblk_offset, if_false: elseblk_offset }
                .emit(),
        );
        code.extend(ret7);
        code.extend(else_ops);
        code.extend(Operation::Jmp { target: header_offset }.emit());
        code.extend(exit_ops);

        let js = compile(&code, Config::default()).unwrap();
        let result = run_js(&js);
        assert_eq!(result, "7", "js:\n{js}");
    }

    /// `Config.add_gen` must reach all the way through the `Func -> TFunc -> jade-cfg-opt
    /// -> Func -> Function` round trip: a `YIELD` op is only valid in a generator, and
    /// without `add_gen` set the underlying `ops_to_js` call rejects it outright.
    #[test]
    fn add_gen_config_reaches_yield_ops() {
        let code = chunk(&[
            Operation::Yield { val: Operand::Literal(5), dest: 0 },
            Operation::Ret(Operand::StateRef(0)),
        ]);

        let err = compile(&code, Config::default()).unwrap_err();
        assert!(err.contains("YIELD"), "expected a YIELD-related error, got: {err}");

        let cfg = Config { add_gen: true, ..Config::default() };
        let js = compile(&code, cfg).unwrap();
        assert!(js.contains("yield"), "got:\n{js}");
    }
}

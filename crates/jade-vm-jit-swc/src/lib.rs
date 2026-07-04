//! Tier 2 (see `docs/bytecode-cfg-plan.md`): reconstruct Jade bytecode as a real
//! `swc-cfg` `Func` (each op's straight-line ops become real `swc_ecma_ast::Stmt`s), run
//! it through the shared `jade-cfg-opt` canonicalizer/constant-folder, then let
//! `swc-cfg`'s own `Cfg::process_block` (patched — see below) + `ssa-reloop2` produce
//! genuinely "proper" structured JS (`while`/`if`/`switch`/`return`, not a block-dispatch
//! loop), serialized via `swc_ecma_codegen`. This is the full-`jsaw-core`-dependency tier;
//! Tier 0 (always on) and Tier 1 (`reloop` feature) in `crates/jade-vm-jit` are the
//! progressively lighter-weight fallbacks this tier is optional on top of.
//!
//! "Proper" here means **optimizer-friendly**, not necessarily human-readable: the point
//! of this tier is output a downstream JS engine's own optimizer/JIT can chew on
//! effectively (real structured control flow, and IIFE-free tenant calls once inlining —
//! see `docs/pluggable-tenant-interface-plan.md` — lands), not prose-quality source.
//!
//! A nested closure (a real `FN` opcode, as `jade-vm-frontend` can now emit) is compiled
//! through *this same tier*, recursively — see `Config::nested_body_compiler`, set in
//! `compile()` — rather than silently downgrading to Tier 0's block-dispatch loop.
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
use std::rc::Rc;

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
/// declared capability exactly as they do for Tier 0/1. The returned [`VecRegistry`] holds
/// any nested functions discovered via a real `FN` opcode (see [`Config::nested_body_compiler`],
/// set here so a nested closure is itself reconstructed as a real CFG `Func` through this
/// same tier, not silently downgraded) — read its `prelude()` for the `const __fnN = ...`
/// declarations to prepend alongside the returned body, exactly as Tier 0/1's own
/// `VecRegistry` output is used. `cfg.tenant_methods` inlines `GET`/`SET` the same way too,
/// via the shared `ops_to_js` per-op emission.
pub fn compile(code: &[u8], cfg: Config) -> Result<(String, VecRegistry), String> {
    swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let reg = Rc::new(RefCell::new(VecRegistry::new()));
        let is_gen = cfg.add_gen;
        let is_async = cfg.add_async;
        let cfg = with_nested_body_compiler(cfg, reg.clone());
        let body = compile_body(code, 0, &cfg, is_gen, is_async, false, &reg)?;
        // `cfg`'s own `nested_body_compiler` closure holds its own clone of `reg` — drop
        // it before `try_unwrap`, or the refcount never drops back to 1.
        drop(cfg);
        let reg = Rc::try_unwrap(reg)
            .map_err(|_| "jit-swc: internal invariant: FnRegistry Rc had lingering clones after compile finished".to_string())?
            .into_inner();
        Ok((body, reg))
    })
}

/// Return a clone of `cfg` with `nested_body_compiler` set to recurse into this tier's own
/// `compile_body` pipeline (sharing `reg` across every level of nesting, so all registered
/// functions land in one combined registry/prelude) instead of `op_fn`'s Tier 0/1 fallback.
///
/// Unlike Tier 0/1's raw per-op emission, Tier 2's `TFunc`/SSA round-trip *hoists a `var`
/// declaration for every referenced free identifier it sees* — including `state` itself
/// (and `tenant`, `nt`, ...), not just Jade's own `v{n}` temporaries. `op_fn`'s normal
/// nested-body wrapping (`const state = Object.create(null);\n{stmts}`) would collide with
/// that hoisted `var state;` (mixing `const`/`var` for the same name in the same scope is
/// a hard `SyntaxError`, unlike `var`/`var` or a parameter/`var` pair, both of which are
/// harmless redeclarations) — so this closure's own output initializes the *already
/// var-hoisted* `state` via a plain assignment instead, and `op_fn` skips its usual const
/// prefix whenever `nested_body_compiler` is in use (see `op_fn`'s doc comment).
fn with_nested_body_compiler(mut cfg: Config, reg: Rc<RefCell<VecRegistry>>) -> Config {
    let base_cfg = cfg.clone();
    cfg.nested_body_compiler = Some(Rc::new(move |code: &[u8], start_ip: usize, is_gen: bool, is_async: bool, double_gen: bool| {
        let inner_cfg = with_nested_body_compiler(base_cfg.clone(), reg.clone());
        let stmts = compile_body(code, start_ip, &inner_cfg, is_gen, is_async, double_gen, &reg)?;
        Ok(format!("state = Object.create(null);\n{stmts}"))
    }));
    cfg
}

/// The reusable core of the Tier 2 pipeline: reconstruct a real CFG `Func` for the
/// function at `code[start_ip..]`, run it through `jade-cfg-opt`, convert to a real
/// `swc_ecma_ast::Function`, and codegen its body. Used for both the top-level program
/// (`compile`) and, via `Config::nested_body_compiler`, every nested closure — `is_gen`/
/// `is_async`/`double_gen` are that function's own already-computed *effective* variant
/// flags (see `Config::nested_body_compiler`'s doc comment).
fn compile_body(
    code: &[u8],
    start_ip: usize,
    cfg: &Config,
    is_gen: bool,
    is_async: bool,
    double_gen: bool,
    reg: &Rc<RefCell<VecRegistry>>,
) -> Result<String, String> {
    let cfg_func = build_cfg_func(code, start_ip, cfg, is_gen, is_async, double_gen, reg)?;
    let tfunc =
        TFunc::try_from(&cfg_func).map_err(|e| format!("jit-swc: Func -> TFunc: {e:?}"))?;
    let tfunc = portal_solutions_jade_cfg_opt::optimize_tfunc(&tfunc)
        .map_err(|e| format!("jit-swc: jade-cfg-opt: {e:?}"))?;
    let function: swc_ecma_ast::Function = (&tfunc)
        .try_into()
        .map_err(|e: portal_jsc_swc_tac::Error| format!("jit-swc: TFunc -> Function: {e:?}"))?;
    let stmts = function.body.map(|b| b.stmts).unwrap_or_default();
    validate_labels(&stmts)?;
    Ok(strip_free_identifier_hoists(&codegen_stmts(&stmts)?))
}

/// Defensive workaround for a real bug in the `TFunc`/SSA round-trip (`portal-jsc-swc-tac`/
/// `-ssa`, vendored — patching those is out of scope here): converting the *re-parsed* AST
/// of Jade's own per-op-emitted text back through `TFunc -> Function` hoists a bare
/// `var <name>;` declaration for **every** distinct identifier the reconstructed body
/// references — not just Jade's own genuine `v{n}`/`$v{n}`/`$k{n}p{n}`/`cff` temporaries,
/// but any free/external identifier too (`state`, `tenant`, `nt`, a nested function's own
/// registered name, and even true JS globals like `Reflect`/`Symbol`). Since the hoisted
/// `var` is left uninitialized, it *shadows* the real outer binding with `undefined` for
/// the rest of the function — turning e.g. every `Reflect.apply(...)` call (emitted by
/// every `CALL` op) into a `TypeError` the moment the reconstructed function actually runs.
/// Strips any `var <name>;` hoist whose name doesn't match Jade's own internal temporary
/// naming convention, leaving genuine Jade-internal hoists (needed across the `cff`-gated
/// blocks this tier's control-flow reconstruction produces) untouched.
fn strip_free_identifier_hoists(js: &str) -> String {
    js.lines()
        .filter(|line| {
            let trimmed = line.trim();
            let Some(name) = trimmed.strip_prefix("var ").and_then(|s| s.strip_suffix(';')) else {
                return true;
            };
            // Only ever drop a single, bare, uninitialized hoist (`var name;`) — anything
            // else (a comma list, an initializer) isn't this specific hoist shape.
            if name.is_empty() || name.contains([',', ' ', '=']) {
                return true;
            }
            is_jade_internal_temp_name(name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `name` matches Jade's own internal temporary-naming convention: `cff` (the
/// control-flow-flag local), `v{n}`/`$v{n}` (Jade value slots), or `$k{n}p{n}` (join-point
/// phi temporaries) — see the JIT tiers' own emission (`jade-vm-jit`'s `JsJit::fresh`) and
/// `ssa-reloop2`'s phi-node naming.
fn is_jade_internal_temp_name(name: &str) -> bool {
    if name == "cff" {
        return true;
    }
    let name = name.strip_prefix('$').unwrap_or(name);
    if let Some(rest) = name.strip_prefix('v') {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
    }
    if let Some(rest) = name.strip_prefix('k')
        && let Some(p_pos) = rest.find('p')
    {
        let (n1, n2) = (&rest[..p_pos], &rest[p_pos + 1..]);
        return !n1.is_empty()
            && !n2.is_empty()
            && n1.bytes().all(|b| b.is_ascii_digit())
            && n2.bytes().all(|b| b.is_ascii_digit());
    }
    false
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
/// terminator into a real `Term`. `is_gen`/`is_async`/`double_gen` are this function's own
/// already-computed *effective* variant flags (see `compile_body`) — used both for the
/// returned `Func`'s own `is_generator`/`is_async` and for per-block emission (in place of
/// reading `cfg.add_gen`/`cfg.add_async` directly, which are the *ambient*, session-wide
/// flags — correct for the top-level call, where there's no other "declared" bit, but not
/// for a nested function, which has its own declared variant to combine with them).
fn build_cfg_func(
    code: &[u8],
    start_ip: usize,
    cfg: &Config,
    is_gen: bool,
    is_async: bool,
    double_gen: bool,
    reg: &Rc<RefCell<VecRegistry>>,
) -> Result<Func, String> {
    let blocks = discover_blocks(code, start_ip)?;
    let mut cfg_out = Cfg::default();
    let mut offset_to_id: BTreeMap<usize, BlockId> = BTreeMap::new();
    for &offset in blocks.keys() {
        offset_to_id.insert(offset, cfg_out.blocks.alloc(CBlock::default()));
    }
    let entry = *offset_to_id
        .get(&start_ip)
        .ok_or("jit-swc: entry offset was not among discovered blocks (internal invariant)")?;

    for (offset, block) in &blocks {
        let id = offset_to_id[offset];
        let stmts = ops_to_stmts(reg, cfg, is_gen, is_async, double_gen, &block.ops, code)?;
        let term = lower_terminator(&block.term, &offset_to_id)?;
        cfg_out.blocks[id] = CBlock { stmts, end: End { catch: Catch::Throw, term, orig_span: None } };
    }
    Ok(Func { cfg: cfg_out, entry, params: vec![], is_generator: is_gen, is_async })
}

/// Emit `ops`' JS text (via `jade-vm-jit`'s own per-op emission) and re-parse it into real
/// `Stmt`s.
fn ops_to_stmts(
    reg: &Rc<RefCell<VecRegistry>>,
    cfg: &Config,
    is_gen: bool,
    is_async: bool,
    double_gen: bool,
    ops: &[Operation],
    code: &[u8],
) -> Result<Vec<Stmt>, String> {
    let js = ops_to_js(reg.clone(), cfg.clone(), is_gen, is_async, double_gen, ops, code)?;
    parse_block_js(&js, is_gen, is_async)
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
        let (js, _reg) = compile(&code, Config::default()).unwrap();
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

            let (js, _reg) = compile(&code, Config::default()).unwrap();
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

        let (js, _reg) = compile(&code, Config::default()).unwrap();
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

        let (js, _reg) = compile(&code, Config::default()).unwrap();
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
        let (js, _reg) = compile(&code, cfg).unwrap();
        assert!(js.contains("yield"), "got:\n{js}");
    }

    /// Drive a compiled `async function` body to completion via Node (an async IIFE
    /// `await`ing the call, `.catch` failing the process on rejection) — actually proving
    /// the `await` suspends/resumes correctly, not just that the keyword parses.
    fn run_js_async(body: &str) -> String {
        let script = format!(
            // `new Function` can only ever construct a plain (non-async) function,
            // regardless of body content — the `AsyncFunction` constructor is not a
            // global, but is reachable via any async function's own prototype chain.
            "const AsyncFunction = Object.getPrototypeOf(async function(){{}}).constructor;\n\
             const fn = new AsyncFunction('tenant','nt','state', {body:?});\n\
             (async () => {{ console.log(JSON.stringify(await fn(undefined,undefined,[]))); }})()\n\
             .catch(e => {{ console.error(e); process.exit(1); }});"
        );
        let output = std::process::Command::new("node").arg("-e").arg(&script).output().expect("node failed");
        assert!(output.status.success(), "node stderr: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// Run a compiled top-level program (whose own `op_call` wraps a generator-shaped
    /// result via `createGuestGen` under ambient `add_gen`) via Node, unwrap the returned
    /// guest-gen object with `unpackGuestGen`, and collect every yielded value.
    /// `createGuestGen`/`unpackGuestGen` stubs (matching `packages/jade-js/shims.ts`'s
    /// contract closely enough for doubleGen driving) are provided so the harness is
    /// self-contained, not dependent on the real TS shims. `prelude` (any nested-function
    /// declarations from `VecRegistry::prelude()`) is placed in the *outer* Node script
    /// scope, not concatenated inside `body` — Tier 2's `TFunc`/SSA round-trip hoists a
    /// `var` for every free identifier a reconstructed function references (including a
    /// nested function's own registered name, `state`, etc.), which would collide with a
    /// `const`/`let` of the same name in the very same scope (unlike the harmless
    /// `var`/`var` or parameter/`var` pairs this never conflicts with) if `prelude` and
    /// `body` shared one function scope.
    fn run_js_gen_values(prelude: &str, body: &str) -> String {
        let shims = r#"
            const THROUGH = Symbol.for("jade.through");
            const GUEST_NEXT = Symbol.for("jade.guest.next");
            function markGuestFn(f) { return f; }
            function createGuestGen(nativeGen) {
                const obj = {};
                const nextFn = function* (sent) {
                    let step = nativeGen.next(sent);
                    while (!step.done && step.value && step.value[THROUGH] !== undefined) {
                        sent = yield step.value;
                        step = nativeGen.next(sent);
                    }
                    return step;
                };
                obj.next = nextFn;
                obj.return = function* (v) { return nativeGen.return ? nativeGen.return(v) : { value: v, done: true }; };
                obj.throw = function* (e) { if (nativeGen.throw) return nativeGen.throw(e); throw e; };
                obj[GUEST_NEXT] = nextFn;
                return obj;
            }
            function* unpackGuestGen(g) {
                const nextFn = g[GUEST_NEXT];
                if (typeof nextFn !== "function") return yield* g;
                let sent;
                while (true) {
                    const result = yield* nextFn.call(g, sent);
                    if (result.done) return result.value;
                    sent = yield result.value;
                }
            }
        "#;
        let script = format!(
            "{shims}\n{prelude}\nconst make = new Function('tenant','nt','state', {body:?});\n\
             const result = make(undefined, undefined, []);\n\
             const out = [];\n\
             for (const v of unpackGuestGen(result)) out.push(v);\n\
             console.log(JSON.stringify(out));",
        );
        let output = std::process::Command::new("node").arg("-e").arg(&script).output().expect("node failed");
        assert!(output.status.success(), "node stderr: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// Mirrors `add_gen_config_reaches_yield_ops` for `add_async` — and, the actual
    /// hardening over that test, drives the result through Node rather than only checking
    /// for the `await` substring: `await 5` resolves synchronously to `5`, so this proves
    /// real event-loop suspension/resumption round-trips correctly.
    #[test]
    fn add_async_config_reaches_await_ops() {
        let code = chunk(&[
            Operation::Await { val: Operand::Literal(5), dest: 0 },
            Operation::Ret(Operand::StateRef(0)),
        ]);

        let err = compile(&code, Config::default()).unwrap_err();
        assert!(err.contains("AWAIT"), "expected an AWAIT-related error, got: {err}");

        let cfg = Config { add_async: true, ..Config::default() };
        let (js, _reg) = compile(&code, cfg).unwrap();
        assert!(js.contains("await"), "got:\n{js}");
        assert_eq!(run_js_async(&js), "5", "js:\n{js}");
    }

    /// Nested `Fn` op inside Tier 2, compiled through Tier 2 itself (not silently
    /// downgraded to Tier 0's block-dispatch loop): builds bytecode by hand, mirroring
    /// `jade-vm-jit`'s own `program_with_fn` test helper, with a nested function declared
    /// as a generator (`variant = 2`) called from the top level under ambient `add_gen` —
    /// a genuine doubleGen combination (declared-gen nested function running under
    /// ambient `add_gen`), only reachable for a *nested* function since Jade bytecode's
    /// top level has no "declared variant" bit of its own.
    #[test]
    fn nested_gen_fn_runs_via_tier2_under_ambient_add_gen() {
        let fn_body = chunk(&[
            Operation::Yield { val: Operand::Literal(7), dest: 0 },
            Operation::Ret(Operand::StateRef(0)),
        ]);
        let mk = |j: u32| Operation::Fn {
            variant: Operand::Literal(2), // declared sync generator
            closure_args: Operand::Literal(0),
            spanner: Operand::Literal(0),
            j,
            dest: 0,
        };
        let call_op = Operation::Call { fn_op: Operand::StateRef(0), args: vec![], dest: 1 };
        let ret_op = Operation::Ret(Operand::StateRef(1));
        // The nested body must sit *after* the entire top-level block (Fn + Call + Ret),
        // not right after the `Fn` op alone — the `Fn` op is a plain value-producing op,
        // not a jump, so `discover_blocks`' straight-line parse of the top-level's own
        // block would otherwise fall straight through into the nested body's bytes
        // instead of stopping at the top level's own `Ret`.
        let j = op_len(&mk(0)) + op_len(&call_op) + op_len(&ret_op);
        let mut code: Vec<u8> = mk(j).emit().collect();
        code.extend(call_op.emit());
        code.extend(ret_op.emit());
        code.extend_from_slice(&fn_body);

        let cfg = Config { add_gen: true, ..Config::default() };
        let (js, reg) = compile(&code, cfg).unwrap();
        let prelude = reg.prelude();

        assert!(prelude.contains("function*"), "expected the nested function registered as a generator, got:\n{prelude}");
        assert!(
            prelude.contains("markGuestFn(__fn0, {abi: \"leading-tenant-nt\"})"),
            "expected the nested function to register its guest ABI, got:\n{prelude}"
        );
        // The nested body itself has no branch/loop, so there isn't much structure to
        // prove was Tier-2-reconstructed rather than Tier-0-emitted here beyond it having
        // compiled and registered at all (the *real* proof that recursion through Tier 2
        // happened, not a downgrade, is `nested_fn_with_branch_is_reconstructed_by_tier2`
        // below, which has actual control flow in the nested body to tell the two apart).
        assert!(!prelude.contains("__ip"), "did not expect a Tier 0 block-dispatch loop, got:\n{prelude}");

        // Drive it: the outer program's own CALL result gets addGen-wrapped
        // (`createGuestGen`) since ambient `add_gen` is set. The nested function is
        // *itself* declared a generator (`variant = 2`) while ALSO running under ambient
        // `add_gen` — the actual doubleGen combination — so its own `yield` is emitted
        // THROUGH-tagged (`emit_op`'s `Operation::Yield` handling, gated on
        // `jit.double_gen`), which `createGuestGen`'s `nextFn` re-yields *unchanged*
        // rather than unwrapping (that unwrapping is `unpackGuestGen`'s caller's job one
        // level further out — there isn't one here, since this test calls the nested
        // generator directly) — so the value observed through `unpackGuestGen` alone is
        // the THROUGH-tagged wrapper itself (`{value: 7, [THROUGH]: true}`; `JSON.stringify`
        // drops the symbol-keyed `THROUGH` marker, leaving `{"value":7}`). This is the
        // correct, distinguishing behavior of doubleGen vs. a plain (non-doubleGen)
        // generator, which yields the bare value with no wrapper at all.
        assert_eq!(run_js_gen_values(&prelude, &js), r#"[{"value":7}]"#, "prelude:\n{prelude}\njs:\n{js}");
    }

    /// Same as above, but the nested function's own body has real control flow (an
    /// `if`/`else` before the `yield`) — proving the nested body was actually
    /// reconstructed through Tier 2's own CFG/relooper pipeline (real `if`, no `switch
    /// (__ip)` block-dispatch loop) rather than falling back to Tier 0's always-correct
    /// but structurally different emission.
    #[test]
    fn nested_fn_with_branch_is_reconstructed_by_tier2() {
        let mk = |j: u32| Operation::Fn {
            variant: Operand::Literal(2),
            closure_args: Operand::Literal(0),
            spanner: Operand::Literal(0),
            j,
            dest: 0,
        };
        let call_op = Operation::Call { fn_op: Operand::StateRef(0), args: vec![], dest: 1 };
        let ret_op = Operation::Ret(Operand::StateRef(1));
        // The nested body must start *after* the entire top-level block (Fn + Call + Ret):
        // every op length here is independent of any operand's actual value, so `j` (and
        // every offset inside the nested body, all absolute into the one shared buffer —
        // see `jade-vm-frontend`'s `compile_program` doc comment) is knowable upfront.
        // Placing the nested body right after the `Fn` op alone (a plain value-producing
        // op, not a jump) would let the top level's own straight-line block-discovery fall
        // straight through into it instead of stopping at the top level's own `Ret`.
        let j = op_len(&mk(0)) + op_len(&call_op) + op_len(&ret_op);

        let bool_len = op_len(&Operation::Bool { val: true, dest: 0 });
        let cond_len = op_len(&Operation::CondJmp { cond: Operand::StateRef(0), if_true: 0, if_false: 0 });
        let jmp_len = op_len(&Operation::Jmp { target: 0 });
        let then_body = chunk(&[Operation::Yield { val: Operand::Literal(1), dest: 1 }]);
        let else_body = chunk(&[Operation::Yield { val: Operand::Literal(2), dest: 1 }]);
        let ret_body = chunk(&[Operation::Ret(Operand::StateRef(1))]);

        let then_offset = j + bool_len + cond_len;
        let jmp_offset = then_offset + then_body.len() as u32;
        let else_offset = jmp_offset + jmp_len;
        let ret_offset = else_offset + else_body.len() as u32;

        let mut fn_body = chunk(&[Operation::Bool { val: true, dest: 0 }]);
        fn_body.extend(
            Operation::CondJmp { cond: Operand::StateRef(0), if_true: then_offset, if_false: else_offset }.emit(),
        );
        fn_body.extend(then_body);
        fn_body.extend(Operation::Jmp { target: ret_offset }.emit());
        fn_body.extend(else_body);
        fn_body.extend(ret_body);

        let mut code: Vec<u8> = mk(j).emit().collect();
        code.extend(call_op.emit());
        code.extend(ret_op.emit());
        assert_eq!(code.len() as u32, j, "internal test invariant: top-level block length must match the precomputed `j`");
        code.extend_from_slice(&fn_body);

        let (js, reg) = compile(&code, Config::default()).unwrap();
        let prelude = reg.prelude();
        assert!(prelude.contains("if ("), "expected the nested body's branch reconstructed as a real `if`, got:\n{prelude}");
        assert!(!prelude.contains("__ip"), "did not expect a Tier 0 block-dispatch loop, got:\n{prelude}");

        assert_eq!(run_js_gen_values(&prelude, &js), "[1]", "prelude:\n{prelude}\njs:\n{js}");
    }
}

//! Build-time orchestrator for `docs/bundle-pass-build-step-plan.md`: composes
//! `jade-primordial-ir`'s real-AST emission with `jade-swc-tenant-exposure`'s bundle pass
//! entirely in-process, with exactly one text-serialization point (the final print), instead of
//! the parse -> emit-text -> reparse chain a file-handoff design would need.
//!
//! Pipeline per `.ts` file under `packages/jade-js`:
//! 1. Try `jade_primordial_ir::translate_source` + `emit_ast` (real AST, no text in between).
//!    Only the files `lower_module`'s closed grammar subset actually covers succeed here today
//!    (`object.ts`/`function.ts`/`reflect.ts`/`array-buffer.ts`, per
//!    `docs/primordial-ir-plan.md`'s Progress notes) — everything else, expectedly, falls back
//!    to step 2. This is a plain fallback, not a hardcoded path allowlist: whatever the IR can
//!    already handle gets its benefit for free as coverage grows, with no orchestrator change.
//! 2. Direct `swc_ecma_parser::Syntax::Typescript` parse for anything step 1 didn't handle.
//! 3. `jade_swc_tenant_exposure::resolve` (required precondition, see that crate's
//!    `ResolvedProgram`) then `transform_program` against the hardcoded `TenantExposureConfig`
//!    below.
//! 4. `swc_ecma_codegen` print — the one point in the whole pipeline where AST becomes text —
//!    written under `packages/jade-js/.generated/`, mirroring the input's relative path.
//!
//! Selection is hardcoded (per the plan's decision 4), not read from a config file or source
//! annotations: `Tenant` (`tenants/multi.ts`) and `MergedTenant` (`tenants/merged.ts`) are the
//! real `Tenant` implementations in the codebase, the only classes whose methods can actually
//! match `TENANT_METHOD_NAMES`.
//!
//! **Correction from the plan's original "Concrete targets" list**: `BufferPrimordialImpl`
//! (`primordials/array-buffer.ts`) is *not* included here despite also having `#private`
//! fields. Running against real files (see below) showed it isn't a `Tenant` implementation at
//! all — its own methods are `record`/`shell`/`constructor`, never `make`/`get`/`set`/`define`/
//! `assign`/`ownKeys` — so no `tenant_methods` selection could ever match anything on it; it
//! was structurally similar (a `#private`-bearing class) but not an actual target for *this*
//! transform.
//!
//! **Real-world finding that drove a real fix**: running this orchestrator against the actual
//! `Tenant`/`MergedTenant` source first produced zero transformations. Every one of their
//! `TENANT_METHOD_NAMES` methods accesses its private fields *indirectly*, through a private
//! helper method (`Tenant.get`/`.set`/etc. call `#shadowForKey`, not `this.#shadow` directly;
//! `MergedTenant`'s call `#requireOwner`/`#marshal`, not `this.#bridges` directly) — the shape
//! `docs/swc-tenant-accessors-and-exposed-functions-plan.md`'s original "Out of scope for
//! version 1" section named: "Private methods, private accessors... are skipped rather than
//! partially rewritten." `jade-swc-tenant-exposure` now also generates a public mangled
//! *forwarding method* (`[MANGLED](...args) { return this.#helper(...args); }`) for a private
//! instance method a selected public method calls directly, and rewrites that call site to use
//! it — see `PrivateUseAnalyzer::method_call_uses`/`make_method_forwarder` in that crate. Both
//! real classes now transform correctly: confirmed against the actual generated output that
//! `get`/`set`/`ownKeys` end up fully `PrivateName`-free (multiple private fields *and* private
//! method calls widened within a single method), while unselected methods (`has`, `delete`,
//! etc.) stay untouched. Private accessors (`get #x()`/`set #x()`) remain out of scope, and a
//! private method referenced without being called (not a direct `this.#x(...)`) still correctly
//! fails closed — see that crate's own tests.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use portal_solutions_jade_swc_tenant_exposure::{TenantExposureConfig, resolve, transform_program};
use swc_common::sync::Lrc;
use swc_common::{FileName, SourceMap};
use swc_ecma_ast::Program;
use swc_ecma_codegen::text_writer::JsWriter;
use swc_ecma_codegen::{Config as CodegenConfig, Emitter, Node};
use swc_ecma_parser::lexer::Lexer;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax};

const GENERATED_DIR_NAME: &str = ".generated";

fn tenant_exposure_config() -> TenantExposureConfig {
    let mut config = TenantExposureConfig::with_tenant_class("Tenant");
    config.tenant_classes.insert("MergedTenant".to_owned());
    config
}

fn jade_js_dir() -> PathBuf {
    if let Some(arg) = std::env::args().nth(1) {
        return PathBuf::from(arg);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/jade-js")
}

fn main() {
    let jade_js_dir = jade_js_dir().canonicalize().unwrap_or_else(|err| {
        eprintln!("jade-bundle-build: cannot resolve jade-js package dir: {err}");
        std::process::exit(2);
    });
    let generated_dir = jade_js_dir.join(GENERATED_DIR_NAME);

    let files = match collect_ts_files(&jade_js_dir, &generated_dir) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("jade-bundle-build: failed to walk {}: {err}", jade_js_dir.display());
            std::process::exit(2);
        }
    };

    let config = tenant_exposure_config();
    let mut had_error = false;
    let mut transformed_tenants = BTreeSet::new();
    let mut ir_backed = 0usize;
    let mut direct_parsed = 0usize;

    for file in &files {
        let relative = file.strip_prefix(&jade_js_dir).expect("file was found under jade_js_dir");
        match process_file(file, relative, &config) {
            Ok(Outcome {
                output,
                used_ir,
                report,
            }) => {
                if used_ir {
                    ir_backed += 1;
                } else {
                    direct_parsed += 1;
                }
                transformed_tenants.extend(report.transformed_tenants);
                // No silent caps: a skip is a deliberate, load-bearing decision by the pass
                // (`ExposureSkipReason`), not noise — surface every one of them so a class that
                // *should* have qualified but didn't (e.g. a mismatch between this config's
                // `tenant_methods` and the class's real shape) is visible, not silently absent
                // from `transformed_tenants`.
                for (name, reason) in &report.skipped {
                    eprintln!(
                        "jade-bundle-build: {}: skipped {name}: {reason:?}",
                        relative.display()
                    );
                }
                let out_path = generated_dir.join(relative);
                if let Err(err) = write_output(&out_path, &output) {
                    eprintln!(
                        "jade-bundle-build: failed to write {}: {err}",
                        out_path.display()
                    );
                    had_error = true;
                }
            }
            Err(err) => {
                eprintln!("jade-bundle-build: failed on {}: {err}", relative.display());
                had_error = true;
            }
        }
    }

    eprintln!(
        "jade-bundle-build: {} file(s) processed ({ir_backed} via primordial-ir AST, {direct_parsed} via direct parse); tenant classes transformed: {:?}",
        files.len(),
        transformed_tenants
    );

    if had_error {
        std::process::exit(1);
    }
}

fn collect_ts_files(root: &Path, generated_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_ts_files_into(root, generated_dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_ts_files_into(dir: &Path, generated_dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == generated_dir {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("node_modules") {
                continue;
            }
            collect_ts_files_into(&path, generated_dir, out)?;
        } else if file_type.is_file() && path.extension().and_then(|e| e.to_str()) == Some("ts") {
            out.push(path);
        }
    }
    Ok(())
}

struct Outcome {
    output: String,
    used_ir: bool,
    report: portal_solutions_jade_swc_tenant_exposure::TenantExposureReport,
}

fn process_file(
    path: &Path,
    relative: &Path,
    config: &TenantExposureConfig,
) -> Result<Outcome, String> {
    let source = std::fs::read_to_string(path).map_err(|err| format!("read: {err}"))?;
    let file_label = relative.display().to_string();

    let (program, used_ir) = match build_program_via_ir(&file_label, &source) {
        Some(program) => (program, true),
        None => (parse_program(&file_label, &source)?, false),
    };

    swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let mut resolved = resolve(program, true);
        let report = transform_program(&mut resolved, config);
        let program = resolved.into_inner();
        let output = print_program(&program)?;
        Ok(Outcome {
            output,
            used_ir,
            report,
        })
    })
}

/// Step 1: `lower_module` + `emit_ast`, real AST the whole way, no text in between. Returns
/// `None` (not an error) whenever `lower_module` rejects the file outright, *or* whenever it
/// only partially lowers (`lowered.skipped` non-empty) — `lower_module` has "per-item
/// resilience" (see `lower::LoweredModule`'s doc comment): a top-level item it can't lower is
/// dropped with only an `eprintln!`, not a hard failure, which is safe for `gen-primordials`'
/// Rust-emission use case (nothing calls the omitted item) but NOT safe here — a dropped
/// function in shipped TypeScript is a silent runtime `ReferenceError` for whatever called it,
/// not a caught build error. Both cases fall back to a direct parse rather than risking that.
fn build_program_via_ir(file_label: &str, source: &str) -> Option<Program> {
    let lowered = portal_solutions_jade_primordial_ir::translate_source(file_label, source).ok()?;
    if !lowered.skipped.is_empty() {
        eprintln!(
            "jade-bundle-build: {file_label}: primordial-ir only partially lowered ({} item(s) skipped) — falling back to a direct parse rather than shipping a partial translation",
            lowered.skipped.len()
        );
        return None;
    }
    let ast_module = portal_solutions_jade_primordial_ir::emit_ast(&lowered.module);
    Some(Program::Module(ast_module))
}

/// Step 2: a plain TypeScript parse for anything step 1 didn't handle — every non-primordial
/// file in the package (`tenants/*.ts` real `Tenant` implementations use real `this`/method
/// semantics `lower_module` explicitly rejects) and any primordial file not yet IR-lowered.
fn parse_program(file_label: &str, source: &str) -> Result<Program, String> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Custom(file_label.to_owned())), source.to_owned());
    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: false,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    Parser::new_from(lexer)
        .parse_program()
        .map_err(|err| format!("parse: {err:?}"))
}

fn print_program(program: &Program) -> Result<String, String> {
    let cm: Lrc<SourceMap> = Default::default();
    let mut buf = Vec::new();
    {
        let mut emitter = Emitter {
            cfg: CodegenConfig::default(),
            cm: cm.clone(),
            comments: None,
            wr: JsWriter::new(cm, "\n", &mut buf, None),
        };
        program
            .emit_with(&mut emitter)
            .map_err(|err| format!("print: {err:?}"))?;
    }
    String::from_utf8(buf).map_err(|err| format!("print produced non-UTF-8 output: {err}"))
}

fn write_output(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

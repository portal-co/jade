//! Regeneration entrypoint (Phase 8 of the primordial-IR plan; wired up early, minimally, to
//! exercise the pipeline end-to-end during Phase 1/4 development). Currently takes exactly one
//! `--file <path>` pair for ad hoc testing against a single primordial source file; the real
//! "read every packages/jade-js/primordials/*.ts and write crates/jade-primordial-rt/src/*.rs"
//! sweep lands with Phase 8 once every file lowers cleanly.

use std::fs;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut file: Option<PathBuf> = None;
    let mut emit_rust = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => file = args.next().map(PathBuf::from),
            "--rust" => emit_rust = true,
            other => {
                eprintln!("unrecognized argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(file) = file else {
        eprintln!("usage: gen-primordials --file <path.ts> [--rust]");
        std::process::exit(2);
    };
    let source = fs::read_to_string(&file).expect("read source file");
    let file_name = file.file_stem().unwrap().to_string_lossy().to_string();
    let module = match portal_solutions_jade_primordial_ir::translate_source(&file_name, &source) {
        Ok(module) => module,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    if emit_rust {
        println!("{}", portal_solutions_jade_primordial_ir::emit_rust_source(&module, &file_name));
    } else {
        println!("{}", portal_solutions_jade_primordial_ir::emit_typescript(&module));
    }
}

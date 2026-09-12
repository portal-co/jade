//! `jade-test262` CLI.
//!
//!   jade-test262 [--shard <name>] [--tenant <name>] [--env a,b] [--report <path>]
//!                [--check] [--update-expectations] [--timeout-ms <n>]
//!   jade-test262 compile --file <test.js>
//!   jade-test262 validate-report <report.json>
//!
//! Bare `--shard …` (no subcommand) means `run --shard …`, so the plan's Phase-0
//! criterion `cargo run -p portal-solutions-jade-test262 -- --shard smoke` works as
//! written.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use portal_solutions_jade_test262::model::Report;
use portal_solutions_jade_test262::pipeline;
use portal_solutions_jade_test262::run;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("compile") => compile(&args[1..]),
        Some("validate-report") => validate_report(&args[1..]),
        Some("run") => run_shard(&args[1..]),
        _ => run_shard(&args[..]),
    }
}

fn compile(args: &[String]) -> ExitCode {
    let Some(file) = opt_value(args, "--file") else {
        eprintln!("compile: missing --file <test.js>");
        return ExitCode::FAILURE;
    };
    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("compile: read {file}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let artifact = pipeline::compile_artifact(&src);
    println!("{}", serde_json::to_string(&artifact).expect("serialize"));
    if artifact.ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn validate_report(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        eprintln!("validate-report: missing <report.json>");
        return ExitCode::FAILURE;
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("validate-report: read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report: Report = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("validate-report: {path}: invalid schema: {e}");
            return ExitCode::FAILURE;
        }
    };
    let problems = report.validate();
    if problems.is_empty() {
        println!("{path}: schema-valid ({} tests)", report.results.len());
        ExitCode::SUCCESS
    } else {
        for p in problems {
            eprintln!("{path}: {p}");
        }
        ExitCode::FAILURE
    }
}

fn run_shard(args: &[String]) -> ExitCode {
    let Some(shard) = opt_value(args, "--shard") else {
        eprintln!("run: missing --shard <smoke|test262:subdir>");
        return ExitCode::FAILURE;
    };
    let envs: Vec<String> = opt_value(args, "--env")
        .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(|| pipeline::NODE_ENVS.iter().map(|s| s.to_string()).collect());
    let report_path = opt_value(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| run::default_expectations_dir().join(format!("../reports/{shard}.json")));
    let timeout_ms = opt_value(args, "--timeout-ms")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(node_timeout_default());

    let cfg = run::RunConfig {
        shard: shard.clone(),
        tenant: opt_value(args, "--tenant").unwrap_or_else(|| "multi".into()),
        envs,
        report_path,
        manifest_path: run::default_manifest_path(),
        expectations_dir: run::default_expectations_dir(),
        check: args.iter().any(|a| a == "--check"),
        update_expectations: args.iter().any(|a| a == "--update-expectations"),
        limit: opt_value(args, "--limit").and_then(|s| s.parse::<usize>().ok()),
        timeout: Duration::from_millis(timeout_ms),
    };

    match run::run(&cfg) {
        Ok(report) => {
            println!("shard {shard}:");
            for (env, kinds) in report.rollup() {
                let mut parts = Vec::new();
                for (kind, n) in kinds {
                    parts.push(format!("{}={n}", portal_solutions_jade_test262::model::kind_name(kind)));
                }
                println!("  {env}: {}", parts.join(" "));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("run failed:\n{e}");
            ExitCode::FAILURE
        }
    }
}

fn node_timeout_default() -> u64 {
    portal_solutions_jade_test262::node::DEFAULT_TIMEOUT.as_millis() as u64
}

fn opt_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

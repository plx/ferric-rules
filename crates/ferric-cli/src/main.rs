//! # Ferric CLI
//!
//! Command-line interface for the Ferric rules engine.
//!
//! ## Phase 5 Baseline Assumptions
//!
//! This binary provides batch and interactive access to the Ferric runtime.
//! Phase 4 diagnostic contracts are preserved:
//!
//! - Source-located diagnostics are rendered with file/line/column context.
//! - Module visibility, ambiguity, and generic dispatch/conflict diagnostics
//!   are displayed without reinterpretation.
//! - Exit codes follow documented contracts (0 = success, 1 = runtime error,
//!   2 = usage error).
//!
//! ## Commands
//!
//! - `run <file>` — Load and execute a CLIPS file
//! - `check <file>` — Load and validate without executing
//! - `repl` — Interactive read-eval-print loop
//! - `version` — Print version information

mod commands;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let exit_code = match args.get(1).map(String::as_str) {
        Some("run") => commands::run::execute(&args[2..]),
        Some("check") => commands::check::execute(&args[2..]),
        Some("repl") => commands::repl::execute(&args[2..]),
        Some("version" | "--version" | "-V") => commands::version::execute(),
        Some(unknown) => {
            eprintln!("ferric: unknown command '{unknown}'");
            eprintln!("Usage: ferric <run|check|repl|version> [args...]");
            2
        }
        None => {
            eprintln!("Usage: ferric <run|check|repl|version> [args...]");
            2
        }
    };

    std::process::exit(exit_code);
}

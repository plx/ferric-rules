//! `ferric run` command — load and execute a CLIPS file.
//!
//! Pipeline: load file → reset → run → print output
//!
//! Exit codes:
//! - 0: Success
//! - 1: Runtime/load error
//! - 2: Usage error (missing file argument)

use std::path::Path;

use ferric_runtime::{Engine, EngineConfig, RunLimit};

/// Execute the `run` subcommand.
pub fn execute(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("ferric run: missing file argument");
        eprintln!("Usage: ferric run <file>");
        return 2;
    }

    let file_path = Path::new(&args[0]);
    if !file_path.exists() {
        eprintln!("ferric run: file not found: {}", file_path.display());
        return 1;
    }

    let mut engine = Engine::new(EngineConfig::default());

    // Load
    if let Err(errors) = engine.load_file(file_path) {
        for err in &errors {
            eprintln!("ferric run: {err}");
        }
        return 1;
    }

    // Reset (asserts initial-fact, processes deffacts)
    if let Err(err) = engine.reset() {
        eprintln!("ferric run: reset failed: {err}");
        return 1;
    }

    // Run
    match engine.run(RunLimit::Unlimited) {
        Ok(_result) => {
            // Print captured output from channel "t" (standard CLIPS output)
            if let Some(output) = engine.get_output("t") {
                print!("{output}");
            }

            // Print any action diagnostics as warnings
            for diag in engine.action_diagnostics() {
                eprintln!("ferric run: warning: {diag}");
            }

            // halt is normal termination in CLIPS — all outcomes are success
            0
        }
        Err(err) => {
            eprintln!("ferric run: execution failed: {err}");
            1
        }
    }
}

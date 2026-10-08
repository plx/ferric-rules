//! `ferric run` command — load and execute a CLIPS file.
//!
//! Pipeline: load file → reset → run → print output
//!
//! Exit codes:
//! - 0: Success
//! - 1: Runtime/load error

use std::path::Path;

use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

use super::common::{emit_action_diagnostics, emit_error};

/// Execute the `run` subcommand.
pub fn execute(json_mode: bool, file_path: &Path) -> i32 {
    if !file_path.exists() {
        emit_error(
            json_mode,
            "run",
            "io_error",
            format_args!("file not found: {}", file_path.display()),
        );
        return 1;
    }

    let mut engine = Engine::new(EngineConfig::default());

    // Load
    if let Err(errors) = engine.load_file(file_path) {
        for err in &errors {
            emit_error(json_mode, "run", "load_error", err);
        }
        return 1;
    }
    // Loading can assert facts or install rules against existing facts. Report
    // their match-time errors before reset clears the diagnostic buffer.
    emit_action_diagnostics(json_mode, "run", &engine);

    // Reset (asserts initial-fact, processes deffacts)
    if let Err(err) = engine.reset() {
        emit_error(
            json_mode,
            "run",
            "runtime_error",
            format_args!("reset failed: {err}"),
        );
        return 1;
    }
    // Reset evaluates LHS expressions while asserting seeds; run starts a new
    // diagnostic buffer even when those errors left no activation to fire.
    emit_action_diagnostics(json_mode, "run", &engine);

    // Run
    match engine.run(RunLimit::Unlimited) {
        Ok(_result) => {
            // Print captured output from channel "t" (standard CLIPS output)
            if let Some(output) = engine.get_output("t") {
                print!("{output}");
            }

            // Print any action diagnostics as warnings
            emit_action_diagnostics(json_mode, "run", &engine);

            // halt is normal termination in CLIPS — all outcomes are success
            0
        }
        Err(err) => {
            emit_error(
                json_mode,
                "run",
                "runtime_error",
                format_args!("execution failed: {err}"),
            );
            1
        }
    }
}

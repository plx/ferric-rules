//! `ferric run` command — load and execute a CLIPS file.
//!
//! Pipeline: load file → reset → run → print output
//!
//! Exit codes:
//! - 0: Success
//! - 1: Runtime/load error

use std::io::Write;
use std::path::Path;

use ferric_rules_runtime::{Engine, EngineConfig, RunLimit};

use super::common::{emit_error, emit_warning};

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

    // Run
    match engine.run(RunLimit::Unlimited) {
        Ok(_result) => {
            // Print captured output from channel "t" (standard CLIPS output)
            if let Err(error) = write_output(&engine, &mut std::io::stdout().lock()) {
                emit_error(json_mode, "run", "io_error", error);
                return 1;
            }

            // Print any action diagnostics as warnings
            for diag in engine.action_diagnostics() {
                emit_warning(json_mode, "run", "action_warning", diag);
            }

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

fn write_output(engine: &Engine, output: &mut impl Write) -> std::io::Result<()> {
    if let Some(bytes) = engine.get_output_bytes("t") {
        output.write_all(bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_output_writer_preserves_invalid_utf8_and_embedded_nul() {
        let mut engine =
            Engine::with_rules("(defrule emit (payload ?value) => (printout t ?value))").unwrap();
        let value = engine.create_string_bytes(b"a\0\xff\xc3").unwrap();
        engine.assert_ordered("payload", value).unwrap();
        engine.run(RunLimit::Unlimited).unwrap();
        let mut output = Vec::new();
        write_output(&engine, &mut output).unwrap();
        assert_eq!(output, b"a\0\xff\xc3");
    }
}

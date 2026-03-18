//! `ferric snapshot` — serialize an engine state to a file.
//!
//! Pipeline: load file → reset → serialize → write output
//!
//! Exit codes:
//! - 0: Success
//! - 1: Load/serialize/IO error

use std::io::Write;
use std::path::Path;

use ferric_runtime::serialization::SerializationFormat;
use ferric_runtime::{Engine, EngineConfig};

use super::common::{emit_error, emit_warning};

/// Execute the `snapshot` subcommand.
///
/// Loads the given CLIPS file, resets the engine, serializes it using the
/// specified format, and writes the result to the output path (or stdout
/// if `output` is `"-"`).
pub fn execute(json_mode: bool, file: &Path, output: &Path, format: SerializationFormat) -> i32 {
    if !file.exists() {
        emit_error(
            json_mode,
            "snapshot",
            "io_error",
            format_args!("file not found: {}", file.display()),
        );
        return 1;
    }

    let mut engine = Engine::new(EngineConfig::default());

    if let Err(errors) = engine.load_file(file) {
        for err in &errors {
            emit_error(json_mode, "snapshot", "load_error", err);
        }
        return 1;
    }

    if let Err(err) = engine.reset() {
        emit_error(
            json_mode,
            "snapshot",
            "runtime_error",
            format_args!("reset failed: {err}"),
        );
        return 1;
    }

    // Emit any load warnings.
    // (load_file only returns warnings via the Ok branch; we check action_diagnostics
    // here since reset can also produce diagnostics in some configurations.)
    for diag in engine.action_diagnostics() {
        emit_warning(json_mode, "snapshot", "action_warning", diag);
    }

    let bytes = match engine.serialize(format) {
        Ok(b) => b,
        Err(err) => {
            emit_error(
                json_mode,
                "snapshot",
                "serialize_error",
                format_args!("serialization failed: {err}"),
            );
            return 1;
        }
    };

    let output_str = output.to_string_lossy();
    if output_str == "-" {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        if let Err(err) = handle.write_all(&bytes) {
            emit_error(
                json_mode,
                "snapshot",
                "io_error",
                format_args!("error writing to stdout: {err}"),
            );
            return 1;
        }
    } else {
        if let Err(err) = std::fs::write(output, &bytes) {
            emit_error(
                json_mode,
                "snapshot",
                "io_error",
                format_args!("error writing to {}: {err}", output.display()),
            );
            return 1;
        }
        eprintln!(
            "Wrote {} bytes ({}) to {}",
            bytes.len(),
            format.name(),
            output.display()
        );
    }

    0
}

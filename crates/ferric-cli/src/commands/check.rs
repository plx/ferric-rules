//! `ferric check` command — load and validate without executing.
//!
//! Pipeline: load file (validates parse + compile) → report
//!
//! Exit codes:
//! - 0: File is valid
//! - 1: Validation/parse/compile error

use std::path::Path;

use ferric_runtime::{Engine, EngineConfig};

use super::common::emit_error;

/// Execute the `check` subcommand.
pub fn execute(json_mode: bool, file_path: &Path) -> i32 {
    if !file_path.exists() {
        emit_error(
            json_mode,
            "check",
            "io_error",
            format_args!("file not found: {}", file_path.display()),
        );
        return 1;
    }

    let mut engine = Engine::new(EngineConfig::default());

    match engine.load_file(file_path) {
        Ok(_) => 0,
        Err(errors) => {
            for err in &errors {
                emit_error(json_mode, "check", "load_error", err);
            }
            1
        }
    }
}

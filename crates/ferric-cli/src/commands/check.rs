//! `ferric check` command — load and validate without executing.
//!
//! Pipeline: load file (validates parse + compile) → report
//!
//! Exit codes:
//! - 0: File is valid
//! - 1: Validation/parse/compile error
//! - 2: Usage error (missing file argument)

use std::path::Path;

use ferric_runtime::{Engine, EngineConfig};

/// Execute the `check` subcommand.
pub fn execute(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("ferric check: missing file argument");
        eprintln!("Usage: ferric check <file>");
        return 2;
    }

    let file_path = Path::new(&args[0]);
    if !file_path.exists() {
        eprintln!("ferric check: file not found: {}", file_path.display());
        return 1;
    }

    let mut engine = Engine::new(EngineConfig::default());

    match engine.load_file(file_path) {
        Ok(_) => 0,
        Err(errors) => {
            for err in &errors {
                eprintln!("ferric check: {err}");
            }
            1
        }
    }
}

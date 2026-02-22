//! `ferric check` command — load and validate without executing.
//!
//! Pipeline: load file (validates parse + compile) → report
//!
//! Exit codes:
//! - 0: File is valid
//! - 1: Validation/parse/compile error
//! - 2: Usage error (missing file argument)

use std::fmt::Write as _;
use std::path::Path;

use ferric_runtime::{Engine, EngineConfig};

/// Execute the `check` subcommand.
pub fn execute(args: &[String]) -> i32 {
    let (json_mode, file_arg) = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };

    let file_path = Path::new(file_arg);
    if !file_path.exists() {
        emit_error(
            json_mode,
            "io_error",
            &format!("file not found: {}", file_path.display()),
        );
        return 1;
    }

    let mut engine = Engine::new(EngineConfig::default());

    match engine.load_file(file_path) {
        Ok(_) => 0,
        Err(errors) => {
            for err in &errors {
                emit_error(json_mode, "load_error", &err.to_string());
            }
            1
        }
    }
}

fn parse_args(args: &[String]) -> Result<(bool, &str), i32> {
    match args {
        [file] => Ok((false, file.as_str())),
        [flag, file] if flag == "--json" => Ok((true, file.as_str())),
        [] => {
            eprintln!("ferric check: missing file argument");
            eprintln!("Usage: ferric check [--json] <file>");
            Err(2)
        }
        _ => {
            eprintln!("ferric check: invalid arguments");
            eprintln!("Usage: ferric check [--json] <file>");
            Err(2)
        }
    }
}

fn emit_error(json_mode: bool, kind: &str, message: &str) {
    if json_mode {
        eprintln!(
            "{{\"command\":\"check\",\"level\":\"error\",\"kind\":\"{}\",\"message\":\"{}\"}}",
            json_escape(kind),
            json_escape(message)
        );
    } else {
        eprintln!("ferric check: {message}");
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(&mut out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

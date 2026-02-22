//! `ferric run` command — load and execute a CLIPS file.
//!
//! Pipeline: load file → reset → run → print output
//!
//! Exit codes:
//! - 0: Success
//! - 1: Runtime/load error
//! - 2: Usage error (missing file argument)

use std::fmt::Write as _;
use std::path::Path;

use ferric_runtime::{Engine, EngineConfig, RunLimit};

/// Execute the `run` subcommand.
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

    // Load
    if let Err(errors) = engine.load_file(file_path) {
        for err in &errors {
            emit_error(json_mode, "load_error", &err.to_string());
        }
        return 1;
    }

    // Reset (asserts initial-fact, processes deffacts)
    if let Err(err) = engine.reset() {
        emit_error(json_mode, "runtime_error", &format!("reset failed: {err}"));
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
                emit_warning(json_mode, "action_warning", &diag.to_string());
            }

            // halt is normal termination in CLIPS — all outcomes are success
            0
        }
        Err(err) => {
            emit_error(
                json_mode,
                "runtime_error",
                &format!("execution failed: {err}"),
            );
            1
        }
    }
}

fn parse_args(args: &[String]) -> Result<(bool, &str), i32> {
    match args {
        [file] => Ok((false, file.as_str())),
        [flag, file] if flag == "--json" => Ok((true, file.as_str())),
        [] => {
            eprintln!("ferric run: missing file argument");
            eprintln!("Usage: ferric run [--json] <file>");
            Err(2)
        }
        _ => {
            eprintln!("ferric run: invalid arguments");
            eprintln!("Usage: ferric run [--json] <file>");
            Err(2)
        }
    }
}

fn emit_error(json_mode: bool, kind: &str, message: &str) {
    if json_mode {
        eprintln!(
            "{{\"command\":\"run\",\"level\":\"error\",\"kind\":\"{}\",\"message\":\"{}\"}}",
            json_escape(kind),
            json_escape(message)
        );
    } else {
        eprintln!("ferric run: {message}");
    }
}

fn emit_warning(json_mode: bool, kind: &str, message: &str) {
    if json_mode {
        eprintln!(
            "{{\"command\":\"run\",\"level\":\"warning\",\"kind\":\"{}\",\"message\":\"{}\"}}",
            json_escape(kind),
            json_escape(message)
        );
    } else {
        eprintln!("ferric run: warning: {message}");
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

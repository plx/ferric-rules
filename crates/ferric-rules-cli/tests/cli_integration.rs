//! CLI integration tests — validates command dispatch, exit codes, and basic behavior.
//!
//! These tests verify the CLI binary can be invoked and responds correctly
//! to each command, including full run/check pipelines using fixture files.

mod cli_helpers;

use cli_helpers::*;

// ---- baseline / dispatch tests ----

#[test]
fn version_command_exits_zero() {
    let output = run_ferric(&["version"]);
    assert_exit_code(&output, 0);
    assert_stdout_contains(&output, env!("CARGO_PKG_VERSION"));
}

#[test]
fn version_flag_exits_zero() {
    let output = run_ferric(&["--version"]);
    assert_exit_code(&output, 0);
}

#[test]
fn version_short_flag_exits_zero() {
    let output = run_ferric(&["-V"]);
    assert_exit_code(&output, 0);
}

#[test]
fn no_args_exits_usage_error() {
    let output = run_ferric(&[]);
    assert_exit_code(&output, 2);
    assert_stderr_contains(&output, "Usage:");
}

#[test]
fn unknown_command_exits_usage_error() {
    let output = run_ferric(&["frobnicate"]);
    assert_exit_code(&output, 2);
    assert_stderr_contains(&output, "unrecognized subcommand");
}

#[test]
fn run_without_file_exits_usage_error() {
    let output = run_ferric(&["run"]);
    assert_exit_code(&output, 2);
    assert_stderr_contains(&output, "required arguments");
}

#[test]
fn check_without_file_exits_usage_error() {
    let output = run_ferric(&["check"]);
    assert_exit_code(&output, 2);
    assert_stderr_contains(&output, "required arguments");
}

#[test]
fn fixture_dir_exists() {
    let dir = std::path::Path::new(FIXTURES_DIR);
    assert!(
        dir.exists(),
        "CLI fixtures directory missing: {}",
        dir.display()
    );
    assert!(dir.is_dir());
}

#[test]
fn all_cli_fixtures_exist() {
    for name in [
        fixtures::HELLO,
        fixtures::CHECK_VALID,
        fixtures::CHECK_INVALID,
        fixtures::MULTI_MODULE,
    ] {
        let path = fixture_path(name);
        assert!(path.exists(), "CLI fixture not found: {}", path.display());
    }
}

// ---- run command tests ----

#[test]
fn run_hello_fixture_exits_zero() {
    let path = fixture_path(fixtures::HELLO);
    let output = run_ferric(&["run", path.to_str().unwrap()]);
    assert_exit_code(&output, 0);
    assert_stdout_contains(&output, "Hello, Ferric!");
}

#[test]
fn run_multi_module_exits_zero() {
    let path = fixture_path(fixtures::MULTI_MODULE);
    let output = run_ferric(&["run", path.to_str().unwrap()]);
    assert_exit_code(&output, 0);
    assert_stdout_contains(&output, "Starting");
}

#[test]
fn run_missing_file_exits_one() {
    let output = run_ferric(&["run", "/nonexistent/file.clp"]);
    assert_exit_code(&output, 1);
    assert_stderr_contains(&output, "file not found");
}

#[test]
fn run_invalid_file_exits_one() {
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["run", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);
    // Diagnostic must appear on stderr
    assert_stderr_contains(&output, "ferric run:");
}

// ---- repl command tests ----

#[test]
fn repl_exit_command() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let bin = env!("CARGO_BIN_EXE_ferric");
    let mut child = Command::new(bin)
        .args(["repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ferric repl");

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "(exit)").unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert_exit_code(&output, 0);
}

#[test]
fn repl_quit_command() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let bin = env!("CARGO_BIN_EXE_ferric");
    let mut child = Command::new(bin)
        .args(["repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ferric repl");

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "(quit)").unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert_exit_code(&output, 0);
}

#[test]
fn repl_eof_exits_zero() {
    use std::process::{Command, Stdio};

    let bin = env!("CARGO_BIN_EXE_ferric");
    let child = Command::new(bin)
        .args(["repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ferric repl");

    // Close stdin immediately (EOF).
    let output = child.wait_with_output().unwrap();
    assert_exit_code(&output, 0);
}

#[test]
fn repl_define_and_run_rule() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let bin = env!("CARGO_BIN_EXE_ferric");
    let mut child = Command::new(bin)
        .args(["repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ferric repl");

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(
        stdin,
        r#"(defrule hello (initial-fact) => (printout t "hi" crlf))"#
    )
    .unwrap();
    writeln!(stdin, "(reset)").unwrap();
    writeln!(stdin, "(run)").unwrap();
    writeln!(stdin, "(exit)").unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert_exit_code(&output, 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hi"), "expected 'hi' in stdout: {stdout}");
}

#[test]
fn repl_facts_command() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let bin = env!("CARGO_BIN_EXE_ferric");
    let mut child = Command::new(bin)
        .args(["repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn ferric repl");

    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "(reset)").unwrap();
    writeln!(stdin, "(assert (color red))").unwrap();
    writeln!(stdin, "(facts)").unwrap();
    writeln!(stdin, "(exit)").unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert_exit_code(&output, 0);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("color"),
        "expected 'color' in facts output: {stdout}"
    );
    assert!(
        stdout.contains("red"),
        "expected 'red' in facts output: {stdout}"
    );
}

// ---- Phase 4 diagnostic parity through CLI ----

#[test]
fn run_invalid_source_shows_diagnostic() {
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["run", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);
    let stderr = stderr_str(&output);
    // Should contain "ferric run:" prefix and some diagnostic text
    assert!(
        stderr.contains("ferric run:"),
        "expected 'ferric run:' prefix in stderr: {stderr}"
    );
    assert!(!stderr.is_empty());
}

#[test]
fn check_invalid_source_shows_diagnostic() {
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["check", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("ferric check:"),
        "expected 'ferric check:' prefix in stderr: {stderr}"
    );
}

#[test]
fn run_invalid_source_json_mode_shows_machine_diagnostic() {
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("\"command\":\"run\""),
        "expected run command key in JSON stderr: {stderr}"
    );
    assert!(
        stderr.contains("\"level\":\"error\""),
        "expected error level in JSON stderr: {stderr}"
    );
    assert!(
        stderr.contains("\"kind\":\"load_error\""),
        "expected load_error kind in JSON stderr: {stderr}"
    );
}

#[test]
fn check_invalid_source_json_mode_shows_machine_diagnostic() {
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["check", "--json", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("\"command\":\"check\""),
        "expected check command key in JSON stderr: {stderr}"
    );
    assert!(
        stderr.contains("\"level\":\"error\""),
        "expected error level in JSON stderr: {stderr}"
    );
    assert!(
        stderr.contains("\"kind\":\"load_error\""),
        "expected load_error kind in JSON stderr: {stderr}"
    );
}

#[test]
fn check_conflict_source_shows_phase4_conflict_diagnostic() {
    use std::io::Write;

    let mut temp = tempfile::NamedTempFile::new().expect("create temp clp");
    writeln!(
        temp,
        "(deffunction compute (?x) (+ ?x 1))\n(defgeneric compute)"
    )
    .expect("write temp clp");
    let path = temp.path().to_str().expect("utf8 temp path");

    let output = run_ferric(&["check", path]);
    assert_exit_code(&output, 1);
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("compute"),
        "expected diagnostic to include conflicting name: {stderr}"
    );
    assert!(
        stderr.contains("deffunction") || stderr.contains("defgeneric"),
        "expected diagnostic to include conflict context: {stderr}"
    );
}

#[test]
fn run_phase4_visibility_warning_is_surfaced() {
    use std::io::Write;

    let mut temp = tempfile::NamedTempFile::new().expect("create temp clp");
    writeln!(
        temp,
        r"
        (defmodule MATH (export ?NONE))
        (deffunction add (?x ?y) (+ ?x ?y))

        (defmodule MAIN)
        (defrule test-call (go) => (printout t (MATH::add 3 4) crlf))
        (deffacts startup (go))
        "
    )
    .expect("write temp clp");
    let path = temp.path().to_str().expect("utf8 temp path");

    let output = run_ferric(&["run", path]);
    assert_exit_code(&output, 0);
    let stderr = stderr_str(&output);
    assert!(
        stderr.contains("warning"),
        "expected warning output for visibility diagnostic: {stderr}"
    );
    assert!(
        stderr.contains("not visible")
            || stderr.contains("not accessible")
            || stderr.contains("NotVisible"),
        "expected visibility diagnostic wording in stderr: {stderr}"
    );
}

// ---- check command tests ----

#[test]
fn check_valid_file_exits_zero() {
    let path = fixture_path(fixtures::CHECK_VALID);
    let output = run_ferric(&["check", path.to_str().unwrap()]);
    assert_exit_code(&output, 0);
}

#[test]
fn check_invalid_file_exits_one() {
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["check", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);
    assert_stderr_contains(&output, "ferric check:");
}

#[test]
fn check_missing_file_exits_one() {
    let output = run_ferric(&["check", "/nonexistent/file.clp"]);
    assert_exit_code(&output, 1);
    assert_stderr_contains(&output, "file not found");
}

// ---- JSON contract lock tests ----
//
// These tests document and lock the machine-readable CLI diagnostics contract.
// The `--json` mode emits one JSON object per line on stderr; engine output
// (printout) goes to stdout.  The required top-level fields are:
//   "command", "level", "kind", "message"
//
// New fields MAY be added in the future; existing fields MUST NOT be removed
// or repurposed (additive-evolution guarantee).

/// Parse every non-empty line of `stderr` as a JSON object, asserting no
/// parse failures, and return the collected values.
fn parse_json_lines(stderr: &str) -> Vec<serde_json::Value> {
    stderr
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("stderr line is not valid JSON: {e}\nline: {line}"))
        })
        .collect()
}

/// Assert that a JSON value has a string field equal to `expected`.
fn assert_json_field_eq(v: &serde_json::Value, field: &str, expected: &str) {
    let actual = v
        .get(field)
        .unwrap_or_else(|| panic!("missing field '{field}' in JSON object: {v}"))
        .as_str()
        .unwrap_or_else(|| panic!("field '{field}' is not a string in: {v}"));
    assert_eq!(
        actual, expected,
        "field '{field}' expected '{expected}', got '{actual}'"
    );
}

#[test]
fn contract_lock_run_json_success_shape() {
    // A successful run produces no error diagnostics on stderr.
    // Engine output goes to stdout.
    let path = fixture_path(fixtures::HELLO);
    let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
    assert_exit_code(&output, 0);

    // Any JSON lines that do appear must be well-formed.
    let stderr = stderr_str(&output);
    let diags = parse_json_lines(&stderr);

    // On success there must be no error-level diagnostics.
    for diag in &diags {
        let level = diag.get("level").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            level, "error",
            "unexpected error diagnostic on successful run: {diag}"
        );
    }
}

#[test]
fn contract_lock_run_json_error_shape() {
    // A load failure with `--json` emits one JSON object per error on stderr.
    // Required fields: command, level, kind, message.
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);

    let stderr = stderr_str(&output);
    let diags = parse_json_lines(&stderr);
    assert!(
        !diags.is_empty(),
        "expected at least one JSON diagnostic on stderr for a load error, got nothing"
    );

    for diag in &diags {
        assert!(
            diag.get("command").is_some(),
            "missing 'command' field: {diag}"
        );
        assert!(diag.get("level").is_some(), "missing 'level' field: {diag}");
        assert!(diag.get("kind").is_some(), "missing 'kind' field: {diag}");
        assert!(
            diag.get("message").is_some(),
            "missing 'message' field: {diag}"
        );
    }
}

#[test]
fn contract_lock_check_json_success() {
    // A valid file checked with `--json` exits 0 with no error diagnostics.
    let path = fixture_path(fixtures::CHECK_VALID);
    let output = run_ferric(&["check", "--json", path.to_str().unwrap()]);
    assert_exit_code(&output, 0);

    let stderr = stderr_str(&output);
    let diags = parse_json_lines(&stderr);
    for diag in &diags {
        let level = diag.get("level").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            level, "error",
            "unexpected error diagnostic on successful check: {diag}"
        );
    }
}

#[test]
fn contract_lock_check_json_error_shape() {
    // An invalid file checked with `--json` exits 1.
    // Required fields: command, level, kind, message.
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["check", "--json", path.to_str().unwrap()]);
    assert_exit_code(&output, 1);

    let stderr = stderr_str(&output);
    let diags = parse_json_lines(&stderr);
    assert!(
        !diags.is_empty(),
        "expected at least one JSON diagnostic on stderr for a check error"
    );

    for diag in &diags {
        assert!(
            diag.get("command").is_some(),
            "missing 'command' field: {diag}"
        );
        assert!(diag.get("level").is_some(), "missing 'level' field: {diag}");
        assert!(diag.get("kind").is_some(), "missing 'kind' field: {diag}");
        assert!(
            diag.get("message").is_some(),
            "missing 'message' field: {diag}"
        );
    }
}

#[test]
fn contract_lock_json_command_field_matches_invocation() {
    // `run --json` diagnostics carry `"command":"run"`.
    {
        let path = fixture_path(fixtures::CHECK_INVALID);
        let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
        let stderr = stderr_str(&output);
        let diags = parse_json_lines(&stderr);
        for diag in &diags {
            assert_json_field_eq(diag, "command", "run");
        }
    }

    // `check --json` diagnostics carry `"command":"check"`.
    {
        let path = fixture_path(fixtures::CHECK_INVALID);
        let output = run_ferric(&["check", "--json", path.to_str().unwrap()]);
        let stderr = stderr_str(&output);
        let diags = parse_json_lines(&stderr);
        for diag in &diags {
            assert_json_field_eq(diag, "command", "check");
        }
    }
}

#[test]
fn contract_lock_json_level_field_is_error_or_warning() {
    // The `level` field MUST be exactly "error" or "warning" — no other values.
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
    let stderr = stderr_str(&output);
    let diags = parse_json_lines(&stderr);
    for diag in &diags {
        let level = diag
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("missing 'level' field: {diag}"));
        assert!(
            level == "error" || level == "warning",
            "level field must be 'error' or 'warning', got '{level}' in: {diag}"
        );
    }
}

#[test]
fn contract_lock_json_kind_field_is_present() {
    // The `kind` field documents the diagnostic category (e.g. "load_error",
    // "io_error", "runtime_error", "action_warning").  It must always be present.
    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
    let stderr = stderr_str(&output);
    let diags = parse_json_lines(&stderr);
    assert!(
        !diags.is_empty(),
        "expected diagnostics for an invalid file"
    );
    for diag in &diags {
        let kind = diag
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("missing 'kind' field: {diag}"));
        assert!(!kind.is_empty(), "'kind' field must not be empty: {diag}");
    }
}

#[test]
fn contract_lock_exit_codes() {
    // Exit codes are the same regardless of --json mode.
    // 0 = success, 1 = runtime/load error, 2 = usage error.

    // 0: valid file, plain mode
    {
        let path = fixture_path(fixtures::HELLO);
        assert_exit_code(&run_ferric(&["run", path.to_str().unwrap()]), 0);
        assert_exit_code(&run_ferric(&["run", "--json", path.to_str().unwrap()]), 0);
    }

    // 1: load error, both modes
    {
        let path = fixture_path(fixtures::CHECK_INVALID);
        assert_exit_code(&run_ferric(&["run", path.to_str().unwrap()]), 1);
        assert_exit_code(&run_ferric(&["run", "--json", path.to_str().unwrap()]), 1);
        assert_exit_code(&run_ferric(&["check", path.to_str().unwrap()]), 1);
        assert_exit_code(&run_ferric(&["check", "--json", path.to_str().unwrap()]), 1);
    }

    // 2: usage error (no file), both modes
    {
        assert_exit_code(&run_ferric(&["run"]), 2);
        assert_exit_code(&run_ferric(&["check"]), 2);
        assert_exit_code(&run_ferric(&[]), 2);
    }
}

#[test]
fn contract_lock_json_mode_diagnostics_on_stderr() {
    // In --json mode: diagnostics go to stderr, engine output to stdout.
    let path = fixture_path(fixtures::HELLO);
    let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
    assert_exit_code(&output, 0);

    // Engine printout lands on stdout.
    let stdout = stdout_str(&output);
    assert!(
        stdout.contains("Hello, Ferric!"),
        "expected engine output on stdout: {stdout}"
    );

    // stderr either empty or contains only valid JSON (no human-readable prefix).
    let stderr = stderr_str(&output);
    for line in stderr.lines().filter(|l| !l.is_empty()) {
        let _: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("non-JSON content on stderr in --json mode: {e}\nline: {line}")
        });
    }
}

#[test]
fn contract_lock_json_additive_evolution_baseline() {
    // Document the required baseline fields.  New fields may be added in future
    // releases, but these four MUST remain present and MUST remain strings.
    const REQUIRED_FIELDS: &[&str] = &["command", "level", "kind", "message"];

    let path = fixture_path(fixtures::CHECK_INVALID);
    let output = run_ferric(&["run", "--json", path.to_str().unwrap()]);
    let stderr = stderr_str(&output);
    let diags = parse_json_lines(&stderr);
    assert!(
        !diags.is_empty(),
        "expected at least one JSON diagnostic for additive-evolution baseline check"
    );

    for diag in &diags {
        for field in REQUIRED_FIELDS {
            let value = diag
                .get(*field)
                .unwrap_or_else(|| panic!("baseline field '{field}' missing from: {diag}"));
            assert!(
                value.is_string(),
                "baseline field '{field}' must be a string, got: {value}"
            );
        }
    }
}

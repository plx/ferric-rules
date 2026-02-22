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
    assert_stderr_contains(&output, "unknown command");
}

#[test]
fn run_without_file_exits_usage_error() {
    let output = run_ferric(&["run"]);
    assert_exit_code(&output, 2);
    assert_stderr_contains(&output, "missing file");
}

#[test]
fn check_without_file_exits_usage_error() {
    let output = run_ferric(&["check"]);
    assert_exit_code(&output, 2);
    assert_stderr_contains(&output, "missing file");
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

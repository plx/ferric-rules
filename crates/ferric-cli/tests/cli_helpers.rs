//! Shared test helpers for CLI command integration testing.
//!
//! Provides utilities for:
//! - Running the `ferric` binary and capturing output
//! - Exit code assertions
//! - stdout/stderr content verification
//! - Fixture file path resolution

use std::path::PathBuf;
use std::process::{Command, Output};

/// Path to CLI test fixture files.
pub const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/cli");

/// Get the path to a specific CLI fixture file.
pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(FIXTURES_DIR).join(name)
}

/// Standard CLI fixture file names.
pub mod fixtures {
    pub const HELLO: &str = "hello.clp";
    pub const CHECK_VALID: &str = "check_valid.clp";
    pub const CHECK_INVALID: &str = "check_invalid.clp";
    pub const MULTI_MODULE: &str = "multi_module.clp";
}

/// Run the ferric CLI binary with the given arguments.
/// Returns the `Output` for assertion.
pub fn run_ferric(args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_ferric");
    Command::new(bin)
        .args(args)
        .output()
        .expect("failed to execute ferric binary")
}

/// Assert that the process exited with the expected code.
pub fn assert_exit_code(output: &Output, expected: i32) {
    let actual = output.status.code().expect("process terminated by signal");
    assert_eq!(
        actual,
        expected,
        "expected exit code {expected}, got {actual}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Get stdout as a String.
pub fn stdout_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Get stderr as a String.
pub fn stderr_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Assert that stdout contains the given substring.
pub fn assert_stdout_contains(output: &Output, expected: &str) {
    let stdout = stdout_str(output);
    assert!(
        stdout.contains(expected),
        "expected stdout to contain '{expected}'\nactual stdout: {stdout}"
    );
}

/// Assert that stderr contains the given substring.
pub fn assert_stderr_contains(output: &Output, expected: &str) {
    let stderr = stderr_str(output);
    assert!(
        stderr.contains(expected),
        "expected stderr to contain '{expected}'\nactual stderr: {stderr}"
    );
}

//! Build matrix verification tests (Pass 009).
//!
//! These tests validate that FFI artifacts build correctly under all profiles
//! and that the default test profile retains normal unwind semantics.
//!
//! The `#[ignore]`d tests invoke `cargo build` as subprocesses and are slow;
//! they are intended for CI validation rather than routine `cargo test` runs.
//!
//! Run the ignored tests explicitly with:
//! ```sh
//! cargo test -p ferric-ffi build_matrix -- --ignored
//! ```

use std::path::Path;
use std::process::Command;

/// Returns the workspace root directory (two levels above `CARGO_MANIFEST_DIR`,
/// since `ferric-ffi` lives at `crates/ferric-ffi`).
fn workspace_root() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root")
        .to_str()
        .expect("workspace root is valid UTF-8")
        .to_string()
}

/// Runs `cargo build -p ferric-ffi --profile <profile>` from the workspace root
/// and returns the process output.
fn cargo_build(profile: &str) -> std::process::Output {
    Command::new("cargo")
        .args(["build", "-p", "ferric-ffi", "--profile", profile])
        .current_dir(workspace_root())
        .output()
        .expect("failed to execute cargo build")
}

// ---------------------------------------------------------------------------
// Platform-specific artifact names
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
const CDYLIB_NAME: &str = "libferric_ffi.dylib";
#[cfg(target_os = "linux")]
const CDYLIB_NAME: &str = "libferric_ffi.so";
#[cfg(target_os = "windows")]
const CDYLIB_NAME: &str = "ferric_ffi.dll";

#[cfg(any(target_os = "macos", target_os = "linux"))]
const STATICLIB_NAME: &str = "libferric_ffi.a";
#[cfg(target_os = "windows")]
const STATICLIB_NAME: &str = "ferric_ffi.lib";

// ---------------------------------------------------------------------------
// Build subprocess tests (slow — marked #[ignore])
// ---------------------------------------------------------------------------

/// Verify that `cargo build -p ferric-ffi --profile ffi-dev` exits successfully.
#[test]
#[ignore = "invokes cargo build as a subprocess; slow, intended for CI"]
fn ffi_dev_profile_builds() {
    let output = cargo_build("ffi-dev");
    assert!(
        output.status.success(),
        "ffi-dev build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Verify that `cargo build -p ferric-ffi --profile ffi-release` exits successfully.
#[test]
#[ignore = "invokes cargo build as a subprocess; slow, intended for CI"]
fn ffi_release_profile_builds() {
    let output = cargo_build("ffi-release");
    assert!(
        output.status.success(),
        "ffi-release build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Verify that the `ffi-dev` profile produces both cdylib and staticlib artifacts
/// at the expected paths.
#[test]
#[ignore = "invokes cargo build as a subprocess; slow, intended for CI"]
fn ffi_dev_produces_artifacts() {
    let output = cargo_build("ffi-dev");
    assert!(
        output.status.success(),
        "ffi-dev build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let root = workspace_root();
    let base = Path::new(&root).join("target/ffi-dev");

    let cdylib = base.join(CDYLIB_NAME);
    let staticlib = base.join(STATICLIB_NAME);

    assert!(
        cdylib.exists(),
        "cdylib artifact not found: {}",
        cdylib.display()
    );
    assert!(
        staticlib.exists(),
        "staticlib artifact not found: {}",
        staticlib.display()
    );
}

/// Verify that the `ffi-release` profile produces both cdylib and staticlib
/// artifacts at the expected paths.
#[test]
#[ignore = "invokes cargo build as a subprocess; slow, intended for CI"]
fn ffi_release_produces_artifacts() {
    let output = cargo_build("ffi-release");
    assert!(
        output.status.success(),
        "ffi-release build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let root = workspace_root();
    let base = Path::new(&root).join("target/ffi-release");

    let cdylib = base.join(CDYLIB_NAME);
    let staticlib = base.join(STATICLIB_NAME);

    assert!(
        cdylib.exists(),
        "cdylib artifact not found: {}",
        cdylib.display()
    );
    assert!(
        staticlib.exists(),
        "staticlib artifact not found: {}",
        staticlib.display()
    );
}

// ---------------------------------------------------------------------------
// Unwind semantics test (runs in normal `cargo test`)
// ---------------------------------------------------------------------------

/// Confirm that the default test profile retains normal unwind semantics.
///
/// If `panic = "abort"` were active for test builds, `catch_unwind` would
/// terminate the process instead of returning `Err`. The fact that this test
/// passes proves the test profile is NOT using `panic = "abort"`.
#[test]
fn default_test_profile_uses_unwind() {
    let result = std::panic::catch_unwind(|| {
        panic!("deliberate panic to verify unwind semantics");
    });
    assert!(
        result.is_err(),
        "catch_unwind should have caught the panic — test profile must use unwind, not abort"
    );
}

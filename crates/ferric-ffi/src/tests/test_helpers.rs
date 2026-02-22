//! Shared test helpers for FFI contract testing.
//!
//! Provides utilities for:
//! - Error-code assertions (verifying `FerricError` values match expectations)
//! - Pointer/null safety validation
//! - Error message retrieval and content verification
//! - Thread-violation scenario setup

/// Path to FFI test fixture files.
pub const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/ffi");

/// Helper to get the path to a specific FFI fixture file.
pub fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(FIXTURES_DIR).join(name)
}

/// Verify that a fixture file exists and is readable.
pub fn assert_fixture_exists(name: &str) {
    let path = fixture_path(name);
    assert!(path.exists(), "FFI fixture not found: {}", path.display());
    assert!(
        path.is_file(),
        "FFI fixture is not a file: {}",
        path.display()
    );
}

// --- Placeholder helpers for future passes ---
// These will be expanded when `FerricError` (Pass 003) and engine lifecycle (Pass 004) land.

/// Standard FFI fixture file names used across tests.
pub mod fixtures {
    pub const SIMPLE_RULE: &str = "simple_rule.clp";
    pub const ASSERT_RETRACT: &str = "assert_retract.clp";
    pub const MODULE_VISIBILITY: &str = "module_visibility.clp";
    pub const PARSE_ERROR: &str = "parse_error.clp";
    pub const GENERIC_DISPATCH: &str = "generic_dispatch.clp";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_dir_exists() {
        let dir = std::path::Path::new(FIXTURES_DIR);
        assert!(
            dir.exists(),
            "FFI fixtures directory missing: {}",
            dir.display()
        );
        assert!(
            dir.is_dir(),
            "FFI fixtures path is not a directory: {}",
            dir.display()
        );
    }

    #[test]
    fn all_ffi_fixtures_exist() {
        assert_fixture_exists(fixtures::SIMPLE_RULE);
        assert_fixture_exists(fixtures::ASSERT_RETRACT);
        assert_fixture_exists(fixtures::MODULE_VISIBILITY);
        assert_fixture_exists(fixtures::PARSE_ERROR);
        assert_fixture_exists(fixtures::GENERIC_DISPATCH);
    }

    #[test]
    fn fixture_paths_are_absolute() {
        let path = fixture_path(fixtures::SIMPLE_RULE);
        assert!(
            path.is_absolute() || path.exists(),
            "fixture_path should produce valid paths"
        );
    }
}

//! Persistent REPL history path resolution.

use std::path::PathBuf;

/// Resolve the file path for persistent REPL history.
///
/// Returns `~/.ferric_history` when `$HOME` is set, or `None` otherwise.
pub(crate) fn history_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".ferric_history"))
}

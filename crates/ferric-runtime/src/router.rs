//! Output routing for `printout` and related I/O functions.
//!
//! CLIPS uses a router abstraction to direct output to different logical
//! destinations (channels). This module provides a minimal implementation
//! that captures output per channel name, enabling tests to inspect output
//! without relying on global state or process-level I/O.

use std::collections::HashMap;

/// An output router that captures output by logical channel name.
///
/// CLIPS uses channel names like `t` (standard output), `stdout`, and
/// `stderr`. All output is captured in per-channel string buffers; no
/// output is written to process I/O. Tests can inspect captured output
/// via [`OutputRouter::get_output`].
#[derive(Clone, Debug, Default)]
pub struct OutputRouter {
    /// Captured output buffers keyed by channel name.
    buffers: HashMap<String, String>,
}

impl OutputRouter {
    /// Create a new router with no captured output.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `data` to the named channel's buffer.
    pub fn write(&mut self, channel: &str, data: &str) {
        self.buffers
            .entry(channel.to_string())
            .or_default()
            .push_str(data);
    }

    /// Return the captured output for `channel`, or `None` if nothing has
    /// been written to that channel.
    #[must_use]
    pub fn get_output(&self, channel: &str) -> Option<&str> {
        self.buffers.get(channel).map(String::as_str)
    }

    /// Clear all captured output across all channels.
    pub fn clear(&mut self) {
        self.buffers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_router_has_no_output() {
        let router = OutputRouter::new();
        assert!(router.get_output("t").is_none());
    }

    #[test]
    fn write_captures_output() {
        let mut router = OutputRouter::new();
        router.write("t", "hello");
        assert_eq!(router.get_output("t"), Some("hello"));
    }

    #[test]
    fn write_appends_to_channel() {
        let mut router = OutputRouter::new();
        router.write("t", "hello");
        router.write("t", " world");
        assert_eq!(router.get_output("t"), Some("hello world"));
    }

    #[test]
    fn separate_channels_are_independent() {
        let mut router = OutputRouter::new();
        router.write("t", "stdout");
        router.write("stderr", "error");
        assert_eq!(router.get_output("t"), Some("stdout"));
        assert_eq!(router.get_output("stderr"), Some("error"));
    }

    #[test]
    fn clear_removes_all_output() {
        let mut router = OutputRouter::new();
        router.write("t", "data");
        router.clear();
        assert!(router.get_output("t").is_none());
    }
}

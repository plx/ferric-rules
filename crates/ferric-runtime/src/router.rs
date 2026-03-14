//! Output routing for `printout` and related I/O functions.
//!
//! CLIPS uses a router abstraction to direct output to different logical
//! destinations (channels). This module provides a minimal implementation
//! that captures output per channel name, enabling tests to inspect output
//! without relying on global state or process-level I/O.

use rustc_hash::FxHashMap as HashMap;

/// An output router that captures output by logical channel name.
///
/// CLIPS uses channel names like `t` (standard output), `stdout`, and
/// `stderr`. All output is captured in per-channel string buffers; no
/// output is written to process I/O. Tests can inspect captured output
/// via [`OutputRouter::get_output`].
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OutputRouter {
    /// Captured output buffers keyed by channel name.
    #[cfg_attr(
        feature = "serde",
        serde(with = "ferric_core::serde_helpers::fx_hash_map")
    )]
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

    /// Clear captured output for a single channel.
    pub fn clear_channel(&mut self, channel: &str) {
        self.buffers.remove(channel);
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

    #[test]
    fn clear_channel_removes_only_target_channel() {
        let mut router = OutputRouter::new();
        router.write("t", "stdout");
        router.write("stderr", "error");

        router.clear_channel("t");

        assert!(router.get_output("t").is_none());
        assert_eq!(router.get_output("stderr"), Some("error"));
    }

    // -----------------------------------------------------------------------
    // Property tests: OutputRouter
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    /// Small fixed pool of channel names used across property tests.
    const CHANNELS: &[&str] = &["t", "stderr", "wtrace", "ch1", "ch2"];

    fn channel_idx_strategy() -> impl Strategy<Value = usize> {
        0usize..CHANNELS.len()
    }

    fn data_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9 ]{0,20}"
    }

    proptest! {
        /// Postcondition: Writing s1 then s2 to the same channel yields exactly s1+s2.
        /// Verifies that `write` performs string concatenation, not replacement.
        #[test]
        fn write_is_concatenation(
            ch_idx in channel_idx_strategy(),
            s1 in data_strategy(),
            s2 in data_strategy(),
        ) {
            let ch = CHANNELS[ch_idx];
            let mut router = OutputRouter::new();
            router.write(ch, &s1);
            router.write(ch, &s2);
            let expected = format!("{s1}{s2}");
            prop_assert_eq!(
                router.get_output(ch),
                Some(expected.as_str()),
                "write did not concatenate: expected {:?}, got {:?}",
                expected, router.get_output(ch)
            );
        }

        /// Isolation invariant: Writing to channel A never affects channel B's output.
        #[test]
        fn channel_isolation(
            a_idx in channel_idx_strategy(),
            b_idx in channel_idx_strategy(),
            data in data_strategy(),
        ) {
            prop_assume!(a_idx != b_idx);
            let ch_a = CHANNELS[a_idx];
            let ch_b = CHANNELS[b_idx];
            let mut router = OutputRouter::new();
            router.write(ch_a, &data);
            // Channel B must remain unaffected by writes to channel A.
            prop_assert!(
                router.get_output(ch_b).is_none(),
                "write to {:?} polluted channel {:?}",
                ch_a, ch_b
            );
        }

        /// Postcondition: After `clear()`, every channel returns `None`.
        /// Verifies that clear() truly removes all buffered output.
        #[test]
        fn clear_removes_all(
            writes in prop::collection::vec(
                (channel_idx_strategy(), data_strategy()),
                1..10
            )
        ) {
            let mut router = OutputRouter::new();
            for (ch_idx, data) in &writes {
                router.write(CHANNELS[*ch_idx], data);
            }
            router.clear();
            // After a full clear, every channel must return None.
            for ch in CHANNELS {
                prop_assert!(
                    router.get_output(ch).is_none(),
                    "clear() left data in channel {:?}",
                    ch
                );
            }
        }

        /// Invariant: `clear_channel(x)` removes only channel x.
        /// All other channels that had data must still have their data.
        #[test]
        fn clear_channel_only_target(
            target_idx in channel_idx_strategy(),
            writes in prop::collection::vec(
                (channel_idx_strategy(), data_strategy()),
                1..10
            )
        ) {
            let target = CHANNELS[target_idx];
            let mut router = OutputRouter::new();
            // Track what ended up in each channel before the clear.
            let mut expected: std::collections::HashMap<usize, String> =
                std::collections::HashMap::new();
            for (ch_idx, data) in &writes {
                router.write(CHANNELS[*ch_idx], data);
                expected.entry(*ch_idx).or_default().push_str(data);
            }
            router.clear_channel(target);
            // Target channel must now be empty.
            prop_assert!(
                router.get_output(target).is_none(),
                "clear_channel({:?}) did not remove the channel",
                target
            );
            // All other channels must be unaffected.
            for (ch_idx, expected_data) in &expected {
                if CHANNELS[*ch_idx] == target {
                    continue;
                }
                prop_assert_eq!(
                    router.get_output(CHANNELS[*ch_idx]),
                    Some(expected_data.as_str()),
                    "clear_channel({:?}) corrupted channel {:?}",
                    target, CHANNELS[*ch_idx]
                );
            }
        }

        /// Precondition: A channel that was never written to always returns `None`.
        #[test]
        fn get_unwritten_returns_none(ch_idx in channel_idx_strategy()) {
            let router = OutputRouter::new();
            prop_assert!(
                router.get_output(CHANNELS[ch_idx]).is_none(),
                "fresh router returned Some for unwritten channel {:?}",
                CHANNELS[ch_idx]
            );
        }
    }

    /// Operations for the `OutputRouter` shadow-model property test.
    #[derive(Clone, Debug)]
    enum RouterOp {
        Write(usize, usize), // (channel_idx, data_idx)
        Clear,
        ClearChannel(usize), // channel_idx
        GetOutput(usize),    // channel_idx
    }

    // Fixed pool of data strings to write.
    const DATA_POOL: &[&str] = &[
        "hello",
        "world",
        "foo",
        "bar",
        "baz",
        "test data",
        "123",
        "abc",
        "",
        "xyz",
    ];

    fn router_op_strategy() -> impl Strategy<Value = RouterOp> {
        prop_oneof![
            5 => (0usize..CHANNELS.len(), 0usize..DATA_POOL.len())
                .prop_map(|(c, d)| RouterOp::Write(c, d)),
            1 => Just(RouterOp::Clear),
            2 => (0usize..CHANNELS.len()).prop_map(RouterOp::ClearChannel),
            2 => (0usize..CHANNELS.len()).prop_map(RouterOp::GetOutput),
        ]
    }

    proptest! {
        /// Full shadow-model consistency test for OutputRouter.
        ///
        /// We drive both a real OutputRouter and a shadow HashMap<usize, String>
        /// with the same sequence of operations, then verify their outputs agree
        /// after every step. This proves that all the router's channel operations
        /// satisfy the combined concatenation + isolation + clear invariants.
        #[test]
        fn shadow_model_consistency(
            ops in prop::collection::vec(router_op_strategy(), 1..50)
        ) {
            let mut router = OutputRouter::new();
            // Shadow model maps channel index to accumulated output string.
            let mut shadow: std::collections::HashMap<usize, String> =
                std::collections::HashMap::new();

            for op in &ops {
                match op {
                    RouterOp::Write(ch_idx, data_idx) => {
                        let ch = CHANNELS[*ch_idx];
                        let data = DATA_POOL[*data_idx];
                        router.write(ch, data);
                        shadow.entry(*ch_idx).or_default().push_str(data);
                    }
                    RouterOp::Clear => {
                        router.clear();
                        shadow.clear();
                    }
                    RouterOp::ClearChannel(ch_idx) => {
                        router.clear_channel(CHANNELS[*ch_idx]);
                        shadow.remove(ch_idx);
                    }
                    RouterOp::GetOutput(ch_idx) => {
                        // Exercise the getter; actual verification is below.
                        let _ = router.get_output(CHANNELS[*ch_idx]);
                    }
                }

                // After every operation: verify every channel agrees with the shadow.
                for (i, ch) in CHANNELS.iter().enumerate() {
                    let router_val = router.get_output(ch);
                    let shadow_val = shadow.get(&i).map(String::as_str);
                    prop_assert_eq!(
                        router_val,
                        shadow_val,
                        "channel {:?} disagrees: router={:?}, shadow={:?}",
                        ch, router_val, shadow_val
                    );
                }
            }
        }
    }
}

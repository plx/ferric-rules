//! Configuration types for [`PinnedEngine`](crate::PinnedEngine).

use ferric_runtime::EngineConfig;

use crate::AutoreleasePolicy;

/// Default capacity for the worker's bounded request queue when
/// [`PinnedEngineOptions::queue_capacity`] is `0`.
pub(crate) const DEFAULT_QUEUE_CAPACITY: usize = 64;

/// Default name used for the worker thread when
/// [`PinnedEngineOptions::thread_name`] is `None`.
pub(crate) const DEFAULT_THREAD_NAME: &str = "ferric-pinned";

/// Maximum thread-name length supported by Linux (excluding NUL terminator).
/// We truncate at this length defensively so the spawn always succeeds.
const MAX_THREAD_NAME_LEN: usize = 15;

/// Configuration for a [`PinnedEngine`](crate::PinnedEngine).
///
/// Zero / `None` values are interpreted as "use default":
///
/// - `max_batch_size == 0` ⇒ drain every immediately-available request per batch.
/// - `queue_capacity == 0` ⇒ use [`DEFAULT_QUEUE_CAPACITY`].
/// - `thread_name == None` ⇒ use [`DEFAULT_THREAD_NAME`].
#[derive(Clone, Debug)]
pub struct PinnedEngineOptions {
    /// Inner engine configuration (strategy, encoding, recursion limit).
    pub engine_config: EngineConfig,
    /// Apple autorelease-pool policy. No-op on non-Apple platforms.
    pub autorelease_policy: AutoreleasePolicy,
    /// Maximum number of requests drained as one batch. `0` ⇒ drain all available.
    pub max_batch_size: usize,
    /// Bounded queue capacity. `0` ⇒ [`DEFAULT_QUEUE_CAPACITY`].
    pub queue_capacity: usize,
    /// Worker thread name. `None` ⇒ [`DEFAULT_THREAD_NAME`].
    /// Truncated at 15 bytes (Linux limit) on a valid UTF-8 boundary.
    pub thread_name: Option<String>,
}

impl Default for PinnedEngineOptions {
    fn default() -> Self {
        Self {
            engine_config: EngineConfig::default(),
            autorelease_policy: AutoreleasePolicy::None,
            max_batch_size: 0,
            queue_capacity: 0,
            thread_name: None,
        }
    }
}

/// Internal "all defaults substituted" view of [`PinnedEngineOptions`].
#[derive(Clone, Debug)]
pub(crate) struct ResolvedOptions {
    pub engine_config: EngineConfig,
    pub autorelease_policy: AutoreleasePolicy,
    /// `0` retained as sentinel for "unbounded batch drain".
    pub max_batch_size: usize,
    /// Always > 0.
    pub queue_capacity: usize,
    /// Always truncated to fit OS thread-name limits.
    pub thread_name: String,
}

impl ResolvedOptions {
    pub(crate) fn from_user(opts: PinnedEngineOptions) -> Self {
        let name = opts
            .thread_name
            .unwrap_or_else(|| DEFAULT_THREAD_NAME.to_string());
        let thread_name = truncate_on_char_boundary(name, MAX_THREAD_NAME_LEN);
        let queue_capacity = if opts.queue_capacity == 0 {
            DEFAULT_QUEUE_CAPACITY
        } else {
            opts.queue_capacity
        };
        Self {
            engine_config: opts.engine_config,
            autorelease_policy: opts.autorelease_policy,
            max_batch_size: opts.max_batch_size,
            queue_capacity,
            thread_name,
        }
    }
}

fn truncate_on_char_boundary(mut s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_substitute_for_zero_and_none() {
        let resolved = ResolvedOptions::from_user(PinnedEngineOptions::default());
        assert_eq!(resolved.queue_capacity, DEFAULT_QUEUE_CAPACITY);
        assert_eq!(resolved.max_batch_size, 0);
        assert_eq!(resolved.thread_name, DEFAULT_THREAD_NAME);
    }

    #[test]
    fn explicit_values_pass_through() {
        let resolved = ResolvedOptions::from_user(PinnedEngineOptions {
            queue_capacity: 8,
            max_batch_size: 4,
            thread_name: Some("worker".to_string()),
            ..Default::default()
        });
        assert_eq!(resolved.queue_capacity, 8);
        assert_eq!(resolved.max_batch_size, 4);
        assert_eq!(resolved.thread_name, "worker");
    }

    #[test]
    fn thread_name_truncated_to_15_bytes() {
        let resolved = ResolvedOptions::from_user(PinnedEngineOptions {
            thread_name: Some("a-very-long-thread-name".to_string()),
            ..Default::default()
        });
        assert!(resolved.thread_name.len() <= 15);
        assert_eq!(resolved.thread_name, "a-very-long-thr");
    }

    #[test]
    fn thread_name_truncates_at_char_boundary() {
        // 14 ascii chars + a 4-byte emoji would cross the boundary at byte 18;
        // truncate must back off to a valid char boundary.
        let mut name = "abcdefghijklm".to_string(); // 13 bytes
        name.push('\u{1F600}'); // 4-byte emoji ⇒ total 17 bytes
        let resolved = ResolvedOptions::from_user(PinnedEngineOptions {
            thread_name: Some(name),
            ..Default::default()
        });
        // Either 13 (drop the emoji) or 15 (if happens to land on boundary).
        // The truncate fn walks backward from 15 → must land at 13.
        assert!(resolved
            .thread_name
            .is_char_boundary(resolved.thread_name.len()));
        assert_eq!(resolved.thread_name, "abcdefghijklm");
    }
}

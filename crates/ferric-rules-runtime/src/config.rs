//! Engine configuration types.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ferric_rules_core::{ConflictResolutionStrategy, StringEncoding};

/// Default combined loop iterations and fact-query work
/// allowed while executing one rule activation.
pub const DEFAULT_MAX_ACTION_LOOP_ITERATIONS: usize = 1_000_000;

/// Engine configuration.
///
/// Includes encoding mode, conflict resolution strategy, and execution limits.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EngineConfig {
    pub string_encoding: StringEncoding,
    pub strategy: ConflictResolutionStrategy,
    /// Requested maximum call depth for user-defined functions and methods.
    ///
    /// Evaluation applies [`Self::effective_max_call_depth`], capped at 32 to
    /// bound native callable frames. Expression nesting has its own limit;
    /// deeply nested bodies can reach that limit before the callable ceiling.
    pub max_call_depth: usize,
    /// Maximum combined loop iterations and fact-query work
    /// per rule activation.
    ///
    /// The budget is shared by RHS loops and loops reached through
    /// deffunctions or generic functions. Each entered loop body and each
    /// expression-query candidate predicate consumes one iteration. Action
    /// queries charge each visited member while traversing their nested sets;
    /// delayed queries also charge each selected body. Nested evaluations share
    /// the same budget. Empty queries consume no iterations.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_max_action_loop_iterations")
    )]
    pub max_action_loop_iterations: usize,
    /// Whether structurally equivalent facts may coexist in working memory.
    ///
    /// This is interior-mutable because evaluator contexts already borrow the
    /// engine configuration immutably. The engine changes it under exclusive
    /// access; an atomic permits concurrent shared engine reads.
    fact_duplication: AtomicBool,
    /// Remaining iterations in the current action execution.
    /// Only meaningful while `action_loop_budget_active` is true.
    #[cfg_attr(feature = "serde", serde(skip, default))]
    action_loop_iterations_remaining: AtomicUsize,
    #[cfg_attr(feature = "serde", serde(skip, default))]
    action_loop_budget_active: AtomicBool,
}

#[cfg(feature = "serde")]
const fn default_max_action_loop_iterations() -> usize {
    DEFAULT_MAX_ACTION_LOOP_ITERATIONS
}

impl EngineConfig {
    /// The callable-depth ceiling used by evaluation, in every build profile.
    ///
    /// Zero disallows user-function/method calls. Larger requested values are
    /// retained in configuration and snapshots but cannot raise this ceiling.
    #[must_use]
    pub fn effective_max_call_depth(&self) -> usize {
        self.max_call_depth.min(32)
    }

    /// CLIPS-compatible strict ASCII mode with Depth strategy.
    #[must_use]
    pub fn ascii() -> Self {
        Self {
            string_encoding: StringEncoding::Ascii,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
            max_action_loop_iterations: DEFAULT_MAX_ACTION_LOOP_ITERATIONS,
            fact_duplication: AtomicBool::new(false),
            action_loop_iterations_remaining: AtomicUsize::new(0),
            action_loop_budget_active: AtomicBool::new(false),
        }
    }

    /// Full UTF-8 mode with Depth strategy.
    #[must_use]
    pub fn utf8() -> Self {
        Self {
            string_encoding: StringEncoding::Utf8,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
            max_action_loop_iterations: DEFAULT_MAX_ACTION_LOOP_ITERATIONS,
            fact_duplication: AtomicBool::new(false),
            action_loop_iterations_remaining: AtomicUsize::new(0),
            action_loop_budget_active: AtomicBool::new(false),
        }
    }

    /// Mixed mode: ASCII symbols, UTF-8 strings with Depth strategy.
    #[must_use]
    pub fn ascii_symbols_utf8_strings() -> Self {
        Self {
            string_encoding: StringEncoding::AsciiSymbolsUtf8Strings,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
            max_action_loop_iterations: DEFAULT_MAX_ACTION_LOOP_ITERATIONS,
            fact_duplication: AtomicBool::new(false),
            action_loop_iterations_remaining: AtomicUsize::new(0),
            action_loop_budget_active: AtomicBool::new(false),
        }
    }

    /// Set the conflict resolution strategy.
    #[must_use]
    pub fn with_strategy(mut self, strategy: ConflictResolutionStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the initial fact-duplication policy.
    ///
    /// CLIPS defaults this policy to disabled.
    #[must_use]
    pub fn with_fact_duplication(self, enabled: bool) -> Self {
        self.fact_duplication.store(enabled, Ordering::Relaxed);
        self
    }

    /// Return whether structurally equivalent facts may coexist.
    #[must_use]
    pub(crate) fn fact_duplication(&self) -> bool {
        self.fact_duplication.load(Ordering::Relaxed)
    }

    /// Change the fact-duplication policy and return its previous value.
    pub(crate) fn set_fact_duplication(&self, enabled: bool) -> bool {
        self.fact_duplication.swap(enabled, Ordering::Relaxed)
    }

    /// Start a fresh per-action loop budget.
    ///
    /// All budget methods are internal and run under exclusive engine access.
    /// The atomics permit shared engine reads; they do not coordinate parallel
    /// evaluations. Keep this invariant when adding evaluator entry points.
    pub(crate) fn begin_action_loop_budget(&self) {
        self.action_loop_iterations_remaining
            .store(self.max_action_loop_iterations, Ordering::Relaxed);
        self.action_loop_budget_active
            .store(true, Ordering::Relaxed);
    }

    /// Start a loop budget only when no enclosing action/evaluation owns one.
    ///
    /// Returns whether this call started the budget.
    pub(crate) fn begin_action_loop_budget_if_inactive(&self) -> bool {
        if self.action_loop_budget_active.load(Ordering::Relaxed) {
            false
        } else {
            self.begin_action_loop_budget();
            true
        }
    }

    /// Finish the active per-action loop budget.
    pub(crate) fn end_action_loop_budget(&self) {
        self.action_loop_budget_active
            .store(false, Ordering::Relaxed);
    }

    /// Consume one iteration, returning `false` when the active budget is
    /// exhausted.
    pub(crate) fn take_action_loop_iteration(&self) -> bool {
        if !self.action_loop_budget_active.load(Ordering::Relaxed) {
            debug_assert!(false, "loop iteration consumed without an active budget");
            return false;
        }
        let remaining = self
            .action_loop_iterations_remaining
            .load(Ordering::Relaxed);
        let Some(next) = remaining.checked_sub(1) else {
            return false;
        };
        self.action_loop_iterations_remaining
            .store(next, Ordering::Relaxed);
        true
    }
}

// Configuration clones own independent evaluator state. Relaxed atomics are
// sufficient: no field publishes other memory, and Engine mutation still needs
// &mut Engine. EvalContexts cannot currently be constructed outside the crate;
// internal concurrent evaluations must use independent configuration values.
impl Clone for EngineConfig {
    fn clone(&self) -> Self {
        Self {
            string_encoding: self.string_encoding,
            strategy: self.strategy,
            max_call_depth: self.max_call_depth,
            max_action_loop_iterations: self.max_action_loop_iterations,
            fact_duplication: AtomicBool::new(self.fact_duplication()),
            action_loop_iterations_remaining: AtomicUsize::new(
                self.action_loop_iterations_remaining
                    .load(Ordering::Relaxed),
            ),
            action_loop_budget_active: AtomicBool::new(
                self.action_loop_budget_active.load(Ordering::Relaxed),
            ),
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self::utf8()
    }
}

impl From<StringEncoding> for EngineConfig {
    fn from(string_encoding: StringEncoding) -> Self {
        Self {
            string_encoding,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
            max_action_loop_iterations: DEFAULT_MAX_ACTION_LOOP_ITERATIONS,
            fact_duplication: AtomicBool::new(false),
            action_loop_iterations_remaining: AtomicUsize::new(0),
            action_loop_budget_active: AtomicBool::new(false),
        }
    }
}

impl From<EngineConfig> for StringEncoding {
    fn from(config: EngineConfig) -> Self {
        config.string_encoding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_encoding_is_utf8() {
        assert_eq!(
            EngineConfig::default().string_encoding,
            StringEncoding::Utf8
        );
    }

    #[test]
    fn ascii_config() {
        assert_eq!(EngineConfig::ascii().string_encoding, StringEncoding::Ascii);
    }

    #[test]
    fn mixed_config() {
        assert_eq!(
            EngineConfig::ascii_symbols_utf8_strings().string_encoding,
            StringEncoding::AsciiSymbolsUtf8Strings
        );
    }

    #[test]
    fn from_string_encoding() {
        let config = EngineConfig::from(StringEncoding::Ascii);
        assert_eq!(config.string_encoding, StringEncoding::Ascii);
    }

    #[test]
    fn into_string_encoding() {
        let encoding: StringEncoding = EngineConfig::ascii_symbols_utf8_strings().into();
        assert_eq!(encoding, StringEncoding::AsciiSymbolsUtf8Strings);
    }

    #[test]
    fn default_action_loop_budget_is_one_million() {
        assert_eq!(
            EngineConfig::default().max_action_loop_iterations,
            DEFAULT_MAX_ACTION_LOOP_ITERATIONS
        );
    }

    // -----------------------------------------------------------------------
    // Property-based tests
    // -----------------------------------------------------------------------

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_encoding() -> impl Strategy<Value = StringEncoding> {
            prop_oneof![
                Just(StringEncoding::Ascii),
                Just(StringEncoding::Utf8),
                Just(StringEncoding::AsciiSymbolsUtf8Strings),
            ]
        }

        fn arb_strategy() -> impl Strategy<Value = ConflictResolutionStrategy> {
            prop_oneof![
                Just(ConflictResolutionStrategy::Depth),
                Just(ConflictResolutionStrategy::Breadth),
                Just(ConflictResolutionStrategy::Lex),
                Just(ConflictResolutionStrategy::Mea),
            ]
        }

        proptest! {
            /// `From<StringEncoding>` roundtrip: encoding survives conversion
            /// to config and back.
            #[test]
            fn encoding_roundtrip(enc in arb_encoding()) {
                let config = EngineConfig::from(enc);
                let recovered: StringEncoding = config.into();
                prop_assert_eq!(recovered, enc);
            }

            /// `with_strategy` preserves the encoding and execution limits.
            #[test]
            fn with_strategy_preserves_other_fields(
                enc in arb_encoding(),
                strategy in arb_strategy(),
            ) {
                let base = EngineConfig::from(enc);
                let original_depth = base.max_call_depth;
                let original_loop_budget = base.max_action_loop_iterations;
                let modified = base.with_strategy(strategy);
                prop_assert_eq!(modified.string_encoding, enc);
                prop_assert_eq!(modified.strategy, strategy);
                prop_assert_eq!(modified.max_call_depth, original_depth);
                prop_assert_eq!(
                    modified.max_action_loop_iterations,
                    original_loop_budget
                );
            }

            /// Named constructors always produce the advertised encoding.
            #[test]
            fn named_constructors_correct_encoding(choice in 0..3_u8) {
                let (config, expected) = match choice {
                    0 => (EngineConfig::ascii(), StringEncoding::Ascii),
                    1 => (EngineConfig::utf8(), StringEncoding::Utf8),
                    _ => (EngineConfig::ascii_symbols_utf8_strings(), StringEncoding::AsciiSymbolsUtf8Strings),
                };
                prop_assert_eq!(config.string_encoding, expected);
                prop_assert_eq!(config.max_call_depth, 64);
                prop_assert_eq!(
                    config.max_action_loop_iterations,
                    DEFAULT_MAX_ACTION_LOOP_ITERATIONS
                );
            }
        }
    }
}

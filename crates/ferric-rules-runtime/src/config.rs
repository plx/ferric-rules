//! Engine configuration types.

use std::cell::Cell;

use ferric_rules_core::{ConflictResolutionStrategy, StringEncoding};

/// Default number of `while` and `loop-for-count` iterations allowed while
/// executing one rule activation.
pub const DEFAULT_MAX_ACTION_LOOP_ITERATIONS: usize = 1_000_000;

/// Engine configuration.
///
/// Includes encoding mode, conflict resolution strategy, and execution limits.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EngineConfig {
    pub string_encoding: StringEncoding,
    pub strategy: ConflictResolutionStrategy,
    /// Maximum call depth for user-defined function recursion.
    ///
    /// Calls that exceed this depth return a `RecursionLimit` error rather than
    /// overflowing the stack.
    pub max_call_depth: usize,
    /// Maximum combined `while` and `loop-for-count` iterations per rule
    /// activation.
    ///
    /// The budget is shared by RHS loops and loops reached through
    /// deffunctions or generic functions. Each entered loop body consumes one
    /// iteration, including nested loop bodies.
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_max_action_loop_iterations")
    )]
    pub max_action_loop_iterations: usize,
    /// Whether structurally equivalent facts may coexist in working memory.
    ///
    /// This is interior-mutable because evaluator contexts already borrow the
    /// engine configuration immutably. `Engine` is thread-affine, so mutation
    /// through `Cell` does not weaken its concurrency contract.
    fact_duplication: Cell<bool>,
    /// Remaining iterations in the current action execution, or `None` when
    /// no action/evaluator root owns a budget.
    #[cfg_attr(feature = "serde", serde(skip, default))]
    action_loop_iterations_remaining: Cell<Option<usize>>,
}

#[cfg(feature = "serde")]
const fn default_max_action_loop_iterations() -> usize {
    DEFAULT_MAX_ACTION_LOOP_ITERATIONS
}

impl EngineConfig {
    /// CLIPS-compatible strict ASCII mode with Depth strategy.
    #[must_use]
    pub fn ascii() -> Self {
        Self {
            string_encoding: StringEncoding::Ascii,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
            max_action_loop_iterations: DEFAULT_MAX_ACTION_LOOP_ITERATIONS,
            fact_duplication: Cell::new(false),
            action_loop_iterations_remaining: Cell::new(None),
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
            fact_duplication: Cell::new(false),
            action_loop_iterations_remaining: Cell::new(None),
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
            fact_duplication: Cell::new(false),
            action_loop_iterations_remaining: Cell::new(None),
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
        self.fact_duplication.set(enabled);
        self
    }

    /// Return whether structurally equivalent facts may coexist.
    #[must_use]
    pub(crate) fn fact_duplication(&self) -> bool {
        self.fact_duplication.get()
    }

    /// Change the fact-duplication policy and return its previous value.
    pub(crate) fn set_fact_duplication(&self, enabled: bool) -> bool {
        self.fact_duplication.replace(enabled)
    }

    /// Start a fresh per-action loop budget.
    pub(crate) fn begin_action_loop_budget(&self) {
        self.action_loop_iterations_remaining
            .set(Some(self.max_action_loop_iterations));
    }

    /// Start a loop budget only when no enclosing action/evaluation owns one.
    ///
    /// Returns whether this call started the budget.
    pub(crate) fn begin_action_loop_budget_if_inactive(&self) -> bool {
        if self.action_loop_iterations_remaining.get().is_some() {
            false
        } else {
            self.begin_action_loop_budget();
            true
        }
    }

    /// Finish the active per-action loop budget.
    pub(crate) fn end_action_loop_budget(&self) {
        self.action_loop_iterations_remaining.set(None);
    }

    /// Consume one iteration, returning `false` when the active budget is
    /// exhausted.
    pub(crate) fn take_action_loop_iteration(&self) -> bool {
        let Some(remaining) = self.action_loop_iterations_remaining.get() else {
            debug_assert!(false, "loop iteration consumed without an active budget");
            return false;
        };
        let Some(next) = remaining.checked_sub(1) else {
            return false;
        };
        self.action_loop_iterations_remaining.set(Some(next));
        true
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
            fact_duplication: Cell::new(false),
            action_loop_iterations_remaining: Cell::new(None),
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

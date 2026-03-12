//! Engine configuration types.

use ferric_core::{ConflictResolutionStrategy, StringEncoding};

/// Engine configuration.
///
/// Includes encoding mode, conflict resolution strategy, and recursion limits.
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
}

impl EngineConfig {
    /// CLIPS-compatible strict ASCII mode with Depth strategy.
    #[must_use]
    pub fn ascii() -> Self {
        Self {
            string_encoding: StringEncoding::Ascii,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
        }
    }

    /// Full UTF-8 mode with Depth strategy.
    #[must_use]
    pub fn utf8() -> Self {
        Self {
            string_encoding: StringEncoding::Utf8,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
        }
    }

    /// Mixed mode: ASCII symbols, UTF-8 strings with Depth strategy.
    #[must_use]
    pub fn ascii_symbols_utf8_strings() -> Self {
        Self {
            string_encoding: StringEncoding::AsciiSymbolsUtf8Strings,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 64,
        }
    }

    /// Set the conflict resolution strategy.
    #[must_use]
    pub fn with_strategy(mut self, strategy: ConflictResolutionStrategy) -> Self {
        self.strategy = strategy;
        self
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

            /// `with_strategy` preserves the encoding and max_call_depth.
            #[test]
            fn with_strategy_preserves_other_fields(
                enc in arb_encoding(),
                strategy in arb_strategy(),
            ) {
                let base = EngineConfig::from(enc);
                let original_depth = base.max_call_depth;
                let modified = base.with_strategy(strategy);
                prop_assert_eq!(modified.string_encoding, enc);
                prop_assert_eq!(modified.strategy, strategy);
                prop_assert_eq!(modified.max_call_depth, original_depth);
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
            }
        }
    }
}

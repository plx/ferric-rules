//! Engine configuration types.

use ferric_core::{ConflictResolutionStrategy, StringEncoding};

/// Engine configuration.
///
/// Includes encoding mode, conflict resolution strategy, and recursion limits.
#[derive(Clone, Debug)]
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
            max_call_depth: 256,
        }
    }

    /// Full UTF-8 mode with Depth strategy.
    #[must_use]
    pub fn utf8() -> Self {
        Self {
            string_encoding: StringEncoding::Utf8,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 256,
        }
    }

    /// Mixed mode: ASCII symbols, UTF-8 strings with Depth strategy.
    #[must_use]
    pub fn ascii_symbols_utf8_strings() -> Self {
        Self {
            string_encoding: StringEncoding::AsciiSymbolsUtf8Strings,
            strategy: ConflictResolutionStrategy::default(),
            max_call_depth: 256,
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
            max_call_depth: 256,
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
}

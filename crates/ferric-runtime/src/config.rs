//! Engine configuration types, focused on encoding mode for Phase 1.

use ferric_core::StringEncoding;

/// Engine configuration (Phase 1 subset — encoding mode only).
///
/// Additional fields will be added in later passes as engine capabilities expand.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub string_encoding: StringEncoding,
}

impl EngineConfig {
    /// CLIPS-compatible strict ASCII mode.
    #[must_use]
    pub fn ascii() -> Self {
        Self {
            string_encoding: StringEncoding::Ascii,
        }
    }

    /// Full UTF-8 mode.
    #[must_use]
    pub fn utf8() -> Self {
        Self {
            string_encoding: StringEncoding::Utf8,
        }
    }

    /// Mixed mode: ASCII symbols, UTF-8 strings.
    #[must_use]
    pub fn ascii_symbols_utf8_strings() -> Self {
        Self {
            string_encoding: StringEncoding::AsciiSymbolsUtf8Strings,
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
        Self { string_encoding }
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

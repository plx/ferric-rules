//! Text encoding mode and related errors.
//!
//! Controls what byte sequences are accepted when creating symbols and strings.
//! See Section 2.4 of the implementation plan.

use thiserror::Error;

/// Text encoding mode for symbols and strings.
///
/// Controls what byte sequences are accepted when creating symbols and strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StringEncoding {
    /// ASCII only for both symbols and strings. Maximum CLIPS compatibility.
    Ascii,
    /// UTF-8 for both symbols and strings. Full internationalization.
    #[default]
    Utf8,
    /// ASCII-only symbols, UTF-8 strings. Identifiers remain ASCII, text data is modern.
    AsciiSymbolsUtf8Strings,
}

/// Errors arising from encoding mode enforcement.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EncodingError {
    #[error("non-ASCII symbol: {0:?}")]
    NonAsciiSymbol(String),

    #[error("non-ASCII string: {0:?}")]
    NonAsciiString(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_encoding_is_utf8() {
        assert_eq!(StringEncoding::default(), StringEncoding::Utf8);
    }

    #[test]
    fn encoding_modes_distinct() {
        assert_ne!(StringEncoding::Ascii, StringEncoding::Utf8);
        assert_ne!(
            StringEncoding::Ascii,
            StringEncoding::AsciiSymbolsUtf8Strings
        );
    }
}

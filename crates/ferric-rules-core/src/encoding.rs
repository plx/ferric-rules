//! Text encoding mode and related errors.
//!
//! Text constructors validate their input under the chosen mode. Explicit byte
//! APIs preserve arbitrary data in permissive modes; strict ASCII policies also
//! apply to stored values, including values restored from snapshots.

use thiserror::Error;

/// Text encoding mode for symbols and strings.
///
/// Text and explicit byte constructors share the strict ASCII restrictions.
/// Permissive byte construction does not promise a valid UTF-8 stored payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StringEncoding {
    /// Strict ASCII strings, symbols, and instance names, including byte APIs.
    Ascii,
    /// UTF-8 text constructors; explicit byte APIs also preserve invalid UTF-8
    /// STRING, SYMBOL, and INSTANCE-NAME payloads.
    #[default]
    Utf8,
    /// Strict ASCII symbols and instance names. Text strings accept UTF-8;
    /// explicit byte STRING construction also preserves invalid UTF-8.
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

    #[error("non-ASCII symbol bytes: {0:?}")]
    NonAsciiSymbolBytes(Vec<u8>),

    #[error("non-ASCII string bytes: {0:?}")]
    NonAsciiStringBytes(Vec<u8>),
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

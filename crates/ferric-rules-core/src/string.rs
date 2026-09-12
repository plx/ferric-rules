//! Encoding-aware string type for Ferric.
//!
//! `FerricString` stores the string data along with its encoding variant.
//! Equality is exact byte equality; ordering is lexicographic by byte value.
//! No Unicode normalization is performed (see Section 2.4.1).

use crate::encoding::{EncodingError, StringEncoding};

/// A string value with encoding awareness.
///
/// Not interned; comparison is by value (exact byte equality).
/// Ordering is lexicographic by byte value, which for UTF-8 is equivalent
/// to ordering by Unicode scalar value.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FerricString {
    /// ASCII-only string (all bytes are in 0..=127).
    Ascii(Box<[u8]>),
    /// UTF-8 string.
    Utf8(Box<str>),
    /// Byte string whose payload is not valid UTF-8.
    Bytes(Box<[u8]>),
}

impl FerricString {
    /// Create a `FerricString` according to the given encoding mode.
    ///
    /// In `Ascii` mode, rejects non-ASCII input.
    /// In `Utf8` or `AsciiSymbolsUtf8Strings` mode, accepts any valid UTF-8.
    pub fn new(s: &str, encoding: StringEncoding) -> Result<Self, EncodingError> {
        match encoding {
            StringEncoding::Ascii => {
                if !s.is_ascii() {
                    return Err(EncodingError::NonAsciiString(s.to_string()));
                }
                Ok(Self::Ascii(s.as_bytes().into()))
            }
            StringEncoding::Utf8 | StringEncoding::AsciiSymbolsUtf8Strings => {
                Ok(Self::Utf8(s.into()))
            }
        }
    }

    /// Construct an exact byte string. Strict ASCII mode still rejects non-ASCII.
    /// Other modes preserve invalid UTF-8 through the explicit byte API.
    pub fn from_bytes(bytes: &[u8], encoding: StringEncoding) -> Result<Self, EncodingError> {
        match std::str::from_utf8(bytes) {
            Ok(text) => Self::new(text, encoding),
            Err(_) if encoding == StringEncoding::Ascii => {
                Err(EncodingError::NonAsciiStringBytes(bytes.to_vec()))
            }
            Err(_) => Ok(Self::Bytes(bytes.into())),
        }
    }

    /// Returns the string content as a byte slice.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Ascii(bytes) | Self::Bytes(bytes) => bytes,
            Self::Utf8(s) => s.as_bytes(),
        }
    }

    /// Return exact UTF-8 text, or a decoding error for a byte payload.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(self.as_bytes())
    }

    /// Returns the length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Returns `true` if the string is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// Equality is exact byte equality (Section 2.4.1).
impl PartialEq for FerricString {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for FerricString {}

// Ordering is lexicographic by byte value (Section 2.4.1).
impl PartialOrd for FerricString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FerricString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl std::hash::Hash for FerricString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

/// Diagnostic display: invalid UTF-8 bytes are escaped. Use `as_bytes` for
/// value transport and CLIPS output; diagnostic spelling is not serialization.
impl std::fmt::Display for FerricString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Ok(text) = self.as_str() {
            f.write_str(text)
        } else {
            for byte in self.as_bytes().escape_ascii() {
                write!(f, "{}", char::from(byte))?;
            }
            Ok(())
        }
    }
}

impl AsRef<[u8]> for FerricString {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl std::borrow::Borrow<[u8]> for FerricString {
    fn borrow(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_mode_accepts_ascii() {
        let s = FerricString::new("hello", StringEncoding::Ascii).unwrap();
        assert_eq!(s.as_str().unwrap(), "hello");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn ascii_mode_rejects_non_ascii() {
        let result = FerricString::new("héllo", StringEncoding::Ascii);
        assert!(result.is_err());
        assert!(matches!(result, Err(EncodingError::NonAsciiString(_))));
    }

    #[test]
    fn utf8_mode_accepts_unicode() {
        let s = FerricString::new("héllo 世界", StringEncoding::Utf8).unwrap();
        assert_eq!(s.as_str().unwrap(), "héllo 世界");
    }

    #[test]
    fn mixed_mode_accepts_unicode_strings() {
        let s = FerricString::new("héllo 世界", StringEncoding::AsciiSymbolsUtf8Strings).unwrap();
        assert_eq!(s.as_str().unwrap(), "héllo 世界");
    }

    #[test]
    fn empty_string() {
        let s = FerricString::new("", StringEncoding::Ascii).unwrap();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn byte_equality_no_normalization() {
        // Composed form: é (U+00E9, single codepoint)
        let composed = FerricString::new("\u{00E9}", StringEncoding::Utf8).unwrap();
        // Decomposed form: e + combining accent (U+0065 U+0301)
        let decomposed = FerricString::new("\u{0065}\u{0301}", StringEncoding::Utf8).unwrap();
        // These look the same visually but have different byte representations.
        assert_ne!(composed, decomposed, "no implicit Unicode normalization");
    }

    #[test]
    fn ordering_is_lexicographic_by_bytes() {
        let a = FerricString::new("abc", StringEncoding::Utf8).unwrap();
        let b = FerricString::new("abd", StringEncoding::Utf8).unwrap();
        assert!(a < b);

        let c = FerricString::new("ab", StringEncoding::Utf8).unwrap();
        assert!(c < a, "shorter prefix is less");
    }

    #[test]
    fn cross_variant_equality() {
        // An ASCII string and a UTF-8 string with the same content should be equal,
        // because equality is by byte content.
        let ascii = FerricString::Ascii(b"hello"[..].into());
        let utf8 = FerricString::Utf8("hello".into());
        assert_eq!(ascii, utf8);
    }

    #[test]
    fn cross_variant_ordering() {
        let ascii = FerricString::Ascii(b"abc"[..].into());
        let utf8 = FerricString::Utf8("abd".into());
        assert!(ascii < utf8);
    }

    #[test]
    fn hash_consistency_across_variants() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let ascii = FerricString::Ascii(b"hello"[..].into());
        let utf8 = FerricString::Utf8("hello".into());

        let hash = |v: &FerricString| {
            let mut h = DefaultHasher::new();
            v.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&ascii), hash(&utf8));
    }
}

#[cfg(test)]
mod byte_tests {
    use super::*;

    #[test]
    fn exact_byte_strings_preserve_invalid_utf8_and_nul() {
        for bytes in [&b"\xc3"[..], &b"a\xff\0z"[..]] {
            let string = FerricString::from_bytes(bytes, StringEncoding::Utf8).unwrap();
            assert_eq!(string.as_bytes(), bytes);
            assert!(string.as_str().is_err());
            assert!(matches!(string, FerricString::Bytes(_)));
        }
        let nul = FerricString::from_bytes(b"a\0z", StringEncoding::Utf8).unwrap();
        assert_eq!(nul.as_str().unwrap().as_bytes(), b"a\0z");
    }

    #[test]
    fn byte_constructor_preserves_text_and_strict_ascii_policy() {
        for text in ["", "ASCII", "é"] {
            let string = FerricString::from_bytes(text.as_bytes(), StringEncoding::Utf8).unwrap();
            assert!(matches!(string, FerricString::Utf8(_)));
            assert_eq!(
                string,
                FerricString::new(text, StringEncoding::Utf8).unwrap()
            );
        }
        assert!(FerricString::from_bytes(b"\xff", StringEncoding::Ascii).is_err());
        assert!(FerricString::from_bytes("é".as_bytes(), StringEncoding::Ascii).is_err());
        assert!(FerricString::from_bytes(b"\xff", StringEncoding::AsciiSymbolsUtf8Strings).is_ok());
    }

    #[test]
    fn byte_hash_and_order_use_the_complete_payload() {
        use std::collections::HashSet;
        let low = FerricString::from_bytes(b"a\0\xc3", StringEncoding::Utf8).unwrap();
        let high = FerricString::from_bytes(b"a\0\xff", StringEncoding::Utf8).unwrap();
        assert!(low < high);
        let mut values = HashSet::new();
        values.insert(low.clone());
        values.insert(high.clone());
        assert_eq!(values.len(), 2);
        assert!(values.contains(low.as_bytes()));
        assert!(values.contains(high.as_bytes()));
    }
}

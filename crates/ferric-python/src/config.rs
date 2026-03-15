//! Configuration enums for Python bindings.

use pyo3::prelude::*;

/// Conflict resolution strategy.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Depth-first (CLIPS default).
    #[pyo3(name = "DEPTH")]
    Depth = 0,
    /// Breadth-first.
    #[pyo3(name = "BREADTH")]
    Breadth = 1,
    /// LEX (lexicographic recency).
    #[pyo3(name = "LEX")]
    Lex = 2,
    /// MEA (means-ends analysis).
    #[pyo3(name = "MEA")]
    Mea = 3,
}

/// String encoding mode.
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// ASCII-only encoding.
    #[pyo3(name = "ASCII")]
    Ascii = 0,
    /// UTF-8 encoding (default).
    #[pyo3(name = "UTF8")]
    Utf8 = 1,
    /// ASCII for symbols, UTF-8 for strings.
    #[pyo3(name = "ASCII_SYMBOLS_UTF8_STRINGS")]
    AsciiSymbolsUtf8Strings = 2,
}

impl From<Strategy> for ferric_core::ConflictResolutionStrategy {
    fn from(s: Strategy) -> Self {
        match s {
            Strategy::Depth => Self::Depth,
            Strategy::Breadth => Self::Breadth,
            Strategy::Lex => Self::Lex,
            Strategy::Mea => Self::Mea,
        }
    }
}

impl From<Encoding> for ferric_core::StringEncoding {
    fn from(e: Encoding) -> Self {
        match e {
            Encoding::Ascii => Self::Ascii,
            Encoding::Utf8 => Self::Utf8,
            Encoding::AsciiSymbolsUtf8Strings => Self::AsciiSymbolsUtf8Strings,
        }
    }
}

/// Serialization format for engine snapshots.
#[cfg(feature = "serde")]
#[pyclass(eq, eq_int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Compact binary (bincode). Fast and small.
    #[pyo3(name = "BINCODE")]
    Bincode = 0,
    /// JSON (human-readable, larger output).
    #[pyo3(name = "JSON")]
    Json = 1,
    /// CBOR (Concise Binary Object Representation).
    #[pyo3(name = "CBOR")]
    Cbor = 2,
    /// `MessagePack` (compact binary, JSON-like schema).
    #[pyo3(name = "MSGPACK")]
    MessagePack = 3,
    /// Postcard (compact, no_std-friendly binary).
    #[pyo3(name = "POSTCARD")]
    Postcard = 4,
}

#[cfg(feature = "serde")]
impl From<Format> for ferric_runtime::SerializationFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Bincode => Self::Bincode,
            Format::Json => Self::Json,
            Format::Cbor => Self::Cbor,
            Format::MessagePack => Self::MessagePack,
            Format::Postcard => Self::Postcard,
        }
    }
}

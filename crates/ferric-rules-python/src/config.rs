//! Configuration enums for Python bindings.

use pyo3::prelude::*;

/// Conflict resolution strategy.
#[pyclass(from_py_object, eq, eq_int, module = "ferric")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Depth-first (CLIPS default).
    #[pyo3(name = "DEPTH")]
    Depth = 0,
    /// Breadth-first.
    #[pyo3(name = "BREADTH")]
    Breadth = 1,
    /// CLIPS LEX: sorted fact recencies, specificity, then older activations.
    #[pyo3(name = "LEX")]
    Lex = 2,
    /// CLIPS MEA: first-pattern recency, then the LEX comparison.
    #[pyo3(name = "MEA")]
    Mea = 3,
}

/// String encoding mode.
#[pyclass(from_py_object, eq, eq_int, module = "ferric")]
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

impl From<Strategy> for ferric_rules_core::ConflictResolutionStrategy {
    fn from(s: Strategy) -> Self {
        match s {
            Strategy::Depth => Self::Depth,
            Strategy::Breadth => Self::Breadth,
            Strategy::Lex => Self::Lex,
            Strategy::Mea => Self::Mea,
        }
    }
}

impl From<Encoding> for ferric_rules_core::StringEncoding {
    fn from(e: Encoding) -> Self {
        match e {
            Encoding::Ascii => Self::Ascii,
            Encoding::Utf8 => Self::Utf8,
            Encoding::AsciiSymbolsUtf8Strings => Self::AsciiSymbolsUtf8Strings,
        }
    }
}

/// Serialization format for engine snapshots.
///
/// Values 0, 3 and 4 belonged to removed codecs and are not reused.
#[cfg(feature = "serde")]
#[pyclass(from_py_object, eq, eq_int, module = "ferric")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// JSON (human-readable, larger output; for debugging and inspection).
    #[pyo3(name = "JSON")]
    Json = 1,
    /// CBOR (Concise Binary Object Representation). Recommended default.
    #[pyo3(name = "CBOR")]
    Cbor = 2,
}

#[cfg(feature = "serde")]
impl From<Format> for ferric_rules_runtime::SerializationFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Json => Self::Json,
            Format::Cbor => Self::Cbor,
        }
    }
}

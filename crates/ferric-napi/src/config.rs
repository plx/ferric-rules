//! Configuration enums for the Node.js binding.

use napi_derive::napi;

/// Conflict resolution strategy for the Rete agenda.
#[napi]
pub enum Strategy {
    /// Depth-first (CLIPS default).
    Depth = 0,
    /// Breadth-first.
    Breadth = 1,
    /// LEX (lexicographic recency).
    Lex = 2,
    /// MEA (means-ends analysis).
    Mea = 3,
}

/// String encoding constraint for symbols and strings.
#[napi]
pub enum Encoding {
    /// ASCII-only.
    Ascii = 0,
    /// UTF-8 (default).
    Utf8 = 1,
    /// ASCII symbols, UTF-8 strings.
    AsciiSymbolsUtf8Strings = 2,
}

/// Serialization format for engine snapshots.
#[cfg(feature = "serde")]
#[napi]
pub enum Format {
    /// Compact binary (bincode). Fast and small.
    Bincode = 0,
    /// JSON (human-readable).
    Json = 1,
    /// CBOR.
    Cbor = 2,
    /// `MessagePack`.
    MessagePack = 3,
    /// Postcard.
    Postcard = 4,
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

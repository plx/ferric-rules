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

#[cfg(feature = "serde")]
impl From<Format> for ferric_rules_runtime::SerializationFormat {
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

/// Validate before napi's integer extraction can truncate or wrap JS numbers.
pub fn checked_u32(value: f64, name: &str) -> napi::Result<u32> {
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&value) {
        return Err(napi::Error::new(
            napi::Status::InvalidArg,
            format!("{name} must be an integer in 0..=4294967295"),
        ));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(value as u32)
}

impl TryFrom<f64> for Strategy {
    type Error = napi::Error;
    fn try_from(value: f64) -> napi::Result<Self> {
        match checked_u32(value, "strategy")? {
            0 => Ok(Self::Depth),
            1 => Ok(Self::Breadth),
            2 => Ok(Self::Lex),
            3 => Ok(Self::Mea),
            _ => Err(napi::Error::new(
                napi::Status::InvalidArg,
                "unknown strategy",
            )),
        }
    }
}

impl TryFrom<f64> for Encoding {
    type Error = napi::Error;
    fn try_from(value: f64) -> napi::Result<Self> {
        match checked_u32(value, "encoding")? {
            0 => Ok(Self::Ascii),
            1 => Ok(Self::Utf8),
            2 => Ok(Self::AsciiSymbolsUtf8Strings),
            _ => Err(napi::Error::new(
                napi::Status::InvalidArg,
                "unknown encoding",
            )),
        }
    }
}

#[cfg(feature = "serde")]
impl TryFrom<f64> for Format {
    type Error = napi::Error;
    fn try_from(value: f64) -> napi::Result<Self> {
        match checked_u32(value, "snapshot format")? {
            0 => Ok(Self::Bincode),
            1 => Ok(Self::Json),
            2 => Ok(Self::Cbor),
            3 => Ok(Self::MessagePack),
            4 => Ok(Self::Postcard),
            _ => Err(napi::Error::new(
                napi::Status::InvalidArg,
                "unknown snapshot format",
            )),
        }
    }
}

#[cfg(feature = "serde")]
pub fn snapshot_format(
    value: Option<f64>,
) -> napi::Result<ferric_rules_runtime::SerializationFormat> {
    Ok(value
        .map(Format::try_from)
        .transpose()?
        .unwrap_or(Format::Cbor)
        .into())
}

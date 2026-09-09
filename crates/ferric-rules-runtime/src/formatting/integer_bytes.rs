//! Numeric and byte renderers for already parsed, canonical directives.
//! No format parser, evaluation, router, float formatter, or error-flag policy.

use super::argument::FormatArgument;
use super::format_types::{FormatSpec, OutputLimit};
use crate::byte_buffer::ByteBuffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntegerConversion {
    Decimal,
    Octal,
    Hex,
    Unsigned,
}

/// Classification only. The evaluator must not equate the unmeasured C
/// float-cast boundary with an ordinary CLIPS type error without evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntegerArgumentError {
    NotNumeric(&'static str),
    NonFinite,
    OutsideSignedRange,
}

pub(crate) fn integer_argument(value: FormatArgument<'_>) -> Result<i64, IntegerArgumentError> {
    match value {
        FormatArgument::Integer(value) => Ok(value),
        FormatArgument::Float(value) => {
            if !value.is_finite() {
                return Err(IntegerArgumentError::NonFinite);
            }
            let truncated = value.trunc();
            // Upper endpoint is exclusive: i64::MAX as f64 rounds to 2^63.
            if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&truncated) {
                return Err(IntegerArgumentError::OutsideSignedRange);
            }
            #[allow(clippy::cast_possible_truncation)]
            Ok(truncated as i64)
        }
        other => Err(IntegerArgumentError::NotNumeric(other.type_name())),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ByteArgumentError {
    ExpectedLexeme(&'static str),
    ExpectedCharacter(&'static str),
}

/// Values here already contain validated borrowed bytes; the runtime adapter
/// must resolve symbol/name ownership before constructing the argument view.
pub(crate) fn lexeme_argument(value: FormatArgument<'_>) -> Result<&[u8], ByteArgumentError> {
    match value {
        FormatArgument::String(bytes)
        | FormatArgument::Symbol(bytes)
        | FormatArgument::InstanceName(bytes) => Ok(bytes),
        other => Err(ByteArgumentError::ExpectedLexeme(other.type_name())),
    }
}

/// Unlike %s, the source's explicit %c admission excludes INSTANCE-NAME/FLOAT.
/// This does not choose the diagnostic severity of an admission failure.
pub(crate) fn character_argument(value: FormatArgument<'_>) -> Result<u8, ByteArgumentError> {
    let bytes = match value {
        FormatArgument::Integer(value) => return Ok(value.to_le_bytes()[0]),
        FormatArgument::String(bytes) | FormatArgument::Symbol(bytes) => bytes,
        other => return Err(ByteArgumentError::ExpectedCharacter(other.type_name())),
    };
    Ok(bytes.first().copied().unwrap_or(0))
}

fn check_append(out: &ByteBuffer, amount: usize, allowed: usize) -> Result<(), OutputLimit> {
    let required = out.as_bytes().len().checked_add(amount);
    match required {
        Some(required) if required <= allowed => Ok(()),
        _ => Err(OutputLimit { required, allowed }),
    }
}

fn repeat(out: &mut ByteBuffer, byte: u8, mut count: usize) {
    let block = [byte; 64];
    while count >= block.len() {
        out.push_bytes(&block);
        count -= block.len();
    }
    out.push_bytes(&block[..count]);
}

/// Render a signed conversion input. d keeps sign and unsigned magnitude;
/// o/x/u reinterpret all 64 bits after the source's signed numeric conversion.
pub(crate) fn render_integer(
    out: &mut ByteBuffer,
    conversion: IntegerConversion,
    value: i64,
    spec: FormatSpec,
    allowed: usize,
) -> Result<(), OutputLimit> {
    let negative = conversion == IntegerConversion::Decimal && value < 0;
    let mut magnitude = if conversion == IntegerConversion::Decimal {
        value.unsigned_abs()
    } else {
        u64::from_ne_bytes(value.to_ne_bytes())
    };
    let radix = match conversion {
        IntegerConversion::Octal => 8,
        IntegerConversion::Hex => 16,
        IntegerConversion::Decimal | IntegerConversion::Unsigned => 10,
    };
    let mut storage = [0; 64];
    let mut start = storage.len();
    loop {
        start -= 1;
        let digit = usize::try_from(magnitude % radix).expect("radix digit fits usize");
        storage[start] = b"0123456789abcdef"[digit];
        magnitude /= radix;
        if magnitude == 0 {
            break;
        }
    }
    let digits = if value == 0 && spec.precision == Some(0) {
        &[][..]
    } else {
        &storage[start..]
    };
    let precision_zeroes = spec.precision.unwrap_or(0).saturating_sub(digits.len());
    let sign_len = usize::from(negative);
    let body_len = digits
        .len()
        .checked_add(precision_zeroes)
        .and_then(|length| length.checked_add(sign_len))
        .ok_or(OutputLimit {
            required: None,
            allowed,
        })?;
    let field_len = spec.width.max(body_len);
    check_append(out, field_len, allowed)?;
    let width_padding = field_len - body_len;
    let width_zeroes = spec.zero_pad && !spec.left_align && spec.precision.is_none();
    if !spec.left_align && !width_zeroes {
        repeat(out, b' ', width_padding);
    }
    if negative {
        out.push_bytes(b"-");
    }
    if width_zeroes {
        repeat(out, b'0', width_padding);
    }
    repeat(out, b'0', precision_zeroes);
    out.push_bytes(digits);
    if spec.left_align {
        repeat(out, b' ', width_padding);
    }
    Ok(())
}

/// %s applies strlen before optional byte precision. Zero fill is ignored.
pub(crate) fn render_lexeme(
    out: &mut ByteBuffer,
    bytes: &[u8],
    spec: FormatSpec,
    allowed: usize,
) -> Result<(), OutputLimit> {
    let c_length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let used = spec
        .precision
        .map_or(c_length, |precision| precision.min(c_length));
    let padding = spec.width.saturating_sub(used);
    check_append(out, spec.width.max(used), allowed)?;
    if !spec.left_align {
        repeat(out, b' ', padding);
    }
    out.push_bytes(&bytes[..used]);
    if spec.left_align {
        repeat(out, b' ', padding);
    }
    Ok(())
}

/// %c renders one byte; precision and zero fill are ignored. The temporary
/// printf buffer becomes a C string before it is appended to outer output.
/// Precision/NUL behavior is confirmed by the sealed extra #340 reference cases.
pub(crate) fn render_character(
    out: &mut ByteBuffer,
    byte: u8,
    spec: FormatSpec,
    allowed: usize,
) -> Result<(), OutputLimit> {
    let padding = spec.width.saturating_sub(1);
    if byte == 0 {
        // Right-aligned spaces precede NUL; left-aligned spaces follow it and
        // disappear with the per-conversion C-string suffix. No NUL is appended.
        let visible = if spec.left_align { 0 } else { padding };
        check_append(out, visible, allowed)?;
        repeat(out, b' ', visible);
        return Ok(());
    }
    check_append(out, spec.width.max(1), allowed)?;
    if !spec.left_align {
        repeat(out, b' ', padding);
    }
    out.push_bytes(&[byte]);
    if spec.left_align {
        repeat(out, b' ', padding);
    }
    Ok(())
}

#[cfg(test)]
#[path = "integer_bytes_tests.rs"]
mod tests;

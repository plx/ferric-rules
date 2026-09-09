//! Deterministic Ferric fallback for scanner-admitted noncanonical modifiers.
//! The five sealed INTEGER7 echoes match this algorithm; extensions are explicit
//! engine policy, not a claim about all libc printf implementations.

use super::format_parser::{Conversion, Directive, SpecAnalysis};
use super::format_types::OutputLimit;
use crate::byte_buffer::ByteBuffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NonCanonicalError {
    /// Canonical instructions and parameter overflows use their own paths.
    NotNonCanonical,
    /// A manually constructed descriptor disagrees with scanner invariants.
    InvalidDescriptor,
    OutputLimit(OutputLimit),
}

struct Prefix<'a> {
    left: bool,
    zero: bool,
    width: &'a [u8],
    precision: Option<&'a [u8]>,
    tail: &'a [u8],
}

fn decimal_spelling(bytes: &[u8]) -> &[u8] {
    let first = bytes.iter().position(|&byte| byte != b'0');
    first.map_or(b"0", |index| &bytes[index..])
}

fn split_prefix<'a>(directive: &Directive<'a>) -> Result<Prefix<'a>, NonCanonicalError> {
    let SpecAnalysis::NonCanonical { offset, byte } = directive.spec else {
        return Err(NonCanonicalError::NotNonCanonical);
    };
    let raw = directive.raw_modifiers;
    if raw.len() > 73
        || raw
            .iter()
            .any(|b| !b.is_ascii_digit() && !matches!(b, b'.' | b'-'))
    {
        return Err(NonCanonicalError::InvalidDescriptor);
    }
    let mut position = 0;
    let mut left = false;
    let mut zero = false;
    while let Some(&byte) = raw.get(position) {
        match byte {
            b'-' => left = true,
            b'0' => zero = true,
            _ => break,
        }
        position += 1;
    }
    let width_start = position;
    while raw.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    let width = &raw[width_start..position];
    let precision = if raw.get(position) == Some(&b'.') {
        position += 1;
        let start = position;
        while raw.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        Some(decimal_spelling(&raw[start..position]))
    } else {
        None
    };
    if raw.get(position) != Some(&byte)
        || directive.range.start.checked_add(position + 1) != Some(offset)
        || directive.range.start.checked_add(raw.len() + 2) != Some(directive.range.end)
    {
        return Err(NonCanonicalError::InvalidDescriptor);
    }
    Ok(Prefix {
        left,
        // Deterministic policy: fold repeated flags, prefer left alignment, and
        // emit a surviving zero flag before width. No padding is performed.
        zero: zero && !left,
        width,
        precision,
        tail: &raw[position..],
    })
}

/// Append a normalized valid prefix followed by the untouched invalid tail.
///
/// Call ONLY after the normal directive operand was evaluated/admitted. This
/// helper neither evaluates/adopts operands nor selects diagnostic flags. It
/// does not retry a failed admission or handle a canonical parameter overflow.
///
/// Width/precision digits are normalized lexically, never parsed to usize or
/// expanded into padding. Empty precision becomes `.0`. Repeated leading flags
/// fold deterministically; the suffix from the first invalid modifier remains
/// byte-exact. CLIPS's integer `ll` insertion precedes the final conversion.
/// A failed append leaves all prior output unchanged.
pub(crate) fn append_noncanonical(
    output: &mut ByteBuffer,
    directive: &Directive<'_>,
    allowed: usize,
) -> Result<(), NonCanonicalError> {
    let prefix = split_prefix(directive)?;
    let integer = matches!(
        directive.conversion,
        Conversion::Decimal | Conversion::Octal | Conversion::Hex | Conversion::Unsigned
    );
    // Every component is bounded by the source scanner's 73-modifier limit.
    let length = 1
        + usize::from(prefix.left)
        + usize::from(prefix.zero)
        + prefix.width.len()
        + prefix.precision.map_or(0, |digits| digits.len() + 1)
        + prefix.tail.len()
        + if integer { 2 } else { 0 }
        + 1;
    let required = output.as_bytes().len().checked_add(length);
    if !required.is_some_and(|required| required <= allowed) {
        return Err(NonCanonicalError::OutputLimit(OutputLimit {
            required,
            allowed,
        }));
    }
    output.push_bytes(b"%");
    if prefix.left {
        output.push_bytes(b"-");
    }
    if prefix.zero {
        output.push_bytes(b"0");
    }
    output.push_bytes(prefix.width);
    if let Some(digits) = prefix.precision {
        output.push_bytes(b".");
        output.push_bytes(digits);
    }
    output.push_bytes(prefix.tail);
    if integer {
        output.push_bytes(b"ll");
    }
    output.push_bytes(&[directive.conversion.as_byte()]);
    Ok(())
}

#[cfg(test)]
#[path = "noncanonical_tests.rs"]
mod tests;

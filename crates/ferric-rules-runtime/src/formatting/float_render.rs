//! Portable renderers for canonical f/e/g directives, with no evaluator wrapper.
use super::format_types::{FormatSpec, OutputLimit};
use crate::byte_buffer::ByteBuffer;
use std::fmt::{self, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatConversion {
    Fixed,
    Scientific,
    General,
}

// Every finite f64 has at most 309 decimal integer positions and at most1074
// decimal fractional positions (its denominator divides 2^1074). Thus1384
// significant digits suffice to represent it exactly. Higher g precision can
// change the notation cutoff, but adds only zeroes that g removes afterward.
const EXACT_SIGNIFICANT_BOUND: usize = 1384;

/// A formatting sink that checks every append before String can allocate.
/// This keeps a rejected conversion from mutating the caller's output.
struct BoundedText {
    text: String,
    limit: usize,
}

impl BoundedText {
    fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
        }
    }
}

impl Write for BoundedText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if text.len() > self.limit.saturating_sub(self.text.len()) {
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

fn limit(allowed: usize) -> OutputLimit {
    OutputLimit {
        required: None,
        allowed,
    }
}

fn check_size(existing: usize, extra: usize, allowed: usize) -> Result<(), OutputLimit> {
    let required = existing.checked_add(extra);
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

fn split_scientific(text: &str) -> (&str, i32) {
    let (mantissa, exponent) = text.split_once('e').expect("finite LowerExp value");
    (mantissa, exponent.parse().expect("finite decimal exponent"))
}

fn append_exponent(out: &mut ByteBuffer, exponent: i32) {
    out.push_bytes(if exponent < 0 { b"e-" } else { b"e+" });
    let digits = exponent.unsigned_abs().to_string();
    if digits.len() < 2 {
        out.push_bytes(b"0");
    }
    out.push_str(&digits);
}

fn scientific_body(text: &str, existing: usize, allowed: usize) -> Result<ByteBuffer, OutputLimit> {
    let (mantissa, exponent) = split_scientific(text);
    let exponent_digits = exponent.unsigned_abs().to_string().len().max(2);
    check_size(existing, mantissa.len() + 2 + exponent_digits, allowed)?;
    let mut body = ByteBuffer::new();
    body.push_str(mantissa);
    append_exponent(&mut body, exponent);
    Ok(body)
}

fn general_body(
    text: &str,
    requested_precision: usize,
    existing: usize,
    allowed: usize,
) -> Result<ByteBuffer, OutputLimit> {
    let (mantissa, exponent) = split_scientific(text);
    let mut digits: Vec<u8> = mantissa.bytes().filter(|byte| *byte != b'.').collect();
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    if digits == b"0" {
        check_size(existing, 1, allowed)?;
        let mut body = ByteBuffer::new();
        body.push_bytes(b"0");
        return Ok(body);
    }
    let use_fixed = exponent >= -4
        && (exponent < 0
            || usize::try_from(exponent).expect("nonnegative exponent") < requested_precision);
    let required = if !use_fixed {
        digits.len()
            + usize::from(digits.len() > 1)
            + 2
            + exponent.unsigned_abs().to_string().len().max(2)
    } else if exponent < 0 {
        2 + usize::try_from(-exponent - 1).expect("negative exponent") + digits.len()
    } else {
        let point = usize::try_from(exponent + 1).expect("nonnegative decimal point");
        point.max(digits.len()) + usize::from(point < digits.len())
    };
    check_size(existing, required, allowed)?;
    let mut body = ByteBuffer::new();
    if !use_fixed {
        body.push_bytes(&digits[..1]);
        if digits.len() > 1 {
            body.push_bytes(b".");
            body.push_bytes(&digits[1..]);
        }
        append_exponent(&mut body, exponent);
    } else if exponent < 0 {
        body.push_bytes(b"0.");
        repeat(
            &mut body,
            b'0',
            usize::try_from(-exponent - 1).expect("negative exponent"),
        );
        body.push_bytes(&digits);
    } else {
        let point = usize::try_from(exponent + 1).expect("nonnegative decimal point");
        if point >= digits.len() {
            body.push_bytes(&digits);
            repeat(&mut body, b'0', point - digits.len());
        } else {
            body.push_bytes(&digits[..point]);
            body.push_bytes(b".");
            body.push_bytes(&digits[point..]);
        }
    }
    Ok(body)
}

/// Render the actual f64 operand. The wrapper converts INTEGER operands to f64
/// before calling this function, as CLIPS `PrintFormatFlag` does for f/e/g.
pub(crate) fn render_float(
    out: &mut ByteBuffer,
    value: f64,
    conversion: FloatConversion,
    spec: FormatSpec,
    allowed: usize,
) -> Result<(), OutputLimit> {
    let existing = out.as_bytes().len();
    check_size(existing, spec.width, allowed)?;
    let negative = value.is_sign_negative();
    let sign = usize::from(negative);
    check_size(existing, sign + 1, allowed)?;
    let remaining = allowed - existing - sign;
    let finite = value.is_finite();
    let body = if finite {
        let magnitude = value.abs();
        match conversion {
            FloatConversion::Fixed | FloatConversion::Scientific => {
                let precision = spec.precision.unwrap_or(6);
                // Reject output-sized precision before invoking the formatter.
                // Scientific final exponent requires e, a sign and >=2 digits.
                let minimum = match conversion {
                    FloatConversion::Fixed => {
                        if precision == 0 {
                            1
                        } else {
                            precision.checked_add(2).ok_or_else(|| limit(allowed))?
                        }
                    }
                    FloatConversion::Scientific => {
                        if precision == 0 {
                            5
                        } else {
                            precision.checked_add(6).ok_or_else(|| limit(allowed))?
                        }
                    }
                    FloatConversion::General => unreachable!(),
                };
                check_size(existing + sign, minimum, allowed)?;
                let mut formatted = BoundedText::new(remaining);
                let result = if conversion == FloatConversion::Fixed {
                    write!(&mut formatted, "{magnitude:.precision$}")
                } else {
                    write!(&mut formatted, "{magnitude:.precision$e}")
                };
                result.map_err(|_| limit(allowed))?;
                if conversion == FloatConversion::Scientific {
                    scientific_body(&formatted.text, existing + sign, allowed)?
                } else {
                    let mut body = ByteBuffer::new();
                    body.push_str(&formatted.text);
                    body
                }
            }
            FloatConversion::General => {
                let requested = spec.precision.unwrap_or(6).max(1);
                let rounded_digits = requested.min(EXACT_SIGNIFICANT_BOUND);
                let precision = rounded_digits - 1;
                // The scratch upper bound is intrinsic to f64, independent of
                // an attacker-provided precision or the final output allowance.
                let mut formatted = BoundedText::new(EXACT_SIGNIFICANT_BOUND + 8);
                write!(&mut formatted, "{magnitude:.precision$e}").map_err(|_| limit(allowed))?;
                general_body(&formatted.text, requested, existing + sign, allowed)?
            }
        }
    } else {
        check_size(existing + sign, 3, allowed)?;
        let mut body = ByteBuffer::new();
        body.push_bytes(if value.is_nan() { b"nan" } else { b"inf" });
        body
    };
    let body_len = body.as_bytes().len();
    let content = body_len.checked_add(sign).ok_or_else(|| limit(allowed))?;
    let field_len = spec.width.max(content);
    check_size(existing, field_len, allowed)?;
    let padding = field_len - content;
    // Nonfinite zero-flag behavior is explicitly pinned separately from finite
    // numbers; finite precision never disables floating width zeroes.
    let zeroes = finite && spec.zero_pad && !spec.left_align;
    if !spec.left_align && !zeroes {
        repeat(out, b' ', padding);
    }
    if negative {
        out.push_bytes(b"-");
    }
    if zeroes {
        repeat(out, b'0', padding);
    }
    out.push_bytes(body.as_bytes());
    if spec.left_align {
        repeat(out, b' ', padding);
    }
    Ok(())
}

#[cfg(test)]
#[path = "float_render_tests.rs"]
mod tests;

//! CLIPS 6.30 numeric scanning shared by source text and runtime field input.

/// The type and value of a scanned number, or a number-like symbol.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumberKind {
    Integer(i64),
    Float(f64),
    /// The original spelling is `input[..consumed]`.
    Symbol,
}

/// A numeric scan result, including recoverable integer saturation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberScan {
    /// Bytes consumed, excluding the delimiter that ended the token.
    pub consumed: usize,
    pub kind: NumberKind,
    /// An integer exceeded the signed 64-bit range and was saturated.
    /// Callers can report a warning while retaining the returned value.
    pub integer_overflow: bool,
}

/// Scan a CLIPS number-like token beginning with a digit, `.`, `+`, or `-`.
///
/// Malformed numbers consume the entire symbol spelling, including its UTF-8
/// bytes. Valid numbers retain their integer or float type; integers saturate
/// at the `i64` limits. The caller owns whitespace, comments, and source-end
/// handling. Empty input or an initial delimiter consumes no bytes.
#[must_use]
pub fn scan_number(input: &[u8]) -> NumberScan {
    let mut cursor = 0;
    let mut phase = NumberPhase::Sign;
    let mut mantissa_digit = false;
    let mut floating = false;
    loop {
        let byte = input.get(cursor).copied();
        match (phase, byte) {
            (NumberPhase::Sign, Some(b'+' | b'-')) => phase = NumberPhase::Integral,
            (NumberPhase::Sign | NumberPhase::Integral, Some(b'0'..=b'9')) => {
                phase = NumberPhase::Integral;
                mantissa_digit = true;
            }
            (NumberPhase::Sign | NumberPhase::Integral, Some(b'.')) => {
                phase = NumberPhase::Decimal;
                floating = true;
            }
            (NumberPhase::Decimal, Some(b'0'..=b'9')) => mantissa_digit = true,
            (
                NumberPhase::Sign | NumberPhase::Integral | NumberPhase::Decimal,
                Some(b'e' | b'E'),
            ) => {
                phase = NumberPhase::ExponentStart;
                floating = true;
            }
            (NumberPhase::ExponentStart, Some(b'0'..=b'9' | b'+' | b'-')) => {
                phase = NumberPhase::ExponentValue;
            }
            (NumberPhase::ExponentValue, Some(b'0'..=b'9')) => {}
            _ if number_delimiter(byte, phase) => {
                // An exponent without digits leaves a symbol.
                if phase == NumberPhase::ExponentStart
                    || (phase == NumberPhase::ExponentValue
                        && matches!(input[cursor - 1], b'+' | b'-'))
                {
                    mantissa_digit = false;
                }
                break;
            }
            _ => {
                // Any other byte turns the whole token into a symbol.
                cursor += 1;
                while input
                    .get(cursor)
                    .copied()
                    .is_some_and(is_symbol_continuation)
                {
                    cursor += 1;
                }
                return NumberScan {
                    consumed: cursor,
                    kind: NumberKind::Symbol,
                    integer_overflow: false,
                };
            }
        }
        cursor += 1;
    }
    let bytes = &input[..cursor];
    let (kind, integer_overflow) = if !mantissa_digit {
        (NumberKind::Symbol, false)
    } else if floating {
        // The grammar admits only ASCII decimal floats, which Rust parses
        // exactly, including signed zero, underflow and infinity.
        let text = std::str::from_utf8(bytes).expect("numeric grammar is ASCII");
        (
            NumberKind::Float(text.parse().expect("scanned decimal float parses")),
            false,
        )
    } else {
        let (value, overflow) = scan_integer(bytes);
        (NumberKind::Integer(value), overflow)
    };
    NumberScan {
        consumed: cursor,
        kind,
        integer_overflow,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NumberPhase {
    Sign,
    Integral,
    Decimal,
    ExponentStart,
    ExponentValue,
}

/// Whether `byte` starts a UTF-8 sequence, which CLIPS 6.30 `scanner.c` accepts as a symbol start.
pub fn is_utf8_start(byte: u8) -> bool {
    (0xc0..=0xf7).contains(&byte)
}

/// Per CLIPS 6.30 `scanner.c`, a symbol continues through printable ASCII except the delimiters `<"()&|~ ;` and through UTF-8 bytes.
pub fn is_symbol_continuation(byte: u8) -> bool {
    !matches!(
        byte,
        b'<' | b'"' | b'(' | b')' | b'&' | b'|' | b'~' | b' ' | b';'
    ) && (byte.is_ascii_graphic() || is_utf8_start(byte) || (0x80..=0xbf).contains(&byte))
}

fn number_delimiter(byte: Option<u8>, phase: NumberPhase) -> bool {
    match byte {
        None | Some(b'<' | b'"' | b'(' | b')' | b'&' | b'|' | b'~' | b' ' | b';') => true,
        Some(byte) => {
            !byte.is_ascii_graphic() && (phase == NumberPhase::Sign || !is_utf8_start(byte))
        }
    }
}

/// Parse decimal digits, saturating at the `i64` range as CLIPS does.
fn scan_integer(bytes: &[u8]) -> (i64, bool) {
    let negative = bytes.first() == Some(&b'-');
    let digits = bytes
        .strip_prefix(b"+")
        .or_else(|| bytes.strip_prefix(b"-"));
    let limit = if negative {
        1_u64 << 63
    } else {
        i64::MAX as u64
    };
    let mut magnitude = 0_u64;
    for &digit in digits.unwrap_or(bytes) {
        match magnitude
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit - b'0')))
        {
            Some(value) if value <= limit => magnitude = value,
            _ => return (if negative { i64::MIN } else { i64::MAX }, true),
        }
    }
    let value = if negative {
        0_i64.wrapping_sub_unsigned(magnitude)
    } else {
        i64::try_from(magnitude).expect("magnitude was checked against i64::MAX")
    };
    (value, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn integer_boundaries_preserve_overflow_notice() {
        for (input, expected, overflow) in [
            ("42", 42, false),
            ("+00042", 42, false),
            ("-00042", -42, false),
            ("9223372036854775807", i64::MAX, false),
            ("-9223372036854775808", i64::MIN, false),
            ("9223372036854775808", i64::MAX, true),
            ("-9223372036854775809", i64::MIN, true),
            ("+99999999999999999999999", i64::MAX, true),
            ("-99999999999999999999999", i64::MIN, true),
        ] {
            assert_eq!(
                scan_number(input.as_bytes()),
                NumberScan {
                    consumed: input.len(),
                    kind: NumberKind::Integer(expected),
                    integer_overflow: overflow,
                },
                "{input}"
            );
        }
        // A suffix makes this a symbol, so its digits must not warn or clamp.
        let result = scan_number(b"99999999999999999999999tail ");
        assert_eq!(result.kind, NumberKind::Symbol);
        assert!(!result.integer_overflow);
    }

    #[test]
    fn float_boundaries_preserve_signed_zero_and_infinity() {
        for (input, expected) in [
            ("1.", 1.0_f64),
            ("1.e3", 1000.0),
            (".5", 0.5),
            ("+.5", 0.5),
            ("-.5e1", -5.0),
            ("-0.0", -0.0),
            ("1e309", f64::INFINITY),
            ("-1e309", f64::NEG_INFINITY),
            ("-1e-999", -0.0),
        ] {
            let result = scan_number(input.as_bytes());
            assert_eq!(result.consumed, input.len());
            assert!(!result.integer_overflow);
            let NumberKind::Float(actual) = result.kind else {
                panic!("expected float for {input}");
            };
            assert_eq!(actual.to_bits(), expected.to_bits(), "{input}");
        }
    }

    #[test]
    fn malformed_numbers_keep_their_spelling_until_a_clips_delimiter() {
        for token in [
            "1st", "0x10", "12abc", "1.abc", "3.14.15", "1-2", "1e5x", "1e5.5", "5f", "5e", "1.5e",
            "1e+", "1e-", "+", "-", ".", "1,2", "1:2", "1é中",
        ] {
            for delimiter in [
                " ", "\t", "\n", "\r", ";", "<", "\"", "(", ")", "&", "|", "~",
            ] {
                let input = format!("{token}{delimiter}tail");
                let result = scan_number(input.as_bytes());
                assert_eq!(result.kind, NumberKind::Symbol, "{input:?}");
                assert_eq!(result.consumed, token.len(), "{input:?}");
                assert_eq!(&input.as_bytes()[..result.consumed], token.as_bytes());
                assert!(!result.integer_overflow);
            }
        }
        let input = b"1\xc3\xa9\x80tail)";
        let result = scan_number(input);
        assert_eq!(result.kind, NumberKind::Symbol);
        assert_eq!(&input[..result.consumed], b"1\xc3\xa9\x80tail");
    }

    proptest! {
        #[test]
        fn arbitrary_byte_input_never_panics_or_consumes_past_the_end(input in prop::collection::vec(any::<u8>(), 0..256)) {
            let result = scan_number(&input);
            prop_assert!(result.consumed <= input.len());
        }
    }
}

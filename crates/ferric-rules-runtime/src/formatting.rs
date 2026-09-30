//! The CLIPS 6.30 `format` control-string grammar and its C `printf`
//! conversions, rendered in Rust, plus CLIPS's `%.15g` float spelling used
//! for ordinary output.
//!
//! Width and precision count bytes, as in C. Where C would emit part of a
//! UTF-8 character (`%.1s` of `"é"`) or a byte of 128 or more (`%c 200`),
//! the result is a Rust string and gets U+FFFD instead.

/// Largest accepted width or precision. CLIPS formats into a fixed buffer;
/// Ferric rejects larger fields instead of allocating them.
pub(crate) const MAX_FIELD: usize = 4096;

/// CLIPS copies at most this many bytes of a directive, `%` included.
const DIRECTIVE_CAPACITY: usize = 75;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Spec {
    pub(crate) left: bool,
    pub(crate) zero: bool,
    pub(crate) width: usize,
    pub(crate) precision: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Conversion {
    Decimal,
    Octal,
    Hex,
    Unsigned,
    Character,
    Lexeme,
    Scientific,
    Fixed,
    General,
}

impl Conversion {
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            b'd' => Self::Decimal,
            b'o' => Self::Octal,
            b'x' => Self::Hex,
            b'u' => Self::Unsigned,
            b'c' => Self::Character,
            b's' => Self::Lexeme,
            b'e' => Self::Scientific,
            b'f' => Self::Fixed,
            b'g' => Self::General,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Piece<'a> {
    /// Literal text, including `%n`-style controls already translated.
    Text(&'a str),
    Directive(Conversion, Spec),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FormatError {
    /// A byte other than a digit, `.` or `-` before the conversion.
    InvalidFlag { directive: String },
    /// Flags, width and precision that are not in `[-0]*digits[.digits]` order.
    Malformed { directive: String },
    /// A width or precision above [`MAX_FIELD`].
    FieldTooLarge { directive: String },
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFlag { directive } => write!(f, "invalid format flag in `{directive}`"),
            Self::Malformed { directive } => {
                write!(f, "unsupported format directive `{directive}`")
            }
            Self::FieldTooLarge { directive } => write!(
                f,
                "format width or precision in `{directive}` exceeds {MAX_FIELD}"
            ),
        }
    }
}

/// Split a control string into text and directives. As in CLIPS, the string
/// ends at its first NUL and an incomplete directive is literal text.
pub(crate) fn parse(control: &str) -> Result<Vec<Piece<'_>>, FormatError> {
    let control = control.split('\0').next().unwrap_or_default();
    let bytes = control.as_bytes();
    let mut pieces = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        let start = position;
        if bytes[start] != b'%' {
            while position < bytes.len() && bytes[position] != b'%' {
                position += 1;
            }
            pieces.push(Piece::Text(&control[start..position]));
            continue;
        }
        let control_text = match bytes.get(start + 1) {
            Some(b'n') => Some("\n"),
            Some(b'r') => Some("\r"),
            Some(b't') => Some("\t"),
            Some(b'v') => Some("\x0b"),
            Some(b'%') => Some("%"),
            _ => None,
        };
        if let Some(text) = control_text {
            pieces.push(Piece::Text(text));
            position = start + 2;
            continue;
        }
        position += 1;
        let mut conversion = None;
        while position < bytes.len()
            && bytes[position] != b'%'
            && position - start < DIRECTIVE_CAPACITY
        {
            let byte = bytes[position];
            position += 1;
            if let Some(kind) = Conversion::from_byte(byte) {
                conversion = Some(kind);
                break;
            }
            if !byte.is_ascii_digit() && byte != b'.' && byte != b'-' {
                return Err(FormatError::InvalidFlag {
                    directive: String::from_utf8_lossy(&bytes[start..position]).into_owned(),
                });
            }
        }
        // Every byte consumed so far is ASCII, so the slice is on a boundary.
        let directive = &control[start..position];
        match conversion {
            Some(kind) => pieces.push(Piece::Directive(
                kind,
                parse_spec(&directive[1..directive.len() - 1], directive)?,
            )),
            None => pieces.push(Piece::Text(directive)),
        }
    }
    Ok(pieces)
}

fn parse_spec(modifiers: &str, directive: &str) -> Result<Spec, FormatError> {
    let mut spec = Spec::default();
    let mut rest = modifiers;
    while let Some(flag) = rest.chars().next().filter(|c| matches!(c, '-' | '0')) {
        spec.left |= flag == '-';
        spec.zero |= flag == '0';
        rest = &rest[1..];
    }
    let (width, rest) = split_digits(rest);
    let (precision, rest) = match rest.strip_prefix('.') {
        Some(rest) => {
            let (digits, rest) = split_digits(rest);
            (Some(digits), rest)
        }
        None => (None, rest),
    };
    if !rest.is_empty() {
        return Err(FormatError::Malformed {
            directive: directive.to_owned(),
        });
    }
    let field = |digits: &str| -> Result<usize, FormatError> {
        let value = if digits.is_empty() {
            0
        } else {
            digits.parse().unwrap_or(usize::MAX)
        };
        if value > MAX_FIELD {
            return Err(FormatError::FieldTooLarge {
                directive: directive.to_owned(),
            });
        }
        Ok(value)
    };
    spec.width = field(width)?;
    spec.precision = precision.map(field).transpose()?;
    Ok(spec)
}

fn split_digits(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

fn pad(out: &mut String, fill: char, count: usize) {
    out.extend(std::iter::repeat(fill).take(count));
}

/// `%d`, `%o`, `%x`, `%u`. Octal, hex and unsigned reinterpret all 64 bits.
pub(crate) fn render_integer(out: &mut String, conversion: Conversion, value: i64, spec: Spec) {
    let negative = conversion == Conversion::Decimal && value < 0;
    let magnitude = if conversion == Conversion::Decimal {
        value.unsigned_abs()
    } else {
        u64::from_ne_bytes(value.to_ne_bytes())
    };
    let digits = match conversion {
        _ if value == 0 && spec.precision == Some(0) => String::new(),
        Conversion::Octal => format!("{magnitude:o}"),
        Conversion::Hex => format!("{magnitude:x}"),
        _ => magnitude.to_string(),
    };
    let precision_zeroes = spec.precision.unwrap_or(0).saturating_sub(digits.len());
    let body_len = digits.len() + precision_zeroes + usize::from(negative);
    let padding = spec.width.saturating_sub(body_len);
    // C ignores the zero flag when a precision is given.
    let zero_fill = spec.zero && !spec.left && spec.precision.is_none();
    if !spec.left && !zero_fill {
        pad(out, ' ', padding);
    }
    if negative {
        out.push('-');
    }
    if zero_fill {
        pad(out, '0', padding);
    }
    pad(out, '0', precision_zeroes);
    out.push_str(&digits);
    if spec.left {
        pad(out, ' ', padding);
    }
}

/// `%f`, `%e` and `%g` of an `f64`, as C prints them.
pub(crate) fn render_float(out: &mut String, value: f64, conversion: Conversion, spec: Spec) {
    let body = if value.is_finite() {
        let magnitude = value.abs();
        match conversion {
            Conversion::Fixed => format!("{magnitude:.*}", spec.precision.unwrap_or(6)),
            Conversion::Scientific => {
                let text = format!("{magnitude:.*e}", spec.precision.unwrap_or(6));
                c_exponent(&text)
            }
            _ => general(magnitude, spec.precision.unwrap_or(6).max(1)),
        }
    } else if value.is_nan() {
        "nan".to_owned()
    } else {
        "inf".to_owned()
    };
    let negative = value.is_sign_negative();
    let content = body.len() + usize::from(negative);
    let padding = spec.width.saturating_sub(content);
    // C zero-fills finite numbers only.
    let zero_fill = value.is_finite() && spec.zero && !spec.left;
    if !spec.left && !zero_fill {
        pad(out, ' ', padding);
    }
    if negative {
        out.push('-');
    }
    if zero_fill {
        pad(out, '0', padding);
    }
    out.push_str(&body);
    if spec.left {
        pad(out, ' ', padding);
    }
}

/// Rewrite Rust's `1.5e3` exponent as C's `1.5e+03`.
fn c_exponent(text: &str) -> String {
    let (mantissa, exponent) = text.split_once('e').expect("LowerExp has an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp exponent is an integer");
    format!(
        "{mantissa}e{}{:02}",
        if exponent < 0 { '-' } else { '+' },
        exponent.unsigned_abs()
    )
}

/// C `%.{precision}g` of a non-negative finite value: round once to
/// `precision` significant digits, choose notation from the rounded exponent,
/// then drop trailing zeros.
fn general(magnitude: f64, precision: usize) -> String {
    let rounded = format!("{magnitude:.*e}", precision - 1);
    let (mantissa, exponent) = rounded.split_once('e').expect("LowerExp has an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp exponent is an integer");
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let digits = match digits.trim_end_matches('0') {
        "" => "0",
        trimmed => trimmed,
    };
    let fixed = exponent >= -4 && usize::try_from(exponent).map_or(true, |e| e < precision);
    if !fixed {
        let mut text = digits[..1].to_owned();
        if digits.len() > 1 {
            text.push('.');
            text.push_str(&digits[1..]);
        }
        return c_exponent(&format!("{text}e{exponent}"));
    }
    if exponent < 0 {
        let zeroes = usize::try_from(-exponent - 1).expect("negative exponent");
        return format!("0.{}{digits}", "0".repeat(zeroes));
    }
    let point = usize::try_from(exponent + 1).expect("non-negative exponent");
    if digits.len() <= point {
        format!("{digits}{}", "0".repeat(point - digits.len()))
    } else {
        format!("{}.{}", &digits[..point], &digits[point..])
    }
}

/// `%s`: the text up to any NUL, cut to `precision` bytes, padded to `width`
/// bytes. The zero flag is ignored.
pub(crate) fn render_lexeme(out: &mut String, text: &str, spec: Spec) {
    let text = text.split('\0').next().unwrap_or_default().as_bytes();
    let used = spec.precision.map_or(text.len(), |p| p.min(text.len()));
    let padding = spec.width.saturating_sub(used);
    if !spec.left {
        pad(out, ' ', padding);
    }
    out.push_str(&String::from_utf8_lossy(&text[..used]));
    if spec.left {
        pad(out, ' ', padding);
    }
}

/// `%c` of one byte; precision and the zero flag are ignored. CLIPS builds
/// each conversion as a C string, so a NUL ends it: right-aligned padding
/// survives and left-aligned padding does not.
pub(crate) fn render_character(out: &mut String, byte: u8, spec: Spec) {
    let padding = spec.width.saturating_sub(1);
    if byte == 0 {
        if !spec.left {
            pad(out, ' ', padding);
        }
        return;
    }
    if !spec.left {
        pad(out, ' ', padding);
    }
    out.push(if byte.is_ascii() {
        char::from(byte)
    } else {
        char::REPLACEMENT_CHARACTER
    });
    if spec.left {
        pad(out, ' ', padding);
    }
}

/// CLIPS `FloatToString`: `%.15g`, plus `.0` when that has no `.` or exponent.
pub(crate) fn append_clips_float(value: f64, out: &mut String) {
    let start = out.len();
    render_float(
        out,
        value,
        Conversion::General,
        Spec {
            precision: Some(15),
            ..Spec::default()
        },
    );
    if !out[start..].contains(['.', 'e']) {
        out.push_str(".0");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(left: bool, zero: bool, width: usize, precision: Option<usize>) -> Spec {
        Spec {
            left,
            zero,
            width,
            precision,
        }
    }

    fn float(value: f64, conversion: Conversion, spec: Spec) -> String {
        let mut out = String::new();
        render_float(&mut out, value, conversion, spec);
        out
    }

    fn clips_float(value: f64) -> String {
        let mut out = String::new();
        append_clips_float(value, &mut out);
        out
    }

    fn integer(value: i64, conversion: Conversion, spec: Spec) -> String {
        let mut out = String::new();
        render_integer(&mut out, conversion, value, spec);
        out
    }

    #[test]
    fn control_strings_split_into_text_controls_and_directives() {
        assert_eq!(
            parse("a%n%%%-08.3d|%5").unwrap(),
            [
                Piece::Text("a"),
                Piece::Text("\n"),
                Piece::Text("%"),
                Piece::Directive(Conversion::Decimal, spec(true, true, 8, Some(3))),
                Piece::Text("|"),
                Piece::Text("%5"),
            ]
        );
        assert_eq!(
            parse("%--5d%0-05d").unwrap(),
            [
                Piece::Directive(Conversion::Decimal, spec(true, false, 5, None)),
                Piece::Directive(Conversion::Decimal, spec(true, true, 5, None)),
            ]
        );
        assert_eq!(
            parse("%.d").unwrap(),
            [Piece::Directive(
                Conversion::Decimal,
                spec(false, false, 0, Some(0))
            )]
        );
        assert_eq!(parse("ok\0%q").unwrap(), [Piece::Text("ok")]);
        assert_eq!(parse("%5%d").unwrap()[0], Piece::Text("%5"));
    }

    #[test]
    fn bad_directives_are_errors() {
        assert!(matches!(parse("%q"), Err(FormatError::InvalidFlag { .. })));
        for control in ["%5-3d", "%.2.3d", "%..d", "%.-3d"] {
            assert!(
                matches!(parse(control), Err(FormatError::Malformed { .. })),
                "{control}"
            );
        }
        assert!(parse("%4096d").is_ok());
        assert!(matches!(
            parse("%4097d"),
            Err(FormatError::FieldTooLarge { .. })
        ));
        assert!(matches!(
            parse("%.99999999999999999999999f"),
            Err(FormatError::FieldTooLarge { .. })
        ));
    }

    #[test]
    fn integers_follow_c_flags_precision_and_radix() {
        use Conversion::{Decimal, Hex, Octal, Unsigned};
        for (value, conversion, spec, expected) in [
            (7, Decimal, spec(false, true, 4, None), "0007"),
            (-7, Decimal, spec(false, true, 4, None), "-007"),
            (7, Decimal, spec(true, true, 5, None), "7    "),
            (-7, Decimal, spec(false, false, 8, Some(3)), "    -007"),
            (7, Decimal, spec(false, true, 8, Some(3)), "     007"),
            (0, Decimal, spec(false, false, 0, Some(0)), ""),
            (0, Decimal, spec(false, true, 5, Some(0)), "     "),
            (
                i64::MIN,
                Decimal,
                spec(false, true, 22, None),
                "-009223372036854775808",
            ),
            (65, Octal, spec(false, true, 8, None), "00000101"),
            (65, Hex, spec(false, false, 8, Some(3)), "     041"),
            (-1, Hex, Spec::default(), "ffffffffffffffff"),
            (-1, Unsigned, Spec::default(), "18446744073709551615"),
        ] {
            assert_eq!(
                integer(value, conversion, spec),
                expected,
                "{value} {spec:?}"
            );
        }
    }

    #[test]
    fn floats_follow_c_notation_padding_and_rounding() {
        use Conversion::{Fixed, General, Scientific};
        for (value, conversion, spec, expected) in [
            (2.5, Fixed, spec(false, true, 8, Some(2)), "00002.50"),
            (-2.5, Fixed, spec(true, true, 8, Some(2)), "-2.50   "),
            (
                -2.5,
                Scientific,
                spec(false, true, 12, Some(2)),
                "-0002.50e+00",
            ),
            (2.5, General, spec(false, true, 10, Some(3)), "00000002.5"),
            (12345.0, Scientific, Spec::default(), "1.234500e+04"),
            (
                1e100,
                Scientific,
                spec(false, false, 0, Some(2)),
                "1.00e+100",
            ),
            (
                -0.0,
                Scientific,
                spec(false, true, 10, Some(2)),
                "-00.00e+00",
            ),
            (2.5, Fixed, spec(false, false, 0, Some(0)), "2"),
            (3.5, Fixed, spec(false, false, 0, Some(0)), "4"),
            (9.9, General, spec(false, false, 0, Some(0)), "1e+01"),
            (-0.0, General, spec(false, false, 0, Some(0)), "-0"),
            (999_999.5, General, Spec::default(), "1e+06"),
            (0.00001, General, Spec::default(), "1e-05"),
            (5e-324, General, Spec::default(), "4.94066e-324"),
            (f64::INFINITY, Fixed, spec(false, true, 8, None), "     inf"),
            (
                f64::NEG_INFINITY,
                General,
                spec(false, true, 8, None),
                "    -inf",
            ),
            (f64::NAN, Fixed, spec(true, true, 8, None), "nan     "),
        ] {
            assert_eq!(float(value, conversion, spec), expected, "{value} {spec:?}");
        }
    }

    #[test]
    fn clips_float_is_fifteen_significant_digits_with_a_point() {
        for (value, expected) in [
            (1.0, "1.0"),
            (-0.0, "-0.0"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.3"),
            (1e-5, "1e-05"),
            (1e14, "100000000000000.0"),
            (1e15, "1e+15"),
            (1.234_567_890_123_456_7, "1.23456789012346"),
            (5e-324, "4.94065645841247e-324"),
            (999_999_999_999_999.5, "1e+15"),
            (f64::INFINITY, "inf.0"),
            (f64::NEG_INFINITY, "-inf.0"),
            (f64::NAN, "nan.0"),
        ] {
            assert_eq!(clips_float(value), expected, "{value}");
        }
    }

    #[test]
    fn lexemes_and_characters_count_bytes_and_replace_partial_utf8() {
        let lexeme = |text: &str, spec: Spec| {
            let mut out = String::new();
            render_lexeme(&mut out, text, spec);
            out
        };
        assert_eq!(lexeme("red", spec(false, true, 6, None)), "   red");
        assert_eq!(lexeme("abcdef", spec(false, false, 0, Some(3))), "abc");
        assert_eq!(lexeme("é", spec(false, false, 4, None)), "  é");
        assert_eq!(lexeme("é", spec(false, false, 0, Some(1))), "\u{fffd}");
        let character = |byte: u8, spec: Spec| {
            let mut out = String::new();
            render_character(&mut out, byte, spec);
            out
        };
        assert_eq!(character(b'A', spec(false, false, 4, Some(0))), "   A");
        assert_eq!(character(b'A', spec(true, false, 4, None)), "A   ");
        assert_eq!(character(0, spec(false, false, 4, None)), "   ");
        assert_eq!(character(0, spec(true, false, 4, None)), "");
        assert_eq!(character(200, Spec::default()), "\u{fffd}");
    }
}

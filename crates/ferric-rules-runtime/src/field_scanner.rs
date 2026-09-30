//! The CLIPS 6.30 field scanner (`scanner.c`) used by `string-to-field`,
//! `explode$` and `read`.
//!
//! It scans bytes, as CLIPS does, and follows CLIPS's first-NUL string limit,
//! numeric grammar, integer saturation and quoted-string escapes (including
//! `utility.c` backspace erasure). The caller turns tokens into values; for
//! valid UTF-8 input every token except an escaped end of input is valid
//! UTF-8, and callers decode the rest lossily.

use std::borrow::Cow;

/// One scanned CLIPS token.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldToken<'a> {
    Integer(i64),
    Float(f64),
    Symbol(Cow<'a, [u8]>),
    String(Cow<'a, [u8]>),
    /// The spelling without its outer brackets.
    InstanceName(Cow<'a, [u8]>),
    /// A variable, wildcard, parenthesis or connective, as its print form.
    Other(Cow<'a, [u8]>),
    /// A byte that starts no CLIPS token.
    Unknown,
    /// End of input (or an ETX byte).
    Stop,
}

impl FieldToken<'_> {
    /// The print form CLIPS stores for a token that is not a value.
    pub(crate) fn print_form(&self) -> Option<&[u8]> {
        match self {
            Self::Other(spelling) => Some(spelling),
            Self::Unknown => Some(b"<<<unprintable character>>>"),
            Self::Stop => Some(b""),
            _ => None,
        }
    }
}

/// A recoverable scanner notice. CLIPS prints these on a router and keeps
/// the scanned value; they are not evaluation errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanNotice {
    /// An integer outside the signed 64-bit range, saturated.
    IntegerRange,
    /// A quoted string that reached the end of the input.
    UnterminatedString,
}

impl ScanNotice {
    /// The logical router CLIPS writes this notice to.
    pub(crate) const fn router(self) -> &'static str {
        match self {
            Self::IntegerRange => "wwarning",
            Self::UnterminatedString => "werror",
        }
    }

    /// The exact text CLIPS writes, including its newlines.
    pub(crate) const fn text(self) -> &'static str {
        match self {
            Self::IntegerRange => "[SCANNER1] WARNING: Over or underflow of long long integer.\n",
            Self::UnterminatedString => {
                "\n[SCANNER1] Encountered End-Of-File while scanning a string\n"
            }
        }
    }
}

/// A scanned token and any notice it produced.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScannedField<'a> {
    pub token: FieldToken<'a>,
    pub notice: Option<ScanNotice>,
}

/// A cursor over one CLIPS string source. Like `OpenStringSource`, it ends
/// at the first NUL byte.
#[derive(Debug)]
pub(crate) struct FieldScanner<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> FieldScanner<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        let end = input.iter().position(|&byte| byte == 0);
        Self {
            input: &input[..end.unwrap_or(input.len())],
            cursor: 0,
        }
    }

    pub(crate) fn next_token(&mut self) -> ScannedField<'a> {
        self.skip_whitespace_and_comments();
        let start = self.cursor;
        let mut notice = None;
        let token = match self.peek() {
            None => FieldToken::Stop,
            Some(byte) if byte.is_ascii_alphabetic() || is_utf8_start(byte) => {
                self.consume_symbol_suffix();
                FieldToken::Symbol(self.slice(start))
            }
            Some(b'0'..=b'9' | b'-' | b'+' | b'.') => self.scan_number(start, &mut notice),
            Some(b'"') => {
                self.cursor += 1;
                self.scan_string(&mut notice)
            }
            Some(b'?') => {
                self.cursor += 1;
                self.scan_variable(start)
            }
            Some(b'$') => {
                self.cursor += 1;
                if self.peek() == Some(b'?') {
                    self.cursor += 1;
                    self.scan_variable(start)
                } else {
                    self.consume_symbol_suffix();
                    FieldToken::Symbol(self.slice(start))
                }
            }
            Some(b'<') => {
                self.cursor += 1;
                self.consume_symbol_suffix();
                FieldToken::Symbol(self.slice(start))
            }
            Some(b'(' | b')' | b'~' | b'|' | b'&') => {
                self.cursor += 1;
                FieldToken::Other(self.slice(start))
            }
            Some(3) => {
                self.cursor += 1;
                FieldToken::Stop
            }
            Some(byte) if byte.is_ascii_graphic() => {
                self.consume_symbol_suffix();
                self.symbol_or_instance_name(start)
            }
            Some(_) => {
                self.cursor += 1;
                FieldToken::Unknown
            }
        };
        ScannedField { token, notice }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }

    fn slice(&self, start: usize) -> Cow<'a, [u8]> {
        Cow::Borrowed(&self.input[start..self.cursor])
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\n' | b'\x0c' | b'\r' | b'\t') => self.cursor += 1,
                Some(b';') => {
                    while let Some(byte) = self.peek() {
                        self.cursor += 1;
                        if matches!(byte, b'\n' | b'\r') {
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn consume_symbol_suffix(&mut self) {
        while self.peek().is_some_and(is_symbol_continuation) {
            self.cursor += 1;
        }
    }

    fn symbol_or_instance_name(&self, start: usize) -> FieldToken<'a> {
        let spelling = &self.input[start..self.cursor];
        if spelling.len() > 2 && spelling.starts_with(b"[") && spelling.ends_with(b"]") {
            FieldToken::InstanceName(Cow::Borrowed(&spelling[1..spelling.len() - 1]))
        } else {
            FieldToken::Symbol(Cow::Borrowed(spelling))
        }
    }

    /// `?name`, `$?name`, `?*global*`, or a bare `?`/`$?` wildcard.
    fn scan_variable(&mut self, start: usize) -> FieldToken<'a> {
        if self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || is_utf8_start(byte) || byte == b'*')
        {
            self.consume_symbol_suffix();
        }
        FieldToken::Other(self.slice(start))
    }

    fn scan_string(&mut self, notice: &mut Option<ScanNotice>) -> FieldToken<'a> {
        let content_start = self.cursor;
        // Borrow the input until an escape or backspace forces a copy.
        let mut decoded: Option<Vec<u8>> = None;
        let terminated = loop {
            let Some(byte) = self.peek() else { break false };
            if byte == b'"' {
                break true;
            }
            if matches!(byte, b'\\' | b'\x08') && decoded.is_none() {
                decoded = Some(self.input[content_start..self.cursor].to_vec());
            }
            self.cursor += 1;
            let mut appended = byte;
            if byte == b'\\' {
                if let Some(next) = self.peek() {
                    appended = next;
                    self.cursor += 1;
                } else {
                    // ScanString appends C's EOF (-1) once; it is not UTF-8.
                    appended = 0xff;
                }
            }
            if let Some(output) = decoded.as_mut() {
                append_string_byte(output, appended);
            }
        };
        let content_end = self.cursor;
        if terminated {
            self.cursor += 1;
        } else {
            *notice = Some(ScanNotice::UnterminatedString);
        }
        FieldToken::String(match decoded {
            Some(output) => Cow::Owned(output),
            None => Cow::Borrowed(&self.input[content_start..content_end]),
        })
    }

    fn scan_number(&mut self, start: usize, notice: &mut Option<ScanNotice>) -> FieldToken<'a> {
        let mut phase = NumberPhase::Sign;
        let mut mantissa_digit = false;
        let mut floating = false;
        loop {
            let byte = self.peek();
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
                            && matches!(self.input[self.cursor - 1], b'+' | b'-'))
                    {
                        mantissa_digit = false;
                    }
                    break;
                }
                _ => {
                    // Any other byte turns the whole token into a symbol.
                    self.cursor += 1;
                    self.consume_symbol_suffix();
                    return self.symbol_or_instance_name(start);
                }
            }
            self.cursor += 1;
        }
        let bytes = &self.input[start..self.cursor];
        if !mantissa_digit {
            return FieldToken::Symbol(Cow::Borrowed(bytes));
        }
        if floating {
            // The grammar above admits only ASCII decimal floats, which Rust
            // parses exactly, including signed zero, underflow and infinity.
            let text = std::str::from_utf8(bytes).expect("numeric grammar is ASCII");
            FieldToken::Float(text.parse().expect("scanned decimal float parses"))
        } else {
            let (value, overflow) = scan_integer(bytes);
            if overflow {
                *notice = Some(ScanNotice::IntegerRange);
            }
            FieldToken::Integer(value)
        }
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

fn is_utf8_start(byte: u8) -> bool {
    (0xc0..=0xf7).contains(&byte)
}

fn is_utf8_continuation(byte: u8) -> bool {
    (0x80..=0xbf).contains(&byte)
}

fn is_symbol_continuation(byte: u8) -> bool {
    !matches!(
        byte,
        b'<' | b'"' | b'(' | b')' | b'&' | b'|' | b'~' | b' ' | b';'
    ) && (byte.is_ascii_graphic() || is_utf8_start(byte) || is_utf8_continuation(byte))
}

fn number_delimiter(byte: Option<u8>, phase: NumberPhase) -> bool {
    match byte {
        None | Some(b'<' | b'"' | b'(' | b')' | b'&' | b'|' | b'~' | b' ' | b';') => true,
        Some(byte) => {
            !byte.is_ascii_graphic() && (phase == NumberPhase::Sign || !is_utf8_start(byte))
        }
    }
}

/// `ExpandStringWithChar`: a backspace erases the previous character.
fn append_string_byte(output: &mut Vec<u8>, byte: u8) {
    if byte == b'\x08' {
        while output.len() > 1 && output.last().copied().is_some_and(is_utf8_continuation) {
            output.pop();
        }
        output.pop();
    } else {
        output.push(byte);
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

    fn first(input: &[u8]) -> ScannedField<'_> {
        FieldScanner::new(input).next_token()
    }

    fn tokens(input: &[u8]) -> Vec<FieldToken<'_>> {
        let mut scanner = FieldScanner::new(input);
        let mut tokens = Vec::new();
        loop {
            let token = scanner.next_token().token;
            if token == FieldToken::Stop {
                return tokens;
            }
            tokens.push(token);
        }
    }

    fn symbol(bytes: &[u8]) -> FieldToken<'_> {
        FieldToken::Symbol(Cow::Borrowed(bytes))
    }

    fn string(bytes: &[u8]) -> FieldToken<'_> {
        FieldToken::String(Cow::Borrowed(bytes))
    }

    fn other(bytes: &[u8]) -> FieldToken<'_> {
        FieldToken::Other(Cow::Borrowed(bytes))
    }

    #[test]
    fn whitespace_and_comments_are_skipped_but_vertical_tab_is_unknown() {
        assert_eq!(
            tokens(b" \t\n\r\x0c; one\r; two\n42 tail"),
            [FieldToken::Integer(42), symbol(b"tail")]
        );
        for input in [b"".as_slice(), b" \t\r\n\x0c", b"; comment", b"\x03rest"] {
            assert_eq!(first(input).token, FieldToken::Stop);
        }
        assert_eq!(first(b"\x0b42").token, FieldToken::Unknown);
        assert_eq!(
            first("\u{a0}abc tail".as_bytes()).token,
            symbol("\u{a0}abc".as_bytes())
        );
    }

    #[test]
    fn delimiters_and_variables_become_print_forms() {
        assert_eq!(
            tokens(b"red(blue)&x|y~z<a ?v $?rest ?*g* ? $? $x"),
            [
                symbol(b"red"),
                other(b"("),
                symbol(b"blue"),
                other(b")"),
                other(b"&"),
                symbol(b"x"),
                other(b"|"),
                symbol(b"y"),
                other(b"~"),
                symbol(b"z"),
                symbol(b"<a"),
                other(b"?v"),
                other(b"$?rest"),
                other(b"?*g*"),
                other(b"?"),
                other(b"$?"),
                symbol(b"$x"),
            ]
        );
        assert_eq!(tokens(b"?2"), [other(b"?"), FieldToken::Integer(2)]);
    }

    #[test]
    fn bracketed_tokens_are_instance_names() {
        for (input, name) in [
            (b"[widget] rest".as_slice(), b"widget".as_slice()),
            (b"[a?b]", b"a?b"),
            (b"[a][b]", b"a][b"),
            (b"[a:b,MAIN::name]", b"a:b,MAIN::name"),
        ] {
            assert_eq!(
                first(input).token,
                FieldToken::InstanceName(Cow::Borrowed(name))
            );
        }
        for input in [b"[]".as_slice(), b"[", b"[open", b"close]", b"[x]tail"] {
            assert_eq!(first(input).token, symbol(input));
        }
        assert_eq!(tokens(b"[a<b]"), [symbol(b"[a"), symbol(b"<b]")]);
    }

    #[test]
    fn integers_saturate_with_a_notice() {
        for (input, expected) in [
            (b"42".as_slice(), 42),
            (b"-17", -17),
            (b"+17", 17),
            (b"00042", 42),
            (b"9223372036854775807", i64::MAX),
            (b"-9223372036854775808", i64::MIN),
        ] {
            assert_eq!(
                first(input),
                ScannedField {
                    token: FieldToken::Integer(expected),
                    notice: None
                }
            );
        }
        for (input, expected) in [
            (b"9223372036854775808 tail".as_slice(), i64::MAX),
            (b"-9223372036854775809", i64::MIN),
            (b"99999999999999999999999999", i64::MAX),
        ] {
            assert_eq!(
                first(input),
                ScannedField {
                    token: FieldToken::Integer(expected),
                    notice: Some(ScanNotice::IntegerRange)
                }
            );
        }
    }

    #[test]
    fn floats_and_malformed_numbers() {
        for (input, expected) in [
            (b"2.5".as_slice(), 2.5_f64),
            (b"2e3", 2000.0),
            (b".5", 0.5),
            (b"-.5", -0.5),
            (b"1.", 1.0),
            (b"1.e+2", 100.0),
            (b"-0.0", -0.0),
            (b"1.0e309", f64::INFINITY),
            (b"-1.0e-999", -0.0),
        ] {
            let FieldToken::Float(actual) = first(input).token else {
                panic!("not a float: {input:?}")
            };
            assert_eq!(actual.to_bits(), expected.to_bits(), "{input:?}");
        }
        for input in [
            b"42abc".as_slice(),
            b"1e",
            b"1e+",
            b"1.2.3",
            b"+",
            b"-",
            b".",
            b"NaN",
            b"0x10",
        ] {
            assert_eq!(first(input).token, symbol(input), "{input:?}");
        }
        assert_eq!(tokens(b"42abc<tail"), [symbol(b"42abc"), symbol(b"<tail")]);
    }

    #[test]
    fn quoted_strings_decode_escapes_and_backspaces() {
        for (input, expected) in [
            (b"\"two words\" tail".as_slice(), b"two words".as_slice()),
            (b"\"\"", b""),
            (b"\"a\\\"b\"", b"a\"b"),
            (b"\"a\\\\b\"", b"a\\b"),
            (b"\"a\\nb\"", b"anb"),
            (b"\"a\nb\"", b"a\nb"),
            (b"\"a\x08b\"", b"b"),
            ("\"aé\x08b\"".as_bytes(), b"ab"),
        ] {
            assert_eq!(
                first(input),
                ScannedField {
                    token: string(expected),
                    notice: None
                },
                "{input:?}"
            );
        }
        assert!(matches!(
            first(b"\"borrowed\"").token,
            FieldToken::String(Cow::Borrowed(_))
        ));
    }

    #[test]
    fn unterminated_strings_keep_their_text_with_a_notice() {
        for (input, expected) in [
            (b"\"unfinished".as_slice(), b"unfinished".as_slice()),
            (b"\"ab\0ignored\"", b"ab"),
            (b"\"end\\", b"end\xff"),
        ] {
            assert_eq!(
                first(input),
                ScannedField {
                    token: string(expected),
                    notice: Some(ScanNotice::UnterminatedString)
                }
            );
        }
    }

    #[test]
    fn input_ends_at_the_first_nul() {
        assert_eq!(tokens(b"42\0ignored"), [FieldToken::Integer(42)]);
    }

    #[test]
    fn only_non_values_have_print_forms() {
        assert_eq!(first(b"(").token.print_form(), Some(b"(".as_slice()));
        assert_eq!(
            first(b"\x7f").token.print_form(),
            Some(b"<<<unprintable character>>>".as_slice())
        );
        for input in [b"42".as_slice(), b"1.0", b"red", b"\"red\"", b"[red]"] {
            assert!(first(input).token.print_form().is_none());
        }
    }
}

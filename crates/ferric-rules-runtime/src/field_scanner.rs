//! Incremental byte field scanning for CLIPS runtime string/router consumers.
//!
//! The byte grammar follows CLIPS 6.30 `scanner.c`; quoted backspace processing
//! follows `utility.c::ExpandStringWithChar`. The caller owns value construction,
//! diagnostics, evaluator flags, and builtin-specific STOP/UNKNOWN handling.

use std::borrow::Cow;
use std::ops::Range;

/// Distinguishes `OpenStringSource`'s first-NUL limit from a router byte stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceKind {
    ClipsString,
    // Retain the reference stream contract in tests until read adopts it.
    #[cfg(test)]
    ByteStream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScanPolicy {
    pub source: SourceKind,
    pub report_incomplete_string: bool,
}

impl Default for ScanPolicy {
    fn default() -> Self {
        Self {
            source: SourceKind::ClipsString,
            report_incomplete_string: true,
        }
    }
}

/// A cursor into one complete input buffer. Byte offsets never split ownership.
/// Future streaming adapters can retain the unread suffix after `next_offset`;
/// a partial input buffer must not be presented as EOF before it is complete.
#[derive(Debug)]
pub(crate) struct FieldScanner<'a> {
    input: &'a [u8],
    cursor: usize,
    end: usize,
    policy: ScanPolicy,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScannedField<'a> {
    pub token: FieldToken<'a>,
    /// Excludes leading whitespace/comments and any peeked delimiter.
    pub source_range: Range<usize>,
    pub next_offset: usize,
    /// These notices do not imply evaluator Error or Halt.
    pub notices: Vec<ScanNotice>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldToken<'a> {
    Integer(i64),
    Float(f64),
    Symbol(Cow<'a, [u8]>),
    String(Cow<'a, [u8]>),
    /// The exact name payload without its outer brackets.
    InstanceName(Cow<'a, [u8]>),
    Variable {
        kind: VariableKind,
        /// Global names omit the two `*` delimiters as well as the prefix.
        name: Cow<'a, [u8]>,
        spelling: Cow<'a, [u8]>,
    },
    Wildcard(WildcardKind),
    Punctuation(PunctuationKind),
    Unknown {
        byte: u8,
    },
    Stop(StopReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VariableKind {
    Single,
    Multifield,
    Global,
    MultifieldGlobal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WildcardKind {
    Single,
    Multifield,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PunctuationKind {
    LeftParen,
    RightParen,
    Not,
    Or,
    And,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StopReason {
    EndOfInput,
    Nul,
    EndOfText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RangeDirection {
    AboveMaximum,
    BelowMinimum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanNoticeChannel {
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ScanNotice {
    IntegerRange {
        direction: RangeDirection,
        range: Range<usize>,
    },
    UnterminatedString {
        range: Range<usize>,
        escaped_eof: bool,
    },
}

impl ScanNotice {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::IntegerRange { .. } | Self::UnterminatedString { .. } => "SCANNER1",
        }
    }

    pub(crate) const fn channel(&self) -> ScanNoticeChannel {
        match self {
            Self::IntegerRange { .. } => ScanNoticeChannel::Warning,
            Self::UnterminatedString { .. } => ScanNoticeChannel::Error,
        }
    }

    pub(crate) const fn message(&self) -> &'static str {
        match self {
            Self::IntegerRange { .. } => "Over or underflow of long long integer.",
            Self::UnterminatedString { .. } => "Encountered End-Of-File while scanning a string",
        }
    }

    /// Exact reference router bytes, including its distinct newline conventions.
    pub(crate) const fn router_bytes(&self) -> &'static [u8] {
        match self {
            Self::IntegerRange { .. } => {
                b"[SCANNER1] WARNING: Over or underflow of long long integer.\n"
            }
            Self::UnterminatedString { .. } => {
                b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n"
            }
        }
    }

    pub(crate) fn range(&self) -> &Range<usize> {
        match self {
            Self::IntegerRange { range, .. } | Self::UnterminatedString { range, .. } => range,
        }
    }
}

impl FieldToken<'_> {
    /// Owns only the selected token, never its unscanned input suffix. This lets
    /// callers release a symbol-table read borrow before interning the result.
    pub(crate) fn into_owned(self) -> FieldToken<'static> {
        match self {
            Self::Integer(value) => FieldToken::Integer(value),
            Self::Float(value) => FieldToken::Float(value),
            Self::Symbol(value) => FieldToken::Symbol(Cow::Owned(value.into_owned())),
            Self::String(value) => FieldToken::String(Cow::Owned(value.into_owned())),
            Self::InstanceName(value) => FieldToken::InstanceName(Cow::Owned(value.into_owned())),
            Self::Variable {
                kind,
                name,
                spelling,
            } => FieldToken::Variable {
                kind,
                name: Cow::Owned(name.into_owned()),
                spelling: Cow::Owned(spelling.into_owned()),
            },
            Self::Wildcard(kind) => FieldToken::Wildcard(kind),
            Self::Punctuation(kind) => FieldToken::Punctuation(kind),
            Self::Unknown { byte } => FieldToken::Unknown { byte },
            Self::Stop(reason) => FieldToken::Stop(reason),
        }
    }

    /// Print forms used by string-to-field/explode$/read for non-atomic tokens.
    /// Atomic tokens are converted directly to typed runtime Values; their
    /// presentation belongs to the existing numeric/lexeme printer, not here.
    pub(crate) fn non_atomic_print_form(&self) -> Option<Cow<'_, [u8]>> {
        let bytes: &[u8] = match self {
            Self::Variable { spelling, .. } => spelling.as_ref(),
            Self::Wildcard(WildcardKind::Single) => b"?",
            Self::Wildcard(WildcardKind::Multifield) => b"$?",
            Self::Punctuation(PunctuationKind::LeftParen) => b"(",
            Self::Punctuation(PunctuationKind::RightParen) => b")",
            Self::Punctuation(PunctuationKind::Not) => b"~",
            Self::Punctuation(PunctuationKind::Or) => b"|",
            Self::Punctuation(PunctuationKind::And) => b"&",
            Self::Unknown { .. } => b"<<<unprintable character>>>",
            Self::Stop(_) => b"",
            Self::Integer(_)
            | Self::Float(_)
            | Self::Symbol(_)
            | Self::String(_)
            | Self::InstanceName(_) => return None,
        };
        Some(Cow::Borrowed(bytes))
    }
}

impl<'a> FieldScanner<'a> {
    pub(crate) fn new(input: &'a [u8], policy: ScanPolicy) -> Self {
        // OpenStringSource itself applies strlen before scanning. Computing the
        // limit does not tokenize, validate, or emit notices for the suffix.
        let end = match policy.source {
            SourceKind::ClipsString => input
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(input.len()),
            #[cfg(test)]
            SourceKind::ByteStream => input.len(),
        };
        Self {
            input,
            cursor: 0,
            end,
            policy,
        }
    }

    pub(crate) fn clips_string(input: &'a [u8]) -> Self {
        Self::new(input, ScanPolicy::default())
    }

    #[cfg(test)]
    pub(crate) const fn position(&self) -> usize {
        self.cursor
    }

    #[cfg(test)]
    pub(crate) fn remaining(&self) -> &'a [u8] {
        &self.input[self.cursor..self.end]
    }

    pub(crate) fn next_token(&mut self) -> ScannedField<'a> {
        self.skip_whitespace_and_comments();
        let start = self.cursor;
        let mut notices = Vec::new();
        let token = match self.peek() {
            None => FieldToken::Stop(StopReason::EndOfInput),
            Some(byte) if byte.is_ascii_alphabetic() || is_utf8_start(byte) => {
                self.consume_symbol_suffix();
                FieldToken::Symbol(Cow::Borrowed(&self.input[start..self.cursor]))
            }
            Some(b'0'..=b'9' | b'-' | b'+' | b'.') => self.scan_number(start, &mut notices),
            Some(b'"') => {
                self.cursor += 1;
                self.scan_string(start, &mut notices)
            }
            Some(b'?') => {
                self.cursor += 1;
                self.scan_variable(start, false)
            }
            Some(b'$') => {
                self.cursor += 1;
                if self.peek() == Some(b'?') {
                    self.cursor += 1;
                    self.scan_variable(start, true)
                } else {
                    self.consume_symbol_suffix();
                    FieldToken::Symbol(Cow::Borrowed(&self.input[start..self.cursor]))
                }
            }
            Some(b'<') => {
                self.cursor += 1;
                self.consume_symbol_suffix();
                FieldToken::Symbol(Cow::Borrowed(&self.input[start..self.cursor]))
            }
            Some(byte @ (b'(' | b')' | b'~' | b'|' | b'&')) => {
                self.cursor += 1;
                let kind = match byte {
                    b'(' => PunctuationKind::LeftParen,
                    b')' => PunctuationKind::RightParen,
                    b'~' => PunctuationKind::Not,
                    b'|' => PunctuationKind::Or,
                    _ => PunctuationKind::And,
                };
                FieldToken::Punctuation(kind)
            }
            Some(byte @ (0 | 3)) => {
                self.cursor += 1;
                FieldToken::Stop(if byte == 0 {
                    StopReason::Nul
                } else {
                    StopReason::EndOfText
                })
            }
            Some(byte) if byte.is_ascii_graphic() => {
                self.consume_symbol_suffix();
                self.symbol_or_instance(start)
            }
            Some(byte) => {
                self.cursor += 1;
                FieldToken::Unknown { byte }
            }
        };
        ScannedField {
            token,
            source_range: start..self.cursor,
            next_offset: self.cursor,
            notices,
        }
    }

    fn peek(&self) -> Option<u8> {
        (self.cursor < self.end).then(|| self.input[self.cursor])
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\n' | b'\x0c' | b'\r' | b'\t') => self.cursor += 1,
                Some(b';') => {
                    self.cursor += 1;
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

    fn symbol_or_instance(&self, start: usize) -> FieldToken<'a> {
        let spelling = &self.input[start..self.cursor];
        if spelling.len() > 2 && spelling.first() == Some(&b'[') && spelling.last() == Some(&b']') {
            FieldToken::InstanceName(Cow::Borrowed(&spelling[1..spelling.len() - 1]))
        } else {
            FieldToken::Symbol(Cow::Borrowed(spelling))
        }
    }

    fn scan_variable(&mut self, start: usize, multifield: bool) -> FieldToken<'a> {
        if !self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || is_utf8_start(byte) || byte == b'*')
        {
            return FieldToken::Wildcard(if multifield {
                WildcardKind::Multifield
            } else {
                WildcardKind::Single
            });
        }
        let name_start = self.cursor;
        self.consume_symbol_suffix();
        let name = &self.input[name_start..self.cursor];
        let global = name.len() > 1 && name.first() == Some(&b'*') && name.last() == Some(&b'*');
        let kind = match (multifield, global) {
            (false, false) => VariableKind::Single,
            (true, false) => VariableKind::Multifield,
            (false, true) => VariableKind::Global,
            (true, true) => VariableKind::MultifieldGlobal,
        };
        FieldToken::Variable {
            kind,
            name: Cow::Borrowed(if global {
                &name[1..name.len() - 1]
            } else {
                name
            }),
            spelling: Cow::Borrowed(&self.input[start..self.cursor]),
        }
    }

    fn scan_string(&mut self, start: usize, notices: &mut Vec<ScanNotice>) -> FieldToken<'a> {
        let content_start = self.cursor;
        let mut decoded: Option<Vec<u8>> = None;
        let mut escaped_eof = false;
        let terminated = loop {
            let Some(byte) = self.peek() else { break false };
            if byte == b'"' {
                break true;
            }
            let mut appended = byte;
            if matches!(byte, b'\\' | b'\x08') && decoded.is_none() {
                decoded = Some(self.input[content_start..self.cursor].to_vec());
            }
            self.cursor += 1;
            if byte == b'\\' {
                if let Some(next) = self.peek() {
                    appended = next;
                    self.cursor += 1;
                } else {
                    // ScanString forwards C EOF (-1) to its byte builder once.
                    appended = 0xff;
                    escaped_eof = true;
                }
            }
            if let Some(output) = decoded.as_mut() {
                append_string_byte(output, appended);
            }
        };
        let content_end = self.cursor;
        if terminated {
            self.cursor += 1;
        } else if self.policy.report_incomplete_string {
            notices.push(ScanNotice::UnterminatedString {
                range: start..self.cursor,
                escaped_eof,
            });
        }
        // A router can deliver NUL inside quotes. The reference still consumes
        // the closing quote, then EnvAddSymbol uses only the C-string prefix.
        let value = if let Some(mut output) = decoded {
            if let Some(nul) = output.iter().position(|&byte| byte == 0) {
                output.truncate(nul);
            }
            Cow::Owned(output)
        } else {
            let bytes = &self.input[content_start..content_end];
            let end = bytes
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(bytes.len());
            Cow::Borrowed(&bytes[..end])
        };
        FieldToken::String(value)
    }

    fn scan_number(&mut self, start: usize, notices: &mut Vec<ScanNotice>) -> FieldToken<'a> {
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
                    if phase == NumberPhase::ExponentStart
                        || (phase == NumberPhase::ExponentValue
                            && matches!(self.input[self.cursor - 1], b'+' | b'-'))
                    {
                        mantissa_digit = false;
                    }
                    break;
                }
                _ => {
                    // The offending byte belongs to the symbol prefix already
                    // consumed by ScanNumber; ScanSymbol then reads its suffix.
                    self.cursor += 1;
                    self.consume_symbol_suffix();
                    return self.symbol_or_instance(start);
                }
            }
            self.cursor += 1;
        }
        let bytes = &self.input[start..self.cursor];
        if !mantissa_digit {
            return FieldToken::Symbol(Cow::Borrowed(bytes));
        }
        if floating {
            // Only ASCII grammar accepted by the phase machine reaches this
            // conversion. Rust's locale-independent conversion preserves the
            // classified value, including signed zero, underflow and infinity.
            let text = std::str::from_utf8(bytes).expect("numeric grammar contains only ASCII");
            FieldToken::Float(text.parse().expect("classified decimal float is valid"))
        } else {
            let (value, direction) = scan_integer(bytes);
            if let Some(direction) = direction {
                notices.push(ScanNotice::IntegerRange {
                    direction,
                    range: start..self.cursor,
                });
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

fn append_string_byte(output: &mut Vec<u8>, byte: u8) {
    if byte == b'\x08' {
        while output.len() > 1
            && output
                .last()
                .is_some_and(|&last| is_utf8_continuation(last))
        {
            output.pop();
        }
        output.pop();
    } else {
        output.push(byte);
    }
}

fn scan_integer(bytes: &[u8]) -> (i64, Option<RangeDirection>) {
    let negative = bytes.first() == Some(&b'-');
    let digits = if matches!(bytes.first(), Some(b'+' | b'-')) {
        &bytes[1..]
    } else {
        bytes
    };
    let limit = if negative {
        1_u64 << 63
    } else {
        i64::MAX as u64
    };
    let mut magnitude = 0_u64;
    let mut overflow = false;
    for &digit in digits {
        match magnitude
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit - b'0')))
        {
            Some(value) if value <= limit => magnitude = value,
            _ => overflow = true,
        }
    }
    if overflow {
        return if negative {
            (i64::MIN, Some(RangeDirection::BelowMinimum))
        } else {
            (i64::MAX, Some(RangeDirection::AboveMaximum))
        };
    }
    if negative && magnitude == 1_u64 << 63 {
        (i64::MIN, None)
    } else {
        let value =
            i64::try_from(magnitude).expect("integer magnitude was checked against its limit");
        (if negative { -value } else { value }, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(input: &[u8]) -> ScannedField<'_> {
        FieldScanner::clips_string(input).next_token()
    }

    fn symbol(bytes: &[u8]) -> FieldToken<'_> {
        FieldToken::Symbol(Cow::Borrowed(bytes))
    }

    fn string(bytes: &[u8]) -> FieldToken<'_> {
        FieldToken::String(Cow::Borrowed(bytes))
    }

    fn stream(input: &[u8]) -> FieldScanner<'_> {
        FieldScanner::new(
            input,
            ScanPolicy {
                source: SourceKind::ByteStream,
                report_incomplete_string: true,
            },
        )
    }

    #[test]
    fn first_token_leaves_malformed_suffix_unscanned() {
        let mut scanner = FieldScanner::clips_string(b" \t42 \"unfinished\\");
        let field = scanner.next_token();
        assert_eq!(field.token, FieldToken::Integer(42));
        assert_eq!(field.source_range, 2..4);
        assert_eq!(field.next_offset, 4);
        assert_eq!(scanner.position(), 4);
        assert_eq!(scanner.remaining(), b" \"unfinished\\");
        assert!(field.notices.is_empty());
    }

    #[test]
    fn comments_and_only_reference_whitespace_are_skipped() {
        let input = b" \t\n\r\x0c; one\r; two\n42 tail";
        let field = first(input);
        assert_eq!(field.token, FieldToken::Integer(42));
        assert_eq!(&input[field.source_range.clone()], b"42");
        assert_eq!(&input[field.next_offset..], b" tail");
        for input in [
            b"".as_slice(),
            b" \t\r\n\x0c",
            b"; comment",
            b";one\r;two\n",
        ] {
            let field = first(input);
            assert_eq!(field.token, FieldToken::Stop(StopReason::EndOfInput));
            assert_eq!(field.source_range, input.len()..input.len());
        }
        assert_eq!(first(b"\x0b42").token, FieldToken::Unknown { byte: 11 });
        assert_eq!(first(b"\x0c42").token, FieldToken::Integer(42));
        assert_eq!(
            first("\u{00a0}abc tail".as_bytes()).token,
            symbol("\u{00a0}abc".as_bytes())
        );
    }

    #[test]
    fn delimiters_are_preserved_for_incremental_consumers() {
        let mut scanner = FieldScanner::clips_string(b"red(blue)&x|y~z<a");
        let expected = [
            symbol(b"red"),
            FieldToken::Punctuation(PunctuationKind::LeftParen),
            symbol(b"blue"),
            FieldToken::Punctuation(PunctuationKind::RightParen),
            FieldToken::Punctuation(PunctuationKind::And),
            symbol(b"x"),
            FieldToken::Punctuation(PunctuationKind::Or),
            symbol(b"y"),
            FieldToken::Punctuation(PunctuationKind::Not),
            symbol(b"z"),
            symbol(b"<a"),
        ];
        for token in expected {
            assert_eq!(scanner.next_token().token, token);
        }
        assert_eq!(
            scanner.next_token().token,
            FieldToken::Stop(StopReason::EndOfInput)
        );
        assert_eq!(first(b"red;comment").next_offset, 3);
        assert_eq!(first(b"red\"quoted\"").next_offset, 3);
        assert_eq!(first(b"red\x0bblue").next_offset, 3);
    }

    #[test]
    fn symbol_byte_classes_do_not_require_valid_utf8() {
        for bytes in [
            b"\xc0\x80tail".as_slice(),
            b"\xf7\xbf",
            b"a\x80b",
            b"a\xc0\x80b",
        ] {
            assert_eq!(first(bytes).token, symbol(bytes));
        }
        for byte in [0x80, 0xbf, 0xf8, 0xff, 0x7f] {
            assert_eq!(first(&[byte]).token, FieldToken::Unknown { byte });
        }
        assert_eq!(first(b"a\xf8rest").token, symbol(b"a"));
        assert_eq!(first(b"a\xffrest").next_offset, 1);
    }

    #[test]
    fn complete_bracket_tokens_have_distinct_instance_name_identity() {
        for (input, name) in [
            (b"[widget] rest".as_slice(), b"widget".as_slice()),
            (b"[a?b] rest", b"a?b"),
            (b"[a\\b]", b"a\\b"),
            (b"[a][b]", b"a][b"),
            (b"[[]]", b"[]"),
            (b"[a:b,MAIN::name]", b"a:b,MAIN::name"),
            (b"[\xc0\x80]", b"\xc0\x80"),
        ] {
            assert_eq!(
                first(input).token,
                FieldToken::InstanceName(Cow::Borrowed(name))
            );
        }
        for input in [b"[]".as_slice(), b"[", b"[open", b"close]", b"[x]tail"] {
            assert_eq!(first(input).token, symbol(input));
        }
        let mut scanner = FieldScanner::clips_string(b"[a<b]");
        assert_eq!(scanner.next_token().token, symbol(b"[a"));
        assert_eq!(scanner.next_token().token, symbol(b"<b]"));
    }

    #[test]
    fn variable_kinds_keep_original_spelling_and_unwrapped_global_names() {
        for (input, kind, name) in [
            (b"?x".as_slice(), VariableKind::Single, b"x".as_slice()),
            (b"$?x", VariableKind::Multifield, b"x"),
            (b"?*global*", VariableKind::Global, b"global"),
            (b"$?*global*", VariableKind::MultifieldGlobal, b"global"),
            (b"?**", VariableKind::Global, b""),
            (b"?*", VariableKind::Single, b"*"),
            (b"?*incomplete", VariableKind::Single, b"*incomplete"),
            (b"?a?b", VariableKind::Single, b"a?b"),
            (b"?\xc0\x80", VariableKind::Single, b"\xc0\x80"),
        ] {
            let token = first(input).token;
            assert_eq!(
                token,
                FieldToken::Variable {
                    kind,
                    name: Cow::Borrowed(name),
                    spelling: Cow::Borrowed(input),
                }
            );
            assert_eq!(token.non_atomic_print_form().as_deref(), Some(input));
        }
    }

    #[test]
    fn wildcard_lookahead_does_not_consume_the_following_token() {
        let mut scanner = FieldScanner::clips_string(b"?2 $?2 ?_x $?_x $");
        let expected = [
            FieldToken::Wildcard(WildcardKind::Single),
            FieldToken::Integer(2),
            FieldToken::Wildcard(WildcardKind::Multifield),
            FieldToken::Integer(2),
            FieldToken::Wildcard(WildcardKind::Single),
            symbol(b"_x"),
            FieldToken::Wildcard(WildcardKind::Multifield),
            symbol(b"_x"),
            symbol(b"$"),
        ];
        for token in expected {
            assert_eq!(scanner.next_token().token, token);
        }
        assert_eq!(first(b"$").next_offset, 1);
        assert_eq!(first(b"$?").next_offset, 2);
        assert_eq!(first(b"$value").token, symbol(b"$value"));
        assert_eq!(first(b"<- rest").token, symbol(b"<-"));
    }

    #[test]
    fn integer_values_preserve_full_range_and_decimal_spelling_rules() {
        for (input, expected) in [
            (b"42".as_slice(), 42),
            (b"-17", -17),
            (b"+17", 17),
            (b"00042", 42),
            (b"-0000", 0),
            (b"9007199254740993", 9_007_199_254_740_993),
            (b"9223372036854775807", i64::MAX),
            (b"-9223372036854775808", i64::MIN),
        ] {
            let field = first(input);
            assert_eq!(field.token, FieldToken::Integer(expected));
            assert_eq!(field.next_offset, input.len());
            assert!(field.notices.is_empty());
        }
    }

    #[test]
    fn integer_overflow_saturates_once_without_consuming_the_next_field() {
        for (input, expected, direction) in [
            (
                b"9223372036854775808 tail".as_slice(),
                i64::MAX,
                RangeDirection::AboveMaximum,
            ),
            (
                b"-9223372036854775809 tail",
                i64::MIN,
                RangeDirection::BelowMinimum,
            ),
            (
                b"9999999999999999999999999999999 tail",
                i64::MAX,
                RangeDirection::AboveMaximum,
            ),
            (
                b"-9999999999999999999999999999999 tail",
                i64::MIN,
                RangeDirection::BelowMinimum,
            ),
        ] {
            let field = first(input);
            assert_eq!(field.token, FieldToken::Integer(expected));
            assert_eq!(&input[field.next_offset..], b" tail");
            assert_eq!(
                field.notices,
                [ScanNotice::IntegerRange {
                    direction,
                    range: field.source_range,
                }]
            );
        }
        // A malformed numeric token is a symbol, with no numeric range notice.
        let symbol_input = b"9999999999999999999999999999999x";
        let field = first(symbol_input);
        assert_eq!(field.token, symbol(symbol_input));
        assert!(field.notices.is_empty());
    }

    #[test]
    fn decimal_float_forms_preserve_sign_overflow_and_underflow() {
        for (input, expected) in [
            (b"2.5".as_slice(), 2.5_f64),
            (b"2e3", 2000.0),
            (b".5", 0.5),
            (b"-.5", -0.5),
            (b"+.5", 0.5),
            (b"1.", 1.0),
            (b"1.e+2", 100.0),
            (b"-0.0", -0.0),
            (b"1.0e309", f64::INFINITY),
            (b"-1.0e309", f64::NEG_INFINITY),
            (b"1.0e-999", 0.0),
            (b"-1.0e-999", -0.0),
        ] {
            let field = first(input);
            let FieldToken::Float(actual) = field.token else {
                panic!("not a float: {input:?}")
            };
            assert_eq!(actual.to_bits(), expected.to_bits(), "{input:?}");
            assert!(field.notices.is_empty());
        }
    }

    #[test]
    fn malformed_numeric_spellings_become_whole_symbols() {
        for input in [
            b"42abc".as_slice(),
            b"1.abc",
            b"1e",
            b"1e+",
            b"1e-",
            b"1.2.3",
            b"+",
            b"-",
            b".",
            b"+.",
            b"-.e1",
            b"+e1",
            b"1e+bad",
            b"NaN",
            b"inf",
            b"0x10",
            b"1_000",
        ] {
            let field = first(input);
            assert_eq!(field.token, symbol(input), "{input:?}");
            assert_eq!(field.next_offset, input.len());
            assert!(field.notices.is_empty());
        }
        let mut scanner = FieldScanner::clips_string(b"42abc<tail");
        assert_eq!(scanner.next_token().token, symbol(b"42abc"));
        assert_eq!(scanner.next_token().token, symbol(b"<tail"));
    }

    #[test]
    fn numeric_utf8_start_and_continuation_bytes_have_different_boundaries() {
        // ScanNumber admits an old UTF8-start byte into symbol fallback, while
        // a standalone continuation byte is left for GetToken as UNKNOWN.
        assert_eq!(first(b"1\xc0\x80tail").token, symbol(b"1\xc0\x80tail"));
        assert_eq!(first(b"1.2\xc0\x80tail").token, symbol(b"1.2\xc0\x80tail"));
        let mut scanner = FieldScanner::clips_string(b"1\x80tail");
        assert_eq!(scanner.next_token().token, FieldToken::Integer(1));
        assert_eq!(
            scanner.next_token().token,
            FieldToken::Unknown { byte: 0x80 }
        );
        assert_eq!(scanner.next_token().token, symbol(b"tail"));
        assert_eq!(first(b"1e+\x80tail").token, symbol(b"1e+"));
        assert_eq!(first(b"42\x0b7").token, FieldToken::Integer(42));
    }

    #[test]
    fn quoted_strings_decode_bytes_without_c_style_escape_substitution() {
        for (input, expected) in [
            (b"\"two words\" tail".as_slice(), b"two words".as_slice()),
            (b"\"\" tail", b""),
            (b"\"a\\\"b\" tail", b"a\"b"),
            (b"\"a\\\\b\" tail", b"a\\b"),
            (b"\"a\\nb\" tail", b"anb"),
            (b"\"a\\tb\" tail", b"atb"),
            (b"\"a\\qb\" tail", b"aqb"),
            (b"\"a\nb\" tail", b"a\nb"),
            (b"\"a\x0bb\" tail", b"a\x0bb"),
            (b"\"a\xffb\" tail", b"a\xffb"),
            (b"\"a\\\xffb\" tail", b"a\xffb"),
        ] {
            let field = first(input);
            assert_eq!(field.token, string(expected), "{input:?}");
            assert_eq!(&input[field.next_offset..], b" tail");
            assert!(field.notices.is_empty());
        }
    }

    #[test]
    fn untransformed_quoted_bytes_borrow_the_token_slice() {
        assert!(matches!(
            first(b"\"borrowed\" tail").token,
            FieldToken::String(Cow::Borrowed(b"borrowed"))
        ));
        assert!(matches!(
            first(b"\"a\\nb\"").token,
            FieldToken::String(Cow::Owned(_))
        ));
    }

    #[test]
    fn quoted_backspace_uses_reference_byte_erasure() {
        for (input, expected) in [
            (b"\"a\x08b\"".as_slice(), b"b".as_slice()),
            (b"\"\x08\x08b\"", b"b"),
            (b"\"a\xc3\xa9\x08b\"", b"ab"),
            (b"\"a\xf0\x9f\x98\x80\x08b\"", b"ab"),
            (b"\"\x80\xbf\x08b\"", b"b"),
            (b"\"a\xff\x08b\"", b"ab"),
            (b"\"a\\\x08b\"", b"b"),
        ] {
            assert_eq!(first(input).token, string(expected), "{input:?}");
        }
    }

    #[test]
    fn incomplete_strings_return_partial_bytes_and_recoverable_notice() {
        for (input, expected, escaped_eof) in [
            (b"\"unfinished".as_slice(), b"unfinished".as_slice(), false),
            (b"\"unfinished\\", b"unfinished\xff", true),
            (b"\"", b"", false),
            (b"\"\\", b"\xff", true),
        ] {
            let field = first(input);
            assert_eq!(field.token, string(expected));
            assert_eq!(field.next_offset, input.len());
            assert_eq!(
                field.notices,
                [ScanNotice::UnterminatedString {
                    range: 0..input.len(),
                    escaped_eof,
                }]
            );
        }
    }

    #[test]
    fn ignore_completion_policy_suppresses_notice_but_preserves_partial_value() {
        let mut scanner = FieldScanner::new(
            b"\"end\\",
            ScanPolicy {
                report_incomplete_string: false,
                ..ScanPolicy::default()
            },
        );
        let field = scanner.next_token();
        assert_eq!(field.token, string(b"end\xff"));
        assert!(field.notices.is_empty());
    }

    #[test]
    fn string_source_stops_at_first_nul_even_inside_quotes() {
        let mut scanner = FieldScanner::clips_string(b"42\0ignored");
        assert_eq!(scanner.next_token().token, FieldToken::Integer(42));
        assert_eq!(
            scanner.next_token().token,
            FieldToken::Stop(StopReason::EndOfInput)
        );
        assert_eq!(scanner.position(), 2);
        assert!(scanner.remaining().is_empty());
        let partial = first(b"\"ab\0ignored\"");
        assert_eq!(partial.token, string(b"ab"));
        assert_eq!(partial.next_offset, 3);
        assert_eq!(
            partial.notices,
            [ScanNotice::UnterminatedString {
                range: 0..3,
                escaped_eof: false,
            }]
        );
        assert_eq!(
            first(b"\0ignored").token,
            FieldToken::Stop(StopReason::EndOfInput)
        );
    }

    #[test]
    fn stream_stop_bytes_are_consumed_and_do_not_permanently_latch_eof() {
        let mut scanner = stream(b"\0first\x03second");
        assert_eq!(
            scanner.next_token().token,
            FieldToken::Stop(StopReason::Nul)
        );
        assert_eq!(scanner.next_token().token, symbol(b"first"));
        assert_eq!(
            scanner.next_token().token,
            FieldToken::Stop(StopReason::EndOfText)
        );
        assert_eq!(scanner.next_token().token, symbol(b"second"));
        let end = scanner.next_token();
        assert_eq!(end.token, FieldToken::Stop(StopReason::EndOfInput));
        assert_eq!(scanner.next_token(), end);
    }

    #[test]
    fn stream_quoted_nul_truncates_value_after_consuming_full_token() {
        let mut scanner = stream(b"\"ab\0ignored\" tail");
        let field = scanner.next_token();
        assert_eq!(field.token, string(b"ab"));
        assert!(field.notices.is_empty());
        assert_eq!(scanner.remaining(), b" tail");
        assert_eq!(scanner.next_token().token, symbol(b"tail"));
        // A later backspace can erase the builder's NUL before C-string intern.
        assert_eq!(stream(b"\"ab\0\x08c\"").next_token().token, string(b"abc"));
        // Stop bytes inside comments are ignored until a line ending or EOF.
        assert_eq!(
            stream(b"; x\0y\x03z\n42").next_token().token,
            FieldToken::Integer(42)
        );
    }

    #[test]
    fn only_non_atomic_tokens_offer_scanner_print_forms() {
        for (input, expected) in [
            (b"?".as_slice(), b"?".as_slice()),
            (b"$?", b"$?"),
            (b"(", b"("),
            (b")", b")"),
            (b"&", b"&"),
            (b"|", b"|"),
            (b"~", b"~"),
            (b"\x0b", b"<<<unprintable character>>>"),
            (b"", b""),
        ] {
            assert_eq!(
                first(input).token.non_atomic_print_form().as_deref(),
                Some(expected)
            );
        }
        for input in [b"42".as_slice(), b"1.0", b"red", b"\"red\"", b"[red]"] {
            assert!(first(input).token.non_atomic_print_form().is_none());
        }
    }

    #[test]
    fn selected_token_can_outlive_its_input_without_copying_the_suffix() {
        let owned = {
            let mut input = b"\"selected\" ".to_vec();
            input.extend(std::iter::repeat(b'x').take(1024));
            first(&input).token.into_owned()
        };
        assert_eq!(owned, string(b"selected"));
        assert!(matches!(owned, FieldToken::String(Cow::Owned(bytes)) if bytes.len() == 8));
        let variable = first(b"$?*name*").token.into_owned();
        assert_eq!(
            variable.non_atomic_print_form().as_deref(),
            Some(b"$?*name*".as_slice())
        );
    }

    #[test]
    fn notice_metadata_has_exact_reference_channel_and_byte_text() {
        let integer = ScanNotice::IntegerRange {
            direction: RangeDirection::AboveMaximum,
            range: 3..22,
        };
        let incomplete = ScanNotice::UnterminatedString {
            range: 7..12,
            escaped_eof: true,
        };
        assert_eq!(integer.code(), "SCANNER1");
        assert_eq!(integer.channel(), ScanNoticeChannel::Warning);
        assert_eq!(integer.message(), "Over or underflow of long long integer.");
        assert_eq!(integer.range(), &(3..22));
        assert_eq!(
            integer.router_bytes(),
            b"[SCANNER1] WARNING: Over or underflow of long long integer.\n"
        );
        assert_eq!(incomplete.code(), "SCANNER1");
        assert_eq!(incomplete.channel(), ScanNoticeChannel::Error);
        assert_eq!(
            incomplete.message(),
            "Encountered End-Of-File while scanning a string"
        );
        assert_eq!(incomplete.range(), &(7..12));
        assert_eq!(
            incomplete.router_bytes(),
            b"\n[SCANNER1] Encountered End-Of-File while scanning a string\n"
        );
    }

    #[test]
    fn every_single_router_byte_makes_progress_except_actual_eof() {
        for byte in u8::MIN..=u8::MAX {
            let input = [byte];
            let mut scanner = stream(&input);
            let field = scanner.next_token();
            assert_eq!(field.next_offset, 1, "byte {byte}");
            assert_eq!(
                scanner.next_token().token,
                FieldToken::Stop(StopReason::EndOfInput)
            );
        }
    }
}

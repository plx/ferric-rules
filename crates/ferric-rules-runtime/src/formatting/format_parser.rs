//! A byte scanner for the CLIPS 6.30 `format` control string.
//!
//! Scanning/counting deliberately precedes operand evaluation. The scanner's
//! digits/dot/minus admission is broader than canonical printf syntax, so an
//! accepted directive can carry a noncanonical modifier analysis. It still
//! consumes one operand. No rendering, evaluator state, or output-limit policy
//! belongs in this module.

use super::format_types::FormatSpec;
use std::ops::Range;

/// `FLAG_MAX - 5` in CLIPS. The leading percent already occupies one byte.
const FRAGMENT_CAPACITY: usize = 75;

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

    pub(crate) fn as_byte(self) -> u8 {
        match self {
            Self::Decimal => b'd',
            Self::Octal => b'o',
            Self::Hex => b'x',
            Self::Unsigned => b'u',
            Self::Character => b'c',
            Self::Lexeme => b's',
            Self::Scientific => b'e',
            Self::Fixed => b'f',
            Self::General => b'g',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericParameter {
    Width,
    Precision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SpecAnalysis {
    Canonical(FormatSpec),
    /// The scanner admitted the bytes, but the ordinary flags/width/precision
    /// grammar did not. `offset` is absolute in the original control string.
    NonCanonical {
        offset: usize,
        byte: u8,
    },
    /// The canonical spelling represents a number larger than `usize::MAX`.
    /// This is a resource classification, not a CLIPS invalid-flag error.
    ParameterOverflow {
        parameter: NumericParameter,
        range: Range<usize>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Directive<'a> {
    /// Includes the initial percent and final conversion, in source bytes.
    pub(crate) range: Range<usize>,
    pub(crate) raw_modifiers: &'a [u8],
    pub(crate) conversion: Conversion,
    pub(crate) spec: SpecAnalysis,
}

#[cfg(test)]
impl Directive<'_> {
    /// Reconstruct CLIPS's small intermediate printf fragment, including its
    /// inserted `ll` for integer/radix conversions. This is source spelling,
    /// not a rendered-result fallback, and it must never be passed to C printf.
    pub(crate) fn clips_printf_fragment(&self) -> Vec<u8> {
        let mut fragment = Vec::with_capacity(self.raw_modifiers.len() + 4);
        fragment.push(b'%');
        fragment.extend_from_slice(self.raw_modifiers);
        if matches!(
            self.conversion,
            Conversion::Decimal | Conversion::Octal | Conversion::Hex | Conversion::Unsigned
        ) {
            fragment.extend_from_slice(b"ll");
        }
        fragment.push(self.conversion.as_byte());
        fragment
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IncompleteReason {
    End,
    Percent,
    ModifierBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiteralKind {
    Plain,
    Incomplete(IncompleteReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FormatPiece<'a> {
    Literal {
        range: Range<usize>,
        bytes: &'a [u8],
        kind: LiteralKind,
    },
    /// Immediate `%n`, `%r`, `%t`, `%v` and `%%` consume no operand.
    Control {
        range: Range<usize>,
        byte: u8,
    },
    Directive(Directive<'a>),
}

/// A fully validated immutable control prefix, with no stored piece list.
/// Its size does not grow with the number of literals or conversions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ValidatedFormat<'a> {
    source: &'a [u8],
    pub(crate) conversion_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OperandCountMismatch {
    pub(crate) expected: usize,
    pub(crate) actual: usize,
}

impl<'a> ValidatedFormat<'a> {
    /// Call after whole-format validation, before evaluating any data operand.
    /// Invalid flags anywhere in the prefix therefore win over a count error.
    pub(crate) fn check_operand_count(&self, actual: usize) -> Result<(), OperandCountMismatch> {
        check_operand_count(self.conversion_count, actual)
    }

    /// Length of the source prefix before the first NUL, in bytes.
    pub(crate) fn effective_len(&self) -> usize {
        self.source.len()
    }

    /// Start a fresh lazy second pass over the same immutable validated bytes.
    pub(crate) fn pieces(&self) -> FormatPieces<'a> {
        FormatPieces(PieceScanner::new(self.source))
    }
}

fn check_operand_count(expected: usize, actual: usize) -> Result<(), OperandCountMismatch> {
    if actual == expected {
        Ok(())
    } else {
        Err(OperandCountMismatch { expected, actual })
    }
}

/// Infallible iteration is available only through a validated control prefix.
/// Each yielded piece borrows its literal/modifier bytes from the caller.
#[derive(Clone, Debug)]
pub(crate) struct FormatPieces<'a>(PieceScanner<'a>);

impl<'a> Iterator for FormatPieces<'a> {
    type Item = FormatPiece<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|piece| {
            // The same scanner already accepted these immutable bytes. No
            // unchecked constructor or mutable source reference is exposed.
            piece.expect("validated format changed between scanner passes")
        })
    }
}

impl std::iter::FusedIterator for FormatPieces<'_> {}

// Existing small parser/rendering tests inspect a collected plan. Production
// has only ValidatedFormat and its constant-space iterator.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormatPlan<'a> {
    pub(crate) pieces: Vec<FormatPiece<'a>>,
    pub(crate) conversion_count: usize,
    pub(crate) effective_len: usize,
}

#[cfg(test)]
impl FormatPlan<'_> {
    pub(crate) fn check_operand_count(&self, actual: usize) -> Result<(), OperandCountMismatch> {
        check_operand_count(self.conversion_count, actual)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InvalidFlag<'a> {
    pub(crate) percent_offset: usize,
    pub(crate) flag_offset: usize,
    pub(crate) flag: u8,
    /// Original bytes from percent through the offending byte, inclusive.
    pub(crate) fragment: &'a [u8],
}

/// Validate the entire first-NUL-delimited control string before the caller
/// checks the operand count or evaluates data. Only the borrowed prefix and
/// its conversion count survive this pass; no piece collection is allocated.
pub(crate) fn validate_format(source: &[u8]) -> Result<ValidatedFormat<'_>, InvalidFlag<'_>> {
    let effective_len = source
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(source.len());
    let source = &source[..effective_len];
    let mut conversion_count = 0;
    for piece in PieceScanner::new(source) {
        if matches!(piece?, FormatPiece::Directive(_)) {
            // A conversion consumes at least two source bytes, so this cannot
            // overflow usize for any representable borrowed slice.
            conversion_count += 1;
        }
    }
    Ok(ValidatedFormat {
        source,
        conversion_count,
    })
}

/// Test-only collected view, built from the exact production scanner.
#[cfg(test)]
pub(crate) fn scan_format(source: &[u8]) -> Result<FormatPlan<'_>, InvalidFlag<'_>> {
    let validated = validate_format(source)?;
    Ok(FormatPlan {
        pieces: validated.pieces().collect(),
        conversion_count: validated.conversion_count,
        effective_len: validated.effective_len(),
    })
}

/// Shared single-piece scanner. Its source has already been cut at the first
/// NUL by `validate_format`. A lexical error ends iteration permanently.
#[derive(Clone, Debug)]
struct PieceScanner<'a> {
    source: &'a [u8],
    position: usize,
}

impl<'a> PieceScanner<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            position: 0,
        }
    }
}

impl<'a> Iterator for PieceScanner<'a> {
    type Item = Result<FormatPiece<'a>, InvalidFlag<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        let source = self.source;
        let start = self.position;
        if start == source.len() {
            return None;
        }
        let mut position = start;
        if source[position] != b'%' {
            position += 1;
            while position < source.len() && source[position] != b'%' {
                position += 1;
            }
            self.position = position;
            return Some(Ok(FormatPiece::Literal {
                range: start..position,
                bytes: &source[start..position],
                kind: LiteralKind::Plain,
            }));
        }

        position += 1;
        let control = match source.get(position) {
            Some(b'n') => Some(b'\n'),
            Some(b'r') => Some(b'\r'),
            Some(b't') => Some(b'\t'),
            Some(b'v') => Some(0x0b),
            Some(b'%') => Some(b'%'),
            _ => None,
        };
        if let Some(byte) = control {
            position += 1;
            self.position = position;
            return Some(Ok(FormatPiece::Control {
                range: start..position,
                byte,
            }));
        }

        let mut conversion = None;
        while position < source.len()
            && source[position] != b'%'
            && position - start < FRAGMENT_CAPACITY
        {
            let byte = source[position];
            position += 1;
            if let Some(kind) = Conversion::from_byte(byte) {
                conversion = Some(kind);
                break;
            }
            if !byte.is_ascii_digit() && byte != b'.' && byte != b'-' {
                self.position = source.len();
                return Some(Err(InvalidFlag {
                    percent_offset: start,
                    flag_offset: position - 1,
                    flag: byte,
                    fragment: &source[start..position],
                }));
            }
        }
        self.position = position;
        if let Some(conversion) = conversion {
            let raw_modifiers = &source[start + 1..position - 1];
            Some(Ok(FormatPiece::Directive(Directive {
                range: start..position,
                raw_modifiers,
                conversion,
                spec: analyze_spec(raw_modifiers, start + 1),
            })))
        } else {
            let reason = if position == source.len() {
                IncompleteReason::End
            } else if source[position] == b'%' {
                IncompleteReason::Percent
            } else {
                IncompleteReason::ModifierBound
            };
            Some(Ok(FormatPiece::Literal {
                range: start..position,
                bytes: &source[start..position],
                kind: LiteralKind::Incomplete(reason),
            }))
        }
    }
}

impl std::iter::FusedIterator for PieceScanner<'_> {}

fn analyze_spec(modifiers: &[u8], absolute_start: usize) -> SpecAnalysis {
    let mut position = 0;
    let mut left_align = false;
    let mut zero_pad = false;
    while let Some(&byte) = modifiers.get(position) {
        match byte {
            b'-' => left_align = true,
            b'0' => zero_pad = true,
            _ => break,
        }
        position += 1;
    }

    let width_start = position;
    while modifiers.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    let width_range = width_start..position;
    let precision_range = if modifiers.get(position) == Some(&b'.') {
        position += 1;
        let start = position;
        while modifiers.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        Some(start..position)
    } else {
        None
    };

    if let Some(&byte) = modifiers.get(position) {
        return SpecAnalysis::NonCanonical {
            offset: absolute_start + position,
            byte,
        };
    }

    let Some(width) = checked_decimal(&modifiers[width_range.clone()]) else {
        return SpecAnalysis::ParameterOverflow {
            parameter: NumericParameter::Width,
            range: absolute_start + width_range.start..absolute_start + width_range.end,
        };
    };
    let precision = if let Some(range) = precision_range {
        let Some(precision) = checked_decimal(&modifiers[range.clone()]) else {
            return SpecAnalysis::ParameterOverflow {
                parameter: NumericParameter::Precision,
                range: absolute_start + range.start..absolute_start + range.end,
            };
        };
        Some(precision)
    } else {
        None
    };

    SpecAnalysis::Canonical(FormatSpec {
        left_align,
        zero_pad,
        width,
        precision,
    })
}

fn checked_decimal(digits: &[u8]) -> Option<usize> {
    digits.iter().try_fold(0usize, |value, &digit| {
        value
            .checked_mul(10)?
            .checked_add(usize::from(digit - b'0'))
    })
}

#[cfg(test)]
#[path = "format_parser_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "streaming_tests.rs"]
mod streaming_tests;

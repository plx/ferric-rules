//! Pure format scanning and rendering. No evaluation, routers, STRING creation,
//! diagnostic flags, user-facing resource ceiling, or snapshot state.

mod argument;
mod float_render;
mod format_parser;
mod format_types;
mod integer_bytes;
mod noncanonical;
pub(crate) use noncanonical::{append_noncanonical, NonCanonicalError};

pub(crate) use argument::FormatArgument;
#[cfg(test)]
pub(crate) use format_parser::{scan_format, FormatPlan, InvalidFlag, OperandCountMismatch};
pub(crate) use format_parser::{
    validate_format, Conversion, Directive, FormatPiece, NumericParameter, SpecAnalysis,
};
pub(crate) use format_types::OutputLimit;
pub(crate) use integer_bytes::{ByteArgumentError, IntegerArgumentError};

use crate::byte_buffer::ByteBuffer;
use float_render::{render_float, FloatConversion};
use integer_bytes::{
    character_argument, integer_argument, lexeme_argument, render_character, render_integer,
    render_lexeme, IntegerConversion,
};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DirectiveError {
    IntegerArgument(IntegerArgumentError),
    ByteArgument(ByteArgumentError),
    ExpectedNumeric(&'static str),
    /// Accepted and operand-admitted, but not a canonical render instruction.
    NonCanonical {
        range: Range<usize>,
        offset: usize,
        byte: u8,
    },
    /// Accepted and operand-admitted, but the numeric parameter cannot fit the
    /// host usize. No CLIPS diagnostic, severity or replacement is implied.
    ParameterOverflow {
        parameter: NumericParameter,
        range: Range<usize>,
    },
    OutputLimit(OutputLimit),
}

/// Append literal/control bytes with the same aggregate allowance as fields.
/// On rejection the caller's existing output is unchanged.
pub(crate) fn append_bytes(
    output: &mut ByteBuffer,
    bytes: &[u8],
    allowed: usize,
) -> Result<(), OutputLimit> {
    let required = output.as_bytes().len().checked_add(bytes.len());
    match required {
        Some(required) if required <= allowed => {
            output.push_bytes(bytes);
            Ok(())
        }
        _ => Err(OutputLimit { required, allowed }),
    }
}

enum PreparedArgument<'a> {
    Integer(IntegerConversion, i64),
    Float(FloatConversion, f64),
    Lexeme(&'a [u8]),
    Character(u8),
}

/// Call after full scan/count validation and after evaluating exactly this
/// operand. Core symbol/name ownership and EvalError/Halt gates belong to the
/// evaluator. Admission runs before reporting a noncanonical/overflow spec,
/// retaining the accepted directive's operand semantics.
///
/// Only this append is atomic: earlier bytes remain on error. A format wrapper
/// should use a local buffer and publish it only after overall success; it must
/// not roll back already executed operand effects.
pub(crate) fn append_directive(
    output: &mut ByteBuffer,
    directive: &Directive<'_>,
    value: FormatArgument<'_>,
    allowed: usize,
) -> Result<(), DirectiveError> {
    let prepared = match directive.conversion {
        Conversion::Decimal | Conversion::Octal | Conversion::Hex | Conversion::Unsigned => {
            let converted = integer_argument(value).map_err(DirectiveError::IntegerArgument)?;
            let conversion = match directive.conversion {
                Conversion::Decimal => IntegerConversion::Decimal,
                Conversion::Octal => IntegerConversion::Octal,
                Conversion::Hex => IntegerConversion::Hex,
                Conversion::Unsigned => IntegerConversion::Unsigned,
                _ => unreachable!(),
            };
            PreparedArgument::Integer(conversion, converted)
        }
        Conversion::Fixed | Conversion::Scientific | Conversion::General => {
            let converted = match value {
                FormatArgument::Float(value) => value,
                #[allow(clippy::cast_precision_loss)] // PrintFormatFlag INTEGER -> double.
                FormatArgument::Integer(value) => value as f64,
                other => return Err(DirectiveError::ExpectedNumeric(other.type_name())),
            };
            let conversion = match directive.conversion {
                Conversion::Fixed => FloatConversion::Fixed,
                Conversion::Scientific => FloatConversion::Scientific,
                Conversion::General => FloatConversion::General,
                _ => unreachable!(),
            };
            PreparedArgument::Float(conversion, converted)
        }
        Conversion::Lexeme => {
            PreparedArgument::Lexeme(lexeme_argument(value).map_err(DirectiveError::ByteArgument)?)
        }
        Conversion::Character => PreparedArgument::Character(
            character_argument(value).map_err(DirectiveError::ByteArgument)?,
        ),
    };
    let spec = match &directive.spec {
        SpecAnalysis::Canonical(spec) => *spec,
        SpecAnalysis::NonCanonical { offset, byte } => {
            return Err(DirectiveError::NonCanonical {
                range: directive.range.clone(),
                offset: *offset,
                byte: *byte,
            });
        }
        SpecAnalysis::ParameterOverflow { parameter, range } => {
            return Err(DirectiveError::ParameterOverflow {
                parameter: *parameter,
                range: range.clone(),
            });
        }
    };
    match prepared {
        PreparedArgument::Integer(conversion, value) => {
            render_integer(output, conversion, value, spec, allowed)
        }
        PreparedArgument::Float(conversion, value) => {
            render_float(output, value, conversion, spec, allowed)
        }
        PreparedArgument::Lexeme(bytes) => render_lexeme(output, bytes, spec, allowed),
        PreparedArgument::Character(byte) => render_character(output, byte, spec, allowed),
    }
    .map_err(DirectiveError::OutputLimit)
}

#[cfg(test)]
mod tests;

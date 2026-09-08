//! Evaluation and diagnostics for byte-preserving CLIPS-style `format`.
//!
//! The pure formatter owns scanning and rendering. This wrapper owns argument
//! effects, the independent Error/Halt flags, and Ferric's bounded allocation
//! policy. The router expression is evaluated; the result is returned as a
//! STRING without writing to a router.

use super::{
    check_arity_min, eval_inner, generic_value_type_name, EvalContext, EvalError, RuntimeExpr,
    SourceSpan,
};
use crate::byte_buffer::ByteBuffer;
use crate::formatting::{
    append_bytes, append_directive, append_noncanonical, validate_format, ByteArgumentError,
    Conversion, Directive, DirectiveError, FormatArgument, FormatPiece, IntegerArgumentError,
    NonCanonicalError, OutputLimit,
};
use ferric_rules_core::{string::FerricString, symbol::SymbolTable, value::Value};

/// Private per-call defense against user-controlled width/precision expansion.
/// The first-NUL control prefix is also bounded before creating its scan plan.
const FORMAT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

pub(super) fn builtin_format(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    builtin_format_with_limit(ctx, args, span, FORMAT_OUTPUT_LIMIT)
}

/// A separate allowance parameter permits small resource-boundary tests without
/// constructing multi-megabyte results. This is not a public configuration API.
#[allow(clippy::too_many_lines)]
pub(super) fn builtin_format_with_limit(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    allowed: usize,
) -> Result<Value, EvalError> {
    if let Err(error) = check_arity_min("format", args, 2, span) {
        return Ok(fail(ctx, error, true));
    }

    // Preserve the existing return-only router contract and source order. The
    // control expression runs before its inherited-Error gate, even if the
    // router or an earlier enclosing expression set Error/Halt.
    let _router = eval_operand(ctx, &args[0])?;
    let control = eval_operand(ctx, &args[1])?;
    if ctx.globals.evaluation_error() {
        return Ok(empty_string(ctx));
    }
    let Value::String(control) = control else {
        return Ok(fail(
            ctx,
            type_error("STRING", generic_value_type_name(&control), span),
            true,
        ));
    };
    let bytes = control.as_bytes();
    // Stop looking once the bounded prefix is exceeded. Bytes following the
    // first NUL do not contribute to either the source cap or the format plan.
    let prefix_len = bytes
        .iter()
        .take(allowed.saturating_add(1))
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    if prefix_len > allowed {
        return Ok(fail(
            ctx,
            unsupported("control prefix exceeds the per-call format limit", span),
            true,
        ));
    }
    let plan = match validate_format(&bytes[..prefix_len]) {
        Ok(plan) => plan,
        Err(error) => {
            return Ok(fail(
                ctx,
                unsupported(
                    format!(
                        "invalid format flag 0x{:02X} at byte {}",
                        error.flag, error.flag_offset
                    ),
                    span,
                ),
                true,
            ));
        }
    };
    debug_assert_eq!(plan.effective_len(), prefix_len);
    if let Err(error) = plan.check_operand_count(args.len() - 2) {
        return Ok(fail(
            ctx,
            EvalError::ArityMismatch {
                name: "format".into(),
                expected: format!("exactly {} arguments", error.expected + 2),
                actual: args.len(),
                span: span.cloned(),
            },
            true,
        ));
    }

    let mut output = ByteBuffer::new();
    let mut operand = 2;
    for piece in plan.pieces() {
        let result = match piece {
            FormatPiece::Literal { bytes, .. } => append_bytes(&mut output, bytes, allowed)
                .map_err(|error| (limit_error(error, span), true)),
            FormatPiece::Control { byte, .. } => append_bytes(&mut output, &[byte], allowed)
                .map_err(|error| (limit_error(error, span), true)),
            FormatPiece::Directive(directive) => {
                let value = eval_operand(ctx, &args[operand])?;
                operand += 1;
                // Numeric and %s use EnvArgTypeCheck's evaluate-then-Error
                // guard. %c uses EnvRtnUnknown and inspects its actual returned
                // Value, even under Error/Halt; a bad %c type is only a warning.
                if directive.conversion != Conversion::Character && ctx.globals.evaluation_error() {
                    return Ok(empty_string(ctx));
                }
                append_value(
                    &mut output,
                    &directive,
                    &value,
                    ctx.symbol_table,
                    allowed,
                    span,
                )
            }
        };
        if let Err((error, fatal)) = result {
            return Ok(fail(ctx, error, fatal));
        }
    }
    match FerricString::from_bytes(output.as_bytes(), ctx.config.string_encoding) {
        Ok(value) => Ok(Value::String(value)),
        Err(_) => Ok(fail(
            ctx,
            type_error("valid string encoding", "invalid result encoding", span),
            true,
        )),
    }
}

/// Use the existing scoped default-return machinery for each operand. Always
/// leave the scope before propagating a Result, including `ReturnControl`. This
/// retains direct arithmetic partial returns without converting their errors
/// into duplicate diagnostics, and preserves a surrounding recovery scope.
fn eval_operand(ctx: &mut EvalContext<'_>, expression: &RuntimeExpr) -> Result<Value, EvalError> {
    ctx.globals.begin_sort_recovery();
    let result = eval_inner(ctx, expression);
    ctx.globals.end_sort_recovery();
    result
}

fn append_value(
    output: &mut ByteBuffer,
    directive: &Directive<'_>,
    value: &Value,
    symbols: &SymbolTable,
    allowed: usize,
    span: Option<&SourceSpan>,
) -> Result<(), (EvalError, bool)> {
    let argument =
        argument(value, symbols, directive.conversion, span).map_err(|error| (error, true))?;
    match append_directive(output, directive, argument, allowed) {
        Ok(()) => Ok(()),
        // Admission has already succeeded. This is the selected deterministic
        // Ferric fallback, not a retry after a failed type/domain check and not
        // a claim about arbitrary platform printf behavior.
        Err(DirectiveError::NonCanonical { .. }) => append_noncanonical(output, directive, allowed)
            .map_err(|error| {
                let error = match error {
                    NonCanonicalError::OutputLimit(error) => limit_error(error, span),
                    NonCanonicalError::NotNonCanonical | NonCanonicalError::InvalidDescriptor => {
                        unsupported("inconsistent noncanonical format descriptor", span)
                    }
                };
                (error, true)
            }),
        Err(error) => Err(render_error(&error, span)),
    }
}

fn argument<'a>(
    value: &'a Value,
    symbols: &'a SymbolTable,
    conversion: Conversion,
    span: Option<&SourceSpan>,
) -> Result<FormatArgument<'a>, EvalError> {
    Ok(match value {
        Value::Integer(value) => FormatArgument::Integer(*value),
        Value::Float(value)
            if matches!(
                conversion,
                Conversion::Decimal | Conversion::Octal | Conversion::Hex | Conversion::Unsigned
            ) =>
        {
            // Preserve Ferric's existing Rust cast policy: truncate finite
            // fractions, saturate overflow/infinities, and map NaN to zero.
            // CLIPS's out-of-range C cast is not a portable parity contract.
            #[allow(clippy::cast_possible_truncation)]
            let value = *value as i64;
            FormatArgument::Integer(value)
        }
        Value::Float(value) => FormatArgument::Float(*value),
        Value::String(value) => FormatArgument::String(value.as_bytes()),
        Value::Symbol(value) => FormatArgument::Symbol(
            symbols
                .resolve_symbol_bytes(*value)
                .ok_or_else(|| unsupported("unresolved format SYMBOL", span))?,
        ),
        Value::InstanceName(value) => FormatArgument::InstanceName(
            symbols
                .resolve_symbol_bytes(value.as_symbol())
                .ok_or_else(|| unsupported("unresolved format INSTANCE-NAME", span))?,
        ),
        other => FormatArgument::Other(generic_value_type_name(other)),
    })
}

fn render_error(error: &DirectiveError, span: Option<&SourceSpan>) -> (EvalError, bool) {
    match error {
        DirectiveError::IntegerArgument(IntegerArgumentError::NotNumeric(actual))
        | DirectiveError::ExpectedNumeric(actual) => {
            (type_error("INTEGER or FLOAT", actual, span), true)
        }
        DirectiveError::ByteArgument(ByteArgumentError::ExpectedLexeme(actual)) => (
            type_error("STRING, SYMBOL, or INSTANCE-NAME", actual, span),
            true,
        ),
        DirectiveError::ByteArgument(ByteArgumentError::ExpectedCharacter(actual)) => (
            type_error("INTEGER, STRING, or SYMBOL", actual, span),
            false,
        ),
        DirectiveError::OutputLimit(error) => (limit_error(*error, span), true),
        DirectiveError::ParameterOverflow { .. } => (
            unsupported("format width or precision exceeds the supported size", span),
            true,
        ),
        // The adapter above implements Ferric's saturating integer policy;
        // these remain explicit guards against a future adapter mismatch.
        DirectiveError::IntegerArgument(
            IntegerArgumentError::NonFinite | IntegerArgumentError::OutsideSignedRange,
        ) => (
            unsupported("integer format operand was not normalized", span),
            true,
        ),
        DirectiveError::NonCanonical { .. } => (
            unsupported("noncanonical format fragment was not handled", span),
            true,
        ),
    }
}

fn type_error(expected: &str, actual: &str, span: Option<&SourceSpan>) -> EvalError {
    EvalError::TypeError {
        function: "format".into(),
        expected: expected.into(),
        actual: actual.into(),
        span: span.cloned(),
    }
}

fn unsupported(reason: impl Into<String>, span: Option<&SourceSpan>) -> EvalError {
    EvalError::UnsupportedOperation {
        operation: "format".into(),
        reason: reason.into(),
        span: span.cloned(),
    }
}

fn limit_error(error: OutputLimit, span: Option<&SourceSpan>) -> EvalError {
    unsupported(
        format!(
            "formatted output exceeds the per-call limit of {} bytes",
            error.allowed
        ),
        span,
    )
}

fn empty_string(ctx: &EvalContext<'_>) -> Value {
    Value::String(
        FerricString::new("", ctx.config.string_encoding)
            .expect("empty STRING fits every encoding"),
    )
}

fn fail(ctx: &mut EvalContext<'_>, error: EvalError, fatal: bool) -> Value {
    if fatal {
        ctx.globals.push_halt_diagnostic(error);
    } else {
        ctx.globals.push_diagnostic(error);
    }
    empty_string(ctx)
}

//! CLIPS field reads over the existing host-framed stdin queue.
//!
//! This adapter owns no cursor, registry, external handle, or snapshot state.
//! Each selected frame is discarded in full. `read` skips scanner STOP frames;
//! `readline` returns exactly one full frame, including an empty frame.

use super::{
    eval_inner, generic_value_type_name, queue_field_scan_notice, scanned_field_encoding_error,
    scanned_field_value, EvalContext, EvalError, RuntimeExpr, SourceSpan,
};
use crate::field_scanner::{FieldScanner, FieldToken};
use ferric_rules_core::{string::FerricString, symbol::SymbolTable, value::Value};

pub(super) fn builtin_read(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if !prepare_input(ctx, args, "read", span)? {
        return Ok(read_error_value(ctx));
    }
    loop {
        let Some(line) = ctx
            .input_buffer
            .as_deref_mut()
            .and_then(std::collections::VecDeque::pop_front)
        else {
            return Ok(eof_value(ctx, "read", span));
        };
        // A frame already belongs to the host's line protocol. Do not split
        // embedded CR/LF, retain a suffix cursor, or scan an ignored tail.
        let scanned = FieldScanner::clips_string(line.as_bytes()).next_token();
        for notice in scanned.notices {
            queue_field_scan_notice(ctx, &notice, "read", span);
        }
        match scanned.token {
            FieldToken::Stop(_) => {}
            // UNKNOWN is a silent, nonfatal STRING result for read. The shared
            // conversion intentionally has a different UNKNOWN print form.
            FieldToken::Unknown { .. } => return Ok(read_error_value(ctx)),
            token => {
                return Ok(match scanned_field_value(ctx, token, "read", span) {
                    Ok(value) => value,
                    Err(error) => failure(ctx, error),
                });
            }
        }
    }
}

pub(super) fn builtin_readline(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if !prepare_input(ctx, args, "readline", span)? {
        return Ok(read_error_value(ctx));
    }
    let Some(line) = ctx
        .input_buffer
        .as_deref_mut()
        .and_then(std::collections::VecDeque::pop_front)
    else {
        return Ok(eof_value(ctx, "readline", span));
    };
    Ok(match FerricString::new(&line, ctx.config.string_encoding) {
        Ok(value) => Value::String(value),
        Err(error) => failure(ctx, scanned_field_encoding_error(error, "readline", span)),
    })
}

/// Return false once a local failure or the framed-input halt policy has chosen
/// the read-error default. `ReturnControl` still propagates as non-local control.
fn prepare_input(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<bool, EvalError> {
    if args.len() > 1 {
        ctx.globals.push_halt_diagnostic(EvalError::ArityMismatch {
            name: function.into(),
            expected: "0 or 1".into(),
            actual: args.len(),
            span: span.cloned(),
        });
        return Ok(false);
    }
    if let Some(expression) = args.first() {
        // GetLogicalName examines the actual returned Value without an
        // inherited-Error gate. Balanced recovery retains builtin partial
        // defaults and diagnoses an unavailable returned router afterward.
        ctx.globals.begin_sort_recovery();
        let result = eval_inner(ctx, expression);
        ctx.globals.end_sort_recovery();
        let value = result?;
        if let Err(error) = stdin_router(&value, ctx.symbol_table, function, span) {
            ctx.globals.push_halt_diagnostic(error);
            return Ok(false);
        }
    }
    // Explicit Ferric framed-input policy: CLIPS's getc-before-Halt can consume
    // one byte, which this queue cannot represent. Preserve whole frames and
    // return READ ERROR without an extra diagnostic. Check Halt, not Error;
    // neither flag nor any earlier diagnostic is cleared here.
    Ok(!ctx.globals.evaluation_halted())
}

fn stdin_router(
    value: &Value,
    symbols: &SymbolTable,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    let bytes = match value {
        Value::String(value) => value.as_bytes(),
        Value::Symbol(value) => symbols
            .resolve_symbol_bytes(*value)
            .ok_or_else(|| logical_name_type_error(function, "unresolved SYMBOL", span))?,
        Value::InstanceName(value) => symbols
            .resolve_symbol_bytes(value.as_symbol())
            .ok_or_else(|| logical_name_type_error(function, "unresolved INSTANCE-NAME", span))?,
        // CLIPS admits numeric logical names. None can name this adapter's
        // stdin aliases, so classify them as unavailable, not an illegal type.
        Value::Integer(_) | Value::Float(_) => {
            return Err(unavailable_router(
                function,
                generic_value_type_name(value),
                span,
            ));
        }
        other => {
            return Err(logical_name_type_error(
                function,
                generic_value_type_name(other),
                span,
            ))
        }
    };
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    if matches!(&bytes[..end], b"t" | b"T" | b"stdin") {
        Ok(())
    } else {
        // No output-router lookup: an output buffer is not an input source.
        Err(unavailable_router(
            function,
            generic_value_type_name(value),
            span,
        ))
    }
}

fn logical_name_type_error(function: &str, actual: &str, span: Option<&SourceSpan>) -> EvalError {
    EvalError::TypeError {
        function: function.into(),
        expected: "STRING, SYMBOL, INSTANCE-NAME, INTEGER, or FLOAT logical name".into(),
        actual: actual.into(),
        span: span.cloned(),
    }
}

fn unavailable_router(function: &str, actual: &str, span: Option<&SourceSpan>) -> EvalError {
    EvalError::UnsupportedOperation {
        operation: function.into(),
        reason: format!("input logical name ({actual}) is not registered; only stdin is available"),
        span: span.cloned(),
    }
}

fn eof_value(ctx: &mut EvalContext<'_>, function: &str, span: Option<&SourceSpan>) -> Value {
    match ctx
        .symbol_table
        .intern_symbol("EOF", ctx.config.string_encoding)
    {
        Ok(value) => Value::Symbol(value),
        Err(error) => failure(ctx, scanned_field_encoding_error(error, function, span)),
    }
}

fn read_error_value(ctx: &EvalContext<'_>) -> Value {
    Value::String(
        FerricString::new("*** READ ERROR ***", ctx.config.string_encoding)
            .expect("ASCII read-error default is encodable"),
    )
}

fn failure(ctx: &mut EvalContext<'_>, error: EvalError) -> Value {
    ctx.globals.push_halt_diagnostic(error);
    read_error_value(ctx)
}

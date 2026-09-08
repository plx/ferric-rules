//! CLIPS value spelling for direct output. Expression evaluation and router
//! delivery remain with the caller; fields are rendered without changing values.

use std::fmt::Write as _;

use ferric_rules_core::{SymbolTable, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    TopLevel,
    Field,
}

/// Append one printout operand. Control SYMBOLs expand only at the top level;
/// STRING fields in multifields use raw quotes, without escaping their contents.
pub(crate) fn append_printout_value(value: &Value, symbols: &SymbolTable, output: &mut String) {
    append_value(value, symbols, output, Context::TopLevel);
}

fn append_value(value: &Value, symbols: &SymbolTable, output: &mut String, context: Context) {
    match value {
        Value::Integer(number) => {
            let _ = write!(output, "{number}");
        }
        Value::Float(number) => append_float(*number, output),
        Value::Symbol(symbol) => {
            if let Some(name) = symbols.resolve_symbol_str(*symbol) {
                if context == Context::TopLevel {
                    match name {
                        "crlf" => output.push('\n'),
                        "tab" => output.push('\t'),
                        "vtab" => output.push('\x0b'),
                        "ff" => output.push('\x0c'),
                        other => output.push_str(other),
                    }
                } else {
                    output.push_str(name);
                }
            }
        }
        Value::String(string) => {
            if context == Context::Field {
                output.push('"');
            }
            output.push_str(string.as_str());
            if context == Context::Field {
                output.push('"');
            }
        }
        Value::Multifield(fields) => {
            output.push('(');
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    output.push(' ');
                }
                append_value(field, symbols, output, Context::Field);
            }
            output.push(')');
        }
        Value::Void => {}
        // Preserve the existing opaque-host-value boundary; this is not a
        // fabricated CLIPS pointer or a typed fact-address representation.
        Value::ExternalAddress(_) => output.push_str("<ExternalAddress>"),
    }
}

/// CLIPS 6.30 `FloatToString` uses `%.15g` and appends `.0` when the result has
/// neither a decimal point nor an exponent. Round once in scientific notation
/// so the notation decision uses the rounded exponent, including cutovers.
fn append_float(value: f64, output: &mut String) {
    if value.is_nan() {
        output.push_str("nan.0");
        return;
    }
    if value.is_infinite() {
        output.push_str(if value.is_sign_negative() {
            "-inf.0"
        } else {
            "inf.0"
        });
        return;
    }

    // LowerExp's precision counts digits after the leading digit. Rust uses
    // round-to-nearest, ties-to-even, as in the reference's default rounding mode.
    let scientific = format!("{value:.14e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("finite scientific float");
    let exponent: i32 = exponent.parse().expect("scientific exponent is an integer");
    if value.is_sign_negative() {
        output.push('-');
    }
    let digits = mantissa.trim_start_matches('-').replace('.', "");
    let digits = digits.trim_end_matches('0');
    if digits.is_empty() {
        output.push_str("0.0");
        return;
    }

    if !(-4..15).contains(&exponent) {
        output.push_str(&digits[..1]);
        if digits.len() > 1 {
            output.push('.');
            output.push_str(&digits[1..]);
        }
        let _ = write!(output, "e{exponent:+03}");
    } else if exponent < 0 {
        output.push_str("0.");
        for _ in 0..(-exponent - 1) {
            output.push('0');
        }
        output.push_str(digits);
    } else {
        let integer_digits = usize::try_from(exponent + 1).expect("nonnegative exponent");
        if digits.len() <= integer_digits {
            output.push_str(digits);
            for _ in digits.len()..integer_digits {
                output.push('0');
            }
            output.push_str(".0");
        } else {
            output.push_str(&digits[..integer_digits]);
            output.push('.');
            output.push_str(&digits[integer_digits..]);
        }
    }
}

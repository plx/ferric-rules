//! CLIPS value spelling for direct output and `implode$`. Expression evaluation
//! and router delivery remain with the caller; rendering does not change values.

use std::fmt::Write as _;

use ferric_rules_core::{SymbolTable, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    TopLevel,
    Field,
    ImplodeField,
}

/// Append one printout operand. Control SYMBOLs expand only at the top level;
/// STRING fields in multifields use raw quotes, without escaping their contents.
pub(crate) fn append_printout_value(value: &Value, symbols: &SymbolTable, output: &mut String) {
    append_value(value, symbols, output, Context::TopLevel);
}

/// Append an `implode$` field, quoting STRINGs and escaping their quotes and
/// backslashes. Control SYMBOLs retain their literal spelling.
pub(crate) fn append_implode_field(value: &Value, symbols: &SymbolTable, output: &mut String) {
    append_value(value, symbols, output, Context::ImplodeField);
}

fn append_value(value: &Value, symbols: &SymbolTable, output: &mut String, context: Context) {
    match value {
        Value::Integer(number) => {
            let _ = write!(output, "{number}");
        }
        Value::Float(number) => crate::formatting::append_clips_float(*number, output),
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
        Value::InstanceName(name) => {
            if let Some(name) = symbols.resolve_symbol_str(name.as_symbol()) {
                output.push('[');
                output.push_str(name);
                output.push(']');
            }
        }
        Value::String(string) => {
            if context != Context::TopLevel {
                output.push('"');
            }
            if context == Context::ImplodeField {
                for character in string.as_str().chars() {
                    if matches!(character, '"' | '\\') {
                        output.push('\\');
                    }
                    output.push(character);
                }
            } else {
                output.push_str(string.as_str());
            }
            if context != Context::TopLevel {
                output.push('"');
            }
        }
        Value::Multifield(fields) => {
            let field_context = if context == Context::ImplodeField {
                // Preserve the existing space-joined shape of host-created
                // nested multifields; source-visible multifields are flat.
                Context::ImplodeField
            } else {
                output.push('(');
                Context::Field
            };
            for (index, field) in fields.iter().enumerate() {
                if index != 0 {
                    output.push(' ');
                }
                append_value(field, symbols, output, field_context);
            }
            if context != Context::ImplodeField {
                output.push(')');
            }
        }
        Value::Void => {}
        Value::FactAddress(address) => {
            if let Some(index) = address.public_index() {
                let _ = write!(output, "<Fact-{index}>");
            } else {
                output.push_str("<Dummy Fact>");
            }
        }
        // Preserve the existing opaque-host-value boundary; this is not a
        // fabricated CLIPS pointer or a typed fact-address representation.
        Value::ExternalAddress(_) => output.push_str("<ExternalAddress>"),
    }
}

use std::fmt::{Display, Write as _};

pub(crate) fn emit_error(json_mode: bool, command: &str, kind: &str, message: impl Display) {
    emit_message(json_mode, command, "error", kind, message);
}

pub(crate) fn emit_warning(json_mode: bool, command: &str, kind: &str, message: impl Display) {
    emit_message(json_mode, command, "warning", kind, message);
}

/// Report the engine's buffered match-time and action errors as warnings.
/// Load, reset and run each start a new buffer, so callers emit after each.
/// `ferric run` reports through the shell session, which drains the buffer.
#[cfg(feature = "serde")]
pub(crate) fn emit_action_diagnostics(
    json_mode: bool,
    command: &str,
    engine: &ferric_rules_runtime::Engine,
) {
    for diagnostic in engine.action_diagnostics() {
        emit_warning(json_mode, command, "action_warning", diagnostic);
    }
}

fn emit_message(json_mode: bool, command: &str, level: &str, kind: &str, message: impl Display) {
    let message = message.to_string();
    if json_mode {
        eprintln!(
            "{{\"command\":\"{}\",\"level\":\"{}\",\"kind\":\"{}\",\"message\":\"{}\"}}",
            json_escape(command),
            json_escape(level),
            json_escape(kind),
            json_escape(&message)
        );
        return;
    }

    if level == "warning" {
        eprintln!("ferric {command}: warning: {message}");
    } else {
        eprintln!("ferric {command}: {message}");
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(&mut out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

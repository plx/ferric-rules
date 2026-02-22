use std::fmt::{Display, Write as _};

pub(crate) fn parse_file_args<'a>(
    args: &'a [String],
    command: &str,
) -> Result<(bool, &'a str), i32> {
    match args {
        [file] => Ok((false, file.as_str())),
        [flag, file] if flag == "--json" => Ok((true, file.as_str())),
        [] => {
            eprintln!("ferric {command}: missing file argument");
            eprintln!("Usage: ferric {command} [--json] <file>");
            Err(2)
        }
        _ => {
            eprintln!("ferric {command}: invalid arguments");
            eprintln!("Usage: ferric {command} [--json] <file>");
            Err(2)
        }
    }
}

pub(crate) fn emit_error(json_mode: bool, command: &str, kind: &str, message: impl Display) {
    emit_message(json_mode, command, "error", kind, message);
}

pub(crate) fn emit_warning(json_mode: bool, command: &str, kind: &str, message: impl Display) {
    emit_message(json_mode, command, "warning", kind, message);
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

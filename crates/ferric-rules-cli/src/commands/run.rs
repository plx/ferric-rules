//! Execute construct files with implicit reset/run, or scripts in source order.

use std::io::{BufRead, IsTerminal, Read};
use std::path::Path;

use ferric_rules_runtime::{Engine, EngineConfig, InputSource, MAX_SOURCE_BYTES};

use super::common::{emit_error, emit_warning};
use super::repl::commands::{parse_commands, ReplCommand};
use super::repl::session::ReplSession;

pub fn execute(json_mode: bool, file_path: &Path) -> i32 {
    let source = match read_source(file_path) {
        Ok(source) => source,
        Err(error) => {
            emit_error(json_mode, "run", "io_error", error);
            return 1;
        }
    };
    // Parse the whole bounded file before executing anything. Syntax errors do
    // not partly execute a script; runtime errors retain completed effects.
    let commands = match parse_commands(&source) {
        Ok(commands) => commands,
        Err(error) => {
            emit_error(json_mode, "run", "load_error", error);
            return 1;
        }
    };
    let construct_only = commands
        .iter()
        .all(|command| matches!(command, ReplCommand::Construct { .. }));
    let mut engine = Engine::new(EngineConfig::default());
    if !std::io::stdin().is_terminal() {
        // Read piped input lazily, one line per `read`/`readline`, so programs
        // that never read neither wait for nor consume the caller's input.
        engine.set_input_source(Some(stdin_line_source(json_mode)));
    }
    let mut session = ReplSession::with_engine(engine, Some(json_mode));
    if construct_only {
        // Keep one load for ordinary files, including its forward declarations.
        if session.load_source(&source).is_err()
            || session.cmd_reset().is_err()
            || session.cmd_run(None).is_err()
        {
            return 1;
        }
    } else {
        for command in commands {
            match session.dispatch(command, false) {
                Ok(true) => break,
                Ok(false) => {}
                Err(()) => return 1,
            }
        }
    }
    0
}

/// One line of standard input per call. EOF, a read error, or invalid UTF-8
/// ends input (the latter two with a single warning).
fn stdin_line_source(json_mode: bool) -> InputSource {
    let mut finished = false;
    Box::new(move || {
        if finished {
            return None;
        }
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) => {
                finished = true;
                None
            }
            Ok(_) => Some(line),
            Err(error) => {
                finished = true;
                emit_warning(
                    json_mode,
                    "run",
                    "io_error",
                    format_args!("reading stdin: {error}; treating it as end of input"),
                );
                None
            }
        }
    })
}

fn read_source(path: &Path) -> Result<String, std::io::Error> {
    let file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            std::io::Error::new(error.kind(), format!("file not found: {}", path.display()))
        } else {
            error
        }
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(std::io::Error::other(format!(
            "source exceeds the {MAX_SOURCE_BYTES}-byte limit"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

//! `read`/`readline` pull from a lazy host input source after queued input.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use ferric_rules_core::Value;
use ferric_rules_runtime::{Engine, EngineConfig, InputSource};

/// A source over `lines` that records how many lines it has handed out.
fn source(lines: &[&str]) -> (InputSource, Arc<Mutex<usize>>) {
    let mut lines: VecDeque<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    let pulled = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&pulled);
    let source: InputSource = Box::new(move || {
        let line = lines.pop_front()?;
        *counter.lock().unwrap() += 1;
        Some(line)
    });
    (source, pulled)
}

fn string(value: &Value) -> &str {
    match value {
        Value::String(text) => text.as_str(),
        other => panic!("expected a string, got {other:?}"),
    }
}

fn symbol<'a>(engine: &'a Engine, value: &Value) -> &'a str {
    match value {
        Value::Symbol(symbol) => engine.resolve_core_symbol(*symbol).unwrap(),
        other => panic!("expected a symbol, got {other:?}"),
    }
}

#[test]
fn readline_pulls_one_source_line_per_call_and_strips_terminators() {
    let mut engine = Engine::new(EngineConfig::default());
    let (input, pulled) = source(&["first\r\n", "second\n", "last"]);
    engine.set_input_source(Some(input));
    assert_eq!(*pulled.lock().unwrap(), 0, "installing must not read");
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(string(&value), "first");
    assert_eq!(*pulled.lock().unwrap(), 1);
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(string(&value), "second");
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(string(&value), "last");
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(symbol(&engine, &value), "EOF");
}

#[test]
fn read_skips_blank_source_lines_and_discards_the_rest_of_its_line() {
    let mut engine = Engine::new(EngineConfig::default());
    let (input, pulled) = source(&["\n", "   \n", "42 ignored\n", "next\n"]);
    engine.set_input_source(Some(input));
    assert!(matches!(
        engine.eval_str("(read)").unwrap(),
        Value::Integer(42)
    ));
    assert_eq!(*pulled.lock().unwrap(), 3, "read stops at its field's line");
    let value = engine.eval_str("(read)").unwrap();
    assert_eq!(symbol(&engine, &value), "next");
    let value = engine.eval_str("(read)").unwrap();
    assert_eq!(symbol(&engine, &value), "EOF");
}

#[test]
fn queued_input_takes_priority_and_blank_queued_lines_fall_back_to_the_source() {
    let mut engine = Engine::new(EngineConfig::default());
    let (input, pulled) = source(&["from-source\n", "7\n"]);
    engine.set_input_source(Some(input));
    engine.push_input("queued");
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(string(&value), "queued");
    assert_eq!(*pulled.lock().unwrap(), 0);
    // A blank queued line does not satisfy `read`, which keeps pulling.
    engine.push_input("");
    let value = engine.eval_str("(read)").unwrap();
    assert_eq!(symbol(&engine, &value), "from-source");
    // Input pushed later is still read before the source's next line.
    engine.push_input("pushed");
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(string(&value), "pushed");
    assert!(matches!(
        engine.eval_str("(read)").unwrap(),
        Value::Integer(7)
    ));
}

#[test]
fn clear_keeps_the_source_and_removing_it_ends_input() {
    let mut engine = Engine::new(EngineConfig::default());
    let (input, _) = source(&["one\n", "two\n"]);
    engine.set_input_source(Some(input));
    engine.push_input("dropped");
    engine.clear();
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(string(&value), "one");
    engine.set_input_source(None);
    let value = engine.eval_str("(readline)").unwrap();
    assert_eq!(symbol(&engine, &value), "EOF");
}

#[cfg(feature = "serde")]
#[test]
fn snapshots_restore_queued_input_but_not_the_source() {
    use ferric_rules_runtime::SerializationFormat;

    let mut engine = Engine::new(EngineConfig::default());
    let (input, pulled) = source(&["live\n"]);
    engine.set_input_source(Some(input));
    engine.push_input("queued");
    let bytes = engine.serialize(SerializationFormat::RECOMMENDED).unwrap();
    let mut restored = Engine::deserialize(&bytes, SerializationFormat::RECOMMENDED).unwrap();
    let value = restored.eval_str("(readline)").unwrap();
    assert_eq!(string(&value), "queued");
    let value = restored.eval_str("(readline)").unwrap();
    assert_eq!(symbol(&restored, &value), "EOF");
    assert_eq!(*pulled.lock().unwrap(), 0);
}

#[test]
fn before_input_hook_receives_pending_output_before_the_source_blocks() {
    let mut engine = Engine::new(EngineConfig::default());
    engine.enable_output_events();
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let hook_log = Arc::clone(&log);
    engine.set_before_input(Some(Box::new(move |events, diagnostics| {
        let mut log = hook_log.lock().unwrap();
        for (channel, text) in events {
            log.push(format!("{channel}:{text}"));
        }
        log.extend(diagnostics.iter().map(ToString::to_string));
    })));
    let source_log = Arc::clone(&log);
    let mut lines = VecDeque::from(["hello\n".to_owned()]);
    engine.set_input_source(Some(Box::new(move || {
        source_log.lock().unwrap().push("read".to_owned());
        lines.pop_front()
    })));
    engine.push_input("queued");
    engine
        .load_str(
            r#"(defrule go =>
                 (printout t "first " (readline) crlf)
                 (printout t "ready" crlf)
                 (bind ?x (readline))
                 (printout t "got " ?x crlf))"#,
        )
        .unwrap();
    engine.reset().unwrap();
    engine
        .run(ferric_rules_runtime::RunLimit::Unlimited)
        .unwrap();
    // Queued input never consults the source, so only the second read
    // delivers the output that precedes it.
    assert_eq!(
        *log.lock().unwrap(),
        vec!["t:first queued\nready\n".to_owned(), "read".to_owned()]
    );
    // Output after the last read stays queued for the host's own drain.
    assert_eq!(
        engine.drain_output_events(),
        vec![("t".to_owned(), "got hello\n".to_owned())]
    );
}

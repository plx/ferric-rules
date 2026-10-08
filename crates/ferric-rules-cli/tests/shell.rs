//! End-to-end shell behavior, including process streams and real fact files.

use std::fmt::Write as _;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn invoke(args: &[&str], input: &str, directory: &std::path::Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferric"))
        .args(args)
        .current_dir(directory)
        .env_remove("HOME")
        // rustyline prompts on piped stdin when TERM is dumb, cons25 or emacs;
        // keep the asserted output independent of the caller's terminal.
        .env_remove("TERM")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn run(source: &str, input: &str, json: bool) -> Output {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("program.clp"), source).unwrap();
    let args = if json {
        vec!["run", "--json", "program.clp"]
    } else {
        vec!["run", "program.clp"]
    };
    invoke(&args, input, directory.path())
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn successful(output: &Output) {
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_queues_stdin_before_loading_and_preserves_lines_and_eof() {
    let output = run(
        r#"(defglobal ?*first* = (read))
        (defrule ask =>
          (printout t "first=" ?*first* ";n=" (read) ";line=[" (readline)
            "];last=[" (readline) "];eof=" (read) crlf))"#,
        "17 discarded\r\n42 discarded\r\n\r\nlast line",
        false,
    );
    successful(&output);
    assert_eq!(
        stdout(&output),
        "first=17;n=42;line=[];last=[last line];eof=EOF\n"
    );
}

#[test]
fn all_standard_channels_keep_source_order_and_json_stderr_stays_clean() {
    let channels = [
        "t", "stdin", "stdout", "stderr", "wclips", "wdialog", "wdisplay", "werror", "wtrace",
        "wwarning",
    ];
    let mut source = String::from("(defrule emit =>");
    let mut expected = String::new();
    for channel in channels {
        write!(
            source,
            " (printout {channel} \"{channel}|\") (printout t \"t|\")"
        )
        .unwrap();
        write!(expected, "{channel}|t|").unwrap();
    }
    source.push(')');
    for json in [false, true] {
        let output = run(&source, "", json);
        successful(&output);
        assert_eq!(stdout(&output), expected);
    }
}

#[test]
fn procedural_files_execute_once_and_assertions_survive_without_implicit_reset() {
    let output = run(
        r"(defrule r (p ?x) => (printout t ?x crlf))
        (reset) (assert (p 7)) (run)",
        "",
        false,
    );
    successful(&output);
    assert_eq!(stdout(&output), "7\n");
    let output = run(
        r#"(defrule r => (printout t "unexpected")) (reset)"#,
        "",
        false,
    );
    successful(&output);
    assert_eq!(stdout(&output), "");
    let output = run(
        r#"(defrule r => (printout t "(reset) is only text"))"#,
        "",
        false,
    );
    successful(&output);
    assert_eq!(stdout(&output), "(reset) is only text");
}

#[test]
fn output_survives_reset_load_failure_and_evaluation_failure() {
    let output = run(
        r#"(printout stdout "before|") (reset) (printout werror "after")"#,
        "",
        false,
    );
    successful(&output);
    assert_eq!(stdout(&output), "before|after");
    let output = run(r#"(progn (printout wwarning "prefix") (/ 1 0))"#, "", true);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(stdout(&output), "prefix");
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        let diagnostic: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(diagnostic["command"], "run");
    }
    let output = run(
        r#"(defglobal ?*x* = (progn (printout stdout "loaded") 1)) (deftemplate broken (unknown x))"#,
        "",
        false,
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(stdout(&output), "loaded");
}

#[test]
fn script_syntax_is_checked_before_effects_and_runtime_locations_are_preserved() {
    let output = run("(printout t prefix)\n(", "", false);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(stdout(&output), "");
    let output = run("(printout t prefix)\n\n  (missing-function)", "", false);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(stdout(&output), "prefix");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("line 3, column 3"),
        "{output:?}"
    );
}

#[test]
fn repl_evaluates_expressions_echoes_values_and_recovers_after_errors() {
    let directory = tempfile::tempdir().unwrap();
    let output = invoke(&["repl"],
        "(printout t \"hi\" crlf)\n(bind ?x 3)\n(+ 1 2)\n(progn (bind ?x 4) (+ ?x 1))\n?x\n(reset)\n(assert (p 9))\n(create$ a \"b\")\ncrlf\n(progn (printout stdout \"partial\") (/ 1 0))\n(+ 5 6)\n( exit ; done\n)\n",
        directory.path());
    assert!(output.status.success(), "{output:?}");
    let out = stdout(&output);
    assert!(out.contains("hi\n3\n3\n5\n"), "{out}");
    assert!(
        out.contains("<Fact-1>\n(a \"b\")\ncrlf\npartial11\n"),
        "{out}"
    );
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains('x') && err.contains("zero"), "{err}");
}

#[test]
fn watch_observes_transient_facts_and_agenda_inspection_preserves_firing_order() {
    let directory = tempfile::tempdir().unwrap();
    let output = invoke(
        &["repl"],
        r#"
(deftemplate person (slot name) (slot age))
(deffacts init (person (name alice) (age 30)))
(defrule birthday (declare (salience 10)) ?p <- (person (age 30)) => (printout t "before|") (modify ?p (age 31)) (printout stdout "after|"))
(defrule transient => (bind ?f (assert (tmp))) (assert (tmp)) (retract ?f))
(watch facts)
(watch rules)
(reset)
(agenda)
(run)
(facts)
(exit)
"#,
        directory.path(),
    );
    successful(&output);
    let out = stdout(&output);
    assert!(
        !out.contains("template-fact") && !out.contains("922337"),
        "{out}"
    );
    assert!(out.contains("birthday: f-1"), "{out}");
    assert!(out.contains("transient: *"), "{out}");
    assert_eq!(out.matches("==> f-3").count(), 1, "{out}");
    let markers = [
        "FIRE",
        "birthday:",
        "before|",
        "<== f-1",
        "(person (name alice) (age 30))",
        "==> f-2",
        "(person (name alice) (age 31))",
        "after|",
        "FIRE",
        "transient:",
        "==> f-3",
        "(tmp)",
        "<== f-3",
        "(tmp)",
    ];
    let mut rest = out.as_str();
    for marker in markers {
        let index = rest
            .find(marker)
            .unwrap_or_else(|| panic!("missing {marker:?} in {rest:?}"));
        rest = &rest[index + marker.len()..];
    }
    assert!(
        rest.contains("f-2") && rest.contains("(person (name alice) (age 31))"),
        "{rest}"
    );
}

#[test]
fn save_and_load_facts_round_trip_named_slots_escapes_and_multislots() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("facts with spaces.fct");
    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let source = format!(
        r#"
(deftemplate person (slot name) (multislot tags))
(reset)
(assert (person (name "a\"b\\c") (tags x "y")))
(assert (person (name empty)))
(assert (ordered 1e20 -0.0 "quote\"slash\\"))
(save-facts {quoted})
(clear)
(deftemplate person (slot name) (multislot tags))
(defrule verified (person (name "a\"b\\c") (tags x "y")) (person (name empty) (tags)) (ordered 1e20 -0.0 "quote\"slash\\") => (printout t "roundtrip verified" crlf))
(load-facts {quoted})
(facts)
(run)
(exit)
"#
    );
    let output = invoke(&["repl"], &source, directory.path());
    successful(&output);
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(
        saved.contains("(person (name \"a\\\"b\\\\c\") (tags x \"y\"))"),
        "{saved}"
    );
    assert!(saved.contains("(person (name empty) (tags))"), "{saved}");
    assert!(
        !saved.contains("(assert") && !saved.contains("template-fact"),
        "{saved}"
    );
    let out = stdout(&output);
    assert!(out.contains("TRUE\nTRUE\n"), "{out}");
    assert!(out.contains("roundtrip verified\n"), "{out}");
    assert!(out.contains("(ordered 1e+20 -0.0"), "{out}");
    assert!(
        out.contains("(person (name") && out.contains("(ordered"),
        "{out}"
    );
}

#[test]
fn repl_routes_whitespace_and_escaped_paths_without_ignoring_extra_arguments() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("a\"b\\c.clp");
    std::fs::write(&path, "(deffacts d (loaded))").unwrap();
    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let source = format!(
        "( load\n {quoted} )\n( reset ) ( facts )\n(load {quoted} extra)\n(+ 2 3)\n(quit)\n"
    );
    let output = invoke(&["repl"], &source, directory.path());
    assert!(output.status.success(), "{output:?}");
    let out = stdout(&output);
    assert!(out.contains("(loaded)") && out.contains("5\n"), "{out}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid arguments for load"),
        "{output:?}"
    );
}

#[test]
fn agenda_lists_module_groups_and_changing_strategy_reorders_pending_activations() {
    let output = run(
        r#"(defrule first => (printout t "first|"))
        (defrule second => (printout t "second|"))
        (defmodule EXTRA)
        (defrule extra =>)
        (reset)
        (printout t "old=" (set-strategy breadth) ";now=" (get-strategy) crlf)
        (agenda *)
        (run)"#,
        "",
        false,
    );
    successful(&output);
    let out = stdout(&output);
    assert!(out.starts_with("old=depth;now=breadth\nMAIN:\n"), "{out}");
    assert!(
        out.contains("EXTRA:\n   0      extra: *\nFor a total of 3 activations.\n"),
        "{out}"
    );
    assert!(out.ends_with("second|first|"), "{out}");
}

#[test]
fn strategy_commands_echo_the_old_and_current_strategy_at_the_prompt() {
    let directory = tempfile::tempdir().unwrap();
    let output = invoke(
        &["repl"],
        "(set-strategy breadth)\n(get-strategy)\n(exit)\n",
        directory.path(),
    );
    successful(&output);
    assert!(stdout(&output).contains("depth\nbreadth\n"), "{output:?}");
}

#[test]
fn source_reset_selects_main_for_the_default_agenda_view() {
    let output = run(
        "(defrule main =>) (defmodule A) (defrule other =>) (reset) (agenda)",
        "",
        false,
    );
    successful(&output);
    assert_eq!(
        stdout(&output),
        "0      main: *\nFor a total of 1 activation.\n"
    );
}

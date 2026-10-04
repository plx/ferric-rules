//! CLI consumers receive actionable, located conditional-element diagnostics.

use std::process::Command;

#[test]
fn unsupported_conditional_elements_are_located_in_text_and_json() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("limits.clp");
    for (pattern, detail) in [
        ("(not (not (not (not (not (item))))))", "nesting depth"),
        ("(exists (not (item)))", "exists"),
        ("(forall (a ?x) (forall (b ?x) (c ?x)))", "forall"),
        ("(not (forall (a ?x) (b ?x)))", "forall"),
        ("(exists (forall (a ?x) (b ?x)))", "forall"),
        ("(forall (a ?x) (b ?x) (c ?x))", "forall"),
        ("(forall (or (a ?x) (b ?x)) (c ?x))", "forall"),
    ] {
        std::fs::write(
            &path,
            format!("; diagnostic location\n(defrule limited\n  {pattern}\n  =>)\n"),
        )
        .unwrap();
        for command in ["run", "check"] {
            for json in [false, true] {
                let mut invocation = Command::new(env!("CARGO_BIN_EXE_ferric"));
                invocation.arg(command);
                if json {
                    invocation.arg("--json");
                }
                let output = invocation.arg(&path).output().unwrap();
                assert_eq!(output.status.code(), Some(1), "{pattern}: {output:?}");
                assert!(output.stdout.is_empty(), "{pattern}: {output:?}");
                let stderr = String::from_utf8(output.stderr).unwrap();
                let messages: Vec<String> = if json {
                    stderr
                        .lines()
                        .map(|line| {
                            let diagnostic: serde_json::Value = serde_json::from_str(line).unwrap();
                            assert_eq!(diagnostic["command"], command);
                            assert_eq!(diagnostic["level"], "error");
                            assert_eq!(diagnostic["kind"], "load_error");
                            diagnostic["message"].as_str().unwrap().to_owned()
                        })
                        .collect()
                } else {
                    vec![stderr]
                };
                assert!(
                    messages.iter().any(|message| {
                        message.contains(detail)
                            && (message.contains("line 3") || message.contains("at 3:"))
                    }),
                    "{command} {pattern}: {messages:?}"
                );
                assert!(
                    messages
                        .iter()
                        .all(|message| !message.contains("unexpectedly")),
                    "internal invariant leaked for {pattern}: {messages:?}"
                );
            }
        }
    }
}

#[test]
fn nested_bindings_disjunction_and_quantified_tests_execute_through_cli() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested.clp");
    for (source, expected) in [
        (
            "(deffacts d (a 1) (b 2))
             (defrule r (or ?f <- (a ?x) ?f <- (b ?x))
               => (retract ?f) (printout t retracted ?x crlf))",
            "retracted2\nretracted1\n",
        ),
        (
            "(defrule no (not (or (b 1) (c 1))) => (printout t absent crlf))",
            "absent\n",
        ),
        (
            "(deffacts d (a 1) (a 2))
             (defrule all-positive (forall (a ?x) (test (> ?x 0)))
               => (printout t all-positive crlf))",
            "all-positive\n",
        ),
    ] {
        std::fs::write(&path, source).unwrap();
        for command in ["check", "run"] {
            let output = Command::new(env!("CARGO_BIN_EXE_ferric"))
                .arg(command)
                .arg(&path)
                .output()
                .unwrap();
            assert!(output.status.success(), "{source}: {output:?}");
            assert!(output.stderr.is_empty(), "{source}: {output:?}");
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                if command == "run" { expected } else { "" },
                "{command}: {source}"
            );
        }
    }
}

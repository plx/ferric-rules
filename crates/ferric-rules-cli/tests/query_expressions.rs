//! Match-time expression failures are visible even when no rule activates.

use std::process::Command;

#[test]
fn lhs_evaluation_diagnostics_survive_load_and_reset_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("match-error.clp");
    for assertion in ["(assert (item 0))", "(deffacts seed (item 0))"] {
        std::fs::write(
            &path,
            format!(
                "(defrule bad (item ?x) (test (> (/ 1 ?x) 0)) => (printout t bad crlf))
                 (defrule good => (printout t continued crlf))
                 {assertion}",
            ),
        )
        .unwrap();
        for json in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_ferric"));
            command.arg("run");
            if json {
                command.arg("--json");
            }
            let output = command.arg(&path).output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"continued\n");
            let stderr = String::from_utf8(output.stderr).unwrap();
            let message = if json {
                let lines: Vec<_> = stderr.lines().collect();
                assert_eq!(lines.len(), 1, "{stderr}");
                let diagnostic: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
                assert_eq!(diagnostic["command"], "run");
                assert_eq!(diagnostic["level"], "warning");
                assert_eq!(diagnostic["kind"], "action_warning");
                diagnostic["message"].as_str().unwrap().to_owned()
            } else {
                stderr
            };
            assert!(message.contains("zero"), "{message}");
            assert!(message.contains("line 1"), "{message}");
        }
    }
}

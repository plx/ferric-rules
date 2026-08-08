//! Black-box tests for the dedicated structured compatibility observation.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE_ID: &str = "tests/fixtures/compat-observation.clp";
const NONCE: &str = "0123456789abcdef0123456789abcdef";
const SOURCE: &str = r#"
(deftemplate person
  (slot name)
  (multislot tags))

(deffacts seed
  (person (name "Ada") (tags))
  (go))

(defrule report-person
  ?g <- (go)
  ?p <- (person (name "Ada"))
  =>
  (retract ?g)
  (modify ?p (tags (create$ alpha 2 3.5)))
  (set-fact-duplication TRUE)
  (assert (signal "ready" 7))
  (assert (signal "ready" 7))
  (printout t "hello" crlf)
  (printout stderr "problem" crlf))
"#;

fn run_ferric(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferric"))
        .args(args)
        .output()
        .expect("execute ferric binary")
}

fn run_ferric_in(current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferric"))
        .current_dir(current_dir)
        .args(args)
        .output()
        .expect("execute ferric binary")
}

fn assert_exit_code(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        stdout_str(output),
        stderr_str(output)
    );
}

fn stdout_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_str(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn sha256(source: &[u8]) -> String {
    format!("{:x}", Sha256::digest(source))
}

fn source_file(source: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("create source file");
    file.write_all(source.as_bytes())
        .expect("write source file");
    file.flush().expect("flush source file");
    file
}

fn observe_with_digests(
    source: &str,
    source_sha256: &str,
    composed_sha256: &str,
) -> std::process::Output {
    let file = source_file(source);
    run_ferric(&[
        "compat-observe",
        "--fixture-id",
        FIXTURE_ID,
        "--nonce",
        NONCE,
        "--source-sha256",
        source_sha256,
        "--composed-sha256",
        composed_sha256,
        file.path().to_str().expect("UTF-8 temporary path"),
    ])
}

fn observe(source: &str, composed_sha256: &str) -> std::process::Output {
    observe_with_digests(source, &sha256(source.as_bytes()), composed_sha256)
}

fn observe_scenario_with_digests(
    repo: &Path,
    plan: &str,
    source_sha256: &str,
    composed_sha256: &str,
) -> Output {
    let plan_path = repo.join("scenario.plan");
    std::fs::write(&plan_path, plan).expect("write scenario plan");
    run_ferric_in(
        repo,
        &[
            "compat-observe",
            "--fixture-id",
            FIXTURE_ID,
            "--nonce",
            NONCE,
            "--source-sha256",
            source_sha256,
            "--composed-sha256",
            composed_sha256,
            "--scenario",
            plan_path.to_str().expect("UTF-8 scenario path"),
        ],
    )
}

fn observe_scenario(repo: &Path, plan: &str, source_sha256: &str) -> Output {
    observe_scenario_with_digests(repo, plan, source_sha256, &sha256(plan.as_bytes()))
}

fn write_scenario_source(repo: &Path, relative_path: &str, source: &str) {
    let path = repo.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create scenario source directory");
    }
    std::fs::write(path, source).expect("write scenario source");
}

fn channel_text<'a>(observation: &'a Value, name: &str) -> &'a str {
    observation["channels"]
        .as_array()
        .expect("channel array")
        .iter()
        .find(|channel| channel["name"] == name)
        .unwrap_or_else(|| panic!("missing channel {name}"))["text"]
        .as_str()
        .expect("channel text")
}

fn parse_single_observation(output: &std::process::Output) -> Value {
    let stdout = stdout_str(output);
    assert_eq!(
        stdout.lines().count(),
        1,
        "expected exactly one JSON line on stdout: {stdout:?}"
    );
    serde_json::from_str(&stdout).expect("stdout must be a JSON observation")
}

fn assert_identity_and_lifecycle(observation: &Value, digest: &str) {
    assert_eq!(observation["schema"], "ferric.compat-observation");
    assert_eq!(observation["version"], 1);
    assert_eq!(observation["engine"]["name"], "ferric");
    assert_eq!(observation["fixture"]["id"], FIXTURE_ID);
    assert_eq!(observation["fixture"]["nonce"], NONCE);
    assert_eq!(observation["fixture"]["source_sha256"], digest);
    assert_eq!(observation["fixture"]["composed_sha256"], digest);
    assert_eq!(observation["phase_reached"], "post-run");

    let lifecycle = observation["lifecycle"]
        .as_array()
        .expect("lifecycle array");
    assert_eq!(lifecycle.len(), 2);
    assert_eq!(lifecycle[0]["sequence"], 0);
    assert_eq!(lifecycle[0]["event"], "START");
    assert_eq!(lifecycle[1]["sequence"], 1);
    assert_eq!(lifecycle[1]["event"], "COMPLETE");
    for record in lifecycle {
        assert_eq!(record["fixture_id"], FIXTURE_ID);
        assert_eq!(record["nonce"], NONCE);
        assert_eq!(record["source_sha256"], digest);
        assert_eq!(record["composed_sha256"], digest);
    }
}

#[test]
fn successful_observation_captures_typed_state_and_lifecycle() {
    let digest = sha256(SOURCE.as_bytes());
    let output = observe(SOURCE, &digest);
    assert_exit_code(&output, 0);
    assert!(
        stderr_str(&output).is_empty(),
        "successful structured observation must not leak diagnostics to stderr"
    );

    let observation = parse_single_observation(&output);
    assert_identity_and_lifecycle(&observation, &digest);

    assert_eq!(observation["run"]["rules_fired"], 1);
    assert!(observation["run"]["fired_rule_names"].is_null());
    assert_eq!(observation["run"]["halt_reason"], "agenda-empty");
    assert_eq!(observation["run"]["agenda_size"], 0);
    assert_eq!(observation["run"]["halted"], false);
    assert_eq!(observation["diagnostics"], Value::Array(Vec::new()));

    let facts = observation["facts"].as_array().expect("facts array");
    assert_eq!(facts.len(), 3);
    for (ordinal, fact) in facts.iter().enumerate() {
        assert_eq!(fact["ordinal"], ordinal);
        assert!(fact["fact_id"]
            .as_str()
            .expect("fact id string")
            .bytes()
            .all(|byte| byte.is_ascii_digit()));
        // With only MAIN registered, ownership is unambiguous despite the
        // absence of a per-fact module accessor.
        assert_eq!(fact["module"], "MAIN");
    }

    let template = facts
        .iter()
        .find(|fact| fact["kind"] == "template")
        .expect("template fact");
    assert_eq!(template["name"], "person");
    let slots = template["slots"].as_array().expect("template slots");
    assert_eq!(slots[0]["name"], "name");
    assert_eq!(slots[0]["value"]["type"], "string");
    assert_eq!(slots[0]["value"]["value"], "Ada");
    assert_eq!(slots[1]["name"], "tags");
    assert_eq!(slots[1]["value"]["type"], "multifield");
    assert_eq!(slots[1]["value"]["values"][0]["type"], "symbol");
    assert_eq!(slots[1]["value"]["values"][0]["value"], "alpha");
    assert_eq!(slots[1]["value"]["values"][1]["type"], "integer");
    assert_eq!(slots[1]["value"]["values"][1]["value"], "2");
    assert_eq!(slots[1]["value"]["values"][2]["type"], "float");
    assert_eq!(slots[1]["value"]["values"][2]["value"], "3.5");
    assert_eq!(slots[1]["value"]["values"][2]["bits"], "0x400c000000000000");

    let ordered = facts
        .iter()
        .filter(|fact| fact["kind"] == "ordered")
        .collect::<Vec<_>>();
    assert_eq!(ordered.len(), 2, "duplicate facts must remain distinct");
    assert_ne!(ordered[0]["fact_id"], ordered[1]["fact_id"]);
    assert_eq!(ordered[0]["fields"], ordered[1]["fields"]);
    assert_eq!(ordered[0]["relation"], "signal");
    assert_eq!(ordered[0]["fields"][0]["type"], "string");
    assert_eq!(ordered[0]["fields"][0]["value"], "ready");
    assert_eq!(ordered[0]["fields"][1]["type"], "integer");
    assert_eq!(ordered[0]["fields"][1]["value"], "7");

    let channels = observation["channels"].as_array().expect("channel array");
    let channel = |name: &str| {
        channels
            .iter()
            .find(|channel| channel["name"] == name)
            .unwrap_or_else(|| panic!("missing channel {name}"))
    };
    assert_eq!(channel("t")["present"], true);
    assert_eq!(channel("t")["text"], "hello\n");
    assert_eq!(channel("stderr")["present"], true);
    assert_eq!(channel("stderr")["text"], "problem\n");
    assert_eq!(channel("stdout")["present"], false);
    assert_eq!(channel("stdout")["text"], "");

    assert_eq!(observation["modules"]["current"], "MAIN");
    assert_eq!(observation["modules"]["focus"], "MAIN");
    assert_eq!(
        observation["modules"]["focus_stack"],
        serde_json::json!(["MAIN"])
    );
    assert_eq!(observation["capabilities"]["fact_modules"], true);
    assert_eq!(observation["capabilities"]["fired_rule_names"], false);
    assert_eq!(
        observation["capabilities"]["router_channel_enumeration"],
        false
    );
    assert_eq!(
        observation["capabilities"]["composed_digest_verification"],
        true
    );
}

#[test]
fn digest_mismatch_emits_completed_harness_failure_observation() {
    let output = observe(SOURCE, &"0".repeat(64));
    assert_exit_code(&output, 1);
    assert!(stderr_str(&output).is_empty());

    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "load");
    assert!(observation["run"].is_null());
    assert_eq!(observation["lifecycle"].as_array().unwrap().len(), 2);
    assert_eq!(observation["lifecycle"][0]["event"], "START");
    assert_eq!(observation["lifecycle"][1]["event"], "COMPLETE");
    assert_eq!(observation["diagnostics"][0]["taxonomy_version"], 1);
    assert_eq!(observation["diagnostics"][0]["phase"], "harness");
    assert_eq!(observation["diagnostics"][0]["category"], "harness-error");
    assert_eq!(observation["diagnostics"][0]["continued"], false);
    assert_eq!(observation["modules"]["current"], "MAIN");
}

#[test]
fn v1_positional_input_remains_compatible_and_does_not_claim_source_verification() {
    let source_sha256 = "1".repeat(64);
    let composed_sha256 = sha256(SOURCE.as_bytes());
    let output = observe_with_digests(SOURCE, &source_sha256, &composed_sha256);
    assert_exit_code(&output, 0);

    let observation = parse_single_observation(&output);
    assert_eq!(
        observation["fixture"]["source_sha256"],
        source_sha256.as_str()
    );
    assert_eq!(
        observation["fixture"]["composed_sha256"],
        composed_sha256.as_str()
    );
    for record in observation["lifecycle"].as_array().unwrap() {
        assert_eq!(record["source_sha256"], source_sha256.as_str());
        assert_eq!(record["composed_sha256"], composed_sha256.as_str());
    }
    assert_eq!(
        observation["capabilities"]["source_digest_verification"],
        false
    );
    assert_eq!(
        observation["capabilities"]["composed_digest_verification"],
        true
    );
}

#[test]
fn parse_failure_emits_completed_versioned_syntax_diagnostic() {
    let invalid_source = "(defrule incomplete";
    let digest = sha256(invalid_source.as_bytes());
    let output = observe(invalid_source, &digest);
    assert_exit_code(&output, 1);
    assert!(stderr_str(&output).is_empty());

    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "parse");
    assert!(observation["run"].is_null());
    assert_eq!(observation["lifecycle"].as_array().unwrap().len(), 2);
    assert_eq!(observation["lifecycle"][0]["event"], "START");
    assert_eq!(observation["lifecycle"][1]["event"], "COMPLETE");
    assert_eq!(observation["diagnostics"][0]["taxonomy_version"], 1);
    assert_eq!(observation["diagnostics"][0]["phase"], "parse");
    assert_eq!(observation["diagnostics"][0]["severity"], "error");
    assert_eq!(observation["diagnostics"][0]["category"], "syntax-error");
    assert_eq!(observation["diagnostics"][0]["continued"], false);
}

#[test]
fn construct_failure_is_distinct_from_parse_failure() {
    let invalid_source = "(not-a-clips-construct)";
    let digest = sha256(invalid_source.as_bytes());
    let output = observe(invalid_source, &digest);
    assert_exit_code(&output, 1);
    assert!(stderr_str(&output).is_empty());

    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "load");
    assert!(observation["run"].is_null());
    assert_eq!(observation["lifecycle"][1]["event"], "COMPLETE");
    assert_eq!(observation["diagnostics"][0]["taxonomy_version"], 1);
    assert_eq!(observation["diagnostics"][0]["phase"], "load");
    assert_eq!(observation["diagnostics"][0]["category"], "construct-error");
    assert_eq!(observation["diagnostics"][0]["continued"], false);
}

#[test]
fn nonfatal_load_diagnostic_records_that_execution_continued() {
    let source = "42";
    let digest = sha256(source.as_bytes());
    let output = observe(source, &digest);
    assert_exit_code(&output, 0);

    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "post-run");
    assert_eq!(observation["run"]["halt_reason"], "agenda-empty");
    assert_eq!(observation["diagnostics"][0]["taxonomy_version"], 1);
    assert_eq!(observation["diagnostics"][0]["phase"], "load");
    assert_eq!(observation["diagnostics"][0]["severity"], "warning");
    assert_eq!(observation["diagnostics"][0]["category"], "construct-error");
    assert_eq!(observation["diagnostics"][0]["continued"], true);
}

#[test]
fn multiple_modules_leave_fact_ownership_explicitly_unavailable() {
    let source = r"
(defmodule MAIN (export ?ALL))
(deffacts main-seed (main-fact))
(defmodule AUX)
(deffacts aux-seed (aux-fact))
";
    let digest = sha256(source.as_bytes());
    let output = observe(source, &digest);
    assert_exit_code(&output, 0);

    let observation = parse_single_observation(&output);
    assert_eq!(observation["capabilities"]["fact_modules"], false);
    let facts = observation["facts"].as_array().expect("facts array");
    assert_eq!(facts.len(), 2);
    assert!(facts.iter().all(|fact| fact["module"].is_null()));
}

#[test]
fn template_facts_report_qualified_ownership_when_another_module_is_current() {
    let source = r"
(defmodule A (export ?ALL))
(defmodule B (import A ?ALL))
(deftemplate A::secret (slot value))
(deffacts B::startup (secret (value 7)))
";
    let digest = sha256(source.as_bytes());
    let output = observe(source, &digest);
    assert_exit_code(&output, 0);

    let observation = parse_single_observation(&output);
    assert_eq!(observation["capabilities"]["fact_modules"], true);
    let facts = observation["facts"].as_array().expect("facts array");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0]["kind"], "template");
    assert_eq!(facts[0]["module"], "A");
    assert_eq!(facts[0]["name"], "secret");
}

#[test]
fn action_error_stops_before_halt_and_is_captured_without_output_leakage() {
    let source = r#"
(deffacts seed (channel t))
(defrule stop-first
  (declare (salience 10))
  (channel ?channel)
  =>
  (printout ?channel "must-not-leak")
  (halt))
(defrule left-on-agenda
  (channel ?channel)
  =>
  (printout t "must-not-run"))
"#;
    let digest = sha256(source.as_bytes());
    let output = observe(source, &digest);
    assert_exit_code(&output, 0);
    assert!(stderr_str(&output).is_empty());

    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "run");
    assert_eq!(observation["run"]["rules_fired"], 1);
    assert_eq!(observation["run"]["halt_reason"], "action-error");
    assert_eq!(observation["run"]["agenda_size"], 1);
    assert_eq!(observation["run"]["halted"], false);
    assert_eq!(observation["diagnostics"][0]["phase"], "run");
    assert_eq!(observation["diagnostics"][0]["severity"], "warning");
    assert_eq!(observation["diagnostics"][0]["taxonomy_version"], 1);
    assert_eq!(
        observation["diagnostics"][0]["category"],
        "evaluation-error"
    );
    assert_eq!(observation["diagnostics"][0]["continued"], false);
    assert!(observation["channels"]
        .as_array()
        .unwrap()
        .iter()
        .all(|channel| channel["text"] == ""));
}

#[test]
fn cli_rejects_noncanonical_digest_before_execution() {
    let file = source_file(SOURCE);
    let digest = sha256(SOURCE.as_bytes());
    let uppercase_digest = digest.to_uppercase();
    let output = run_ferric(&[
        "compat-observe",
        "--fixture-id",
        FIXTURE_ID,
        "--nonce",
        NONCE,
        "--source-sha256",
        &uppercase_digest,
        "--composed-sha256",
        &digest,
        file.path().to_str().expect("UTF-8 temporary path"),
    ]);

    assert_exit_code(&output, 2);
    assert!(stdout_str(&output).is_empty());
    assert!(stderr_str(&output).contains("64 lowercase hexadecimal"));
}

#[test]
fn scenario_stages_a_late_source_before_reset_and_run() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let late_path = "tests/examples/late.clp";
    let primary = r"
(deftemplate marker (slot value))
(defrule report
  (marker (value ?value))
  =>
  (printout t ?value crlf))
";
    let late = "(deffacts late-seed (marker (value late)))\n";
    write_scenario_source(repo.path(), primary_path, primary);
    write_scenario_source(repo.path(), late_path, late);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         SOURCE|late|{}|{late_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|LOAD|late|stop\n\
         STEP|3|RESET|-|stop\n\
         STEP|4|RUN|-1|stop\n\
         END\n",
        sha256(late.as_bytes())
    );

    let output = observe_scenario(repo.path(), &plan, &primary_digest);
    assert_exit_code(&output, 0);
    assert!(stderr_str(&output).is_empty());
    let observation = parse_single_observation(&output);
    let plan_digest = sha256(plan.as_bytes());
    assert_eq!(
        observation["fixture"]["source_sha256"],
        primary_digest.as_str()
    );
    assert_eq!(
        observation["fixture"]["composed_sha256"],
        plan_digest.as_str()
    );
    for record in observation["lifecycle"]
        .as_array()
        .expect("lifecycle array")
    {
        assert_eq!(record["source_sha256"], primary_digest.as_str());
        assert_eq!(record["composed_sha256"], plan_digest.as_str());
    }
    assert_eq!(observation["phase_reached"], "post-run");
    assert_eq!(observation["run"]["rules_fired"], 1);
    assert_eq!(channel_text(&observation, "t"), "late\n");
    assert_eq!(
        observation["capabilities"]["source_digest_verification"],
        true
    );
}

#[test]
fn scenario_late_definition_replaces_a_callable_in_the_same_engine() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let replacement_path = "tests/examples/replacement.clp";
    let primary = r"
(deffunction answer () 1)
(deffacts seed (go))
(defrule report (go) => (printout t (answer) crlf))
";
    let replacement = "(deffunction answer () 2)\n";
    write_scenario_source(repo.path(), primary_path, primary);
    write_scenario_source(repo.path(), replacement_path, replacement);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         SOURCE|replacement|{}|{replacement_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|LOAD|replacement|stop\n\
         STEP|3|RESET|-|stop\n\
         STEP|4|RUN|-1|stop\n\
         END\n",
        sha256(replacement.as_bytes())
    );

    let output = observe_scenario(repo.path(), &plan, &primary_digest);
    assert_exit_code(&output, 0);
    let observation = parse_single_observation(&output);
    assert_eq!(observation["run"]["rules_fired"], 1);
    assert_eq!(channel_text(&observation, "t"), "2\n");
}

#[test]
fn scenario_expected_load_error_can_continue_to_reset_and_run() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let broken_path = "tests/examples/broken.clp";
    let primary = "(deffacts seed (go))\n(defrule report (go) => (printout t ok crlf))\n";
    let broken = "(not-a-clips-construct)\n";
    write_scenario_source(repo.path(), primary_path, primary);
    write_scenario_source(repo.path(), broken_path, broken);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         SOURCE|broken|{}|{broken_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|LOAD|broken|continue\n\
         STEP|3|RESET|-|stop\n\
         STEP|4|RUN|-1|stop\n\
         END\n",
        sha256(broken.as_bytes())
    );

    let output = observe_scenario(repo.path(), &plan, &primary_digest);
    assert_exit_code(&output, 0);
    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "post-run");
    assert_eq!(channel_text(&observation, "t"), "ok\n");
    let diagnostic = observation["diagnostics"]
        .as_array()
        .expect("diagnostic array")
        .iter()
        .find(|diagnostic| diagnostic["phase"] == "load")
        .expect("continued load diagnostic");
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["category"], "construct-error");
    assert_eq!(diagnostic["continued"], true);
}

#[test]
fn scenario_stop_policy_terminates_on_load_error() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let primary = "(not-a-clips-construct)\n";
    write_scenario_source(repo.path(), primary_path, primary);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|RESET|-|stop\n\
         STEP|3|RUN|-1|stop\n\
         END\n"
    );

    let output = observe_scenario(repo.path(), &plan, &primary_digest);
    assert_exit_code(&output, 1);
    let observation = parse_single_observation(&output);
    assert_eq!(observation["phase_reached"], "load");
    assert!(observation["run"].is_null());
    assert_eq!(observation["diagnostics"][0]["category"], "construct-error");
    assert_eq!(observation["diagnostics"][0]["continued"], false);
}

#[test]
fn scenario_applies_the_declared_conflict_strategy() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/strategy.clp";
    let primary = r"
(deffacts seed (go))
(defrule first (go) => (printout t first crlf))
(defrule second (go) => (printout t second crlf))
";
    write_scenario_source(repo.path(), primary_path, primary);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|RESET|-|stop\n\
         STEP|3|SET-STRATEGY|breadth|stop\n\
         STEP|4|RUN|-1|stop\n\
         END\n"
    );

    let output = observe_scenario(repo.path(), &plan, &primary_digest);
    assert_exit_code(&output, 0);
    let observation = parse_single_observation(&output);
    assert_eq!(observation["run"]["rules_fired"], 2);
    assert_eq!(channel_text(&observation, "t"), "second\nfirst\n");
}

#[test]
fn scenario_rejects_malformed_sequence_before_execution() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let primary = "(deffacts seed (must-not-run))\n";
    write_scenario_source(repo.path(), primary_path, primary);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         STEP|2|LOAD|primary|stop\n\
         STEP|3|RUN|-1|stop\n\
         END\n"
    );

    let output = observe_scenario(repo.path(), &plan, &primary_digest);
    assert_exit_code(&output, 1);
    let observation = parse_single_observation(&output);
    assert!(observation["run"].is_null());
    assert_eq!(observation["diagnostics"][0]["phase"], "harness");
    assert_eq!(observation["diagnostics"][0]["category"], "harness-error");
    assert!(observation["diagnostics"][0]["message"]
        .as_str()
        .expect("diagnostic message")
        .contains("sequence"));
}

#[test]
fn scenario_rejects_absolute_and_parent_traversal_source_paths() {
    for escaped_path in [
        "../outside.clp",
        "/tmp/outside.clp",
        "tests/examples/nested/../outside.clp",
        "tests/examples/./outside.clp",
    ] {
        let repo = tempfile::tempdir().expect("create scenario repository");
        let declared_digest = "0".repeat(64);
        let plan = format!(
            "FERRIC-COMPAT-SCENARIO|1\n\
             SOURCE|primary|{declared_digest}|{escaped_path}\n\
             STEP|1|LOAD|primary|stop\n\
             STEP|2|RESET|-|stop\n\
             STEP|3|RUN|-1|stop\n\
             END\n"
        );

        let output = observe_scenario(repo.path(), &plan, &declared_digest);
        assert_exit_code(&output, 1);
        let observation = parse_single_observation(&output);
        assert_eq!(observation["diagnostics"][0]["phase"], "harness");
        assert!(observation["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("repo-relative"));
    }
}

#[test]
fn scenario_rejects_a_stale_declared_source_digest_before_execution() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let primary = "(deffacts seed (must-not-run))\n";
    write_scenario_source(repo.path(), primary_path, primary);
    let stale_digest = "0".repeat(64);
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{stale_digest}|{primary_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|RESET|-|stop\n\
         STEP|3|RUN|-1|stop\n\
         END\n"
    );

    let output = observe_scenario(repo.path(), &plan, &stale_digest);
    assert_exit_code(&output, 1);
    let observation = parse_single_observation(&output);
    assert!(observation["run"].is_null());
    assert_eq!(observation["diagnostics"][0]["phase"], "harness");
    assert!(observation["diagnostics"][0]["message"]
        .as_str()
        .expect("diagnostic message")
        .contains("digest mismatch"));
}

#[test]
fn scenario_rejects_a_primary_digest_that_disagrees_with_the_invocation() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let primary = "(deffacts seed (must-not-run))\n";
    write_scenario_source(repo.path(), primary_path, primary);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|RESET|-|stop\n\
         STEP|3|RUN|-1|stop\n\
         END\n"
    );

    let output = observe_scenario(repo.path(), &plan, &"1".repeat(64));
    assert_exit_code(&output, 1);
    let observation = parse_single_observation(&output);
    assert!(observation["run"].is_null());
    assert_eq!(observation["diagnostics"][0]["phase"], "harness");
    assert!(observation["diagnostics"][0]["message"]
        .as_str()
        .expect("diagnostic message")
        .contains("primary source digest mismatch"));
}

#[test]
fn scenario_rejects_a_stale_composed_plan_digest_before_parsing() {
    let repo = tempfile::tempdir().expect("create scenario repository");
    let primary_path = "tests/examples/primary.clp";
    let primary = "(deffacts seed (must-not-run))\n";
    write_scenario_source(repo.path(), primary_path, primary);
    let primary_digest = sha256(primary.as_bytes());
    let plan = format!(
        "FERRIC-COMPAT-SCENARIO|1\n\
         SOURCE|primary|{primary_digest}|{primary_path}\n\
         STEP|1|LOAD|primary|stop\n\
         STEP|2|RESET|-|stop\n\
         STEP|3|RUN|-1|stop\n\
         END\n"
    );

    let output =
        observe_scenario_with_digests(repo.path(), &plan, &primary_digest, &"0".repeat(64));
    assert_exit_code(&output, 1);
    let observation = parse_single_observation(&output);
    assert!(observation["run"].is_null());
    assert_eq!(observation["diagnostics"][0]["phase"], "harness");
    assert!(observation["diagnostics"][0]["message"]
        .as_str()
        .expect("diagnostic message")
        .contains("scenario plan digest mismatch"));
}

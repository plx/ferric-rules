use ferric_rules_runtime::{Engine, EngineConfig, LoadError, RunLimit, Value};
use std::fmt::Write;

fn binary_groups(count: usize) -> String {
    (0..count).fold(String::new(), |mut source, n| {
        write!(source, "(or (left {n}) (right {n})) ").unwrap();
        source
    })
}

#[test]
fn excessive_ce_and_slot_products_fail_before_installation() {
    let slot_or = std::iter::repeat("0|1")
        .take(9)
        .collect::<Vec<_>>()
        .join(" ");
    for lhs in [
        binary_groups(9),
        binary_groups(80), // mathematical product exceeds usize; must never wrap
        format!("(choice {slot_or})"),
        format!("(not (and (seed) {}))", binary_groups(9)),
    ] {
        let mut engine = Engine::new(EngineConfig::default());
        let errors = engine
            .load_str(&format!("(defrule excessive {lhs} =>)"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, LoadError::ResourceLimit {
            rule, resource, required, limit: 256, line: 1, ..
        } if rule == "excessive" && resource.contains("alternatives") && *required > 256)),
            "{errors:?}"
        );
        assert!(engine.rules().is_empty());
        assert_eq!(engine.agenda_len(), 0);
        #[cfg(debug_assertions)]
        engine.debug_assert_consistency();
    }
}

#[test]
fn supported_product_preserves_matching_and_failed_replacement_is_atomic() {
    let mut engine = Engine::new(EngineConfig::default());
    engine
        .load_str(&format!(
            "(defrule choose {} => (assert (selected)))",
            binary_groups(8)
        ))
        .unwrap();
    for n in 0..8 {
        engine.assert_ordered("left", Value::Integer(n)).unwrap();
    }
    assert_eq!(engine.agenda_len(), 1);
    assert!(engine
        .load_str(&format!(
            "(defrule choose {} => (assert (wrong)))",
            binary_groups(9)
        ))
        .is_err());
    assert_eq!(engine.agenda_len(), 1);
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("selected").unwrap().len(), 1);
    assert!(engine.find_facts("wrong").unwrap().is_empty());
}

#[test]
fn excessive_network_depth_is_rejected_without_replacing_existing_work() {
    for lhs in [
        (0..65).fold(String::new(), |mut source, n| {
            write!(source, "(r{n}) ").unwrap();
            source
        }),
        format!(
            "(wide {})",
            std::iter::repeat("1")
                .take(65)
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ] {
        let mut engine = Engine::with_rules("(defrule keep => (assert (kept)))").unwrap();
        let errors = engine
            .load_str(&format!("(defrule keep {lhs} =>)"))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("supported limit of 64")),
            "{errors:?}"
        );
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
        assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
    }
}

#[test]
fn supported_network_depth_runs_and_retracts_on_a_small_native_stack() {
    const CHILD: &str = "FERRIC_SOURCE_LIMIT_STACK_CHILD";
    const NAME: &str = "supported_network_depth_runs_and_retracts_on_a_small_native_stack";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let lhs = (0..64).fold(String::new(), |mut source, n| {
                    write!(source, "(r{n} ?v{n}) ").unwrap();
                    source
                });
                let mut engine =
                    Engine::with_rules(&format!("(defrule deep {lhs} => (assert (matched)))"))
                        .unwrap();
                let mut ids = Vec::new();
                for n in (0..64).rev() {
                    ids.push(
                        engine
                            .assert_ordered(&format!("r{n}"), Value::Integer(n))
                            .unwrap(),
                    );
                }
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
                engine.retract(*ids.last().unwrap()).unwrap();
                assert_eq!(engine.agenda_len(), 0);
                engine.reset().unwrap();
                assert_eq!(engine.facts().unwrap().count(), 0);

                let fields = std::iter::repeat("1")
                    .take(64)
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut engine =
                    Engine::with_rules(&format!("(defrule alpha (wide {fields}) =>)")).unwrap();
                let id = engine
                    .assert_ordered("wide", vec![Value::Integer(1); 64])
                    .unwrap();
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
                engine.retract(id).unwrap();

                // Exercise both bounds in one propagation using distinct relations.
                let lhs = (0..64).fold(String::new(), |mut source, n| {
                    write!(source, "(combined{n} {fields} ?v{n}) ").unwrap();
                    source
                });
                let mut engine =
                    Engine::with_rules(&format!("(defrule combined {lhs} =>)")).unwrap();
                let mut ids = Vec::new();
                for n in (0..64).rev() {
                    ids.push(
                        engine
                            .assert_ordered(&format!("combined{n}"), vec![Value::Integer(1); 65])
                            .unwrap(),
                    );
                }
                assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
                engine.retract(*ids.last().unwrap()).unwrap();
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture"])
        .env(CHILD, "1")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "bounded network aborted its isolated small-stack process: {status}"
    );
}

#[test]
fn source_size_is_checked_before_parsing_and_preserves_existing_state() {
    let mut engine = Engine::with_rules("(defrule keep => (assert (kept)))").unwrap();
    let source = " ".repeat(16 * 1024 * 1024 + 1);
    let errors = engine.load_str(&source).unwrap_err();
    assert!(matches!(
        &errors[0],
        LoadError::ResourceLimit {
            resource: "source bytes",
            ..
        }
    ));
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
}

#[test]
fn source_files_are_bounded_before_reading_and_keep_io_errors_distinct() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.clp");
    // A sparse file proves file size is not used to preallocate the whole input.
    std::fs::File::create(&path)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let mut engine = Engine::with_rules("(defrule keep => (assert (kept)))").unwrap();
    let errors = engine.load_file(&path).unwrap_err();
    assert!(matches!(
        &errors[0],
        LoadError::ResourceLimit {
            resource: "source bytes",
            required: 16_777_217,
            ..
        }
    ));
    std::fs::write(&path, [0xff]).unwrap();
    assert!(matches!(
        &engine.load_file(&path).unwrap_err()[0],
        LoadError::Io(error) if error.kind() == std::io::ErrorKind::InvalidData
    ));
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        &engine.load_file(&path).unwrap_err()[0],
        LoadError::Io(error) if error.kind() == std::io::ErrorKind::NotFound
    ));
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
    std::fs::write(&path, "(defrule loaded => (assert (from-file)))").unwrap();
    engine.load_file(&path).unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("from-file").unwrap().len(), 1);
}

#[test]
fn empty_lhs_rules_still_obey_the_per_rule_byte_limit() {
    let mut engine = Engine::with_rules("(defrule keep => (assert (kept)))").unwrap();
    let payload = "a".repeat(8 * 1024 * 1024);
    let errors = engine
        .load_str(&format!("(defrule keep => (printout t \"{payload}\"))"))
        .unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            LoadError::ResourceLimit {
                resource: "expanded source bytes estimate",
                ..
            }
        )),
        "{errors:?}"
    );
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert_eq!(engine.find_facts("kept").unwrap().len(), 1);
}

#[test]
fn load_facts_bounds_file_reads_and_reports_resource_limits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.fct");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(1024 * 1024 * 1024)
        .unwrap();
    let escaped = path.to_string_lossy().replace('\\', "\\\\");
    let mut engine = Engine::with_rules(&format!(
        "(deffacts seeds (retained 7)) (defrule read => (load-facts \"{escaped}\"))"
    ))
    .unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    let diagnostics = engine.action_diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let diagnostic = diagnostics[0].to_string();
    assert!(diagnostic.contains("load-facts"), "{diagnostic}");
    assert!(diagnostic.contains("source bytes"), "{diagnostic}");
    assert!(diagnostic.contains("16777217"), "{diagnostic}");
    assert_eq!(engine.facts().unwrap().count(), 1);
    assert_eq!(engine.find_facts("retained").unwrap().len(), 1);

    std::fs::write(&path, "(loaded 42)").unwrap();
    engine.reset().unwrap();
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.find_facts("loaded").unwrap().len(), 1);
    assert_eq!(engine.find_facts("retained").unwrap().len(), 1);
}

#[test]
fn per_load_expansion_budget_preserves_already_installed_constructs() {
    let payload = "a".repeat(3 * 1024 * 1024);
    let source = (0..3).fold(String::new(), |mut source, n| {
        writeln!(
            source,
            "(defrule large{n} (or (left) (right)) => (printout t \"{payload}\"))"
        )
        .unwrap();
        source
    });
    let mut engine = Engine::new(EngineConfig::default());
    let errors = engine.load_str(&source).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, LoadError::ResourceLimit {
        rule, resource: "load expansion byte work", ..
    } if rule == "large2")),
        "{errors:?}"
    );
    // Loading is incremental across constructs. The third rule must not be
    // installed partially, while the first two remain usable.
    engine.assert_ordered("left", Vec::new()).unwrap();
    assert_eq!(engine.agenda_len(), 2);
}

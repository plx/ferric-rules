//! Public representation contracts for exact byte lexemes and instance names.
//!
//! These are host/API foundation controls, not scanner conformance goldens.
//! They use exact byte payloads supplied by the host and ordinary source
//! literals. The STRING/SYMBOL/INSTANCE-NAME distinctions follow CLIPS 6.30
//! scanner and predicate source; no claim is made that issues #338/#339 work.

use ferric_rules::core::{Fact, InstanceName, Value};
use ferric_rules::runtime::{Engine, EngineConfig, EngineError, HaltReason, HostValue, RunLimit};

const BYTES: &[u8] = b"a\0\xc3\xff";

fn load(engine: &mut Engine, source: &str) {
    engine
        .load_str(source)
        .unwrap_or_else(|errors| panic!("{errors:?}"));
}

fn run(engine: &mut Engine, expected: usize) {
    let result = engine.run(RunLimit::Count(100)).unwrap();
    assert_eq!(result.rules_fired, expected);
    assert!(
        engine.action_diagnostics().is_empty(),
        "{:?}",
        engine.action_diagnostics()
    );
}

fn global_integer(engine: &Engine, name: &str) -> i64 {
    let Some(Value::Integer(value)) = engine.get_global(name) else {
        panic!("global {name} must retain INTEGER identity");
    };
    *value
}

fn byte_output_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, "(defrule emit (payload ?string ?symbol ?name) => (printout t ?string \"|\" ?symbol \"|\" ?name))");
    engine.reset().unwrap();
    let string: HostValue = engine.create_string_bytes(BYTES).unwrap().into();
    let symbol = engine.symbol_value_bytes(BYTES).unwrap();
    let name = engine.instance_name_value_bytes(BYTES).unwrap();
    assert!(!string.as_value().structural_eq(symbol.as_value()));
    assert!(!name.as_value().structural_eq(symbol.as_value()));
    assert!(!name.as_value().structural_eq(string.as_value()));
    assert_eq!(string.as_value().type_name(), "STRING");
    assert_eq!(symbol.as_value().type_name(), "SYMBOL");
    assert_eq!(name.as_value().type_name(), "INSTANCE-NAME");
    engine
        .assert_ordered("payload", [string, symbol, name])
        .unwrap();
    engine
}

fn check_byte_output(engine: &Engine) {
    assert_eq!(
        engine.get_output_bytes("t"),
        Some(&b"a\0\xc3\xff|a\0\xc3\xff|[a\0\xc3\xff]"[..])
    );
    assert!(engine.get_output("t").is_err());
    assert_eq!(engine.get_output("unwritten").unwrap(), None);
}

#[test]
fn byte_values_keep_typed_identity_and_exact_checked_output() {
    let mut engine = byte_output_engine();
    run(&mut engine, 1);
    check_byte_output(&engine);
    let (_, Fact::Ordered(fact)) = engine.find_facts("payload").unwrap()[0] else {
        panic!("ordered payload");
    };
    let Value::String(string) = &fact.fields[0] else {
        panic!("STRING")
    };
    assert_eq!(string.as_bytes(), BYTES);
    assert!(string.as_str().is_err());
    let Value::Symbol(symbol) = fact.fields[1] else {
        panic!("SYMBOL")
    };
    assert_eq!(engine.resolve_core_symbol_bytes(symbol), Some(BYTES));
    let Value::InstanceName(name) = fact.fields[2] else {
        panic!("INSTANCE-NAME")
    };
    assert_eq!(
        engine.resolve_core_symbol_bytes(name.as_symbol()),
        Some(BYTES)
    );
}

const INSTANCE_SOURCE: &str = r#"
(deftemplate holder (slot value (type INSTANCE-NAME)))
(defglobal ?*captured* = FALSE)
(defgeneric classify)
(defmethod classify ((?value INSTANCE-NAME)) INSTANCE-NAME)
(defmethod classify ((?value SYMBOL)) SYMBOL)
(deffacts names (holder (value [widget])) (tag [widget]) (tag widget))
(defrule describe (holder (value ?name)) (tag [widget])
 =>
 (bind ?*captured* ?name)
 (printout t (instance-namep ?name) ":"
   (symbolp ?name) ":" (lexemep ?name) ":"
   (classify widget) ":" (eq ?name widget) ":"
   (eq (symbol-to-instance-name widget) [widget]) ":"
   (eq (instance-name-to-symbol [widget]) widget) crlf))
"#;
const INSTANCE_OUTPUT: &str = "TRUE:FALSE:FALSE:SYMBOL:FALSE:TRUE:TRUE\n";

fn instance_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, INSTANCE_SOURCE);
    engine.reset().unwrap();
    engine
}

fn check_instance_result(engine: &Engine) {
    assert_eq!(engine.get_output("t").unwrap(), Some(INSTANCE_OUTPUT));
    let Some(Value::InstanceName(name)) = engine.get_global("captured") else {
        panic!("typed global")
    };
    assert_eq!(
        engine.resolve_core_symbol_bytes(name.as_symbol()),
        Some(&b"widget"[..])
    );
}

#[test]
fn source_instance_literals_use_constant_matching_type_constraints_and_symbol_methods() {
    let mut engine = instance_engine();
    run(&mut engine, 1);
    check_instance_result(&engine);
    let symbol = engine.symbol_value("widget").unwrap();
    assert!(engine
        .assert_template("holder", &["value"], symbol)
        .is_err());
    let string = engine.create_string("widget").unwrap();
    assert!(engine
        .assert_template("holder", &["value"], string)
        .is_err());
    run(&mut engine, 0);
}

fn missing_instance_engine(expression: &str) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    load(
        &mut engine,
        &format!(
            "(defglobal ?*result* = pending ?*body* = 0 ?*after* = 0)
             (defgeneric classify)
             (defmethod classify ((?value INSTANCE-NAME))
               (bind ?*body* 1) INSTANCE-NAME)
             (defmethod classify ((?value SYMBOL)) SYMBOL)
             (defrule missing =>
               (bind ?*result* {expression})
               (bind ?*after* 1)
               (assert (after)))"
        ),
    );
    engine.reset().unwrap();
    engine
}

fn check_missing_instance_result(engine: &Engine) {
    let Some(Value::Symbol(result)) = engine.get_global("result") else {
        panic!("failed instance lookup must retain a SYMBOL result")
    };
    assert_eq!(engine.resolve_core_symbol(*result), Some("FALSE"));
    assert_eq!(global_integer(engine, "body"), 0);
    assert_eq!(global_integer(engine, "after"), 0);
    assert!(engine.find_facts("after").unwrap().is_empty());
    assert!(!engine.action_diagnostics().is_empty());
    assert!(engine
        .action_diagnostics()
        .iter()
        .any(|error| error.to_string().to_ascii_lowercase().contains("instance")));
}

fn run_missing_instance(engine: &mut Engine) {
    let result = engine.run(RunLimit::Count(100)).unwrap();
    assert_eq!(result.rules_fired, 1);
    assert_eq!(result.halt_reason, HaltReason::ActionError);
    check_missing_instance_result(engine);
}

#[test]
fn absent_instance_type_and_method_lookup_return_false_and_halt_the_rhs() {
    // A name value does not create a COOL instance. CLIPS looks up the named
    // instance for both `type` and generic argument dispatch before proceeding.
    for expression in ["(type [widget])", "(classify [widget])"] {
        run_missing_instance(&mut missing_instance_engine(expression));
    }
}

#[test]
fn instance_name_ownership_survives_nesting_and_rejects_forged_core_values() {
    let mut owner = Engine::new(EngineConfig::utf8());
    let mut other = Engine::new(EngineConfig::utf8());
    let handle = owner.intern_instance_name_bytes(BYTES).unwrap();
    assert_eq!(owner.resolve_instance_name_bytes(handle), Some(BYTES));
    assert!(owner.resolve_instance_name(handle).is_err());
    assert_eq!(other.resolve_instance_name_bytes(handle), None);
    let genuine: HostValue = handle.into();
    assert!(matches!(
        other.assert_ordered("foreign", genuine.clone()),
        Err(EngineError::ForeignHandle)
    ));
    let nested = HostValue::multifield(vec![7_i64.into(), genuine.clone()]).unwrap();
    assert!(matches!(
        other.assert_ordered("foreign", nested),
        Err(EngineError::ForeignHandle)
    ));
    let foreign = other.instance_name_value_bytes(BYTES).unwrap();
    assert!(matches!(
        HostValue::multifield(vec![genuine.clone(), foreign]),
        Err(EngineError::ForeignHandle)
    ));
    let Value::InstanceName(raw) = genuine.as_value() else {
        panic!("typed name")
    };
    let forged = Value::InstanceName(InstanceName::from_symbol(raw.as_symbol()));
    assert!(matches!(
        owner.assert_ordered("forged", forged.clone()),
        Err(EngineError::InvalidHostValue(_))
    ));
    let forged_nested =
        Value::Multifield(Box::new([Value::Integer(1), forged].into_iter().collect()));
    assert!(matches!(
        HostValue::multifield(vec![genuine, forged_nested.into()]),
        Err(EngineError::InvalidHostValue(_))
    ));
    let portable = owner.create_string_bytes(BYTES).unwrap();
    other.assert_ordered("portable", portable).unwrap();
    assert!(owner.find_facts("forged").unwrap().is_empty());
    assert!(other.find_facts("foreign").unwrap().is_empty());
    owner.reset().unwrap();
    assert_eq!(owner.resolve_instance_name_bytes(handle), Some(BYTES));
    owner.clear();
    assert_eq!(owner.resolve_instance_name_bytes(handle), None);
}

#[test]
fn explicit_byte_constructors_preserve_strict_encoding_modes() {
    let mut ascii = Engine::new(EngineConfig::ascii());
    let mut mixed = Engine::new(EngineConfig::ascii_symbols_utf8_strings());
    for engine in [&mut ascii, &mut mixed] {
        assert!(engine.symbol_value_bytes(BYTES).is_err());
        assert!(engine.instance_name_value_bytes(BYTES).is_err());
        assert!(engine.instance_name_value("é").is_err());
        assert!(engine.instance_name_value("ASCII").is_ok());
    }
    assert!(ascii.create_string_bytes(BYTES).is_err());
    let bytes = mixed.create_string_bytes(BYTES).unwrap();
    assert_eq!(bytes.as_bytes(), BYTES);
    assert!(matches!(
        ascii.assert_ordered("raw", bytes),
        Err(EngineError::InvalidHostValue(_))
    ));
}

const MATCHING_RULES: &str = r"
(defrule joined (key ?value) (row ?value)
 => (bind ?*joined* (+ ?*joined* 1)))
(defrule missing (key ?value) (not (row ?value))
 => (bind ?*missing* (+ ?*missing* 1)))
";
const SELECTED: &[u8] = b"key-23\xff";

fn matching_engine(late: bool) -> Engine {
    let mut engine = Engine::new(EngineConfig::utf8());
    load(&mut engine, "(defglobal ?*joined* = 0 ?*missing* = 0)");
    if !late {
        load(&mut engine, MATCHING_RULES);
    }
    engine.reset().unwrap();
    for index in 0..24 {
        let mut bytes = format!("key-{index}").into_bytes();
        bytes.push(0xff);
        let string: HostValue = engine.create_string_bytes(&bytes).unwrap().into();
        let symbol = engine.symbol_value_bytes(&bytes).unwrap();
        let name = engine.instance_name_value_bytes(&bytes).unwrap();
        for value in [string, symbol, name] {
            engine.assert_ordered("row", value.clone()).unwrap();
            if index == 23 {
                engine.assert_ordered("key", value).unwrap();
            }
        }
    }
    if late {
        load(&mut engine, MATCHING_RULES);
    }
    engine
}

fn retract_and_reassert_selected_row(engine: &mut Engine) {
    let handle = engine
        .find_facts("row")
        .unwrap()
        .into_iter()
        .find_map(|(handle, fact)| {
            let Fact::Ordered(fact) = fact else {
                return None;
            };
            matches!(&fact.fields[0], Value::String(string) if string.as_bytes() == SELECTED)
                .then_some(handle)
        })
        .unwrap();
    let saved = engine.get_fact_owned(handle).unwrap().unwrap();
    engine.retract(handle).unwrap();
    run(engine, 1);
    assert_eq!(global_integer(engine, "joined"), 3);
    assert_eq!(global_integer(engine, "missing"), 1);
    engine.assert(saved).unwrap();
    run(engine, 1);
    assert_eq!(global_integer(engine, "joined"), 4);
    assert_eq!(global_integer(engine, "missing"), 1);
    run(engine, 0);
}

#[test]
fn byte_atoms_match_through_indexed_joins_and_retraction_with_late_backfill() {
    for late in [false, true] {
        let mut engine = matching_engine(late);
        run(&mut engine, 3);
        assert_eq!(global_integer(&engine, "joined"), 3);
        assert_eq!(global_integer(&engine, "missing"), 0);
        retract_and_reassert_selected_row(&mut engine);
    }
}

#[cfg(feature = "serde")]
mod snapshots {
    use super::*;
    use ferric_rules::runtime::SerializationFormat;

    const FORMATS: [SerializationFormat; 5] = [
        SerializationFormat::Bincode,
        SerializationFormat::Json,
        SerializationFormat::Cbor,
        SerializationFormat::MessagePack,
        SerializationFormat::Postcard,
    ];

    fn restore(engine: &Engine, format: SerializationFormat) -> Engine {
        let bytes = engine
            .serialize(format)
            .unwrap_or_else(|error| panic!("{format:?}: {error}"));
        Engine::deserialize(&bytes, format).unwrap_or_else(|error| panic!("{format:?}: {error}"))
    }

    #[test]
    fn all_codecs_preserve_pending_matches_completed_refraction_and_retraction() {
        for format in FORMATS {
            for late in [false, true] {
                let original = matching_engine(late);
                let old_handle = original
                    .find_facts("key")
                    .unwrap()
                    .into_iter()
                    .find_map(|(handle, fact)| {
                        let Fact::Ordered(fact) = fact else {
                            return None;
                        };
                        matches!(fact.fields[0], Value::InstanceName(_)).then_some(handle)
                    })
                    .unwrap();
                let old_fact = original.get_fact_owned(old_handle).unwrap().unwrap();
                let old_value = old_fact.value(0).unwrap();
                let mut pending = restore(&original, format);
                assert!(matches!(
                    pending.assert(old_fact),
                    Err(EngineError::ForeignHandle)
                ));
                assert!(matches!(
                    pending.assert_ordered("foreign", old_value),
                    Err(EngineError::ForeignHandle)
                ));
                run(&mut pending, 3);
                assert_eq!(global_integer(&pending, "joined"), 3);
                assert_eq!(global_integer(&pending, "missing"), 0);
                let mut completed = restore(&pending, format);
                run(&mut completed, 0);
                retract_and_reassert_selected_row(&mut completed);
                run(&mut restore(&completed, format), 0);
            }
        }
    }

    #[test]
    fn all_codecs_preserve_invalid_output_bytes_before_and_after_firing() {
        for format in FORMATS {
            let original = byte_output_engine();
            let mut pending = restore(&original, format);
            run(&mut pending, 1);
            check_byte_output(&pending);
            let mut completed = restore(&pending, format);
            check_byte_output(&completed);
            run(&mut completed, 0);
        }
    }

    #[test]
    fn all_codecs_preserve_source_instance_literals_and_symbol_dispatch() {
        for format in FORMATS {
            let mut pending = restore(&instance_engine(), format);
            run(&mut pending, 1);
            check_instance_result(&pending);
            let mut completed = restore(&pending, format);
            check_instance_result(&completed);
            run(&mut completed, 0);
        }
    }

    #[test]
    fn all_codecs_preserve_missing_instance_lookup_failure_and_completed_state() {
        for format in FORMATS {
            for expression in ["(type [widget])", "(classify [widget])"] {
                let mut pending = restore(&missing_instance_engine(expression), format);
                run_missing_instance(&mut pending);
                let mut completed = restore(&pending, format);
                check_missing_instance_result(&completed);
                run(&mut completed, 0);
                assert_eq!(global_integer(&completed, "after"), 0);
            }
        }
    }
}

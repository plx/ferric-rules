use std::env;
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::ptr;

use ferric_rules::core::{
    ConflictResolutionStrategy, ExternalAddress, ExternalTypeId, Fact, Multifield, StringEncoding,
    Value,
};
use ferric_rules::runtime::{
    Engine, EngineConfig, EngineError, HaltReason, LoadError, RunLimit, SerializationFormat,
};
use serde_json::{json, Value as JsonValue};
use slotmap::Key;

const HIGH_ID_ITERATIONS: usize = 1_048_577;

fn root() -> Result<PathBuf, String> {
    env::var_os("FERRIC_BINDINGS_CONFORMANCE_ROOT")
        .map(PathBuf::from)
        .ok_or_else(|| "FERRIC_BINDINGS_CONFORMANCE_ROOT is not set".to_string())
}

fn fixture(name: &str) -> Result<String, String> {
    let path = root()?
        .join("tests")
        .join("bindings-conformance")
        .join("fixtures")
        .join(name);
    fs::read_to_string(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn load(engine: &mut Engine, source: &str) -> Result<(), String> {
    engine
        .load_str(source)
        .map(|_| ())
        .map_err(|errors| format!("load failed: {errors:?}"))
}

fn new_with_fixture(name: &str) -> Result<Engine, String> {
    Engine::with_rules(&fixture(name)?).map_err(|error| format!("init failed: {error}"))
}

fn halt_reason(reason: HaltReason) -> &'static str {
    match reason {
        HaltReason::AgendaEmpty => "agenda_empty",
        HaltReason::LimitReached => "limit_reached",
        HaltReason::HaltRequested => "halt_requested",
        HaltReason::ActionError => "action_error",
    }
}

fn normalize_value(value: &Value, engine: &Engine) -> JsonValue {
    match value {
        Value::Void => json!({"type": "void"}),
        Value::Integer(value) => json!({"type": "integer", "value": value.to_string()}),
        Value::Float(value) => json!({"type": "float", "value": value.to_string()}),
        Value::Symbol(symbol) => json!({
            "type": "symbol",
            "value": engine.resolve_symbol(*symbol).unwrap_or("<unknown>")
        }),
        Value::String(value) => json!({"type": "string", "value": value.as_str()}),
        Value::Multifield(values) => json!({
            "type": "multifield",
            "value": values
                .as_slice()
                .iter()
                .map(|value| normalize_value(value, engine))
                .collect::<Vec<_>>()
        }),
        Value::ExternalAddress(_) => json!({"type": "external_address"}),
    }
}

fn asserted_field(engine: &mut Engine, value: Value) -> Result<JsonValue, String> {
    let id = engine
        .assert_ordered("probe", vec![value])
        .map_err(|error| error.to_string())?;
    let fact = engine
        .get_fact(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "asserted fact disappeared".to_string())?;
    match fact {
        Fact::Ordered(fact) => fact
            .fields
            .first()
            .map(|value| normalize_value(value, engine))
            .ok_or_else(|| "asserted fact has no field".to_string()),
        Fact::Template(_) => Err("expected ordered fact".to_string()),
    }
}

fn value_case(case_id: &str) -> Result<JsonValue, String> {
    let mut engine = Engine::new(EngineConfig::default());
    match case_id {
        "value.void" => asserted_field(&mut engine, Value::Void),
        "value.integer.boundaries" => {
            let minimum = asserted_field(&mut engine, Value::Integer(i64::MIN))?;
            let maximum = asserted_field(&mut engine, Value::Integer(i64::MAX))?;
            Ok(json!({"minimum": minimum, "maximum": maximum}))
        }
        "value.float" => asserted_field(&mut engine, Value::Float(1.5)),
        "value.symbol.explicit" => {
            let symbol = engine
                .intern_symbol("red")
                .map_err(|error| error.to_string())?;
            asserted_field(&mut engine, Value::Symbol(symbol))
        }
        "value.string.explicit" | "value.string.plain-host" => {
            let value = engine
                .create_string("red")
                .map_err(|error| error.to_string())?;
            asserted_field(&mut engine, Value::String(value))
        }
        "value.multifield.nested" => {
            let blue = engine
                .intern_symbol("blue")
                .map_err(|error| error.to_string())?;
            let text = engine
                .create_string("text")
                .map_err(|error| error.to_string())?;
            let nested: Multifield = vec![Value::Integer(9)].into_iter().collect();
            let values: Multifield = vec![
                Value::Void,
                Value::Integer(7),
                Value::Float(2.5),
                Value::Symbol(blue),
                Value::String(text),
                Value::Multifield(Box::new(nested)),
            ]
            .into_iter()
            .collect();
            asserted_field(&mut engine, Value::Multifield(Box::new(values)))
        }
        "value.external-address" => {
            let external = Value::ExternalAddress(ExternalAddress {
                type_id: ExternalTypeId(7),
                pointer: ptr::null_mut::<c_void>(),
            });
            let accepted = engine.assert_ordered("probe", vec![external]).is_ok();
            Ok(json!({
                "host_representation": "opaque",
                "ingress": if accepted { "accepted" } else { "rejected" }
            }))
        }
        _ => Err(format!("unknown value case {case_id}")),
    }
}

fn configuration_default() -> JsonValue {
    let config = EngineConfig::default();
    let engine = Engine::new(config.clone());
    let unicode = engine
        .create_string("é")
        .map(|_| "accepted")
        .unwrap_or("rejected");
    let strategy = match config.strategy {
        ConflictResolutionStrategy::Depth => "depth",
        ConflictResolutionStrategy::Breadth => "breadth",
        ConflictResolutionStrategy::Lex => "lex",
        ConflictResolutionStrategy::Mea => "mea",
    };
    json!({
        "max_call_depth": config.max_call_depth,
        "strategy": strategy,
        "unicode": unicode
    })
}

fn configuration_custom() -> Result<JsonValue, String> {
    let mut config = EngineConfig::from(StringEncoding::Ascii);
    config.max_call_depth = 1;
    let mut engine = Engine::new(config);
    let ascii_unicode = if engine.create_string("é").is_err() {
        "rejected"
    } else {
        "accepted"
    };
    load(&mut engine, &fixture("custom-config.clp")?)?;
    engine.reset().map_err(|error| error.to_string())?;
    let run = engine
        .run(RunLimit::Unlimited)
        .map_err(|error| error.to_string())?;
    if run.halt_reason != HaltReason::ActionError {
        return Err("custom max_call_depth did not bound recursion".to_string());
    }
    Ok(json!({
        "ascii_unicode": ascii_unicode,
        "max_call_depth": "configurable",
        "strategy_count": 4
    }))
}

fn error_case(case_id: &str) -> Result<JsonValue, String> {
    let mut engine = Engine::new(EngineConfig::default());
    let family = match case_id {
        "error.parse" => match engine.load_str("(defrule incomplete") {
            Err(errors)
                if errors
                    .iter()
                    .any(|error| matches!(error, LoadError::Parse(_))) =>
            {
                "parse"
            }
            other => return Err(format!("unexpected parse probe result: {other:?}")),
        },
        "error.compile" => match engine.load_str("(defrule bad => (nonexistent-fn))") {
            Err(errors)
                if errors
                    .iter()
                    .any(|error| matches!(error, LoadError::Compile(_))) =>
            {
                "compile"
            }
            other => return Err(format!("unexpected compile probe result: {other:?}")),
        },
        "error.unsupported-construct" => match engine.load_str("(defclass Probe (is-a USER))") {
            Err(errors)
                if errors
                    .iter()
                    .any(|error| matches!(error, LoadError::UnsupportedForm { .. })) =>
            {
                "compile"
            }
            other => return Err(format!("unexpected unsupported probe result: {other:?}")),
        },
        "error.runtime" => {
            let id = engine
                .assert_ordered("stale", Vec::<Value>::new())
                .map_err(|error| error.to_string())?;
            engine.retract(id).map_err(|error| error.to_string())?;
            match engine.retract(id) {
                Err(EngineError::FactNotFound(_)) => "fact_not_found",
                other => return Err(format!("unexpected runtime probe result: {other:?}")),
            }
        }
        _ => return Err(format!("unknown error case {case_id}")),
    };
    Ok(json!({"family": family}))
}

fn fact_lifecycle() -> Result<JsonValue, String> {
    let mut engine = Engine::new(EngineConfig::default());
    load(&mut engine, &fixture("template.clp")?)?;
    engine.reset().map_err(|error| error.to_string())?;

    let ordered_id = engine
        .assert_ordered("ordered", vec![Value::Integer(7)])
        .map_err(|error| error.to_string())?;
    let ordered_snapshot = engine
        .get_fact(ordered_id)
        .map_err(|error| error.to_string())?
        .cloned()
        .ok_or_else(|| "ordered fact missing".to_string())?;
    engine
        .retract(ordered_id)
        .map_err(|error| error.to_string())?;

    let name = engine
        .create_string("Ada")
        .map_err(|error| error.to_string())?;
    let template_id = engine
        .assert_template("person", &["name"], vec![Value::String(name)])
        .map_err(|error| error.to_string())?;
    let template_snapshot = engine
        .get_fact(template_id)
        .map_err(|error| error.to_string())?
        .cloned()
        .ok_or_else(|| "template fact missing".to_string())?;
    engine
        .retract(template_id)
        .map_err(|error| error.to_string())?;

    let count = engine.facts().map_err(|error| error.to_string())?.count();
    Ok(json!({
        "count_after_retract": count,
        "ordered_snapshot_retained": matches!(ordered_snapshot, Fact::Ordered(_)),
        "template_snapshot_retained": matches!(template_snapshot, Fact::Template(_))
    }))
}

fn run_once(limit: RunLimit) -> Result<JsonValue, String> {
    let mut engine = new_with_fixture("run-limits.clp")?;
    let result = engine.run(limit).map_err(|error| error.to_string())?;
    Ok(json!({
        "fired": result.rules_fired,
        "reason": halt_reason(result.halt_reason)
    }))
}

fn execution_run_limits() -> Result<JsonValue, String> {
    Ok(json!({
        "zero": run_once(RunLimit::Count(0))?,
        "one": run_once(RunLimit::Count(1))?,
        "unlimited": run_once(RunLimit::Unlimited)?
    }))
}

fn execution_step() -> Result<JsonValue, String> {
    let mut engine = new_with_fixture("one-rule.clp")?;
    let first = engine
        .step()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "first step did not fire".to_string())?;
    let first_rule = engine.rule_name(first.rule_id).map(str::to_owned);
    let empty = engine.step().map_err(|error| error.to_string())?.is_none();
    Ok(json!({"first_rule": first_rule, "empty": empty}))
}

fn execution_fixture(name: &str) -> Result<(Engine, JsonValue), String> {
    let mut engine = new_with_fixture(name)?;
    let result = engine
        .run(RunLimit::Unlimited)
        .map_err(|error| error.to_string())?;
    let normalized = json!({
        "fired": result.rules_fired,
        "reason": halt_reason(result.halt_reason)
    });
    Ok((engine, normalized))
}

fn execution_diagnostic() -> Result<JsonValue, String> {
    let (engine, mut result) = execution_fixture("diagnostic.clp")?;
    result["diagnostic_count"] = json!(engine.action_diagnostics().len());
    Ok(result)
}

fn snapshot_roundtrip() -> Result<JsonValue, String> {
    let mut engine = new_with_fixture("snapshot.clp")?;
    engine
        .assert_ordered("seed", Vec::<Value>::new())
        .map_err(|error| error.to_string())?;
    let bytes = engine
        .serialize(SerializationFormat::Json)
        .map_err(|error| error.to_string())?;
    let mut restored = Engine::deserialize(&bytes, SerializationFormat::Json)
        .map_err(|error| error.to_string())?;
    let fact_count = restored.facts().map_err(|error| error.to_string())?.count();
    let run = restored
        .run(RunLimit::Unlimited)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "fact_count": fact_count,
        "format": "json",
        "rules_fired": run.rules_fired
    }))
}

fn embedded_nul() -> Result<JsonValue, String> {
    let mut engine = Engine::new(EngineConfig::default());
    let string = engine
        .create_string("a\0b")
        .map_err(|error| error.to_string())?;
    asserted_field(&mut engine, Value::String(string))
}

fn high_fact_id() -> Result<JsonValue, String> {
    let mut engine = Engine::new(EngineConfig::default());
    for _ in 0..HIGH_ID_ITERATIONS {
        let id = engine
            .assert_ordered("generation", Vec::<Value>::new())
            .map_err(|error| error.to_string())?;
        engine.retract(id).map_err(|error| error.to_string())?;
    }
    let id = engine
        .assert_ordered("generation", Vec::<Value>::new())
        .map_err(|error| error.to_string())?;
    let above_safe_integer = id.data().as_ffi() > 9_007_199_254_740_991;
    let roundtrip = above_safe_integer
        && engine
            .get_fact(id)
            .map_err(|error| error.to_string())?
            .is_some();
    Ok(json!({"roundtrip": roundtrip}))
}

fn run_case(case_id: &str) -> Result<JsonValue, String> {
    if case_id.starts_with("value.") {
        return value_case(case_id);
    }
    if case_id.starts_with("error.") {
        return error_case(case_id);
    }
    match case_id {
        "configuration.default" => Ok(configuration_default()),
        "configuration.custom" => configuration_custom(),
        "fact.lifecycle" => fact_lifecycle(),
        "execution.run-limits" => execution_run_limits(),
        "execution.step" => execution_step(),
        "execution.halt" => execution_fixture("halt.clp").map(|(_, result)| result),
        "execution.diagnostic" => execution_diagnostic(),
        "execution.batch-boundary-halt" => {
            execution_fixture("batch-boundary-halt.clp").map(|(_, result)| result)
        }
        "snapshot.json-roundtrip" => snapshot_roundtrip(),
        "lifecycle.close" => Ok(json!({
            "explicit": false,
            "idempotent": false,
            "post_close": "not_applicable"
        })),
        "robustness.embedded-nul" => embedded_nul(),
        "identifier.high-fact-id" => high_fact_id(),
        "count.run-result-width" => Ok(json!({
            "run_count_bits": usize::BITS,
            "run_limit_bits": usize::BITS
        })),
        _ => Err(format!("unknown case {case_id}")),
    }
}

fn main() {
    let result = (|| -> Result<(), String> {
        let case_path = env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .ok_or_else(|| "usage: rust-adapter CASE_IDS_PATH".to_string())?;
        let cases = fs::read_to_string(&case_path)
            .map_err(|error| format!("cannot read {}: {error}", case_path.display()))?;
        for case_id in cases.lines().filter(|line| !line.is_empty()) {
            let result = run_case(case_id)?;
            println!(
                "{}",
                serde_json::to_string(&json!({"case": case_id, "result": result}))
                    .map_err(|error| error.to_string())?
            );
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("rust conformance adapter: {error}");
        std::process::exit(1);
    }
}

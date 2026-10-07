//! Template attributes retain distinctions needed for validation and defaults.

use crate::{
    interpret_constructs, parse_sexprs, ActionExpr, Cardinality, Construct, DefaultValue, FileId,
    InterpretError, InterpreterConfig, LiteralKind, NumericBound, NumericRange, SlotDefinition,
    SlotValueType, TemplateConstruct,
};

fn interpret(source: &str) -> Result<TemplateConstruct, Vec<InterpretError>> {
    let parsed = parse_sexprs(source, FileId(7));
    assert!(parsed.errors.is_empty(), "{source}: {:?}", parsed.errors);
    let mut result = interpret_constructs(&parsed.exprs, &InterpreterConfig::default());
    if !result.errors.is_empty() {
        return Err(result.errors);
    }
    assert_eq!(result.constructs.len(), 1);
    let Construct::Template(template) = result.constructs.remove(0) else {
        panic!("expected template")
    };
    Ok(template)
}

fn slot(attributes: &str) -> SlotDefinition {
    interpret(&format!("(deftemplate p (slot x {attributes}))"))
        .unwrap()
        .slots
        .remove(0)
}

fn multislot(attributes: &str) -> SlotDefinition {
    interpret(&format!("(deftemplate p (multislot x {attributes}))"))
        .unwrap()
        .slots
        .remove(0)
}

fn rejected(attributes: &str, diagnostic: &str) {
    let errors = interpret(&format!("(deftemplate p (multislot x {attributes}))")).unwrap_err();
    assert!(
        errors
            .iter()
            .any(|error| error.to_string().contains(diagnostic)),
        "{attributes}: {errors:?}"
    );
}

#[test]
fn issue_repro_preserves_constraints_and_dynamic_expression() {
    let template = interpret(
        "(deftemplate light
        (slot color (allowed-symbols red green))
        (slot n (type INTEGER) (range 0 10))
        (multislot tags (cardinality 1 2))
        (slot id (default-dynamic (gensym*))))",
    )
    .unwrap();
    let colors = &template.slots[0].constraints.allowed_values;
    assert_eq!(colors.len(), 1);
    assert_eq!(colors[0].kind, SlotValueType::Symbol);
    assert!(matches!(&colors[0].values[0].value, LiteralKind::Symbol(value) if value == "red"));
    assert!(matches!(&colors[0].values[1].value, LiteralKind::Symbol(value) if value == "green"));
    assert_eq!(
        template.slots[1].constraints.range,
        Some(NumericRange {
            min: Some(NumericBound::Integer(0)),
            max: Some(NumericBound::Integer(10)),
        })
    );
    assert_eq!(
        template.slots[2].constraints.cardinality,
        Some(Cardinality {
            min: 1,
            max: Some(2)
        })
    );
    let Some(DefaultValue::Dynamic(expressions)) = &template.slots[3].default else {
        panic!("expected dynamic default")
    };
    assert!(matches!(&expressions[..], [ActionExpr::FunctionCall(call)] if call.name == "gensym*"));
}

#[test]
fn disjoint_categories_keep_canonical_kinds_and_source_value_order() {
    let sets = slot(
        r#"(allowed-floats 3.5 1.5) (allowed-strings "z" "a")
        (allowed-integers 7 2) (allowed-symbols last first)"#,
    )
    .constraints
    .allowed_values;
    assert_eq!(
        sets.iter().map(|set| set.kind).collect::<Vec<_>>(),
        vec![
            SlotValueType::Symbol,
            SlotValueType::String,
            SlotValueType::Integer,
            SlotValueType::Float,
        ]
    );
    assert!(matches!(&sets[0].values[0].value, LiteralKind::Symbol(value) if value == "last"));
    assert!(matches!(&sets[1].values[0].value, LiteralKind::String(value) if value == "z"));
    assert!(matches!(sets[2].values[0].value, LiteralKind::Integer(7)));
    assert!(
        matches!(sets[3].values[0].value, LiteralKind::Float(value) if value.to_bits() == 3.5_f64.to_bits())
    );
}

#[test]
fn composite_lists_keep_forbidden_empty_kinds_and_unrestricted_other_categories() {
    let numbers = slot("(allowed-numbers 1)").constraints.allowed_values;
    assert_eq!(numbers.len(), 2);
    assert_eq!(numbers[0].kind, SlotValueType::Integer);
    assert_eq!(numbers[0].values.len(), 1);
    assert_eq!(numbers[1].kind, SlotValueType::Float);
    assert!(numbers[1].values.is_empty());
    let lexemes = slot("(allowed-lexemes symbol)").constraints.allowed_values;
    assert_eq!(lexemes.len(), 2);
    assert_eq!(lexemes[1].kind, SlotValueType::String);
    assert!(lexemes[1].values.is_empty());
    let values = slot("(allowed-values [one])").constraints.allowed_values;
    assert_eq!(values.len(), 5);
    assert!(values[..4].iter().all(|set| set.values.is_empty()));
    assert_eq!(values[4].kind, SlotValueType::InstanceName);
    assert!(
        matches!(&values[4].values[0].value, LiteralKind::InstanceName(value) if value == "one")
    );
    assert!(!values.iter().any(|set| matches!(
        set.kind,
        SlotValueType::FactAddress | SlotValueType::ExternalAddress
    )));
}

#[test]
fn whitelist_numeric_kinds_and_signed_zero_bits_are_preserved() {
    let values = slot("(allowed-values 1 1.0 0.0 -0.0)")
        .constraints
        .allowed_values;
    assert!(matches!(values[2].values[0].value, LiteralKind::Integer(1)));
    let bits: Vec<_> = values[3]
        .values
        .iter()
        .map(|value| {
            let LiteralKind::Float(number) = value.value else {
                panic!("expected FLOAT")
            };
            number.to_bits()
        })
        .collect();
    assert_eq!(
        bits,
        [1.0_f64.to_bits(), 0.0_f64.to_bits(), (-0.0_f64).to_bits()]
    );
}

#[test]
fn variable_placeholders_clear_restrictions_but_keep_attribute_conflicts() {
    for attribute in [
        "allowed-symbols",
        "allowed-strings",
        "allowed-lexemes",
        "allowed-integers",
        "allowed-floats",
        "allowed-numbers",
        "allowed-values",
    ] {
        assert!(slot(&format!("(type INTEGER) ({attribute} ?VARIABLE)"))
            .constraints
            .allowed_values
            .is_empty());
        rejected(
            &format!("({attribute} ?VARIABLE) ({attribute} ?VARIABLE)"),
            "duplicate",
        );
    }
    let parsed =
        multislot("(type SYMBOL) (range ?VARIABLE ?VARIABLE) (cardinality ?VARIABLE ?VARIABLE)");
    assert_eq!(
        parsed.constraints.range,
        Some(NumericRange {
            min: None,
            max: None
        })
    );
    assert_eq!(
        parsed.constraints.cardinality,
        Some(Cardinality { min: 0, max: None })
    );
    rejected(
        "(allowed-values ?VARIABLE) (allowed-symbols x)",
        "CSTRNPSR3",
    );
    rejected(
        "(allowed-numbers ?VARIABLE) (range ?VARIABLE ?VARIABLE)",
        "CSTRNPSR3",
    );
}

#[test]
fn overlapping_facets_and_numeric_ranges_reject_in_either_order() {
    for (left, right) in [
        ("allowed-values a", "allowed-symbols a"),
        ("allowed-values a", "allowed-strings \"s\""),
        ("allowed-values a", "allowed-lexemes a"),
        ("allowed-values a", "allowed-integers 1"),
        ("allowed-values a", "allowed-floats 1.0"),
        ("allowed-values a", "allowed-numbers 1"),
        ("allowed-lexemes a", "allowed-symbols a"),
        ("allowed-lexemes a", "allowed-strings \"s\""),
        ("allowed-numbers 1", "allowed-integers 1"),
        ("allowed-numbers 1", "allowed-floats 1.0"),
        ("range 0 2", "allowed-values a"),
        ("range 0 2", "allowed-integers 1"),
        ("range 0 2", "allowed-floats 1.0"),
        ("range 0 2", "allowed-numbers 1"),
    ] {
        rejected(&format!("({left}) ({right})"), "CSTRNPSR3");
        rejected(&format!("({right}) ({left})"), "CSTRNPSR3");
    }
    assert!(slot("(allowed-symbols a) (range 0 2)")
        .constraints
        .range
        .is_some());
}

#[test]
fn declared_types_validate_facet_categories_and_endpoint_kinds() {
    for (types, attribute) in [
        ("INTEGER", "allowed-symbols a"),
        ("INTEGER", "allowed-numbers 1"),
        ("FLOAT", "allowed-numbers 1.0"),
        ("INTEGER", "allowed-values 1 a"),
        ("NUMBER", "allowed-values a"),
        ("FACT-ADDRESS", "allowed-values a"),
        ("EXTERNAL-ADDRESS", "allowed-values a"),
        ("FLOAT", "range 2 4"),
        ("INTEGER", "range 2.0 4.0"),
        ("INTEGER", "range 2.5 3.5"),
        ("SYMBOL", "range 1 3"),
    ] {
        rejected(&format!("(type {types}) ({attribute})"), "CSTRNPSR1");
        rejected(&format!("({attribute}) (type {types})"), "CSTRNPSR1");
    }
    for attributes in [
        "(type INTEGER) (allowed-values 1)",
        "(type NUMBER) (allowed-numbers 1 2.0)",
        "(type LEXEME) (allowed-lexemes a \"s\")",
        "(type INTEGER SYMBOL) (range 2 4)",
        "(type NUMBER) (range 2 4.0)",
        "(type INSTANCE-NAME) (allowed-values [one])",
    ] {
        slot(attributes);
    }
}

#[test]
fn malformed_or_unsupported_constraint_syntax_is_rejected() {
    for attributes in [
        "(allowed-symbols)",
        "(allowed-values)",
        "(allowed-symbols a ?VARIABLE)",
        "(allowed-symbols ?other)",
        "(allowed-symbols a 1)",
        "(allowed-strings symbol)",
        "(allowed-integers 1.0)",
        "(allowed-floats 1)",
        "(allowed-numbers symbol)",
        "(allowed-values (create$ a))",
        "(allowed-values ?*global*)",
        "(allowed-classes A)",
        "(allowed-instance-names [one])",
        "(range 0)",
        "(range 0 1 2)",
        "(range 2 1)",
        "(range 0 nope)",
        "(range ?unknown 1)",
        "(range 0 1) (range 0 1)",
        "(cardinality -1 2)",
        "(cardinality 0 -1)",
        "(cardinality 1.0 2)",
        "(cardinality 0)",
        "(cardinality 0 1 2)",
        "(cardinality 2 1)",
        "(cardinality 0 1) (cardinality 0 1)",
    ] {
        rejected(attributes, "");
    }
    assert!(interpret("(deftemplate p (slot x (cardinality 0 1)))").is_err());
    assert!(interpret("(deftemplate p (field x (cardinality 0 1)))").is_err());
}

#[test]
fn bounds_preserve_large_integers_and_unbounded_endpoints() {
    let parsed = multislot(
        "(range -9223372036854775808 9223372036854775807) (cardinality 0 9223372036854775807)",
    );
    assert_eq!(
        parsed.constraints.range,
        Some(NumericRange {
            min: Some(NumericBound::Integer(i64::MIN)),
            max: Some(NumericBound::Integer(i64::MAX))
        })
    );
    assert_eq!(
        parsed.constraints.cardinality,
        Some(Cardinality {
            min: 0,
            max: Some(u64::try_from(i64::MAX).unwrap())
        })
    );
    assert_eq!(
        slot("(range ?VARIABLE -5)").constraints.range,
        Some(NumericRange {
            min: None,
            max: Some(NumericBound::Integer(-5))
        })
    );
}

#[test]
fn literals_keep_legacy_variants_and_computed_defaults_keep_expression_trees() {
    assert!(matches!(
        slot("(default 7)").default,
        Some(DefaultValue::Value(_))
    ));
    assert!(matches!(
        slot("(default ?NONE)").default,
        Some(DefaultValue::None)
    ));
    assert!(matches!(
        slot("(default ?DERIVE)").default,
        Some(DefaultValue::Derive)
    ));
    let Some(DefaultValue::Values(values)) =
        multislot("(default 1 (create$ 2 (create$ 3)) 4)").default
    else {
        panic!("expected literal multifield default")
    };
    assert_eq!(values.len(), 4);
    let Some(DefaultValue::Expressions(expressions)) =
        multislot("(default 1 (create$ 2 (+ 3 4)) ?*g*)").default
    else {
        panic!("expected computed default")
    };
    assert_eq!(expressions.len(), 3);
    assert!(
        matches!(&expressions[1], ActionExpr::FunctionCall(call) if call.name == "create$" && call.args.len() == 2)
    );
    assert!(matches!(&expressions[2], ActionExpr::GlobalVariable(name, _) if name == "g"));
}

#[test]
fn dynamic_defaults_keep_literals_globals_and_lexical_loop_query_forms() {
    for source in [
        "(default-dynamic 7)",
        "(default-dynamic ?*later*)",
        "(default-dynamic (gensym*))",
        "(default-dynamic (loop-for-count (?i 1 1) do ?i))",
        "(default-dynamic (progn$ (?x (create$ 1)) ?x))",
        "(default-dynamic (do-for-fact ((?f p)) TRUE ?f:v))",
    ] {
        assert!(
            matches!(slot(source).default, Some(DefaultValue::Dynamic(_))),
            "{source}"
        );
    }
    let Some(DefaultValue::Dynamic(expressions)) =
        multislot("(default-dynamic 1 (create$ 2 3) ?*g*)").default
    else {
        panic!("expected dynamic expressions")
    };
    assert_eq!(expressions.len(), 3);
    assert!(
        matches!(multislot("(default-dynamic)").default, Some(DefaultValue::Dynamic(values)) if values.is_empty())
    );
    assert!(
        matches!(multislot("(default)").default, Some(DefaultValue::Values(values)) if values.is_empty())
    );
}

#[test]
fn conflicting_defaults_and_scalar_multifields_reject_before_evaluation() {
    for attributes in [
        "(default 1) (default 2)",
        "(default 1) (default-dynamic 2)",
        "(default-dynamic 1) (default 2)",
        "(default-dynamic 1) (default-dynamic 2)",
        "(default ?NONE a)",
        "(default a ?DERIVE)",
        "(default-dynamic ?NONE)",
        "(default-dynamic ?DERIVE)",
    ] {
        rejected(attributes, "default");
    }
    for attributes in [
        "(default)",
        "(default 1 2)",
        "(default (create$ 7))",
        "(default-dynamic)",
        "(default-dynamic 1 2)",
        "(default-dynamic (create$ 7))",
    ] {
        let errors = interpret(&format!("(deftemplate p (slot x {attributes}))")).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("DEFAULT1")),
            "{attributes}: {errors:?}"
        );
    }
}

#[test]
fn errors_keep_the_actual_attribute_source_location() {
    let errors =
        interpret("(deftemplate p\n (slot x\n  (type INTEGER)\n  (range 1.0 2.0)))").unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].to_string().contains("line 4, column 3"),
        "{:?}",
        errors[0]
    );
}

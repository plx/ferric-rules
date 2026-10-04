//! Rule declaration syntax; expression values are checked by the runtime.

use super::{interpret_action_expr_inner, ActionExpr, Atom, InterpretError, SExpr};

#[derive(Default)]
pub(super) struct Declarations {
    pub salience: i32,
    pub salience_expression: Option<ActionExpr>,
    pub auto_focus: bool,
}

pub(super) fn interpret(
    elements: &[SExpr],
    index: &mut usize,
) -> Result<Declarations, InterpretError> {
    let mut result = Declarations::default();
    let mut saw_salience = false;
    let mut saw_auto_focus = false;
    while let Some(declaration) = elements.get(*index).and_then(SExpr::as_list) {
        if declaration.first().and_then(SExpr::as_symbol) != Some("declare") {
            break;
        }
        for attribute in &declaration[1..] {
            let fields = attribute.as_list().ok_or_else(|| {
                InterpretError::expected("a salience or auto-focus declaration", attribute.span())
            })?;
            let name = fields.first().and_then(SExpr::as_symbol);
            match name {
                Some("salience") => {
                    if std::mem::replace(&mut saw_salience, true) {
                        return Err(InterpretError::expected(
                            "only one salience declaration per rule",
                            attribute.span(),
                        ));
                    }
                    if fields.len() != 2 {
                        return Err(InterpretError::expected(
                            "one salience expression",
                            attribute.span(),
                        ));
                    }
                    if let Some(Atom::Integer(value)) = fields[1].as_atom() {
                        if !(-10_000..=10_000).contains(value) {
                            return Err(InterpretError::expected(
                                "one salience integer in -10000..=10000",
                                attribute.span(),
                            ));
                        }
                        result.salience = i32::try_from(*value).expect("validated salience range");
                    } else {
                        result.salience_expression = Some(interpret_action_expr_inner(&fields[1])?);
                    }
                }
                Some("auto-focus") => {
                    if std::mem::replace(&mut saw_auto_focus, true) {
                        return Err(InterpretError::expected(
                            "only one auto-focus declaration per rule",
                            attribute.span(),
                        ));
                    }
                    result.auto_focus = match fields {
                        [_, value] if value.as_symbol() == Some("TRUE") => true,
                        [_, value] if value.as_symbol() == Some("FALSE") => false,
                        _ => {
                            return Err(InterpretError::expected(
                                "one literal TRUE or FALSE for auto-focus",
                                attribute.span(),
                            ))
                        }
                    };
                }
                _ => {
                    return Err(InterpretError::expected(
                        "a supported declaration: salience or auto-focus",
                        attribute.span(),
                    ))
                }
            }
        }
        *index += 1;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::stage2::{
        interpret_constructs, ActionExpr, Construct, InterpretResult, InterpreterConfig,
        LiteralKind, RuleConstruct,
    };
    use crate::{parse_sexprs, FileId};
    use proptest::prelude::*;

    fn parse(source: &str) -> InterpretResult {
        let parsed = parse_sexprs(source, FileId(7));
        assert!(parsed.errors.is_empty(), "{source}: {:?}", parsed.errors);
        interpret_constructs(&parsed.exprs, &InterpreterConfig::default())
    }

    fn rule(source: &str) -> RuleConstruct {
        let result = parse(source);
        assert!(result.errors.is_empty(), "{source}: {:?}", result.errors);
        assert_eq!(result.constructs.len(), 1);
        let Construct::Rule(rule) = result.constructs.into_iter().next().unwrap() else {
            panic!("expected rule");
        };
        rule
    }

    #[test]
    fn defaults_and_literal_salience_preserve_the_existing_ast_contract() {
        let default = rule("(defrule ordinary =>)");
        assert_eq!(default.salience, 0);
        assert!(default.salience_expression.is_none());
        assert!(!default.auto_focus);
        for value in [-10_000, -1, 0, 1, 10_000] {
            let parsed = rule(&format!("(defrule r (declare (salience {value})) =>)"));
            assert_eq!(parsed.salience, value);
            assert!(parsed.salience_expression.is_none());
        }
        for value in [i64::MIN, -10_001, 10_001, i64::MAX] {
            let parsed = parse(&format!("(defrule r (declare (salience {value})) =>)"));
            assert_eq!(parsed.errors.len(), 1);
            assert!(parsed.constructs.is_empty());
        }
    }

    #[test]
    fn salience_call_retains_nested_expression_and_source_span() {
        let expression = "(+ ?*base* (length$ (create$ a b)))";
        let source =
            format!("(defrule r\n (declare (salience {expression}) (auto-focus TRUE)) =>)");
        let parsed = rule(&source);
        assert_eq!(parsed.salience, 0);
        assert!(parsed.auto_focus);
        let Some(ActionExpr::FunctionCall(call)) = parsed.salience_expression else {
            panic!("expected retained call");
        };
        assert_eq!(call.name, "+");
        assert_eq!(call.args.len(), 2);
        assert!(matches!(&call.args[0], ActionExpr::GlobalVariable(name, _) if name == "base"));
        assert!(
            matches!(&call.args[1], ActionExpr::FunctionCall(nested) if nested.name == "length$")
        );
        assert_eq!(call.span.file_id, FileId(7));
        assert_eq!(call.span.start.line, 2);
        assert_eq!(
            &source[call.span.start.offset..call.span.end.offset],
            expression
        );
    }

    #[test]
    fn salience_noninteger_literals_and_variables_are_retained_for_runtime_validation() {
        for expression in [
            "1.5",
            "FALSE",
            "\"text\"",
            "[instance]",
            "?local",
            "$?fields",
            "?*global*",
        ] {
            let parsed = rule(&format!("(defrule r (declare (salience {expression})) =>)"));
            let retained = parsed.salience_expression.unwrap();
            match expression {
                "1.5" => assert!(
                    matches!(retained, ActionExpr::Literal(value) if matches!(value.value, LiteralKind::Float(_)))
                ),
                "FALSE" => assert!(
                    matches!(retained, ActionExpr::Literal(value) if matches!(value.value, LiteralKind::Symbol(_)))
                ),
                "\"text\"" => assert!(
                    matches!(retained, ActionExpr::Literal(value) if matches!(value.value, LiteralKind::String(_)))
                ),
                "[instance]" => assert!(
                    matches!(retained, ActionExpr::Literal(value) if matches!(value.value, LiteralKind::InstanceName(_)))
                ),
                "?local" => {
                    assert!(matches!(retained, ActionExpr::Variable(name, _) if name == "local"));
                }
                "$?fields" => {
                    assert!(
                        matches!(retained, ActionExpr::Variable(name, _) if name == "$?fields")
                    );
                }
                "?*global*" => assert!(
                    matches!(retained, ActionExpr::GlobalVariable(name, _) if name == "global")
                ),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn salience_control_forms_keep_their_typed_expression_structure() {
        let conditional = rule("(defrule r (declare (salience (if TRUE then 10 else 20))) =>)");
        assert!(matches!(
            conditional.salience_expression,
            Some(ActionExpr::If { .. })
        ));
        let loop_expression =
            rule("(defrule r (declare (salience (loop-for-count (?i 1 3) do (break)))) =>)");
        assert!(matches!(
            loop_expression.salience_expression,
            Some(ActionExpr::LoopForCount { .. })
        ));
        let local_sequence = rule("(defrule r (declare (salience (progn (bind ?x 5) ?x))) =>)");
        assert!(
            matches!(local_sequence.salience_expression, Some(ActionExpr::FunctionCall(call)) if call.name == "progn")
        );
    }

    #[test]
    fn declarations_combine_in_either_order_and_across_declare_forms() {
        for declarations in [
            "(declare (salience 7) (auto-focus TRUE))",
            "(declare (auto-focus TRUE) (salience 7))",
            "(declare (salience 7)) (declare (auto-focus TRUE))",
            "(declare (auto-focus TRUE)) (declare (salience 7))",
        ] {
            let parsed = rule(&format!(
                "(defrule r \"comment\" {declarations} (fact) => (printout t done))"
            ));
            assert_eq!(parsed.comment.as_deref(), Some("comment"));
            assert_eq!(parsed.salience, 7);
            assert!(parsed.auto_focus);
            assert_eq!(parsed.patterns.len(), 1);
            assert_eq!(parsed.actions.len(), 1);
        }
        let disabled = rule("(defrule r (declare (auto-focus FALSE)) =>)");
        assert!(!disabled.auto_focus);
        assert!(disabled.salience_expression.is_none());
    }

    #[test]
    fn duplicate_detection_is_independent_and_survives_zero_or_false_values() {
        for declarations in [
            "(declare (salience 0) (salience 1))",
            "(declare (salience 0)) (declare (salience (+ 1 2)))",
            "(declare (salience (+ 1 2))) (declare (salience 0))",
            "(declare (auto-focus FALSE) (auto-focus TRUE))",
            "(declare (auto-focus FALSE)) (declare (salience 0) (auto-focus FALSE))",
            "(declare (auto-focus TRUE) (salience 0)) (declare (salience 0))",
        ] {
            let parsed = parse(&format!(
                "(defrule bad {declarations} =>)\n(defrule good =>)"
            ));
            assert_eq!(parsed.errors.len(), 1, "{declarations}");
            assert!(parsed.errors[0].message.contains("only one"));
            assert_eq!(parsed.constructs.len(), 1);
            assert!(matches!(&parsed.constructs[0], Construct::Rule(rule) if rule.name == "good"));
        }
    }

    #[test]
    fn auto_focus_requires_one_exact_boolean_symbol() {
        for value in [
            "",
            "true",
            "false",
            "\"TRUE\"",
            "[TRUE]",
            "1",
            "?flag",
            "?*flag*",
            "(not FALSE)",
            "TRUE FALSE",
        ] {
            let parsed = parse(&format!("(defrule r (declare (auto-focus {value})) =>)"));
            assert_eq!(parsed.errors.len(), 1, "{value}");
            assert!(parsed.errors[0].message.contains("literal TRUE or FALSE"));
            assert!(parsed.constructs.is_empty());
        }
    }

    #[test]
    fn malformed_declarations_preserve_later_construct_recovery() {
        for declaration in [
            "(salience)",
            "(salience 1 2)",
            "(salience (if TRUE 1))",
            "(unknown TRUE)",
            "()",
            "salience",
        ] {
            let source = format!("(defrule bad (declare {declaration}) =>)\n(defrule good (declare (auto-focus TRUE)) =>)");
            let parsed = parse(&source);
            assert_eq!(parsed.errors.len(), 1, "{declaration}");
            assert_eq!(parsed.errors[0].span.file_id, FileId(7));
            assert_eq!(parsed.errors[0].span.start.line, 1);
            assert_eq!(parsed.constructs.len(), 1);
            assert!(
                matches!(&parsed.constructs[0], Construct::Rule(rule) if rule.name == "good" && rule.auto_focus)
            );
        }
    }

    proptest! {
        #[test]
        fn independent_declarations_preserve_values(
            salience in -10_000_i32..=10_000,
            auto_focus in any::<bool>(),
            focus_first in any::<bool>(),
        ) {
            let focus = if auto_focus { "TRUE" } else { "FALSE" };
            let declarations = if focus_first {
                format!("(declare (auto-focus {focus})) (declare (salience {salience}))")
            } else {
                format!("(declare (salience {salience})) (declare (auto-focus {focus}))")
            };
            let parsed = rule(&format!("(defrule r {declarations} =>)"));
            prop_assert_eq!(parsed.salience, salience);
            prop_assert_eq!(parsed.auto_focus, auto_focus);
            prop_assert!(parsed.salience_expression.is_none());
        }
    }
}

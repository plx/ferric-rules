//! Conditional-element operand sequences, including fact-address prefixes.

use super::{interpret_pattern, Atom, Connective, InterpretError, Pattern, SExpr, Span};

pub(super) fn interpret_sequence(
    mut expressions: &[SExpr],
    bindings_allowed: bool,
) -> Result<Vec<Pattern>, InterpretError> {
    let mut patterns = Vec::new();
    while let Some((first, remaining)) = expressions.split_first() {
        if let Some(Atom::SingleVar(variable)) = first.as_atom() {
            if !bindings_allowed {
                return Err(InterpretError::invalid(
                    "fact-address binding is not allowed inside 'not', 'exists', or 'forall'",
                    first.span(),
                ));
            }
            let arrow = remaining.first().ok_or_else(|| {
                InterpretError::missing("'<-' after fact-address variable", first.span())
            })?;
            if !matches!(arrow.as_atom(), Some(Atom::Connective(Connective::Assign))) {
                return Err(InterpretError::expected(
                    "'<-' after fact-address variable",
                    arrow.span(),
                ));
            }
            let target = remaining
                .get(1)
                .ok_or_else(|| InterpretError::missing("fact pattern after '<-'", arrow.span()))?;
            let pattern = interpret_pattern(target, bindings_allowed)?;
            if !matches!(pattern, Pattern::Ordered(_) | Pattern::Template(_)) {
                return Err(InterpretError::expected(
                    "ordered or template fact pattern after '<-'",
                    target.span(),
                ));
            }
            patterns.push(Pattern::Assigned {
                variable: variable.clone(),
                pattern: Box::new(pattern),
                span: Span::merge(first.span(), target.span()),
            });
            expressions = &remaining[2..];
        } else {
            if matches!(first.as_atom(), Some(Atom::Connective(Connective::Assign)))
                || remaining.first().is_some_and(|next| {
                    matches!(next.as_atom(), Some(Atom::Connective(Connective::Assign)))
                })
            {
                return Err(InterpretError::expected(
                    "single-field variable before '<-'",
                    first.span(),
                ));
            }
            patterns.push(interpret_pattern(first, bindings_allowed)?);
            expressions = remaining;
        }
    }
    Ok(patterns)
}

#[cfg(test)]
mod tests {
    use super::super::{interpret_constructs, Construct, InterpreterConfig};
    use super::*;
    use crate::{parse_sexprs, FileId};

    fn parse(source: &str) -> super::super::InterpretResult {
        let parsed = parse_sexprs(source, FileId(0));
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        interpret_constructs(&parsed.exprs, &InterpreterConfig::default())
    }

    #[test]
    fn nested_and_or_assignments_preserve_operands_and_full_source_spans() {
        let source =
            "(defrule r\n (or (and (ready) ?f <- (a ?x)) ?f <- (b (value ?x)))\n => (retract ?f))";
        let result = parse(source);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let Construct::Rule(rule) = &result.constructs[0] else {
            panic!("expected rule");
        };
        let Pattern::Or(branches, _) = &rule.patterns[0] else {
            panic!("expected OR");
        };
        assert_eq!(branches.len(), 2);
        let Pattern::And(first, _) = &branches[0] else {
            panic!("expected AND");
        };
        assert_eq!(first.len(), 2);
        assert!(matches!(&first[0], Pattern::Ordered(p) if p.relation == "ready"));
        for (assigned, text, template) in [
            (&first[1], "?f <- (a ?x)", false),
            (&branches[1], "?f <- (b (value ?x))", true),
        ] {
            let Pattern::Assigned {
                variable,
                pattern,
                span,
            } = assigned
            else {
                panic!("expected assigned fact");
            };
            assert_eq!(variable, "f");
            assert_eq!(span.start.line, 2);
            assert_eq!(&source[span.start.offset..span.end.offset], text);
            assert_eq!(matches!(pattern.as_ref(), Pattern::Template(_)), template);
        }
    }

    #[test]
    fn positive_groups_count_one_assignment_as_one_operand() {
        for head in ["and", "or", "logical"] {
            let result = parse(&format!("(defrule r ({head} ?f <- (a)) =>)"));
            assert!(result.errors.is_empty(), "{head}: {:?}", result.errors);
            let Construct::Rule(rule) = &result.constructs[0] else {
                panic!("rule")
            };
            let patterns = match &rule.patterns[0] {
                Pattern::And(patterns, _)
                | Pattern::Or(patterns, _)
                | Pattern::Logical(patterns, _) => patterns,
                pattern => panic!("unexpected {pattern:?}"),
            };
            assert!(
                matches!(patterns.as_slice(), [Pattern::Assigned { variable, .. }] if variable == "f")
            );
        }
    }

    #[test]
    fn malformed_assignment_prefixes_report_the_offending_source_token() {
        for (lhs, marker, message) in [
            ("?f", "?f", "'<-' after fact-address variable"),
            ("(and ?f (a))", "(a)", "'<-' after fact-address variable"),
            ("(and ?f <-)", "<-", "fact pattern after '<-'"),
            (
                "(or $?f <- (a) (b))",
                "$?f",
                "single-field variable before '<-'",
            ),
            ("(and <- (a))", "<-", "single-field variable before '<-'"),
            ("(and ?f <- (not (a)))", "(not", "fact pattern after '<-'"),
            ("?f <- (and (a) (b))", "(and", "fact pattern after '<-'"),
            ("?f <- (or (a) (b))", "(or", "fact pattern after '<-'"),
        ] {
            let source = format!("(defrule r\n  {lhs}\n =>)");
            let result = parse(&source);
            assert!(result.constructs.is_empty(), "{lhs}");
            assert_eq!(result.errors.len(), 1, "{lhs}");
            let error = &result.errors[0];
            assert!(error.message.contains(message), "{lhs}: {}", error.message);
            assert_eq!(error.span.start.line, 2, "{lhs}");
            assert_eq!(
                error.span.start.offset,
                source.find(marker).unwrap(),
                "{lhs}"
            );
        }
    }

    #[test]
    fn negative_quantifiers_reject_address_bindings_at_any_nested_depth() {
        for lhs in [
            "(not ?f <- (a))",
            "(not (and ?f <- (a) (b)))",
            "(exists ?f <- (a))",
            "(exists (or (a) (and ?f <- (b))))",
            "(forall ?f <- (a) (b))",
            "(forall (a) (and ?f <- (b)))",
        ] {
            let source = format!("(defrule r\n  {lhs}\n =>)");
            let result = parse(&source);
            assert_eq!(result.errors.len(), 1, "{lhs}");
            let error = &result.errors[0];
            assert!(
                error
                    .message
                    .contains("fact-address binding is not allowed"),
                "{lhs}: {error:?}"
            );
            assert_eq!(error.span.start.offset, source.find("?f").unwrap());
        }
    }
}

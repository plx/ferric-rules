//! CLIPS 6.30 rule specificity, computed from unoptimized source constraints.
//!
//! `RuleComplexity` counts generated comparisons rather than every source AST
//! node. In particular, ordinary predicate arguments and field-length checks do
//! not contribute. The reference stores the result in an unsigned 11-bit field.
//! This follows `RuleComplexity`/`ExpressionComplexity` in CLIPS 6.30 `rulepsr.c`,
//! including the primary-versus-secondary test placement done by `reorder.c`.

use std::collections::HashSet;

use ferric_rules_parser::{Constraint, Pattern, RuleConstruct, SExpr};

const COMPLEXITY_MASK: u16 = 0x07ff;

/// Return the specificity of one normalized, CE-OR-expanded rule variant.
///
/// Field disjunctions remain intact: each alternative contributes its tests.
/// Inspect the parsed constraints rather than indexed alpha/beta comparisons:
/// constant folding and predicate lowering can change the number of tests.
pub(crate) fn rule_complexity(rule: &RuleConstruct) -> u16 {
    sequence(&rule.patterns, &mut HashSet::new()).complexity
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Attachment {
    /// Initial tests and tests after a join-from-right retain a test CE.
    #[default]
    Standalone,
    /// Tests on positive patterns are included in their primary network test.
    Primary,
    /// CLIPS excludes a simple negative/exists join's secondary network test.
    Secondary,
    /// Consecutive standalone tests are combined into one test CE.
    Test,
}

#[derive(Clone, Copy, Default)]
struct Sequence {
    complexity: u16,
    /// Only zero, one, or multiple normalized CEs matter to wrapper reduction.
    elements: u8,
    attachment: Attachment,
    secondary_test: bool,
}

impl Sequence {
    fn add(&mut self, contribution: u16) {
        self.complexity = self.complexity.wrapping_add(contribution) & COMPLEXITY_MASK;
    }

    fn pattern(&mut self, contribution: u16, attachment: Attachment) {
        self.add(contribution);
        self.elements = self.elements.saturating_add(1).min(2);
        self.attachment = attachment;
        self.secondary_test = false;
    }

    fn test(&mut self, contribution: u16) {
        match self.attachment {
            Attachment::Secondary => {
                // Its tests are excluded from specificity, but their boundary
                // still prevents an enclosing NCC collapsing to a simple CE.
                if !self.secondary_test {
                    self.elements = self.elements.saturating_add(1).min(2);
                    self.secondary_test = true;
                }
            }
            Attachment::Primary | Attachment::Test => self.add(contribution),
            Attachment::Standalone => {
                self.pattern(contribution.wrapping_add(1), Attachment::Test);
            }
        }
    }

    fn quantified(&mut self, inner: Self) {
        if inner.elements == 1 && inner.attachment == Attachment::Test {
            // Negation/existence around only tests normalizes back into a test.
            self.test(inner.complexity.wrapping_sub(1) & COMPLEXITY_MASK);
        } else if inner.elements == 1
            && matches!(
                inner.attachment,
                Attachment::Primary | Attachment::Secondary
            )
        {
            self.pattern(inner.complexity, Attachment::Secondary);
        } else {
            self.pattern(inner.complexity, Attachment::Standalone);
        }
    }
}

fn sequence(patterns: &[Pattern], bound: &mut HashSet<String>) -> Sequence {
    let mut result = Sequence::default();
    for pattern in patterns {
        append_pattern(&mut result, pattern, bound);
    }
    result
}

fn append_pattern(result: &mut Sequence, pattern: &Pattern, bound: &mut HashSet<String>) {
    match pattern {
        Pattern::Ordered(pattern) => {
            let cost = constraint_list(&pattern.constraints, bound);
            result.pattern(cost.wrapping_add(1), Attachment::Primary);
        }
        Pattern::Template(pattern) => {
            let mut cost = 1_u16;
            for slot in &pattern.slot_constraints {
                cost = cost.wrapping_add(constraint_list(&slot.constraints, bound));
            }
            result.pattern(cost, Attachment::Primary);
        }
        Pattern::Assigned {
            variable, pattern, ..
        } => {
            bound.insert(variable.clone());
            append_pattern(result, pattern, bound);
        }
        Pattern::And(children, _) | Pattern::Logical(children, _) => {
            for child in children {
                append_pattern(result, child, bound);
            }
        }
        Pattern::Not(inner, _) => {
            let nested = sequence(std::slice::from_ref(inner.as_ref()), &mut bound.clone());
            result.quantified(nested);
        }
        Pattern::Exists(children, _) => {
            let nested = sequence(children, &mut bound.clone());
            result.quantified(nested);
        }
        Pattern::Forall(children, _) => {
            let mut local = bound.clone();
            let mut nested = Sequence::default();
            for child in children {
                if let Some(cost) = test_only_complexity(child) {
                    nested.test(cost);
                } else {
                    append_pattern(&mut nested, child, &mut local);
                }
            }
            result.quantified(nested);
        }
        Pattern::Test(expression, _) => result.test(expression_complexity(expression)),
        Pattern::Or(_, _) => unreachable!("CE disjunctions are expanded before rule compilation"),
    }
}

// Forall preserves its source operands while its fact-free consequent can be
// any boolean tree of tests. These boolean wrappers add no complexity themselves.
fn test_only_complexity(pattern: &Pattern) -> Option<u16> {
    match pattern {
        Pattern::Test(expression, _) => Some(expression_complexity(expression)),
        Pattern::Not(inner, _) => test_only_complexity(inner),
        Pattern::And(children, _) | Pattern::Or(children, _) | Pattern::Exists(children, _) => {
            children.iter().try_fold(0_u16, |cost, child| {
                Some(cost.wrapping_add(test_only_complexity(child)?) & COMPLEXITY_MASK)
            })
        }
        _ => None,
    }
}

fn constraint_list(constraints: &[Constraint], bound: &mut HashSet<String>) -> u16 {
    constraints.iter().fold(0_u16, |cost, constraint| {
        cost.wrapping_add(constraint_complexity(constraint, bound, true)) & COMPLEXITY_MASK
    })
}

fn constraint_complexity(
    constraint: &Constraint,
    bound: &mut HashSet<String>,
    can_bind: bool,
) -> u16 {
    match constraint {
        Constraint::Literal(_) | Constraint::ReturnValue(_, _) => 1,
        Constraint::Variable(name, _) | Constraint::MultiVariable(name, _) => {
            u16::from(!can_bind || !bound.insert(name.clone()))
        }
        Constraint::Wildcard(_) | Constraint::MultiWildcard(_) => 0,
        Constraint::Predicate(expression, _) => expression_complexity(expression),
        Constraint::Not(inner, _) => constraint_complexity(inner, bound, false),
        Constraint::And(children, _) => children.iter().fold(0_u16, |cost, child| {
            cost.wrapping_add(constraint_complexity(child, bound, can_bind)) & COMPLEXITY_MASK
        }),
        Constraint::Or(children, _) => children.iter().fold(0_u16, |cost, child| {
            cost.wrapping_add(constraint_complexity(child, bound, false)) & COMPLEXITY_MASK
        }),
    }
}

fn expression_complexity(expression: &SExpr) -> u16 {
    let Some(parts) = expression.as_list() else {
        return 0;
    };
    match parts.first().and_then(SExpr::as_symbol) {
        Some("and" | "or" | "not") => parts[1..].iter().fold(0_u16, |cost, child| {
            cost.wrapping_add(expression_complexity(child)) & COMPLEXITY_MASK
        }),
        Some(_) => 1,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_rules_parser::{
        interpret_constructs, parse_sexprs, Construct, FileId, InterpreterConfig,
    };

    // These scores come from CLIPS 6.30's RuleComplexity after CE reordering.
    // Ordering corpus cases additionally pin the observable LEX/MEA behavior.

    fn complexity(lhs: &str) -> u16 {
        let source = format!("(defrule example {lhs} =>)");
        let parsed = parse_sexprs(&source, FileId(0));
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        let interpreted = interpret_constructs(&parsed.exprs, &InterpreterConfig::default());
        assert!(interpreted.errors.is_empty(), "{:?}", interpreted.errors);
        let Construct::Rule(rule) = &interpreted.constructs[0] else {
            panic!("expected a rule");
        };
        rule_complexity(rule)
    }

    #[test]
    fn comparisons_count_but_bindings_and_multifield_lengths_do_not() {
        for (lhs, expected) in [
            ("", 0),
            ("(u ?)", 1),
            ("(u 3)", 2),
            ("(u ~1)", 2),
            ("(u 1|2|3)", 4),
            ("(u 3|3)", 3),
            ("(u 3&3)", 3),
            ("(v ?x ?x ?)", 2),
            ("(v $?x $?x $?)", 2),
            ("(v $?a $?b)", 1),
            ("(v ?x&?x ? ?)", 2),
            ("(v ?x&1|?x&2 ?x ?)", 5),
            ("(u ?x) (v ?x|1 ?x|1 ?)", 6),
            ("(u ?x) (v ~?x ~?x ~?x)", 5),
            ("(explicit (x ?))", 1),
            ("(explicit (x 3))", 2),
            ("(explicit (ys $?a $?b))", 1),
        ] {
            assert_eq!(complexity(lhs), expected, "{lhs}");
        }
    }

    #[test]
    fn predicates_count_outer_calls_and_boolean_combinations() {
        for (lhs, expected) in [
            ("(u ?x&:(> (+ ?x 1) 0))", 2),
            ("(u ?x&:(and (> ?x 0) (< ?x 10)))", 3),
            ("(u ?x&:(helper ?x))", 2),
            ("(u ?x&:(gen ?x))", 2),
            ("(u =(+ 1 2))", 2),
            ("(u ?x&:(> ?x 1)&:(> ?x 1))", 3),
            ("(u ?x) (test (> ?x 0))", 2),
            ("(u ?x) (test (> ?x 0)) (test (< ?x 10))", 3),
            ("(u ?x) (not (test (< ?x 0)))", 2),
        ] {
            assert_eq!(complexity(lhs), expected, "{lhs}");
        }
    }

    #[test]
    fn test_attachment_matches_primary_secondary_and_ncc_networks() {
        for (lhs, expected) in [
            ("(test (> 1 0))", 2),
            ("(test (> 1 0)) (u ?)", 3),
            ("(test (> 1 0)) (u ?) (test (> 2 0))", 4),
            ("(u ?x) (not (absent)) (test (> ?x 0))", 2),
            ("(u ?x) (exists (b ?)) (test (> ?x 0))", 2),
            ("(u ?x) (not (absent)) (test (> ?x 0)) (test (< ?x 5))", 2),
            ("(u ?x) (not (and (absent) (missing))) (test (> ?x 0))", 5),
            (
                "(u ?x) (not (and (absent) (missing))) (test (> ?x 0)) (test (< ?x 10))",
                6,
            ),
            ("(u ?x) (not (and (absent ?y) (test (> ?y 0))))", 3),
            (
                "(u ?x) (not (and (absent ?y) (missing) (test (> ?y 0))))",
                4,
            ),
            ("(not (and (not (u ?)) (test (> 1 0)))) (test (> 1 0))", 3),
            ("(exists (not (u ?)) (test (> 1 0))) (test (> 1 0))", 3),
        ] {
            assert_eq!(complexity(lhs), expected, "{lhs}");
        }
    }

    #[test]
    fn quantified_patterns_count_inner_tests_without_leaking_bindings() {
        for (lhs, expected) in [
            ("(not (absent)) (u ?)", 2),
            ("(exists (u ?)) (b ?)", 2),
            ("(not (and (absent) (missing))) (u ?)", 3),
            ("(exists (a ?) (b ?)) (u ?)", 3),
            ("(forall (missing) (absent)) (u ?)", 3),
            ("(forall (u ?x) (u ?x)) (b ?)", 4),
            ("(not (not (u ?))) (b ?)", 2),
            ("(u ?x) (not (and (a ?x) (b ?x)))", 5),
            ("(not (not (and (u ?x) (test (> ?x 0)))))", 2),
            ("(exists (test (> 1 0)) (u ?))", 3),
            ("(not (absent ?x)) (u ?x)", 2),
            ("(exists (a ?x)) (b ?x)", 2),
            ("(not (and (absent ?x) (missing ?x))) (u ?x)", 4),
            ("(forall (u ?x) (u ?x)) (b ?x)", 4),
            ("(u ?x) (forall (v ?x) (u ?x))", 5),
            ("(u ?x) (not (and (a ?y) (not (b ?y)))) (v ?y)", 5),
            ("?f <- (u ?x) (v ?f ?x)", 4),
            ("(forall (u ?x) (test (> ?x 0)))", 2),
            ("(forall (u ?x) (and (test (> ?x 0)) (test (< ?x 10))))", 3),
            ("(forall (u ?x) (or (test (> ?x 0)) (test (< ?x -1))))", 3),
            (
                "(forall (u ?x) (exists (and (test (> ?x 0)) (not (test (> ?x 10))))))",
                3,
            ),
            ("(forall (u ?x) (test (> ?x 0))) (test (> 1 0))", 2),
        ] {
            assert_eq!(complexity(lhs), expected, "{lhs}");
        }
    }

    #[test]
    fn complexity_wraps_like_the_reference_eleven_bit_field() {
        for (predicates, expected) in [(2046, 2047), (2047, 0), (2048, 1)] {
            let lhs = format!("(u ?x&:(and {}))", "(> ?x 0) ".repeat(predicates));
            assert_eq!(complexity(&lhs), expected);

            // Compilation may fold these duplicate comparisons, but its saved
            // score must retain the source specificity and the reference wrap.
            let mut engine = crate::Engine::new(crate::EngineConfig::utf8());
            engine
                .load_str(&format!("(defrule wide {lhs} =>)"))
                .unwrap();
            let info = engine.rule_info.iter().flatten().next().unwrap();
            assert_eq!(info.complexity, expected);
        }
    }

    #[test]
    fn compilation_preserves_specificity_before_constraint_optimization() {
        let mut engine = crate::Engine::new(crate::EngineConfig::utf8());
        engine
            .load_str(
                "(defrule duplicate (u 3|3) =>)
                 (defrule comparison (u ?x&:(> ?x 0)) =>)
                 (defrule partition (v $?a $?b) =>)
                 (defrule nested (u ?x&:(> (+ ?x 1) 0)) =>)",
            )
            .unwrap();
        let scores: Vec<_> = engine
            .rule_info
            .iter()
            .flatten()
            .map(|info| (info.name.as_str(), info.complexity))
            .collect();
        assert_eq!(
            scores,
            [
                ("duplicate", 3),
                ("comparison", 2),
                ("partition", 1),
                ("nested", 2),
            ]
        );
    }

    #[test]
    fn normalized_ce_variants_retain_individual_reference_scores() {
        for (lhs, expected) in [
            ("(or (u ?) (u 1&1))", vec![1, 3]),
            ("(or (not (test FALSE)) (test TRUE))", vec![1, 1]),
            ("(or (exists (test (> 1 0))) (u ?))", vec![2, 1]),
            ("(not (or (a) (b)))", vec![2]),
            ("(not (not (or (a) (b))))", vec![2]),
            ("(forall (u ?x) (test (> ?x 0)))", vec![2]),
            (
                "(forall (u ?x) (or (test (> ?x 0)) (test (< ?x -1))))",
                vec![3],
            ),
            ("(forall (u ?x) (test (> ?x 0))) (test (> 1 0))", vec![2]),
        ] {
            let mut engine = crate::Engine::new(crate::EngineConfig::utf8());
            engine
                .load_str(&format!("(defrule example {lhs} =>)"))
                .unwrap();
            let scores: Vec<_> = engine
                .rule_info
                .iter()
                .flatten()
                .map(|info| info.complexity)
                .collect();
            assert_eq!(scores, expected, "{lhs}");
        }
    }
}

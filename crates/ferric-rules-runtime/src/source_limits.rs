//! Bounded source expansion before cloning disjunction products.
//!
//! The estimate is deliberately conservative: independent choice counts are
//! multiplied even when some alternatives would share work. It is checked
//! before normalization and again before slot/CE expansion, since normalization
//! can duplicate constraints inside negative conjunctions.

use ferric_rules_parser::{Constraint, Pattern, RuleConstruct};

use crate::loader::LoadError;

pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOAD_PATTERN_NODES: usize = 1_048_576;
const MAX_LOAD_EXPANDED_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn check_source_size(bytes: usize) -> Result<(), LoadError> {
    if bytes > MAX_SOURCE_BYTES {
        return Err(LoadError::ResourceLimit {
            rule: "<source>".to_string(),
            resource: "source bytes",
            required: bytes,
            limit: MAX_SOURCE_BYTES,
            line: 1,
            column: 1,
        });
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct LoadBudget {
    nodes: usize,
    bytes: usize,
}

impl LoadBudget {
    fn charge(
        &mut self,
        rule: &RuleConstruct,
        nodes: usize,
        bytes: usize,
    ) -> Result<(), LoadError> {
        let nodes = self.nodes.saturating_add(nodes);
        let bytes = self.bytes.saturating_add(bytes);
        check(
            rule,
            "load expansion node work",
            nodes,
            MAX_LOAD_PATTERN_NODES,
        )?;
        check(
            rule,
            "load expansion byte work",
            bytes,
            MAX_LOAD_EXPANDED_BYTES,
        )?;
        self.nodes = nodes;
        self.bytes = bytes;
        Ok(())
    }
}

pub const MAX_RULE_ALTERNATIVES: usize = 256;
pub const MAX_EXPANDED_PATTERN_NODES: usize = 16_384;
pub const MAX_EXPANDED_RULE_BYTES: usize = 8 * 1024 * 1024;

enum Input<'a> {
    Pattern(&'a Pattern),
    Constraint(&'a Constraint),
}

pub(crate) fn check_expansion(
    rule: &RuleConstruct,
    budget: &mut LoadBudget,
) -> Result<(), LoadError> {
    let mut pending: Vec<_> = rule.patterns.iter().map(Input::Pattern).collect();
    let mut nodes = 0usize;
    let mut alternatives = 1usize;
    let source_bytes = rule.span.end.offset.saturating_sub(rule.span.start.offset);
    while let Some(input) = pending.pop() {
        nodes += 1;
        let factor = match input {
            Input::Pattern(pattern) => match pattern {
                Pattern::Ordered(pattern) => {
                    pending.extend(pattern.constraints.iter().map(Input::Constraint));
                    1
                }
                Pattern::Template(pattern) => {
                    pending.extend(
                        pattern
                            .slot_constraints
                            .iter()
                            .map(|slot| Input::Constraint(&slot.constraint)),
                    );
                    1
                }
                Pattern::Not(inner, _) | Pattern::Assigned { pattern: inner, .. } => {
                    pending.push(Input::Pattern(inner));
                    1
                }
                Pattern::And(children, _)
                | Pattern::Or(children, _)
                | Pattern::Logical(children, _)
                | Pattern::Exists(children, _)
                | Pattern::Forall(children, _) => {
                    pending.extend(children.iter().map(Input::Pattern));
                    if matches!(pattern, Pattern::Or(..)) {
                        children.len().max(1)
                    } else {
                        1
                    }
                }
                Pattern::Test(_, _) => 1,
            },
            Input::Constraint(constraint) => match constraint {
                Constraint::And(children, _) | Constraint::Or(children, _) => {
                    pending.extend(children.iter().map(Input::Constraint));
                    if matches!(constraint, Constraint::Or(..)) {
                        children.len().max(1)
                    } else {
                        1
                    }
                }
                Constraint::Not(inner, _) => {
                    pending.push(Input::Constraint(inner));
                    1
                }
                _ => 1,
            },
        };
        // Saturation is an error-bound sentinel, never an allocation length.
        alternatives = alternatives.saturating_mul(factor);
        check(
            rule,
            "disjunction alternatives estimate",
            alternatives,
            MAX_RULE_ALTERNATIVES,
        )?;
        check(
            rule,
            "expanded pattern nodes estimate",
            nodes.saturating_mul(alternatives),
            MAX_EXPANDED_PATTERN_NODES,
        )?;
        check(
            rule,
            "expanded source bytes estimate",
            source_bytes.saturating_mul(alternatives),
            MAX_EXPANDED_RULE_BYTES,
        )?;
    }
    // Empty-LHS rules still clone their RHS during normalization/installation.
    check(
        rule,
        "expanded source bytes estimate",
        source_bytes.saturating_mul(alternatives),
        MAX_EXPANDED_RULE_BYTES,
    )?;
    budget.charge(
        rule,
        nodes.saturating_mul(alternatives),
        source_bytes.saturating_mul(alternatives),
    )
}

fn check(
    rule: &RuleConstruct,
    resource: &'static str,
    required: usize,
    limit: usize,
) -> Result<(), LoadError> {
    if required > limit {
        return Err(LoadError::ResourceLimit {
            rule: rule.name.clone(),
            resource,
            required,
            limit,
            line: rule.span.start.line,
            column: rule.span.start.column,
        });
    }
    Ok(())
}

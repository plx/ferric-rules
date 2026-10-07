//! Owned fact-query selection shared by RHS actions and expression evaluation.

use std::collections::HashSet;
use std::sync::Arc;

use ferric_rules_core::{Fact, FactAddress, TemplateId};

use crate::evaluator::{CompactFactBinding, EvalError};
use crate::modules::ModuleId;
use crate::Engine;

fn invalid_query(message: impl Into<String>) -> EvalError {
    EvalError::TypeError {
        function: "fact-query".into(),
        expected: "valid query members and visible templates".into(),
        actual: message.into(),
        span: None,
    }
}

pub(crate) type QueryCandidate = Vec<(String, CompactFactBinding)>;

/// A live, iterative nested-loop cursor. Each level remembers chronology,
/// rather than an arena slot, so removal/reuse cannot revive an old member.
/// Outer members remain selected while their inner levels advance.
pub(crate) struct ActionQueryCursor {
    members: Vec<(String, TemplateId)>,
    after: Vec<Option<u64>>,
    current: Vec<Option<CompactFactBinding>>,
    retained: rustc_hash::FxHashMap<FactAddress, Arc<Fact>>,
    epoch: u64,
    level: usize,
    finished: bool,
}

impl ActionQueryCursor {
    pub(crate) fn new(
        bindings: &[(String, String)],
        engine: &mut Engine,
        current_module: ModuleId,
    ) -> Result<Self, EvalError> {
        if bindings.is_empty() {
            return Err(invalid_query("query requires at least one member"));
        }
        let resolver = crate::loader::TemplateResolver {
            template_local_ids: &engine.template_local_ids,
            template_modules: &engine.template_modules,
            module_registry: &engine.module_registry,
        };
        let mut names = HashSet::new();
        let mut members = Vec::with_capacity(bindings.len());
        for (name, template) in bindings {
            if !crate::evaluator::valid_query_member(name) || !names.insert(name) {
                return Err(invalid_query("invalid or duplicate query member"));
            }
            let template = resolver
                .resolve_query_reference(template, current_module)
                .map_err(invalid_query)?;
            members.push((name.clone(), template));
        }
        // Resolve every declaration before recognizing an empty product.
        let finished = members.iter().any(|(_, template)| {
            engine
                .fact_base
                .next_template_fact_after(*template, None)
                .is_none()
        });
        Ok(Self {
            after: vec![None; members.len()],
            current: vec![None; members.len()],
            members,
            retained: rustc_hash::FxHashMap::default(),
            epoch: engine.fact_epoch,
            level: 0,
            finished,
        })
    }

    pub(crate) fn next(
        &mut self,
        engine: &mut Engine,
        query_name: &str,
    ) -> Result<Option<QueryCandidate>, EvalError> {
        if self.epoch != engine.fact_epoch {
            self.finished = true;
        }
        while !self.finished {
            let next = engine
                .fact_base
                .next_template_fact_after(self.members[self.level].1, self.after[self.level]);
            // Charge traversal as well as predicates: mutation can empty an
            // inner set while many outer prefixes remain to be visited.
            if let Some((fact_id, timestamp)) = next {
                crate::evaluator::consume_action_loop_iteration(&engine.config, query_name, None)?;
                self.after[self.level] = Some(timestamp);
                let address = crate::fact_address::make_fact_address(
                    &engine.fact_base,
                    engine.initial_fact_id,
                    engine.fact_epoch,
                    engine.fact_index_starts_at_zero,
                    fact_id,
                )
                .expect("chronological index only yields live facts");
                let retained = self.retained.entry(address.clone()).or_insert_with(|| {
                    Arc::new(
                        engine
                            .fact_base
                            .get(fact_id)
                            .expect("chronological index only yields live facts")
                            .fact
                            .clone(),
                    )
                });
                self.current[self.level] =
                    Some(CompactFactBinding::retained(address, retained.clone()));
                if self.level + 1 == self.members.len() {
                    let candidate = self
                        .members
                        .iter()
                        .zip(&self.current)
                        .map(|((name, _), member)| {
                            (
                                name.clone(),
                                member.as_ref().expect("complete query tuple").clone(),
                            )
                        })
                        .collect();
                    return Ok(Some(candidate));
                }
                self.level += 1;
                self.after[self.level] = None;
            } else if self.level == 0 {
                self.finished = true;
            } else {
                self.current[self.level] = None;
                self.after[self.level] = None;
                self.level -= 1;
            }
        }
        Ok(None)
    }
}

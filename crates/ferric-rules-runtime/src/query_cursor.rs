//! Owned fact-query selection shared by RHS actions and expression evaluation.

use std::sync::Arc;

use ferric_rules_core::{Fact, FactAddress};

use crate::evaluator::{CompactFactBinding, EvalError};
use crate::query_targets::{QueryTarget, ResolvedQueryMember};
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
    members: Vec<ResolvedQueryMember>,
    target_index: Vec<usize>,
    target_epoch: Vec<u64>,
    after: Vec<Option<u64>>,
    current: Vec<Option<CompactFactBinding>>,
    retained: rustc_hash::FxHashMap<FactAddress, Arc<Fact>>,
    level: usize,
    finished: bool,
}

impl ActionQueryCursor {
    pub(crate) fn new(
        members: Vec<ResolvedQueryMember>,
        engine: &mut Engine,
    ) -> Result<Self, EvalError> {
        if members.is_empty() || members.iter().any(|member| member.targets.is_empty()) {
            return Err(invalid_query("query requires nonempty members and targets"));
        }
        // Restrictions were all resolved before checking whether the product is empty.
        let finished = members.iter().any(|member| {
            member
                .targets
                .iter()
                .all(|target| next_fact(engine, *target, None).is_none())
        });
        Ok(Self {
            after: vec![None; members.len()],
            target_index: vec![0; members.len()],
            target_epoch: vec![engine.fact_epoch; members.len()],
            current: vec![None; members.len()],
            members,
            retained: rustc_hash::FxHashMap::default(),
            level: 0,
            finished,
        })
    }

    pub(crate) fn next(
        &mut self,
        engine: &mut Engine,
        query_name: &str,
    ) -> Result<Option<QueryCandidate>, EvalError> {
        while !self.finished {
            let target = self.members[self.level].targets[self.target_index[self.level]];
            // Reset ends the old target's fact chain, but later alternatives
            // start against the new fact base. Selected outer payloads survive.
            let next = if self.target_epoch[self.level] == engine.fact_epoch {
                next_fact(engine, target, self.after[self.level])
            } else {
                None
            };
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
                        .map(|(binding, member)| {
                            (
                                binding.variable.clone(),
                                member.as_ref().expect("complete query tuple").clone(),
                            )
                        })
                        .collect();
                    return Ok(Some(candidate));
                }
                self.level += 1;
                self.after[self.level] = None;
                self.target_index[self.level] = 0;
                self.target_epoch[self.level] = engine.fact_epoch;
            } else if self.target_index[self.level] + 1 < self.members[self.level].targets.len() {
                self.target_index[self.level] += 1;
                self.after[self.level] = None;
                self.target_epoch[self.level] = engine.fact_epoch;
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

fn next_fact(
    engine: &mut Engine,
    target: QueryTarget,
    after: Option<u64>,
) -> Option<(ferric_rules_core::FactId, u64)> {
    match target {
        QueryTarget::Template(id) => engine.fact_base.next_template_fact_after(id, after),
        QueryTarget::Ordered(relation) => {
            engine.fact_base.next_relation_fact_after(relation, after)
        }
    }
}

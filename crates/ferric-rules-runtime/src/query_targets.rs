//! Query target resolution and lifetime shared by action and expression queries.

use std::collections::HashSet;

use ferric_rules_core::{Symbol, TemplateId, Value};

use crate::evaluator::{EvalError, RuntimeExpr, RuntimeQueryBinding, SourceSpan};
use crate::loader::TemplateLookupError;
use crate::modules::ModuleId;
use crate::Engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryTarget {
    Template(TemplateId),
    Ordered(Symbol),
}

pub(crate) struct ResolvedQueryMember {
    pub(crate) variable: String,
    pub(crate) targets: Vec<QueryTarget>,
}

impl Engine {
    /// Validate a source reference without inventing an implied template.
    pub(crate) fn query_reference(
        &self,
        name: &str,
        module: ModuleId,
    ) -> Result<Option<TemplateId>, String> {
        if name.contains("::") {
            return Err(format!(
                "qualified template `{name}` is unsupported in fact queries"
            ));
        }
        match self.template_resolver().resolve_id(name, module) {
            Ok(id) => Ok(Some(id)),
            Err(TemplateLookupError::Unknown) if self.has_implicit_template(name, module) => {
                Ok(None)
            }
            Err(_) => self
                .template_resolver()
                .resolve_reference(name, module)
                .map(Some),
        }
    }

    /// Resolve and retain each target immediately, before evaluating the next operand.
    pub(crate) fn retain_query_targets(
        &mut self,
        value: &Value,
        module: ModuleId,
        span: Option<&SourceSpan>,
    ) -> Result<Vec<QueryTarget>, EvalError> {
        let error = |actual: String| EvalError::TypeError {
            function: "fact-query".into(),
            expected: "[PRNTUTIL2] a nonempty symbol or multifield of template symbols".into(),
            actual,
            span: span.cloned(),
        };
        let values = match value {
            Value::Symbol(_) => std::slice::from_ref(value),
            Value::Multifield(values) if !values.is_empty() => values.as_slice(),
            _ => return Err(error(format!("invalid query restriction {value:?}"))),
        };
        let mut targets = Vec::with_capacity(values.len());
        for value in values {
            let Value::Symbol(symbol) = value else {
                return Err(error(format!("invalid query restriction {value:?}")));
            };
            let name = self
                .resolve_core_symbol(*symbol)
                .ok_or_else(|| error("unresolved query symbol".into()))?
                .to_owned();
            let target = match self
                .query_reference(&name, module)
                .map_err(|message| error(format!("[PRNTUTIL1] {message}")))?
            {
                Some(id) => QueryTarget::Template(id),
                None => QueryTarget::Ordered(*symbol),
            };
            self.active_query_targets.push(target);
            targets.push(target);
        }
        Ok(targets)
    }
}

pub(crate) fn prepare_query_members<E: From<EvalError>>(
    bindings: &[RuntimeQueryBinding],
    mut evaluate: impl FnMut(&RuntimeExpr, Option<&SourceSpan>) -> Result<Vec<QueryTarget>, E>,
) -> Result<Vec<ResolvedQueryMember>, E> {
    let mut names = HashSet::new();
    let mut members = Vec::with_capacity(bindings.len());
    for binding in bindings {
        if !crate::evaluator::valid_query_member(&binding.variable)
            || !names.insert(&binding.variable)
            || binding.restrictions.is_empty()
        {
            return Err(EvalError::TypeError {
                function: "fact-query".into(),
                expected: "distinct named members with nonempty restrictions".into(),
                actual: binding.variable.clone(),
                span: binding.span.clone(),
            }
            .into());
        }
        let mut targets = Vec::new();
        for expression in &binding.restrictions {
            targets.extend(evaluate(expression, binding.span.as_ref())?);
        }
        members.push(ResolvedQueryMember {
            variable: binding.variable.clone(),
            targets,
        });
    }
    Ok(members)
}

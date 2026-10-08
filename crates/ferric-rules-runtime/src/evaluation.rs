//! One-form host evaluation, separate from construct loading and dynamic `eval`.

use std::sync::Arc;

use ferric_rules_core::{BindingSet, Value, VarMap};
use ferric_rules_parser::{interpret_action_expr, parse_sexprs, FileId};

use crate::evaluator::{self, CallableLocals, EvalContext, EvalError};
use crate::{Engine, LoadError};

/// A source/preparation failure or a runtime failure from [`Engine::eval_str`].
#[derive(Debug, thiserror::Error)]
pub enum EvalStrError {
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Source(Vec<LoadError>),
    #[error(transparent)]
    Evaluation(#[from] EvalError),
}

impl From<LoadError> for EvalStrError {
    fn from(error: LoadError) -> Self {
        Self::Source(vec![error])
    }
}

impl Engine {
    /// Evaluate exactly one expression using fresh local bindings.
    ///
    /// Whitespace and comments may surround the expression. Local bindings last
    /// for this call only; globals and engine effects persist. Output and effects
    /// produced before a runtime error are retained. Construct definitions belong
    /// in [`Self::load_str`]. The language's `eval` builtin has its own first-form
    /// parsing and variable-scope policy.
    pub fn eval_str(&mut self, source: &str) -> Result<Value, EvalStrError> {
        crate::source_limits::check_source_size(source.len())?;
        let mut parsed = parse_sexprs(source, FileId(0));
        if !parsed.errors.is_empty() {
            return Err(EvalStrError::Source(
                parsed.errors.into_iter().map(LoadError::Parse).collect(),
            ));
        }
        if parsed.exprs.len() != 1 {
            let (line, column) = parsed.exprs.get(1).map_or((1, 1), |expression| {
                (expression.span().start.line, expression.span().start.column)
            });
            return Err(LoadError::Compile(format!(
                "expected exactly one expression at line {line}, column {column}; found {}",
                parsed.exprs.len()
            ))
            .into());
        }
        let expression =
            interpret_action_expr(&parsed.exprs.remove(0)).map_err(LoadError::Interpret)?;
        let current_module = self.module_registry.current_module();
        let expression = Arc::new(self.prepare_root_expression(&expression, current_module)?);
        let bindings = BindingSet::new();
        let var_map = VarMap::new();
        let mut locals = CallableLocals::default();
        // Like a CLIPS top-level command, the expression keeps the templates
        // and ordered relations it names in use while it runs: its own `build`
        // cannot redefine them (CSTRCPSR4) and `clear` refuses (CONSTRCT1).
        let result =
            self.with_active_expressions(current_module, [Arc::clone(&expression)], |engine| {
                evaluator::eval(
                    &mut EvalContext {
                        engine,
                        bindings: &bindings,
                        var_map: &var_map,
                        callable_locals: Some(&mut locals),
                        call_depth: 0,
                        expression_depth: 0,
                        current_module,
                        global_module: None,
                        method_chain: None,
                        compact_fact_bindings: None,
                        allow_engine_effects: true,
                    },
                    &expression,
                )
            });
        self.flush_expression_output();
        self.host.prune(&self.fact_base);
        result.map_err(EvalStrError::Evaluation)
    }
}

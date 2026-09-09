//! Source arity for read/readline, without treating fact/slot names as calls.

use super::{ActionExpr, Engine, FunctionCall, LoadError};
use crate::modules::ModuleId;

enum Source<'a> {
    Expression(&'a ActionExpr),
    Call(&'a FunctionCall),
}

impl Engine {
    pub(super) fn validate_queued_input_expression(
        &self,
        expression: &ActionExpr,
        module: ModuleId,
    ) -> Result<(), LoadError> {
        self.validate_queued_input_source(Source::Expression(expression), module)
    }

    pub(super) fn validate_queued_input_call(
        &self,
        call: &FunctionCall,
        module: ModuleId,
    ) -> Result<(), LoadError> {
        self.validate_queued_input_source(Source::Call(call), module)
    }

    #[allow(clippy::too_many_lines)]
    fn validate_queued_input_source(
        &self,
        root: Source<'_>,
        module: ModuleId,
    ) -> Result<(), LoadError> {
        let mut pending = vec![root];
        while let Some(source) = pending.pop() {
            let expression = match source {
                Source::Expression(expression) => expression,
                Source::Call(call) => {
                    if matches!(call.name.as_str(), "read" | "readline") && call.args.len() > 1 {
                        return Err(Self::compile_error_at(
                            &call.span,
                            &format!(
                                "{} expects 0 or 1 arguments, got {}",
                                call.name,
                                call.args.len()
                            ),
                        ));
                    }
                    match call.name.as_str() {
                        // Reuse the existing rule-action validator's grammar:
                        // relation/slot heads are data; only field values are
                        // expressions. Resolving the template selects which.
                        "assert" => {
                            for argument in &call.args {
                                if let ActionExpr::FunctionCall(fact) = argument {
                                    if self.resolve_template_id(&fact.name, module).is_ok() {
                                        for slot in &fact.args {
                                            if let ActionExpr::FunctionCall(pair) = slot {
                                                pending.extend(
                                                    pair.args.iter().map(Source::Expression),
                                                );
                                            } else {
                                                pending.push(Source::Expression(slot));
                                            }
                                        }
                                    } else {
                                        pending.extend(fact.args.iter().map(Source::Expression));
                                    }
                                } else {
                                    pending.push(Source::Expression(argument));
                                }
                            }
                        }
                        "modify" | "duplicate" => {
                            if let Some(target) = call.args.first() {
                                pending.push(Source::Expression(target));
                            }
                            for slot in call.args.iter().skip(1) {
                                if let ActionExpr::FunctionCall(pair) = slot {
                                    pending.extend(pair.args.iter().map(Source::Expression));
                                } else {
                                    pending.push(Source::Expression(slot));
                                }
                            }
                        }
                        _ => pending.extend(call.args.iter().map(Source::Expression)),
                    }
                    continue;
                }
            };
            match expression {
                ActionExpr::Literal(_)
                | ActionExpr::Variable(..)
                | ActionExpr::GlobalVariable(..) => {}
                ActionExpr::FunctionCall(call) => pending.push(Source::Call(call)),
                ActionExpr::If {
                    condition,
                    then_actions,
                    else_actions,
                    ..
                } => {
                    pending.push(Source::Expression(condition));
                    pending.extend(
                        then_actions
                            .iter()
                            .chain(else_actions)
                            .map(Source::Expression),
                    );
                }
                ActionExpr::While {
                    condition, body, ..
                } => {
                    pending.push(Source::Expression(condition));
                    pending.extend(body.iter().map(Source::Expression));
                }
                ActionExpr::LoopForCount {
                    start, end, body, ..
                } => {
                    pending.extend([Source::Expression(start), Source::Expression(end)]);
                    pending.extend(body.iter().map(Source::Expression));
                }
                ActionExpr::Progn {
                    list_expr, body, ..
                } => {
                    pending.push(Source::Expression(list_expr));
                    pending.extend(body.iter().map(Source::Expression));
                }
                ActionExpr::QueryAction { query, body, .. } => {
                    pending.push(Source::Expression(query));
                    pending.extend(body.iter().map(Source::Expression));
                }
                ActionExpr::Switch {
                    expr,
                    cases,
                    default,
                    ..
                } => {
                    pending.push(Source::Expression(expr));
                    for (condition, body) in cases {
                        pending.push(Source::Expression(condition));
                        pending.extend(body.iter().map(Source::Expression));
                    }
                    if let Some(body) = default {
                        pending.extend(body.iter().map(Source::Expression));
                    }
                }
            }
        }
        Ok(())
    }
}

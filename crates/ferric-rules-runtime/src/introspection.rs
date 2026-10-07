//! CLIPS construct lists and template slot metadata.

use ferric_rules_core::{Multifield, Value};
use ferric_rules_parser::{NumericBound, SlotType, SlotValueType};

use crate::evaluator::{eval_inner, EvalContext, EvalError, RuntimeExpr, SourceSpan};
use crate::modules::ModuleId;
use crate::templates::RegisteredTemplate;

pub(crate) fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "deftemplate-slot-names"
            | "deftemplate-slot-allowed-values"
            | "deftemplate-slot-types"
            | "deftemplate-slot-default-value"
            | "deftemplate-slot-defaultp"
            | "deftemplate-slot-existp"
            | "deftemplate-slot-multip"
            | "deftemplate-slot-singlep"
            | "deftemplate-slot-range"
            | "deftemplate-slot-cardinality"
            | "get-deftemplate-list"
            | "get-defglobal-list"
            | "get-defrule-list"
    )
}

fn symbol(ctx: &mut EvalContext<'_>, name: &str) -> Value {
    Value::Symbol(
        ctx.engine
            .symbol_table
            .intern_symbol(name, ctx.engine.config.string_encoding)
            .expect("introspection names have already passed engine encoding checks"),
    )
}

fn boolean(ctx: &mut EvalContext<'_>, value: bool) -> Value {
    symbol(ctx, if value { "TRUE" } else { "FALSE" })
}

fn fields(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Multifield(Box::new(values.into_iter().collect::<Multifield>()))
}

fn names(
    ctx: &mut EvalContext<'_>,
    values: impl IntoIterator<Item = String>,
) -> Result<Value, EvalError> {
    values
        .into_iter()
        .map(|name| {
            ctx.engine
                .symbol_table
                .intern_symbol(&name, ctx.engine.config.string_encoding)
                .map(Value::Symbol)
                .map_err(|error| EvalError::UnsupportedOperation {
                    operation: "construct introspection".to_owned(),
                    reason: error.to_string(),
                    span: None,
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(fields)
}

fn argument_symbol(
    ctx: &mut EvalContext<'_>,
    name: &str,
    expression: &RuntimeExpr,
    span: Option<&SourceSpan>,
) -> Result<String, EvalError> {
    let value = eval_inner(ctx, expression)?;
    let Value::Symbol(value) = value else {
        return Err(EvalError::TypeError {
            function: name.to_owned(),
            expected: "SYMBOL".to_owned(),
            actual: value.type_name().to_owned(),
            span: span.cloned(),
        });
    };
    Ok(ctx
        .engine
        .symbol_table
        .resolve_symbol_str(value)
        .unwrap_or("")
        .to_owned())
}

fn check_arity(
    name: &str,
    args: &[RuntimeExpr],
    expected: usize,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    if args.len() != expected {
        return Err(EvalError::ArityMismatch {
            name: name.to_owned(),
            expected: expected.to_string(),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    Ok(())
}

pub(crate) fn eval(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if name.starts_with("get-") {
        return construct_list(ctx, name, args, span);
    }
    let slot_names = name == "deftemplate-slot-names";
    check_arity(name, args, if slot_names { 1 } else { 2 }, span)?;
    let raw = argument_symbol(ctx, name, &args[0], span)?;
    // Construct introspection accepts an explicit owner even without imports;
    // ordinary calls and unqualified names retain their visibility rules.
    let template = if let Ok(crate::qualified_name::QualifiedName::Qualified { module, name }) =
        crate::qualified_name::parse_qualified_name(&raw)
    {
        ctx.engine
            .module_registry
            .get_by_name(&module)
            .and_then(|owner| {
                ctx.engine
                    .template_defs
                    .iter()
                    .find(|(id, template)| {
                        ctx.engine.template_modules.get(*id) == Some(&owner)
                            && template.name.rsplit("::").next() == Some(name.as_str())
                    })
                    .map(|(_, template)| template.clone())
            })
    } else {
        ctx.engine
            .resolve_template_id(&raw, ctx.current_module)
            .ok()
            .and_then(|id| ctx.engine.template_defs.get(id))
            .cloned()
    };
    if template.is_none() && !ctx.engine.has_implicit_template(&raw, ctx.current_module) {
        ctx.engine.globals.push_printout_event(
            "werror".to_owned(),
            format!("[PRNTUTIL1] Unable to find deftemplate {raw}.\n"),
        );
        return Ok(boolean(ctx, false));
    }
    // The built-in initial-fact is a deftemplate without slots in CLIPS, not
    // an implied relation with the single `implied` multislot.
    let implied = template.is_none()
        && !crate::qualified_name::parse_qualified_name(&raw)
            .is_ok_and(|parsed| parsed.local_name() == "initial-fact");
    if slot_names {
        return names(
            ctx,
            template.as_ref().map_or_else(
                || {
                    if implied {
                        vec!["implied".to_owned()]
                    } else {
                        Vec::new()
                    }
                },
                |template| template.slot_names.clone(),
            ),
        );
    }
    let slot = argument_symbol(ctx, name, &args[1], span)?;
    let index = template
        .as_ref()
        .and_then(|template| template.slot_index(&slot));
    let exists = index.is_some() || (implied && slot == "implied");
    if name == "deftemplate-slot-existp" {
        return Ok(boolean(ctx, exists));
    }
    if !exists {
        return Err(EvalError::UnsupportedOperation {
            operation: name.to_owned(),
            reason: format!(
                "[TMPLTDEF1] Invalid slot {slot} not defined in corresponding deftemplate {raw}."
            ),
            span: span.cloned(),
        });
    }
    if let Some(template) = template {
        slot_metadata(ctx, name, &template, index.expect("existing explicit slot"))
    } else {
        Ok(implied_metadata(ctx, name))
    }
}

fn numeric(value: NumericBound) -> Value {
    match value {
        NumericBound::Integer(value) => Value::Integer(value),
        NumericBound::Float(value) => Value::Float(value),
    }
}

fn type_names(ctx: &mut EvalContext<'_>, allowed: Option<&[SlotValueType]>) -> Value {
    const ORDER: [(Option<SlotValueType>, &str); 8] = [
        (Some(SlotValueType::Float), "FLOAT"),
        (Some(SlotValueType::Integer), "INTEGER"),
        (Some(SlotValueType::Symbol), "SYMBOL"),
        (Some(SlotValueType::String), "STRING"),
        (Some(SlotValueType::ExternalAddress), "EXTERNAL-ADDRESS"),
        (Some(SlotValueType::FactAddress), "FACT-ADDRESS"),
        (None, "INSTANCE-ADDRESS"),
        (Some(SlotValueType::InstanceName), "INSTANCE-NAME"),
    ];
    fields(
        ORDER
            .into_iter()
            .filter(|(kind, _)| {
                allowed.map_or(true, |allowed| {
                    kind.is_some_and(|kind| allowed.contains(&kind))
                })
            })
            .map(|(_, name)| symbol(ctx, name)),
    )
}

fn slot_metadata(
    ctx: &mut EvalContext<'_>,
    name: &str,
    template: &RegisteredTemplate,
    index: usize,
) -> Result<Value, EvalError> {
    let multiple = template.slot_types[index] == SlotType::Multi;
    let constraint = &template.constraints[index];
    Ok(match name {
        "deftemplate-slot-multip" => boolean(ctx, multiple),
        "deftemplate-slot-singlep" => boolean(ctx, !multiple),
        "deftemplate-slot-types" => type_names(ctx, template.allowed_types[index].as_deref()),
        "deftemplate-slot-allowed-values" => {
            if constraint.allowed_values.is_empty() {
                boolean(ctx, false)
            } else {
                fields(constraint.allowed_values_in_order.iter().cloned())
            }
        }
        "deftemplate-slot-defaultp" => {
            if template.requires_value(index) {
                boolean(ctx, false)
            } else {
                symbol(
                    ctx,
                    if template.dynamic_defaults[index].is_some() {
                        "dynamic"
                    } else {
                        "static"
                    },
                )
            }
        }
        "deftemplate-slot-default-value" => {
            if template.requires_value(index) {
                symbol(ctx, "?NONE")
            } else if let Some(default) = &template.dynamic_defaults[index] {
                // As in CLIPS, the query reports the evaluated expression:
                // slot shape and constraints are checked only on assertion.
                crate::template_defaults::evaluate_dynamic_raw(
                    ctx,
                    default,
                    template.slot_types[index],
                )?
            } else {
                template.defaults[index].clone()
            }
        }
        "deftemplate-slot-range" => {
            if template.allowed_types[index]
                .as_ref()
                .is_some_and(|allowed| {
                    !allowed.contains(&SlotValueType::Integer)
                        && !allowed.contains(&SlotValueType::Float)
                })
            {
                boolean(ctx, false)
            } else {
                let lower = constraint
                    .range
                    .and_then(|range| range.min)
                    .map_or_else(|| symbol(ctx, "-oo"), numeric);
                let upper = constraint
                    .range
                    .and_then(|range| range.max)
                    .map_or_else(|| symbol(ctx, "+oo"), numeric);
                fields([lower, upper])
            }
        }
        "deftemplate-slot-cardinality" => {
            if multiple {
                let minimum = constraint
                    .cardinality
                    .map_or(0, |cardinality| cardinality.min);
                let maximum = constraint
                    .cardinality
                    .and_then(|cardinality| cardinality.max)
                    .map_or_else(
                        || symbol(ctx, "+oo"),
                        |value| {
                            Value::Integer(i64::try_from(value).expect("validated cardinality"))
                        },
                    );
                fields([
                    Value::Integer(i64::try_from(minimum).expect("validated cardinality")),
                    maximum,
                ])
            } else {
                fields([])
            }
        }
        _ => unreachable!("unknown template metadata operation"),
    })
}

fn implied_metadata(ctx: &mut EvalContext<'_>, name: &str) -> Value {
    match name {
        "deftemplate-slot-multip" => boolean(ctx, true),
        "deftemplate-slot-singlep" | "deftemplate-slot-allowed-values" => boolean(ctx, false),
        "deftemplate-slot-types" => type_names(ctx, None),
        "deftemplate-slot-defaultp" => symbol(ctx, "static"),
        "deftemplate-slot-default-value" => fields([]),
        "deftemplate-slot-range" => {
            let low = symbol(ctx, "-oo");
            let high = symbol(ctx, "+oo");
            fields([low, high])
        }
        "deftemplate-slot-cardinality" => {
            let high = symbol(ctx, "+oo");
            fields([Value::Integer(0), high])
        }
        _ => unreachable!("unknown implied slot operation"),
    }
}

fn construct_list(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::ArityMismatch {
            name: name.to_owned(),
            expected: "0 or 1".to_owned(),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    let requested = args
        .first()
        .map(|expr| argument_symbol(ctx, name, expr, span))
        .transpose()?;
    let all = requested.as_deref() == Some("*");
    let module = if all {
        None
    } else if let Some(requested) = &requested {
        let Some(module) = ctx.engine.module_registry.get_by_name(requested) else {
            ctx.engine.globals.push_printout_event("werror".to_owned(),
                format!("[ARGACCES5] Function {name} expected argument #1 to be of type defmodule name\n"));
            return Ok(fields([]));
        };
        Some(module)
    } else {
        Some(ctx.current_module)
    };
    let mut entries = list_entries(ctx, name);
    if all {
        entries.sort_by_key(|(module, _)| module.0);
    }
    let names_to_return = entries
        .into_iter()
        .filter(|(owner, _)| module.map_or(true, |module| module == *owner))
        .map(|(owner, local)| {
            if all {
                format!(
                    "{}::{local}",
                    ctx.engine.module_registry.module_name(owner).unwrap_or("?")
                )
            } else {
                local
            }
        })
        .collect::<Vec<_>>();
    names(ctx, names_to_return)
}

fn list_entries(ctx: &EvalContext<'_>, name: &str) -> Vec<(ModuleId, String)> {
    match name {
        "get-deftemplate-list" => ctx.engine.template_declarations.clone(),
        "get-defglobal-list" => ctx
            .engine
            .registered_globals
            .iter()
            .map(|(module, name, _)| (*module, name.clone()))
            .collect(),
        "get-defrule-list" => ctx.engine.rule_declarations.clone(),
        _ => unreachable!("unknown construct list"),
    }
}

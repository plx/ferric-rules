//! Fact persistence shared by actions and expression evaluation.

use std::io::Write;

use ferric_rules_core::{Fact, Value};

use crate::evaluator::{self, EvalContext, EvalError, RuntimeExpr, SourceSpan};
use crate::modules::ModuleId;
use crate::Engine;

fn type_error(name: &str, value: &Value, expected: &str, span: Option<&SourceSpan>) -> EvalError {
    EvalError::TypeError {
        function: name.to_owned(),
        expected: expected.to_owned(),
        actual: value.type_name().to_owned(),
        span: span.cloned(),
    }
}

fn boolean(ctx: &mut EvalContext<'_>, value: bool) -> Value {
    if value {
        evaluator::clips_true(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        )
    } else {
        evaluator::clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        )
    }
}

fn notice(ctx: &mut EvalContext<'_>, message: String) -> Value {
    ctx.engine
        .globals
        .push_printout_event("werror".to_owned(), message);
    boolean(ctx, false)
}

pub(crate) fn eval_call(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if args.is_empty() || (name == "load-facts" && args.len() != 1) {
        return Err(EvalError::ArityMismatch {
            name: name.to_owned(),
            expected: if name == "load-facts" {
                "1"
            } else {
                "at least 1"
            }
            .to_owned(),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    let filename = evaluator::eval_inner(ctx, &args[0])?;
    let filename = match &filename {
        Value::String(value) => value.as_str().to_owned(),
        Value::Symbol(value) => ctx
            .engine
            .resolve_core_symbol(*value)
            .unwrap_or("???")
            .to_owned(),
        other => return Err(type_error(name, other, "SYMBOL or STRING filename", span)),
    };
    if name == "load-facts" {
        return load(ctx, &filename, span);
    }
    let mut visible = false;
    if let Some(mode) = args.get(1) {
        let mode = evaluator::eval_inner(ctx, mode)?;
        visible = match mode {
            Value::Symbol(symbol) => match ctx.engine.resolve_core_symbol(symbol) {
                Some("local") => false,
                Some("visible") => true,
                _ => return Ok(notice(ctx, "[ARGACCES5] Function save-facts expected argument #2 to be of type symbol with value local or visible\n".to_owned())),
            },
            // As in CLIPS, a mode that is not a SYMBOL stops the evaluation.
            other => return Err(type_error(name, &other, "SYMBOL mode", span)),
        };
    }
    let Ok(file) = std::fs::File::create(&filename) else {
        return Ok(notice(
            ctx,
            format!("[ARGACCES2] Function save-facts was unable to open file {filename}.\n"),
        ));
    };
    let mut selectors = Vec::new();
    for (index, argument) in args.iter().enumerate().skip(2) {
        let value = evaluator::eval_inner(ctx, argument)?;
        // As in CLIPS, each selector is checked in the module current once it
        // has evaluated, so one that runs a root `reset` resolves in MAIN.
        let module = crate::effects::dynamic_module(ctx);
        let Value::Symbol(symbol) = value else {
            // CLIPS reports a selector that is not a SYMBOL and continues.
            return Ok(notice(
                ctx,
                format!(
                    "[ARGACCES5] Function save-facts expected argument #{} to be of type symbol\n",
                    index + 1
                ),
            ));
        };
        let name = ctx
            .engine
            .resolve_core_symbol(symbol)
            .unwrap_or("???")
            .to_owned();
        let found = ctx
            .engine
            .template_declarations
            .iter()
            .any(|(owner, local)| {
                selector_matches(ctx.engine, &name, *owner, local)
                    && in_scope(ctx.engine, module, *owner, local, visible)
            });
        if !found {
            return Ok(notice(ctx, format!("[ARGACCES5] Function save-facts expected argument #{} to be of type {} deftemplate name\n", index + 1, if visible { "visible" } else { "local" })));
        }
        selectors.push(name);
    }
    let module = crate::effects::dynamic_module(ctx);
    match save(ctx.engine, module, file, visible, &selectors) {
        Ok(()) => Ok(boolean(ctx, true)),
        Err(_) => Ok(notice(
            ctx,
            format!("[ARGACCES2] Function save-facts was unable to open file {filename}.\n"),
        )),
    }
}

/// Load a fact file. As in CLIPS, a file that cannot be opened writes a notice
/// and returns FALSE, while a content or source-limit error stops the enclosing
/// evaluation after keeping the facts asserted before it.
fn load(
    ctx: &mut EvalContext<'_>,
    filename: &str,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let failure = |error: &dyn std::fmt::Display| EvalError::UnsupportedOperation {
        operation: "load-facts".to_owned(),
        reason: format!("{error}\nFunction load-facts encountered an error"),
        span: span.cloned(),
    };
    let source = match crate::source_limits::read_source_file(std::path::Path::new(filename)) {
        Ok(source) => source,
        Err(crate::LoadError::Io(_)) => {
            return Ok(notice(
                ctx,
                format!("[ARGACCES2] Function load-facts was unable to open file {filename}.\n"),
            ))
        }
        Err(error) => return Err(failure(&error)),
    };
    let module = ctx.engine.module_registry.current_module();
    let target = crate::effects::dynamic_module(ctx);
    ctx.engine.module_registry.set_current_module(target);
    let result = ctx.engine.load_facts_str(&source);
    ctx.engine.module_registry.set_current_module(module);
    match result {
        Ok(_) => Ok(boolean(ctx, true)),
        Err(error) => Err(failure(&error)),
    }
}

fn in_scope(engine: &Engine, from: ModuleId, owner: ModuleId, name: &str, visible: bool) -> bool {
    owner == from
        || (visible
            && engine
                .module_registry
                .is_construct_visible(from, owner, "deftemplate", name))
}

fn selector_matches(engine: &Engine, selector: &str, owner: ModuleId, local: &str) -> bool {
    match selector.split_once("::") {
        Some((module, name)) => {
            name == local && engine.module_registry.module_name(owner) == Some(module)
        }
        None => selector == local,
    }
}

fn save(
    engine: &Engine,
    module: ModuleId,
    file: std::fs::File,
    visible: bool,
    selectors: &[String],
) -> std::io::Result<()> {
    let mut entries: Vec<_> = engine.fact_base.iter().collect();
    entries.sort_by_key(|(_, entry)| entry.timestamp);
    let mut writer = std::io::BufWriter::new(file);
    for (_, entry) in entries {
        let (owner, local) = match &entry.fact {
            Fact::Template(fact) => {
                let template = &engine.template_defs[fact.template_id];
                (
                    engine
                        .template_modules
                        .get(fact.template_id)
                        .copied()
                        .unwrap_or(ModuleId(0)),
                    template.name.rsplit("::").next().unwrap_or(&template.name),
                )
            }
            Fact::Ordered(fact) => {
                let raw = engine.resolve_core_symbol(fact.relation).unwrap_or("???");
                let local = raw.rsplit("::").next().unwrap_or(raw);
                let owner = engine
                    .template_declarations
                    .iter()
                    .find(|(_, name)| name == local)
                    .map_or(ModuleId(0), |(owner, _)| *owner);
                (owner, local)
            }
        };
        if !in_scope(engine, module, owner, local, visible)
            || (!selectors.is_empty()
                && !selectors
                    .iter()
                    .any(|selector| selector_matches(engine, selector, owner, local)))
        {
            continue;
        }
        let line = engine
            .format_fact_for_save(&entry.fact)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        writeln!(writer, "{line}")?;
    }
    writer.flush()
}

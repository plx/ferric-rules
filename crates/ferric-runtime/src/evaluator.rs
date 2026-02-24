//! Expression evaluation for RHS actions and test CEs.
//!
//! Provides a shared evaluation pipeline that both right-hand-side action
//! arguments and `test` conditional element expressions pass through.
//!
//! Parser-level expression types (`ActionExpr`, `SExpr`) are first translated
//! into a normalized `RuntimeExpr`, which is then evaluated against a set of
//! variable bindings to produce a runtime `Value`.

use std::collections::VecDeque;

use ferric_core::binding::{BindingSet, VarMap};
use ferric_core::string::FerricString;
use ferric_core::symbol::SymbolTable;
use ferric_core::value::Value;
use ferric_core::StringEncoding;

use crate::config::EngineConfig;
use crate::functions::{FunctionEnv, GenericFunction, GenericRegistry, GlobalStore, UserFunction};
// Qualified name utilities: wired into dispatch chain in passes 003/004.
#[allow(unused_imports)]
use crate::qualified_name::{parse_qualified_name, QualifiedName};

// ---------------------------------------------------------------------------
// Source span for diagnostics
// ---------------------------------------------------------------------------

/// Source location for evaluation errors.
#[derive(Clone, Debug)]
pub struct SourceSpan {
    pub line: u32,
    pub column: u32,
}

impl std::fmt::Display for SourceSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}:{}", self.line, self.column)
    }
}

/// Format an optional source span for error messages.
fn format_span(span: Option<&SourceSpan>) -> String {
    span.map_or_else(|| "unknown location".to_string(), ToString::to_string)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors during expression evaluation.
#[derive(Clone, Debug, thiserror::Error)]
pub enum EvalError {
    #[error("unknown function `{name}` at {}", format_span(.span.as_ref()))]
    UnknownFunction {
        name: String,
        span: Option<SourceSpan>,
    },

    #[error("wrong number of arguments for `{name}`: expected {expected}, got {actual} at {}", format_span(.span.as_ref()))]
    ArityMismatch {
        name: String,
        expected: String,
        actual: usize,
        span: Option<SourceSpan>,
    },

    #[error("type error in `{function}`: expected {expected}, got {actual} at {}", format_span(.span.as_ref()))]
    TypeError {
        function: String,
        expected: String,
        actual: String,
        span: Option<SourceSpan>,
    },

    #[error("unbound variable `?{name}` at {}", format_span(.span.as_ref()))]
    UnboundVariable {
        name: String,
        span: Option<SourceSpan>,
    },

    #[error("unbound global variable `?*{name}*` at {}", format_span(.span.as_ref()))]
    UnboundGlobal {
        name: String,
        span: Option<SourceSpan>,
    },

    #[error("division by zero in `{function}` at {}", format_span(.span.as_ref()))]
    DivisionByZero {
        function: String,
        span: Option<SourceSpan>,
    },

    #[error("recursion limit exceeded for `{name}` (depth {depth}) at {}", format_span(.span.as_ref()))]
    RecursionLimit {
        name: String,
        depth: usize,
        span: Option<SourceSpan>,
    },

    #[error("no applicable method for `{name}` with argument types ({actual_types}) at {}", format_span(.span.as_ref()))]
    NoApplicableMethod {
        name: String,
        actual_types: String,
        span: Option<SourceSpan>,
    },

    #[error("not visible: `{name}` ({construct_type} defined in module `{owning_module}`) is not accessible from module `{from_module}` at {}", format_span(.span.as_ref()))]
    NotVisible {
        name: String,
        construct_type: String,
        from_module: String,
        owning_module: String,
        span: Option<SourceSpan>,
    },
}

// ---------------------------------------------------------------------------
// Loop safety cap
// ---------------------------------------------------------------------------

/// Maximum number of loop iterations before an error is raised.
///
/// Applies to `while` loops and `loop-for-count` ranges. Prevents infinite
/// loops and overly large iteration counts from hanging the engine.
const MAX_LOOP_ITERATIONS: usize = 1_000_000;

// ---------------------------------------------------------------------------
// Runtime expression model
// ---------------------------------------------------------------------------

/// A runtime expression for evaluation.
///
/// This is the normalized expression model consumed by the evaluator.
/// Both RHS `ActionExpr` and test CE `SExpr` are translated into this form.
#[derive(Clone, Debug)]
pub enum RuntimeExpr {
    /// A literal value (already resolved to a runtime Value).
    Literal(Value),
    /// A bound variable reference (resolved via `VarMap` at eval time).
    BoundVar {
        name: String,
        span: Option<SourceSpan>,
    },
    /// A global variable reference (e.g., `?*name*`).
    GlobalVar {
        name: String,
        span: Option<SourceSpan>,
    },
    /// A function call with evaluated arguments.
    Call {
        name: String,
        args: Vec<RuntimeExpr>,
        span: Option<SourceSpan>,
    },
    /// CLIPS `(if <condition> then <action>* [else <action>*])` form.
    ///
    /// Evaluates `condition`; if truthy evaluates `then_branch` in order and
    /// returns the last value, otherwise evaluates `else_branch`.
    /// Returns `Value::Void` when the selected branch is empty.
    ///
    /// Each branch entry pairs the original parser `ActionExpr` (needed by
    /// the action executor for assert/retract/printout arg handling) with its
    /// pre-compiled `RuntimeExpr` counterpart (used as the `runtime_call` hint
    /// in action dispatch and for pure expression evaluation).
    If {
        condition: Box<RuntimeExpr>,
        then_branch: Vec<(ferric_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        else_branch: Vec<(ferric_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
    /// CLIPS `(while <condition> do <action>*)` loop.
    ///
    /// Evaluates `condition` before each iteration; executes `body` while
    /// truthy. Returns the last body value from the last iteration, or the
    /// `FALSE` symbol if never entered.
    ///
    /// Body entries follow the same paired representation as `If` branches.
    While {
        condition: Box<RuntimeExpr>,
        body: Vec<(ferric_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
    /// CLIPS `(loop-for-count (?var start end) do <action>*)` loop.
    ///
    /// Iterates an integer counter from `start` to `end` (inclusive).
    /// If `var_name` is `Some`, the counter is bound under that name for
    /// each iteration. Returns `FALSE`.
    LoopForCount {
        var_name: Option<String>,
        start: Box<RuntimeExpr>,
        end: Box<RuntimeExpr>,
        body: Vec<(ferric_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
    /// CLIPS `(progn$ (?var <expr>) <action>*)` / `foreach` loop.
    ///
    /// Evaluates `list_expr` to a multifield value, then iterates each element
    /// binding `var_name` to the element and `<var_name>-index` to the 1-based
    /// index. Returns the last body value from the last iteration, or `FALSE`
    /// if the multifield is empty.
    Progn {
        var_name: String,
        list_expr: Box<RuntimeExpr>,
        body: Vec<(ferric_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
}

// ---------------------------------------------------------------------------
// Method dispatch chain
// ---------------------------------------------------------------------------

/// Active generic dispatch chain for `call-next-method` support.
///
/// When a generic method is executing, this tracks the ordered list of
/// applicable methods and the current position so `call-next-method` can
/// advance to the next one.
#[derive(Clone, Debug)]
pub struct MethodChain {
    /// Name of the generic function being dispatched.
    pub generic_name: String,
    /// Module where the generic is defined.
    pub generic_module: crate::modules::ModuleId,
    /// All applicable methods, sorted most-specific-first.
    pub applicable_methods: Vec<crate::functions::RegisteredMethod>,
    /// Index of the currently executing method in `applicable_methods`.
    pub current_index: usize,
    /// The evaluated argument values (used to rebind parameters in the next method).
    pub arg_values: Vec<Value>,
}

// ---------------------------------------------------------------------------
// Evaluation context
// ---------------------------------------------------------------------------

/// Context needed for expression evaluation.
pub struct EvalContext<'a> {
    pub bindings: &'a BindingSet,
    pub var_map: &'a VarMap,
    pub symbol_table: &'a mut SymbolTable,
    pub config: &'a EngineConfig,
    pub functions: &'a FunctionEnv,
    pub globals: &'a mut GlobalStore,
    /// Registry of generic functions for dispatch.
    pub generics: &'a GenericRegistry,
    /// Current call depth, for recursion limit enforcement.
    pub call_depth: usize,
    /// The module that the current evaluation is executing in.
    pub current_module: crate::modules::ModuleId,
    /// Module registry for visibility checks.
    pub module_registry: &'a crate::modules::ModuleRegistry,
    /// Function-to-module map for visibility checking.
    pub function_modules:
        &'a std::collections::HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    /// Global-to-module map for visibility checking.
    pub global_modules:
        &'a std::collections::HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    /// Generic-to-module map for visibility checking.
    pub generic_modules:
        &'a std::collections::HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>,
    /// Active method dispatch chain for `call-next-method` (None when not inside a generic method).
    pub method_chain: Option<MethodChain>,
    /// Input buffer for `read`/`readline`. `None` when no input source is connected.
    pub input_buffer: Option<&'a mut VecDeque<String>>,
}

fn module_label(ctx: &EvalContext<'_>, module_id: crate::modules::ModuleId) -> String {
    ctx.module_registry
        .module_name(module_id)
        .unwrap_or("?")
        .to_string()
}

fn sorted_dedup_modules(
    mut modules: Vec<crate::modules::ModuleId>,
) -> Vec<crate::modules::ModuleId> {
    modules.sort_by_key(|m| m.0);
    modules.dedup();
    modules
}

fn visible_modules_for_construct(
    ctx: &EvalContext<'_>,
    modules: &[crate::modules::ModuleId],
    construct_type: &str,
    local_name: &str,
) -> Vec<crate::modules::ModuleId> {
    sorted_dedup_modules(
        modules
            .iter()
            .copied()
            .filter(|module_id| {
                ctx.module_registry.is_construct_visible(
                    ctx.current_module,
                    *module_id,
                    construct_type,
                    local_name,
                )
            })
            .collect(),
    )
}

#[derive(Clone, Copy)]
struct AmbiguityMessages<'a> {
    expected: &'a str,
    actual: &'a str,
}

fn resolve_visible_owner_module(
    ctx: &EvalContext<'_>,
    all_modules: &[crate::modules::ModuleId],
    construct_type: &str,
    local_name: &str,
    display_name: &str,
    ambiguity: AmbiguityMessages<'_>,
    span: Option<SourceSpan>,
) -> Result<crate::modules::ModuleId, EvalError> {
    let visible = visible_modules_for_construct(ctx, all_modules, construct_type, local_name);
    match visible.as_slice() {
        [owner] => Ok(*owner),
        [] => Err(EvalError::NotVisible {
            name: display_name.to_string(),
            construct_type: construct_type.to_string(),
            from_module: module_label(ctx, ctx.current_module),
            owning_module: module_label(ctx, all_modules[0]),
            span,
        }),
        _ => Err(EvalError::TypeError {
            function: display_name.to_string(),
            expected: ambiguity.expected.to_string(),
            actual: ambiguity.actual.to_string(),
            span,
        }),
    }
}

fn resolve_unqualified_callable_module(
    ctx: &EvalContext<'_>,
    name: &str,
    construct_type: &str,
    modules_for_name: &[crate::modules::ModuleId],
    local_binding_exists: bool,
    ambiguity: AmbiguityMessages<'_>,
    span: Option<SourceSpan>,
) -> Result<Option<crate::modules::ModuleId>, EvalError> {
    if modules_for_name.is_empty() {
        return Ok(None);
    }
    if local_binding_exists {
        return Ok(Some(ctx.current_module));
    }
    let owner = resolve_visible_owner_module(
        ctx,
        modules_for_name,
        construct_type,
        name,
        name,
        ambiguity,
        span,
    )?;
    Ok(Some(owner))
}

// ---------------------------------------------------------------------------
// Main evaluation function
// ---------------------------------------------------------------------------

/// Evaluate a runtime expression to a `Value`.
#[allow(clippy::too_many_lines)] // The visibility checks add necessary verbosity
pub fn eval(ctx: &mut EvalContext<'_>, expr: &RuntimeExpr) -> Result<Value, EvalError> {
    match expr {
        RuntimeExpr::Literal(v) => Ok(v.clone()),
        RuntimeExpr::BoundVar { name, span } => {
            let sym = ctx
                .symbol_table
                .intern_symbol(name, ctx.config.string_encoding)
                .map_err(|_| EvalError::UnboundVariable {
                    name: name.clone(),
                    span: span.clone(),
                })?;
            let var_id = ctx
                .var_map
                .lookup(sym)
                .ok_or_else(|| EvalError::UnboundVariable {
                    name: name.clone(),
                    span: span.clone(),
                })?;
            ctx.bindings
                .get(var_id)
                .map(|v| (**v).clone())
                .ok_or_else(|| EvalError::UnboundVariable {
                    name: name.clone(),
                    span: span.clone(),
                })
        }
        RuntimeExpr::GlobalVar { name, span } => {
            // Module-qualified global references (MODULE::name) use the qualified path.
            if name.contains("::") {
                return resolve_qualified_global(ctx, name, span.clone());
            }
            if let Some(value) = ctx.globals.get(ctx.current_module, name).cloned() {
                return Ok(value);
            }

            let all_modules = sorted_dedup_modules(ctx.globals.modules_for_name(name));
            if all_modules.is_empty() {
                return Err(EvalError::UnboundGlobal {
                    name: name.clone(),
                    span: span.clone(),
                });
            }

            let owner = resolve_visible_owner_module(
                ctx,
                &all_modules,
                "defglobal",
                name,
                &format!("?*{name}*"),
                AmbiguityMessages {
                    expected: "unambiguous global resolution",
                    actual: "multiple visible globals; use MODULE::name",
                },
                span.clone(),
            )?;
            ctx.globals
                .get(owner, name)
                .cloned()
                .ok_or_else(|| EvalError::UnboundGlobal {
                    name: name.clone(),
                    span: span.clone(),
                })
        }
        RuntimeExpr::Call { name, args, span } => {
            // call-next-method: advance to next method in the dispatch chain.
            if name == "call-next-method" {
                return dispatch_call_next_method(ctx, args, span.clone());
            }
            // Module-qualified calls (MODULE::name) bypass the builtin dispatch
            // and go directly to the qualified resolution path.
            if name.contains("::") {
                return dispatch_qualified_call(ctx, name, args, span.clone());
            }
            match dispatch_builtin(ctx, name, args, span.clone()) {
                Ok(v) => Ok(v),
                Err(EvalError::UnknownFunction { .. }) => {
                    let function_modules =
                        sorted_dedup_modules(ctx.functions.modules_for_name(name));
                    if let Some(target_module) = resolve_unqualified_callable_module(
                        ctx,
                        name,
                        "deffunction",
                        &function_modules,
                        ctx.functions.contains(ctx.current_module, name),
                        AmbiguityMessages {
                            expected: "unambiguous deffunction resolution",
                            actual: "multiple visible deffunctions; use MODULE::name",
                        },
                        span.clone(),
                    )? {
                        if let Some(func) = ctx.functions.get(target_module, name).cloned() {
                            return dispatch_user_function(
                                ctx,
                                &func,
                                target_module,
                                args,
                                span.clone(),
                            );
                        }
                    }

                    let generic_modules = sorted_dedup_modules(ctx.generics.modules_for_name(name));
                    if let Some(target_module) = resolve_unqualified_callable_module(
                        ctx,
                        name,
                        "defgeneric",
                        &generic_modules,
                        ctx.generics.contains(ctx.current_module, name),
                        AmbiguityMessages {
                            expected: "unambiguous defgeneric resolution",
                            actual: "multiple visible defgenerics; use MODULE::name",
                        },
                        span.clone(),
                    )? {
                        if let Some(generic) = ctx.generics.get(target_module, name).cloned() {
                            return dispatch_generic(
                                ctx,
                                &generic,
                                target_module,
                                args,
                                span.clone(),
                            );
                        }
                    }

                    Err(EvalError::UnknownFunction {
                        name: name.clone(),
                        span: span.clone(),
                    })
                }
                Err(e) => Err(e),
            }
        }
        RuntimeExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let cond_value = eval(ctx, condition)?;
            let branch = if is_truthy(&cond_value, ctx.symbol_table) {
                then_branch
            } else {
                else_branch
            };
            let mut result = Value::Void;
            for (action_expr, rt_expr) in branch {
                if let Some(rt) = rt_expr {
                    result = eval(ctx, rt)?;
                } else {
                    // Pre-compilation failed; translate on the fly.
                    let rt = from_action_expr(action_expr, ctx.symbol_table, ctx.config)?;
                    result = eval(ctx, &rt)?;
                }
            }
            Ok(result)
        }
        RuntimeExpr::While {
            condition, body, ..
        } => {
            let mut result = clips_false(ctx.symbol_table, ctx.config.string_encoding);
            let mut iterations = 0usize;
            loop {
                let cond_value = eval(ctx, condition)?;
                if !is_truthy(&cond_value, ctx.symbol_table) {
                    break;
                }
                iterations += 1;
                if iterations > MAX_LOOP_ITERATIONS {
                    return Err(EvalError::TypeError {
                        function: "while".to_string(),
                        expected: "loop to terminate".to_string(),
                        actual: format!("exceeded maximum iterations ({MAX_LOOP_ITERATIONS})"),
                        span: None,
                    });
                }
                for (action_expr, rt_expr) in body {
                    if let Some(rt) = rt_expr {
                        result = eval(ctx, rt)?;
                    } else {
                        let rt = from_action_expr(action_expr, ctx.symbol_table, ctx.config)?;
                        result = eval(ctx, &rt)?;
                    }
                }
            }
            Ok(result)
        }
        RuntimeExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            span,
        } => {
            let start_val = eval(ctx, start)?;
            let end_val = eval(ctx, end)?;
            #[allow(clippy::cast_possible_truncation)] // intentional float-to-int for loop bounds
            let start_int = match &start_val {
                Value::Integer(n) => *n,
                Value::Float(f) => *f as i64,
                _ => {
                    return Err(EvalError::TypeError {
                        function: "loop-for-count".to_string(),
                        expected: "integer start value".to_string(),
                        actual: format!("{start_val:?}"),
                        span: span.clone(),
                    })
                }
            };
            #[allow(clippy::cast_possible_truncation)] // intentional float-to-int for loop bounds
            let end_int = match &end_val {
                Value::Integer(n) => *n,
                Value::Float(f) => *f as i64,
                _ => {
                    return Err(EvalError::TypeError {
                        function: "loop-for-count".to_string(),
                        expected: "integer end value".to_string(),
                        actual: format!("{end_val:?}"),
                        span: span.clone(),
                    })
                }
            };

            let false_val = clips_false(ctx.symbol_table, ctx.config.string_encoding);

            if start_int > end_int {
                return Ok(false_val);
            }

            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let total_iters = (end_int - start_int + 1) as usize;
            if total_iters > MAX_LOOP_ITERATIONS {
                return Err(EvalError::TypeError {
                    function: "loop-for-count".to_string(),
                    expected: "loop range within limits".to_string(),
                    actual: format!(
                        "range {start_int}..{end_int} exceeds maximum ({MAX_LOOP_ITERATIONS})"
                    ),
                    span: span.clone(),
                });
            }

            let mut result = clips_false(ctx.symbol_table, ctx.config.string_encoding);
            for counter in start_int..=end_int {
                // Build a binding frame for this iteration.
                let mut iter_var_map = ctx.var_map.clone();
                let mut iter_bindings = ctx.bindings.clone();

                if let Some(var) = var_name {
                    let sym = ctx
                        .symbol_table
                        .intern_symbol(var, ctx.config.string_encoding)
                        .map_err(|_| EvalError::TypeError {
                            function: "loop-for-count".to_string(),
                            expected: "valid loop variable name".to_string(),
                            actual: var.clone(),
                            span: span.clone(),
                        })?;
                    let var_id =
                        iter_var_map
                            .get_or_create(sym)
                            .map_err(|_| EvalError::TypeError {
                                function: "loop-for-count".to_string(),
                                expected: "bindable variable".to_string(),
                                actual: var.clone(),
                                span: span.clone(),
                            })?;
                    iter_bindings.set(var_id, std::rc::Rc::new(Value::Integer(counter)));
                }

                let mut iter_ctx = EvalContext {
                    bindings: &iter_bindings,
                    var_map: &iter_var_map,
                    symbol_table: ctx.symbol_table,
                    config: ctx.config,
                    functions: ctx.functions,
                    globals: ctx.globals,
                    generics: ctx.generics,
                    call_depth: ctx.call_depth,
                    current_module: ctx.current_module,
                    module_registry: ctx.module_registry,
                    function_modules: ctx.function_modules,
                    global_modules: ctx.global_modules,
                    generic_modules: ctx.generic_modules,
                    method_chain: ctx.method_chain.clone(),
                    input_buffer: ctx.input_buffer.as_deref_mut(),
                };

                for (action_expr, rt_expr) in body {
                    if let Some(rt) = rt_expr {
                        result = eval(&mut iter_ctx, rt)?;
                    } else {
                        let rt =
                            from_action_expr(action_expr, iter_ctx.symbol_table, iter_ctx.config)?;
                        result = eval(&mut iter_ctx, &rt)?;
                    }
                }
            }
            Ok(result)
        }
        RuntimeExpr::Progn {
            var_name,
            list_expr,
            body,
            span,
        } => {
            let list_val = eval(ctx, list_expr)?;
            let elements: Vec<Value> = match list_val {
                Value::Multifield(mf) => mf.as_slice().to_vec(),
                // A scalar value is treated as a single-element multifield.
                other => vec![other],
            };

            let false_val = clips_false(ctx.symbol_table, ctx.config.string_encoding);
            if elements.is_empty() {
                return Ok(false_val);
            }

            let index_var_name = format!("{var_name}-index");

            let mut result = clips_false(ctx.symbol_table, ctx.config.string_encoding);
            for (idx, element) in elements.iter().enumerate() {
                #[allow(clippy::cast_possible_wrap)] // usize→i64: element counts can't exceed i64
                let one_based = idx as i64 + 1;

                // Build a binding frame for this iteration.
                let mut iter_var_map = ctx.var_map.clone();
                let mut iter_bindings = ctx.bindings.clone();

                // Bind the element variable.
                let elem_sym = ctx
                    .symbol_table
                    .intern_symbol(var_name, ctx.config.string_encoding)
                    .map_err(|_| EvalError::TypeError {
                        function: "progn$".to_string(),
                        expected: "valid loop variable name".to_string(),
                        actual: var_name.clone(),
                        span: span.clone(),
                    })?;
                let elem_var_id =
                    iter_var_map
                        .get_or_create(elem_sym)
                        .map_err(|_| EvalError::TypeError {
                            function: "progn$".to_string(),
                            expected: "bindable variable".to_string(),
                            actual: var_name.clone(),
                            span: span.clone(),
                        })?;
                iter_bindings.set(elem_var_id, std::rc::Rc::new(element.clone()));

                // Bind the index variable (<var>-index).
                let idx_sym = ctx
                    .symbol_table
                    .intern_symbol(&index_var_name, ctx.config.string_encoding)
                    .map_err(|_| EvalError::TypeError {
                        function: "progn$".to_string(),
                        expected: "valid index variable name".to_string(),
                        actual: index_var_name.clone(),
                        span: span.clone(),
                    })?;
                let idx_var_id =
                    iter_var_map
                        .get_or_create(idx_sym)
                        .map_err(|_| EvalError::TypeError {
                            function: "progn$".to_string(),
                            expected: "bindable index variable".to_string(),
                            actual: index_var_name.clone(),
                            span: span.clone(),
                        })?;
                iter_bindings.set(idx_var_id, std::rc::Rc::new(Value::Integer(one_based)));

                let mut iter_ctx = EvalContext {
                    bindings: &iter_bindings,
                    var_map: &iter_var_map,
                    symbol_table: ctx.symbol_table,
                    config: ctx.config,
                    functions: ctx.functions,
                    globals: ctx.globals,
                    generics: ctx.generics,
                    call_depth: ctx.call_depth,
                    current_module: ctx.current_module,
                    module_registry: ctx.module_registry,
                    function_modules: ctx.function_modules,
                    global_modules: ctx.global_modules,
                    generic_modules: ctx.generic_modules,
                    method_chain: ctx.method_chain.clone(),
                    input_buffer: ctx.input_buffer.as_deref_mut(),
                };

                for (action_expr, rt_expr) in body {
                    if let Some(rt) = rt_expr {
                        result = eval(&mut iter_ctx, rt)?;
                    } else {
                        let rt =
                            from_action_expr(action_expr, iter_ctx.symbol_table, iter_ctx.config)?;
                        result = eval(&mut iter_ctx, &rt)?;
                    }
                }
            }
            Ok(result)
        }
    }
}

// ---------------------------------------------------------------------------
// Module-qualified dispatch helpers
// ---------------------------------------------------------------------------

/// Dispatch a module-qualified function call (`MODULE::name`).
///
/// Qualified calls never fall through to builtins — `MAIN::+` is always an
/// error, not a silent alias for the `+` builtin.
fn dispatch_qualified_call(
    ctx: &mut EvalContext<'_>,
    raw_name: &str,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let qualified = parse_qualified_name(raw_name).map_err(|msg| EvalError::TypeError {
        function: raw_name.to_string(),
        expected: "valid MODULE::name form".to_string(),
        actual: msg,
        span: span.clone(),
    })?;

    let (module_name, local_name) = match &qualified {
        QualifiedName::Qualified { module, name } => (module.as_str(), name.as_str()),
        QualifiedName::Unqualified(_) => {
            // Should not reach here since we checked for "::" in eval(), but
            // handle gracefully.
            return Err(EvalError::UnknownFunction {
                name: raw_name.to_string(),
                span,
            });
        }
    };

    // Verify the target module exists.
    let target_module_id = ctx
        .module_registry
        .get_by_name(module_name)
        .ok_or_else(|| EvalError::TypeError {
            function: raw_name.to_string(),
            expected: format!("existing module `{module_name}`"),
            actual: "unknown module".to_string(),
            span: span.clone(),
        })?;

    // Try user-defined function first.
    if let Some(func) = ctx.functions.get(target_module_id, local_name).cloned() {
        if !ctx.module_registry.is_construct_visible(
            ctx.current_module,
            target_module_id,
            "deffunction",
            local_name,
        ) {
            return Err(EvalError::NotVisible {
                name: raw_name.to_string(),
                construct_type: "deffunction".to_string(),
                from_module: module_label(ctx, ctx.current_module),
                owning_module: module_name.to_string(),
                span,
            });
        }
        return dispatch_user_function(ctx, &func, target_module_id, args, span);
    }

    // Try generic function.
    if let Some(generic) = ctx.generics.get(target_module_id, local_name).cloned() {
        if !ctx.module_registry.is_construct_visible(
            ctx.current_module,
            target_module_id,
            "defgeneric",
            local_name,
        ) {
            return Err(EvalError::NotVisible {
                name: raw_name.to_string(),
                construct_type: "defgeneric".to_string(),
                from_module: module_label(ctx, ctx.current_module),
                owning_module: module_name.to_string(),
                span,
            });
        }
        return dispatch_generic(ctx, &generic, target_module_id, args, span);
    }

    Err(EvalError::UnknownFunction {
        name: raw_name.to_string(),
        span,
    })
}

/// Resolve a module-qualified global variable reference (`MODULE::name`).
fn resolve_qualified_global(
    ctx: &mut EvalContext<'_>,
    raw_name: &str,
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let qualified = parse_qualified_name(raw_name).map_err(|msg| EvalError::TypeError {
        function: format!("?*{raw_name}*"),
        expected: "valid MODULE::name form".to_string(),
        actual: msg,
        span: span.clone(),
    })?;

    let (module_name, local_name) = match &qualified {
        QualifiedName::Qualified { module, name } => (module.as_str(), name.as_str()),
        QualifiedName::Unqualified(_) => {
            return Err(EvalError::UnboundGlobal {
                name: raw_name.to_string(),
                span,
            });
        }
    };

    // Verify the target module exists.
    let target_module_id = ctx
        .module_registry
        .get_by_name(module_name)
        .ok_or_else(|| EvalError::TypeError {
            function: format!("?*{raw_name}*"),
            expected: format!("existing module `{module_name}`"),
            actual: "unknown module".to_string(),
            span: span.clone(),
        })?;

    if !ctx.globals.contains(target_module_id, local_name) {
        return Err(EvalError::UnboundGlobal {
            name: raw_name.to_string(),
            span,
        });
    }

    if !ctx.module_registry.is_construct_visible(
        ctx.current_module,
        target_module_id,
        "defglobal",
        local_name,
    ) {
        return Err(EvalError::NotVisible {
            name: format!("?*{raw_name}*"),
            construct_type: "defglobal".to_string(),
            from_module: module_label(ctx, ctx.current_module),
            owning_module: module_name.to_string(),
            span,
        });
    }

    ctx.globals
        .get(target_module_id, local_name)
        .cloned()
        .ok_or_else(|| EvalError::UnboundGlobal {
            name: raw_name.to_string(),
            span,
        })
}

// ---------------------------------------------------------------------------
// User-defined function dispatch
// ---------------------------------------------------------------------------

fn execute_callable_body(
    ctx: &mut EvalContext<'_>,
    var_map: &VarMap,
    bindings: &BindingSet,
    body: &[ferric_parser::ActionExpr],
    current_module: crate::modules::ModuleId,
    method_chain: Option<MethodChain>,
) -> Result<Value, EvalError> {
    // Translate body expressions (ActionExpr → RuntimeExpr) BEFORE constructing
    // the inner EvalContext, because from_action_expr also needs &mut symbol_table.
    let mut body_exprs = Vec::with_capacity(body.len());
    for body_expr in body {
        body_exprs.push(from_action_expr(body_expr, ctx.symbol_table, ctx.config)?);
    }

    // Execute body expressions in an inner frame that inherits shared runtime state.
    let mut inner_ctx = EvalContext {
        bindings,
        var_map,
        symbol_table: ctx.symbol_table,
        config: ctx.config,
        functions: ctx.functions,
        globals: ctx.globals,
        generics: ctx.generics,
        call_depth: ctx.call_depth + 1,
        current_module,
        module_registry: ctx.module_registry,
        function_modules: ctx.function_modules,
        global_modules: ctx.global_modules,
        generic_modules: ctx.generic_modules,
        method_chain,
        input_buffer: ctx.input_buffer.as_deref_mut(),
    };

    let mut result = Value::Void;
    for body_expr in &body_exprs {
        result = eval(&mut inner_ctx, body_expr)?;
    }
    Ok(result)
}

/// Dispatch a call to a user-defined function.
#[allow(clippy::too_many_lines)]
fn dispatch_user_function(
    ctx: &mut EvalContext<'_>,
    func: &UserFunction,
    fn_module: crate::modules::ModuleId,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let span_ref = span.as_ref();

    // Check recursion limit before doing anything else.
    if ctx.call_depth >= ctx.config.max_call_depth {
        return Err(EvalError::RecursionLimit {
            name: func.name.clone(),
            depth: ctx.call_depth,
            span,
        });
    }

    // Evaluate all arguments in the caller's context.
    let arg_values = eval_args(ctx, args)?;

    // Check arity.
    let required = func.parameters.len();
    if func.wildcard_parameter.is_none() {
        if arg_values.len() != required {
            return Err(EvalError::ArityMismatch {
                name: func.name.clone(),
                expected: required.to_string(),
                actual: arg_values.len(),
                span,
            });
        }
    } else if arg_values.len() < required {
        return Err(EvalError::ArityMismatch {
            name: func.name.clone(),
            expected: format!("{required}+"),
            actual: arg_values.len(),
            span,
        });
    }

    // Build a fresh binding frame for the function call.
    let (fn_var_map, fn_bindings) = bind_callable_arguments(
        ctx,
        &func.name,
        &func.parameters,
        func.wildcard_parameter.as_deref(),
        &arg_values,
        span_ref,
    )?;
    // Execute in the function's definition module so visibility is checked from
    // the function's definition site.
    execute_callable_body(ctx, &fn_var_map, &fn_bindings, &func.body, fn_module, None)
}

// ---------------------------------------------------------------------------
// Generic function dispatch
// ---------------------------------------------------------------------------

/// Check if a runtime value matches a CLIPS type restriction name.
fn value_matches_type(value: &Value, type_name: &str) -> bool {
    match type_name {
        "INTEGER" => matches!(value, Value::Integer(_)),
        "FLOAT" => matches!(value, Value::Float(_)),
        "NUMBER" => matches!(value, Value::Integer(_) | Value::Float(_)),
        "SYMBOL" => matches!(value, Value::Symbol(_)),
        "STRING" => matches!(value, Value::String(_)),
        "LEXEME" => matches!(value, Value::Symbol(_) | Value::String(_)),
        "MULTIFIELD" => matches!(value, Value::Multifield(_)),
        "EXTERNAL-ADDRESS" => matches!(value, Value::ExternalAddress(_)),
        _ => false,
    }
}

/// Get the CLIPS type name for a runtime value (for `NoApplicableMethod` errors).
fn generic_value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) => "INTEGER",
        Value::Float(_) => "FLOAT",
        Value::Symbol(_) => "SYMBOL",
        Value::String(_) => "STRING",
        Value::Multifield(_) => "MULTIFIELD",
        Value::ExternalAddress(_) => "EXTERNAL-ADDRESS",
        Value::Void => "VOID",
    }
}

/// Count how many distinct concrete types a restriction set covers.
///
/// Returns `usize::MAX` when the restriction list is empty (no restriction = matches
/// everything = least specific). Otherwise, counts the distinct concrete leaf types
/// implied by the given type names, using the CLIPS type hierarchy:
/// - `NUMBER` expands to `INTEGER` + `FLOAT`
/// - `LEXEME` expands to `SYMBOL` + `STRING`
fn restriction_concrete_type_count(restrictions: &[String]) -> usize {
    if restrictions.is_empty() {
        return usize::MAX; // No restriction = matches everything, least specific
    }
    let mut count = 0usize;
    // Tracks whether each concrete type has been counted:
    // 0=INTEGER, 1=FLOAT, 2=SYMBOL, 3=STRING, 4=MULTIFIELD, 5=EXTERNAL-ADDRESS
    let mut seen = [false; 6];
    for t in restrictions {
        match t.as_str() {
            "INTEGER" => {
                if !seen[0] {
                    seen[0] = true;
                    count += 1;
                }
            }
            "FLOAT" => {
                if !seen[1] {
                    seen[1] = true;
                    count += 1;
                }
            }
            "NUMBER" => {
                if !seen[0] {
                    seen[0] = true;
                    count += 1;
                }
                if !seen[1] {
                    seen[1] = true;
                    count += 1;
                }
            }
            "SYMBOL" => {
                if !seen[2] {
                    seen[2] = true;
                    count += 1;
                }
            }
            "STRING" => {
                if !seen[3] {
                    seen[3] = true;
                    count += 1;
                }
            }
            "LEXEME" => {
                if !seen[2] {
                    seen[2] = true;
                    count += 1;
                }
                if !seen[3] {
                    seen[3] = true;
                    count += 1;
                }
            }
            "MULTIFIELD" => {
                if !seen[4] {
                    seen[4] = true;
                    count += 1;
                }
            }
            "EXTERNAL-ADDRESS" => {
                if !seen[5] {
                    seen[5] = true;
                    count += 1;
                }
            }
            _ => {}
        }
    }
    count
}

/// Compare two methods by specificity. Returns `Ordering::Less` if `a` is more specific
/// than `b`, `Ordering::Greater` if `b` is more specific, and `Ordering::Equal` only
/// when the two methods are identical in specificity (resolved by index tie-break).
///
/// Comparison is performed parameter-by-parameter (left to right). A parameter with
/// fewer covered concrete types is more specific. After all explicit parameters, a
/// method without a wildcard is more specific than one with a wildcard.
fn compare_method_specificity(
    a: &crate::functions::RegisteredMethod,
    b: &crate::functions::RegisteredMethod,
) -> std::cmp::Ordering {
    let max_params = a.type_restrictions.len().max(b.type_restrictions.len());
    for i in 0..max_params {
        let a_count = a
            .type_restrictions
            .get(i)
            .map_or(usize::MAX, |r| restriction_concrete_type_count(r));
        let b_count = b
            .type_restrictions
            .get(i)
            .map_or(usize::MAX, |r| restriction_concrete_type_count(r));
        let ord = a_count.cmp(&b_count);
        if ord != std::cmp::Ordering::Equal {
            return ord; // Fewer concrete types = more specific = Less
        }
    }
    // Tie-break 1: method without wildcard > method with wildcard.
    match (
        a.wildcard_parameter.is_some(),
        b.wildcard_parameter.is_some(),
    ) {
        (false, true) => return std::cmp::Ordering::Less,
        (true, false) => return std::cmp::Ordering::Greater,
        _ => {}
    }
    // Tie-break 2: lower index wins (preserves CLIPS definition order).
    a.index.cmp(&b.index)
}

/// Check if a method is applicable for the given evaluated arguments.
fn method_applicable(method: &crate::functions::RegisteredMethod, arg_values: &[Value]) -> bool {
    let required_count = method.parameters.len();
    let has_wildcard = method.wildcard_parameter.is_some();

    // Arity check: exact match without wildcard, or at-least match with wildcard.
    if has_wildcard {
        if arg_values.len() < required_count {
            return false;
        }
    } else if arg_values.len() != required_count {
        return false;
    }

    // Type restriction check for each required parameter.
    for (i, restrictions) in method.type_restrictions.iter().enumerate() {
        if restrictions.is_empty() {
            continue; // No restriction = any type.
        }
        if i >= arg_values.len() {
            return false; // Shouldn't happen given arity check, but be safe.
        }
        if !restrictions
            .iter()
            .any(|t| value_matches_type(&arg_values[i], t))
        {
            return false;
        }
    }

    true
}

/// Dispatch a call to a generic function.
fn dispatch_generic(
    ctx: &mut EvalContext<'_>,
    generic: &GenericFunction,
    generic_module: crate::modules::ModuleId,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    // Evaluate all arguments first (eager evaluation).
    let arg_values = eval_args(ctx, args)?;

    // Collect all applicable methods, sorted by specificity (most specific first).
    let mut applicable: Vec<crate::functions::RegisteredMethod> = generic
        .methods
        .iter()
        .filter(|m| method_applicable(m, &arg_values))
        .cloned()
        .collect();
    applicable.sort_by(compare_method_specificity);

    if applicable.is_empty() {
        let types: Vec<&str> = arg_values.iter().map(generic_value_type_name).collect();
        return Err(EvalError::NoApplicableMethod {
            name: generic.name.clone(),
            actual_types: types.join(", "),
            span,
        });
    }

    let method = applicable[0].clone();

    // Check recursion limit.
    if ctx.call_depth >= ctx.config.max_call_depth {
        return Err(EvalError::RecursionLimit {
            name: generic.name.clone(),
            depth: ctx.call_depth,
            span,
        });
    }

    // Build the dispatch chain for call-next-method support.
    let chain = MethodChain {
        generic_name: generic.name.clone(),
        generic_module,
        applicable_methods: applicable,
        current_index: 0,
        arg_values: arg_values.clone(),
    };

    // Build parameter bindings for the selected method.
    let (fn_var_map, fn_bindings) = bind_callable_arguments(
        ctx,
        &generic.name,
        &method.parameters,
        method.wildcard_parameter.as_deref(),
        &arg_values,
        span.as_ref(),
    )?;
    // The method body executes in the generic's definition module.
    execute_callable_body(
        ctx,
        &fn_var_map,
        &fn_bindings,
        &method.body,
        generic_module,
        Some(chain),
    )
}

/// Handle `(call-next-method)` — advance to the next method in the generic dispatch chain.
fn dispatch_call_next_method(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    // call-next-method takes no arguments.
    if !args.is_empty() {
        return Err(EvalError::ArityMismatch {
            name: "call-next-method".to_string(),
            expected: "0".to_string(),
            actual: args.len(),
            span,
        });
    }

    // Must be inside a generic method dispatch chain.
    let chain = match &ctx.method_chain {
        Some(c) => c.clone(),
        None => {
            return Err(EvalError::TypeError {
                function: "call-next-method".to_string(),
                expected: "called from within a generic method body".to_string(),
                actual: "called outside generic dispatch context".to_string(),
                span,
            });
        }
    };

    let next_index = chain.current_index + 1;
    if next_index >= chain.applicable_methods.len() {
        return Err(EvalError::NoApplicableMethod {
            name: format!("call-next-method for `{}`", chain.generic_name),
            actual_types: "no next method in dispatch chain".to_string(),
            span,
        });
    }

    // Recursion limit check.
    if ctx.call_depth >= ctx.config.max_call_depth {
        return Err(EvalError::RecursionLimit {
            name: format!("call-next-method for `{}`", chain.generic_name),
            depth: ctx.call_depth,
            span,
        });
    }

    let next_method = chain.applicable_methods[next_index].clone();

    // Bind parameters for the next method using the original arguments.
    let (fn_var_map, fn_bindings) = bind_callable_arguments(
        ctx,
        &chain.generic_name,
        &next_method.parameters,
        next_method.wildcard_parameter.as_deref(),
        &chain.arg_values,
        span.as_ref(),
    )?;

    // Execute next method body with updated chain position.
    let next_chain = MethodChain {
        generic_name: chain.generic_name.clone(),
        generic_module: chain.generic_module,
        applicable_methods: chain.applicable_methods.clone(),
        current_index: next_index,
        arg_values: chain.arg_values.clone(),
    };
    execute_callable_body(
        ctx,
        &fn_var_map,
        &fn_bindings,
        &next_method.body,
        chain.generic_module,
        Some(next_chain),
    )
}

fn bind_callable_arguments(
    ctx: &mut EvalContext<'_>,
    callable_name: &str,
    parameters: &[String],
    wildcard_parameter: Option<&str>,
    arg_values: &[Value],
    span: Option<&SourceSpan>,
) -> Result<(VarMap, BindingSet), EvalError> {
    let mut var_map = VarMap::new();
    let mut bindings = BindingSet::new();

    for (param_name, value) in parameters.iter().zip(arg_values.iter()) {
        bind_parameter(
            ctx,
            callable_name,
            &mut var_map,
            &mut bindings,
            param_name,
            value.clone(),
            span,
        )?;
    }

    if let Some(wildcard_name) = wildcard_parameter {
        let extra_values = arg_values[parameters.len()..]
            .iter()
            .cloned()
            .collect::<ferric_core::Multifield>();
        bind_parameter(
            ctx,
            callable_name,
            &mut var_map,
            &mut bindings,
            wildcard_name,
            Value::Multifield(Box::new(extra_values)),
            span,
        )?;
    }

    Ok((var_map, bindings))
}

fn bind_parameter(
    ctx: &mut EvalContext<'_>,
    callable_name: &str,
    var_map: &mut VarMap,
    bindings: &mut BindingSet,
    parameter_name: &str,
    value: Value,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    let sym = ctx
        .symbol_table
        .intern_symbol(parameter_name, ctx.config.string_encoding)
        .map_err(|_| EvalError::TypeError {
            function: callable_name.to_string(),
            expected: "valid parameter name".to_string(),
            actual: parameter_name.to_string(),
            span: span.cloned(),
        })?;
    let var_id = var_map
        .get_or_create(sym)
        .map_err(|_| EvalError::TypeError {
            function: callable_name.to_string(),
            expected: "bindable variable".to_string(),
            actual: parameter_name.to_string(),
            span: span.cloned(),
        })?;
    bindings.set(var_id, std::rc::Rc::new(value));
    Ok(())
}

// ---------------------------------------------------------------------------
// Translation: ActionExpr -> RuntimeExpr
// ---------------------------------------------------------------------------

/// Translate a parser `ActionExpr` to a `RuntimeExpr`.
#[allow(clippy::too_many_lines)] // Each new loop form adds ~20 lines of translation boilerplate
pub fn from_action_expr(
    expr: &ferric_parser::ActionExpr,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    match expr {
        ferric_parser::ActionExpr::Literal(lit) => {
            let value = literal_to_value(&lit.value, symbol_table, config)?;
            Ok(RuntimeExpr::Literal(value))
        }
        ferric_parser::ActionExpr::Variable(name, span) => Ok(RuntimeExpr::BoundVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_parser::ActionExpr::GlobalVariable(name, span) => Ok(RuntimeExpr::GlobalVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_parser::ActionExpr::FunctionCall(call) => {
            let mut args = Vec::with_capacity(call.args.len());
            for arg in &call.args {
                args.push(from_action_expr(arg, symbol_table, config)?);
            }
            Ok(RuntimeExpr::Call {
                name: call.name.clone(),
                args,
                span: Some(SourceSpan {
                    line: call.span.start.line,
                    column: call.span.start.column,
                }),
            })
        }
        ferric_parser::ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            span,
        } => {
            let condition_rt = from_action_expr(condition, symbol_table, config)?;
            let mut then_branch = Vec::with_capacity(then_actions.len());
            for a in then_actions {
                let rt = from_action_expr(a, symbol_table, config).ok().map(Box::new);
                then_branch.push((a.clone(), rt));
            }
            let mut else_branch = Vec::with_capacity(else_actions.len());
            for a in else_actions {
                let rt = from_action_expr(a, symbol_table, config).ok().map(Box::new);
                else_branch.push((a.clone(), rt));
            }
            Ok(RuntimeExpr::If {
                condition: Box::new(condition_rt),
                then_branch,
                else_branch,
                span: Some(SourceSpan {
                    line: span.start.line,
                    column: span.start.column,
                }),
            })
        }
        ferric_parser::ActionExpr::While {
            condition,
            body,
            span,
        } => {
            let condition_rt = from_action_expr(condition, symbol_table, config)?;
            let mut body_rt = Vec::with_capacity(body.len());
            for a in body {
                let rt = from_action_expr(a, symbol_table, config).ok().map(Box::new);
                body_rt.push((a.clone(), rt));
            }
            Ok(RuntimeExpr::While {
                condition: Box::new(condition_rt),
                body: body_rt,
                span: Some(SourceSpan {
                    line: span.start.line,
                    column: span.start.column,
                }),
            })
        }
        ferric_parser::ActionExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            span,
        } => {
            let start_rt = from_action_expr(start, symbol_table, config)?;
            let end_rt = from_action_expr(end, symbol_table, config)?;
            let mut body_rt = Vec::with_capacity(body.len());
            for a in body {
                let rt = from_action_expr(a, symbol_table, config).ok().map(Box::new);
                body_rt.push((a.clone(), rt));
            }
            Ok(RuntimeExpr::LoopForCount {
                var_name: var_name.clone(),
                start: Box::new(start_rt),
                end: Box::new(end_rt),
                body: body_rt,
                span: Some(SourceSpan {
                    line: span.start.line,
                    column: span.start.column,
                }),
            })
        }
        ferric_parser::ActionExpr::Progn {
            var_name,
            list_expr,
            body,
            span,
        } => {
            let list_rt = from_action_expr(list_expr, symbol_table, config)?;
            let mut body_rt = Vec::with_capacity(body.len());
            for a in body {
                let rt = from_action_expr(a, symbol_table, config).ok().map(Box::new);
                body_rt.push((a.clone(), rt));
            }
            Ok(RuntimeExpr::Progn {
                var_name: var_name.clone(),
                list_expr: Box::new(list_rt),
                body: body_rt,
                span: Some(SourceSpan {
                    line: span.start.line,
                    column: span.start.column,
                }),
            })
        }
    }
}

/// Translate a parser `SExpr` (from test CE) to a `RuntimeExpr`.
///
/// Interprets the S-expression as a function call expression.
/// The first element of a list is the function name, remaining elements are
/// arguments. Atoms are interpreted as literals or variable references.
pub fn from_sexpr(
    expr: &ferric_parser::SExpr,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    match expr {
        ferric_parser::SExpr::List(items, span) => {
            if items.is_empty() {
                return Err(EvalError::UnknownFunction {
                    name: String::new(),
                    span: Some(SourceSpan {
                        line: span.start.line,
                        column: span.start.column,
                    }),
                });
            }
            let func_name = match &items[0] {
                ferric_parser::SExpr::Atom(ferric_parser::Atom::Symbol(s), _) => s.clone(),
                ferric_parser::SExpr::Atom(ferric_parser::Atom::Connective(connective), _) => {
                    connective_to_function_name(*connective).to_string()
                }
                other => {
                    return Err(EvalError::UnknownFunction {
                        name: format!("{other:?}"),
                        span: Some(SourceSpan {
                            line: span.start.line,
                            column: span.start.column,
                        }),
                    });
                }
            };
            let mut args = Vec::with_capacity(items.len() - 1);
            for item in &items[1..] {
                args.push(from_sexpr(item, symbol_table, config)?);
            }
            Ok(RuntimeExpr::Call {
                name: func_name,
                args,
                span: Some(SourceSpan {
                    line: span.start.line,
                    column: span.start.column,
                }),
            })
        }
        ferric_parser::SExpr::Atom(atom, span) => {
            sexpr_atom_to_runtime(atom, span, symbol_table, config)
        }
    }
}

/// Convert a raw S-expression atom into a `RuntimeExpr`.
fn sexpr_atom_to_runtime(
    atom: &ferric_parser::Atom,
    span: &ferric_parser::Span,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    match atom {
        ferric_parser::Atom::Integer(n) => Ok(RuntimeExpr::Literal(Value::Integer(*n))),
        ferric_parser::Atom::Float(f) => Ok(RuntimeExpr::Literal(Value::Float(*f))),
        ferric_parser::Atom::String(s) => {
            let fs =
                FerricString::new(s, config.string_encoding).map_err(|_| EvalError::TypeError {
                    function: "literal".to_string(),
                    expected: "valid string".to_string(),
                    actual: format!("encoding error for {s:?}"),
                    span: Some(SourceSpan {
                        line: span.start.line,
                        column: span.start.column,
                    }),
                })?;
            Ok(RuntimeExpr::Literal(Value::String(fs)))
        }
        ferric_parser::Atom::Symbol(s) => {
            let sym = symbol_table
                .intern_symbol(s, config.string_encoding)
                .map_err(|_| EvalError::TypeError {
                    function: "literal".to_string(),
                    expected: "valid symbol".to_string(),
                    actual: format!("encoding error for {s:?}"),
                    span: Some(SourceSpan {
                        line: span.start.line,
                        column: span.start.column,
                    }),
                })?;
            Ok(RuntimeExpr::Literal(Value::Symbol(sym)))
        }
        ferric_parser::Atom::SingleVar(name) => Ok(RuntimeExpr::BoundVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_parser::Atom::MultiVar(name) => Ok(RuntimeExpr::BoundVar {
            name: format!("$?{name}"),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_parser::Atom::GlobalVar(name) => Ok(RuntimeExpr::GlobalVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_parser::Atom::Connective(_) => Err(EvalError::UnknownFunction {
            name: "connective".to_string(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
    }
}

/// Map a parser connective to its corresponding builtin function name.
fn connective_to_function_name(connective: ferric_parser::Connective) -> &'static str {
    match connective {
        ferric_parser::Connective::And => "and",
        ferric_parser::Connective::Or => "or",
        ferric_parser::Connective::Not => "not",
        ferric_parser::Connective::Equals => "=",
        ferric_parser::Connective::Colon => ":",
        ferric_parser::Connective::Assign => "<-",
    }
}

// ---------------------------------------------------------------------------
// Literal conversion helper
// ---------------------------------------------------------------------------

/// Convert a `LiteralKind` to a runtime `Value`.
fn literal_to_value(
    lit: &ferric_parser::LiteralKind,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<Value, EvalError> {
    match lit {
        ferric_parser::LiteralKind::Integer(n) => Ok(Value::Integer(*n)),
        ferric_parser::LiteralKind::Float(f) => Ok(Value::Float(*f)),
        ferric_parser::LiteralKind::String(s) => {
            let fs =
                FerricString::new(s, config.string_encoding).map_err(|_| EvalError::TypeError {
                    function: "literal".to_string(),
                    expected: "valid string".to_string(),
                    actual: format!("encoding error for {s:?}"),
                    span: None,
                })?;
            Ok(Value::String(fs))
        }
        ferric_parser::LiteralKind::Symbol(s) => {
            let sym = symbol_table
                .intern_symbol(s, config.string_encoding)
                .map_err(|_| EvalError::TypeError {
                    function: "literal".to_string(),
                    expected: "valid symbol".to_string(),
                    actual: format!("encoding error for {s:?}"),
                    span: None,
                })?;
            Ok(Value::Symbol(sym))
        }
    }
}

// ---------------------------------------------------------------------------
// Truth helpers
// ---------------------------------------------------------------------------

/// Check if a value is "truthy" by CLIPS convention.
///
/// `FALSE` symbol and `Void` are falsy; everything else is truthy (including
/// 0, empty string, etc.).
pub fn is_truthy(value: &Value, symbol_table: &SymbolTable) -> bool {
    match value {
        Value::Void => false,
        Value::Symbol(sym) => {
            // Check if this symbol resolves to "FALSE"
            symbol_table.resolve_symbol_str(*sym) != Some("FALSE")
        }
        _ => true,
    }
}

/// Return the CLIPS TRUE symbol value.
pub fn clips_true(symbol_table: &mut SymbolTable, encoding: StringEncoding) -> Value {
    let sym = symbol_table
        .intern_symbol("TRUE", encoding)
        .expect("TRUE is valid ASCII");
    Value::Symbol(sym)
}

/// Return the CLIPS FALSE symbol value.
pub fn clips_false(symbol_table: &mut SymbolTable, encoding: StringEncoding) -> Value {
    let sym = symbol_table
        .intern_symbol("FALSE", encoding)
        .expect("FALSE is valid ASCII");
    Value::Symbol(sym)
}

/// Return a CLIPS boolean symbol based on a condition.
fn clips_bool(cond: bool, symbol_table: &mut SymbolTable, encoding: StringEncoding) -> Value {
    if cond {
        clips_true(symbol_table, encoding)
    } else {
        clips_false(symbol_table, encoding)
    }
}

// ---------------------------------------------------------------------------
// Value type name helper
// ---------------------------------------------------------------------------

/// Returns the CLIPS type name for a value (for error messages).
fn value_type_name(v: &Value) -> &'static str {
    v.type_name()
}

// ---------------------------------------------------------------------------
// Numeric helpers
// ---------------------------------------------------------------------------

/// Internal numeric representation for arithmetic.
enum Numeric {
    Int(i64),
    Flt(f64),
}

/// Extract a numeric value from a `Value`, or return a type error.
fn as_numeric(v: &Value, function: &str, span: Option<&SourceSpan>) -> Result<Numeric, EvalError> {
    match v {
        Value::Integer(i) => Ok(Numeric::Int(*i)),
        Value::Float(f) => Ok(Numeric::Flt(*f)),
        _ => Err(EvalError::TypeError {
            function: function.to_string(),
            expected: "INTEGER or FLOAT".to_string(),
            actual: value_type_name(v).to_string(),
            span: span.cloned(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Built-in function dispatch
// ---------------------------------------------------------------------------

/// Dispatch a built-in function call.
fn dispatch_builtin(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let span_ref = span.as_ref();
    match name {
        // Arithmetic
        "+" => builtin_add(ctx, args, span_ref),
        "-" => builtin_sub(ctx, args, span_ref),
        "*" => builtin_mul(ctx, args, span_ref),
        "/" => builtin_div(ctx, args, span_ref),
        "div" => builtin_int_div(ctx, args, span_ref),
        "mod" => builtin_mod(ctx, args, span_ref),
        "abs" => builtin_abs(ctx, args, span_ref),
        "min" => builtin_min(ctx, args, span_ref),
        "max" => builtin_max(ctx, args, span_ref),

        // Comparison
        ">" => builtin_cmp_gt(ctx, args, span_ref),
        "<" => builtin_cmp_lt(ctx, args, span_ref),
        ">=" => builtin_cmp_gte(ctx, args, span_ref),
        "<=" => builtin_cmp_lte(ctx, args, span_ref),
        "=" => builtin_cmp_eq(ctx, args, span_ref),
        "!=" | "<>" => builtin_cmp_neq(ctx, args, span_ref),
        "eq" => builtin_eq(ctx, args, span_ref),
        "neq" => builtin_neq(ctx, args, span_ref),

        // Boolean
        "and" => builtin_and(ctx, args, span_ref),
        "or" => builtin_or(ctx, args, span_ref),
        "not" => builtin_not(ctx, args, span_ref),

        // Type predicates
        "integerp" => builtin_integerp(ctx, args, span_ref),
        "floatp" => builtin_floatp(ctx, args, span_ref),
        "numberp" => builtin_numberp(ctx, args, span_ref),
        "symbolp" => builtin_symbolp(ctx, args, span_ref),
        "stringp" => builtin_stringp(ctx, args, span_ref),
        "lexemep" => builtin_lexemep(ctx, args, span_ref),
        "multifieldp" => builtin_multifieldp(ctx, args, span_ref),
        "evenp" => builtin_evenp(ctx, args, span_ref),
        "oddp" => builtin_oddp(ctx, args, span_ref),

        // Type conversion
        "integer" => builtin_to_integer(ctx, args, span_ref),
        "float" => builtin_to_float(ctx, args, span_ref),

        // String/Symbol
        "str-cat" => builtin_str_cat(ctx, args, span_ref),
        "sym-cat" => builtin_sym_cat(ctx, args, span_ref),
        "str-length" => builtin_str_length(ctx, args, span_ref),
        "sub-string" => builtin_sub_string(ctx, args, span_ref),

        // Multifield
        "create$" => builtin_create_mf(ctx, args, span_ref),
        "length$" => builtin_length_mf(ctx, args, span_ref),
        "nth$" => builtin_nth_mf(ctx, args, span_ref),
        "member$" => builtin_member_mf(ctx, args, span_ref),
        "subsetp" => builtin_subsetp(ctx, args, span_ref),

        // I/O and environment
        "format" => builtin_format(ctx, args, span_ref),
        "read" => builtin_read(ctx, args, span_ref),
        "readline" => builtin_readline(ctx, args, span_ref),

        // Agenda/focus query
        "get-focus" => builtin_get_focus(ctx, args, span_ref),
        "get-focus-stack" => builtin_get_focus_stack(ctx, args, span_ref),

        // Special forms
        "bind" => dispatch_bind(ctx, args, span_ref),

        _ => Err(EvalError::UnknownFunction {
            name: name.to_string(),
            span,
        }),
    }
}

// ---------------------------------------------------------------------------
// `bind` special form
// ---------------------------------------------------------------------------

/// `bind` — set a global variable. The first argument must be an unevaluated
/// global variable reference (`?*name*`); the second is the new value.
///
/// Returns the value that was bound.
fn dispatch_bind(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("bind", args, 2, span)?;

    match &args[0] {
        RuntimeExpr::GlobalVar { name, .. } => {
            let name = name.clone();
            let value = eval(ctx, &args[1])?;
            let target_module = if name.contains("::") {
                let qualified =
                    parse_qualified_name(&name).map_err(|msg| EvalError::TypeError {
                        function: "bind".to_string(),
                        expected: "valid MODULE::name form".to_string(),
                        actual: msg,
                        span: span.cloned(),
                    })?;
                let (module_name, local_name) = match &qualified {
                    QualifiedName::Qualified { module, name } => (module.as_str(), name.as_str()),
                    QualifiedName::Unqualified(_) => {
                        return Err(EvalError::UnboundGlobal {
                            name,
                            span: span.cloned(),
                        })
                    }
                };
                let module_id = ctx
                    .module_registry
                    .get_by_name(module_name)
                    .ok_or_else(|| EvalError::TypeError {
                        function: "bind".to_string(),
                        expected: format!("existing module `{module_name}`"),
                        actual: "unknown module".to_string(),
                        span: span.cloned(),
                    })?;
                if !ctx.globals.contains(module_id, local_name) {
                    return Err(EvalError::UnboundGlobal {
                        name,
                        span: span.cloned(),
                    });
                }
                if !ctx.module_registry.is_construct_visible(
                    ctx.current_module,
                    module_id,
                    "defglobal",
                    local_name,
                ) {
                    return Err(EvalError::NotVisible {
                        name: format!("?*{name}*"),
                        construct_type: "defglobal".to_string(),
                        from_module: module_label(ctx, ctx.current_module),
                        owning_module: module_name.to_string(),
                        span: span.cloned(),
                    });
                }
                module_id
            } else if ctx.globals.contains(ctx.current_module, &name) {
                ctx.current_module
            } else {
                let all_modules = sorted_dedup_modules(ctx.globals.modules_for_name(&name));
                if all_modules.is_empty() {
                    return Err(EvalError::UnboundGlobal {
                        name,
                        span: span.cloned(),
                    });
                }
                let visible = visible_modules_for_construct(ctx, &all_modules, "defglobal", &name);
                match visible.as_slice() {
                    [module_id] => *module_id,
                    [] => {
                        return Err(EvalError::NotVisible {
                            name: format!("?*{name}*"),
                            construct_type: "defglobal".to_string(),
                            from_module: module_label(ctx, ctx.current_module),
                            owning_module: module_label(ctx, all_modules[0]),
                            span: span.cloned(),
                        })
                    }
                    _ => {
                        return Err(EvalError::TypeError {
                            function: "bind".to_string(),
                            expected: "unambiguous global reference".to_string(),
                            actual: "multiple visible globals; use MODULE::name".to_string(),
                            span: span.cloned(),
                        })
                    }
                }
            };

            let local_name = if let Some((_, local_name)) = name.split_once("::") {
                local_name
            } else {
                name.as_str()
            };
            ctx.globals.set(target_module, local_name, value.clone());
            Ok(value)
        }
        _ => Err(EvalError::TypeError {
            function: "bind".to_string(),
            expected: "global variable reference (?*name*)".to_string(),
            actual: "non-global-variable".to_string(),
            span: span.cloned(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Arity check helpers
// ---------------------------------------------------------------------------

fn check_arity_exact(
    name: &str,
    args: &[RuntimeExpr],
    expected: usize,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    if args.len() != expected {
        return Err(EvalError::ArityMismatch {
            name: name.to_string(),
            expected: expected.to_string(),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    Ok(())
}

fn check_arity_min(
    name: &str,
    args: &[RuntimeExpr],
    min: usize,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    if args.len() < min {
        return Err(EvalError::ArityMismatch {
            name: name.to_string(),
            expected: format!("{min}+"),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Evaluate arguments helper
// ---------------------------------------------------------------------------

fn eval_args(ctx: &mut EvalContext<'_>, args: &[RuntimeExpr]) -> Result<Vec<Value>, EvalError> {
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval(ctx, arg)?);
    }
    Ok(values)
}

// ---------------------------------------------------------------------------
// Arithmetic built-ins
// ---------------------------------------------------------------------------

/// `+` (variadic, 0+ args, identity=0)
#[allow(clippy::cast_precision_loss)]
fn builtin_add(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let values = eval_args(ctx, args)?;
    if values.is_empty() {
        return Ok(Value::Integer(0));
    }
    let mut use_float = false;
    let mut int_sum: i64 = 0;
    let mut float_sum: f64 = 0.0;
    for v in &values {
        match as_numeric(v, "+", span)? {
            Numeric::Int(i) => {
                int_sum = int_sum.wrapping_add(i);
                float_sum += i as f64;
            }
            Numeric::Flt(f) => {
                use_float = true;
                float_sum += f;
            }
        }
    }
    if use_float {
        Ok(Value::Float(float_sum))
    } else {
        Ok(Value::Integer(int_sum))
    }
}

/// `-` (1+ args: unary negate or subtraction)
#[allow(clippy::cast_precision_loss)]
fn builtin_sub(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("-", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    if values.len() == 1 {
        // Unary negation
        return match as_numeric(&values[0], "-", span)? {
            Numeric::Int(i) => Ok(Value::Integer(-i)),
            Numeric::Flt(f) => Ok(Value::Float(-f)),
        };
    }
    // Subtraction: first - rest
    let mut use_float = false;
    let mut int_result: i64 = 0;
    let mut float_result: f64 = 0.0;
    for (idx, v) in values.iter().enumerate() {
        match as_numeric(v, "-", span)? {
            Numeric::Int(i) => {
                if idx == 0 {
                    int_result = i;
                    float_result = i as f64;
                } else {
                    int_result = int_result.wrapping_sub(i);
                    float_result -= i as f64;
                }
            }
            Numeric::Flt(f) => {
                use_float = true;
                if idx == 0 {
                    float_result = f;
                } else {
                    float_result -= f;
                }
            }
        }
    }
    if use_float {
        Ok(Value::Float(float_result))
    } else {
        Ok(Value::Integer(int_result))
    }
}

/// `*` (variadic, 0+ args, identity=1)
#[allow(clippy::cast_precision_loss)]
fn builtin_mul(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let values = eval_args(ctx, args)?;
    if values.is_empty() {
        return Ok(Value::Integer(1));
    }
    let mut use_float = false;
    let mut int_prod: i64 = 1;
    let mut float_prod: f64 = 1.0;
    for v in &values {
        match as_numeric(v, "*", span)? {
            Numeric::Int(i) => {
                int_prod = int_prod.wrapping_mul(i);
                float_prod *= i as f64;
            }
            Numeric::Flt(f) => {
                use_float = true;
                float_prod *= f;
            }
        }
    }
    if use_float {
        Ok(Value::Float(float_prod))
    } else {
        Ok(Value::Integer(int_prod))
    }
}

/// `/` (2 args, float division)
#[allow(clippy::cast_precision_loss)]
fn builtin_div(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("/", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let lhs = match as_numeric(&values[0], "/", span)? {
        Numeric::Int(i) => i as f64,
        Numeric::Flt(f) => f,
    };
    let rhs = match as_numeric(&values[1], "/", span)? {
        Numeric::Int(i) => i as f64,
        Numeric::Flt(f) => f,
    };
    if rhs == 0.0 {
        return Err(EvalError::DivisionByZero {
            function: "/".to_string(),
            span: span.cloned(),
        });
    }
    Ok(Value::Float(lhs / rhs))
}

/// `div` (2 args, integer division)
#[allow(clippy::cast_possible_truncation)]
fn builtin_int_div(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("div", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let lhs = match as_numeric(&values[0], "div", span)? {
        Numeric::Int(i) => i,
        Numeric::Flt(f) => f as i64,
    };
    let rhs = match as_numeric(&values[1], "div", span)? {
        Numeric::Int(i) => i,
        Numeric::Flt(f) => f as i64,
    };
    if rhs == 0 {
        return Err(EvalError::DivisionByZero {
            function: "div".to_string(),
            span: span.cloned(),
        });
    }
    Ok(Value::Integer(lhs / rhs))
}

/// `mod` (2 args, integer modulo)
#[allow(clippy::cast_possible_truncation)]
fn builtin_mod(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("mod", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let lhs = match as_numeric(&values[0], "mod", span)? {
        Numeric::Int(i) => i,
        Numeric::Flt(f) => f as i64,
    };
    let rhs = match as_numeric(&values[1], "mod", span)? {
        Numeric::Int(i) => i,
        Numeric::Flt(f) => f as i64,
    };
    if rhs == 0 {
        return Err(EvalError::DivisionByZero {
            function: "mod".to_string(),
            span: span.cloned(),
        });
    }
    Ok(Value::Integer(lhs % rhs))
}

/// `abs` (1 arg)
fn builtin_abs(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("abs", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    match as_numeric(&values[0], "abs", span)? {
        Numeric::Int(i) => Ok(Value::Integer(i.abs())),
        Numeric::Flt(f) => Ok(Value::Float(f.abs())),
    }
}

/// `min` (1+ args)
#[allow(clippy::cast_precision_loss)]
fn builtin_min(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("min", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let mut use_float = false;
    let mut min_int: i64 = i64::MAX;
    let mut min_float: f64 = f64::INFINITY;
    for v in &values {
        match as_numeric(v, "min", span)? {
            Numeric::Int(i) => {
                if i < min_int {
                    min_int = i;
                }
                if (i as f64) < min_float {
                    min_float = i as f64;
                }
            }
            Numeric::Flt(f) => {
                use_float = true;
                if f < min_float {
                    min_float = f;
                }
            }
        }
    }
    if use_float {
        Ok(Value::Float(min_float))
    } else {
        Ok(Value::Integer(min_int))
    }
}

/// `max` (1+ args)
#[allow(clippy::cast_precision_loss)]
fn builtin_max(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("max", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let mut use_float = false;
    let mut max_int: i64 = i64::MIN;
    let mut max_float: f64 = f64::NEG_INFINITY;
    for v in &values {
        match as_numeric(v, "max", span)? {
            Numeric::Int(i) => {
                if i > max_int {
                    max_int = i;
                }
                if (i as f64) > max_float {
                    max_float = i as f64;
                }
            }
            Numeric::Flt(f) => {
                use_float = true;
                if f > max_float {
                    max_float = f;
                }
            }
        }
    }
    if use_float {
        Ok(Value::Float(max_float))
    } else {
        Ok(Value::Integer(max_int))
    }
}

// ---------------------------------------------------------------------------
// Comparison built-ins
// ---------------------------------------------------------------------------

/// Helper: extract two numeric values for comparison.
#[allow(clippy::cast_precision_loss)]
fn eval_cmp_pair(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<(f64, f64), EvalError> {
    check_arity_exact(name, args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let lhs = match as_numeric(&values[0], name, span)? {
        Numeric::Int(i) => i as f64,
        Numeric::Flt(f) => f,
    };
    let rhs = match as_numeric(&values[1], name, span)? {
        Numeric::Int(i) => i as f64,
        Numeric::Flt(f) => f,
    };
    Ok((lhs, rhs))
}

fn builtin_cmp_gt(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let (l, r) = eval_cmp_pair(ctx, ">", args, span)?;
    Ok(clips_bool(
        l > r,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_cmp_lt(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let (l, r) = eval_cmp_pair(ctx, "<", args, span)?;
    Ok(clips_bool(
        l < r,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_cmp_gte(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let (l, r) = eval_cmp_pair(ctx, ">=", args, span)?;
    Ok(clips_bool(
        l >= r,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_cmp_lte(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let (l, r) = eval_cmp_pair(ctx, "<=", args, span)?;
    Ok(clips_bool(
        l <= r,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

#[allow(clippy::float_cmp)]
fn builtin_cmp_eq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let (l, r) = eval_cmp_pair(ctx, "=", args, span)?;
    Ok(clips_bool(
        (l - r).abs() < f64::EPSILON || l == r,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

#[allow(clippy::float_cmp)]
fn builtin_cmp_neq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let (l, r) = eval_cmp_pair(ctx, "!=", args, span)?;
    Ok(clips_bool(
        !((l - r).abs() < f64::EPSILON || l == r),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

/// `eq` — value equality (symbols, strings, numbers).
fn builtin_eq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("eq", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let result = values[0].structural_eq(&values[1]);
    Ok(clips_bool(
        result,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

/// `neq` — value inequality.
fn builtin_neq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("neq", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let result = !values[0].structural_eq(&values[1]);
    Ok(clips_bool(
        result,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

// ---------------------------------------------------------------------------
// Boolean built-ins
// ---------------------------------------------------------------------------

/// `and` (variadic)
fn builtin_and(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    _span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    for arg in args {
        let v = eval(ctx, arg)?;
        if !is_truthy(&v, ctx.symbol_table) {
            return Ok(clips_false(ctx.symbol_table, ctx.config.string_encoding));
        }
    }
    Ok(clips_true(ctx.symbol_table, ctx.config.string_encoding))
}

/// `or` (variadic)
fn builtin_or(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    _span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    for arg in args {
        let v = eval(ctx, arg)?;
        if is_truthy(&v, ctx.symbol_table) {
            return Ok(clips_true(ctx.symbol_table, ctx.config.string_encoding));
        }
    }
    Ok(clips_false(ctx.symbol_table, ctx.config.string_encoding))
}

/// `not` (1 arg)
fn builtin_not(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("not", args, 1, span)?;
    let v = eval(ctx, &args[0])?;
    Ok(clips_bool(
        !is_truthy(&v, ctx.symbol_table),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

// ---------------------------------------------------------------------------
// Type predicate built-ins
// ---------------------------------------------------------------------------

fn builtin_integerp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("integerp", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    Ok(clips_bool(
        matches!(values[0], Value::Integer(_)),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_floatp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("floatp", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    Ok(clips_bool(
        matches!(values[0], Value::Float(_)),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_numberp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("numberp", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    Ok(clips_bool(
        matches!(values[0], Value::Integer(_) | Value::Float(_)),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_symbolp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("symbolp", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    Ok(clips_bool(
        matches!(values[0], Value::Symbol(_)),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_stringp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("stringp", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    Ok(clips_bool(
        matches!(values[0], Value::String(_)),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_lexemep(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("lexemep", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    Ok(clips_bool(
        matches!(values[0], Value::Symbol(_) | Value::String(_)),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_multifieldp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("multifieldp", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    Ok(clips_bool(
        matches!(values[0], Value::Multifield(_)),
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

fn builtin_evenp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("evenp", args, 1, span)?;
    let val = eval(ctx, &args[0])?;
    match val {
        Value::Integer(n) => Ok(clips_bool(
            n % 2 == 0,
            ctx.symbol_table,
            ctx.config.string_encoding,
        )),
        _ => Err(EvalError::TypeError {
            function: "evenp".to_string(),
            expected: "INTEGER".to_string(),
            actual: value_type_name(&val).to_string(),
            span: span.cloned(),
        }),
    }
}

fn builtin_oddp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("oddp", args, 1, span)?;
    let val = eval(ctx, &args[0])?;
    match val {
        Value::Integer(n) => Ok(clips_bool(
            n % 2 != 0,
            ctx.symbol_table,
            ctx.config.string_encoding,
        )),
        _ => Err(EvalError::TypeError {
            function: "oddp".to_string(),
            expected: "INTEGER".to_string(),
            actual: value_type_name(&val).to_string(),
            span: span.cloned(),
        }),
    }
}

#[allow(clippy::cast_precision_loss)]
fn builtin_to_integer(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("integer", args, 1, span)?;
    let val = eval(ctx, &args[0])?;
    match val {
        Value::Integer(_) => Ok(val),
        #[allow(clippy::cast_possible_truncation)]
        Value::Float(f) => Ok(Value::Integer(f as i64)),
        _ => Err(EvalError::TypeError {
            function: "integer".to_string(),
            expected: "INTEGER or FLOAT".to_string(),
            actual: value_type_name(&val).to_string(),
            span: span.cloned(),
        }),
    }
}

#[allow(clippy::cast_precision_loss)]
fn builtin_to_float(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("float", args, 1, span)?;
    let val = eval(ctx, &args[0])?;
    match val {
        Value::Float(_) => Ok(val),
        Value::Integer(n) => Ok(Value::Float(n as f64)),
        _ => Err(EvalError::TypeError {
            function: "float".to_string(),
            expected: "INTEGER or FLOAT".to_string(),
            actual: value_type_name(&val).to_string(),
            span: span.cloned(),
        }),
    }
}

// ---------------------------------------------------------------------------
// String/Symbol built-ins
// ---------------------------------------------------------------------------

/// Format a float value the same way CLIPS does: always include a decimal point.
///
/// If the fractional part is zero, formats as `"<n>.0"`. Otherwise uses
/// Rust's default float-to-string formatting which already includes decimals.
fn format_float_for_str_cat(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{f:.1}")
    } else {
        f.to_string()
    }
}

/// Append each value's string representation to `buf`, using the symbol table
/// to resolve symbol names.
///
/// Shared by `str-cat` and `sym-cat`.  Multifield elements are space-separated.
fn concat_values_to_string(ctx: &mut EvalContext<'_>, values: &[Value], buf: &mut String) {
    use std::fmt::Write as _;
    for val in values {
        match val {
            Value::Integer(n) => {
                // Use write! to avoid clippy::format_push_string warning.
                let _ = write!(buf, "{n}");
            }
            Value::Float(f) => buf.push_str(&format_float_for_str_cat(*f)),
            Value::Symbol(sym) => {
                if let Some(name) = ctx.symbol_table.resolve_symbol_str(*sym) {
                    buf.push_str(name);
                }
            }
            Value::String(s) => buf.push_str(s.as_str()),
            Value::Multifield(mf) => {
                // Collect first to avoid holding borrow through recursive call.
                let elems: Vec<Value> = mf.iter().cloned().collect();
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        buf.push(' ');
                    }
                    concat_values_to_string(ctx, std::slice::from_ref(elem), buf);
                }
            }
            Value::Void => {}
            Value::ExternalAddress(_) => buf.push_str("<ExternalAddress>"),
        }
    }
}

/// `str-cat` — concatenate 0+ values into a STRING.
///
/// Each argument is converted to its string representation and the results
/// are concatenated.  Integers format as decimal strings, floats always
/// include a decimal point, symbols and strings contribute their content,
/// multifields contribute space-separated elements.
fn builtin_str_cat(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let values = eval_args(ctx, args)?;
    let mut result = String::new();
    concat_values_to_string(ctx, &values, &mut result);
    let fs = FerricString::new(&result, ctx.config.string_encoding).map_err(|e| {
        EvalError::TypeError {
            function: "str-cat".to_string(),
            expected: "encodable string".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        }
    })?;
    Ok(Value::String(fs))
}

/// `sym-cat` — concatenate 0+ values into a SYMBOL.
///
/// Same as `str-cat` but returns a SYMBOL value instead of a STRING.
fn builtin_sym_cat(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let values = eval_args(ctx, args)?;
    let mut result = String::new();
    concat_values_to_string(ctx, &values, &mut result);
    let sym = ctx
        .symbol_table
        .intern_symbol(&result, ctx.config.string_encoding)
        .map_err(|e| EvalError::TypeError {
            function: "sym-cat".to_string(),
            expected: "encodable symbol name".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `str-length` — return the character length of a STRING.
///
/// Takes 1 argument (must be STRING). Returns an INTEGER.
fn builtin_str_length(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("str-length", args, 1, span)?;
    let val = eval(ctx, &args[0])?;
    match &val {
        Value::String(s) => {
            let char_len = i64::try_from(s.as_str().chars().count()).unwrap_or(i64::MAX);
            Ok(Value::Integer(char_len))
        }
        _ => Err(EvalError::TypeError {
            function: "str-length".to_string(),
            expected: "STRING".to_string(),
            actual: generic_value_type_name(&val).to_string(),
            span: span.cloned(),
        }),
    }
}

/// `sub-string` — extract a substring by 1-indexed inclusive position.
///
/// `(sub-string <start> <end> <string>)` — both `start` and `end` are
/// 1-indexed and inclusive.  Out-of-range or inverted indices return an
/// empty string.
fn builtin_sub_string(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("sub-string", args, 3, span)?;
    let values = eval_args(ctx, args)?;

    let start = match &values[0] {
        Value::Integer(n) => *n,
        _ => {
            return Err(EvalError::TypeError {
                function: "sub-string".to_string(),
                expected: "INTEGER (start position)".to_string(),
                actual: generic_value_type_name(&values[0]).to_string(),
                span: span.cloned(),
            })
        }
    };
    let end = match &values[1] {
        Value::Integer(n) => *n,
        _ => {
            return Err(EvalError::TypeError {
                function: "sub-string".to_string(),
                expected: "INTEGER (end position)".to_string(),
                actual: generic_value_type_name(&values[1]).to_string(),
                span: span.cloned(),
            })
        }
    };
    let s = match &values[2] {
        Value::String(s) => s.as_str(),
        _ => {
            return Err(EvalError::TypeError {
                function: "sub-string".to_string(),
                expected: "STRING".to_string(),
                actual: generic_value_type_name(&values[2]).to_string(),
                span: span.cloned(),
            })
        }
    };

    // CLIPS uses 1-indexed inclusive bounds. Convert to Rust 0-indexed.
    let make_empty_string = |ctx: &mut EvalContext<'_>| {
        FerricString::new("", ctx.config.string_encoding).map_err(|e| EvalError::TypeError {
            function: "sub-string".to_string(),
            expected: "encodable string".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        })
    };

    let char_len = s.chars().count();
    let char_len_i64 = i64::try_from(char_len).unwrap_or(i64::MAX);
    if start < 1 || end < 1 || end < start || start > char_len_i64 {
        let fs = make_empty_string(ctx)?;
        return Ok(Value::String(fs));
    }

    let Ok(start_char_idx) = usize::try_from(start - 1) else {
        let fs = make_empty_string(ctx)?;
        return Ok(Value::String(fs));
    };
    let end_char_exclusive = usize::try_from(end).unwrap_or(usize::MAX).min(char_len);

    let mut start_byte_idx = None;
    let mut end_byte_idx = None;
    for (char_idx, (byte_idx, _)) in s.char_indices().enumerate() {
        if char_idx == start_char_idx {
            start_byte_idx = Some(byte_idx);
        }
        if char_idx == end_char_exclusive {
            end_byte_idx = Some(byte_idx);
            break;
        }
    }

    let start_byte_idx = start_byte_idx.unwrap_or(s.len());
    let end_byte_idx = end_byte_idx.unwrap_or(s.len());
    let substr = &s[start_byte_idx..end_byte_idx];

    let fs = FerricString::new(substr, ctx.config.string_encoding).map_err(|e| {
        EvalError::TypeError {
            function: "sub-string".to_string(),
            expected: "encodable string".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        }
    })?;
    Ok(Value::String(fs))
}

// ---------------------------------------------------------------------------
// Multifield built-ins
// ---------------------------------------------------------------------------

/// `create$` — create a multifield from 0+ arguments.
///
/// If any argument is itself a multifield, its elements are flattened into
/// the result (CLIPS implicit multifield flattening).
fn builtin_create_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    _span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let values = eval_args(ctx, args)?;
    let mut result = ferric_core::value::Multifield::new();
    for val in values {
        match val {
            Value::Multifield(mf) => {
                for elem in mf.iter() {
                    result.push(elem.clone());
                }
            }
            other => result.push(other),
        }
    }
    Ok(Value::Multifield(Box::new(result)))
}

/// `length$` — return the length of a multifield.
///
/// Takes 1 argument (must be MULTIFIELD). Returns an INTEGER.
fn builtin_length_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("length$", args, 1, span)?;
    let val = eval(ctx, &args[0])?;
    match &val {
        #[allow(clippy::cast_possible_wrap)] // multifield length fits in i64 in practice
        Value::Multifield(mf) => Ok(Value::Integer(mf.len() as i64)),
        _ => Err(EvalError::TypeError {
            function: "length$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&val).to_string(),
            span: span.cloned(),
        }),
    }
}

/// `nth$` — get the nth element of a multifield (1-indexed).
///
/// `(nth$ <index> <multifield>)`. Returns the value at that position.
/// Returns a `TypeError` if the index is out of range.
fn builtin_nth_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("nth$", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let Value::Integer(index) = &values[0] else {
        return Err(EvalError::TypeError {
            function: "nth$".to_string(),
            expected: "INTEGER (index)".to_string(),
            actual: generic_value_type_name(&values[0]).to_string(),
            span: span.cloned(),
        });
    };
    let index = *index;
    let Value::Multifield(mf) = &values[1] else {
        return Err(EvalError::TypeError {
            function: "nth$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    // CLIPS uses 1-based indexing. Convert safely: index < 1 catches negative and zero.
    let idx = usize::try_from(index - 1).ok().filter(|&i| i < mf.len());
    let Some(idx) = idx else {
        return Err(EvalError::TypeError {
            function: "nth$".to_string(),
            expected: format!("index 1..{}", mf.len()),
            actual: format!("index {index}"),
            span: span.cloned(),
        });
    };
    Ok(mf[idx].clone())
}

/// `member$` — test membership in a multifield.
///
/// `(member$ <value> <multifield>)` — returns the 1-based index if found,
/// or the CLIPS FALSE symbol if not found. Comparison uses structural equality.
fn builtin_member_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("member$", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let needle = &values[0];
    let Value::Multifield(mf) = &values[1] else {
        return Err(EvalError::TypeError {
            function: "member$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    for (i, elem) in mf.iter().enumerate() {
        if needle.structural_eq(elem) {
            // 1-based position; multifield indices fit comfortably in i64.
            #[allow(clippy::cast_possible_wrap)]
            return Ok(Value::Integer((i + 1) as i64));
        }
    }
    Ok(clips_bool(
        false,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

/// `subsetp` — test if one multifield is a subset of another.
///
/// `(subsetp <multifield1> <multifield2>)` — returns TRUE if every element of
/// multifield1 appears in multifield2 (using structural equality), FALSE otherwise.
/// An empty set is a subset of any set.
fn builtin_subsetp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("subsetp", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let Value::Multifield(mf1) = &values[0] else {
        return Err(EvalError::TypeError {
            function: "subsetp".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[0]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Multifield(mf2) = &values[1] else {
        return Err(EvalError::TypeError {
            function: "subsetp".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    // Every element in mf1 must appear somewhere in mf2.
    let is_subset = mf1
        .iter()
        .all(|needle| mf2.iter().any(|elem| needle.structural_eq(elem)));
    Ok(clips_bool(
        is_subset,
        ctx.symbol_table,
        ctx.config.string_encoding,
    ))
}

// ===========================================================================
// I/O and environment builtins
// ===========================================================================

/// `format` — CLIPS-style printf formatting.
///
/// `(format <channel> <format-string> <arg>*)`
///
/// Returns the formatted string. The channel argument is evaluated but not
/// used for output (the evaluator has no router access; use `printout` for
/// output to a channel).
///
/// Format directives:
/// - `%d` — integer
/// - `%f` — float (default 6 decimal places)
/// - `%e` — scientific notation
/// - `%g` — general (shorter of `%f` and `%e`)
/// - `%s` — string representation
/// - `%n` — newline character
/// - `%r` — carriage return
/// - `%%` — literal percent sign
/// - Width/precision: `%10d`, `%-10s`, `%6.2f`, etc.
fn builtin_format(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("format", args, 2, span)?;

    // First arg is channel (evaluate but don't use for output)
    let _channel = eval(ctx, &args[0])?;

    // Second arg is format string
    let fmt_str = match eval(ctx, &args[1])? {
        Value::String(s) => s.as_str().to_string(),
        other => {
            return Err(EvalError::TypeError {
                function: "format".to_string(),
                expected: "STRING".to_string(),
                actual: generic_value_type_name(&other).to_string(),
                span: span.cloned(),
            });
        }
    };

    // Remaining args are format arguments
    let format_args = eval_args(ctx, &args[2..])?;

    let result = apply_format_string(&fmt_str, &format_args, ctx.symbol_table, span)?;
    let fs = FerricString::new(&result, ctx.config.string_encoding).map_err(|_| {
        EvalError::TypeError {
            function: "format".to_string(),
            expected: "valid string encoding".to_string(),
            actual: "result contains invalid characters".to_string(),
            span: span.cloned(),
        }
    })?;
    Ok(Value::String(fs))
}

/// Apply CLIPS format directives to produce a formatted string.
#[allow(clippy::too_many_lines)]
fn apply_format_string(
    fmt: &str,
    args: &[Value],
    symbol_table: &SymbolTable,
    span: Option<&SourceSpan>,
) -> Result<String, EvalError> {
    use std::fmt::Write as FmtWrite;

    let mut result = String::new();
    let mut chars = fmt.chars().peekable();
    let mut arg_idx = 0;

    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }

        match chars.peek() {
            None => {
                result.push('%'); // trailing % — just emit it
            }
            Some('%') => {
                chars.next();
                result.push('%');
            }
            Some('n') => {
                chars.next();
                result.push('\n');
            }
            Some('r') => {
                chars.next();
                result.push('\r');
            }
            _ => {
                // Parse optional flags, width, precision
                let mut left_align = false;
                let mut width: Option<usize> = None;
                let mut precision: Option<usize> = None;

                if chars.peek() == Some(&'-') {
                    left_align = true;
                    chars.next();
                }

                // Width
                let mut width_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        width_str.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !width_str.is_empty() {
                    width = width_str.parse().ok();
                }

                // Precision
                if chars.peek() == Some(&'.') {
                    chars.next();
                    let mut prec_str = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() {
                            prec_str.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    precision = prec_str.parse().ok();
                }

                // Conversion character
                let Some(conv) = chars.next() else {
                    result.push('%');
                    continue;
                };

                if arg_idx >= args.len() {
                    return Err(EvalError::ArityMismatch {
                        name: "format".to_string(),
                        expected: format!("{}+", arg_idx + 3),
                        actual: args.len() + 2,
                        span: span.cloned(),
                    });
                }

                let arg = &args[arg_idx];
                arg_idx += 1;

                let formatted = match conv {
                    'd' => {
                        let n = match arg {
                            Value::Integer(i) => *i,
                            #[allow(clippy::cast_possible_truncation)]
                            Value::Float(f) => *f as i64,
                            _ => {
                                return Err(EvalError::TypeError {
                                    function: "format".to_string(),
                                    expected: "NUMBER for %d".to_string(),
                                    actual: generic_value_type_name(arg).to_string(),
                                    span: span.cloned(),
                                })
                            }
                        };
                        format!("{n}")
                    }
                    'f' => {
                        let f = match arg {
                            Value::Float(f) => *f,
                            #[allow(clippy::cast_precision_loss)]
                            Value::Integer(i) => *i as f64,
                            _ => {
                                return Err(EvalError::TypeError {
                                    function: "format".to_string(),
                                    expected: "NUMBER for %f".to_string(),
                                    actual: generic_value_type_name(arg).to_string(),
                                    span: span.cloned(),
                                })
                            }
                        };
                        let prec = precision.unwrap_or(6);
                        format!("{f:.prec$}")
                    }
                    'e' => {
                        let f = match arg {
                            Value::Float(f) => *f,
                            #[allow(clippy::cast_precision_loss)]
                            Value::Integer(i) => *i as f64,
                            _ => {
                                return Err(EvalError::TypeError {
                                    function: "format".to_string(),
                                    expected: "NUMBER for %e".to_string(),
                                    actual: generic_value_type_name(arg).to_string(),
                                    span: span.cloned(),
                                })
                            }
                        };
                        let prec = precision.unwrap_or(6);
                        format!("{f:.prec$e}")
                    }
                    'g' => {
                        let f = match arg {
                            Value::Float(f) => *f,
                            #[allow(clippy::cast_precision_loss)]
                            Value::Integer(i) => *i as f64,
                            _ => {
                                return Err(EvalError::TypeError {
                                    function: "format".to_string(),
                                    expected: "NUMBER for %g".to_string(),
                                    actual: generic_value_type_name(arg).to_string(),
                                    span: span.cloned(),
                                })
                            }
                        };
                        let f_str = format!("{f:.6}");
                        let e_str = format!("{f:.6e}");
                        if f_str.len() <= e_str.len() {
                            f_str
                        } else {
                            e_str
                        }
                    }
                    's' => format_value_for_format(arg, symbol_table),
                    _ => {
                        // Unknown directive — just emit literal
                        format!("%{conv}")
                    }
                };

                // Apply width and alignment
                match (width, left_align) {
                    (Some(w), true) => {
                        write!(result, "{formatted:<w$}").unwrap();
                    }
                    (Some(w), false) => {
                        write!(result, "{formatted:>w$}").unwrap();
                    }
                    (None, _) => {
                        result.push_str(&formatted);
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Format a value for `%s` in `format` strings.
fn format_value_for_format(value: &Value, symbol_table: &SymbolTable) -> String {
    match value {
        Value::Integer(n) => n.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        Value::Symbol(sym) => symbol_table
            .resolve_symbol_str(*sym)
            .unwrap_or("???")
            .to_string(),
        Value::String(s) => s.as_str().to_string(),
        Value::Void => String::new(),
        Value::ExternalAddress(_) => "<ExternalAddress>".to_string(),
        Value::Multifield(mf) => {
            let parts: Vec<String> = mf
                .as_slice()
                .iter()
                .map(|v| format_value_for_format(v, symbol_table))
                .collect();
            format!("({})", parts.join(" "))
        }
    }
}

/// Intern the `EOF` symbol — shared helper for `read`/`readline`.
fn intern_eof_symbol(
    ctx: &mut EvalContext<'_>,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let sym = ctx
        .symbol_table
        .intern_symbol("EOF", ctx.config.string_encoding)
        .map_err(|_| EvalError::TypeError {
            function: "read".to_string(),
            expected: "valid string encoding".to_string(),
            actual: "cannot intern EOF symbol".to_string(),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `read` — read a single atom from the input buffer.
///
/// `(read)` or `(read <channel>)`
///
/// Pops a line from the input buffer and parses the first whitespace-delimited
/// token as a typed value (integer, float, quoted string, or symbol).
/// Returns `Symbol("EOF")` when no input is available.
fn builtin_read(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::ArityMismatch {
            name: "read".to_string(),
            expected: "0 or 1".to_string(),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    // Evaluate channel arg if present (but don't use it)
    if !args.is_empty() {
        let _ = eval(ctx, &args[0])?;
    }

    let Some(buffer) = ctx.input_buffer.as_deref_mut() else {
        return intern_eof_symbol(ctx, span);
    };

    let Some(line) = buffer.pop_front() else {
        return intern_eof_symbol(ctx, span);
    };

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return intern_eof_symbol(ctx, span);
    }

    // Get first whitespace-delimited token
    let token_str = trimmed.split_whitespace().next().unwrap_or(trimmed);

    // Try to parse as integer
    if let Ok(i) = token_str.parse::<i64>() {
        return Ok(Value::Integer(i));
    }

    // Try to parse as float
    if let Ok(f) = token_str.parse::<f64>() {
        return Ok(Value::Float(f));
    }

    // If quoted string: "..."
    if token_str.starts_with('"') && token_str.ends_with('"') && token_str.len() >= 2 {
        let inner = &token_str[1..token_str.len() - 1];
        let fs = FerricString::new(inner, ctx.config.string_encoding).map_err(|_| {
            EvalError::TypeError {
                function: "read".to_string(),
                expected: "valid string encoding".to_string(),
                actual: format!("cannot create string from `{inner}`"),
                span: span.cloned(),
            }
        })?;
        return Ok(Value::String(fs));
    }

    // Otherwise it's a symbol
    let sym = ctx
        .symbol_table
        .intern_symbol(token_str, ctx.config.string_encoding)
        .map_err(|_| EvalError::TypeError {
            function: "read".to_string(),
            expected: "valid symbol".to_string(),
            actual: format!("cannot intern `{token_str}`"),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `readline` — read a complete line from the input buffer as a string.
///
/// `(readline)` or `(readline <channel>)`
///
/// Returns the complete line as a `STRING` value (without a trailing newline).
/// Returns `Symbol("EOF")` when no input is available.
fn builtin_readline(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::ArityMismatch {
            name: "readline".to_string(),
            expected: "0 or 1".to_string(),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    if !args.is_empty() {
        let _ = eval(ctx, &args[0])?;
    }

    let Some(buffer) = ctx.input_buffer.as_deref_mut() else {
        return intern_eof_symbol(ctx, span);
    };

    match buffer.pop_front() {
        Some(line) => {
            let fs = FerricString::new(&line, ctx.config.string_encoding).map_err(|_| {
                EvalError::TypeError {
                    function: "readline".to_string(),
                    expected: "valid string encoding".to_string(),
                    actual: "cannot create string from input line".to_string(),
                    span: span.cloned(),
                }
            })?;
            Ok(Value::String(fs))
        }
        None => intern_eof_symbol(ctx, span),
    }
}

// ---------------------------------------------------------------------------
// Agenda/focus query builtins
// ---------------------------------------------------------------------------

/// `get-focus` — return the current focus module name as a symbol.
///
/// (get-focus)  ; takes no arguments
fn builtin_get_focus(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("get-focus", args, 0, span)?;

    let focus_id = ctx.module_registry.current_focus();
    let module_name = focus_id
        .and_then(|id| ctx.module_registry.module_name(id))
        .unwrap_or("MAIN");

    let sym = ctx
        .symbol_table
        .intern_symbol(module_name, ctx.config.string_encoding)
        .map_err(|_| EvalError::TypeError {
            function: "get-focus".to_string(),
            expected: "valid module name".to_string(),
            actual: format!("cannot intern `{module_name}`"),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `get-focus-stack` — return the focus stack as a multifield of module name symbols.
///
/// (get-focus-stack)  ; takes no arguments
/// Returns a multifield with the top of the stack first.
fn builtin_get_focus_stack(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("get-focus-stack", args, 0, span)?;

    let stack = ctx.module_registry.focus_stack();
    let mut result = ferric_core::value::Multifield::new();

    // Return in top-first order (reverse of internal stack order)
    for &module_id in stack.iter().rev() {
        let name = ctx.module_registry.module_name(module_id).unwrap_or("???");
        let sym = ctx
            .symbol_table
            .intern_symbol(name, ctx.config.string_encoding)
            .map_err(|_| EvalError::TypeError {
                function: "get-focus-stack".to_string(),
                expected: "valid module name".to_string(),
                actual: format!("cannot intern `{name}`"),
                span: span.cloned(),
            })?;
        result.push(Value::Symbol(sym));
    }

    Ok(Value::Multifield(Box::new(result)))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::binding::{BindingSet, VarMap};
    use std::rc::Rc;

    type ModuleNameMap =
        std::collections::HashMap<(crate::modules::ModuleId, String), crate::modules::ModuleId>;
    type TestCtx = (
        SymbolTable,
        VarMap,
        BindingSet,
        EngineConfig,
        FunctionEnv,
        GlobalStore,
        GenericRegistry,
        crate::modules::ModuleRegistry,
        ModuleNameMap,
    );

    /// Create a default test context tuple.
    fn test_ctx() -> TestCtx {
        let symbol_table = SymbolTable::new();
        let var_map = VarMap::new();
        let bindings = BindingSet::new();
        let config = EngineConfig::utf8();
        let functions = FunctionEnv::new();
        let globals = GlobalStore::new();
        let generics = GenericRegistry::new();
        let module_registry = crate::modules::ModuleRegistry::new();
        let empty_modules: ModuleNameMap = std::collections::HashMap::new();
        (
            symbol_table,
            var_map,
            bindings,
            config,
            functions,
            globals,
            generics,
            module_registry,
            empty_modules,
        )
    }

    /// Helper to evaluate a `RuntimeExpr` with default context.
    fn eval_expr(expr: &RuntimeExpr) -> Result<Value, EvalError> {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let main_id = mr.main_module_id();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: main_id,
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        eval(&mut ctx, expr)
    }

    /// Helper to check if a value is the TRUE symbol.
    fn is_true_symbol(v: &Value, st: &SymbolTable) -> bool {
        if let Value::Symbol(sym) = v {
            st.resolve_symbol_str(*sym) == Some("TRUE")
        } else {
            false
        }
    }

    /// Helper to check if a value is the FALSE symbol.
    fn is_false_symbol(v: &Value, st: &SymbolTable) -> bool {
        if let Value::Symbol(sym) = v {
            st.resolve_symbol_str(*sym) == Some("FALSE")
        } else {
            false
        }
    }

    /// Build a Call expression from name and literal args.
    fn call(name: &str, args: Vec<RuntimeExpr>) -> RuntimeExpr {
        RuntimeExpr::Call {
            name: name.to_string(),
            args,
            span: None,
        }
    }

    fn int(n: i64) -> RuntimeExpr {
        RuntimeExpr::Literal(Value::Integer(n))
    }

    fn float(f: f64) -> RuntimeExpr {
        RuntimeExpr::Literal(Value::Float(f))
    }

    // -------------------------------------------------------------------
    // Literal evaluation
    // -------------------------------------------------------------------

    #[test]
    fn eval_literal_integer() {
        let result = eval_expr(&int(42)).unwrap();
        assert!(result.structural_eq(&Value::Integer(42)));
    }

    #[test]
    fn eval_literal_float() {
        let result = eval_expr(&float(3.125)).unwrap();
        assert!(result.structural_eq(&Value::Float(3.125)));
    }

    #[test]
    fn eval_literal_void() {
        let result = eval_expr(&RuntimeExpr::Literal(Value::Void)).unwrap();
        assert!(result.structural_eq(&Value::Void));
    }

    // -------------------------------------------------------------------
    // Bound variable evaluation
    // -------------------------------------------------------------------

    #[test]
    fn eval_bound_variable() {
        let (mut st, mut vm, mut bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let sym = st.intern_symbol("x", StringEncoding::Utf8).unwrap();
        let var_id = vm.get_or_create(sym).unwrap();
        bs.set(var_id, Rc::new(Value::Integer(99)));

        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(
            &mut ctx,
            &RuntimeExpr::BoundVar {
                name: "x".to_string(),
                span: None,
            },
        )
        .unwrap();
        assert!(result.structural_eq(&Value::Integer(99)));
    }

    #[test]
    fn eval_unbound_variable_returns_error() {
        let result = eval_expr(&RuntimeExpr::BoundVar {
            name: "missing".to_string(),
            span: None,
        });
        assert!(matches!(result, Err(EvalError::UnboundVariable { .. })));
    }

    #[test]
    fn eval_unbound_variable_preserves_source_span() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let action = ferric_parser::ActionExpr::Variable("missing".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();

        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };

        match eval(&mut ctx, &runtime).unwrap_err() {
            EvalError::UnboundVariable {
                span: Some(span), ..
            } => {
                assert_eq!(span.line, 1);
                assert_eq!(span.column, 1);
            }
            other => panic!("expected UnboundVariable with span, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Global variable evaluation
    // -------------------------------------------------------------------

    #[test]
    fn eval_global_variable_returns_error_when_unset() {
        let result = eval_expr(&RuntimeExpr::GlobalVar {
            name: "count".to_string(),
            span: None,
        });
        assert!(matches!(result, Err(EvalError::UnboundGlobal { .. })));
    }

    #[test]
    fn eval_unbound_global_preserves_source_span() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let action = ferric_parser::ActionExpr::GlobalVariable("count".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();

        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };

        match eval(&mut ctx, &runtime).unwrap_err() {
            EvalError::UnboundGlobal {
                span: Some(span), ..
            } => {
                assert_eq!(span.line, 1);
                assert_eq!(span.column, 1);
            }
            other => panic!("expected UnboundGlobal with span, got {other:?}"),
        }
    }

    #[test]
    fn eval_global_variable_returns_value_when_set() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        gs.set(mr.main_module_id(), "count", Value::Integer(42));
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(
            &mut ctx,
            &RuntimeExpr::GlobalVar {
                name: "count".to_string(),
                span: None,
            },
        )
        .unwrap();
        assert!(result.structural_eq(&Value::Integer(42)));
    }

    // -------------------------------------------------------------------
    // bind special form
    // -------------------------------------------------------------------

    #[test]
    fn bind_sets_existing_global_variable() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        gs.set(mr.main_module_id(), "x", Value::Integer(0));
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call(
            "bind",
            vec![
                RuntimeExpr::GlobalVar {
                    name: "x".to_string(),
                    span: None,
                },
                int(99),
            ],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(99)));
        assert!(ctx
            .globals
            .get(mr.main_module_id(), "x")
            .unwrap()
            .structural_eq(&Value::Integer(99)));
    }

    #[test]
    fn bind_with_non_global_first_arg_returns_type_error() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("bind", vec![int(5), int(10)]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn bind_arity_error() {
        let result = eval_expr(&call(
            "bind",
            vec![RuntimeExpr::GlobalVar {
                name: "x".to_string(),
                span: None,
            }],
        ));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // User-defined function dispatch
    // -------------------------------------------------------------------

    fn make_double_func() -> UserFunction {
        // (deffunction double (?x) (* ?x 2))
        UserFunction {
            name: "double".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: None,
            body: vec![ferric_parser::ActionExpr::FunctionCall(
                ferric_parser::FunctionCall {
                    name: "*".to_string(),
                    args: vec![
                        ferric_parser::ActionExpr::Variable("x".to_string(), dummy_span()),
                        ferric_parser::ActionExpr::Literal(ferric_parser::LiteralValue {
                            value: ferric_parser::LiteralKind::Integer(2),
                            span: dummy_span(),
                        }),
                    ],
                    span: dummy_span(),
                },
            )],
        }
    }

    #[test]
    fn user_function_simple_call() {
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics, mr, em) = test_ctx();
        fenv.register(mr.main_module_id(), make_double_func());
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("double", vec![int(5)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(10)));
    }

    #[test]
    fn user_function_multiple_params() {
        // (deffunction add (?a ?b) (+ ?a ?b))
        let func = UserFunction {
            name: "add".to_string(),
            parameters: vec!["a".to_string(), "b".to_string()],
            wildcard_parameter: None,
            body: vec![ferric_parser::ActionExpr::FunctionCall(
                ferric_parser::FunctionCall {
                    name: "+".to_string(),
                    args: vec![
                        ferric_parser::ActionExpr::Variable("a".to_string(), dummy_span()),
                        ferric_parser::ActionExpr::Variable("b".to_string(), dummy_span()),
                    ],
                    span: dummy_span(),
                },
            )],
        };
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics, mr, em) = test_ctx();
        fenv.register(mr.main_module_id(), func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("add", vec![int(3), int(7)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(10)));
    }

    #[test]
    fn user_function_wrong_arity_returns_error() {
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics, mr, em) = test_ctx();
        fenv.register(mr.main_module_id(), make_double_func());
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        // double expects 1 arg, passing 2
        let expr = call("double", vec![int(1), int(2)]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn user_function_wildcard_parameter() {
        // (deffunction first (?x $?rest) ?x)
        let func = UserFunction {
            name: "first".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: Some("rest".to_string()),
            body: vec![ferric_parser::ActionExpr::Variable(
                "x".to_string(),
                dummy_span(),
            )],
        };
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics, mr, em) = test_ctx();
        fenv.register(mr.main_module_id(), func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("first", vec![int(10), int(20), int(30)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(10)));
    }

    #[test]
    fn user_function_recursion_limit_error() {
        // (deffunction inf (?x) (inf ?x)) — infinite recursion
        let func = UserFunction {
            name: "inf".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: None,
            body: vec![ferric_parser::ActionExpr::FunctionCall(
                ferric_parser::FunctionCall {
                    name: "inf".to_string(),
                    args: vec![ferric_parser::ActionExpr::Variable(
                        "x".to_string(),
                        dummy_span(),
                    )],
                    span: dummy_span(),
                },
            )],
        };
        let (mut st, vm, bs, mut cfg, mut fenv, mut gs, generics, mr, em) = test_ctx();
        cfg.max_call_depth = 10; // Low limit for the test
        fenv.register(mr.main_module_id(), func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("inf", vec![int(1)]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::RecursionLimit { .. })));
    }

    #[test]
    fn user_function_calls_builtin() {
        // (deffunction inc (?x) (+ ?x 1))
        let func = UserFunction {
            name: "inc".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: None,
            body: vec![ferric_parser::ActionExpr::FunctionCall(
                ferric_parser::FunctionCall {
                    name: "+".to_string(),
                    args: vec![
                        ferric_parser::ActionExpr::Variable("x".to_string(), dummy_span()),
                        ferric_parser::ActionExpr::Literal(ferric_parser::LiteralValue {
                            value: ferric_parser::LiteralKind::Integer(1),
                            span: dummy_span(),
                        }),
                    ],
                    span: dummy_span(),
                },
            )],
        };
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics, mr, em) = test_ctx();
        fenv.register(mr.main_module_id(), func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &call("inc", vec![int(5)])).unwrap();
        assert!(result.structural_eq(&Value::Integer(6)));
    }

    #[test]
    fn user_function_calls_another_user_function() {
        // (deffunction double (?x) (* ?x 2))
        // (deffunction quadruple (?x) (double (double ?x)))
        let double = make_double_func();
        let quadruple = UserFunction {
            name: "quadruple".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: None,
            body: vec![ferric_parser::ActionExpr::FunctionCall(
                ferric_parser::FunctionCall {
                    name: "double".to_string(),
                    args: vec![ferric_parser::ActionExpr::FunctionCall(
                        ferric_parser::FunctionCall {
                            name: "double".to_string(),
                            args: vec![ferric_parser::ActionExpr::Variable(
                                "x".to_string(),
                                dummy_span(),
                            )],
                            span: dummy_span(),
                        },
                    )],
                    span: dummy_span(),
                },
            )],
        };
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics, mr, em) = test_ctx();
        fenv.register(mr.main_module_id(), double);
        fenv.register(mr.main_module_id(), quadruple);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &call("quadruple", vec![int(3)])).unwrap();
        assert!(result.structural_eq(&Value::Integer(12)));
    }

    // -------------------------------------------------------------------
    // Arithmetic: +
    // -------------------------------------------------------------------

    #[test]
    fn eval_add_two_integers() {
        let expr = call("+", vec![int(1), int(2)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(3)));
    }

    #[test]
    fn eval_add_three_integers() {
        let expr = call("+", vec![int(1), int(2), int(3)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(6)));
    }

    #[test]
    fn eval_add_mixed_promotes_to_float() {
        let expr = call("+", vec![float(1.0), int(2)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Float(3.0)));
    }

    #[test]
    fn eval_add_no_args_returns_zero() {
        let expr = call("+", vec![]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(0)));
    }

    // -------------------------------------------------------------------
    // Arithmetic: -
    // -------------------------------------------------------------------

    #[test]
    fn eval_sub_two_integers() {
        let expr = call("-", vec![int(5), int(3)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(2)));
    }

    #[test]
    fn eval_sub_unary_negate() {
        let expr = call("-", vec![int(5)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(-5)));
    }

    #[test]
    fn eval_sub_no_args_returns_arity_error() {
        let expr = call("-", vec![]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // Arithmetic: *
    // -------------------------------------------------------------------

    #[test]
    fn eval_mul_two_integers() {
        let expr = call("*", vec![int(3), int(4)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(12)));
    }

    #[test]
    fn eval_mul_no_args_returns_one() {
        let expr = call("*", vec![]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(1)));
    }

    // -------------------------------------------------------------------
    // Arithmetic: /
    // -------------------------------------------------------------------

    #[test]
    fn eval_div_float_division() {
        let expr = call("/", vec![int(10), int(3)]);
        let result = eval_expr(&expr).unwrap();
        // 10 / 3 = 3.333...
        if let Value::Float(f) = result {
            assert!((f - 10.0 / 3.0).abs() < 1e-10);
        } else {
            panic!("expected Float");
        }
    }

    #[test]
    fn eval_div_by_zero_returns_error() {
        let expr = call("/", vec![int(1), int(0)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::DivisionByZero { .. })));
    }

    // -------------------------------------------------------------------
    // Arithmetic: div, mod, abs
    // -------------------------------------------------------------------

    #[test]
    fn eval_int_div() {
        let expr = call("div", vec![int(10), int(3)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(3)));
    }

    #[test]
    fn eval_mod_operation() {
        let expr = call("mod", vec![int(10), int(3)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(1)));
    }

    #[test]
    fn eval_abs_negative() {
        let expr = call("abs", vec![int(-42)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(42)));
    }

    #[test]
    fn eval_abs_float() {
        let expr = call("abs", vec![float(-3.125)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Float(3.125)));
    }

    // -------------------------------------------------------------------
    // Arithmetic: min, max
    // -------------------------------------------------------------------

    #[test]
    fn eval_min_integers() {
        let expr = call("min", vec![int(5), int(3), int(7)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(3)));
    }

    #[test]
    fn eval_max_integers() {
        let expr = call("max", vec![int(5), int(3), int(7)]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(7)));
    }

    // -------------------------------------------------------------------
    // Comparison
    // -------------------------------------------------------------------

    #[test]
    fn eval_gt_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call(">", vec![int(5), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_lt_false() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("<", vec![int(5), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_eq_numeric_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("=", vec![int(3), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_neq_numeric() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("!=", vec![int(3), int(4)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_gte_equal() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call(">=", vec![int(5), int(5)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_lte_less() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("<=", vec![int(3), int(5)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    // -------------------------------------------------------------------
    // Value equality: eq / neq
    // -------------------------------------------------------------------

    #[test]
    fn eval_eq_same_integers() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("eq", vec![int(42), int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_neq_different_types() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("neq", vec![int(1), float(1.0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        // Integer(1) and Float(1.0) are different types under structural_eq
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    // -------------------------------------------------------------------
    // Boolean
    // -------------------------------------------------------------------

    #[test]
    fn eval_and_both_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let true_sym = clips_true(&mut st, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call(
            "and",
            vec![
                RuntimeExpr::Literal(true_sym.clone()),
                RuntimeExpr::Literal(true_sym),
            ],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_and_one_false() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let true_sym = clips_true(&mut st, StringEncoding::Utf8);
        let false_sym = clips_false(&mut st, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call(
            "and",
            vec![
                RuntimeExpr::Literal(true_sym),
                RuntimeExpr::Literal(false_sym),
            ],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_or_one_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let false_sym = clips_false(&mut st, StringEncoding::Utf8);
        let true_sym = clips_true(&mut st, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call(
            "or",
            vec![
                RuntimeExpr::Literal(false_sym),
                RuntimeExpr::Literal(true_sym),
            ],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_not_false_returns_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let false_sym = clips_false(&mut st, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("not", vec![RuntimeExpr::Literal(false_sym)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_not_true_returns_false() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let true_sym = clips_true(&mut st, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("not", vec![RuntimeExpr::Literal(true_sym)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    // -------------------------------------------------------------------
    // Nested expressions
    // -------------------------------------------------------------------

    #[test]
    fn eval_nested_expression() {
        // (+ 1 (* 2 3)) = 7
        let expr = call("+", vec![int(1), call("*", vec![int(2), int(3)])]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(7)));
    }

    #[test]
    fn eval_deeply_nested() {
        // (+ (- 10 5) (* 2 (+ 1 1))) = 5 + 4 = 9
        let expr = call(
            "+",
            vec![
                call("-", vec![int(10), int(5)]),
                call("*", vec![int(2), call("+", vec![int(1), int(1)])]),
            ],
        );
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(9)));
    }

    // -------------------------------------------------------------------
    // Type predicates
    // -------------------------------------------------------------------

    #[test]
    fn eval_integerp_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("integerp", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_integerp_false_on_float() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("integerp", vec![float(3.125)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_floatp_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("floatp", vec![float(3.125)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_numberp_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("numberp", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_numberp_float() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("numberp", vec![float(1.0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_symbolp_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let sym = st.intern_symbol("foo", StringEncoding::Utf8).unwrap();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("symbolp", vec![RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_stringp_true() {
        let fs = FerricString::new("hello", StringEncoding::Utf8).unwrap();
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("stringp", vec![RuntimeExpr::Literal(Value::String(fs))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    // -------------------------------------------------------------------
    // lexemep
    // -------------------------------------------------------------------

    #[test]
    fn eval_lexemep_true_for_symbol() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let sym = st.intern_symbol("hello", StringEncoding::Utf8).unwrap();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("lexemep", vec![RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_lexemep_true_for_string() {
        let fs = FerricString::new("hi", StringEncoding::Utf8).unwrap();
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("lexemep", vec![RuntimeExpr::Literal(Value::String(fs))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_lexemep_false_for_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("lexemep", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_lexemep_arity_error() {
        let result = eval_expr(&call("lexemep", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // multifieldp
    // -------------------------------------------------------------------

    #[test]
    fn eval_multifieldp_true_for_multifield() {
        use ferric_core::value::Multifield;
        let mf = Multifield::new();
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call(
            "multifieldp",
            vec![RuntimeExpr::Literal(Value::Multifield(Box::new(mf)))],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_multifieldp_false_for_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("multifieldp", vec![int(0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_multifieldp_arity_error() {
        let result = eval_expr(&call("multifieldp", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // evenp
    // -------------------------------------------------------------------

    #[test]
    fn eval_evenp_true_for_even_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("evenp", vec![int(4)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_evenp_true_for_zero() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("evenp", vec![int(0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_evenp_false_for_odd_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("evenp", vec![int(7)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_evenp_type_error_on_float() {
        let result = eval_expr(&call("evenp", vec![float(2.0)]));
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn eval_evenp_arity_error() {
        let result = eval_expr(&call("evenp", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // oddp
    // -------------------------------------------------------------------

    #[test]
    fn eval_oddp_true_for_odd_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("oddp", vec![int(7)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_oddp_false_for_zero() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("oddp", vec![int(0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_oddp_false_for_even_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("oddp", vec![int(4)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_oddp_type_error_on_float() {
        let result = eval_expr(&call("oddp", vec![float(3.0)]));
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn eval_oddp_arity_error() {
        let result = eval_expr(&call("oddp", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // integer (type conversion)
    // -------------------------------------------------------------------

    #[test]
    fn eval_to_integer_from_integer_passthrough() {
        let result = eval_expr(&call("integer", vec![int(42)])).unwrap();
        assert!(result.structural_eq(&Value::Integer(42)));
    }

    #[test]
    fn eval_to_integer_from_float_truncates() {
        let result = eval_expr(&call("integer", vec![float(3.7)])).unwrap();
        assert!(result.structural_eq(&Value::Integer(3)));
    }

    #[test]
    fn eval_to_integer_from_float_negative_truncates() {
        let result = eval_expr(&call("integer", vec![float(-2.9)])).unwrap();
        assert!(result.structural_eq(&Value::Integer(-2)));
    }

    #[test]
    fn eval_to_integer_type_error_on_symbol() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let sym = st.intern_symbol("foo", StringEncoding::Utf8).unwrap();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("integer", vec![RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn eval_to_integer_arity_error() {
        let result = eval_expr(&call("integer", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // float (type conversion)
    // -------------------------------------------------------------------

    #[test]
    fn eval_to_float_from_float_passthrough() {
        let result = eval_expr(&call("float", vec![float(3.5)])).unwrap();
        assert!(result.structural_eq(&Value::Float(3.5)));
    }

    #[test]
    fn eval_to_float_from_integer_converts() {
        let result = eval_expr(&call("float", vec![int(42)])).unwrap();
        assert!(result.structural_eq(&Value::Float(42.0)));
    }

    #[test]
    fn eval_to_float_type_error_on_symbol() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let sym = st.intern_symbol("bar", StringEncoding::Utf8).unwrap();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("float", vec![RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn eval_to_float_arity_error() {
        let result = eval_expr(&call("float", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    // -------------------------------------------------------------------
    // Error cases
    // -------------------------------------------------------------------

    #[test]
    fn eval_unknown_function_returns_error() {
        let expr = call("nonexistent", vec![int(1)]);
        let result = eval_expr(&expr);
        assert!(matches!(
            result,
            Err(EvalError::UnknownFunction { ref name, .. }) if name == "nonexistent"
        ));
    }

    #[test]
    fn eval_not_arity_error() {
        let expr = call("not", vec![int(1), int(2)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn eval_type_error_add_symbol() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let sym = st.intern_symbol("foo", StringEncoding::Utf8).unwrap();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("+", vec![int(1), RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    // -------------------------------------------------------------------
    // is_truthy tests
    // -------------------------------------------------------------------

    #[test]
    fn truthy_integer_zero_is_truthy() {
        let st = SymbolTable::new();
        assert!(is_truthy(&Value::Integer(0), &st));
    }

    #[test]
    fn truthy_void_is_falsy() {
        let st = SymbolTable::new();
        assert!(!is_truthy(&Value::Void, &st));
    }

    #[test]
    fn truthy_false_symbol_is_falsy() {
        let mut st = SymbolTable::new();
        let false_val = clips_false(&mut st, StringEncoding::Utf8);
        assert!(!is_truthy(&false_val, &st));
    }

    #[test]
    fn truthy_true_symbol_is_truthy() {
        let mut st = SymbolTable::new();
        let true_val = clips_true(&mut st, StringEncoding::Utf8);
        assert!(is_truthy(&true_val, &st));
    }

    // -------------------------------------------------------------------
    // Translation: from_action_expr
    // -------------------------------------------------------------------

    #[test]
    fn translate_action_expr_literal_integer() {
        let (mut st, _, _, cfg, _, _, _, _, _) = test_ctx();
        let action = ferric_parser::ActionExpr::Literal(ferric_parser::LiteralValue {
            value: ferric_parser::LiteralKind::Integer(42),
            span: dummy_span(),
        });
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();
        assert!(matches!(runtime, RuntimeExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn translate_action_expr_variable() {
        let (mut st, _, _, cfg, _, _, _, _, _) = test_ctx();
        let action = ferric_parser::ActionExpr::Variable("x".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();
        assert!(matches!(
            runtime,
            RuntimeExpr::BoundVar {
                ref name,
                span: Some(_),
            } if name == "x"
        ));
    }

    #[test]
    fn translate_action_expr_global_var() {
        let (mut st, _, _, cfg, _, _, _, _, _) = test_ctx();
        let action = ferric_parser::ActionExpr::GlobalVariable("count".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();
        assert!(matches!(
            runtime,
            RuntimeExpr::GlobalVar {
                ref name,
                span: Some(_),
            } if name == "count"
        ));
    }

    #[test]
    fn translate_action_expr_function_call() {
        let (mut st, _, _, cfg, _, _, _, _, _) = test_ctx();
        let action = ferric_parser::ActionExpr::FunctionCall(ferric_parser::FunctionCall {
            name: "+".to_string(),
            args: vec![
                ferric_parser::ActionExpr::Literal(ferric_parser::LiteralValue {
                    value: ferric_parser::LiteralKind::Integer(1),
                    span: dummy_span(),
                }),
                ferric_parser::ActionExpr::Literal(ferric_parser::LiteralValue {
                    value: ferric_parser::LiteralKind::Integer(2),
                    span: dummy_span(),
                }),
            ],
            span: dummy_span(),
        });
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();
        assert!(matches!(
            runtime,
            RuntimeExpr::Call { ref name, ref args, .. } if name == "+" && args.len() == 2
        ));
    }

    // -------------------------------------------------------------------
    // Translation: from_sexpr
    // -------------------------------------------------------------------

    #[test]
    fn translate_sexpr_integer_atom() {
        let (mut st, _, _, cfg, _, _, _, _, _) = test_ctx();
        let sexpr = ferric_parser::SExpr::Atom(ferric_parser::Atom::Integer(42), dummy_span());
        let runtime = from_sexpr(&sexpr, &mut st, &cfg).unwrap();
        assert!(matches!(runtime, RuntimeExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn translate_sexpr_function_call() {
        let (mut st, _, _, cfg, _, _, _, _, _) = test_ctx();
        let sexpr = ferric_parser::SExpr::List(
            vec![
                ferric_parser::SExpr::Atom(
                    ferric_parser::Atom::Symbol(">".to_string()),
                    dummy_span(),
                ),
                ferric_parser::SExpr::Atom(
                    ferric_parser::Atom::SingleVar("x".to_string()),
                    dummy_span(),
                ),
                ferric_parser::SExpr::Atom(ferric_parser::Atom::Integer(10), dummy_span()),
            ],
            dummy_span(),
        );
        let runtime = from_sexpr(&sexpr, &mut st, &cfg).unwrap();
        assert!(matches!(
            runtime,
            RuntimeExpr::Call { ref name, ref args, .. } if name == ">" && args.len() == 2
        ));
    }

    // -------------------------------------------------------------------
    // Specificity scoring unit tests (pass 006)
    // -------------------------------------------------------------------

    #[test]
    fn restriction_concrete_type_count_integer() {
        let r = vec!["INTEGER".to_string()];
        assert_eq!(restriction_concrete_type_count(&r), 1);
    }

    #[test]
    fn restriction_concrete_type_count_number_expands_to_two() {
        let r = vec!["NUMBER".to_string()];
        assert_eq!(restriction_concrete_type_count(&r), 2);
    }

    #[test]
    fn restriction_concrete_type_count_integer_and_float_deduped() {
        // ["INTEGER", "FLOAT"] and ["NUMBER"] should both count as 2 distinct concrete types.
        let r = vec!["INTEGER".to_string(), "FLOAT".to_string()];
        assert_eq!(restriction_concrete_type_count(&r), 2);
    }

    #[test]
    fn restriction_concrete_type_count_empty_is_max() {
        let r: Vec<String> = vec![];
        assert_eq!(restriction_concrete_type_count(&r), usize::MAX);
    }

    #[test]
    fn restriction_concrete_type_count_lexeme_expands_to_two() {
        let r = vec!["LEXEME".to_string()];
        assert_eq!(restriction_concrete_type_count(&r), 2);
    }

    /// Build a minimal `RegisteredMethod` for specificity comparison tests.
    fn make_method(
        index: i32,
        type_restrictions: Vec<Vec<String>>,
        wildcard: bool,
    ) -> crate::functions::RegisteredMethod {
        crate::functions::RegisteredMethod {
            index,
            parameters: (0..type_restrictions.len())
                .map(|i| format!("p{i}"))
                .collect(),
            type_restrictions,
            wildcard_parameter: if wildcard {
                Some("rest".to_string())
            } else {
                None
            },
            body: vec![],
        }
    }

    #[test]
    fn compare_specificity_integer_more_specific_than_number() {
        let integer_method = make_method(0, vec![vec!["INTEGER".to_string()]], false);
        let number_method = make_method(1, vec![vec!["NUMBER".to_string()]], false);
        assert_eq!(
            compare_method_specificity(&integer_method, &number_method),
            std::cmp::Ordering::Less,
            "INTEGER method should be more specific (Less) than NUMBER method"
        );
        assert_eq!(
            compare_method_specificity(&number_method, &integer_method),
            std::cmp::Ordering::Greater,
        );
    }

    #[test]
    fn compare_specificity_restricted_more_specific_than_unrestricted() {
        let restricted = make_method(0, vec![vec!["INTEGER".to_string()]], false);
        let unrestricted = make_method(1, vec![vec![]], false);
        assert_eq!(
            compare_method_specificity(&restricted, &unrestricted),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn compare_specificity_no_wildcard_more_specific_than_wildcard() {
        let fixed = make_method(0, vec![vec!["INTEGER".to_string()]], false);
        let variadic = make_method(1, vec![vec!["INTEGER".to_string()]], true);
        assert_eq!(
            compare_method_specificity(&fixed, &variadic),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn compare_specificity_index_tiebreak() {
        // Two methods with identical type restrictions and no wildcard: lower index wins.
        let m0 = make_method(0, vec![vec!["INTEGER".to_string()]], false);
        let m1 = make_method(1, vec![vec!["INTEGER".to_string()]], false);
        assert_eq!(
            compare_method_specificity(&m0, &m1),
            std::cmp::Ordering::Less,
        );
    }

    // -------------------------------------------------------------------
    // Error display tests
    // -------------------------------------------------------------------

    #[test]
    fn eval_error_display_unknown_function() {
        let err = EvalError::UnknownFunction {
            name: "foobar".to_string(),
            span: Some(SourceSpan {
                line: 5,
                column: 10,
            }),
        };
        let msg = format!("{err}");
        assert!(msg.contains("foobar"));
        assert!(msg.contains("line 5:10"));
    }

    #[test]
    fn eval_error_display_no_span() {
        let err = EvalError::UnboundVariable {
            name: "x".to_string(),
            span: None,
        };
        let msg = format!("{err}");
        assert!(msg.contains("unknown location"));
    }

    #[test]
    fn eval_error_display_recursion_limit() {
        let err = EvalError::RecursionLimit {
            name: "inf".to_string(),
            depth: 256,
            span: None,
        };
        let msg = format!("{err}");
        assert!(msg.contains("inf"));
        assert!(msg.contains("256"));
    }

    // -------------------------------------------------------------------
    // String/Symbol built-ins (pass 009)
    // -------------------------------------------------------------------

    /// Helper: make a STRING `RuntimeExpr` literal.
    fn str_lit(s: &str) -> RuntimeExpr {
        let fs = FerricString::new(s, StringEncoding::Utf8).unwrap();
        RuntimeExpr::Literal(Value::String(fs))
    }

    #[test]
    fn str_cat_zero_args_returns_empty_string() {
        let expr = call("str-cat", vec![]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), ""),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn str_cat_integers() {
        let expr = call("str-cat", vec![int(42), int(-7)]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "42-7"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn str_cat_float_whole_number_includes_decimal() {
        let expr = call("str-cat", vec![float(3.0)]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "3.0"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn str_cat_float_with_fraction() {
        let expr = call("str-cat", vec![float(1.5)]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "1.5"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn str_cat_string_args() {
        let expr = call("str-cat", vec![str_lit("hello"), str_lit(" world")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "hello world"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn str_cat_mixed_types() {
        // (str-cat "val=" 42) => "val=42"
        let expr = call("str-cat", vec![str_lit("val="), int(42)]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "val=42"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn str_cat_returns_string_not_symbol() {
        let expr = call("str-cat", vec![str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        assert!(
            matches!(result, Value::String(_)),
            "str-cat should return STRING"
        );
    }

    #[test]
    fn sym_cat_returns_symbol_not_string() {
        let expr = call("sym-cat", vec![str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        // Should be a Symbol, not a String.
        assert!(
            matches!(result, Value::Symbol(_)),
            "sym-cat should return SYMBOL"
        );
    }

    #[test]
    fn sym_cat_concatenates_values() {
        // sym-cat with string args: the symbol's name should be the concatenation.
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("sym-cat", vec![str_lit("foo"), str_lit("bar")]);
        let result = eval(&mut ctx, &expr).unwrap();
        match result {
            Value::Symbol(sym) => {
                let name = ctx.symbol_table.resolve_symbol_str(sym);
                assert_eq!(name, Some("foobar"), "sym-cat should intern 'foobar'");
            }
            other => panic!("expected SYMBOL, got {other:?}"),
        }
    }

    #[test]
    fn sym_cat_zero_args_returns_empty_symbol() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let expr = call("sym-cat", vec![]);
        let result = eval(&mut ctx, &expr).unwrap();
        match result {
            Value::Symbol(sym) => {
                let name = ctx.symbol_table.resolve_symbol_str(sym);
                assert_eq!(name, Some(""), "sym-cat with no args interns empty string");
            }
            other => panic!("expected SYMBOL, got {other:?}"),
        }
    }

    #[test]
    fn str_length_empty_string_returns_zero() {
        let expr = call("str-length", vec![str_lit("")]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(0)));
    }

    #[test]
    fn str_length_nonempty_string() {
        let expr = call("str-length", vec![str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(5)));
    }

    #[test]
    fn str_length_counts_utf8_characters() {
        let expr = call("str-length", vec![str_lit("é")]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(1)));
    }

    #[test]
    fn str_length_arity_error_no_args() {
        let expr = call("str-length", vec![]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn str_length_arity_error_too_many_args() {
        let expr = call("str-length", vec![str_lit("a"), str_lit("b")]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn str_length_type_error_on_integer() {
        let expr = call("str-length", vec![int(42)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn sub_string_basic_extraction() {
        // (sub-string 2 4 "hello") => "ell"
        let expr = call("sub-string", vec![int(2), int(4), str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "ell"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_full_string() {
        // (sub-string 1 5 "hello") => "hello"
        let expr = call("sub-string", vec![int(1), int(5), str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "hello"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_out_of_range_start_returns_empty() {
        // start > len: returns empty
        let expr = call("sub-string", vec![int(10), int(15), str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), ""),
            other => panic!("expected empty STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_inverted_indices_returns_empty() {
        // end < start: returns empty
        let expr = call("sub-string", vec![int(4), int(2), str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), ""),
            other => panic!("expected empty STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_zero_start_returns_empty() {
        // start < 1: returns empty
        let expr = call("sub-string", vec![int(0), int(3), str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), ""),
            other => panic!("expected empty STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_clamps_end_to_length() {
        // end beyond length is OK: returns up to end of string
        let expr = call("sub-string", vec![int(3), int(100), str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "llo"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_uses_character_positions_for_utf8() {
        let expr = call("sub-string", vec![int(2), int(2), str_lit("héllo")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "é"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_utf8_out_of_range_start_returns_empty() {
        let expr = call("sub-string", vec![int(2), int(2), str_lit("é")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), ""),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn sub_string_arity_error() {
        let expr = call("sub-string", vec![int(1), int(2)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn sub_string_type_error_non_integer_start() {
        let expr = call(
            "sub-string",
            vec![str_lit("oops"), int(3), str_lit("hello")],
        );
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn sub_string_type_error_non_string_third_arg() {
        let expr = call("sub-string", vec![int(1), int(3), int(42)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    // -------------------------------------------------------------------
    // Multifield built-ins (pass 010)
    // -------------------------------------------------------------------

    /// Helper: create a MULTIFIELD `RuntimeExpr` literal from a `Vec` of `Value`s.
    fn mf_lit(values: Vec<Value>) -> RuntimeExpr {
        let mf: ferric_core::value::Multifield = values.into_iter().collect();
        RuntimeExpr::Literal(Value::Multifield(Box::new(mf)))
    }

    #[test]
    fn create_mf_zero_args_returns_empty_multifield() {
        let expr = call("create$", vec![]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::Multifield(mf) => assert!(mf.is_empty()),
            other => panic!("expected MULTIFIELD, got {other:?}"),
        }
    }

    #[test]
    fn create_mf_with_scalar_args() {
        let expr = call("create$", vec![int(1), int(2), int(3)]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::Multifield(mf) => {
                assert_eq!(mf.len(), 3);
                assert!(mf[0].structural_eq(&Value::Integer(1)));
                assert!(mf[1].structural_eq(&Value::Integer(2)));
                assert!(mf[2].structural_eq(&Value::Integer(3)));
            }
            other => panic!("expected MULTIFIELD, got {other:?}"),
        }
    }

    #[test]
    fn create_mf_flattens_nested_multifield() {
        // (create$ 1 (create$ 2 3) 4) => (1 2 3 4)
        let inner = mf_lit(vec![Value::Integer(2), Value::Integer(3)]);
        let expr = call("create$", vec![int(1), inner, int(4)]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::Multifield(mf) => {
                assert_eq!(mf.len(), 4);
                assert!(mf[0].structural_eq(&Value::Integer(1)));
                assert!(mf[1].structural_eq(&Value::Integer(2)));
                assert!(mf[2].structural_eq(&Value::Integer(3)));
                assert!(mf[3].structural_eq(&Value::Integer(4)));
            }
            other => panic!("expected MULTIFIELD, got {other:?}"),
        }
    }

    #[test]
    fn length_mf_returns_length() {
        let mf = mf_lit(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let expr = call("length$", vec![mf]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(3)));
    }

    #[test]
    fn length_mf_of_empty_multifield() {
        let expr = call("length$", vec![mf_lit(vec![])]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(0)));
    }

    #[test]
    fn length_mf_arity_error() {
        let result = eval_expr(&call("length$", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn length_mf_type_error_on_integer() {
        let result = eval_expr(&call("length$", vec![int(42)]));
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn nth_mf_first_element() {
        // (nth$ 1 (create$ 10 20 30)) => 10
        let mf = mf_lit(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]);
        let expr = call("nth$", vec![int(1), mf]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(10)));
    }

    #[test]
    fn nth_mf_last_element() {
        // (nth$ 3 (create$ 10 20 30)) => 30
        let mf = mf_lit(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]);
        let expr = call("nth$", vec![int(3), mf]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(30)));
    }

    #[test]
    fn nth_mf_out_of_range_returns_error() {
        let mf = mf_lit(vec![Value::Integer(10), Value::Integer(20)]);
        let expr = call("nth$", vec![int(5), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn nth_mf_zero_index_returns_error() {
        let mf = mf_lit(vec![Value::Integer(10)]);
        let expr = call("nth$", vec![int(0), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn nth_mf_negative_index_returns_error() {
        let mf = mf_lit(vec![Value::Integer(10)]);
        let expr = call("nth$", vec![int(-1), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn nth_mf_arity_error() {
        let result = eval_expr(&call("nth$", vec![int(1)]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn nth_mf_type_error_non_integer_index() {
        let mf = mf_lit(vec![Value::Integer(10)]);
        let expr = call("nth$", vec![float(1.0), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn nth_mf_type_error_non_multifield_second_arg() {
        let expr = call("nth$", vec![int(1), int(99)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn member_mf_found_returns_index() {
        // (member$ 20 (create$ 10 20 30)) => 2
        let mf = mf_lit(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]);
        let expr = call("member$", vec![int(20), mf]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(2)));
    }

    #[test]
    fn member_mf_first_element_returns_one() {
        let mf = mf_lit(vec![Value::Integer(42), Value::Integer(99)]);
        let expr = call("member$", vec![int(42), mf]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(1)));
    }

    #[test]
    fn member_mf_not_found_returns_false() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf = mf_lit(vec![Value::Integer(10), Value::Integer(20)]);
        let expr = call("member$", vec![int(99), mf]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn member_mf_empty_multifield_returns_false() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf = mf_lit(vec![]);
        let expr = call("member$", vec![int(1), mf]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn member_mf_arity_error() {
        let result = eval_expr(&call("member$", vec![int(1)]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn member_mf_type_error_non_multifield_second_arg() {
        let expr = call("member$", vec![int(1), int(99)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn subsetp_true_when_subset() {
        // (subsetp (create$ 1 2) (create$ 1 2 3)) => TRUE
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1), Value::Integer(2)]);
        let mf2 = mf_lit(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn subsetp_true_when_equal_sets() {
        // (subsetp (create$ 1 2) (create$ 1 2)) => TRUE
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1), Value::Integer(2)]);
        let mf2 = mf_lit(vec![Value::Integer(1), Value::Integer(2)]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn subsetp_true_for_empty_subset() {
        // (subsetp (create$) (create$ 1 2 3)) => TRUE (empty set is subset of any)
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf1 = mf_lit(vec![]);
        let mf2 = mf_lit(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn subsetp_true_for_both_empty() {
        // (subsetp (create$) (create$)) => TRUE
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf1 = mf_lit(vec![]);
        let mf2 = mf_lit(vec![]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn subsetp_false_when_not_subset() {
        // (subsetp (create$ 1 4) (create$ 1 2 3)) => FALSE (4 not in second)
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1), Value::Integer(4)]);
        let mf2 = mf_lit(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn subsetp_false_when_superset_is_empty() {
        // (subsetp (create$ 1) (create$)) => FALSE (1 not in empty)
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1)]);
        let mf2 = mf_lit(vec![]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn subsetp_arity_error() {
        let mf = mf_lit(vec![]);
        let result = eval_expr(&call("subsetp", vec![mf]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn subsetp_type_error_first_arg_not_multifield() {
        let mf = mf_lit(vec![]);
        let expr = call("subsetp", vec![int(1), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn subsetp_type_error_second_arg_not_multifield() {
        let mf = mf_lit(vec![]);
        let expr = call("subsetp", vec![mf, int(1)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn multifieldp_true_for_create_mf_result() {
        // Test that create$ produces a value recognized by multifieldp.
        // (multifieldp (create$ 1 2)) => TRUE
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let inner_expr = call("create$", vec![int(1), int(2)]);
        let expr = call("multifieldp", vec![inner_expr]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    // -----------------------------------------------------------------------
    // format tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_basic_string() {
        // (format nil "hello %s" "world") => "hello world"
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let nil_sym = RuntimeExpr::Literal(Value::Symbol(
            st.intern_symbol("nil", ferric_core::StringEncoding::Utf8)
                .unwrap(),
        ));
        let fmt = RuntimeExpr::Literal(Value::String(
            FerricString::new("hello %s", ferric_core::StringEncoding::Utf8).unwrap(),
        ));
        let arg = RuntimeExpr::Literal(Value::String(
            FerricString::new("world", ferric_core::StringEncoding::Utf8).unwrap(),
        ));
        let expr = call("format", vec![nil_sym, fmt, arg]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::String(s) = result else {
            panic!("expected String result");
        };
        assert_eq!(s.as_str(), "hello world");
    }

    #[test]
    fn test_format_integer() {
        // (format nil "count: %d" 42) => "count: 42"
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("count: %d", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                int(42),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "count: 42");
    }

    #[test]
    fn test_format_float() {
        // (format nil "val: %f" 1.5) => "val: 1.500000"
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("val: %f", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                RuntimeExpr::Literal(Value::Float(1.5)),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "val: 1.500000");
    }

    #[test]
    fn test_format_float_precision() {
        // (format nil "%.2f" 1.5) => "1.50"
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%.2f", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                RuntimeExpr::Literal(Value::Float(1.5)),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "1.50");
    }

    #[test]
    fn test_format_width_right_align() {
        // (format nil "%10d" 42) => "        42"
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%10d", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                int(42),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "        42");
    }

    #[test]
    fn test_format_width_left_align() {
        // (format nil "%-10d" 42) => "42        "
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%-10d", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                int(42),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "42        ");
    }

    #[test]
    fn test_format_newline() {
        // (format nil "a%nb") => "a\nb"
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("a%nb", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "a\nb");
    }

    #[test]
    fn test_format_percent_literal() {
        // (format nil "100%%") => "100%"
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("100%%", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "100%");
    }

    #[test]
    fn test_format_multiple_args() {
        // (format nil "%s is %d" "age" 25) => "age is 25"
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%s is %d", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("age", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                int(25),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "age is 25");
    }

    #[test]
    fn test_format_scientific() {
        // (format nil "%e" 12345.0) => contains "e" notation
        let result = eval_expr(&call(
            "format",
            vec![
                RuntimeExpr::Literal(Value::Void),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%e", ferric_core::StringEncoding::Utf8).unwrap(),
                )),
                RuntimeExpr::Literal(Value::Float(12345.0)),
            ],
        ))
        .unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert!(
            s.as_str().contains('e'),
            "expected scientific notation, got: {}",
            s.as_str()
        );
    }

    // -----------------------------------------------------------------------
    // read tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer = std::collections::VecDeque::new();
        input_buffer.push_back("42".to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_read_float() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer = std::collections::VecDeque::new();
        input_buffer.push_back("2.5".to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(matches!(result, Value::Float(f) if (f - 2.5).abs() < 1e-10));
    }

    #[test]
    fn test_read_symbol() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer = std::collections::VecDeque::new();
        input_buffer.push_back("hello".to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::Symbol(sym) = result else {
            panic!("expected Symbol");
        };
        assert_eq!(ctx.symbol_table.resolve_symbol_str(sym), Some("hello"));
    }

    #[test]
    fn test_read_eof_empty_buffer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer: std::collections::VecDeque<String> =
            std::collections::VecDeque::new();
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::Symbol(sym) = result else {
            panic!("expected Symbol(EOF)");
        };
        assert_eq!(ctx.symbol_table.resolve_symbol_str(sym), Some("EOF"));
    }

    #[test]
    fn test_read_quoted_string() {
        // push `"hello"` (with actual quotes) → String("hello")
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer = std::collections::VecDeque::new();
        input_buffer.push_back(r#""hello""#.to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "hello");
    }

    // -----------------------------------------------------------------------
    // readline tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_readline_returns_full_line() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer = std::collections::VecDeque::new();
        input_buffer.push_back("hello world".to_string());
        let expr = call("readline", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "hello world");
    }

    #[test]
    fn test_readline_eof_empty() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer: std::collections::VecDeque<String> =
            std::collections::VecDeque::new();
        let expr = call("readline", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::Symbol(sym) = result else {
            panic!("expected Symbol(EOF)");
        };
        assert_eq!(ctx.symbol_table.resolve_symbol_str(sym), Some("EOF"));
    }

    #[test]
    fn test_readline_multiple_lines() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let mut input_buffer = std::collections::VecDeque::new();
        input_buffer.push_back("first line".to_string());
        input_buffer.push_back("second line".to_string());
        let expr = call("readline", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: mr.main_module_id(),
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: Some(&mut input_buffer),
        };
        let first = eval(&mut ctx, &expr).unwrap();
        let second = eval(&mut ctx, &expr).unwrap();
        let Value::String(s1) = first else {
            panic!("expected String");
        };
        let Value::String(s2) = second else {
            panic!("expected String");
        };
        assert_eq!(s1.as_str(), "first line");
        assert_eq!(s2.as_str(), "second line");
    }

    #[test]
    fn test_readline_no_input_source() {
        // input_buffer is None (default eval_expr context) — should return EOF symbol
        let result = eval_expr(&call("readline", vec![])).unwrap();
        assert!(matches!(result, Value::Symbol(_)));
    }

    /// Helper: create a dummy parser Span for test construction.
    fn dummy_span() -> ferric_parser::Span {
        ferric_parser::Span::new(
            ferric_parser::Position {
                offset: 0,
                line: 1,
                column: 1,
            },
            ferric_parser::Position {
                offset: 1,
                line: 1,
                column: 2,
            },
            ferric_parser::FileId(0),
        )
    }

    // -------------------------------------------------------------------
    // Agenda/focus query builtins
    // -------------------------------------------------------------------

    #[test]
    fn test_get_focus_returns_current_module() {
        // Default focus is MAIN
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let main_id = mr.main_module_id();
        let expr = call("get-focus", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: main_id,
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        match result {
            Value::Symbol(sym) => {
                let name = ctx.symbol_table.resolve_symbol_str(sym).unwrap();
                assert_eq!(name, "MAIN");
            }
            _ => panic!("expected Symbol, got {result:?}"),
        }
    }

    #[test]
    fn test_get_focus_arity_error() {
        // get-focus takes no arguments; passing one should error.
        let result = eval_expr(&call("get-focus", vec![int(1)]));
        assert!(
            matches!(result, Err(EvalError::ArityMismatch { .. })),
            "expected ArityMismatch, got: {result:?}"
        );
    }

    #[test]
    fn test_get_focus_stack_returns_multifield() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics, mr, em) = test_ctx();
        let main_id = mr.main_module_id();
        let expr = call("get-focus-stack", vec![]);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
            current_module: main_id,
            module_registry: &mr,
            function_modules: &em,
            global_modules: &em,
            generic_modules: &em,
            method_chain: None,
            input_buffer: None,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        match result {
            Value::Multifield(mf) => {
                assert_eq!(
                    mf.len(),
                    1,
                    "default focus stack should have one entry (MAIN)"
                );
                match &mf.as_slice()[0] {
                    Value::Symbol(sym) => {
                        let name = ctx.symbol_table.resolve_symbol_str(*sym).unwrap();
                        assert_eq!(name, "MAIN");
                    }
                    other => panic!("expected Symbol in multifield, got {other:?}"),
                }
            }
            _ => panic!("expected Multifield, got {result:?}"),
        }
    }

    #[test]
    fn test_get_focus_stack_arity_error() {
        let result = eval_expr(&call("get-focus-stack", vec![int(1)]));
        assert!(
            matches!(result, Err(EvalError::ArityMismatch { .. })),
            "expected ArityMismatch, got: {result:?}"
        );
    }
}

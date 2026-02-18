//! Expression evaluation for RHS actions and test CEs.
//!
//! Provides a shared evaluation pipeline that both right-hand-side action
//! arguments and `test` conditional element expressions pass through.
//!
//! Parser-level expression types (`ActionExpr`, `SExpr`) are first translated
//! into a normalized `RuntimeExpr`, which is then evaluated against a set of
//! variable bindings to produce a runtime `Value`.

use ferric_core::binding::{BindingSet, VarMap};
use ferric_core::string::FerricString;
use ferric_core::symbol::SymbolTable;
use ferric_core::value::Value;
use ferric_core::StringEncoding;

use crate::config::EngineConfig;
use crate::functions::{FunctionEnv, GenericFunction, GenericRegistry, GlobalStore, UserFunction};

// ---------------------------------------------------------------------------
// Source span for diagnostics
// ---------------------------------------------------------------------------

/// Source location for evaluation errors.
#[derive(Clone, Debug)]
pub struct SourceSpan {
    pub line: u32,
    pub column: u32,
}

/// Format an optional source span for error messages.
fn format_span(span: Option<&SourceSpan>) -> String {
    match span {
        Some(s) => format!("line {}:{}", s.line, s.column),
        None => "unknown location".to_string(),
    }
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
}

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
    BoundVar(String),
    /// A global variable reference (e.g., `?*name*`).
    GlobalVar(String),
    /// A function call with evaluated arguments.
    Call {
        name: String,
        args: Vec<RuntimeExpr>,
        span: Option<SourceSpan>,
    },
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
}

// ---------------------------------------------------------------------------
// Main evaluation function
// ---------------------------------------------------------------------------

/// Evaluate a runtime expression to a `Value`.
pub fn eval(ctx: &mut EvalContext<'_>, expr: &RuntimeExpr) -> Result<Value, EvalError> {
    match expr {
        RuntimeExpr::Literal(v) => Ok(v.clone()),
        RuntimeExpr::BoundVar(name) => {
            let sym = ctx
                .symbol_table
                .intern_symbol(name, ctx.config.string_encoding)
                .map_err(|_| EvalError::UnboundVariable {
                    name: name.clone(),
                    span: None,
                })?;
            let var_id = ctx
                .var_map
                .lookup(sym)
                .ok_or_else(|| EvalError::UnboundVariable {
                    name: name.clone(),
                    span: None,
                })?;
            ctx.bindings
                .get(var_id)
                .map(|v| (**v).clone())
                .ok_or_else(|| EvalError::UnboundVariable {
                    name: name.clone(),
                    span: None,
                })
        }
        RuntimeExpr::GlobalVar(name) => {
            ctx.globals
                .get(name)
                .cloned()
                .ok_or_else(|| EvalError::UnboundGlobal {
                    name: name.clone(),
                    span: None,
                })
        }
        RuntimeExpr::Call { name, args, span } => {
            match dispatch_builtin(ctx, name, args, span.clone()) {
                Ok(v) => Ok(v),
                Err(EvalError::UnknownFunction { .. }) => {
                    // Try user-defined function first.
                    // Clone to avoid holding a borrow on ctx.functions while
                    // needing &mut ctx for evaluation.
                    if let Some(func) = ctx.functions.get(name).cloned() {
                        dispatch_user_function(ctx, &func, args, span.clone())
                    } else if let Some(generic) = ctx.generics.get(name).cloned() {
                        dispatch_generic(ctx, &generic, args, span.clone())
                    } else {
                        Err(EvalError::UnknownFunction {
                            name: name.clone(),
                            span: span.clone(),
                        })
                    }
                }
                Err(e) => Err(e),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// User-defined function dispatch
// ---------------------------------------------------------------------------

/// Dispatch a call to a user-defined function.
#[allow(clippy::too_many_lines)]
fn dispatch_user_function(
    ctx: &mut EvalContext<'_>,
    func: &UserFunction,
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
    let mut fn_var_map = VarMap::new();
    let mut fn_bindings = BindingSet::new();

    // Bind regular parameters.
    for (i, param) in func.parameters.iter().enumerate() {
        let sym = ctx
            .symbol_table
            .intern_symbol(param, ctx.config.string_encoding)
            .map_err(|_| EvalError::TypeError {
                function: func.name.clone(),
                expected: "valid parameter name".to_string(),
                actual: param.clone(),
                span: span_ref.cloned(),
            })?;
        let var_id = fn_var_map
            .get_or_create(sym)
            .map_err(|_| EvalError::TypeError {
                function: func.name.clone(),
                expected: "bindable variable".to_string(),
                actual: param.clone(),
                span: span_ref.cloned(),
            })?;
        fn_bindings.set(var_id, std::rc::Rc::new(arg_values[i].clone()));
    }

    // Bind wildcard parameter (collects remaining args into a Multifield).
    if let Some(wildcard) = &func.wildcard_parameter {
        let rest: Vec<Value> = arg_values[required..].to_vec();
        let sym = ctx
            .symbol_table
            .intern_symbol(wildcard, ctx.config.string_encoding)
            .map_err(|_| EvalError::TypeError {
                function: func.name.clone(),
                expected: "valid parameter name".to_string(),
                actual: wildcard.clone(),
                span: span_ref.cloned(),
            })?;
        let var_id = fn_var_map
            .get_or_create(sym)
            .map_err(|_| EvalError::TypeError {
                function: func.name.clone(),
                expected: "bindable variable".to_string(),
                actual: wildcard.clone(),
                span: span_ref.cloned(),
            })?;
        fn_bindings.set(
            var_id,
            std::rc::Rc::new(Value::Multifield(Box::new(
                rest.into_iter().collect::<ferric_core::Multifield>(),
            ))),
        );
    }

    // Translate body expressions (ActionExpr → RuntimeExpr) BEFORE constructing
    // the inner EvalContext, because from_action_expr also needs &mut symbol_table.
    let mut body_exprs = Vec::with_capacity(func.body.len());
    for body_expr in &func.body {
        body_exprs.push(from_action_expr(body_expr, ctx.symbol_table, ctx.config)?);
    }

    // Evaluate body expressions sequentially in the function's binding frame.
    // The inner context shares globals and the function environment so that
    // recursive calls and calls to other user-defined functions work correctly.
    let mut result = Value::Void;
    {
        let mut inner_ctx = EvalContext {
            bindings: &fn_bindings,
            var_map: &fn_var_map,
            symbol_table: ctx.symbol_table,
            config: ctx.config,
            functions: ctx.functions,
            globals: ctx.globals,
            generics: ctx.generics,
            call_depth: ctx.call_depth + 1,
        };
        for body_expr in &body_exprs {
            result = eval(&mut inner_ctx, body_expr)?;
        }
    }

    Ok(result)
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
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    // Evaluate all arguments first (eager evaluation).
    let mut arg_values = Vec::with_capacity(args.len());
    for arg in args {
        arg_values.push(eval(ctx, arg)?);
    }

    // Find the first applicable method (methods are sorted by index ascending).
    let applicable_method = generic
        .methods
        .iter()
        .find(|m| method_applicable(m, &arg_values));

    let method = if let Some(m) = applicable_method {
        m.clone()
    } else {
        let types: Vec<&str> = arg_values.iter().map(generic_value_type_name).collect();
        return Err(EvalError::NoApplicableMethod {
            name: generic.name.clone(),
            actual_types: types.join(", "),
            span,
        });
    };

    // Check recursion limit.
    if ctx.call_depth >= ctx.config.max_call_depth {
        return Err(EvalError::RecursionLimit {
            name: generic.name.clone(),
            depth: ctx.call_depth,
            span,
        });
    }

    // Build parameter bindings for the selected method.
    let mut fn_var_map = VarMap::new();
    let mut fn_bindings = BindingSet::new();

    for (i, param_name) in method.parameters.iter().enumerate() {
        let sym = ctx
            .symbol_table
            .intern_symbol(param_name, ctx.config.string_encoding)
            .map_err(|_| EvalError::TypeError {
                function: generic.name.clone(),
                expected: "valid parameter name".to_string(),
                actual: param_name.clone(),
                span: span.clone(),
            })?;
        let var_id = fn_var_map
            .get_or_create(sym)
            .map_err(|_| EvalError::TypeError {
                function: generic.name.clone(),
                expected: "bindable variable".to_string(),
                actual: param_name.clone(),
                span: span.clone(),
            })?;
        fn_bindings.set(var_id, std::rc::Rc::new(arg_values[i].clone()));
    }

    // Bind wildcard parameter if present.
    if let Some(ref wildcard_name) = method.wildcard_parameter {
        let extra_values: Vec<Value> = arg_values[method.parameters.len()..].to_vec();
        let sym = ctx
            .symbol_table
            .intern_symbol(wildcard_name, ctx.config.string_encoding)
            .map_err(|_| EvalError::TypeError {
                function: generic.name.clone(),
                expected: "valid parameter name".to_string(),
                actual: wildcard_name.clone(),
                span: span.clone(),
            })?;
        let var_id = fn_var_map
            .get_or_create(sym)
            .map_err(|_| EvalError::TypeError {
                function: generic.name.clone(),
                expected: "bindable variable".to_string(),
                actual: wildcard_name.clone(),
                span: span.clone(),
            })?;
        fn_bindings.set(
            var_id,
            std::rc::Rc::new(Value::Multifield(Box::new(
                extra_values
                    .into_iter()
                    .collect::<ferric_core::Multifield>(),
            ))),
        );
    }

    // Translate body expressions (ActionExpr → RuntimeExpr) BEFORE constructing
    // the inner EvalContext, because from_action_expr also needs &mut symbol_table.
    let mut body_exprs = Vec::with_capacity(method.body.len());
    for body_expr in &method.body {
        body_exprs.push(from_action_expr(body_expr, ctx.symbol_table, ctx.config)?);
    }

    // Evaluate body expressions sequentially in the method's binding frame.
    let mut result = Value::Void;
    {
        let mut inner_ctx = EvalContext {
            bindings: &fn_bindings,
            var_map: &fn_var_map,
            symbol_table: ctx.symbol_table,
            config: ctx.config,
            functions: ctx.functions,
            globals: ctx.globals,
            generics: ctx.generics,
            call_depth: ctx.call_depth + 1,
        };
        for expr in &body_exprs {
            result = eval(&mut inner_ctx, expr)?;
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Translation: ActionExpr -> RuntimeExpr
// ---------------------------------------------------------------------------

/// Translate a parser `ActionExpr` to a `RuntimeExpr`.
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
        ferric_parser::ActionExpr::Variable(name, _span) => Ok(RuntimeExpr::BoundVar(name.clone())),
        ferric_parser::ActionExpr::GlobalVariable(name, _span) => {
            Ok(RuntimeExpr::GlobalVar(name.clone()))
        }
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
        ferric_parser::Atom::SingleVar(name) => Ok(RuntimeExpr::BoundVar(name.clone())),
        ferric_parser::Atom::MultiVar(name) => Ok(RuntimeExpr::BoundVar(format!("$?{name}"))),
        ferric_parser::Atom::GlobalVar(name) => Ok(RuntimeExpr::GlobalVar(name.clone())),
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
        RuntimeExpr::GlobalVar(name) => {
            let name = name.clone();
            let value = eval(ctx, &args[1])?;
            ctx.globals.set(&name, value.clone());
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

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::binding::{BindingSet, VarMap};
    use std::rc::Rc;

    /// Create a default test context tuple.
    fn test_ctx() -> (
        SymbolTable,
        VarMap,
        BindingSet,
        EngineConfig,
        FunctionEnv,
        GlobalStore,
        GenericRegistry,
    ) {
        let symbol_table = SymbolTable::new();
        let var_map = VarMap::new();
        let bindings = BindingSet::new();
        let config = EngineConfig::utf8();
        let functions = FunctionEnv::new();
        let globals = GlobalStore::new();
        let generics = GenericRegistry::new();
        (
            symbol_table,
            var_map,
            bindings,
            config,
            functions,
            globals,
            generics,
        )
    }

    /// Helper to evaluate a `RuntimeExpr` with default context.
    fn eval_expr(expr: &RuntimeExpr) -> Result<Value, EvalError> {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, mut vm, mut bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        };
        let result = eval(&mut ctx, &RuntimeExpr::BoundVar("x".to_string())).unwrap();
        assert!(result.structural_eq(&Value::Integer(99)));
    }

    #[test]
    fn eval_unbound_variable_returns_error() {
        let result = eval_expr(&RuntimeExpr::BoundVar("missing".to_string()));
        assert!(matches!(result, Err(EvalError::UnboundVariable { .. })));
    }

    // -------------------------------------------------------------------
    // Global variable evaluation
    // -------------------------------------------------------------------

    #[test]
    fn eval_global_variable_returns_error_when_unset() {
        let result = eval_expr(&RuntimeExpr::GlobalVar("count".to_string()));
        assert!(matches!(result, Err(EvalError::UnboundGlobal { .. })));
    }

    #[test]
    fn eval_global_variable_returns_value_when_set() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        gs.set("count", Value::Integer(42));
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let result = eval(&mut ctx, &RuntimeExpr::GlobalVar("count".to_string())).unwrap();
        assert!(result.structural_eq(&Value::Integer(42)));
    }

    // -------------------------------------------------------------------
    // bind special form
    // -------------------------------------------------------------------

    #[test]
    fn bind_sets_global_variable() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call(
            "bind",
            vec![RuntimeExpr::GlobalVar("x".to_string()), int(99)],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(99)));
        assert!(ctx
            .globals
            .get("x")
            .unwrap()
            .structural_eq(&Value::Integer(99)));
    }

    #[test]
    fn bind_with_non_global_first_arg_returns_type_error() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("bind", vec![int(5), int(10)]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn bind_arity_error() {
        let result = eval_expr(&call("bind", vec![RuntimeExpr::GlobalVar("x".to_string())]));
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
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics) = test_ctx();
        fenv.register(make_double_func());
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics) = test_ctx();
        fenv.register(func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("add", vec![int(3), int(7)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(10)));
    }

    #[test]
    fn user_function_wrong_arity_returns_error() {
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics) = test_ctx();
        fenv.register(make_double_func());
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics) = test_ctx();
        fenv.register(func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, mut cfg, mut fenv, mut gs, generics) = test_ctx();
        cfg.max_call_depth = 10; // Low limit for the test
        fenv.register(func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics) = test_ctx();
        fenv.register(func);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, cfg, mut fenv, mut gs, generics) = test_ctx();
        fenv.register(double);
        fenv.register(quadruple);
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call(">", vec![int(5), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_lt_false() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("<", vec![int(5), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_eq_numeric_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("=", vec![int(3), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_neq_numeric() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("!=", vec![int(3), int(4)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_gte_equal() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call(">=", vec![int(5), int(5)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_lte_less() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("eq", vec![int(42), int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_neq_different_types() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        };
        let expr = call("not", vec![RuntimeExpr::Literal(false_sym)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_not_true_returns_false() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("integerp", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_integerp_false_on_float() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("integerp", vec![float(3.125)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_floatp_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("floatp", vec![float(3.125)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_numberp_integer() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("numberp", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_numberp_float() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("numberp", vec![float(1.0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_symbolp_true() {
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        };
        let expr = call("symbolp", vec![RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
    }

    #[test]
    fn eval_stringp_true() {
        let fs = FerricString::new("hello", StringEncoding::Utf8).unwrap();
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
        let mut ctx = EvalContext {
            bindings: &bs,
            var_map: &vm,
            symbol_table: &mut st,
            config: &cfg,
            functions: &fenv,
            globals: &mut gs,
            generics: &generics,
            call_depth: 0,
        };
        let expr = call("stringp", vec![RuntimeExpr::Literal(Value::String(fs))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, ctx.symbol_table));
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
        let (mut st, vm, bs, cfg, fenv, mut gs, generics) = test_ctx();
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
        let (mut st, _, _, cfg, _, _, _) = test_ctx();
        let action = ferric_parser::ActionExpr::Literal(ferric_parser::LiteralValue {
            value: ferric_parser::LiteralKind::Integer(42),
            span: dummy_span(),
        });
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();
        assert!(matches!(runtime, RuntimeExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn translate_action_expr_variable() {
        let (mut st, _, _, cfg, _, _, _) = test_ctx();
        let action = ferric_parser::ActionExpr::Variable("x".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();
        assert!(matches!(runtime, RuntimeExpr::BoundVar(ref n) if n == "x"));
    }

    #[test]
    fn translate_action_expr_global_var() {
        let (mut st, _, _, cfg, _, _, _) = test_ctx();
        let action = ferric_parser::ActionExpr::GlobalVariable("count".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut st, &cfg).unwrap();
        assert!(matches!(runtime, RuntimeExpr::GlobalVar(ref n) if n == "count"));
    }

    #[test]
    fn translate_action_expr_function_call() {
        let (mut st, _, _, cfg, _, _, _) = test_ctx();
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
        let (mut st, _, _, cfg, _, _, _) = test_ctx();
        let sexpr = ferric_parser::SExpr::Atom(ferric_parser::Atom::Integer(42), dummy_span());
        let runtime = from_sexpr(&sexpr, &mut st, &cfg).unwrap();
        assert!(matches!(runtime, RuntimeExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn translate_sexpr_function_call() {
        let (mut st, _, _, cfg, _, _, _) = test_ctx();
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
}

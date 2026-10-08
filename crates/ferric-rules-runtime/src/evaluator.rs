//! Expression evaluation for RHS actions and test CEs.
//!
//! Provides a shared evaluation pipeline that both right-hand-side action
//! arguments and `test` conditional element expressions pass through.
//!
//! Parser-level expression types (`ActionExpr`, `SExpr`) are first translated
//! into a normalized `RuntimeExpr`, which is then evaluated against a set of
//! variable bindings to produce a runtime `Value`.

#[cfg(feature = "tracing")]
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::fact_address::{live_fact_id, public_fact_index};
use crate::field_scanner::{FieldScanner, FieldToken};
use ferric_rules_core::binding::{BindingSet, ValueRef, VarMap};
use ferric_rules_core::string::FerricString;
use ferric_rules_core::symbol::{InstanceName, Symbol, SymbolTable};
use ferric_rules_core::value::Value;
use ferric_rules_core::{Fact, FactAddress, FactBase, FactId, StringEncoding};

use crate::config::EngineConfig;
use crate::functions::{GenericFunction, UserFunction};
use crate::qualified_name::{parse_qualified_name, QualifiedName};
use crate::tracing_support::ferric_event;
#[cfg(feature = "tracing")]
use crate::tracing_support::ferric_span;

// ---------------------------------------------------------------------------
// Source span for diagnostics
// ---------------------------------------------------------------------------

/// Source location for evaluation errors.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    #[error("expression nesting limit exceeded (limit {limit})")]
    ExpressionNestingLimit { limit: usize },

    #[error("action iteration limit exceeded in `{function}` (limit {limit}) at {}", format_span(.span.as_ref()))]
    ActionIterationLimit {
        function: String,
        limit: usize,
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

    #[error("`return` is not valid outside a callable at {}", format_span(.span.as_ref()))]
    ReturnOutsideCallable { span: Option<SourceSpan> },

    /// Internal non-local control signal. Callable boundaries consume this
    /// variant, and the public evaluator converts an escaped signal into
    /// [`EvalError::ReturnOutsideCallable`].
    #[doc(hidden)]
    #[error("internal return control escaped its callable at {}", format_span(.span.as_ref()))]
    ReturnControl {
        value: Value,
        span: Option<SourceSpan>,
    },

    #[error("unsupported operation `{operation}`: {reason} at {}", format_span(.span.as_ref()))]
    UnsupportedOperation {
        operation: String,
        reason: String,
        span: Option<SourceSpan>,
    },

    #[error("`break` is not valid outside a loop body at {}", format_span(.span.as_ref()))]
    BreakOutsideLoop { span: Option<SourceSpan> },

    /// Internal non-local control signal consumed by the nearest loop body.
    #[doc(hidden)]
    #[error("internal break control escaped its loop at {}", format_span(.span.as_ref()))]
    BreakControl { span: Option<SourceSpan> },

    #[error("[EMATHFUN1] Domain error for {function} function at {}", format_span(.span.as_ref()))]
    MathDomain {
        function: String,
        span: Option<SourceSpan>,
    },

    #[error("[EMATHFUN2] Argument overflow for {function} function at {}", format_span(.span.as_ref()))]
    MathOverflow {
        function: String,
        span: Option<SourceSpan>,
    },

    #[error("[EMATHFUN3] Singularity at asymptote in {function} function at {}", format_span(.span.as_ref()))]
    MathSingularity {
        function: String,
        span: Option<SourceSpan>,
    },
}

// ---------------------------------------------------------------------------
// Runtime expression model
// ---------------------------------------------------------------------------

/// One fact-query member and the target expressions evaluated before traversal.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RuntimeQueryBinding {
    pub variable: String,
    pub restrictions: Vec<RuntimeExpr>,
    pub span: Option<SourceSpan>,
}

/// A runtime expression for evaluation.
///
/// This is the normalized expression model consumed by the evaluator.
/// Both RHS `ActionExpr` and test CE `SExpr` are translated into this form.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Returns the `FALSE` symbol when the selected branch is empty.
    ///
    /// Each branch entry pairs the original parser `ActionExpr` (needed by
    /// the action executor for assert/retract/printout arg handling) with its
    /// pre-compiled `RuntimeExpr` counterpart (used as the `runtime_call` hint
    /// in action dispatch and for pure expression evaluation).
    If {
        condition: Box<RuntimeExpr>,
        then_branch: Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        else_branch: Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
    /// CLIPS `(while <condition> do <action>*)` loop.
    ///
    /// Evaluates `condition` before each iteration; executes `body` while
    /// truthy. Returns `FALSE` after completion or `break`.
    ///
    /// Body entries follow the same paired representation as `If` branches.
    While {
        condition: Box<RuntimeExpr>,
        body: Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
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
        body: Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
    /// CLIPS `(progn$ (?var <expr>) <action>*)` / `foreach` loop.
    ///
    /// Evaluates `list_expr` to a multifield value, then iterates each element
    /// binding `var_name` to the element and `<var_name>-index` to the 1-based
    /// index. Returns the last body value from the last iteration, or `FALSE`
    /// if the multifield is empty. A `break` returns `Value::Void`.
    Progn {
        var_name: String,
        list_expr: Box<RuntimeExpr>,
        body: Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
    /// CLIPS fact-query macro forms (`do-for-fact`, `do-for-all-facts`,
    /// `delayed-do-for-all-facts`, `any-factp`, `find-fact`, `find-all-facts`).
    ///
    /// The runtime iterates facts matching each `(variable, template)` binding,
    /// evaluates `query` (with bound variables in scope), and for body-carrying
    /// forms executes `body` for each match.
    QueryAction {
        /// The specific macro name (e.g. `"do-for-fact"`).
        name: String,
        /// `(variable_name, template_name)` binding pairs.
        bindings: Vec<RuntimeQueryBinding>,
        /// Query expression (evaluated to a boolean).
        query: Box<RuntimeExpr>,
        /// Body items (empty for `any-factp`, `find-fact`, `find-all-facts`).
        body: Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        span: Option<SourceSpan>,
    },
    /// CLIPS `(switch <expr> (case <val> then <action>*) ... [(default <action>*)])`.
    ///
    /// Evaluates `expr` once, then iterates cases comparing with equality.
    /// Executes the first matching case body (no fall-through). Falls to
    /// `default` if no case matches.
    #[allow(clippy::type_complexity)]
    Switch {
        expr: Box<RuntimeExpr>,
        cases: Vec<(
            RuntimeExpr,
            Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>,
        )>,
        default: Option<Vec<(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)>>,
        span: Option<SourceSpan>,
    },
    /// Fact mutation syntax preserves relation and slot wrappers as data.
    EffectCall {
        call: Box<ferric_rules_parser::FunctionCall>,
    },
}

// ---------------------------------------------------------------------------
// Method dispatch chain
// ---------------------------------------------------------------------------

/// Active generic dispatch chain for `call-next-method` support.
///
/// When a generic method is executing, this tracks the ordered list of
/// methods and the current position. Restriction
/// queries run only while selecting the current or next method.
#[derive(Clone, Debug)]
pub struct MethodChain {
    /// Name of the generic function being dispatched.
    pub generic_name: String,
    /// Module where the generic is defined.
    pub generic_module: crate::modules::ModuleId,
    /// All methods, in dispatch precedence order; replacement arguments may change applicability.
    pub candidate_methods: Vec<crate::functions::RegisteredMethod>,
    /// Index of the currently executing method in `candidate_methods`.
    pub current_index: usize,
    /// The evaluated argument values (used to rebind parameters in the next method).
    pub arg_values: Vec<Value>,
}

// ---------------------------------------------------------------------------
// Evaluation context
// ---------------------------------------------------------------------------

/// Fact identities used by compact slot references, independently of mutable
/// RHS values or same-named loop variables.
pub(crate) type CompactFactBindings = std::collections::HashMap<String, CompactFactBinding>;

/// A lexical fact member. Query bodies may retain the selected immutable record
/// after retraction; ordinary LHS addresses continue to require a live fact.
#[derive(Clone, Debug)]
pub(crate) struct CompactFactBinding {
    address: FactAddress,
    retained: Option<Arc<Fact>>,
}

impl CompactFactBinding {
    pub(crate) fn live(address: FactAddress) -> Self {
        Self {
            address,
            retained: None,
        }
    }

    pub(crate) fn retained(address: FactAddress, fact: Arc<Fact>) -> Self {
        Self {
            address,
            retained: Some(fact),
        }
    }

    #[cfg(test)]
    pub(crate) fn fact_id(&self) -> FactId {
        self.address
            .fact_id()
            .expect("query members are real facts")
    }

    pub(crate) fn address(&self) -> &FactAddress {
        &self.address
    }

    fn record<'a>(&'a self, facts: &'a FactBase, epoch: u64) -> Option<&'a Fact> {
        live_fact_id(facts, epoch, &self.address)
            .and_then(|id| facts.get(id))
            .map(|entry| &entry.fact)
            .or(self.retained.as_deref())
    }
}

pub(crate) const COMPACT_FACT_SLOT_REF: &str = "__fact_slot_ref";

/// Mutable state belonging to one callable invocation, never to the engine.
#[derive(Default)]
pub(crate) struct CallableLocals {
    values: HashMap<String, Value>,
    /// These names read from the current immutable iterator/query frame before
    /// local values; writes to generated indexes remain ordinary assignments.
    lexical_names: HashSet<String>,
    protected_names: HashSet<String>,
}

impl CallableLocals {
    pub(crate) fn from_values(values: HashMap<String, Value>) -> Self {
        Self {
            values,
            ..Self::default()
        }
    }

    pub(crate) fn into_values(self) -> HashMap<String, Value> {
        self.values
    }
}

/// Context needed for expression evaluation.
///
/// Construction is crate-private through exclusive engine access.
/// Engine entry points evaluate under exclusive engine access. The configuration
/// includes an action budget shared with nested calls: future constructors must
/// preserve exclusive evaluation or give independent roots separate configs.
pub struct EvalContext<'a> {
    pub(crate) engine: &'a mut crate::Engine,
    pub bindings: &'a BindingSet,
    pub var_map: &'a VarMap,
    /// Invocation-local overrides over the immutable original parameter frame.
    pub(crate) callable_locals: Option<&'a mut CallableLocals>,
    pub call_depth: usize,
    pub(crate) expression_depth: usize,
    /// Lexical module, preserved when reset changes the engine's focus.
    pub current_module: crate::modules::ModuleId,
    /// Dynamic defaults bind callable names at definition but read direct globals
    /// in the assertion caller. Ordinary callable boundaries clear this override.
    pub(crate) global_module: Option<crate::modules::ModuleId>,
    pub method_chain: Option<MethodChain>,
    pub(crate) compact_fact_bindings: Option<&'a CompactFactBindings>,
    /// Engine mutation is forbidden during matching, including nested callables.
    pub(crate) allow_engine_effects: bool,
}

fn ordinary_binding(ctx: &mut EvalContext<'_>, name: &str) -> Option<Value> {
    // Single-field and multifield spellings refer to the same binding.
    let name = name.strip_prefix("$?").unwrap_or(name);
    if let Some(locals) = ctx.callable_locals.as_deref() {
        if let Some(value) = locals.values.get(name) {
            if !locals.lexical_names.contains(name) {
                return Some(value.clone());
            }
        }
    }
    let symbol = ctx
        .engine
        .symbol_table
        .intern_symbol(name, ctx.engine.config.string_encoding)
        .ok()?;
    let variable = ctx.var_map.lookup(symbol)?;
    ctx.bindings.get(variable).map(|value| (**value).clone())
}

/// Lexical iterator and query names read from their temporary immutable frame.
/// Local assignments persist; scope metadata is restored on every exit.
fn with_callable_local_scope<'a, T>(
    ctx: &mut EvalContext<'_>,
    names: impl IntoIterator<Item = &'a str>,
    protected_name: Option<&str>,
    execute: impl FnOnce(&mut EvalContext<'_>) -> Result<T, EvalError>,
) -> Result<T, EvalError> {
    let mut added = Vec::new();
    let mut added_protection = false;
    if let Some(locals) = ctx.callable_locals.as_deref_mut() {
        for name in names {
            let name = name.strip_prefix("$?").unwrap_or(name);
            if locals.lexical_names.insert(name.to_string()) {
                added.push(name.to_string());
            }
        }
        if let Some(name) = protected_name {
            added_protection = locals.protected_names.insert(name.to_string());
        }
    }
    let result = execute(ctx);
    if let Some(locals) = ctx.callable_locals.as_deref_mut() {
        for name in added {
            locals.lexical_names.remove(&name);
        }
        if added_protection {
            locals
                .protected_names
                .remove(protected_name.expect("added protected name"));
        }
    }
    result
}

fn module_label(ctx: &EvalContext<'_>, module_id: crate::modules::ModuleId) -> String {
    ctx.engine
        .module_registry
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

fn global_lookup_module(ctx: &EvalContext<'_>) -> crate::modules::ModuleId {
    ctx.global_module.unwrap_or(ctx.current_module)
}

fn visible_modules_for_construct(
    ctx: &EvalContext<'_>,
    modules: &[crate::modules::ModuleId],
    construct_type: &str,
    local_name: &str,
) -> Vec<crate::modules::ModuleId> {
    let lookup_module = if construct_type == "defglobal" {
        global_lookup_module(ctx)
    } else {
        ctx.current_module
    };
    sorted_dedup_modules(
        modules
            .iter()
            .copied()
            .filter(|module_id| {
                ctx.engine.module_registry.is_construct_visible(
                    lookup_module,
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
            from_module: module_label(
                ctx,
                if construct_type == "defglobal" {
                    global_lookup_module(ctx)
                } else {
                    ctx.current_module
                },
            ),
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

#[cfg(feature = "tracing")]
thread_local! {
    static EVAL_RUN_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "tracing")]
struct EvalRunGuard;

#[cfg(feature = "tracing")]
impl EvalRunGuard {
    fn is_active() -> bool {
        EVAL_RUN_ACTIVE.with(Cell::get)
    }

    fn enter_root() -> Self {
        EVAL_RUN_ACTIVE.with(|active| {
            debug_assert!(
                !active.get(),
                "eval root guard should be entered once per run"
            );
            active.set(true);
        });
        Self
    }
}

#[cfg(feature = "tracing")]
impl Drop for EvalRunGuard {
    fn drop(&mut self) {
        EVAL_RUN_ACTIVE.with(|active| active.set(false));
    }
}

/// Evaluate a runtime expression to a `Value`.
///
/// Root entrypoint for expression evaluation. Recursive evaluation should flow
/// through `eval_inner`, which keeps tracing lightweight on deep call stacks.
pub fn eval(ctx: &mut EvalContext<'_>, expr: &RuntimeExpr) -> Result<Value, EvalError> {
    finish_root_evaluation(eval_with_budget(ctx, expr))
}

/// Action loops consume breaks at their own lexical boundary. Ordinary callers
/// use `eval`, which rejects any control signal that escaped its expression.
pub(crate) fn eval_action_expression(
    ctx: &mut EvalContext<'_>,
    expr: &RuntimeExpr,
) -> Result<Value, EvalError> {
    match eval_with_budget(ctx, expr) {
        Err(EvalError::ReturnControl { span, .. }) => {
            Err(EvalError::ReturnOutsideCallable { span })
        }
        other => other,
    }
}

fn eval_with_budget(ctx: &mut EvalContext<'_>, expr: &RuntimeExpr) -> Result<Value, EvalError> {
    let owns_action_loop_budget = ctx.engine.config.begin_action_loop_budget_if_inactive();

    #[cfg(feature = "tracing")]
    let result = {
        if EvalRunGuard::is_active() {
            eval_inner(ctx, expr)
        } else {
            let _run_guard = EvalRunGuard::enter_root();
            ferric_span!(debug_span, "eval_root", call_depth = ctx.call_depth);
            eval_inner(ctx, expr)
        }
    };

    #[cfg(not(feature = "tracing"))]
    let result = { eval_inner(ctx, expr) };

    if owns_action_loop_budget {
        ctx.engine.config.end_action_loop_budget();
    }
    result
}

/// Whether `name` has the `MODULE::name` form. Every call and global read
/// asks this; the single-byte scan rejects the usual colon-free name without
/// setting up a substring search.
fn is_module_qualified(name: &str) -> bool {
    name.contains(':') && name.contains("::")
}

fn finish_root_evaluation(result: Result<Value, EvalError>) -> Result<Value, EvalError> {
    result.map_err(contain_control_signals)
}

/// Report a `return` or `break` signal that reached an evaluation boundary
/// it may not cross (a root, or a template default) as the user-facing error.
pub(crate) fn contain_control_signals(error: EvalError) -> EvalError {
    match error {
        EvalError::ReturnControl { span, .. } => EvalError::ReturnOutsideCallable { span },
        EvalError::BreakControl { span } => EvalError::BreakOutsideLoop { span },
        other => other,
    }
}

/// Limit active evaluator frames across callable bodies and nested expressions.
/// Counting only user calls lets nested `if`/argument expressions multiply the
/// native stack at each recursion level. This counter is local to evaluation;
/// it introduces no shared mutable state or atomics on the expression path.
const MAX_EXPRESSION_DEPTH: usize = 64;

pub(crate) fn eval_inner(
    ctx: &mut EvalContext<'_>,
    expr: &RuntimeExpr,
) -> Result<Value, EvalError> {
    if ctx.expression_depth >= MAX_EXPRESSION_DEPTH {
        return Err(EvalError::ExpressionNestingLimit {
            limit: MAX_EXPRESSION_DEPTH,
        });
    }
    ctx.expression_depth += 1;
    let result = eval_dispatch(ctx, expr);
    ctx.expression_depth -= 1;
    result
}

#[allow(clippy::too_many_lines)] // The visibility checks add necessary verbosity
fn eval_dispatch(ctx: &mut EvalContext<'_>, expr: &RuntimeExpr) -> Result<Value, EvalError> {
    match expr {
        RuntimeExpr::EffectCall { call } => crate::effects::eval_syntax(ctx, call),
        RuntimeExpr::Literal(v) => Ok(v.clone()),
        RuntimeExpr::BoundVar { name, span } => {
            ordinary_binding(ctx, name).ok_or_else(|| EvalError::UnboundVariable {
                name: name.clone(),
                span: span.clone(),
            })
        }
        RuntimeExpr::GlobalVar { name, span } => {
            // Module-qualified global references (MODULE::name) use the qualified path.
            if is_module_qualified(name) {
                return resolve_qualified_global(ctx, name, span.clone());
            }
            if let Some(value) = ctx
                .engine
                .globals
                .get(global_lookup_module(ctx), name)
                .cloned()
            {
                return Ok(value);
            }

            let all_modules = sorted_dedup_modules(ctx.engine.globals.modules_for_name(name));
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
            ctx.engine
                .globals
                .get(owner, name)
                .cloned()
                .ok_or_else(|| EvalError::UnboundGlobal {
                    name: name.clone(),
                    span: span.clone(),
                })
        }
        RuntimeExpr::Call { name, args, span } => {
            // CLIPS expands explicit sequence operands before evaluating the
            // remaining arguments. Keep those ordinary expressions intact so
            // short-circuit calls still decide whether to evaluate them.
            let expanded = expand_call_arguments(ctx, args)?;
            let args = expanded.as_deref().unwrap_or(args);
            if expanded.is_some() {
                validate_expanded_arity(name, args.len(), span.as_ref())?;
            }
            // Per-call events preserve call structure without per-frame spans.
            // Cap deep-call emission to avoid amplifying stack pressure in
            // pathological recursion.
            if ctx.call_depth <= 128 {
                ferric_event!(debug, name = %name, call_depth = ctx.call_depth, "eval_call");
            }
            // call-next-method: advance to next method in the dispatch chain.
            if name == "call-next-method" {
                return dispatch_call_next_method(ctx, args, span.clone());
            }
            // Module-qualified calls (MODULE::name) bypass the builtin dispatch
            // and go directly to the qualified resolution path.
            if is_module_qualified(name) {
                return dispatch_qualified_call(ctx, name, args, span.clone());
            }
            match dispatch_builtin(ctx, name, args, span.clone()) {
                Ok(v) => Ok(v),
                // Only this call's own name falls through to user callables;
                // an unknown name inside a builtin's arguments (a `sort`
                // predicate, a `funcall` target) keeps its own name.
                Err(EvalError::UnknownFunction { name: unknown, .. })
                    if unknown.as_str() == name.as_str() =>
                {
                    let function_modules =
                        sorted_dedup_modules(ctx.engine.functions.modules_for_name(name));
                    if let Some(target_module) = resolve_unqualified_callable_module(
                        ctx,
                        name,
                        "deffunction",
                        &function_modules,
                        ctx.engine.functions.contains(ctx.current_module, name),
                        AmbiguityMessages {
                            expected: "unambiguous deffunction resolution",
                            actual: "multiple visible deffunctions; use MODULE::name",
                        },
                        span.clone(),
                    )? {
                        if let Some(func) = ctx.engine.functions.get(target_module, name).cloned() {
                            return dispatch_user_function(
                                ctx,
                                &func,
                                target_module,
                                args,
                                span.clone(),
                            );
                        }
                    }

                    let generic_modules =
                        sorted_dedup_modules(ctx.engine.generics.modules_for_name(name));
                    if let Some(target_module) = resolve_unqualified_callable_module(
                        ctx,
                        name,
                        "defgeneric",
                        &generic_modules,
                        ctx.engine.generics.contains(ctx.current_module, name),
                        AmbiguityMessages {
                            expected: "unambiguous defgeneric resolution",
                            actual: "multiple visible defgenerics; use MODULE::name",
                        },
                        span.clone(),
                    )? {
                        if let Some(generic) = ctx.engine.generics.get(target_module, name).cloned()
                        {
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
            let cond_value = eval_inner(ctx, condition)?;
            let branch = if is_truthy(&cond_value, &ctx.engine.symbol_table) {
                then_branch
            } else {
                else_branch
            };
            eval_sequence(ctx, branch)
        }
        RuntimeExpr::While {
            condition,
            body,
            span,
        } => {
            loop {
                let cond_value = eval_inner(ctx, condition)?;
                if !is_truthy(&cond_value, &ctx.engine.symbol_table) {
                    break;
                }
                consume_action_loop_iteration(&ctx.engine.config, "while", span.clone())?;
                match eval_sequence(ctx, body) {
                    Ok(_) => {}
                    Err(EvalError::BreakControl { .. }) => break,
                    Err(error) => return Err(error),
                }
            }
            Ok(clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ))
        }
        RuntimeExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            span,
        } => {
            let start_val = eval_inner(ctx, start)?;
            let end_val = eval_inner(ctx, end)?;
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

            let false_val = clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            );

            if start_int > end_int {
                return Ok(false_val);
            }

            // The frame layout is the same for every iteration, and the body
            // cannot change it, so only the counter binding is replaced.
            let mut iter_var_map = ctx.var_map.clone();
            let mut iter_bindings = ctx.bindings.clone();
            let counter_var = match var_name {
                Some(var) => {
                    let sym = ctx
                        .engine
                        .symbol_table
                        .intern_symbol(var, ctx.engine.config.string_encoding)
                        .map_err(|_| EvalError::TypeError {
                            function: "loop-for-count".to_string(),
                            expected: "valid loop variable name".to_string(),
                            actual: var.clone(),
                            span: span.clone(),
                        })?;
                    Some(
                        iter_var_map
                            .get_or_create(sym)
                            .map_err(|_| EvalError::TypeError {
                                function: "loop-for-count".to_string(),
                                expected: "bindable variable".to_string(),
                                actual: var.clone(),
                                span: span.clone(),
                            })?,
                    )
                }
                None => None,
            };

            with_callable_local_scope(ctx, var_name.as_deref(), var_name.as_deref(), |ctx| {
                for counter in start_int..=end_int {
                    consume_action_loop_iteration(
                        &ctx.engine.config,
                        "loop-for-count",
                        span.clone(),
                    )?;
                    if let Some(var_id) = counter_var {
                        iter_bindings.set(var_id, ValueRef::new(Value::Integer(counter)));
                    }
                    let mut iter_ctx = EvalContext {
                        global_module: ctx.global_module,
                        bindings: &iter_bindings,
                        var_map: &iter_var_map,
                        callable_locals: ctx.callable_locals.as_deref_mut(),
                        call_depth: ctx.call_depth,
                        expression_depth: ctx.expression_depth,
                        current_module: ctx.current_module,
                        method_chain: ctx.method_chain.clone(),
                        compact_fact_bindings: ctx.compact_fact_bindings,
                        engine: ctx.engine,
                        allow_engine_effects: ctx.allow_engine_effects,
                    };

                    match eval_sequence(&mut iter_ctx, body) {
                        Ok(_) => {}
                        Err(EvalError::BreakControl { .. }) => break,
                        Err(error) => return Err(error),
                    }
                }
                Ok(())
            })?;
            Ok(false_val)
        }
        RuntimeExpr::Progn {
            var_name,
            list_expr,
            body,
            span,
        } => {
            let list_val = eval_inner(ctx, list_expr)?;
            let elements: Vec<Value> = match list_val {
                Value::Multifield(mf) => mf.as_slice().to_vec(),
                // A scalar value is treated as a single-element multifield.
                other => vec![other],
            };

            let false_val = clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            );
            if elements.is_empty() {
                return Ok(false_val);
            }

            let index_var_name = format!("{var_name}-index");

            // One frame serves every iteration; only the two bindings change.
            let mut iter_var_map = ctx.var_map.clone();
            let mut iter_bindings = ctx.bindings.clone();
            let elem_sym = ctx
                .engine
                .symbol_table
                .intern_symbol(var_name, ctx.engine.config.string_encoding)
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
            let idx_sym = ctx
                .engine
                .symbol_table
                .intern_symbol(&index_var_name, ctx.engine.config.string_encoding)
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

            let mut result = clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            );
            with_callable_local_scope(
                ctx,
                [var_name.as_str(), index_var_name.as_str()],
                Some(var_name),
                |ctx| {
                    for (idx, element) in elements.into_iter().enumerate() {
                        #[allow(clippy::cast_possible_wrap)]
                        // usize→i64: element counts can't exceed i64
                        let one_based = idx as i64 + 1;
                        iter_bindings.set(elem_var_id, ValueRef::new(element));
                        iter_bindings.set(idx_var_id, ValueRef::new(Value::Integer(one_based)));

                        let mut iter_ctx = EvalContext {
                            global_module: ctx.global_module,
                            bindings: &iter_bindings,
                            var_map: &iter_var_map,
                            callable_locals: ctx.callable_locals.as_deref_mut(),
                            call_depth: ctx.call_depth,
                            expression_depth: ctx.expression_depth,
                            current_module: ctx.current_module,
                            method_chain: ctx.method_chain.clone(),
                            compact_fact_bindings: ctx.compact_fact_bindings,
                            engine: ctx.engine,
                            allow_engine_effects: ctx.allow_engine_effects,
                        };

                        match eval_sequence(&mut iter_ctx, body) {
                            Ok(value) => result = value,
                            Err(EvalError::BreakControl { .. }) => {
                                result = Value::Void;
                                break;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    Ok(())
                },
            )?;
            Ok(result)
        }
        RuntimeExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            let disc_value = eval_inner(ctx, expr)?;
            // Find matching case (equality comparison)
            for (test_val_expr, case_body) in cases {
                let test_value = eval_inner(ctx, test_val_expr)?;
                if disc_value.structural_eq(&test_value) {
                    return eval_sequence(ctx, case_body);
                }
            }
            // No case matched; fall to default
            if let Some(default_body) = default {
                eval_sequence(ctx, default_body)
            } else {
                Ok(clips_false(
                    &mut ctx.engine.symbol_table,
                    ctx.engine.config.string_encoding,
                ))
            }
        }
        RuntimeExpr::QueryAction {
            name,
            bindings,
            query,
            body,
            span,
        } => eval_fact_query(ctx, name, bindings, query, body, span.as_ref()),
    }
}

/// Evaluate a structured body while preserving non-local control signals.
fn eval_sequence(
    ctx: &mut EvalContext<'_>,
    body: &[(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)],
) -> Result<Value, EvalError> {
    let mut result = clips_false(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    );
    for (source, compiled) in body {
        result = if let Some(expression) = compiled {
            eval_inner(ctx, expression)?
        } else {
            let expression =
                from_action_expr(source, &mut ctx.engine.symbol_table, &ctx.engine.config)?;
            eval_inner(ctx, &expression)?
        };
    }
    Ok(result)
}

fn query_error(name: &str, actual: impl Into<String>, span: Option<&SourceSpan>) -> EvalError {
    EvalError::TypeError {
        function: name.into(),
        expected: "valid fact-query expression".into(),
        actual: actual.into(),
        span: span.cloned(),
    }
}

pub(crate) fn valid_query_member(name: &str) -> bool {
    use ferric_rules_parser::{lex, FileId, Token};
    !name.is_empty() && lex(&format!("?{name}"), FileId(0)).is_ok_and(|tokens| {
        matches!(tokens.as_slice(), [token] if matches!(&token.token, Token::SingleVar(parsed) if parsed == name))
    })
}

fn validate_query_predicate_body(
    ctx: &mut EvalContext<'_>,
    body: &[(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)],
    name: &str,
    span: Option<&SourceSpan>,
    depth: usize,
) -> Result<(), EvalError> {
    for (action, compiled) in body {
        if let Some(expr) = compiled {
            validate_query_predicate(ctx, expr, name, span, depth)?;
        } else {
            let expr = from_action_expr(action, &mut ctx.engine.symbol_table, &ctx.engine.config)?;
            validate_query_predicate(ctx, &expr, name, span, depth)?;
        }
    }
    Ok(())
}

/// CLIPS rejects local binds syntactically anywhere within a predicate, even
/// in a branch that would not execute. Called function bodies have their own
/// lexical scope and are deliberately not traversed here.
pub(crate) fn validate_query_predicate(
    ctx: &mut EvalContext<'_>,
    expr: &RuntimeExpr,
    name: &str,
    span: Option<&SourceSpan>,
    depth: usize,
) -> Result<(), EvalError> {
    if depth >= MAX_EXPRESSION_DEPTH {
        return Err(EvalError::ExpressionNestingLimit {
            limit: MAX_EXPRESSION_DEPTH,
        });
    }
    let next = depth + 1;
    match expr {
        RuntimeExpr::EffectCall { call } => {
            for argument in
                crate::effects::evaluated_arguments(ctx.engine, ctx.current_module, call)
            {
                let argument =
                    from_action_expr(argument, &mut ctx.engine.symbol_table, &ctx.engine.config)?;
                validate_query_predicate(ctx, &argument, name, span, next)?;
            }
        }
        RuntimeExpr::Literal(_) | RuntimeExpr::BoundVar { .. } | RuntimeExpr::GlobalVar { .. } => {}
        RuntimeExpr::Call {
            name: function,
            args,
            ..
        } => {
            if function == "bind" && !matches!(args.first(), Some(RuntimeExpr::GlobalVar { .. })) {
                return Err(query_error(
                    name,
                    "[FACTQPSR2] local bind is not allowed in a query predicate",
                    span,
                ));
            }
            crate::loader::validate_query_callable(
                function,
                &ctx.engine.functions,
                &ctx.engine.generics,
                &ctx.engine.module_registry,
                ctx.current_module,
                None,
            )
            .map_err(|error| query_error(name, error, span))?;
            for arg in args {
                validate_query_predicate(ctx, arg, name, span, next)?;
            }
        }
        RuntimeExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            validate_query_predicate(ctx, condition, name, span, next)?;
            validate_query_predicate_body(ctx, then_branch, name, span, next)?;
            validate_query_predicate_body(ctx, else_branch, name, span, next)?;
        }
        RuntimeExpr::While {
            condition, body, ..
        } => {
            validate_query_predicate(ctx, condition, name, span, next)?;
            validate_query_predicate_body(ctx, body, name, span, next)?;
        }
        RuntimeExpr::LoopForCount {
            start, end, body, ..
        } => {
            validate_query_predicate(ctx, start, name, span, next)?;
            validate_query_predicate(ctx, end, name, span, next)?;
            validate_query_predicate_body(ctx, body, name, span, next)?;
        }
        RuntimeExpr::Progn {
            list_expr, body, ..
        } => {
            validate_query_predicate(ctx, list_expr, name, span, next)?;
            validate_query_predicate_body(ctx, body, name, span, next)?;
        }
        RuntimeExpr::QueryAction {
            bindings,
            query,
            body,
            ..
        } => {
            for restriction in bindings.iter().flat_map(|binding| &binding.restrictions) {
                validate_query_predicate(ctx, restriction, name, span, next)?;
            }
            validate_query_predicate(ctx, query, name, span, next)?;
            validate_query_predicate_body(ctx, body, name, span, next)?;
        }
        RuntimeExpr::Switch {
            expr,
            cases,
            default,
            ..
        } => {
            validate_query_predicate(ctx, expr, name, span, next)?;
            for (case, body) in cases {
                validate_query_predicate(ctx, case, name, span, next)?;
                validate_query_predicate_body(ctx, body, name, span, next)?;
            }
            if let Some(body) = default {
                validate_query_predicate_body(ctx, body, name, span, next)?;
            }
        }
    }
    Ok(())
}

fn with_expression_query_candidate<T>(
    ctx: &mut EvalContext<'_>,
    candidate: &crate::query_cursor::QueryCandidate,
    execute: impl FnOnce(&mut EvalContext<'_>) -> Result<T, EvalError>,
) -> Result<T, EvalError> {
    let mut var_map = ctx.var_map.clone();
    let mut bindings = ctx.bindings.clone();
    let mut compact_facts = ctx.compact_fact_bindings.cloned().unwrap_or_default();
    for (name, member) in candidate {
        let symbol = ctx
            .engine
            .symbol_table
            .intern_symbol(name, ctx.engine.config.string_encoding)
            .map_err(|error| query_error("query", error.to_string(), None))?;
        let variable = var_map
            .get_or_create(symbol)
            .map_err(|error| query_error("query", error.to_string(), None))?;
        bindings.set(
            variable,
            ValueRef::new(Value::FactAddress(member.address().clone())),
        );
        compact_facts.insert(name.clone(), member.clone());
    }
    with_callable_local_scope(
        ctx,
        candidate.iter().map(|(name, _)| name.as_str()),
        None,
        |ctx| {
            let mut child = EvalContext {
                global_module: ctx.global_module,
                engine: ctx.engine,
                bindings: &bindings,
                var_map: &var_map,
                callable_locals: ctx.callable_locals.as_deref_mut(),
                call_depth: ctx.call_depth,
                expression_depth: ctx.expression_depth,
                current_module: ctx.current_module,
                method_chain: ctx.method_chain.clone(),
                compact_fact_bindings: Some(&compact_facts),
                allow_engine_effects: ctx.allow_engine_effects,
            };
            execute(&mut child)
        },
    )
}

fn eval_fact_query(
    ctx: &mut EvalContext<'_>,
    name: &str,
    members: &[RuntimeQueryBinding],
    predicate: &RuntimeExpr,
    body: &[(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let retained = ctx.engine.active_query_targets.len();
    let result = eval_fact_query_inner(ctx, name, members, predicate, body, span);
    ctx.engine.active_query_targets.truncate(retained);
    result
}

fn eval_fact_query_inner(
    ctx: &mut EvalContext<'_>,
    name: &str,
    members: &[RuntimeQueryBinding],
    predicate: &RuntimeExpr,
    body: &[(ferric_rules_parser::ActionExpr, Option<Box<RuntimeExpr>>)],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let action = matches!(
        name,
        "do-for-fact" | "do-for-all-facts" | "delayed-do-for-all-facts"
    );
    if !action && !matches!(name, "any-factp" | "find-fact" | "find-all-facts") {
        return Err(EvalError::UnsupportedOperation {
            operation: name.into(),
            reason: "unknown fact query".into(),
            span: span.cloned(),
        });
    }
    if members.is_empty() || (!action && !body.is_empty()) {
        return Err(query_error(
            name,
            "a nonempty binding list and no trailing body on result queries are required",
            span,
        ));
    }
    validate_query_predicate(ctx, predicate, name, span, 0)?;
    let members = crate::query_targets::prepare_query_members(members, |expression, span| {
        let value = eval_inner(ctx, expression)?;
        ctx.engine
            .retain_query_targets(&value, ctx.current_module, span)
    })?;
    let mut cursor = crate::query_cursor::ActionQueryCursor::new(members, ctx.engine)?;
    let delayed = name == "delayed-do-for-all-facts";
    let mut selected = Vec::new();
    let mut found = ferric_rules_core::Multifield::new();
    let mut result = clips_false(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    );
    while let Some(candidate) = cursor.next(ctx.engine, name)? {
        let matches = with_expression_query_candidate(ctx, &candidate, |ctx| {
            let value = eval_inner(ctx, predicate)?;
            Ok(is_truthy(&value, &ctx.engine.symbol_table))
        })?;
        if !matches {
            continue;
        }
        if name == "any-factp" {
            return Ok(clips_true(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ));
        }
        if !action {
            found.extend(
                candidate
                    .iter()
                    .map(|(_, member)| Value::FactAddress(member.address().clone())),
            );
            if name == "find-fact" {
                break;
            }
        } else if delayed {
            selected.push(candidate);
        } else {
            match with_expression_query_candidate(ctx, &candidate, |ctx| eval_sequence(ctx, body)) {
                Ok(value) => result = value,
                Err(EvalError::BreakControl { .. }) => return Ok(Value::Void),
                Err(error) => return Err(error),
            }
            if name == "do-for-fact" {
                break;
            }
        }
    }
    for candidate in selected {
        // Each delayed body costs one action-loop iteration, as in the RHS form.
        consume_action_loop_iteration(&ctx.engine.config, name, span.cloned())?;
        match with_expression_query_candidate(ctx, &candidate, |ctx| eval_sequence(ctx, body)) {
            Ok(value) => result = value,
            Err(EvalError::BreakControl { .. }) => return Ok(Value::Void),
            Err(error) => return Err(error),
        }
    }
    Ok(if action || name == "any-factp" {
        result
    } else {
        Value::Multifield(Box::new(found))
    })
}

pub(crate) fn consume_action_loop_iteration(
    config: &EngineConfig,
    function: &str,
    span: Option<SourceSpan>,
) -> Result<(), EvalError> {
    if config.take_action_loop_iteration() {
        Ok(())
    } else {
        Err(EvalError::ActionIterationLimit {
            function: function.to_string(),
            limit: config.max_action_loop_iterations,
            span,
        })
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
        .engine
        .module_registry
        .get_by_name(module_name)
        .ok_or_else(|| EvalError::TypeError {
            function: raw_name.to_string(),
            expected: format!("existing module `{module_name}`"),
            actual: "unknown module".to_string(),
            span: span.clone(),
        })?;

    // Try user-defined function first.
    if let Some(func) = ctx
        .engine
        .functions
        .get(target_module_id, local_name)
        .cloned()
    {
        if !ctx.engine.module_registry.is_construct_visible(
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
    if let Some(generic) = ctx
        .engine
        .generics
        .get(target_module_id, local_name)
        .cloned()
    {
        if !ctx.engine.module_registry.is_construct_visible(
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
        .engine
        .module_registry
        .get_by_name(module_name)
        .ok_or_else(|| EvalError::TypeError {
            function: format!("?*{raw_name}*"),
            expected: format!("existing module `{module_name}`"),
            actual: "unknown module".to_string(),
            span: span.clone(),
        })?;

    if !ctx.engine.globals.contains(target_module_id, local_name) {
        return Err(EvalError::UnboundGlobal {
            name: raw_name.to_string(),
            span,
        });
    }

    if !ctx.engine.module_registry.is_construct_visible(
        global_lookup_module(ctx),
        target_module_id,
        "defglobal",
        local_name,
    ) {
        return Err(EvalError::NotVisible {
            name: format!("?*{raw_name}*"),
            construct_type: "defglobal".to_string(),
            from_module: module_label(ctx, global_lookup_module(ctx)),
            owning_module: module_name.to_string(),
            span,
        });
    }

    ctx.engine
        .globals
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
    body: &[ferric_rules_parser::ActionExpr],
    current_module: crate::modules::ModuleId,
    method_chain: Option<MethodChain>,
) -> Result<Value, EvalError> {
    // Translate body expressions (ActionExpr → RuntimeExpr) BEFORE constructing
    // the inner EvalContext, because from_action_expr also needs &mut symbol_table.
    let mut body_exprs = Vec::with_capacity(body.len());
    for body_expr in body {
        body_exprs.push(from_action_expr(
            body_expr,
            &mut ctx.engine.symbol_table,
            &ctx.engine.config,
        )?);
    }

    // Each invocation has its own overrides, leaving original parameters intact
    // for local unbind and leaving caller locals outside the callable's scope.
    let mut callable_locals = CallableLocals::default();
    // Execute body expressions in an inner frame that inherits shared runtime state.
    let mut inner_ctx = EvalContext {
        global_module: None,
        bindings,
        var_map,
        callable_locals: Some(&mut callable_locals),
        call_depth: ctx.call_depth + 1,
        expression_depth: ctx.expression_depth,
        current_module,
        method_chain,
        compact_fact_bindings: None,
        engine: ctx.engine,
        allow_engine_effects: ctx.allow_engine_effects,
    };

    let mut result = clips_false(
        &mut inner_ctx.engine.symbol_table,
        inner_ctx.engine.config.string_encoding,
    );
    for body_expr in &body_exprs {
        match eval_inner(&mut inner_ctx, body_expr) {
            Ok(value) => result = value,
            Err(EvalError::ReturnControl { value, .. }) => return Ok(value),
            Err(EvalError::BreakControl { span }) => {
                return Err(EvalError::BreakOutsideLoop { span })
            }
            Err(error) => return Err(error),
        }
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
    with_active_callable(ctx, fn_module, &func.name, |ctx| {
        dispatch_user_function_inner(ctx, func, fn_module, args, span)
    })
}

fn with_active_callable<T>(
    ctx: &mut EvalContext<'_>,
    module: crate::modules::ModuleId,
    name: &str,
    evaluate: impl FnOnce(&mut EvalContext<'_>) -> Result<T, EvalError>,
) -> Result<T, EvalError> {
    ctx.engine.active_callables.push((module, name.to_owned()));
    let result = evaluate(ctx);
    ctx.engine.active_callables.pop();
    result
}

fn dispatch_user_function_inner(
    ctx: &mut EvalContext<'_>,
    func: &UserFunction,
    fn_module: crate::modules::ModuleId,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let span_ref = span.as_ref();
    let max_call_depth = ctx.engine.config.effective_max_call_depth();

    // Check recursion limit before doing anything else.
    if ctx.call_depth >= max_call_depth {
        ferric_event!(
            warn,
            callable = %func.name,
            call_depth = ctx.call_depth,
            max_call_depth,
            "eval_recursion_limit_reached"
        );
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
        "INSTANCE-NAME" => matches!(value, Value::InstanceName(_)),
        "MULTIFIELD" => matches!(value, Value::Multifield(_)),
        "EXTERNAL-ADDRESS" => matches!(value, Value::ExternalAddress(_)),
        "FACT-ADDRESS" => matches!(value, Value::FactAddress(_)),
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
        Value::InstanceName(_) => "INSTANCE-NAME",
        Value::Multifield(_) => "MULTIFIELD",
        Value::ExternalAddress(_) => "EXTERNAL-ADDRESS",
        Value::FactAddress(_) => "FACT-ADDRESS",
        Value::Void => "VOID",
    }
}

/// The result of comparing two restrictions, as CLIPS 6.30's
/// `TypeListCompare` and `RestrictionsCompare` report it: the first one
/// outranks the second, is outranked by it, differs without either one
/// outranking the other, or is identical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestrictionPrecedence {
    Higher,
    Lower,
    Different,
    Identical,
}

/// Every superclass of a CLIPS 6.30 system class, as
/// `(class-superclasses <class> inherit)` reports it. Other names have none.
fn type_superclasses(type_name: &str) -> &'static [&'static str] {
    match type_name {
        "PRIMITIVE" | "USER" => &["OBJECT"],
        "NUMBER" | "LEXEME" | "MULTIFIELD" | "ADDRESS" | "INSTANCE" => &["PRIMITIVE", "OBJECT"],
        "INTEGER" | "FLOAT" => &["NUMBER", "PRIMITIVE", "OBJECT"],
        "SYMBOL" | "STRING" => &["LEXEME", "PRIMITIVE", "OBJECT"],
        "EXTERNAL-ADDRESS" | "FACT-ADDRESS" => &["ADDRESS", "PRIMITIVE", "OBJECT"],
        "INSTANCE-ADDRESS" => &["INSTANCE", "ADDRESS", "PRIMITIVE", "OBJECT"],
        "INSTANCE-NAME" => &["INSTANCE", "PRIMITIVE", "OBJECT"],
        "INITIAL-OBJECT" => &["USER", "OBJECT"],
        _ => &[],
    }
}

/// Compare two parameter type lists as CLIPS 6.30's `TypeListCompare` does.
///
/// An empty list (any type) is outranked by any other list. Otherwise, at the
/// first position in written order where one type is a subclass of the other,
/// the subclass wins. Failing that, the shorter list wins. Lists of equal
/// length that differ anywhere are `Different`: neither outranks the other.
fn type_list_compare(a: &[String], b: &[String]) -> RestrictionPrecedence {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return RestrictionPrecedence::Identical,
        (true, false) => return RestrictionPrecedence::Lower,
        (false, true) => return RestrictionPrecedence::Higher,
        (false, false) => {}
    }
    let mut differ = false;
    for (a_type, b_type) in a.iter().zip(b) {
        if a_type != b_type {
            differ = true;
            if type_superclasses(a_type).contains(&b_type.as_str()) {
                return RestrictionPrecedence::Higher;
            }
            if type_superclasses(b_type).contains(&a_type.as_str()) {
                return RestrictionPrecedence::Lower;
            }
        }
    }
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Less => RestrictionPrecedence::Higher,
        std::cmp::Ordering::Greater => RestrictionPrecedence::Lower,
        std::cmp::Ordering::Equal if differ => RestrictionPrecedence::Different,
        std::cmp::Ordering::Equal => RestrictionPrecedence::Identical,
    }
}

/// Whether method `a` has strictly higher dispatch precedence than `b`, as
/// CLIPS 6.30's `RestrictionsCompare` decides when it places a new method.
///
/// The relation is not transitive (a wildcard slot loses at once to a method
/// without a wildcard, while typed slots compare by their type lists), so it
/// must not drive a sort: [`GenericFunction::methods_by_precedence`] uses it to
/// insert methods one at a time, as CLIPS does.
pub(crate) fn method_has_higher_precedence(
    a: &crate::functions::RegisteredMethod,
    b: &crate::functions::RegisteredMethod,
) -> bool {
    compare_method_restrictions(a, b).is_lt()
}

/// Compare two methods' restrictions. Returns `Ordering::Less` if `a` has
/// higher precedence than `b`, `Ordering::Greater` if `b` does, and
/// `Ordering::Equal` when neither outranks the other.
///
/// Following CLIPS 6.30, each method's restriction slots are its fixed
/// parameters followed by its wildcard, compared left to right. A wildcard slot
/// loses at once to a regular parameter of a method that has no wildcard.
/// Otherwise the slots' type lists are compared ([`type_list_compare`]); the
/// first slot whose lists are not identical decides, and lists that differ
/// without either outranking the other leave the methods unranked. When the
/// lists are identical, a query outranks no query. When the shared slots tie,
/// a method without a wildcard wins, then the method with more slots.
fn compare_method_restrictions(
    a: &crate::functions::RegisteredMethod,
    b: &crate::functions::RegisteredMethod,
) -> std::cmp::Ordering {
    let a_slots = method_slot_count(a);
    let b_slots = method_slot_count(b);
    for i in 0..a_slots.min(b_slots) {
        let a_slot = method_slot(a, i);
        let b_slot = method_slot(b, i);
        if a_slot.wildcard && b.wildcard_parameter.is_none() {
            return std::cmp::Ordering::Greater;
        }
        if b_slot.wildcard && a.wildcard_parameter.is_none() {
            return std::cmp::Ordering::Less;
        }
        match type_list_compare(a_slot.types, b_slot.types) {
            RestrictionPrecedence::Higher => return std::cmp::Ordering::Less,
            RestrictionPrecedence::Lower => return std::cmp::Ordering::Greater,
            RestrictionPrecedence::Different => return std::cmp::Ordering::Equal,
            RestrictionPrecedence::Identical => {}
        }
        let order = b_slot.query.cmp(&a_slot.query);
        if !order.is_eq() {
            return order;
        }
    }
    a.wildcard_parameter
        .is_some()
        .cmp(&b.wildcard_parameter.is_some())
        .then_with(|| b_slots.cmp(&a_slots))
}

/// One restriction slot of a method: a fixed parameter or the wildcard.
struct MethodSlot<'a> {
    wildcard: bool,
    types: &'a [String],
    query: bool,
}

fn method_slot_count(method: &crate::functions::RegisteredMethod) -> usize {
    method.parameters.len() + usize::from(method.wildcard_parameter.is_some())
}

fn method_slot(method: &crate::functions::RegisteredMethod, slot: usize) -> MethodSlot<'_> {
    if slot < method.parameters.len() {
        MethodSlot {
            wildcard: false,
            types: method
                .type_restrictions
                .get(slot)
                .map_or(&[][..], Vec::as_slice),
            query: method
                .parameter_queries
                .get(slot)
                .is_some_and(Option::is_some),
        }
    } else {
        MethodSlot {
            wildcard: true,
            types: &method.wildcard_type_restrictions,
            query: method.wildcard_query.is_some(),
        }
    }
}

/// Prefilter dispatch candidates. A method with a query keeps its type checks
/// for selection, where they interleave with the queries in argument order;
/// a method without one has no observable checks, so its types filter here.
fn method_may_apply(method: &crate::functions::RegisteredMethod, arg_values: &[Value]) -> bool {
    let fixed_count = method.parameters.len();
    let arity = if method.wildcard_parameter.is_some() {
        arg_values.len() >= fixed_count
    } else {
        arg_values.len() == fixed_count
    };
    if !arity
        || method.parameter_queries.iter().any(Option::is_some)
        || method.wildcard_query.is_some()
    {
        return arity;
    }
    arg_values.iter().enumerate().all(|(i, value)| {
        let types = if i < fixed_count {
            method
                .type_restrictions
                .get(i)
                .map_or(&[][..], Vec::as_slice)
        } else {
            method.wildcard_type_restrictions.as_slice()
        };
        types.is_empty() || types.iter().any(|kind| value_matches_type(value, kind))
    })
}

struct SelectedMethod {
    index: usize,
    var_map: VarMap,
    bindings: BindingSet,
}

/// Evaluate one restriction query with every parameter bound. Caller locals
/// and method chains are not visible in a restriction query's scope.
fn method_query_matches(
    ctx: &mut EvalContext<'_>,
    query: &ferric_rules_parser::ActionExpr,
    module: crate::modules::ModuleId,
    var_map: &VarMap,
    bindings: &BindingSet,
) -> Result<bool, EvalError> {
    let mut locals = CallableLocals::default();
    let mut query_ctx = EvalContext {
        global_module: None,
        bindings,
        var_map,
        callable_locals: Some(&mut locals),
        call_depth: ctx.call_depth + 1,
        expression_depth: ctx.expression_depth,
        current_module: module,
        method_chain: None,
        compact_fact_bindings: None,
        engine: ctx.engine,
        allow_engine_effects: ctx.allow_engine_effects,
    };
    let expression = from_action_expr(
        query,
        &mut query_ctx.engine.symbol_table,
        &query_ctx.engine.config,
    )?;
    let value = finish_root_evaluation(eval_inner(&mut query_ctx, &expression))?;
    Ok(is_truthy(&value, &query_ctx.engine.symbol_table))
}

/// Walk the original arguments in order, as CLIPS 6.30 does: each argument's
/// type is checked against its restriction, then that restriction's query
/// runs, stopping at the first failure. Excess arguments reuse the wildcard
/// restriction, so its query runs once per excess argument (never when there
/// are none). All parameters are bound before the first query runs, so a
/// query can reference later parameters and the flattened wildcard.
fn method_applies(
    ctx: &mut EvalContext<'_>,
    chain: &MethodChain,
    method: &crate::functions::RegisteredMethod,
    bound: &mut Option<(VarMap, BindingSet)>,
    span: Option<&SourceSpan>,
) -> Result<bool, EvalError> {
    let fixed_count = method.parameters.len();
    for (i, value) in chain.arg_values.iter().enumerate() {
        let (types, query) = if i < fixed_count {
            (
                method
                    .type_restrictions
                    .get(i)
                    .map_or(&[][..], Vec::as_slice),
                method.parameter_queries.get(i).and_then(Option::as_ref),
            )
        } else {
            (
                method.wildcard_type_restrictions.as_slice(),
                method.wildcard_query.as_ref(),
            )
        };
        if !types.is_empty() && !types.iter().any(|kind| value_matches_type(value, kind)) {
            return Ok(false);
        }
        let Some(query) = query else {
            continue;
        };
        let (var_map, bindings) = match bound {
            Some(bound) => bound,
            None => bound.insert(bind_callable_arguments(
                ctx,
                &chain.generic_name,
                &method.parameters,
                method.wildcard_parameter.as_deref(),
                &chain.arg_values,
                span,
            )?),
        };
        if !method_query_matches(ctx, query, chain.generic_module, var_map, bindings)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn select_method(
    ctx: &mut EvalContext<'_>,
    chain: &MethodChain,
    start: usize,
    span: Option<&SourceSpan>,
) -> Result<Option<SelectedMethod>, EvalError> {
    for (index, method) in chain.candidate_methods.iter().enumerate().skip(start) {
        // Replacement arguments from override-next-method can change arity
        // and type applicability, so the chain holds every method.
        if !method_may_apply(method, &chain.arg_values) {
            continue;
        }
        let mut bound = None;
        if !method_applies(ctx, chain, method, &mut bound, span)? {
            continue;
        }
        let (var_map, bindings) = match bound {
            Some(bound) => bound,
            None => bind_callable_arguments(
                ctx,
                &chain.generic_name,
                &method.parameters,
                method.wildcard_parameter.as_deref(),
                &chain.arg_values,
                span,
            )?,
        };
        return Ok(Some(SelectedMethod {
            index,
            var_map,
            bindings,
        }));
    }
    Ok(None)
}

fn no_applicable_method(name: &str, arg_values: &[Value], span: Option<SourceSpan>) -> EvalError {
    ferric_event!(
        debug,
        callable = name,
        arg_count = arg_values.len(),
        "dispatch_generic_no_applicable_method"
    );
    EvalError::NoApplicableMethod {
        name: name.to_owned(),
        actual_types: arg_values
            .iter()
            .map(generic_value_type_name)
            .collect::<Vec<_>>()
            .join(", "),
        span,
    }
}

/// Dispatch a call after evaluating arguments once. Restriction queries, and
/// the type checks of methods that have them, run in argument order as
/// selection reaches each candidate, throughout the call-next-method chain.
fn dispatch_generic(
    ctx: &mut EvalContext<'_>,
    generic: &GenericFunction,
    generic_module: crate::modules::ModuleId,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let arg_values = eval_args(ctx, args)?;
    // Argument effects may legally replace a generic's methods before it is
    // executing. Selection observes the current definition after those effects.
    let generic = ctx
        .engine
        .generics
        .get(generic_module, &generic.name)
        .cloned()
        .unwrap_or_else(|| generic.clone());
    with_active_callable(ctx, generic_module, &generic.name, |ctx| {
        dispatch_generic_values(ctx, &generic, generic_module, arg_values, span)
    })
}

fn dispatch_generic_values(
    ctx: &mut EvalContext<'_>,
    generic: &GenericFunction,
    generic_module: crate::modules::ModuleId,
    arg_values: Vec<Value>,
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    if !generic
        .methods_by_precedence()
        .any(|method| method_may_apply(method, &arg_values))
    {
        return Err(no_applicable_method(&generic.name, &arg_values, span));
    }
    let candidates: Vec<_> = generic.methods_by_precedence().cloned().collect();
    let max_call_depth = ctx.engine.config.effective_max_call_depth();
    if ctx.call_depth >= max_call_depth {
        ferric_event!(warn, callable = %generic.name, call_depth = ctx.call_depth, max_call_depth, "eval_recursion_limit_reached");
        return Err(EvalError::RecursionLimit {
            name: generic.name.clone(),
            depth: ctx.call_depth,
            span,
        });
    }
    let mut chain = MethodChain {
        generic_name: generic.name.clone(),
        generic_module,
        candidate_methods: candidates,
        current_index: 0,
        arg_values,
    };
    let selected = select_method(ctx, &chain, 0, span.as_ref())?
        .ok_or_else(|| no_applicable_method(&generic.name, &chain.arg_values, span))?;
    chain.current_index = selected.index;
    let method = chain.candidate_methods[selected.index].clone();
    execute_callable_body(
        ctx,
        &selected.var_map,
        &selected.bindings,
        &method.body,
        generic_module,
        Some(chain),
    )
}

/// Each call-next-method invocation searches again from its caller's position,
/// retaining original arguments and re-evaluating the reached query expressions.
fn dispatch_call_next_method(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::ArityMismatch {
            name: "call-next-method".to_string(),
            expected: "0".to_string(),
            actual: args.len(),
            span,
        });
    }
    let Some(mut chain) = ctx.method_chain.clone() else {
        return Err(EvalError::TypeError {
            function: "call-next-method".to_string(),
            expected: "called from within a generic method body".to_string(),
            actual: "called outside generic dispatch context".to_string(),
            span,
        });
    };
    let max_call_depth = ctx.engine.config.effective_max_call_depth();
    if ctx.call_depth >= max_call_depth {
        ferric_event!(warn, callable = %chain.generic_name, call_depth = ctx.call_depth, max_call_depth, "eval_recursion_limit_reached");
        return Err(EvalError::RecursionLimit {
            name: format!("call-next-method for `{}`", chain.generic_name),
            depth: ctx.call_depth,
            span,
        });
    }
    let selected = select_method(ctx, &chain, chain.current_index + 1, span.as_ref())?
        .ok_or_else(|| {
            ferric_event!(debug, callable = %chain.generic_name, current_index = chain.current_index, "call_next_method_missing_next");
            EvalError::NoApplicableMethod {
                name: format!("call-next-method for `{}`", chain.generic_name),
                actual_types: "no next method in dispatch chain".to_string(), span,
            }
        })?;
    chain.current_index = selected.index;
    let method = chain.candidate_methods[selected.index].clone();
    execute_callable_body(
        ctx,
        &selected.var_map,
        &selected.bindings,
        &method.body,
        chain.generic_module,
        Some(chain),
    )
}

/// Generic control operations preserve the caller's chain and argument values.
pub(crate) fn dispatch_method_control(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    match name {
        "next-methodp" => {
            check_arity_exact(name, args, 0, span.as_ref())?;
            let applicable = if let Some(chain) = ctx.method_chain.clone() {
                select_method(ctx, &chain, chain.current_index + 1, span.as_ref())?.is_some()
            } else {
                false
            };
            Ok(clips_bool(
                applicable,
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ))
        }
        "override-next-method" => {
            let mut chain = ctx
                .method_chain
                .clone()
                .ok_or_else(|| EvalError::TypeError {
                    function: name.to_owned(),
                    expected: "called from within a generic method body".to_owned(),
                    actual: "called outside generic dispatch context".to_owned(),
                    span: span.clone(),
                })?;
            chain.arg_values = eval_args(ctx, args)?;
            let selected = select_method(ctx, &chain, chain.current_index + 1, span.as_ref())?
                .ok_or_else(|| {
                    no_applicable_method(&chain.generic_name, &chain.arg_values, span.clone())
                })?;
            invoke_control_method(ctx, chain, &selected, span)
        }
        "call-specific-method" => dispatch_specific_method(ctx, args, span),
        _ => unreachable!("unknown generic control operation"),
    }
}

fn invoke_control_method(
    ctx: &mut EvalContext<'_>,
    mut chain: MethodChain,
    selected: &SelectedMethod,
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    if ctx.call_depth >= ctx.engine.config.effective_max_call_depth() {
        return Err(EvalError::RecursionLimit {
            name: chain.generic_name.clone(),
            depth: ctx.call_depth,
            span,
        });
    }
    chain.current_index = selected.index;
    let method = chain.candidate_methods[selected.index].clone();
    execute_callable_body(
        ctx,
        &selected.var_map,
        &selected.bindings,
        &method.body,
        chain.generic_module,
        Some(chain),
    )
}

fn specific_generic(
    ctx: &EvalContext<'_>,
    raw: &str,
    span: Option<&SourceSpan>,
) -> Result<(crate::modules::ModuleId, GenericFunction), EvalError> {
    let missing = || EvalError::UnsupportedOperation {
        operation: "call-specific-method".to_owned(),
        reason: format!("unable to find generic function `{raw}`"),
        span: span.cloned(),
    };
    let parsed = parse_qualified_name(raw).map_err(|_| missing())?;
    let (module, local) = match &parsed {
        QualifiedName::Qualified { module, name } => {
            let owner = ctx
                .engine
                .module_registry
                .get_by_name(module)
                .ok_or_else(missing)?;
            (owner, name.as_str())
        }
        QualifiedName::Unqualified(name) => {
            let owners = ctx.engine.generics.modules_for_name(name);
            let owner = resolve_unqualified_callable_module(
                ctx,
                name,
                "defgeneric",
                &owners,
                ctx.engine.generics.contains(ctx.current_module, name),
                AmbiguityMessages {
                    expected: "unambiguous generic function",
                    actual: "multiple visible generic functions",
                },
                span.cloned(),
            )?
            .ok_or_else(missing)?;
            (owner, name.as_str())
        }
    };
    let generic = ctx
        .engine
        .generics
        .get(module, local)
        .cloned()
        .ok_or_else(missing)?;
    Ok((module, generic))
}

fn dispatch_specific_method(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let name = "call-specific-method";
    check_arity_min(name, args, 2, span.as_ref())?;
    let value = eval_inner(ctx, &args[0])?;
    let Value::Symbol(symbol) = value else {
        return Err(EvalError::TypeError {
            function: name.to_owned(),
            expected: "SYMBOL generic name".to_owned(),
            actual: value.type_name().to_owned(),
            span,
        });
    };
    let raw = ctx
        .engine
        .symbol_table
        .resolve_symbol_str(symbol)
        .unwrap_or("")
        .to_owned();
    // Resolve each selector before evaluating operands that CLIPS would skip on failure.
    let (module, generic) = specific_generic(ctx, &raw, span.as_ref())?;
    let value = eval_inner(ctx, &args[1])?;
    let Value::Integer(index) = value else {
        return Err(EvalError::TypeError {
            function: name.to_owned(),
            expected: "INTEGER method index".to_owned(),
            actual: value.type_name().to_owned(),
            span,
        });
    };
    // Selector effects run before the generic is executing and can add methods.
    let generic = ctx
        .engine
        .generics
        .get(module, &generic.name)
        .cloned()
        .unwrap_or(generic);
    let methods: Vec<_> = generic.methods_by_precedence().cloned().collect();
    let position = methods
        .iter()
        .position(|method| i64::from(method.index) == index)
        .ok_or_else(|| EvalError::UnsupportedOperation {
            operation: name.to_owned(),
            reason: format!("unable to find method `{raw}` #{index}"),
            span: span.clone(),
        })?;
    with_active_callable(ctx, module, &generic.name, |ctx| {
        let values = eval_args(ctx, &args[2..])?;
        let chain = MethodChain {
            generic_name: generic.name.clone(),
            generic_module: module,
            candidate_methods: methods,
            current_index: position,
            arg_values: values,
        };
        let method = &chain.candidate_methods[position];
        let mut bound = None;
        if !method_may_apply(method, &chain.arg_values)
            || !method_applies(ctx, &chain, method, &mut bound, span.as_ref())?
        {
            return Err(no_applicable_method(&raw, &chain.arg_values, span));
        }
        let (var_map, bindings) = match bound {
            Some(bound) => bound,
            None => bind_callable_arguments(
                ctx,
                &raw,
                &method.parameters,
                method.wildcard_parameter.as_deref(),
                &chain.arg_values,
                span.as_ref(),
            )?,
        };
        invoke_control_method(
            ctx,
            chain,
            &SelectedMethod {
                index: position,
                var_map,
                bindings,
            },
            span,
        )
    })
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
        let mut extra_values = ferric_rules_core::Multifield::new();
        for value in &arg_values[parameters.len()..] {
            match value {
                Value::Multifield(fields) => {
                    for field in fields.iter() {
                        extra_values.push(field.clone());
                    }
                }
                value => extra_values.push(value.clone()),
            }
        }
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
        .engine
        .symbol_table
        .intern_symbol(parameter_name, ctx.engine.config.string_encoding)
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
    bindings.set(var_id, ValueRef::new(value));
    Ok(())
}

// ---------------------------------------------------------------------------
// Translation: ActionExpr -> RuntimeExpr
// ---------------------------------------------------------------------------

pub(crate) fn validate_action_depth(
    root: &ferric_rules_parser::ActionExpr,
) -> Result<(), EvalError> {
    use ferric_rules_parser::ActionExpr;
    let mut pending = vec![(root, 0)];
    while let Some((expr, depth)) = pending.pop() {
        if depth >= 16 {
            return Err(EvalError::ExpressionNestingLimit { limit: 16 });
        }
        let mut branches = Vec::new();
        match expr {
            ActionExpr::Literal(_) | ActionExpr::Variable(..) | ActionExpr::GlobalVariable(..) => {}
            ActionExpr::FunctionCall(call) => branches.push(&call.args),
            ActionExpr::If {
                condition,
                then_actions,
                else_actions,
                ..
            } => {
                pending.push((condition, depth + 1));
                branches.extend([then_actions, else_actions]);
            }
            ActionExpr::While {
                condition, body, ..
            } => {
                pending.push((condition, depth + 1));
                branches.push(body);
            }
            ActionExpr::LoopForCount {
                start, end, body, ..
            } => {
                pending.extend([(start.as_ref(), depth + 1), (end.as_ref(), depth + 1)]);
                branches.push(body);
            }
            ActionExpr::Progn {
                list_expr, body, ..
            } => {
                pending.push((list_expr, depth + 1));
                branches.push(body);
            }
            ActionExpr::QueryAction {
                bindings,
                query,
                body,
                ..
            } => {
                pending.extend(
                    bindings
                        .iter()
                        .flat_map(|binding| &binding.restrictions)
                        .map(|expression| (expression, depth + 1)),
                );
                pending.push((query, depth + 1));
                branches.push(body);
            }
            ActionExpr::Switch {
                expr,
                cases,
                default,
                ..
            } => {
                pending.push((expr, depth + 1));
                for (case, body) in cases {
                    pending.push((case, depth + 1));
                    branches.push(body);
                }
                branches.extend(default.iter());
            }
        }
        for branch in branches {
            pending.extend(branch.iter().map(|expr| (expr, depth + 1)));
        }
    }
    Ok(())
}

/// Translate a parser expression after bounding recursive conversion and cloning.
pub fn from_action_expr(
    expr: &ferric_rules_parser::ActionExpr,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    validate_action_depth(expr)?;
    from_action_expr_inner(expr, symbol_table, config)
}

/// Translate a parser `ActionExpr` to a `RuntimeExpr`.
#[allow(clippy::too_many_lines)] // Each new loop form adds ~20 lines of translation boilerplate
fn from_action_expr_inner(
    expr: &ferric_rules_parser::ActionExpr,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    match expr {
        ferric_rules_parser::ActionExpr::Literal(lit) => {
            let value = literal_to_value(&lit.value, symbol_table, config)?;
            Ok(RuntimeExpr::Literal(value))
        }
        ferric_rules_parser::ActionExpr::Variable(name, span) => Ok(RuntimeExpr::BoundVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_rules_parser::ActionExpr::GlobalVariable(name, span) => Ok(RuntimeExpr::GlobalVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_rules_parser::ActionExpr::FunctionCall(call) => {
            if matches!(call.name.as_str(), "assert" | "modify" | "duplicate") {
                // Fact and slot heads translate as plain calls, so this only
                // surfaces literal encoding errors at load time. The effect
                // itself evaluates the raw syntax.
                for arg in &call.args {
                    from_action_expr_inner(arg, symbol_table, config)?;
                }
                return Ok(RuntimeExpr::EffectCall {
                    call: Box::new(call.clone()),
                });
            }
            let mut args = Vec::with_capacity(call.args.len());
            for arg in &call.args {
                args.push(from_action_expr_inner(arg, symbol_table, config)?);
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
        ferric_rules_parser::ActionExpr::If {
            condition,
            then_actions,
            else_actions,
            span,
        } => {
            let condition_rt = from_action_expr_inner(condition, symbol_table, config)?;
            let mut then_branch = Vec::with_capacity(then_actions.len());
            for a in then_actions {
                let rt = from_action_expr_inner(a, symbol_table, config)
                    .ok()
                    .map(Box::new);
                then_branch.push((a.clone(), rt));
            }
            let mut else_branch = Vec::with_capacity(else_actions.len());
            for a in else_actions {
                let rt = from_action_expr_inner(a, symbol_table, config)
                    .ok()
                    .map(Box::new);
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
        ferric_rules_parser::ActionExpr::While {
            condition,
            body,
            span,
        } => {
            let condition_rt = from_action_expr_inner(condition, symbol_table, config)?;
            let mut body_rt = Vec::with_capacity(body.len());
            for a in body {
                let rt = from_action_expr_inner(a, symbol_table, config)
                    .ok()
                    .map(Box::new);
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
        ferric_rules_parser::ActionExpr::LoopForCount {
            var_name,
            start,
            end,
            body,
            span,
        } => {
            let start_rt = from_action_expr_inner(start, symbol_table, config)?;
            let end_rt = from_action_expr_inner(end, symbol_table, config)?;
            let mut body_rt = Vec::with_capacity(body.len());
            for a in body {
                let rt = from_action_expr_inner(a, symbol_table, config)
                    .ok()
                    .map(Box::new);
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
        ferric_rules_parser::ActionExpr::Progn {
            var_name,
            list_expr,
            body,
            span,
        } => {
            let list_rt = from_action_expr_inner(list_expr, symbol_table, config)?;
            let mut body_rt = Vec::with_capacity(body.len());
            for a in body {
                let rt = from_action_expr_inner(a, symbol_table, config)
                    .ok()
                    .map(Box::new);
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
        ferric_rules_parser::ActionExpr::QueryAction {
            name,
            bindings,
            query,
            body,
            span,
        } => {
            let query_rt = from_action_expr_inner(query, symbol_table, config)?;
            let mut body_rt = Vec::with_capacity(body.len());
            for a in body {
                let rt = from_action_expr_inner(a, symbol_table, config)
                    .ok()
                    .map(Box::new);
                body_rt.push((a.clone(), rt));
            }
            Ok(RuntimeExpr::QueryAction {
                name: name.clone(),
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        Ok(RuntimeQueryBinding {
                            variable: binding.variable.clone(),
                            restrictions: binding
                                .restrictions
                                .iter()
                                .map(|expression| {
                                    from_action_expr_inner(expression, symbol_table, config)
                                })
                                .collect::<Result<_, EvalError>>()?,
                            span: Some(SourceSpan {
                                line: binding.span.start.line,
                                column: binding.span.start.column,
                            }),
                        })
                    })
                    .collect::<Result<_, EvalError>>()?,
                query: Box::new(query_rt),
                body: body_rt,
                span: Some(SourceSpan {
                    line: span.start.line,
                    column: span.start.column,
                }),
            })
        }
        ferric_rules_parser::ActionExpr::Switch {
            expr,
            cases,
            default,
            span,
        } => {
            let expr_rt = from_action_expr_inner(expr, symbol_table, config)?;
            let mut cases_rt = Vec::with_capacity(cases.len());
            for (test_val, actions) in cases {
                let test_rt = from_action_expr_inner(test_val, symbol_table, config)?;
                let mut actions_rt = Vec::with_capacity(actions.len());
                for a in actions {
                    let rt = from_action_expr_inner(a, symbol_table, config)
                        .ok()
                        .map(Box::new);
                    actions_rt.push((a.clone(), rt));
                }
                cases_rt.push((test_rt, actions_rt));
            }
            let default_rt = default.as_ref().map(|actions| {
                actions
                    .iter()
                    .map(|a| {
                        let rt = from_action_expr_inner(a, symbol_table, config)
                            .ok()
                            .map(Box::new);
                        (a.clone(), rt)
                    })
                    .collect()
            });
            Ok(RuntimeExpr::Switch {
                expr: Box::new(expr_rt),
                cases: cases_rt,
                default: default_rt,
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
    expr: &ferric_rules_parser::SExpr,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    let mut pending = vec![(expr, 0)];
    while let Some((value, depth)) = pending.pop() {
        if depth >= 16 {
            return Err(EvalError::ExpressionNestingLimit { limit: 16 });
        }
        if let ferric_rules_parser::SExpr::List(items, _) = value {
            pending.extend(items.iter().map(|value| (value, depth + 1)));
        }
    }
    from_sexpr_inner(expr, symbol_table, config)
}

fn from_sexpr_inner(
    expr: &ferric_rules_parser::SExpr,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    match expr {
        ferric_rules_parser::SExpr::List(items, span) => {
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
                ferric_rules_parser::SExpr::Atom(ferric_rules_parser::Atom::Symbol(s), _) => {
                    s.clone()
                }
                ferric_rules_parser::SExpr::Atom(
                    ferric_rules_parser::Atom::Connective(connective),
                    _,
                ) => connective_to_function_name(*connective).to_string(),
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
            if matches!(func_name.as_str(), "assert" | "modify" | "duplicate") {
                let action = ferric_rules_parser::interpret_action_expr(expr).map_err(|error| {
                    EvalError::UnsupportedOperation {
                        operation: func_name,
                        reason: error.to_string(),
                        span: Some(SourceSpan {
                            line: span.start.line,
                            column: span.start.column,
                        }),
                    }
                })?;
                return from_action_expr_inner(&action, symbol_table, config);
            }
            for item in &items[1..] {
                args.push(from_sexpr_inner(item, symbol_table, config)?);
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
        ferric_rules_parser::SExpr::Atom(atom, span) => {
            sexpr_atom_to_runtime(atom, span, symbol_table, config)
        }
    }
}

/// Convert a raw S-expression atom into a `RuntimeExpr`.
fn sexpr_atom_to_runtime(
    atom: &ferric_rules_parser::Atom,
    span: &ferric_rules_parser::Span,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<RuntimeExpr, EvalError> {
    match atom {
        ferric_rules_parser::Atom::Integer(n) => Ok(RuntimeExpr::Literal(Value::Integer(*n))),
        ferric_rules_parser::Atom::Float(f) => Ok(RuntimeExpr::Literal(Value::Float(*f))),
        ferric_rules_parser::Atom::String(s) => {
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
        ferric_rules_parser::Atom::Symbol(s) | ferric_rules_parser::Atom::InstanceName(s) => {
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
            Ok(RuntimeExpr::Literal(
                if matches!(atom, ferric_rules_parser::Atom::InstanceName(_)) {
                    Value::InstanceName(InstanceName::from_symbol(sym))
                } else {
                    Value::Symbol(sym)
                },
            ))
        }
        ferric_rules_parser::Atom::SingleVar(name) => Ok(RuntimeExpr::BoundVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_rules_parser::Atom::MultiVar(name) => Ok(RuntimeExpr::BoundVar {
            name: format!("$?{name}"),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_rules_parser::Atom::GlobalVar(name) => Ok(RuntimeExpr::GlobalVar {
            name: name.clone(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
        ferric_rules_parser::Atom::Connective(_) => Err(EvalError::UnknownFunction {
            name: "connective".to_string(),
            span: Some(SourceSpan {
                line: span.start.line,
                column: span.start.column,
            }),
        }),
    }
}

/// Map a parser connective to its corresponding builtin function name.
fn connective_to_function_name(connective: ferric_rules_parser::Connective) -> &'static str {
    match connective {
        ferric_rules_parser::Connective::And => "and",
        ferric_rules_parser::Connective::Or => "or",
        ferric_rules_parser::Connective::Not => "not",
        ferric_rules_parser::Connective::Equals => "=",
        ferric_rules_parser::Connective::Colon => ":",
        ferric_rules_parser::Connective::Assign => "<-",
    }
}

// ---------------------------------------------------------------------------
// Literal conversion helper
// ---------------------------------------------------------------------------

/// Convert a `LiteralKind` to a runtime `Value`.
fn literal_to_value(
    lit: &ferric_rules_parser::LiteralKind,
    symbol_table: &mut SymbolTable,
    config: &EngineConfig,
) -> Result<Value, EvalError> {
    match lit {
        ferric_rules_parser::LiteralKind::Integer(n) => Ok(Value::Integer(*n)),
        ferric_rules_parser::LiteralKind::Float(f) => Ok(Value::Float(*f)),
        ferric_rules_parser::LiteralKind::String(s) => {
            let fs =
                FerricString::new(s, config.string_encoding).map_err(|_| EvalError::TypeError {
                    function: "literal".to_string(),
                    expected: "valid string".to_string(),
                    actual: format!("encoding error for {s:?}"),
                    span: None,
                })?;
            Ok(Value::String(fs))
        }
        ferric_rules_parser::LiteralKind::Symbol(s)
        | ferric_rules_parser::LiteralKind::InstanceName(s) => {
            let sym = symbol_table
                .intern_symbol(s, config.string_encoding)
                .map_err(|_| EvalError::TypeError {
                    function: "literal".to_string(),
                    expected: "valid symbol".to_string(),
                    actual: format!("encoding error for {s:?}"),
                    span: None,
                })?;
            Ok(
                if matches!(lit, ferric_rules_parser::LiteralKind::InstanceName(_)) {
                    Value::InstanceName(InstanceName::from_symbol(sym))
                } else {
                    Value::Symbol(sym)
                },
            )
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

/// Extract a numeric value from a `Value` as `f64`, or return a type error.
#[allow(clippy::cast_precision_loss)]
fn as_float(v: &Value, function: &str, span: Option<&SourceSpan>) -> Result<f64, EvalError> {
    match v {
        Value::Integer(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
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

/// Returns true if `name` is a supported evaluator builtin callable.
#[allow(clippy::too_many_lines)]
pub(crate) fn is_builtin_callable(name: &str) -> bool {
    if crate::introspection::is_builtin(name)
        || matches!(
            name,
            "next-methodp" | "call-specific-method" | "override-next-method"
        )
    {
        return true;
    }
    if crate::environment::is_builtin(name) {
        return true;
    }
    matches!(
        name,
        "+" | "-"
            | "*"
            | "/"
            | "div"
            | "mod"
            | "abs"
            | "min"
            | "max"
            | ">"
            | "<"
            | ">="
            | "<="
            | "="
            | "!="
            | "<>"
            | "eq"
            | "neq"
            | "and"
            | "or"
            | "not"
            | "integerp"
            | "floatp"
            | "numberp"
            | "symbolp"
            | "stringp"
            | "lexemep"
            | "instance-namep"
            | "symbol-to-instance-name"
            | "instance-name-to-symbol"
            | "multifieldp"
            | "evenp"
            | "oddp"
            | "integer"
            | "float"
            | "str-cat"
            | "sym-cat"
            | "gensym"
            | "gensym*"
            | "setgen"
            | "set-fact-duplication"
            | "get-fact-duplication"
            | "refresh-agenda"
            | "watch"
            | "unwatch"
            | "str-length"
            | "sub-string"
            | "create$"
            | "expand$"
            | "delete-member$"
            | "replace-member$"
            | "progn"
            | "length"
            | "length$"
            | "subseq$"
            | "nth"
            | "implode$"
            | "member"
            | "nth$"
            | "member$"
            | "subsetp"
            | "printout"
            | "format"
            | "read"
            | "readline"
            | "get-focus"
            | "get-focus-stack"
            | "close"
            | "return"
            | "break"
            | "load"
            | "undefrule"
            | "ppdefrule"
            | "rules"
            | "bind"
            | "sqrt"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asinh"
            | "acosh"
            | "atanh"
            | "exp"
            | "log"
            | "log10"
            | "**"
            | "pi"
            | "round"
            | "ceiling"
            | "floor"
            | "deg-rad"
            | "rad-deg"
            | "deg-grad"
            | "grad-deg"
            | "str-index"
            | "upcase"
            | "lowcase"
            | "str-compare"
            | "string-to-field"
            | "explode$"
            | "str-explode"
            | "insert$"
            | "delete$"
            | "replace$"
            | "first$"
            | "rest$"
            | "sort"
            | "funcall"
            | "assert"
            | "retract"
            | "modify"
            | "duplicate"
            | "halt"
            | "focus"
            | "reset"
            | "clear"
            | "fact-existp"
            | "fact-index"
            | "fact-relation"
            | "fact-slot-value"
            | "fact-slot-names"
            | COMPACT_FACT_SLOT_REF
            | "load-facts"
            | "save-facts"
    )
}

/// Dispatch a built-in function call.
#[allow(clippy::too_many_lines)]
fn dispatch_builtin(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<SourceSpan>,
) -> Result<Value, EvalError> {
    let span_ref = span.as_ref();
    if crate::introspection::is_builtin(name) {
        return crate::introspection::eval(ctx, name, args, span_ref);
    }
    if matches!(
        name,
        "next-methodp" | "call-specific-method" | "override-next-method"
    ) {
        return dispatch_method_control(ctx, name, args, span);
    }
    if crate::environment::is_builtin(name) {
        return crate::environment::eval(ctx, name, args, span_ref);
    }
    match name {
        "retract" | "halt" | "focus" | "reset" | "clear" | "load-facts" | "save-facts" => {
            crate::effects::eval_call(ctx, name, args, span_ref)
        }
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

        // Transcendental math
        "sqrt" => builtin_sqrt(ctx, args, span_ref),
        "sin" => builtin_sin(ctx, args, span_ref),
        "cos" => builtin_cos(ctx, args, span_ref),
        "tan" => builtin_tan(ctx, args, span_ref),
        "asin" => builtin_asin(ctx, args, span_ref),
        "acos" => builtin_acos(ctx, args, span_ref),
        "atan" => builtin_atan(ctx, args, span_ref),
        "atan2" => builtin_atan2(ctx, args, span_ref),
        "sinh" => builtin_sinh(ctx, args, span_ref),
        "cosh" => builtin_cosh(ctx, args, span_ref),
        "tanh" => builtin_tanh(ctx, args, span_ref),
        "asinh" => builtin_asinh(ctx, args, span_ref),
        "acosh" => builtin_acosh(ctx, args, span_ref),
        "atanh" => builtin_atanh(ctx, args, span_ref),
        "exp" => builtin_exp(ctx, args, span_ref),
        "log" => builtin_log(ctx, args, span_ref),
        "log10" => builtin_log10(ctx, args, span_ref),
        "**" => builtin_pow(ctx, args, span_ref),
        "pi" => builtin_pi(ctx, args, span_ref),
        "round" => builtin_round(ctx, args, span_ref),
        "ceiling" => builtin_ceiling(ctx, args, span_ref),
        "floor" => builtin_floor(ctx, args, span_ref),
        "deg-rad" => builtin_deg_rad(ctx, args, span_ref),
        "rad-deg" => builtin_rad_deg(ctx, args, span_ref),
        "deg-grad" => builtin_deg_grad(ctx, args, span_ref),
        "grad-deg" => builtin_grad_deg(ctx, args, span_ref),

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
        "instance-namep" | "symbol-to-instance-name" | "instance-name-to-symbol" => {
            builtin_instance_name(ctx, name, args, span_ref)
        }
        "evenp" => builtin_evenp(ctx, args, span_ref),
        "oddp" => builtin_oddp(ctx, args, span_ref),

        // Type conversion
        "integer" => builtin_to_integer(ctx, args, span_ref),
        "float" => builtin_to_float(ctx, args, span_ref),

        // String/Symbol
        "str-cat" => builtin_str_cat(ctx, args, span_ref),
        "sym-cat" => builtin_sym_cat(ctx, args, span_ref),
        "gensym" => builtin_gensym(ctx, args, span_ref),
        "gensym*" => builtin_gensym_star(ctx, args, span_ref),
        "setgen" => builtin_setgen(ctx, args, span_ref),
        "set-fact-duplication" => builtin_set_fact_duplication(ctx, args, span_ref),
        "get-fact-duplication" => builtin_get_fact_duplication(ctx, args, span_ref),
        "refresh-agenda" => builtin_refresh_agenda(ctx, args, span_ref),
        "watch" => builtin_watch(ctx, args, span_ref),
        "unwatch" => builtin_unwatch(ctx, args, span_ref),
        "str-length" => builtin_str_length(ctx, args, span_ref),
        "sub-string" => builtin_sub_string(ctx, args, span_ref),
        "str-index" => builtin_str_index(ctx, args, span_ref),
        "upcase" => builtin_upcase(ctx, args, span_ref),
        "lowcase" => builtin_lowcase(ctx, args, span_ref),
        "str-compare" => builtin_str_compare(ctx, args, span_ref),
        "string-to-field" => builtin_string_to_field(ctx, args, span_ref),
        "explode$" | "str-explode" => builtin_explode_mf(ctx, args, span_ref),

        // Multifield
        "create$" => builtin_create_mf(ctx, args, span_ref),
        "expand$" => Err(EvalError::UnsupportedOperation {
            operation: "expand$".into(),
            reason: "expand$ requires an argument position in another function call".into(),
            span,
        }),
        "delete-member$" => builtin_edit_members(ctx, args, span_ref, false),
        "replace-member$" => builtin_edit_members(ctx, args, span_ref, true),
        "length" => builtin_length(ctx, args, span_ref),
        "length$" => builtin_length_mf(ctx, args, span_ref),
        "subseq$" => builtin_subseq_mf(ctx, args, span_ref),
        "nth" => builtin_nth(ctx, args, span_ref),
        "implode$" => builtin_implode_mf(ctx, args, span_ref),
        "member" => builtin_member(ctx, args, span_ref),
        "nth$" => builtin_nth_mf(ctx, args, span_ref),
        "member$" => builtin_member_mf(ctx, args, span_ref),
        "subsetp" => builtin_subsetp(ctx, args, span_ref),
        "insert$" => builtin_insert_mf(ctx, args, span_ref),
        "delete$" => builtin_delete_mf(ctx, args, span_ref),
        "replace$" => builtin_replace_mf(ctx, args, span_ref),
        "first$" => builtin_first_mf(ctx, args, span_ref),
        "rest$" => builtin_rest_mf(ctx, args, span_ref),
        "sort" => builtin_sort(ctx, args, span_ref),

        // I/O and environment
        "printout" => builtin_printout(ctx, args, span_ref),
        "format" => builtin_format(ctx, args, span_ref),
        "read" => builtin_read(ctx, args, span_ref),
        "readline" => builtin_readline(ctx, args, span_ref),
        "close" => builtin_close(ctx, args, span_ref),
        "return" => builtin_return(ctx, args, span_ref),
        "break" => builtin_break(args, span_ref),
        "load" => builtin_load(ctx, args, span_ref),
        "undefrule" => builtin_undefrule(ctx, args, span_ref),
        "ppdefrule" => builtin_ppdefrule(ctx, args, span_ref),
        "rules" => builtin_rules(ctx, args, span_ref),

        // Agenda/focus query
        "get-focus" => builtin_get_focus(ctx, args, span_ref),
        "get-focus-stack" => builtin_get_focus_stack(ctx, args, span_ref),

        // Dynamic dispatch
        "funcall" => builtin_funcall(ctx, args, span_ref),

        // Fact introspection
        "fact-existp" => builtin_fact_existp(ctx, args, span_ref),
        "fact-index" => builtin_fact_index(ctx, args, span_ref),
        "fact-relation" => builtin_fact_relation(ctx, args, span_ref),
        "fact-slot-value" => builtin_fact_slot_value(ctx, args, span_ref),
        "fact-slot-names" => builtin_fact_slot_names(ctx, args, span_ref),
        COMPACT_FACT_SLOT_REF => builtin_compact_fact_slot_ref(ctx, args, span_ref),

        // Fact I/O — require engine access; return FALSE when called from pure
        // expression context (the real implementation lives in actions.rs).

        // Special forms
        "bind" => dispatch_bind(ctx, args, span_ref),
        "progn" => {
            let mut result = clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            );
            for arg in args {
                result = eval_inner(ctx, arg)?;
            }
            Ok(result)
        }

        _ => Err(EvalError::UnknownFunction {
            name: name.to_string(),
            span,
        }),
    }
}

// ---------------------------------------------------------------------------
// `bind` special form
// ---------------------------------------------------------------------------

/// `bind` — update an invocation-local override or an existing global variable.
/// A local bind with no values removes its override; multiple values are spliced
/// into a multifield. Global assignment retains its existing one-value policy.
///
/// Returns the value that was bound.
#[allow(clippy::too_many_lines)] // Keep the existing global visibility policy beside local dispatch.
fn dispatch_bind(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if let Some(RuntimeExpr::BoundVar { name, .. }) = args.first() {
        return dispatch_local_bind(ctx, name, &args[1..], span);
    }
    check_arity_exact("bind", args, 2, span)?;

    match &args[0] {
        RuntimeExpr::GlobalVar { name, .. } => {
            let name = name.clone();
            let value = eval_inner(ctx, &args[1])?;
            let target_module = if is_module_qualified(&name) {
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
                    .engine
                    .module_registry
                    .get_by_name(module_name)
                    .ok_or_else(|| EvalError::TypeError {
                        function: "bind".to_string(),
                        expected: format!("existing module `{module_name}`"),
                        actual: "unknown module".to_string(),
                        span: span.cloned(),
                    })?;
                if !ctx.engine.globals.contains(module_id, local_name) {
                    return Err(EvalError::UnboundGlobal {
                        name,
                        span: span.cloned(),
                    });
                }
                if !ctx.engine.module_registry.is_construct_visible(
                    global_lookup_module(ctx),
                    module_id,
                    "defglobal",
                    local_name,
                ) {
                    return Err(EvalError::NotVisible {
                        name: format!("?*{name}*"),
                        construct_type: "defglobal".to_string(),
                        from_module: module_label(ctx, global_lookup_module(ctx)),
                        owning_module: module_name.to_string(),
                        span: span.cloned(),
                    });
                }
                module_id
            } else if ctx
                .engine
                .globals
                .contains(global_lookup_module(ctx), &name)
            {
                global_lookup_module(ctx)
            } else {
                let all_modules = sorted_dedup_modules(ctx.engine.globals.modules_for_name(&name));
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
                            from_module: module_label(ctx, global_lookup_module(ctx)),
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
            ctx.engine
                .globals
                .set(target_module, local_name, value.clone());
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

fn dispatch_local_bind(
    ctx: &mut EvalContext<'_>,
    name: &str,
    values: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if ctx.callable_locals.is_none() {
        return Err(EvalError::UnsupportedOperation {
            operation: "bind".into(),
            reason: "local binding requires a callable invocation".into(),
            span: span.cloned(),
        });
    }
    let name = name.strip_prefix("$?").unwrap_or(name);
    if ctx
        .callable_locals
        .as_deref()
        .is_some_and(|locals| locals.protected_names.contains(name))
    {
        return Err(EvalError::UnsupportedOperation {
            operation: "bind".into(),
            reason: format!("cannot rebind active iteration variable ?{name}"),
            span: span.cloned(),
        });
    }
    // Evaluate before replacing the binding so an error leaves its previous
    // value intact, while nested expressions retain their own side effects.
    let value = match values {
        [] => None,
        [value] => Some(eval_inner(ctx, value)?),
        _ => {
            let values = eval_args(ctx, values)?;
            let mut fields = ferric_rules_core::Multifield::new();
            for value in values {
                match value {
                    Value::Multifield(multifield) => {
                        fields.extend(multifield.iter().cloned());
                    }
                    Value::Void => {}
                    value => fields.push(value),
                }
            }
            Some(Value::Multifield(Box::new(fields)))
        }
    };
    let locals = ctx
        .callable_locals
        .as_deref_mut()
        .expect("checked callable frame");
    if let Some(value) = value {
        locals.values.insert(name.to_string(), value.clone());
        Ok(value)
    } else {
        locals.values.remove(name);
        Ok(clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ))
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

/// Expand each explicit sequence operand once, in source order. Ordinary
/// operands remain expressions, preserving their original evaluation policy.
fn expand_call_arguments(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
) -> Result<Option<Vec<RuntimeExpr>>, EvalError> {
    if !args
        .iter()
        .any(|arg| matches!(arg, RuntimeExpr::Call { name, .. } if name == "expand$"))
    {
        return Ok(None);
    }
    let mut expanded = Vec::with_capacity(args.len());
    for arg in args {
        if let RuntimeExpr::Call { name, args, span } = arg {
            if name == "expand$" {
                check_arity_exact("expand$", args, 1, span.as_ref())?;
                let value = eval_inner(ctx, &args[0])?;
                let Value::Multifield(fields) = value else {
                    return Err(EvalError::TypeError {
                        function: "expand$".into(),
                        expected: "MULTIFIELD".into(),
                        actual: generic_value_type_name(&value).into(),
                        span: span.clone(),
                    });
                };
                expanded.extend(fields.iter().cloned().map(RuntimeExpr::Literal));
                continue;
            }
        }
        expanded.push(arg.clone());
    }
    Ok(Some(expanded))
}

fn validate_expanded_arity(
    name: &str,
    count: usize,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    crate::builtin_validation::validate_runtime_arity(name, count).map_err(|message| {
        EvalError::ArityMismatch {
            name: name.into(),
            expected: message,
            actual: count,
            span: span.cloned(),
        }
    })
}

fn eval_args(ctx: &mut EvalContext<'_>, args: &[RuntimeExpr]) -> Result<Vec<Value>, EvalError> {
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval_inner(ctx, arg)?);
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

/// `mod` preserves integers only when both operands are integers.
#[allow(clippy::cast_precision_loss)]
fn builtin_mod(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("mod", args, 2, span)?;
    let lhs = as_numeric(&eval_inner(ctx, &args[0])?, "mod", span)?;
    let rhs = as_numeric(&eval_inner(ctx, &args[1])?, "mod", span)?;
    if matches!(rhs, Numeric::Int(0)) || matches!(rhs, Numeric::Flt(value) if value == 0.0) {
        return Err(EvalError::DivisionByZero {
            function: "mod".to_string(),
            span: span.cloned(),
        });
    }
    if let (Numeric::Int(lhs), Numeric::Int(rhs)) = (&lhs, &rhs) {
        return Ok(Value::Integer(lhs.checked_rem(*rhs).unwrap_or(0)));
    }
    // CLIPS 6.30 computes `a - trunc(a / b) * b`, not C's `fmod`: the two
    // differ when the quotient is inexact or overflows.
    let float = |value| match value {
        Numeric::Int(value) => value as f64,
        Numeric::Flt(value) => value,
    };
    let (lhs, rhs) = (float(lhs), float(rhs));
    Ok(Value::Float(lhs - (lhs / rhs).trunc() * rhs))
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
fn builtin_min(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    builtin_extremum(ctx, args, span, "min", std::cmp::Ordering::Less)
}

/// `max` (1+ args)
fn builtin_max(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    builtin_extremum(ctx, args, span, "max", std::cmp::Ordering::Greater)
}

/// Return the first selected operand unchanged, including its type and zero sign.
#[allow(clippy::cast_precision_loss)] // CLIPS converts mixed pairs, but compares integer pairs exactly.
fn builtin_extremum(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    function: &str,
    preferred: std::cmp::Ordering,
) -> Result<Value, EvalError> {
    check_arity_min(function, args, 1, span)?;
    // As in CLIPS, each operand is checked as it is evaluated, so a
    // non-numeric operand stops the later ones from being evaluated.
    let mut selected = eval_inner(ctx, &args[0])?;
    let mut selected_numeric = as_numeric(&selected, function, span)?;
    for arg in &args[1..] {
        let value = eval_inner(ctx, arg)?;
        let candidate = as_numeric(&value, function, span)?;
        let ordering = match (&candidate, &selected_numeric) {
            (Numeric::Int(a), Numeric::Int(b)) => Some(a.cmp(b)),
            (Numeric::Int(a), Numeric::Flt(b)) => (*a as f64).partial_cmp(b),
            (Numeric::Flt(a), Numeric::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Numeric::Flt(a), Numeric::Flt(b)) => a.partial_cmp(b),
        };
        // Ties retain the current operand. Future comparisons use its type,
        // even when an earlier, discarded operand was a float.
        if ordering == Some(preferred) {
            selected = value;
            selected_numeric = candidate;
        }
    }
    Ok(selected)
}

// ---------------------------------------------------------------------------
// Transcendental math built-ins
// ---------------------------------------------------------------------------

fn builtin_sqrt(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("sqrt", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "sqrt", span)?;
    reject_out_of_domain("sqrt", f < 0.0, span)?;
    Ok(Value::Float(f.sqrt()))
}

fn builtin_sin(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("sin", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "sin", span)?;
    Ok(Value::Float(f.sin()))
}

fn builtin_cos(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("cos", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "cos", span)?;
    Ok(Value::Float(f.cos()))
}

fn builtin_tan(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("tan", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "tan", span)?;
    if f.cos().abs() < 1.0e-15 {
        return Err(EvalError::MathSingularity {
            function: "tan".into(),
            span: span.cloned(),
        });
    }
    Ok(Value::Float(f.tan()))
}

fn builtin_asin(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("asin", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "asin", span)?;
    reject_out_of_domain("asin", outside_unit_interval(f), span)?;
    Ok(Value::Float(f.asin()))
}

fn builtin_acos(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("acos", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "acos", span)?;
    reject_out_of_domain("acos", outside_unit_interval(f), span)?;
    Ok(Value::Float(f.acos()))
}

fn builtin_atan(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("atan", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "atan", span)?;
    Ok(Value::Float(f.atan()))
}

fn builtin_atan2(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("atan2", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let y = as_float(&values[0], "atan2", span)?;
    let x = as_float(&values[1], "atan2", span)?;
    Ok(Value::Float(y.atan2(x)))
}

fn builtin_sinh(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("sinh", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "sinh", span)?;
    Ok(Value::Float(f.sinh()))
}

fn builtin_cosh(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("cosh", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "cosh", span)?;
    Ok(Value::Float(f.cosh()))
}

fn builtin_tanh(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("tanh", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "tanh", span)?;
    Ok(Value::Float(f.tanh()))
}

fn builtin_asinh(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("asinh", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "asinh", span)?;
    Ok(Value::Float(f.asinh()))
}

fn builtin_acosh(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("acosh", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "acosh", span)?;
    reject_out_of_domain("acosh", f < 1.0, span)?;
    Ok(Value::Float(f.acosh()))
}

fn builtin_atanh(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("atanh", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "atanh", span)?;
    reject_out_of_domain("atanh", f >= 1.0 || f <= -1.0, span)?;
    Ok(Value::Float(f.atanh()))
}

fn builtin_exp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("exp", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "exp", span)?;
    Ok(Value::Float(f.exp()))
}

fn builtin_log(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("log", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "log", span)?;
    validate_log_argument("log", f, span)?;
    Ok(Value::Float(f.ln()))
}

fn builtin_log10(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("log10", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "log10", span)?;
    validate_log_argument("log10", f, span)?;
    Ok(Value::Float(f.log10()))
}

fn builtin_pow(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("**", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let base = as_float(&values[0], "**", span)?;
    let exp = as_float(&values[1], "**", span)?;
    reject_out_of_domain(
        "**",
        base == 0.0 && exp <= 0.0 || base < 0.0 && exp.fract() != 0.0,
        span,
    )?;
    Ok(Value::Float(base.powf(exp)))
}

/// Whether `f` is out of the `asin`/`acos` domain. Unlike
/// `!(-1.0..=1.0).contains(&f)`, NaN is not out of the domain.
#[allow(clippy::manual_range_contains)]
fn outside_unit_interval(f: f64) -> bool {
    f < -1.0 || f > 1.0
}

/// Like CLIPS, each caller tests for an out-of-domain argument rather than
/// membership in the domain, so a NaN argument propagates instead of failing.
fn reject_out_of_domain(
    function: &str,
    out_of_domain: bool,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    if out_of_domain {
        Err(EvalError::MathDomain {
            function: function.into(),
            span: span.cloned(),
        })
    } else {
        Ok(())
    }
}

fn validate_log_argument(
    function: &str,
    value: f64,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    reject_out_of_domain(function, value < 0.0, span)?;
    if value == 0.0 {
        return Err(EvalError::MathOverflow {
            function: function.into(),
            span: span.cloned(),
        });
    }
    Ok(())
}

fn builtin_pi(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("pi", args, 0, span)?;
    let _ = ctx;
    Ok(Value::Float(std::f64::consts::PI))
}

#[allow(clippy::cast_possible_truncation)]
fn builtin_round(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("round", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let rounded = match as_numeric(&values[0], "round", span)? {
        Numeric::Int(value) => value,
        // CLIPS chooses the lower integer at half ties, including the
        // floating-point subtraction effects around representable boundaries.
        Numeric::Flt(value) => (value - 0.5).ceil() as i64,
    };
    Ok(Value::Integer(rounded))
}

#[allow(clippy::cast_possible_truncation)]
fn builtin_ceiling(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("ceiling", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "ceiling", span)?;
    Ok(Value::Integer(f.ceil() as i64))
}

#[allow(clippy::cast_possible_truncation)]
fn builtin_floor(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("floor", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "floor", span)?;
    Ok(Value::Integer(f.floor() as i64))
}

fn builtin_deg_rad(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("deg-rad", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "deg-rad", span)?;
    Ok(Value::Float(f * std::f64::consts::PI / 180.0))
}

fn builtin_rad_deg(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("rad-deg", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "rad-deg", span)?;
    Ok(Value::Float(f * 180.0 / std::f64::consts::PI))
}

fn builtin_deg_grad(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("deg-grad", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "deg-grad", span)?;
    Ok(Value::Float(f * 10.0 / 9.0))
}

fn builtin_grad_deg(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("grad-deg", args, 1, span)?;
    let values = eval_args(ctx, args)?;
    let f = as_float(&values[0], "grad-deg", span)?;
    Ok(Value::Float(f * 9.0 / 10.0))
}

// ---------------------------------------------------------------------------
// Comparison built-ins
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum NumericComparison {
    Greater,
    Less,
    GreaterOrEqual,
    LessOrEqual,
    Equal,
    NotEqual,
}

impl NumericComparison {
    #[allow(clippy::cast_precision_loss)]
    fn matches(self, left: &Numeric, right: &Numeric) -> bool {
        let order = match (left, right) {
            (Numeric::Int(left), Numeric::Int(right)) => Some(left.cmp(right)),
            (Numeric::Flt(left), Numeric::Flt(right)) => left.partial_cmp(right),
            (Numeric::Int(left), Numeric::Flt(right)) => (*left as f64).partial_cmp(right),
            (Numeric::Flt(left), Numeric::Int(right)) => left.partial_cmp(&(*right as f64)),
        };
        match self {
            Self::Greater => order == Some(Ordering::Greater),
            Self::Less => order == Some(Ordering::Less),
            Self::GreaterOrEqual => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
            Self::LessOrEqual => matches!(order, Some(Ordering::Less | Ordering::Equal)),
            Self::Equal => order == Some(Ordering::Equal),
            Self::NotEqual => order != Some(Ordering::Equal),
        }
    }
}

/// Evaluate only the operands needed to decide a numeric comparison chain.
/// Equality and inequality keep the first operand; ordering advances the cursor.
fn eval_cmp_chain(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    comparison: NumericComparison,
) -> Result<Value, EvalError> {
    check_arity_min(name, args, 2, span)?;
    let mut previous = as_numeric(&eval_inner(ctx, &args[0])?, name, span)?;
    for argument in &args[1..] {
        let next = as_numeric(&eval_inner(ctx, argument)?, name, span)?;
        if !comparison.matches(&previous, &next) {
            return Ok(clips_bool(
                false,
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ));
        }
        if !matches!(
            comparison,
            NumericComparison::Equal | NumericComparison::NotEqual
        ) {
            previous = next;
        }
    }
    Ok(clips_bool(
        true,
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

fn builtin_cmp_gt(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_cmp_chain(ctx, ">", args, span, NumericComparison::Greater)
}

fn builtin_cmp_lt(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_cmp_chain(ctx, "<", args, span, NumericComparison::Less)
}

fn builtin_cmp_gte(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_cmp_chain(ctx, ">=", args, span, NumericComparison::GreaterOrEqual)
}

fn builtin_cmp_lte(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_cmp_chain(ctx, "<=", args, span, NumericComparison::LessOrEqual)
}

fn builtin_cmp_eq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_cmp_chain(ctx, "=", args, span, NumericComparison::Equal)
}

fn builtin_cmp_neq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_cmp_chain(ctx, "!=", args, span, NumericComparison::NotEqual)
}

/// `eq` — value equality (symbols, strings, numbers).
fn builtin_eq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("eq", args, 2, span)?;
    let first = eval_inner(ctx, &args[0])?;
    let mut result = true;
    for argument in &args[1..] {
        if !first.structural_eq(&eval_inner(ctx, argument)?) {
            result = false;
            break;
        }
    }
    Ok(clips_bool(
        result,
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `neq` — value inequality.
fn builtin_neq(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("neq", args, 2, span)?;
    let first = eval_inner(ctx, &args[0])?;
    let mut result = true;
    for argument in &args[1..] {
        if first.structural_eq(&eval_inner(ctx, argument)?) {
            result = false;
            break;
        }
    }
    Ok(clips_bool(
        result,
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        let v = eval_inner(ctx, arg)?;
        if !is_truthy(&v, &ctx.engine.symbol_table) {
            return Ok(clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ));
        }
    }
    Ok(clips_true(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `or` (variadic)
fn builtin_or(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    _span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    for arg in args {
        let v = eval_inner(ctx, arg)?;
        if is_truthy(&v, &ctx.engine.symbol_table) {
            return Ok(clips_true(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ));
        }
    }
    Ok(clips_false(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `not` (1 arg)
fn builtin_not(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("not", args, 1, span)?;
    let v = eval_inner(ctx, &args[0])?;
    Ok(clips_bool(
        !is_truthy(&v, &ctx.engine.symbol_table),
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `instance-namep` and the SYMBOL/INSTANCE-NAME conversions. Ferric has no
/// object system, so these only change the value's type tag. As in CLIPS,
/// `symbol-to-instance-name` takes a SYMBOL and `instance-name-to-symbol`
/// takes an INSTANCE-NAME or a SYMBOL.
fn builtin_instance_name(
    ctx: &mut EvalContext<'_>,
    name: &str,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact(name, args, 1, span)?;
    let value = eval_inner(ctx, &args[0])?;
    let expected = match (name, &value) {
        ("instance-namep", _) => {
            return Ok(clips_bool(
                matches!(value, Value::InstanceName(_)),
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ))
        }
        ("symbol-to-instance-name", Value::Symbol(symbol)) => {
            return Ok(Value::InstanceName(InstanceName::from_symbol(*symbol)))
        }
        ("instance-name-to-symbol", Value::InstanceName(instance)) => {
            return Ok(Value::Symbol(instance.as_symbol()))
        }
        ("instance-name-to-symbol", Value::Symbol(symbol)) => return Ok(Value::Symbol(*symbol)),
        ("symbol-to-instance-name", _) => "SYMBOL",
        _ => "INSTANCE-NAME or SYMBOL",
    };
    Err(EvalError::TypeError {
        function: name.to_string(),
        expected: expected.to_string(),
        actual: generic_value_type_name(&value).to_string(),
        span: span.cloned(),
    })
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

fn builtin_evenp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("evenp", args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    match val {
        Value::Integer(n) => Ok(clips_bool(
            n % 2 == 0,
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
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
    let val = eval_inner(ctx, &args[0])?;
    match val {
        Value::Integer(n) => Ok(clips_bool(
            n % 2 != 0,
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
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
    let val = eval_inner(ctx, &args[0])?;
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
    let val = eval_inner(ctx, &args[0])?;
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

/// Append each value's string representation to `buf`, using the symbol table
/// to resolve symbol names.
///
/// Shared by `str-cat` and `sym-cat`; only scalar printable atoms are accepted.
fn concat_values_to_string(
    ctx: &EvalContext<'_>,
    values: &[Value],
    buf: &mut String,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    use std::fmt::Write as _;
    for val in values {
        match val {
            Value::Integer(n) => {
                // Use write! to avoid clippy::format_push_string warning.
                let _ = write!(buf, "{n}");
            }
            Value::Float(f) => crate::formatting::append_clips_float(*f, buf),
            Value::Symbol(sym) => {
                if let Some(name) = ctx.engine.symbol_table.resolve_symbol_str(*sym) {
                    buf.push_str(name);
                }
            }
            // CLIPS concatenates an instance name's spelling without brackets.
            Value::InstanceName(name) => {
                if let Some(name) = ctx.engine.symbol_table.resolve_symbol_str(name.as_symbol()) {
                    buf.push_str(name);
                }
            }
            Value::String(s) => buf.push_str(s.as_str()),
            Value::Multifield(_)
            | Value::Void
            | Value::ExternalAddress(_)
            | Value::FactAddress(_) => {
                return Err(EvalError::TypeError {
                    function: function.into(),
                    expected: "STRING, SYMBOL, INSTANCE-NAME, INTEGER, or FLOAT".into(),
                    actual: generic_value_type_name(val).into(),
                    span: span.cloned(),
                });
            }
        }
    }
    Ok(())
}

/// `str-cat` — concatenate 0+ values into a STRING.
///
/// Each argument is converted to its string representation and the results
/// are concatenated.  Integers format as decimal strings, floats always
/// include a decimal point, symbols and strings contribute their content,
/// multifields and address values are rejected.
fn builtin_str_cat(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("str-cat", args, 1, span)?;
    let mut result = String::new();
    for argument in args {
        let value = eval_inner(ctx, argument)?;
        concat_values_to_string(
            ctx,
            std::slice::from_ref(&value),
            &mut result,
            "str-cat",
            span,
        )?;
    }
    let fs = FerricString::new(&result, ctx.engine.config.string_encoding).map_err(|e| {
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
    check_arity_min("sym-cat", args, 1, span)?;
    let mut result = String::new();
    for argument in args {
        let value = eval_inner(ctx, argument)?;
        concat_values_to_string(
            ctx,
            std::slice::from_ref(&value),
            &mut result,
            "sym-cat",
            span,
        )?;
    }
    let sym = ctx
        .engine
        .symbol_table
        .intern_symbol(&result, ctx.engine.config.string_encoding)
        .map_err(|e| EvalError::TypeError {
            function: "sym-cat".to_string(),
            expected: "encodable symbol name".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `gensym` — generate a unique symbol name using the `gen` prefix.
fn builtin_gensym(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("gensym", args, 0, span)?;
    let suffix = ctx.engine.globals.next_gensym_counter();
    let symbol_name = format!("gen{suffix}");
    let sym = ctx
        .engine
        .symbol_table
        .intern_symbol(&symbol_name, ctx.engine.config.string_encoding)
        .map_err(|e| EvalError::TypeError {
            function: "gensym".to_string(),
            expected: "encodable symbol name".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `gensym*` — CLIPS-compatible alias of `gensym`.
fn builtin_gensym_star(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("gensym*", args, 0, span)?;
    let suffix = ctx.engine.globals.next_gensym_counter();
    let symbol_name = format!("gen{suffix}");
    let sym = ctx
        .engine
        .symbol_table
        .intern_symbol(&symbol_name, ctx.engine.config.string_encoding)
        .map_err(|e| EvalError::TypeError {
            function: "gensym*".to_string(),
            expected: "encodable symbol name".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `setgen` — set the next numeric suffix used by `gensym`/`gensym*`.
fn builtin_setgen(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("setgen", args, 1, span)?;
    let value = eval_inner(ctx, &args[0])?;
    match value {
        Value::Integer(n) if n >= 1 => {
            ctx.engine.globals.set_gensym_counter(n);
            Ok(Value::Integer(n))
        }
        Value::Integer(_) => Err(EvalError::TypeError {
            function: "setgen".to_string(),
            expected: "positive INTEGER".to_string(),
            actual: "non-positive integer".to_string(),
            span: span.cloned(),
        }),
        _ => Err(EvalError::TypeError {
            function: "setgen".to_string(),
            expected: "INTEGER".to_string(),
            actual: value_type_name(&value).to_string(),
            span: span.cloned(),
        }),
    }
}

/// `set-fact-duplication` — change the engine policy and return its prior value.
fn builtin_set_fact_duplication(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("set-fact-duplication", args, 1, span)?;
    let value = eval_inner(ctx, &args[0])?;
    let previous = ctx
        .engine
        .config
        .set_fact_duplication(is_truthy(&value, &ctx.engine.symbol_table));
    Ok(clips_bool(
        previous,
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `get-fact-duplication` — return the engine's current duplication policy.
fn builtin_get_fact_duplication(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("get-fact-duplication", args, 0, span)?;
    Ok(clips_bool(
        ctx.engine.config.fact_duplication(),
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// Dynamic salience/agenda refresh is outside the supported static-salience subset.
fn builtin_refresh_agenda(
    _ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("refresh-agenda", args, 0, span)?;
    Err(EvalError::UnsupportedOperation {
        operation: "refresh-agenda".into(),
        reason: "dynamic salience and explicit agenda refresh are not supported".into(),
        span: span.cloned(),
    })
}

/// Enable transient observer output for supported watch targets.
fn builtin_watch(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    configure_watch(ctx, args, "watch", true, span)
}

/// Disable transient observer output for supported watch targets.
fn builtin_unwatch(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    configure_watch(ctx, args, "unwatch", false, span)
}

fn configure_watch(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    name: &str,
    enabled: bool,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact(name, args, 1, span)?;
    let value = eval_inner(ctx, &args[0])?;
    let Value::Symbol(symbol) = value else {
        return Err(EvalError::TypeError {
            function: name.to_owned(),
            expected: "SYMBOL watch target".to_owned(),
            actual: value.type_name().to_owned(),
            span: span.cloned(),
        });
    };
    match ctx.engine.symbol_table.resolve_symbol_str(symbol) {
        Some("facts") => {
            ctx.engine.set_watch_facts(enabled);
        }
        Some("rules") => {
            ctx.engine.set_watch_rules(enabled);
        }
        Some("all") => {
            ctx.engine.set_watch_facts(enabled);
            ctx.engine.set_watch_rules(enabled);
        }
        target => {
            return Err(EvalError::UnsupportedOperation {
                operation: name.to_owned(),
                reason: format!(
                    "unsupported watch target `{}`; expected facts, rules, or all",
                    target.unwrap_or("<invalid>")
                ),
                span: span.cloned(),
            })
        }
    }
    Ok(Value::Void)
}

/// `str-length` — the character length of a STRING, SYMBOL or INSTANCE-NAME.
fn builtin_str_length(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("str-length", args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    let lexeme = lexeme_text(&val, &ctx.engine.symbol_table, "str-length", span)?;
    let char_len = i64::try_from(lexeme.chars().count()).unwrap_or(i64::MAX);
    Ok(Value::Integer(char_len))
}

/// `sub-string` — extract a substring by 1-indexed inclusive position.
///
/// `(sub-string <start> <end> <lexeme>)` accepts STRING, SYMBOL or
/// INSTANCE-NAME text.
/// Bounds are inclusive, with starts below one and ends beyond the text clipped.
/// An end below one returns an empty STRING without evaluating the text;
/// positive ends evaluate the text even for reversed or out-of-range starts.
fn builtin_sub_string(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("sub-string", args, 3, span)?;

    let start = match eval_inner(ctx, &args[0])? {
        Value::Integer(n) => n.max(1),
        value => {
            return Err(EvalError::TypeError {
                function: "sub-string".to_string(),
                expected: "INTEGER (start position)".to_string(),
                actual: generic_value_type_name(&value).to_string(),
                span: span.cloned(),
            })
        }
    };
    let end = match eval_inner(ctx, &args[1])? {
        Value::Integer(n) => n,
        value => {
            return Err(EvalError::TypeError {
                function: "sub-string".to_string(),
                expected: "INTEGER (end position)".to_string(),
                actual: generic_value_type_name(&value).to_string(),
                span: span.cloned(),
            })
        }
    };

    let make_empty_string = |ctx: &mut EvalContext<'_>| {
        FerricString::new("", ctx.engine.config.string_encoding).map_err(|e| EvalError::TypeError {
            function: "sub-string".to_string(),
            expected: "encodable string".to_string(),
            actual: format!("{e}"),
            span: span.cloned(),
        })
    };
    if end < 1 {
        return make_empty_string(ctx).map(Value::String);
    }

    let text = eval_inner(ctx, &args[2])?;
    let s = lexeme_text(&text, &ctx.engine.symbol_table, "sub-string", span)?;

    // CLIPS uses 1-indexed inclusive bounds. Convert to Rust 0-indexed.
    let char_len = s.chars().count();
    let char_len_i64 = i64::try_from(char_len).unwrap_or(i64::MAX);
    if end < start || start > char_len_i64 {
        return make_empty_string(ctx).map(Value::String);
    }

    let Ok(start_char_idx) = usize::try_from(start - 1) else {
        return make_empty_string(ctx).map(Value::String);
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

    let fs = FerricString::new(substr, ctx.engine.config.string_encoding).map_err(|e| {
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
// String search/transform built-ins
// ---------------------------------------------------------------------------

/// The text of a STRING, SYMBOL or INSTANCE-NAME, as CLIPS's string
/// functions see it: an instance name contributes its unbracketed name.
fn lexeme_text<'a>(
    v: &'a Value,
    symbol_table: &'a SymbolTable,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<&'a str, EvalError> {
    let symbol = match v {
        Value::String(s) => return Ok(s.as_str()),
        Value::Symbol(s) => *s,
        Value::InstanceName(name) => name.as_symbol(),
        _ => {
            return Err(EvalError::TypeError {
                function: function.to_string(),
                expected: "STRING, SYMBOL, or INSTANCE-NAME".to_string(),
                actual: generic_value_type_name(v).to_string(),
                span: span.cloned(),
            })
        }
    };
    Ok(symbol_table.resolve_symbol_str(symbol).unwrap_or("???"))
}

/// Owned [`lexeme_text`].
fn as_lexeme_str(
    v: &Value,
    symbol_table: &SymbolTable,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<String, EvalError> {
    lexeme_text(v, symbol_table, function, span).map(str::to_string)
}

/// `str-index` — find substring, return 1-based position or FALSE.
/// An empty needle returns the position after the haystack's last character.
fn builtin_str_index(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("str-index", args, 2, span)?;
    let needle_value = eval_inner(ctx, &args[0])?;
    let needle = as_lexeme_str(&needle_value, &ctx.engine.symbol_table, "str-index", span)?;
    let haystack_value = eval_inner(ctx, &args[1])?;
    let haystack = as_lexeme_str(&haystack_value, &ctx.engine.symbol_table, "str-index", span)?;
    let position = if needle.is_empty() {
        Some(haystack.len())
    } else {
        haystack.find(needle.as_str())
    };
    match position {
        Some(byte_pos) => {
            // Convert byte offset to 1-based character position.
            let char_pos = haystack[..byte_pos].chars().count() + 1;
            Ok(Value::Integer(i64::try_from(char_pos).unwrap_or(i64::MAX)))
        }
        None => Ok(clips_bool(
            false,
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        )),
    }
}

/// `upcase`/`lowcase` — convert the text of a STRING, SYMBOL or
/// INSTANCE-NAME, preserving its type. Like CLIPS, only ASCII letters change.
fn convert_case(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    function: &str,
    convert: fn(&str) -> String,
) -> Result<Value, EvalError> {
    check_arity_exact(function, args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    let converted = convert(lexeme_text(&val, &ctx.engine.symbol_table, function, span)?);
    let encoding_error = |expected: &str, error: String| EvalError::TypeError {
        function: function.to_string(),
        expected: expected.to_string(),
        actual: error,
        span: span.cloned(),
    };
    if matches!(val, Value::String(_)) {
        return FerricString::new(&converted, ctx.engine.config.string_encoding)
            .map(Value::String)
            .map_err(|e| encoding_error("encodable string", e.to_string()));
    }
    let symbol = ctx
        .engine
        .symbol_table
        .intern_symbol(&converted, ctx.engine.config.string_encoding)
        .map_err(|e| encoding_error("encodable symbol", e.to_string()))?;
    Ok(if matches!(val, Value::InstanceName(_)) {
        Value::InstanceName(InstanceName::from_symbol(symbol))
    } else {
        Value::Symbol(symbol)
    })
}

/// `upcase` — uppercase a STRING, SYMBOL or INSTANCE-NAME.
fn builtin_upcase(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    convert_case(ctx, args, span, "upcase", str::to_ascii_uppercase)
}

/// `lowcase` — lowercase a STRING, SYMBOL or INSTANCE-NAME.
fn builtin_lowcase(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    convert_case(ctx, args, span, "lowcase", str::to_ascii_lowercase)
}

/// `str-compare` — lexicographic comparison, returns -1, 0, or 1.
fn builtin_str_compare(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("str-compare", args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let a = as_lexeme_str(&values[0], &ctx.engine.symbol_table, "str-compare", span)?;
    let b = as_lexeme_str(&values[1], &ctx.engine.symbol_table, "str-compare", span)?;
    let result = match a.cmp(&b) {
        std::cmp::Ordering::Less => -1i64,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    Ok(Value::Integer(result))
}

/// `string-to-field` — the first CLIPS field of a STRING, SYMBOL or
/// INSTANCE-NAME, scanned as CLIPS does; the rest of the text is ignored.
fn builtin_string_to_field(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("string-to-field", args, 1, span)?;
    let value = eval_inner(ctx, &args[0])?;
    let text = match &value {
        Value::String(s) => std::borrow::Cow::Borrowed(s.as_str()),
        Value::Symbol(symbol) => std::borrow::Cow::Owned(
            ctx.engine
                .symbol_table
                .resolve_symbol_str(*symbol)
                .unwrap_or_default()
                .to_owned(),
        ),
        Value::InstanceName(name) => std::borrow::Cow::Owned(
            ctx.engine
                .symbol_table
                .resolve_symbol_str(name.as_symbol())
                .unwrap_or_default()
                .to_owned(),
        ),
        other => {
            return Err(EvalError::TypeError {
                function: "string-to-field".to_string(),
                expected: "STRING, SYMBOL, or INSTANCE-NAME".to_string(),
                actual: generic_value_type_name(other).to_string(),
                span: span.cloned(),
            })
        }
    };
    let scanned = scan_field(ctx, &mut FieldScanner::new(text.as_bytes()));
    match scanned {
        FieldToken::Stop => {
            intern_scanned_symbol(ctx, "EOF", "string-to-field", span).map(Value::Symbol)
        }
        FieldToken::Unknown => scanned_string(ctx, "*** ERROR ***", "string-to-field", span),
        token => scanned_field_value(ctx, token, "string-to-field", span),
    }
}

/// `explode$` / `str-explode` — every CLIPS field of a STRING, in order.
/// Tokens that are not values (such as `(`) become STRINGs of their print form.
fn builtin_explode_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("explode$", args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    let Value::String(s) = &val else {
        return Err(EvalError::TypeError {
            function: "explode$".to_string(),
            expected: "STRING".to_string(),
            actual: generic_value_type_name(&val).to_string(),
            span: span.cloned(),
        });
    };
    let mut scanner = FieldScanner::new(s.as_str().as_bytes());
    let mut result = ferric_rules_core::value::Multifield::new();
    loop {
        match scan_field(ctx, &mut scanner) {
            FieldToken::Stop => return Ok(Value::Multifield(Box::new(result))),
            token => result.push(scanned_field_value(ctx, token, "explode$", span)?),
        }
    }
}

/// Scan the next field, writing any scanner notice to its CLIPS router.
fn scan_field<'a>(ctx: &mut EvalContext<'_>, scanner: &mut FieldScanner<'a>) -> FieldToken<'a> {
    let field = scanner.next_token();
    if let Some(notice) = field.notice {
        ctx.engine
            .globals
            .push_printout_event(notice.router().to_string(), notice.text().to_string());
    }
    field.token
}

/// The value of a scanned token. Tokens that are not values become STRINGs of
/// their print form. Scanned text is decoded as UTF-8; the only invalid
/// sequence the scanner can produce (a string ending in an escaped end of
/// input) becomes U+FFFD, where CLIPS keeps a raw byte.
fn scanned_field_value(
    ctx: &mut EvalContext<'_>,
    token: FieldToken<'_>,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    match token {
        FieldToken::Integer(value) => Ok(Value::Integer(value)),
        FieldToken::Float(value) => Ok(Value::Float(value)),
        FieldToken::Symbol(bytes) => {
            intern_scanned_symbol(ctx, &String::from_utf8_lossy(&bytes), function, span)
                .map(Value::Symbol)
        }
        FieldToken::InstanceName(bytes) => {
            intern_scanned_symbol(ctx, &String::from_utf8_lossy(&bytes), function, span)
                .map(|symbol| Value::InstanceName(InstanceName::from_symbol(symbol)))
        }
        FieldToken::String(bytes) => {
            scanned_string(ctx, &String::from_utf8_lossy(&bytes), function, span)
        }
        other => {
            let print_form = other
                .print_form()
                .expect("non-value token has a print form");
            scanned_string(ctx, &String::from_utf8_lossy(print_form), function, span)
        }
    }
}

fn intern_scanned_symbol(
    ctx: &mut EvalContext<'_>,
    text: &str,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<Symbol, EvalError> {
    ctx.engine
        .symbol_table
        .intern_symbol(text, ctx.engine.config.string_encoding)
        .map_err(|error| scanned_encoding_error(&error, function, span))
}

fn scanned_string(
    ctx: &EvalContext<'_>,
    text: &str,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    FerricString::new(text, ctx.engine.config.string_encoding)
        .map(Value::String)
        .map_err(|error| scanned_encoding_error(&error, function, span))
}

fn scanned_encoding_error(
    error: &impl std::fmt::Display,
    function: &str,
    span: Option<&SourceSpan>,
) -> EvalError {
    EvalError::TypeError {
        function: function.to_string(),
        expected: "field permitted by the configured encoding".to_string(),
        actual: error.to_string(),
        span: span.cloned(),
    }
}

// ---------------------------------------------------------------------------
// Multifield built-ins
// ---------------------------------------------------------------------------

/// `create$` — create a multifield from 0+ arguments.
///
/// If any argument is itself a multifield, its elements are flattened into
/// the result (CLIPS implicit multifield flattening). Scalar VOID results
/// contribute no field, but their expressions are still evaluated.
fn builtin_create_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    _span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let values = eval_args(ctx, args)?;
    let mut result = ferric_rules_core::value::Multifield::new();
    for val in values {
        match val {
            Value::Multifield(mf) => {
                for elem in mf.iter() {
                    result.push(elem.clone());
                }
            }
            Value::Void => {}
            other => result.push(other),
        }
    }
    Ok(Value::Multifield(Box::new(result)))
}

/// `length` and `length$` accept a multifield, symbol, or string.
/// Lexemes count stored bytes, rather than Unicode scalar values.
fn builtin_length(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    builtin_length_value(ctx, args, span, "length")
}

fn builtin_length_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    builtin_length_value(ctx, args, span, "length$")
}

fn builtin_length_value(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    function: &str,
) -> Result<Value, EvalError> {
    check_arity_exact(function, args, 1, span)?;
    let value = eval_inner(ctx, &args[0])?;
    let length = match &value {
        Value::Multifield(values) => values.len(),
        Value::String(value) => value.as_bytes().len(),
        Value::Symbol(value) => ctx.engine.symbol_table.resolve_symbol(*value).len(),
        _ => {
            return Err(EvalError::TypeError {
                function: function.into(),
                expected: "MULTIFIELD, SYMBOL, or STRING".into(),
                actual: generic_value_type_name(&value).into(),
                span: span.cloned(),
            })
        }
    };
    Ok(Value::Integer(i64::try_from(length).unwrap_or(i64::MAX)))
}

/// `subseq$` — extract a 1-indexed inclusive multifield slice.
///
/// `(subseq$ <multifield> <start> <end>)`.
/// Out-of-range indices are clamped; an inverted range returns an empty multifield.
fn builtin_subseq_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("subseq$", args, 3, span)?;
    let values = eval_args(ctx, args)?;

    let Value::Multifield(mf) = &values[0] else {
        return Err(EvalError::TypeError {
            function: "subseq$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[0]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Integer(start_raw) = values[1] else {
        return Err(EvalError::TypeError {
            function: "subseq$".to_string(),
            expected: "INTEGER (start)".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Integer(end_raw) = values[2] else {
        return Err(EvalError::TypeError {
            function: "subseq$".to_string(),
            expected: "INTEGER (end)".to_string(),
            actual: generic_value_type_name(&values[2]).to_string(),
            span: span.cloned(),
        });
    };

    #[allow(clippy::cast_possible_wrap)] // multifield length fits in i64 in practice
    let len = mf.len() as i64;
    if len == 0 {
        return Ok(Value::Multifield(Box::default()));
    }

    let start = start_raw.max(1);
    let end = end_raw.min(len);
    if start > end {
        return Ok(Value::Multifield(Box::default()));
    }

    let Some(start_idx) = usize::try_from(start - 1).ok() else {
        return Ok(Value::Multifield(Box::default()));
    };
    let Some(end_idx_inclusive) = usize::try_from(end - 1).ok() else {
        return Ok(Value::Multifield(Box::default()));
    };

    let mut out = ferric_rules_core::value::Multifield::new();
    for value in &mf[start_idx..=end_idx_inclusive] {
        out.push(value.clone());
    }
    Ok(Value::Multifield(Box::new(out)))
}

/// `nth` — compatibility alias for `nth$`.
fn builtin_nth(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_nth(ctx, args, span, "nth")
}

/// `implode$` — convert a multifield to a space-separated STRING, quoting and
/// escaping its STRING fields.
fn builtin_implode_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("implode$", args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    match &val {
        Value::Multifield(mf) => {
            let mut result = String::new();
            for (idx, element) in mf.iter().enumerate() {
                if idx > 0 {
                    result.push(' ');
                }
                crate::value_print::append_implode_field(
                    element,
                    &ctx.engine.symbol_table,
                    &mut result,
                );
            }
            let fs =
                FerricString::new(&result, ctx.engine.config.string_encoding).map_err(|e| {
                    EvalError::TypeError {
                        function: "implode$".to_string(),
                        expected: "encodable string".to_string(),
                        actual: format!("{e}"),
                        span: span.cloned(),
                    }
                })?;
            Ok(Value::String(fs))
        }
        _ => Err(EvalError::TypeError {
            function: "implode$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&val).to_string(),
            span: span.cloned(),
        }),
    }
}

/// `nth$` — get the nth element of a multifield (1-indexed).
///
/// Returns the selected value, or the symbol `nil` for an absent position.
fn builtin_nth_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_nth(ctx, args, span, "nth$")
}

fn eval_nth(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    function: &str,
) -> Result<Value, EvalError> {
    check_arity_exact(function, args, 2, span)?;
    let index = match eval_inner(ctx, &args[0])? {
        Value::Integer(index) => Numeric::Int(index),
        Value::Float(index) => Numeric::Flt(index),
        other => {
            return Err(EvalError::TypeError {
                function: function.to_string(),
                expected: "INTEGER or FLOAT (index)".to_string(),
                actual: generic_value_type_name(&other).to_string(),
                span: span.cloned(),
            });
        }
    };
    // Even an absent numeric position must evaluate and validate the multifield.
    let value = eval_inner(ctx, &args[1])?;
    let Value::Multifield(mf) = value else {
        return Err(EvalError::TypeError {
            function: function.to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&value).to_string(),
            span: span.cloned(),
        });
    };
    let position = match index {
        Numeric::Int(index) => index.checked_sub(1),
        Numeric::Flt(index) => {
            // Runtime FLOAT indices truncate toward zero. The exclusive upper
            // bound is 2^63: i64::MAX rounds up to it when represented as f64.
            if index.is_finite() && (1.0..9_223_372_036_854_775_808.0).contains(&index) {
                #[allow(clippy::cast_possible_truncation)]
                let integer = index as i64;
                Some(integer - 1)
            } else {
                None
            }
        }
    };
    if let Some(value) = position
        .and_then(|position| usize::try_from(position).ok())
        .and_then(|position| mf.get(position))
    {
        return Ok(value.clone());
    }
    let nil = ctx
        .engine
        .symbol_table
        .intern_symbol("nil", ctx.engine.config.string_encoding)
        .expect("nil is valid ASCII");
    Ok(Value::Symbol(nil))
}

/// `member$` — find a scalar or contiguous subsequence in a multifield.
///
/// Returns the first 1-based index for a scalar or singleton needle, an inclusive
/// two-INTEGER range for other matching sequences, or the symbol FALSE. An empty
/// needle matches as (1 0) only when the haystack is nonempty.
fn builtin_member_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_member(ctx, args, span, "member$")
}

/// `member` — compatibility alias for `member$`.
fn builtin_member(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    eval_member(ctx, args, span, "member")
}

fn eval_member(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    function: &str,
) -> Result<Value, EvalError> {
    check_arity_exact(function, args, 2, span)?;
    let values = eval_args(ctx, args)?;
    let Value::Multifield(haystack) = &values[1] else {
        return Err(EvalError::TypeError {
            function: function.to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    let needle = match &values[0] {
        Value::Multifield(needle) => needle.as_slice(),
        scalar => std::slice::from_ref(scalar),
    };
    let offset = if haystack.is_empty() {
        None
    } else if needle.is_empty() {
        // CLIPS returns (1 0) here. Guard before windows(), which rejects zero.
        Some(0)
    } else {
        haystack.windows(needle.len()).position(|window| {
            window
                .iter()
                .zip(needle)
                .all(|(element, expected)| element.structural_eq(expected))
        })
    };
    if let Some(offset) = offset {
        // Positions are bounded by the allocated multifield's length; even the
        // empty-needle result has representable endpoints (1, 0).
        let start = i64::try_from(offset + 1).expect("multifield index fits in INTEGER");
        if needle.len() == 1 {
            return Ok(Value::Integer(start));
        }
        let end = i64::try_from(offset + needle.len()).expect("multifield index fits in INTEGER");
        let range = [Value::Integer(start), Value::Integer(end)]
            .into_iter()
            .collect();
        return Ok(Value::Multifield(Box::new(range)));
    }
    Ok(clips_bool(
        false,
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
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
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

// ---------------------------------------------------------------------------
// Multifield modification built-ins
// ---------------------------------------------------------------------------

/// `insert$` — insert values at a position in a multifield.
///
/// `(insert$ <multifield> <integer-index> <value>+)` — 1-based index.
/// Index 1 = before first element. Index > length = append.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn builtin_insert_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("insert$", args, 3, span)?;
    let values = eval_args(ctx, args)?;
    let Value::Multifield(mf) = &values[0] else {
        return Err(EvalError::TypeError {
            function: "insert$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[0]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Integer(index) = &values[1] else {
        return Err(EvalError::TypeError {
            function: "insert$".to_string(),
            expected: "INTEGER (index)".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    // Convert 1-based to 0-based, clamping to valid range.
    let insert_pos = ((*index).max(1) - 1) as usize;
    let insert_pos = insert_pos.min(mf.len());
    // Collect values to insert, splicing multifields.
    let mut to_insert = Vec::new();
    for v in &values[2..] {
        match v {
            Value::Multifield(inner) => to_insert.extend(inner.iter().cloned()),
            _ => to_insert.push(v.clone()),
        }
    }
    // Build result by splicing insert_pos prefix, new values, then suffix.
    let mut result = ferric_rules_core::value::Multifield::new();
    for v in mf[..insert_pos].iter().cloned() {
        result.push(v);
    }
    for v in to_insert {
        result.push(v);
    }
    for v in mf[insert_pos..].iter().cloned() {
        result.push(v);
    }
    Ok(Value::Multifield(Box::new(result)))
}

/// `delete$` — remove a range from a multifield.
///
/// `(delete$ <multifield> <begin> <end>)` — 1-based inclusive indices.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn builtin_delete_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("delete$", args, 3, span)?;
    let values = eval_args(ctx, args)?;
    let Value::Multifield(mf) = &values[0] else {
        return Err(EvalError::TypeError {
            function: "delete$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[0]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Integer(begin) = &values[1] else {
        return Err(EvalError::TypeError {
            function: "delete$".to_string(),
            expected: "INTEGER (begin)".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Integer(end) = &values[2] else {
        return Err(EvalError::TypeError {
            function: "delete$".to_string(),
            expected: "INTEGER (end)".to_string(),
            actual: generic_value_type_name(&values[2]).to_string(),
            span: span.cloned(),
        });
    };
    let begin_0 = ((*begin).max(1) - 1) as usize;
    let end_0 = (*end).max(0) as usize;
    let mut result = ferric_rules_core::value::Multifield::new();
    let end_clamped = end_0.min(mf.len());
    if begin_0 < mf.len() && end_0 >= 1 && begin_0 < end_clamped {
        for v in mf[..begin_0].iter().cloned() {
            result.push(v);
        }
        for v in mf[end_clamped..].iter().cloned() {
            result.push(v);
        }
    } else {
        for v in mf.iter().cloned() {
            result.push(v);
        }
    }
    Ok(Value::Multifield(Box::new(result)))
}

/// `replace$` — replace a range in a multifield with new values.
///
/// `(replace$ <multifield> <begin> <end> <value>+)` — 1-based inclusive.
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
fn builtin_replace_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("replace$", args, 4, span)?;
    let values = eval_args(ctx, args)?;
    let Value::Multifield(mf) = &values[0] else {
        return Err(EvalError::TypeError {
            function: "replace$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&values[0]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Integer(begin) = &values[1] else {
        return Err(EvalError::TypeError {
            function: "replace$".to_string(),
            expected: "INTEGER (begin)".to_string(),
            actual: generic_value_type_name(&values[1]).to_string(),
            span: span.cloned(),
        });
    };
    let Value::Integer(end) = &values[2] else {
        return Err(EvalError::TypeError {
            function: "replace$".to_string(),
            expected: "INTEGER (end)".to_string(),
            actual: generic_value_type_name(&values[2]).to_string(),
            span: span.cloned(),
        });
    };
    let begin_0 = ((*begin).max(1) - 1) as usize;
    let end_0 = (*end).max(0) as usize;
    // Collect replacement values, splicing multifields.
    let mut replacements = Vec::new();
    for v in &values[3..] {
        match v {
            Value::Multifield(inner) => replacements.extend(inner.iter().cloned()),
            _ => replacements.push(v.clone()),
        }
    }
    let end_clamped = end_0.min(mf.len());
    let mut result = ferric_rules_core::value::Multifield::new();
    if begin_0 <= end_clamped {
        for v in mf[..begin_0].iter().cloned() {
            result.push(v);
        }
        for v in replacements {
            result.push(v);
        }
        for v in mf[end_clamped..].iter().cloned() {
            result.push(v);
        }
    } else {
        for v in mf.iter().cloned() {
            result.push(v);
        }
    }
    Ok(Value::Multifield(Box::new(result)))
}

/// Delete or replace every scalar/subsequence search match in a multifield.
fn builtin_edit_members(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
    replace: bool,
) -> Result<Value, EvalError> {
    let name = if replace {
        "replace-member$"
    } else {
        "delete-member$"
    };
    check_arity_min(name, args, if replace { 3 } else { 2 }, span)?;
    let value = eval_inner(ctx, &args[0])?;
    let Value::Multifield(fields) = value else {
        return Err(EvalError::TypeError {
            function: name.into(),
            expected: "MULTIFIELD".into(),
            actual: generic_value_type_name(&value).into(),
            span: span.cloned(),
        });
    };
    let mut fields: Vec<Value> = fields.iter().cloned().collect();
    let replacement = if replace {
        match eval_inner(ctx, &args[1])? {
            Value::Multifield(fields) => fields.iter().cloned().collect(),
            value => vec![value],
        }
    } else {
        Vec::new()
    };
    let patterns = eval_args(ctx, &args[if replace { 2 } else { 1 }..])?;
    let mut cursor = 0;
    while let Some((position, length)) = find_member_pattern(&fields, &patterns, cursor) {
        if length == 0 {
            return Err(EvalError::TypeError {
                function: name.into(),
                expected: "nonempty search pattern for a nonempty multifield".into(),
                actual: "empty MULTIFIELD".into(),
                span: span.cloned(),
            });
        }
        fields.splice(position..position + length, replacement.iter().cloned());
        // Deletion can create a new match across the removed fields. Replacement
        // skips its inserted fields, so a replacement containing the search
        // pattern cannot repeatedly replace itself.
        cursor = if replace {
            position + replacement.len()
        } else {
            0
        };
    }
    Ok(Value::Multifield(Box::new(fields.into_iter().collect())))
}

fn find_member_pattern(
    fields: &[Value],
    patterns: &[Value],
    start: usize,
) -> Option<(usize, usize)> {
    for position in start..fields.len() {
        for pattern in patterns {
            let expected = match pattern {
                Value::Multifield(values) => values.as_slice(),
                value => std::slice::from_ref(value),
            };
            if fields[position..].len() >= expected.len()
                && fields[position..position + expected.len()]
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| actual.structural_eq(expected))
            {
                return Some((position, expected.len()));
            }
        }
    }
    None
}

/// `first$` — return first element as a single-element multifield.
fn builtin_first_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("first$", args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    let Value::Multifield(mf) = &val else {
        return Err(EvalError::TypeError {
            function: "first$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&val).to_string(),
            span: span.cloned(),
        });
    };
    match mf.first() {
        Some(v) => Ok(Value::Multifield(Box::new(
            std::iter::once(v.clone()).collect(),
        ))),
        None => Ok(Value::Multifield(
            Box::<ferric_rules_core::value::Multifield>::default(),
        )),
    }
}

/// `rest$` — return all but first element as a multifield.
fn builtin_rest_mf(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("rest$", args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    let Value::Multifield(mf) = &val else {
        return Err(EvalError::TypeError {
            function: "rest$".to_string(),
            expected: "MULTIFIELD".to_string(),
            actual: generic_value_type_name(&val).to_string(),
            span: span.cloned(),
        });
    };
    if mf.len() <= 1 {
        Ok(Value::Multifield(
            Box::<ferric_rules_core::value::Multifield>::default(),
        ))
    } else {
        Ok(Value::Multifield(Box::new(
            mf[1..].iter().cloned().collect(),
        )))
    }
}

/// `sort` — `(sort <predicate> <value>...)`.
///
/// Scalar values and the fields of multifield values form one sequence, which
/// is merge-sorted in CLIPS 6.30's comparison order. The predicate is called
/// with the current left and right fields; any result other than the symbol
/// `FALSE` puts the right field first, so `>` sorts ascending.
fn builtin_sort(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("sort", args, 1, span)?;
    let name_value = eval_inner(ctx, &args[0])?;
    let Value::Symbol(symbol) = name_value else {
        return Err(EvalError::TypeError {
            function: "sort".to_string(),
            expected: "SYMBOL (function name)".to_string(),
            actual: generic_value_type_name(&name_value).to_string(),
            span: span.cloned(),
        });
    };
    let name = ctx
        .engine
        .symbol_table
        .resolve_symbol_str(symbol)
        .unwrap_or("")
        .to_string();
    let predicate = resolve_named_callable(ctx, &name, span)?;
    let mut fields = Vec::new();
    for arg in &args[1..] {
        match eval_inner(ctx, arg)? {
            Value::Multifield(mf) => fields.extend(mf.iter().cloned()),
            value => fields.push(value),
        }
    }
    let mut scratch = vec![Value::Void; fields.len()];
    merge_sort_fields(ctx, &predicate, &name, &mut fields, &mut scratch, span)?;
    Ok(Value::Multifield(Box::new(fields.into_iter().collect())))
}

/// Top-down merge sort with the left half rounded up, comparing each pair of
/// current heads once: the order in which CLIPS 6.30 calls the predicate.
fn merge_sort_fields(
    ctx: &mut EvalContext<'_>,
    predicate: &NamedCallable,
    name: &str,
    fields: &mut [Value],
    scratch: &mut [Value],
    span: Option<&SourceSpan>,
) -> Result<(), EvalError> {
    let len = fields.len();
    if len < 2 {
        return Ok(());
    }
    let middle = len.div_ceil(2);
    {
        let (left, right) = fields.split_at_mut(middle);
        let (left_scratch, right_scratch) = scratch.split_at_mut(middle);
        merge_sort_fields(ctx, predicate, name, left, left_scratch, span)?;
        merge_sort_fields(ctx, predicate, name, right, right_scratch, span)?;
    }
    let (mut left, mut right) = (0, middle);
    for slot in scratch.iter_mut() {
        let take_right = if left == middle {
            true
        } else if right == len {
            false
        } else {
            let pair = [
                RuntimeExpr::Literal(fields[left].clone()),
                RuntimeExpr::Literal(fields[right].clone()),
            ];
            let result = predicate.call(ctx, name, &pair, span.cloned())?;
            !matches!(result, Value::Symbol(sym)
                if ctx.engine.symbol_table.resolve_symbol_str(sym) == Some("FALSE"))
        };
        let taken = if take_right { &mut right } else { &mut left };
        // A consumed field is never compared again, so move it out.
        std::mem::swap(slot, &mut fields[*taken]);
        *taken += 1;
    }
    fields.swap_with_slice(scratch);
    Ok(())
}

// ---------------------------------------------------------------------------
// Dynamic dispatch built-ins
// ---------------------------------------------------------------------------

/// `funcall` — call a function by name at runtime.
///
/// `(funcall <name> <args>...)` — evaluates the first argument to get a
/// function name (symbol or string), then dispatches with remaining args.
fn builtin_funcall(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("funcall", args, 1, span)?;
    let name_val = eval_inner(ctx, &args[0])?;
    let fn_name = match &name_val {
        Value::Symbol(sym) => ctx
            .engine
            .symbol_table
            .resolve_symbol_str(*sym)
            .unwrap_or("")
            .to_string(),
        Value::String(s) => s.as_str().to_string(),
        _ => {
            return Err(EvalError::TypeError {
                function: "funcall".to_string(),
                expected: "SYMBOL or STRING (function name)".to_string(),
                actual: generic_value_type_name(&name_val).to_string(),
                span: span.cloned(),
            })
        }
    };
    resolve_named_callable(ctx, &fn_name, span)?;
    // funcall evaluates its operands before invoking even a short-circuit target
    // or checking that target's arity. Resolve first so an unknown name does not
    // evaluate operands; preserve values as single arguments until dispatch.
    let arguments = eval_args(ctx, &args[1..])?
        .into_iter()
        .map(RuntimeExpr::Literal)
        .collect::<Vec<_>>();
    // random's own count check draws and returns that value with a notice.
    if fn_name != "random" {
        validate_expanded_arity(&fn_name, arguments.len(), span)?;
    }
    // Eager operands may build a replacement before the target starts executing.
    resolve_named_callable(ctx, &fn_name, span)?.call(ctx, &fn_name, &arguments, span.cloned())
}

/// A function named by a runtime value (`funcall`, `sort`).
enum NamedCallable {
    Builtin,
    Function(UserFunction, crate::modules::ModuleId),
    Generic(GenericFunction, crate::modules::ModuleId),
}

impl NamedCallable {
    fn call(
        &self,
        ctx: &mut EvalContext<'_>,
        name: &str,
        args: &[RuntimeExpr],
        span: Option<SourceSpan>,
    ) -> Result<Value, EvalError> {
        match self {
            Self::Builtin => dispatch_builtin(ctx, name, args, span),
            Self::Function(func, module) => dispatch_user_function(ctx, func, *module, args, span),
            Self::Generic(generic, module) => dispatch_generic(ctx, generic, *module, args, span),
        }
    }
}

/// Resolve a function name from the current module: builtins first, then
/// visible deffunctions, then visible defgenerics.
fn resolve_named_callable(
    ctx: &EvalContext<'_>,
    name: &str,
    span: Option<&SourceSpan>,
) -> Result<NamedCallable, EvalError> {
    if is_builtin_callable(name) {
        return Ok(NamedCallable::Builtin);
    }

    let function_modules = sorted_dedup_modules(ctx.engine.functions.modules_for_name(name));
    if let Some(target_module) = resolve_unqualified_callable_module(
        ctx,
        name,
        "deffunction",
        &function_modules,
        ctx.engine.functions.contains(ctx.current_module, name),
        AmbiguityMessages {
            expected: "unambiguous deffunction resolution",
            actual: "multiple visible deffunctions; use MODULE::name",
        },
        span.cloned(),
    )? {
        if let Some(func) = ctx.engine.functions.get(target_module, name) {
            return Ok(NamedCallable::Function(func.clone(), target_module));
        }
    }

    let generic_modules = sorted_dedup_modules(ctx.engine.generics.modules_for_name(name));
    if let Some(target_module) = resolve_unqualified_callable_module(
        ctx,
        name,
        "defgeneric",
        &generic_modules,
        ctx.engine.generics.contains(ctx.current_module, name),
        AmbiguityMessages {
            expected: "unambiguous defgeneric resolution",
            actual: "multiple visible defgenerics; use MODULE::name",
        },
        span.cloned(),
    )? {
        if let Some(generic) = ctx.engine.generics.get(target_module, name) {
            return Ok(NamedCallable::Generic(generic.clone(), target_module));
        }
    }

    Err(EvalError::UnknownFunction {
        name: name.to_string(),
        span: span.cloned(),
    })
}

/// Whether a call named `name` would run a deffunction or defgeneric (not a
/// builtin), resolved from the current module as evaluating the call would.
pub(crate) fn call_names_user_callable(ctx: &EvalContext<'_>, name: &str) -> bool {
    if is_module_qualified(name) {
        let Ok(QualifiedName::Qualified { module, name }) = parse_qualified_name(name) else {
            return false;
        };
        return ctx
            .engine
            .module_registry
            .get_by_name(&module)
            .is_some_and(|module| {
                ctx.engine.functions.get(module, &name).is_some()
                    || ctx.engine.generics.get(module, &name).is_some()
            });
    }
    matches!(
        resolve_named_callable(ctx, name, None),
        Ok(NamedCallable::Function(..) | NamedCallable::Generic(..))
    )
}

// ===========================================================================
// I/O and environment builtins
// ===========================================================================

/// `close` — router close command accepted for compatibility.
fn builtin_close(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("close", args, 1, span)?;
    let _ = eval_inner(ctx, &args[0])?;
    Ok(clips_true(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

fn builtin_break(args: &[RuntimeExpr], span: Option<&SourceSpan>) -> Result<Value, EvalError> {
    check_arity_exact("break", args, 0, span)?;
    Err(EvalError::BreakControl {
        span: span.cloned(),
    })
}

/// `return` — unwind the current callable with its argument (or VOID).
fn builtin_return(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let value = match args {
        [] => Value::Void,
        [expr] => eval_inner(ctx, expr)?,
        _ => {
            return Err(EvalError::ArityMismatch {
                name: "return".to_string(),
                expected: "0 or 1".to_string(),
                actual: args.len(),
                span: span.cloned(),
            });
        }
    };
    Err(EvalError::ReturnControl {
        value,
        span: span.cloned(),
    })
}

/// `load` — runtime load command placeholder (currently non-mutating).
fn builtin_load(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("load", args, 1, span)?;
    let path_value = eval_inner(ctx, &args[0])?;
    match path_value {
        Value::String(_) | Value::Symbol(_) => Ok(clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        )),
        _ => Err(EvalError::TypeError {
            function: "load".to_string(),
            expected: "STRING or SYMBOL".to_string(),
            actual: generic_value_type_name(&path_value).to_string(),
            span: span.cloned(),
        }),
    }
}

/// `undefrule` — runtime rule removal placeholder accepted for compatibility.
fn builtin_undefrule(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("undefrule", args, 1, span)?;
    let _ = eval_args(ctx, args)?;
    Ok(clips_true(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `ppdefrule` — pretty-print command placeholder accepted for compatibility.
fn builtin_ppdefrule(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("ppdefrule", args, 1, span)?;
    let _ = eval_inner(ctx, &args[0])?;
    Ok(clips_true(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `rules` — command accepted in expression contexts; action path owns output.
fn builtin_rules(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::ArityMismatch {
            name: "rules".to_string(),
            expected: "0 or 1".to_string(),
            actual: args.len(),
            span: span.cloned(),
        });
    }
    if let Some(arg) = args.first() {
        let _ = eval_inner(ctx, arg)?;
    }
    Ok(clips_true(
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

fn output_channel_name(
    value: &Value,
    symbol_table: &SymbolTable,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<String, EvalError> {
    match value {
        Value::Symbol(sym) => Ok(symbol_table
            .resolve_symbol_str(*sym)
            .unwrap_or("???")
            .to_string()),
        Value::String(s) => Ok(s.as_str().to_string()),
        Value::InstanceName(name) => Ok(symbol_table
            .resolve_symbol_str(name.as_symbol())
            .unwrap_or("???")
            .to_string()),
        Value::Integer(n) => Ok(n.to_string()),
        Value::Float(f) => Ok(f.to_string()),
        other => Err(EvalError::TypeError {
            function: function.to_string(),
            expected: "SYMBOL, STRING, INSTANCE-NAME, INTEGER, or FLOAT channel".to_string(),
            actual: generic_value_type_name(other).to_string(),
            span: span.cloned(),
        }),
    }
}

/// `printout` — evaluator-level output command for deffunction/method bodies.
///
/// Events are queued into `GlobalStore` and flushed by the action executor
/// after each evaluation step that has access to an output router.
fn builtin_printout(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_min("printout", args, 1, span)?;
    let channel_value = eval_inner(ctx, &args[0])?;
    let channel = output_channel_name(&channel_value, &ctx.engine.symbol_table, "printout", span)?;
    if channel == "nil" {
        return Ok(Value::Void);
    }

    for expr in &args[1..] {
        let value = eval_inner(ctx, expr)?;
        let mut output = String::new();
        crate::value_print::append_printout_value(&value, &ctx.engine.symbol_table, &mut output);
        // Queue each completed argument before evaluating the next one, so
        // nested output and errors preserve the already-written prefix.
        ctx.engine
            .globals
            .push_printout_event(channel.clone(), output);
    }

    Ok(Value::Void)
}

/// `format` — CLIPS `printf`-style formatting.
///
/// `(format <channel> <control> <arg>*)`
///
/// Returns the formatted STRING and writes it to the evaluated channel unless
/// its logical name is `nil`. As in CLIPS, the whole control string is checked
/// and the argument count must match its directives before any argument is
/// evaluated. The completed output is queued only after formatting succeeds.
/// See [`crate::formatting`] for the directives.
fn builtin_format(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    use crate::formatting::Piece;

    check_arity_min("format", args, 2, span)?;
    let channel_value = eval_inner(ctx, &args[0])?;
    let channel = output_channel_name(&channel_value, &ctx.engine.symbol_table, "format", span)?;
    let control = match eval_inner(ctx, &args[1])? {
        Value::String(s) => s,
        other => {
            return Err(EvalError::TypeError {
                function: "format".to_string(),
                expected: "STRING".to_string(),
                actual: generic_value_type_name(&other).to_string(),
                span: span.cloned(),
            });
        }
    };
    let pieces = crate::formatting::parse(control.as_str()).map_err(|error| {
        EvalError::UnsupportedOperation {
            operation: "format".to_string(),
            reason: error.to_string(),
            span: span.cloned(),
        }
    })?;
    let directives = pieces
        .iter()
        .filter(|piece| matches!(piece, Piece::Directive(..)))
        .count();
    if args.len() != directives + 2 {
        return Err(EvalError::ArityMismatch {
            name: "format".to_string(),
            expected: format!("exactly {}", directives + 2),
            actual: args.len(),
            span: span.cloned(),
        });
    }

    let mut output = String::new();
    let mut operands = args[2..].iter();
    for piece in pieces {
        match piece {
            Piece::Text(text) => output.push_str(text),
            Piece::Directive(conversion, spec) => {
                let operand = operands.next().expect("operand count was checked");
                let value = eval_inner(ctx, operand)?;
                render_format_value(
                    &mut output,
                    conversion,
                    spec,
                    &value,
                    &ctx.engine.symbol_table,
                )
                .map_err(|expected| EvalError::TypeError {
                    function: "format".to_string(),
                    expected: expected.to_string(),
                    actual: generic_value_type_name(&value).to_string(),
                    span: span.cloned(),
                })?;
            }
        }
    }
    let fs = FerricString::new(&output, ctx.engine.config.string_encoding).map_err(|_| {
        EvalError::TypeError {
            function: "format".to_string(),
            expected: "valid string encoding".to_string(),
            actual: "result contains invalid characters".to_string(),
            span: span.cloned(),
        }
    })?;
    if channel != "nil" {
        ctx.engine.globals.push_printout_event(channel, output);
    }
    Ok(Value::String(fs))
}

/// Render one `format` argument, or name the types its directive accepts.
fn render_format_value(
    out: &mut String,
    conversion: crate::formatting::Conversion,
    spec: crate::formatting::Spec,
    value: &Value,
    symbols: &SymbolTable,
) -> Result<(), &'static str> {
    use crate::formatting::{self as fmt, Conversion};

    fn lexeme<'a>(value: &'a Value, symbols: &'a SymbolTable) -> Option<&'a str> {
        match value {
            Value::String(s) => Some(s.as_str()),
            Value::Symbol(symbol) => symbols.resolve_symbol_str(*symbol),
            Value::InstanceName(name) => symbols.resolve_symbol_str(name.as_symbol()),
            _ => None,
        }
    }

    match conversion {
        Conversion::Decimal | Conversion::Octal | Conversion::Hex | Conversion::Unsigned => {
            let number = match value {
                Value::Integer(i) => *i,
                // C truncates toward zero; Rust's cast also saturates.
                #[allow(clippy::cast_possible_truncation)]
                Value::Float(f) => *f as i64,
                _ => return Err("INTEGER or FLOAT"),
            };
            fmt::render_integer(out, conversion, number, spec);
        }
        Conversion::Fixed | Conversion::Scientific | Conversion::General => {
            let number = match value {
                Value::Float(f) => *f,
                #[allow(clippy::cast_precision_loss)]
                Value::Integer(i) => *i as f64,
                _ => return Err("INTEGER or FLOAT"),
            };
            fmt::render_float(out, number, conversion, spec);
        }
        Conversion::Lexeme => {
            let text = lexeme(value, symbols).ok_or("STRING, SYMBOL, or INSTANCE-NAME")?;
            fmt::render_lexeme(out, text, spec);
        }
        Conversion::Character => {
            let byte = match value {
                Value::Integer(i) => i.to_le_bytes()[0],
                Value::String(_) | Value::Symbol(_) => lexeme(value, symbols)
                    .and_then(|text| text.bytes().next())
                    .unwrap_or(0),
                _ => return Err("INTEGER, STRING, or SYMBOL"),
            };
            fmt::render_character(out, byte, spec);
        }
    }
    Ok(())
}

/// Intern the `EOF` symbol — shared helper for `read`/`readline`.
fn intern_eof_symbol(
    ctx: &mut EvalContext<'_>,
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    let sym = ctx
        .engine
        .symbol_table
        .intern_symbol("EOF", ctx.engine.config.string_encoding)
        .map_err(|_| EvalError::TypeError {
            function: "read".to_string(),
            expected: "valid string encoding".to_string(),
            actual: "cannot intern EOF symbol".to_string(),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// `read` — read one CLIPS field from the queued input lines, then from the
/// engine's input source.
///
/// `(read)` or `(read <channel>)`
///
/// Like CLIPS reading `stdin`, each call consumes whole lines until one
/// contains a field, returns that line's first field and discards the rest.
/// Returns the symbol `EOF` when the input runs out first.
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
        let _ = eval_inner(ctx, &args[0])?;
    }

    loop {
        let Some(line) = ctx.engine.next_input_line() else {
            return intern_eof_symbol(ctx, span);
        };
        match scan_field(ctx, &mut FieldScanner::new(line.as_bytes())) {
            FieldToken::Stop => {}
            FieldToken::Unknown => {
                return scanned_string(ctx, "*** READ ERROR ***", "read", span);
            }
            token => return scanned_field_value(ctx, token, "read", span),
        }
    }
}

/// `readline` — read a complete line from the queued input, or else from the
/// engine's input source, as a string.
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
        let _ = eval_inner(ctx, &args[0])?;
    }

    match ctx.engine.next_input_line() {
        Some(line) => {
            let fs = FerricString::new(&line, ctx.engine.config.string_encoding).map_err(|_| {
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

    let focus_id = ctx.engine.module_registry.current_focus();
    let module_name = focus_id
        .and_then(|id| ctx.engine.module_registry.module_name(id))
        .unwrap_or("MAIN");

    let sym = ctx
        .engine
        .symbol_table
        .intern_symbol(module_name, ctx.engine.config.string_encoding)
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

    let stack = ctx.engine.module_registry.focus_stack();
    let mut result = ferric_rules_core::value::Multifield::new();

    // Return in top-first order (reverse of internal stack order)
    for &module_id in stack.iter().rev() {
        let name = ctx
            .engine
            .module_registry
            .module_name(module_id)
            .unwrap_or("???");
        let sym = ctx
            .engine
            .symbol_table
            .intern_symbol(name, ctx.engine.config.string_encoding)
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
// Fact introspection builtins
// ===========================================================================

/// The fact a CLIPS fact designator names: a fact address, or an INTEGER
/// fact index as `fact-index` returns it. Both forms resolve only live facts;
/// integer values are never decoded as internal slot-map keys.
pub(crate) fn designated_fact(
    fact_base: &FactBase,
    initial_fact_id: Option<FactId>,
    zero_based: bool,
    epoch: u64,
    value: &Value,
) -> Option<FactId> {
    if let Value::FactAddress(address) = value {
        return live_fact_id(fact_base, epoch, address);
    }
    let Value::Integer(index) = *value else {
        return None;
    };
    let index = u64::try_from(index).ok()?;
    fact_base
        .iter()
        .map(|(id, _)| id)
        .find(|&id| public_fact_index(fact_base, initial_fact_id, zero_based, id) == Some(index))
}

/// Evaluate a fact-function designator argument. `None` means the value
/// names no live fact: a missing or negative index, a stale or dummy address,
/// or a value of another type. CLIPS 6.30 reports each of these with a
/// recoverable notice and returns `FALSE`, so none of them stops the rule.
fn eval_fact_designator(
    ctx: &mut EvalContext<'_>,
    arg: &RuntimeExpr,
) -> Result<Option<FactId>, EvalError> {
    let value = eval_inner(ctx, arg)?;
    Ok(designated_fact(
        &ctx.engine.fact_base,
        ctx.engine.initial_fact_id,
        ctx.engine.fact_index_starts_at_zero,
        ctx.engine.fact_epoch,
        &value,
    ))
}

/// `(fact-existp <fact-address-or-index>)` — whether the fact is in working memory.
fn builtin_fact_existp(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("fact-existp", args, 1, span)?;
    let fact = eval_fact_designator(ctx, &args[0])?;
    let exists = fact.is_some();
    Ok(clips_bool(
        exists,
        &mut ctx.engine.symbol_table,
        ctx.engine.config.string_encoding,
    ))
}

/// `(fact-index <fact-address>)` — return the public assertion index, or -1
/// when the addressed fact has been retracted. Like CLIPS 6.30, any other
/// argument type (including an INTEGER index) also returns -1 and continues.
fn builtin_fact_index(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("fact-index", args, 1, span)?;
    let val = eval_inner(ctx, &args[0])?;
    let Value::FactAddress(address) = &val else {
        return Ok(Value::Integer(-1));
    };
    if live_fact_id(&ctx.engine.fact_base, ctx.engine.fact_epoch, address).is_none() {
        return Ok(Value::Integer(-1));
    }
    let index = address
        .public_index()
        .and_then(|index| i64::try_from(index).ok())
        .ok_or_else(|| EvalError::UnsupportedOperation {
            operation: "fact-index".into(),
            reason: "fact index exceeds the signed 64-bit integer range".into(),
            span: span.cloned(),
        })?;
    Ok(Value::Integer(index))
}

/// `(fact-relation <fact-address-or-index>)` — the relation name of a fact as a SYMBOL.
///
/// For ordered facts this is the relation symbol; for template facts it is the
/// template name. Returns `FALSE` if the fact does not exist.
fn builtin_fact_relation(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("fact-relation", args, 1, span)?;
    let fact = eval_fact_designator(ctx, &args[0])?;
    let Some(fact_id) = fact else {
        return Ok(clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ));
    };
    let Some(entry) = ctx.engine.fact_base.get(fact_id) else {
        return Ok(clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ));
    };
    let relation_name: String = match &entry.fact {
        ferric_rules_core::Fact::Ordered(of) => ctx
            .engine
            .symbol_table
            .resolve_symbol_str(of.relation)
            .unwrap_or("???")
            .to_string(),
        ferric_rules_core::Fact::Template(tf) => {
            // Look up the template name from template_defs.
            ctx.engine.template_defs.get(tf.template_id).map_or_else(
                || format!("<template:{:?}>", tf.template_id),
                |reg| reg.name.clone(),
            )
        }
    };
    let sym = ctx
        .engine
        .symbol_table
        .intern_symbol(&relation_name, ctx.engine.config.string_encoding)
        .map_err(|_| EvalError::TypeError {
            function: "fact-relation".into(),
            expected: "valid symbol".into(),
            actual: format!("encoding error for {relation_name:?}"),
            span: span.cloned(),
        })?;
    Ok(Value::Symbol(sym))
}

/// Resolve the parser's compact fact-slot form without evaluating the member
/// name as an ordinary variable. Query membership survives loop shadowing.
fn builtin_compact_fact_slot_ref(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact(COMPACT_FACT_SLOT_REF, args, 2, span)?;
    let RuntimeExpr::BoundVar { name, .. } = &args[0] else {
        return Err(EvalError::TypeError {
            function: COMPACT_FACT_SLOT_REF.into(),
            expected: "fact-address variable".into(),
            actual: "non-variable expression".into(),
            span: span.cloned(),
        });
    };
    if name.is_empty() || name.starts_with("$?") {
        return Err(EvalError::TypeError {
            function: COMPACT_FACT_SLOT_REF.into(),
            expected: "named single-field fact-address variable".into(),
            actual: name.clone(),
            span: span.cloned(),
        });
    }
    let RuntimeExpr::Literal(Value::Symbol(slot)) = &args[1] else {
        return Err(EvalError::TypeError {
            function: COMPACT_FACT_SLOT_REF.into(),
            expected: "literal slot symbol".into(),
            actual: "non-symbol expression".into(),
            span: span.cloned(),
        });
    };
    let slot_name = ctx
        .engine
        .symbol_table
        .resolve_symbol_str(*slot)
        .ok_or_else(|| EvalError::TypeError {
            function: COMPACT_FACT_SLOT_REF.into(),
            expected: "registered slot symbol".into(),
            actual: "unknown symbol".into(),
            span: span.cloned(),
        })?;
    let member = ctx
        .compact_fact_bindings
        .and_then(|bindings| bindings.get(name));
    let Some(member) = member else {
        // A colon is also legal in an ordinary local name. Only an active
        // lexical fact member takes precedence over that exact full name.
        let full_name = format!("{name}:{slot_name}");
        if let Some(value) = ordinary_binding(ctx, &full_name) {
            return Ok(value);
        }
        return Err(EvalError::UnboundVariable {
            name: name.clone(),
            span: span.cloned(),
        });
    };
    let fact = member
        .record(&ctx.engine.fact_base, ctx.engine.fact_epoch)
        .ok_or_else(|| EvalError::TypeError {
            function: COMPACT_FACT_SLOT_REF.into(),
            expected: "live or retained query fact".into(),
            actual: format!("fact bound to ?{name} no longer exists"),
            span: span.cloned(),
        })?;
    if matches!(fact, Fact::Ordered(_)) && member.retained.is_none() {
        return Err(EvalError::TypeError {
            function: COMPACT_FACT_SLOT_REF.into(),
            expected: "template fact".into(),
            actual: "ordered fact".into(),
            span: span.cloned(),
        });
    }
    read_record_slot_value(ctx, fact, slot_name, COMPACT_FACT_SLOT_REF, span)?.ok_or_else(|| {
        EvalError::TypeError {
            function: COMPACT_FACT_SLOT_REF.into(),
            expected: "fact record and template metadata".into(),
            actual: "fact context or template metadata unavailable".into(),
            span: span.cloned(),
        }
    })
}

/// `(fact-slot-value <fact-address-or-index> <slot-name>)` — the value of a named slot.
///
/// For template facts, returns the value at the named slot position.
/// For ordered facts, the only valid slot name is `"implied"`, which returns
/// a multifield of all field values. Missing evaluator metadata returns FALSE.
///
/// As in CLIPS 6.30, the designator is resolved before the slot argument is
/// evaluated: a missing or negative index, a stale or dummy address, or a
/// value of another type returns FALSE without evaluating the slot argument.
/// Only an invalid slot, or slot argument type, on a live fact is an error.
fn builtin_fact_slot_value(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("fact-slot-value", args, 2, span)?;
    let Some(fact_id) = eval_fact_designator(ctx, &args[0])? else {
        return Ok(clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ));
    };
    let value = match &args[1] {
        // These operands cannot run engine effects, so the designated fact
        // is still live when its slot is read.
        RuntimeExpr::Literal(_) | RuntimeExpr::BoundVar { .. } | RuntimeExpr::GlobalVar { .. } => {
            let slot = eval_inner(ctx, &args[1])?;
            let slot_name =
                as_lexeme_str(&slot, &ctx.engine.symbol_table, "fact-slot-value", span)?;
            read_fact_slot_value(ctx, fact_id, &slot_name, "fact-slot-value", span, || {
                "fact does not exist".into()
            })?
        }
        // A slot expression may run `(reset)` or a retraction, after which
        // the slotmap key can name a later fact. Like CLIPS 6.30, which keeps
        // the designated record busy, read the record designated before the
        // slot argument ran.
        _ => {
            let is_initial_fact = ctx.engine.initial_fact_id == Some(fact_id);
            let record = ctx
                .engine
                .fact_base
                .get(fact_id)
                .map(|entry| entry.fact.clone());
            let slot = eval_inner(ctx, &args[1])?;
            let slot_name =
                as_lexeme_str(&slot, &ctx.engine.symbol_table, "fact-slot-value", span)?;
            match record {
                None => {
                    return Err(EvalError::TypeError {
                        function: "fact-slot-value".into(),
                        expected: "valid fact index".into(),
                        actual: "fact does not exist".into(),
                        span: span.cloned(),
                    });
                }
                Some(_) if is_initial_fact => {
                    // CLIPS's `initial-fact` is a deftemplate without slots.
                    return Err(EvalError::TypeError {
                        function: "fact-slot-value".into(),
                        expected: "valid slot name in template `initial-fact`".into(),
                        actual: format!("unknown slot `{slot_name}`"),
                        span: span.cloned(),
                    });
                }
                Some(fact) => {
                    read_record_slot_value(ctx, &fact, &slot_name, "fact-slot-value", span)?
                }
            }
        }
    };
    Ok(value.unwrap_or_else(|| {
        clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        )
    }))
}

/// Read a live fact's slot using the current template registry.
fn read_fact_slot_value(
    ctx: &EvalContext<'_>,
    fact_id: FactId,
    slot_name: &str,
    function: &str,
    span: Option<&SourceSpan>,
    missing_fact: impl FnOnce() -> String,
) -> Result<Option<Value>, EvalError> {
    let Some(entry) = ctx.engine.fact_base.get(fact_id) else {
        return Err(EvalError::TypeError {
            function: function.into(),
            expected: "valid fact index".into(),
            actual: missing_fact(),
            span: span.cloned(),
        });
    };
    if ctx.engine.initial_fact_id == Some(fact_id) {
        // CLIPS's `initial-fact` is a deftemplate without slots.
        return Err(EvalError::TypeError {
            function: function.into(),
            expected: "valid slot name in template `initial-fact`".into(),
            actual: format!("unknown slot `{slot_name}`"),
            span: span.cloned(),
        });
    }
    read_record_slot_value(ctx, &entry.fact, slot_name, function, span)
}

/// Shared slot decoding for live explicit introspection and lexical query
/// records. Only compact query-member lookup may supply a retained record.
fn read_record_slot_value(
    ctx: &EvalContext<'_>,
    fact: &Fact,
    slot_name: &str,
    function: &str,
    span: Option<&SourceSpan>,
) -> Result<Option<Value>, EvalError> {
    let value = match fact {
        ferric_rules_core::Fact::Template(tf) => {
            let Some(reg) = ctx.engine.template_defs.get(tf.template_id) else {
                return Ok(None);
            };
            let Some(&pos) = reg.slot_index.get(slot_name) else {
                return Err(EvalError::TypeError {
                    function: function.into(),
                    expected: format!("valid slot name in template `{}`", reg.name),
                    actual: format!("unknown slot `{slot_name}`"),
                    span: span.cloned(),
                });
            };
            tf.slots
                .get(pos)
                .cloned()
                .ok_or_else(|| EvalError::TypeError {
                    function: function.into(),
                    expected: format!("stored slot `{slot_name}` in template `{}`", reg.name),
                    actual: format!("slot position {pos} is absent from the fact"),
                    span: span.cloned(),
                })?
        }
        ferric_rules_core::Fact::Ordered(of) => {
            if slot_name != "implied" {
                return Err(EvalError::TypeError {
                    function: function.into(),
                    expected: r#""implied" (only valid slot for ordered facts)"#.into(),
                    actual: format!("`{slot_name}`"),
                    span: span.cloned(),
                });
            }
            let mf: ferric_rules_core::value::Multifield = of.fields.iter().cloned().collect();
            Value::Multifield(Box::new(mf))
        }
    };
    Ok(Some(value))
}

/// `(fact-slot-names <fact-address-or-index>)` — a fact's slot names as a multifield of SYMBOLs.
///
/// For template facts, returns slot names in declaration order (requires `template_defs`).
/// For ordered facts, returns a single-element multifield `(implied)`, and for
/// the protected `initial-fact` (a slotless deftemplate in CLIPS) `()`.
/// Returns `FALSE` if the fact does not exist.
fn builtin_fact_slot_names(
    ctx: &mut EvalContext<'_>,
    args: &[RuntimeExpr],
    span: Option<&SourceSpan>,
) -> Result<Value, EvalError> {
    check_arity_exact("fact-slot-names", args, 1, span)?;
    let fact = eval_fact_designator(ctx, &args[0])?;
    let Some(fact_id) = fact else {
        return Ok(clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ));
    };
    let Some(entry) = ctx.engine.fact_base.get(fact_id) else {
        return Ok(clips_false(
            &mut ctx.engine.symbol_table,
            ctx.engine.config.string_encoding,
        ));
    };
    if ctx.engine.initial_fact_id == Some(fact_id) {
        return Ok(Value::Multifield(Box::default()));
    }
    match &entry.fact {
        ferric_rules_core::Fact::Template(tf) => {
            let mut result = ferric_rules_core::value::Multifield::new();
            if let Some(reg) = ctx.engine.template_defs.get(tf.template_id) {
                for slot_name in &reg.slot_names {
                    let sym = ctx
                        .engine
                        .symbol_table
                        .intern_symbol(slot_name, ctx.engine.config.string_encoding)
                        .map_err(|_| EvalError::TypeError {
                            function: "fact-slot-names".into(),
                            expected: "valid slot name".into(),
                            actual: format!("encoding error for {slot_name:?}"),
                            span: span.cloned(),
                        })?;
                    result.push(Value::Symbol(sym));
                }
            }
            Ok(Value::Multifield(Box::new(result)))
        }
        ferric_rules_core::Fact::Ordered(_) => {
            // Ordered facts have a single implicit slot named "implied".
            let sym = ctx
                .engine
                .symbol_table
                .intern_symbol("implied", ctx.engine.config.string_encoding)
                .map_err(|_| EvalError::TypeError {
                    function: "fact-slot-names".into(),
                    expected: "valid symbol".into(),
                    actual: "encoding error for \"implied\"".into(),
                    span: span.cloned(),
                })?;
            let mut result = ferric_rules_core::value::Multifield::new();
            result.push(Value::Symbol(sym));
            Ok(Value::Multifield(Box::new(result)))
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_address::make_fact_address;
    use ferric_rules_core::binding::{BindingSet, ValueRef, VarMap};

    fn test_ctx() -> (crate::Engine, VarMap, BindingSet) {
        (
            crate::Engine::new(EngineConfig::utf8()),
            VarMap::new(),
            BindingSet::new(),
        )
    }

    /// Helper to evaluate a `RuntimeExpr` with default context.
    fn eval_expr(expr: &RuntimeExpr) -> Result<Value, EvalError> {
        eval_expr_with_output(expr).0
    }

    fn eval_expr_with_output(
        expr: &RuntimeExpr,
    ) -> (Result<Value, EvalError>, Vec<(String, String)>) {
        let (mut engine, vm, bs) = test_ctx();
        let main_id = engine.module_registry.main_module_id();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: main_id,
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, expr);
        (result, engine.globals.take_printout_events())
    }

    fn test_fact_address(engine: &crate::Engine, id: FactId) -> FactAddress {
        make_fact_address(
            &engine.fact_base,
            engine.initial_fact_id,
            engine.fact_epoch,
            engine.fact_index_starts_at_zero,
            id,
        )
        .expect("test fact is live")
    }

    fn address_value(engine: &crate::Engine, id: FactId) -> Value {
        Value::FactAddress(test_fact_address(engine, id))
    }

    fn eval_index(address: Value, engine: &mut crate::Engine) -> Result<i64, EvalError> {
        with_compact_context(engine, None, |ctx| {
            eval(
                ctx,
                &RuntimeExpr::Call {
                    name: "fact-index".into(),
                    args: vec![RuntimeExpr::Literal(address)],
                    span: Some(SourceSpan { line: 4, column: 7 }),
                },
            )
            .map(|value| match value {
                Value::Integer(index) => index,
                value => panic!("fact-index returned {value:?}"),
            })
        })
    }

    fn compact_test_engine() -> (crate::Engine, FactId) {
        let mut engine = crate::Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                "(deftemplate item (slot value) (multislot tags)) \
                 (deffacts seed (item (value 10) (tags a b)))",
            )
            .unwrap();
        engine.reset().unwrap();
        let template = engine.template_ids["item"];
        let fact = engine.fact_base.facts_by_template(template).next().unwrap();
        (engine, fact)
    }

    fn compact_ref(engine: &mut crate::Engine, member: &str, slot: &str) -> RuntimeExpr {
        let slot = engine
            .symbol_table
            .intern_symbol(slot, engine.config.string_encoding)
            .unwrap();
        RuntimeExpr::Call {
            name: COMPACT_FACT_SLOT_REF.into(),
            args: vec![
                RuntimeExpr::BoundVar {
                    name: member.into(),
                    span: None,
                },
                RuntimeExpr::Literal(Value::Symbol(slot)),
            ],
            span: Some(SourceSpan { line: 4, column: 7 }),
        }
    }

    fn with_compact_context<T>(
        engine: &mut crate::Engine,
        scope: Option<&CompactFactBindings>,
        run: impl FnOnce(&mut EvalContext<'_>) -> T,
    ) -> T {
        with_compact_local_context(engine, scope, None, run)
    }

    fn with_compact_local_context<T>(
        engine: &mut crate::Engine,
        scope: Option<&CompactFactBindings>,
        callable_locals: Option<&mut CallableLocals>,
        run: impl FnOnce(&mut EvalContext<'_>) -> T,
    ) -> T {
        // Deliberately collide with the compact member name: slot lookup must
        // retain the lexical fact even when the ordinary value is a scalar.
        let mut var_map = VarMap::new();
        let mut bindings = BindingSet::new();
        let name = engine
            .symbol_table
            .intern_symbol("f", engine.config.string_encoding)
            .unwrap();
        let variable = var_map.get_or_create(name).unwrap();
        bindings.set(variable, ValueRef::new(Value::Integer(42)));
        let current_module = engine.module_registry.main_module_id();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bindings,
            var_map: &var_map,
            callable_locals,
            call_depth: 0,
            expression_depth: 0,
            current_module,
            method_chain: None,
            compact_fact_bindings: scope,
            engine,
            allow_engine_effects: true,
        };
        run(&mut ctx)
    }

    fn local_read(name: &str) -> RuntimeExpr {
        RuntimeExpr::BoundVar {
            name: name.into(),
            span: None,
        }
    }

    fn source_action(source: &str) -> ferric_rules_parser::ActionExpr {
        let parsed = ferric_rules_parser::parse_sexprs(source, ferric_rules_parser::FileId(0));
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        ferric_rules_parser::interpret_action_expr(&parsed.exprs[0]).unwrap()
    }

    fn eval_source(ctx: &mut EvalContext<'_>, source: &str) -> Result<Value, EvalError> {
        let expression = from_action_expr(
            &source_action(source),
            &mut ctx.engine.symbol_table,
            &ctx.engine.config,
        )?;
        eval(ctx, &expression)
    }

    #[test]
    fn expression_effects_preserve_slot_syntax_and_return_addresses() {
        let mut engine = crate::Engine::new(EngineConfig::utf8());
        engine.load_str("(deftemplate item (slot bind))").unwrap();
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            let first = eval_source(ctx, "(bind ?saved (assert (item (bind (+ 1 2)))))").unwrap();
            assert!(matches!(first, Value::FactAddress(_)));
            assert!(matches!(
                eval_source(ctx, "(fact-slot-value ?saved bind)").unwrap(),
                Value::Integer(3)
            ));
            let changed = eval_source(ctx, "(bind ?saved (modify ?saved (bind 4)))").unwrap();
            assert!(!first.structural_eq(&changed));
            let copied = eval_source(ctx, "(duplicate ?saved (bind 5))").unwrap();
            assert!(matches!(copied, Value::FactAddress(_)));
            assert!(matches!(
                eval_source(ctx, "(retract ?saved)").unwrap(),
                Value::Void
            ));
            assert!(!is_truthy(
                &eval_source(ctx, "(fact-existp ?saved)").unwrap(),
                &ctx.engine.symbol_table
            ));
        });
    }

    #[test]
    fn engine_effect_permission_is_inherited_and_checked_before_operands() {
        let mut engine = crate::Engine::new(EngineConfig::utf8());
        engine
            .load_str(
                "(defglobal ?*touched* = 0)
            (deffunction effect () (assert (value (bind ?*touched* 1))))",
            )
            .unwrap();
        with_compact_context(&mut engine, None, |ctx| {
            ctx.allow_engine_effects = false;
            assert!(matches!(
                eval_source(ctx, "(effect)"),
                Err(EvalError::UnsupportedOperation { .. })
            ));
            assert!(matches!(
                eval_source(ctx, "?*touched*").unwrap(),
                Value::Integer(0)
            ));
            assert_eq!(ctx.engine.fact_count(), 0);
        });
    }

    #[test]
    fn expression_queries_return_body_values_and_retain_retracted_members() {
        let (mut engine, _) = compact_test_engine();
        add_query_item(&mut engine, 20);
        with_compact_context(&mut engine, None, |ctx| {
            assert!(matches!(
                eval_source(
                    ctx,
                    "(do-for-all-facts ((?x item)) TRUE (retract ?x) ?x:value)"
                )
                .unwrap(),
                Value::Integer(20)
            ));
            let empty = eval_source(ctx, "(do-for-fact ((?x item)) TRUE 99)").unwrap();
            assert!(!is_truthy(&empty, &ctx.engine.symbol_table));
            eval_source(ctx, "(assert (item (value 30)))").unwrap();
            assert!(matches!(
                eval_source(ctx, "(do-for-all-facts ((?x item)) TRUE (break) 99)").unwrap(),
                Value::Void
            ));
        });
    }

    #[test]
    fn delayed_expression_queries_retain_old_records_across_reset() {
        let (mut engine, _) = compact_test_engine();
        add_query_item(&mut engine, 20);
        with_compact_context(&mut engine, None, |ctx| {
            assert!(matches!(
                eval_source(
                    ctx,
                    "(delayed-do-for-all-facts ((?x item)) TRUE (reset) ?x:value)"
                )
                .unwrap(),
                Value::Integer(20)
            ));
            assert!(matches!(
                eval_source(ctx, "(do-for-all-facts ((?x item)) TRUE (reset) ?x:value)").unwrap(),
                Value::Integer(10)
            ));
        });
    }

    #[test]
    fn procedural_defaults_are_false_values_in_callable_expressions() {
        let mut engine = crate::Engine::new(EngineConfig::default());
        engine
            .load_str(
                "(deffunction nop ()) (defmethod nopm ((?x INTEGER)))
             (deffunction conditional (?x) (if (> ?x 0) then yes))
             (deffunction count () (loop-for-count 2 do 77))
             (deffunction spin () (bind ?n 0) (while (< ?n 2) do (bind ?n (+ ?n 1))))",
            )
            .unwrap();
        with_compact_context(&mut engine, None, |ctx| {
            for source in [
                "(nop)",
                "(nopm 1)",
                "(conditional -1)",
                "(count)",
                "(spin)",
                "(switch 1 (case 2 then yes))",
                "(foreach ?x (create$) ?x)",
            ] {
                let value = eval_source(ctx, source).unwrap();
                assert!(
                    matches!(value, Value::Symbol(symbol) if ctx.engine.symbol_table.resolve_symbol_str(symbol) == Some("FALSE")),
                    "{source}: {value:?}"
                );
            }
            let fields = eval_source(ctx, "(create$ a (conditional -1) b)").unwrap();
            assert!(matches!(fields, Value::Multifield(values) if values.len() == 3));
        });
    }

    #[test]
    fn evaluator_breaks_stop_the_nearest_loop_and_restore_local_scope() {
        let mut engine = crate::Engine::new(EngineConfig::default());
        engine
            .load_str(
                "(deffunction nested () (bind ?n 0)
                (loop-for-count (?i 1 3) do
                    (loop-for-count (?j 1 4) do (bind ?n (+ ?n 1)) (break))
                    (bind ?n (+ ?n 10))) ?n)
             (deffunction early () (loop-for-count 5 do (return 7)) 99)",
            )
            .unwrap();
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            assert!(matches!(
                eval_source(ctx, "(nested)").unwrap(),
                Value::Integer(33)
            ));
            assert!(matches!(
                eval_source(ctx, "(early)").unwrap(),
                Value::Integer(7)
            ));
            eval(ctx, &local_bind("x", vec![int(99)])).unwrap();
            for source in ["(while TRUE do (break))", "(loop-for-count 5 do (break))"] {
                let value = eval_source(ctx, source).unwrap();
                assert!(!is_truthy(&value, &ctx.engine.symbol_table), "{source}");
            }
            for source in [
                "(progn$ (?x (create$ a b c)) (if (eq ?x b) then (break)) ?x)",
                "(foreach ?x (create$ a b c) (if (eq ?x b) then (break)) ?x)",
            ] {
                assert!(
                    matches!(eval_source(ctx, source).unwrap(), Value::Void),
                    "{source}"
                );
                assert!(matches!(
                    eval(ctx, &local_read("x")).unwrap(),
                    Value::Integer(99)
                ));
            }
            let value = eval_source(ctx, "(foreach ?x (create$ a b c) ?x)").unwrap();
            assert!(
                matches!(value, Value::Symbol(symbol) if ctx.engine.symbol_table.resolve_symbol_str(symbol) == Some("c"))
            );
        });
    }

    #[test]
    fn escaped_breaks_cannot_cross_callable_or_public_evaluator_boundaries() {
        let mut engine = crate::Engine::new(EngineConfig::default());
        engine.load_str("(deffunction broken () 0)").unwrap();
        // Bypass source validation to exercise the runtime boundary directly.
        engine.functions.register(
            engine.module_registry.main_module_id(),
            UserFunction {
                name: "broken".into(),
                parameters: Vec::new(),
                wildcard_parameter: None,
                body: vec![source_action("(break)")],
            },
        );
        with_compact_context(&mut engine, None, |ctx| {
            assert!(matches!(
                eval_source(ctx, "(break)"),
                Err(EvalError::BreakOutsideLoop { .. })
            ));
            assert!(matches!(
                eval_action_expression(ctx, &call("break", vec![])),
                Err(EvalError::BreakControl { .. })
            ));
            assert!(matches!(
                eval_source(ctx, "(while TRUE do (broken))"),
                Err(EvalError::BreakOutsideLoop { .. })
            ));
            assert!(matches!(
                eval_source(ctx, "(break 1)"),
                Err(EvalError::ArityMismatch { .. })
            ));
        });
    }

    #[test]
    fn wildcard_bindings_flatten_only_excess_arguments_after_type_selection() {
        let mut engine = crate::Engine::new(EngineConfig::default());
        engine
            .load_str(
                "(deffunction count ($?values) (length$ ?values))
             (deffunction sizes (?first $?rest) (create$ (length$ ?first) (length$ ?rest)))
             (defmethod symbols (($?values SYMBOL)) (length$ ?values))
             (defmethod fields (($?values MULTIFIELD)) (length$ ?values))
             (defmethod queried (($?values (= (length$ ?values) 3))) (length$ ?values))",
            )
            .unwrap();
        with_compact_context(&mut engine, None, |ctx| {
            for (source, expected) in [
                ("(count (create$ a b c))", 3),
                ("(count x (create$ a b) y)", 4),
                ("(count (create$))", 0),
                ("(symbols a b)", 2),
                ("(symbols)", 0),
                ("(fields (create$ a b) (create$ c))", 3),
                ("(queried a (create$ b c))", 3),
            ] {
                assert!(
                    matches!(eval_source(ctx, source).unwrap(), Value::Integer(value) if value == expected),
                    "{source}"
                );
            }
            assert!(matches!(
                eval_source(ctx, "(symbols (create$ a b))"),
                Err(EvalError::NoApplicableMethod { .. })
            ));
            let sizes = eval_source(ctx, "(sizes (create$ 1 2) 3 (create$ 4 5))").unwrap();
            assert!(sizes.structural_eq(&Value::Multifield(Box::new(
                [Value::Integer(2), Value::Integer(3)].into_iter().collect()
            ))));
        });
    }

    #[test]
    fn method_queries_are_lazy_and_call_next_rechecks_reached_candidates() {
        let mut engine = crate::Engine::new(EngineConfig::default());
        engine.load_str(
            "(defglobal ?*queries* = 0)
             (deffunction probe (?answer) (bind ?*queries* (+ ?*queries* 1)) ?answer)
             (defmethod choose 10 ((?x INTEGER (probe TRUE))) first)
             (defmethod choose 20 ((?x NUMBER (probe TRUE))) second)
             (defmethod chain 10 ((?x INTEGER (probe TRUE))) (create$ (call-next-method) (call-next-method)))
             (defmethod chain 20 ((?x INTEGER (probe FALSE))) skipped)
             (defmethod chain 30 ((?x NUMBER (probe TRUE))) third)",
        ).unwrap();
        with_compact_context(&mut engine, None, |ctx| {
            let value = eval_source(ctx, "(choose 1)").unwrap();
            assert!(
                matches!(value, Value::Symbol(symbol) if ctx.engine.symbol_table.resolve_symbol_str(symbol) == Some("first"))
            );
            assert!(matches!(
                ctx.engine.globals.get(ctx.current_module, "queries"),
                Some(Value::Integer(1))
            ));
            ctx.engine
                .globals
                .set(ctx.current_module, "queries", Value::Integer(0));
            let value = eval_source(ctx, "(chain 1)").unwrap();
            let Value::Multifield(fields) = value else {
                panic!("expected two next-method results")
            };
            assert_eq!(fields.len(), 2);
            assert!(fields.iter().all(|value| matches!(value, Value::Symbol(symbol) if ctx.engine.symbol_table.resolve_symbol_str(*symbol) == Some("third"))));
            assert!(matches!(
                ctx.engine.globals.get(ctx.current_module, "queries"),
                Some(Value::Integer(5))
            ));
        });
    }

    #[test]
    fn method_queries_see_later_parameters_and_errors_abort_selection() {
        let mut engine = crate::Engine::new(EngineConfig::default());
        engine
            .load_str(
                "(defmethod later ((?x INTEGER (< ?x ?later)) (?later INTEGER)) yes)
             (defmethod failure 1 ((?x INTEGER (/ 1 0))) unreachable)
             (defmethod failure 2 ((?x NUMBER)) fallback)
             (defmethod priority 1 ((?x (eq ?x 1))) queried)
             (defmethod priority 2 ((?x INTEGER)) typed)
             (defmethod shape 1 (($?values INTEGER)) wildcard)
             (defmethod shape 2 (?x) fixed)",
            )
            .unwrap();
        with_compact_context(&mut engine, None, |ctx| {
            assert!(eval_source(ctx, "(later 1 2)").is_ok());
            assert!(matches!(
                eval_source(ctx, "(later 2 1)"),
                Err(EvalError::NoApplicableMethod { .. })
            ));
            assert!(matches!(
                eval_source(ctx, "(failure 1)"),
                Err(EvalError::DivisionByZero { .. })
            ));
            for (source, expected) in [("(priority 1)", "typed"), ("(shape 1)", "fixed")] {
                let value = eval_source(ctx, source).unwrap();
                assert!(
                    matches!(value, Value::Symbol(symbol) if ctx.engine.symbol_table.resolve_symbol_str(symbol) == Some(expected)),
                    "{source}"
                );
            }
        });
    }

    fn local_bind(name: &str, values: Vec<RuntimeExpr>) -> RuntimeExpr {
        call(
            "bind",
            std::iter::once(local_read(name)).chain(values).collect(),
        )
    }

    #[test]
    fn callable_local_unbind_restores_parameters_and_normalizes_multifield_names() {
        let (mut engine, _) = compact_test_engine();
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            assert!(eval(ctx, &local_read("f"))
                .unwrap()
                .structural_eq(&Value::Integer(42)));
            assert!(eval(ctx, &local_bind("$?f", vec![int(99)]))
                .unwrap()
                .structural_eq(&Value::Integer(99)));
            assert!(eval(ctx, &local_read("f"))
                .unwrap()
                .structural_eq(&Value::Integer(99)));
            let unbound = eval(ctx, &local_bind("f", vec![])).unwrap();
            assert!(!is_truthy(&unbound, &ctx.engine.symbol_table));
            assert!(eval(ctx, &local_read("$?f"))
                .unwrap()
                .structural_eq(&Value::Integer(42)));
            eval(ctx, &local_bind("temporary", vec![int(7)])).unwrap();
            eval(ctx, &local_bind("$?temporary", vec![])).unwrap();
            assert!(matches!(
                eval(ctx, &local_read("temporary")),
                Err(EvalError::UnboundVariable { .. })
            ));
        });
    }

    #[test]
    fn callable_local_many_values_splice_multifields_and_omit_void() {
        let (mut engine, _) = compact_test_engine();
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            let fields = call("create$", vec![int(2), int(3)]);
            let result = eval(
                ctx,
                &local_bind(
                    "values",
                    vec![int(1), fields, RuntimeExpr::Literal(Value::Void), int(4)],
                ),
            )
            .unwrap();
            let expected: ferric_rules_core::Multifield = (1..=4).map(Value::Integer).collect();
            assert!(result.structural_eq(&Value::Multifield(Box::new(expected))));
            assert!(eval(ctx, &local_read("$?values"))
                .unwrap()
                .structural_eq(&result));
            assert!(matches!(
                eval(
                    ctx,
                    &local_bind("f", vec![RuntimeExpr::Literal(Value::Void)])
                )
                .unwrap(),
                Value::Void
            ));
            assert!(matches!(eval(ctx, &local_read("f")).unwrap(), Value::Void));
            let empty = eval(
                ctx,
                &local_bind(
                    "values",
                    vec![
                        RuntimeExpr::Literal(Value::Void),
                        RuntimeExpr::Literal(Value::Void),
                    ],
                ),
            )
            .unwrap();
            assert!(matches!(empty, Value::Multifield(ref values) if values.is_empty()));
        });
    }

    #[test]
    fn callable_local_rhs_error_keeps_prior_value_and_completed_nested_effects() {
        let (mut engine, _) = compact_test_engine();
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            eval(ctx, &local_bind("f", vec![int(99)])).unwrap();
            let expr = local_bind(
                "f",
                vec![
                    local_bind("trace", vec![int(1)]),
                    call("/", vec![int(1), int(0)]),
                    local_bind("trace", vec![int(2)]),
                ],
            );
            assert!(eval(ctx, &expr).is_err());
            assert!(eval(ctx, &local_read("f"))
                .unwrap()
                .structural_eq(&Value::Integer(99)));
            assert!(eval(ctx, &local_read("trace"))
                .unwrap()
                .structural_eq(&Value::Integer(1)));
            let nested = local_bind(
                "f",
                vec![
                    local_bind("f", vec![int(2)]),
                    call("/", vec![int(1), int(0)]),
                ],
            );
            assert!(eval(ctx, &nested).is_err());
            assert!(eval(ctx, &local_read("f"))
                .unwrap()
                .structural_eq(&Value::Integer(2)));
        });
    }

    #[test]
    fn callable_local_scope_restores_nested_metadata_on_errors_and_return() {
        let (mut engine, _) = compact_test_engine();
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            eval(ctx, &local_bind("f", vec![int(99)])).unwrap();
            for returning in [false, true] {
                let result: Result<(), EvalError> =
                    with_callable_local_scope(ctx, ["f", "f"], Some("f"), |ctx| {
                        assert!(eval(ctx, &local_read("f"))
                            .unwrap()
                            .structural_eq(&Value::Integer(42)));
                        // An illegal iterator bind fails before evaluating its RHS.
                        assert!(eval(
                            ctx,
                            &local_bind("$?f", vec![local_bind("forbidden-effect", vec![int(1)])])
                        )
                        .is_err());
                        let nested: Result<(), EvalError> =
                            with_callable_local_scope(ctx, ["f"], Some("f"), |_| {
                                Err(EvalError::UnboundVariable {
                                    name: "missing".into(),
                                    span: None,
                                })
                            });
                        assert!(nested.is_err());
                        assert!(ctx
                            .callable_locals
                            .as_deref()
                            .unwrap()
                            .protected_names
                            .contains("f"));
                        eval(ctx, &local_bind("progress", vec![int(7)])).unwrap();
                        if returning {
                            Err(EvalError::ReturnControl {
                                value: Value::Integer(8),
                                span: None,
                            })
                        } else {
                            Err(EvalError::UnboundVariable {
                                name: "missing".into(),
                                span: None,
                            })
                        }
                    });
                assert!(result.is_err());
                assert!(eval(ctx, &local_read("f"))
                    .unwrap()
                    .structural_eq(&Value::Integer(99)));
                assert!(eval(ctx, &local_read("progress"))
                    .unwrap()
                    .structural_eq(&Value::Integer(7)));
                assert!(eval(ctx, &local_read("forbidden-effect")).is_err());
                let locals = ctx.callable_locals.as_deref().unwrap();
                assert!(locals.lexical_names.is_empty());
                assert!(locals.protected_names.is_empty());
            }
        });
    }

    #[test]
    fn callable_local_query_scope_masks_members_and_restores_on_early_exits() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let slot = compact_ref(&mut engine, "f", "value");
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            eval(ctx, &local_bind("f", vec![int(99)])).unwrap();
            eval(ctx, &local_bind("f:value", vec![int(91)])).unwrap();
            let same_fact = call(
                "eq",
                vec![
                    local_read("f"),
                    RuntimeExpr::Literal(Value::FactAddress(address.clone())),
                ],
            );
            let nested = expression_query("any-factp", &[("f", "item")], same_fact.clone());
            let predicate = call(
                "and",
                vec![nested, same_fact, call("=", vec![slot.clone(), int(10)])],
            );
            let query = expression_query("any-factp", &[("f", "item")], predicate);
            let found = eval(ctx, &query).unwrap();
            assert!(is_truthy(&found, &ctx.engine.symbol_table));
            for predicate in [
                call("/", vec![int(1), int(0)]),
                call("return", vec![int(8)]),
            ] {
                let query = expression_query("any-factp", &[("f", "item")], predicate);
                assert!(eval(ctx, &query).is_err());
                assert!(ctx
                    .callable_locals
                    .as_deref()
                    .unwrap()
                    .lexical_names
                    .is_empty());
            }
            assert!(eval(ctx, &local_read("f"))
                .unwrap()
                .structural_eq(&Value::Integer(99)));
            assert!(eval(ctx, &slot).unwrap().structural_eq(&Value::Integer(91)));
        });
    }

    #[test]
    fn callable_local_frames_are_fresh_after_return_or_failure() {
        let (mut engine, _) = compact_test_engine();
        engine
            .load_str(
                "(deffunction isolated (?f) (bind ?temporary 7) (bind ?f 8) (return ?temporary))
                         (deffunction failed () (bind ?temporary 9) (/ 1 0))
                         (deffunction absent () ?temporary)",
            )
            .unwrap();
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, None, Some(&mut locals), |ctx| {
            eval(ctx, &local_bind("f", vec![int(99)])).unwrap();
            eval(ctx, &local_bind("temporary", vec![int(100)])).unwrap();
            for _ in 0..2 {
                assert!(eval(ctx, &call("isolated", vec![int(42)]))
                    .unwrap()
                    .structural_eq(&Value::Integer(7)));
                assert!(eval(ctx, &call("failed", vec![])).is_err());
                assert!(matches!(
                    eval(ctx, &call("absent", vec![])),
                    Err(EvalError::UnboundVariable { .. })
                ));
                assert!(eval(ctx, &local_read("f"))
                    .unwrap()
                    .structural_eq(&Value::Integer(99)));
                assert!(eval(ctx, &local_read("temporary"))
                    .unwrap()
                    .structural_eq(&Value::Integer(100)));
            }
        });
    }

    fn expression_query(
        name: &str,
        members: &[(&str, &str)],
        predicate: RuntimeExpr,
    ) -> RuntimeExpr {
        RuntimeExpr::QueryAction {
            name: name.into(),
            bindings: members
                .iter()
                .map(|(member, template)| RuntimeQueryBinding {
                    variable: (*member).into(),
                    restrictions: vec![RuntimeExpr::Call {
                        name: "sym-cat".into(),
                        args: vec![RuntimeExpr::Literal(Value::String(
                            FerricString::new(template, ferric_rules_core::StringEncoding::Utf8)
                                .unwrap(),
                        ))],
                        span: None,
                    }],
                    span: None,
                })
                .collect(),
            query: Box::new(predicate),
            body: Vec::new(),
            span: Some(SourceSpan { line: 4, column: 7 }),
        }
    }

    fn add_query_item(engine: &mut crate::Engine, value: i64) -> FactId {
        engine.fact_base.assert_template(
            engine.template_ids["item"],
            vec![Value::Integer(value), Value::Multifield(Box::default())].into_boxed_slice(),
        )
    }

    #[test]
    fn expression_query_orders_reused_storage_and_flattens_tuples() {
        let (mut engine, old) = compact_test_engine();
        let first = add_query_item(&mut engine, 20);
        engine.fact_base.retract(old).unwrap();
        let second = add_query_item(&mut engine, 30);
        let expr = expression_query("find-all-facts", &[("a", "item"), ("b", "item")], int(1));
        with_compact_context(&mut engine, None, |ctx| {
            let result = eval(ctx, &expr).unwrap();
            let expected: ferric_rules_core::Multifield =
                [first, first, first, second, second, first, second, second]
                    .into_iter()
                    .map(|fact| {
                        Value::FactAddress(
                            make_fact_address(
                                &ctx.engine.fact_base,
                                ctx.engine.initial_fact_id,
                                ctx.engine.fact_epoch,
                                ctx.engine.fact_index_starts_at_zero,
                                fact,
                            )
                            .unwrap(),
                        )
                    })
                    .collect();
            assert!(result.structural_eq(&Value::Multifield(Box::new(expected))));
        });
    }

    #[test]
    fn expression_query_stops_before_enumerating_large_cartesian_product() {
        let (mut engine, _) = compact_test_engine();
        for value in 11..50 {
            add_query_item(&mut engine, value);
        }
        engine.config.max_action_loop_iterations = 5;
        let members = [
            ("a", "item"),
            ("b", "item"),
            ("c", "item"),
            ("d", "item"),
            ("e", "item"),
        ];
        for name in ["any-factp", "find-fact"] {
            let expr = expression_query(name, &members, int(1));
            with_compact_context(&mut engine, None, |ctx| {
                let result = eval(ctx, &expr).unwrap();
                if name == "any-factp" {
                    assert!(is_truthy(&result, &ctx.engine.symbol_table));
                } else {
                    assert!(matches!(result, Value::Multifield(fields) if fields.len() == 5));
                }
            });
        }
        for name in ["any-factp", "find-fact", "find-all-facts"] {
            let predicate = call("=", vec![int(1), int(2)]);
            let expr = expression_query(name, &members, predicate);
            with_compact_context(&mut engine, None, |ctx| {
                assert!(matches!(
                    eval(ctx, &expr),
                    Err(EvalError::ActionIterationLimit { limit: 5, .. })
                ));
            });
        }
    }

    #[test]
    fn expression_query_inherits_nested_budget_and_expression_depth() {
        let (mut engine, _) = compact_test_engine();
        engine.config.max_action_loop_iterations = 1;
        let inner = expression_query("any-factp", &[("b", "item")], int(1));
        let outer = expression_query("any-factp", &[("a", "item")], inner);
        with_compact_context(&mut engine, None, |ctx| {
            assert!(matches!(
                eval(ctx, &outer),
                Err(EvalError::ActionIterationLimit { limit: 1, .. })
            ));
            assert_eq!(ctx.expression_depth, 0);
        });
        engine.config.max_action_loop_iterations = 10;
        with_compact_context(&mut engine, None, |ctx| {
            ctx.expression_depth = MAX_EXPRESSION_DEPTH - 1;
            assert!(matches!(
                eval(ctx, &outer),
                Err(EvalError::ExpressionNestingLimit { .. })
            ));
            assert_eq!(ctx.expression_depth, MAX_EXPRESSION_DEPTH - 1);
        });
    }

    #[test]
    fn expression_query_empty_candidates_are_lazy_but_declarations_are_checked() {
        let (mut engine, fact) = compact_test_engine();
        engine.fact_base.retract(fact).unwrap();
        engine.config.max_action_loop_iterations = 0;
        for name in ["any-factp", "find-fact", "find-all-facts"] {
            let expr = expression_query(name, &[("f", "item")], call("/", vec![int(1), int(0)]));
            with_compact_context(&mut engine, None, |ctx| {
                let result = eval(ctx, &expr).unwrap();
                if name == "any-factp" {
                    assert!(!is_truthy(&result, &ctx.engine.symbol_table));
                } else {
                    assert!(matches!(result, Value::Multifield(fields) if fields.is_empty()));
                }
            });
        }
        let expr = expression_query("any-factp", &[("a", "item"), ("b", "missing")], int(1));
        with_compact_context(&mut engine, None, |ctx| {
            assert!(
                matches!(eval(ctx, &expr), Err(EvalError::TypeError { actual, .. }) if actual.contains("unknown template"))
            );
        });
        let unknown = expression_query(
            "any-factp",
            &[("f", "item")],
            call("absent-predicate", vec![]),
        );
        with_compact_context(&mut engine, None, |ctx| {
            assert!(
                matches!(eval(ctx, &unknown), Err(EvalError::TypeError { actual, .. }) if actual.contains("absent-predicate"))
            );
        });
    }

    #[test]
    fn expression_query_shadows_and_restores_both_scopes_on_success_and_error() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        add_query_item(&mut engine, 20);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        let slot = compact_ref(&mut engine, "f", "value");
        let inner = expression_query(
            "any-factp",
            &[("f", "item")],
            call("=", vec![slot.clone(), int(20)]),
        );
        let outer = expression_query(
            "find-fact",
            &[("f", "item")],
            call("and", vec![inner, call("=", vec![slot.clone(), int(10)])]),
        );
        let error = expression_query(
            "any-factp",
            &[("f", "item")],
            compact_ref(&mut engine, "f", "missing"),
        );
        let returned =
            expression_query("any-factp", &[("f", "item")], call("return", vec![int(7)]));
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            let result = eval(ctx, &outer).unwrap();
            assert!(
                matches!(result, Value::Multifield(fields) if fields.len() == 1 && fields[0].structural_eq(&Value::FactAddress(address.clone())))
            );
            assert!(eval(ctx, &error).is_err());
            assert!(matches!(
                eval(ctx, &returned),
                Err(EvalError::ReturnOutsideCallable { .. })
            ));
            assert!(eval(ctx, &slot).unwrap().structural_eq(&Value::Integer(10)));
            let ordinary = RuntimeExpr::BoundVar {
                name: "f".into(),
                span: None,
            };
            assert!(eval(ctx, &ordinary)
                .unwrap()
                .structural_eq(&Value::Integer(42)));
            assert_eq!(ctx.compact_fact_bindings.unwrap()["f"].fact_id(), fact);
        });
    }

    #[test]
    fn expression_query_rejects_restored_malformed_headers() {
        let (mut engine, _) = compact_test_engine();
        let mut malformed = vec![
            expression_query("any-factp", &[], int(1)),
            expression_query("any-factp", &[("f", "item"), ("f", "item")], int(1)),
            expression_query("any-factp", &[("f", "MAIN::item")], int(1)),
        ];
        for member in ["", "?", "$?f", "f:slot", "*global*", "f trailing"] {
            malformed.push(expression_query("find-fact", &[(member, "item")], int(1)));
        }
        let mut body = expression_query("find-all-facts", &[("f", "item")], int(1));
        let RuntimeExpr::QueryAction { body: items, .. } = &mut body else {
            unreachable!()
        };
        items.push((
            ferric_rules_parser::ActionExpr::Variable("f".into(), dummy_span()),
            Some(Box::new(int(99))),
        ));
        malformed.push(body);
        for expr in malformed {
            with_compact_context(&mut engine, None, |ctx| {
                assert!(
                    matches!(eval(ctx, &expr), Err(EvalError::TypeError { .. })),
                    "{expr:?}"
                );
            });
        }
        let expr = expression_query("forged-query", &[("f", "item")], int(1));
        with_compact_context(&mut engine, None, |ctx| {
            assert!(matches!(
                eval(ctx, &expr),
                Err(EvalError::UnsupportedOperation { .. })
            ));
        });
    }

    #[test]
    fn expression_query_rejects_local_bind_in_cached_and_fallback_predicate_bodies() {
        use ferric_rules_parser::{ActionExpr, FunctionCall};
        let (mut engine, _) = compact_test_engine();
        let bind = ActionExpr::FunctionCall(FunctionCall {
            name: "bind".into(),
            args: vec![ActionExpr::Variable("temporary".into(), dummy_span())],
            span: dummy_span(),
        });
        for cached in [false, true] {
            let compiled = cached.then(|| {
                Box::new(from_action_expr(&bind, &mut engine.symbol_table, &engine.config).unwrap())
            });
            let predicate = RuntimeExpr::If {
                condition: Box::new(call("=", vec![int(1), int(2)])),
                then_branch: vec![(bind.clone(), compiled)],
                else_branch: Vec::new(),
                span: None,
            };
            let expr = expression_query("any-factp", &[("f", "item")], predicate);
            with_compact_context(&mut engine, None, |ctx| {
                assert!(
                    matches!(eval(ctx, &expr), Err(EvalError::TypeError { actual, .. }) if actual.contains("FACTQPSR2"))
                );
            });
        }
    }

    #[test]
    fn compact_slot_reads_scalar_and_multifield_from_lexical_fact() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        for slot in ["value", "tags"] {
            let compact = compact_ref(&mut engine, "f", slot);
            let RuntimeExpr::Call { args, .. } = &compact else {
                unreachable!();
            };
            let explicit = call(
                "fact-slot-value",
                vec![
                    RuntimeExpr::Literal(Value::FactAddress(address.clone())),
                    args[1].clone(),
                ],
            );
            with_compact_context(&mut engine, Some(&scope), |ctx| {
                let value = eval(ctx, &compact).unwrap();
                assert!(value.structural_eq(&eval(ctx, &explicit).unwrap()));
                if slot == "value" {
                    assert!(value.structural_eq(&Value::Integer(10)));
                } else {
                    assert!(matches!(value, Value::Multifield(fields) if fields.len() == 2));
                }
            });
        }
    }

    #[test]
    fn compact_slot_requires_template_while_explicit_accessor_reads_ordered_implied() {
        let (mut engine, _) = compact_test_engine();
        let relation = engine
            .symbol_table
            .intern_symbol("row", engine.config.string_encoding)
            .unwrap();
        let fact = engine
            .fact_base
            .assert_ordered(relation, smallvec::smallvec![Value::Integer(7)]);
        let address = test_fact_address(&engine, fact);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        let compact = compact_ref(&mut engine, "f", "implied");
        let RuntimeExpr::Call { args, .. } = &compact else {
            unreachable!();
        };
        let explicit = call(
            "fact-slot-value",
            vec![
                RuntimeExpr::Literal(Value::FactAddress(address.clone())),
                args[1].clone(),
            ],
        );
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            assert!(matches!(
                eval(ctx, &compact),
                Err(EvalError::TypeError { expected, .. }) if expected == "template fact"
            ));
            let value = eval(ctx, &explicit).unwrap();
            assert!(
                matches!(value, Value::Multifield(fields) if fields.len() == 1 && fields[0].structural_eq(&Value::Integer(7)))
            );
        });
    }

    #[test]
    fn compact_slot_rejects_malformed_arguments_without_evaluating_them() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        let compact = compact_ref(&mut engine, "f", "value");
        let RuntimeExpr::Call { args, .. } = compact else {
            unreachable!();
        };
        let division = call("/", vec![int(1), int(0)]);
        let malformed = [
            vec![division.clone(), args[1].clone()],
            vec![args[0].clone(), division],
            vec![args[0].clone(), int(10)],
            vec![
                RuntimeExpr::BoundVar {
                    name: String::new(),
                    span: None,
                },
                args[1].clone(),
            ],
            vec![
                RuntimeExpr::BoundVar {
                    name: "$?f".into(),
                    span: None,
                },
                args[1].clone(),
            ],
        ];
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            assert!(matches!(
                eval(ctx, &call(COMPACT_FACT_SLOT_REF, vec![])),
                Err(EvalError::ArityMismatch { actual: 0, .. })
            ));
            for args in malformed {
                assert!(matches!(
                    eval(ctx, &call(COMPACT_FACT_SLOT_REF, args)),
                    Err(EvalError::TypeError { function, .. }) if function == COMPACT_FACT_SLOT_REF
                ));
            }
        });
    }

    #[test]
    fn compact_slot_reports_absent_scope_stale_fact_and_missing_slot() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        let compact = compact_ref(&mut engine, "f", "value");
        with_compact_context(&mut engine, None, |ctx| {
            assert!(
                matches!(eval(ctx, &compact), Err(EvalError::UnboundVariable {
                name, span: Some(SourceSpan { line: 4, column: 7 })
            }) if name == "f")
            );
        });
        let absent = compact_ref(&mut engine, "missing", "value");
        let missing_slot = compact_ref(&mut engine, "f", "missing");
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            assert!(matches!(
                eval(ctx, &absent),
                Err(EvalError::UnboundVariable { .. })
            ));
            let error = eval(ctx, &missing_slot).unwrap_err();
            assert!(
                matches!(error, EvalError::TypeError { ref actual, .. } if actual.contains("unknown slot `missing`"))
            );
        });
        engine.fact_base.retract(fact).unwrap();
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            let error = eval(ctx, &compact).unwrap_err();
            assert!(
                matches!(error, EvalError::TypeError { ref actual, .. } if actual.contains("no longer exists"))
            );
        });
    }

    #[test]
    fn retained_compact_record_keeps_old_identity_while_explicit_reads_stay_live() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let record = Arc::new(engine.fact_base.get(fact).unwrap().fact.clone());
        let member = CompactFactBinding::retained(address.clone(), Arc::clone(&record));
        let shared = member.clone();
        assert!(Arc::ptr_eq(
            member.retained.as_ref().unwrap(),
            shared.retained.as_ref().unwrap()
        ));
        assert_eq!(shared.fact_id(), fact);
        engine.fact_base.retract(fact).unwrap();
        let replacement = add_query_item(&mut engine, 20);
        assert_ne!(replacement, fact);
        let scope = CompactFactBindings::from([("f".into(), member)]);
        let compact = compact_ref(&mut engine, "f", "value");
        let tags = compact_ref(&mut engine, "f", "tags");
        let RuntimeExpr::Call { args, .. } = &compact else {
            unreachable!()
        };
        let explicit = call(
            "fact-slot-value",
            vec![
                RuntimeExpr::Literal(Value::FactAddress(address.clone())),
                args[1].clone(),
            ],
        );
        let replacement_slot = call(
            "fact-slot-value",
            vec![
                RuntimeExpr::Literal(address_value(&engine, replacement)),
                args[1].clone(),
            ],
        );
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            assert!(eval(ctx, &compact)
                .unwrap()
                .structural_eq(&Value::Integer(10)));
            assert!(
                matches!(eval(ctx, &tags).unwrap(), Value::Multifield(fields) if fields.len() == 2)
            );
            let exists = eval(
                ctx,
                &call(
                    "fact-existp",
                    vec![RuntimeExpr::Literal(Value::FactAddress(address.clone()))],
                ),
            )
            .unwrap();
            assert!(!is_truthy(&exists, &ctx.engine.symbol_table));
            assert!(eval(
                ctx,
                &call(
                    "fact-index",
                    vec![RuntimeExpr::Literal(Value::FactAddress(address.clone()))]
                )
            )
            .unwrap()
            .structural_eq(&Value::Integer(-1)));
            // A retained compact record does not revive its ordinary address.
            assert!(!is_truthy(
                &eval(ctx, &explicit).unwrap(),
                &ctx.engine.symbol_table
            ));
            assert!(eval(ctx, &replacement_slot)
                .unwrap()
                .structural_eq(&Value::Integer(20)));
        });
    }

    #[test]
    fn retained_compact_scope_survives_nested_result_queries_and_their_errors() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let record = Arc::new(engine.fact_base.get(fact).unwrap().fact.clone());
        let scope = CompactFactBindings::from([(
            "f".into(),
            CompactFactBinding::retained(address.clone(), Arc::clone(&record)),
        )]);
        engine.fact_base.retract(fact).unwrap();
        add_query_item(&mut engine, 20);
        let compact = compact_ref(&mut engine, "f", "value");
        let inherited = expression_query(
            "any-factp",
            &[("g", "item")],
            call("=", vec![compact.clone(), int(10)]),
        );
        let shadowed = expression_query(
            "any-factp",
            &[("f", "item")],
            call("=", vec![compact.clone(), int(20)]),
        );
        let error = expression_query(
            "any-factp",
            &[("f", "item")],
            compact_ref(&mut engine, "f", "missing"),
        );
        let returned =
            expression_query("any-factp", &[("f", "item")], call("return", vec![int(7)]));
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            for expr in [inherited, shadowed] {
                let result = eval(ctx, &expr).unwrap();
                assert!(is_truthy(&result, &ctx.engine.symbol_table));
            }
            assert!(matches!(
                eval(ctx, &error),
                Err(EvalError::TypeError { .. })
            ));
            assert!(matches!(
                eval(ctx, &returned),
                Err(EvalError::ReturnOutsideCallable { .. })
            ));
            assert!(eval(ctx, &compact)
                .unwrap()
                .structural_eq(&Value::Integer(10)));
            assert_eq!(ctx.compact_fact_bindings.unwrap()["f"].fact_id(), fact);
        });
        assert_eq!(
            Arc::strong_count(&record),
            2,
            "temporary query scopes must release retained records"
        );
    }

    #[test]
    fn retained_compact_record_checks_template_metadata_and_reads_ordered_implied() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let record = Arc::new(engine.fact_base.get(fact).unwrap().fact.clone());
        let scope = CompactFactBindings::from([(
            "f".into(),
            CompactFactBinding::retained(address.clone(), record),
        )]);
        engine.fact_base.retract(fact).unwrap();
        let compact = compact_ref(&mut engine, "f", "value");
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            // The retained record itself supplies fields; the template registry
            // still supplies and checks the slot layout.
            ctx.engine.fact_base = FactBase::new();
            assert!(eval(ctx, &compact)
                .unwrap()
                .structural_eq(&Value::Integer(10)));
            ctx.engine.template_defs.clear();
            assert!(matches!(
                eval(ctx, &compact),
                Err(EvalError::TypeError { .. })
            ));
        });
        let relation = engine
            .symbol_table
            .intern_symbol("row", engine.config.string_encoding)
            .unwrap();
        let ordered = engine
            .fact_base
            .assert_ordered(relation, smallvec::smallvec![Value::Integer(3)]);
        let ordered_address = test_fact_address(&engine, ordered);
        let record = Arc::new(engine.fact_base.retract(ordered).unwrap().fact);
        let scope = CompactFactBindings::from([(
            "f".into(),
            CompactFactBinding::retained(ordered_address, record),
        )]);
        let implied = compact_ref(&mut engine, "f", "implied");
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            let Value::Multifield(fields) = eval(ctx, &implied).unwrap() else {
                panic!("ordered implied slot must be a multifield");
            };
            assert_eq!(fields.len(), 1);
            assert!(fields[0].structural_eq(&Value::Integer(3)));
        });
    }

    #[test]
    fn integer_designators_never_alias_slot_map_address_bits() {
        use slotmap::Key;
        let (engine, fact) = compact_test_engine();
        let raw = i64::from_ne_bytes(fact.data().as_ffi().to_ne_bytes());
        assert_eq!(
            designated_fact(
                &engine.fact_base,
                engine.initial_fact_id,
                engine.fact_index_starts_at_zero,
                engine.fact_epoch,
                &Value::Integer(raw)
            ),
            None
        );
        assert_eq!(
            designated_fact(
                &engine.fact_base,
                engine.initial_fact_id,
                engine.fact_index_starts_at_zero,
                engine.fact_epoch,
                &address_value(&engine, fact)
            ),
            Some(fact)
        );
    }

    #[test]
    fn compact_slot_requires_context_while_explicit_accessor_keeps_false_fallback() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        let compact = compact_ref(&mut engine, "f", "value");
        let RuntimeExpr::Call { args, .. } = &compact else {
            unreachable!();
        };
        let explicit = call(
            "fact-slot-value",
            vec![
                RuntimeExpr::Literal(Value::FactAddress(address.clone())),
                args[1].clone(),
            ],
        );
        for missing_fact_base in [true, false] {
            with_compact_context(&mut engine, Some(&scope), |ctx| {
                if missing_fact_base {
                    ctx.engine.fact_base = FactBase::new();
                } else {
                    ctx.engine.template_defs.clear();
                }
                assert!(matches!(
                    eval(ctx, &compact),
                    Err(EvalError::TypeError { .. })
                ));
                let fallback = eval(ctx, &explicit).unwrap();
                assert!(!is_truthy(&fallback, &ctx.engine.symbol_table));
            });
        }
    }

    #[test]
    fn compact_slot_access_stays_lazy_under_boolean_short_circuiting() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        let missing = compact_ref(&mut engine, "f", "missing");
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            let truth = RuntimeExpr::Literal(clips_true(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ));
            let falsehood = RuntimeExpr::Literal(clips_false(
                &mut ctx.engine.symbol_table,
                ctx.engine.config.string_encoding,
            ));
            let skipped = call(">", vec![missing.clone(), int(0)]);
            let result = eval(ctx, &call("or", vec![truth, skipped.clone()])).unwrap();
            assert!(is_truthy(&result, &ctx.engine.symbol_table));
            let result = eval(ctx, &call("and", vec![falsehood, skipped])).unwrap();
            assert!(!is_truthy(&result, &ctx.engine.symbol_table));
            assert!(eval(ctx, &missing).is_err());
        });
    }

    #[test]
    fn compact_slot_scope_survives_scalar_loop_and_progn_shadowing() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let scope =
            CompactFactBindings::from([("f".into(), CompactFactBinding::live(address.clone()))]);
        let compact = compact_ref(&mut engine, "f", "value");
        let body = vec![(
            ferric_rules_parser::ActionExpr::Variable("unused".into(), dummy_span()),
            Some(Box::new(local_bind("seen", vec![compact]))),
        )];
        let loops = [
            RuntimeExpr::LoopForCount {
                var_name: Some("f".into()),
                start: Box::new(int(2)),
                end: Box::new(int(2)),
                body: body.clone(),
                span: None,
            },
            RuntimeExpr::Progn {
                var_name: "f".into(),
                list_expr: Box::new(call("create$", vec![int(2)])),
                body,
                span: None,
            },
        ];
        let mut locals = CallableLocals::default();
        with_compact_local_context(&mut engine, Some(&scope), Some(&mut locals), |ctx| {
            for expr in loops {
                eval(ctx, &expr).unwrap();
                assert!(eval(ctx, &local_read("seen"))
                    .unwrap()
                    .structural_eq(&Value::Integer(10)));
            }
        });
    }

    #[test]
    fn compact_slot_scope_is_not_inherited_by_callable_or_method_bodies() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let record = Arc::new(engine.fact_base.retract(fact).unwrap().fact);
        let scope = CompactFactBindings::from([(
            "f".into(),
            CompactFactBinding::retained(address.clone(), record),
        )]);
        let body = [ferric_rules_parser::ActionExpr::FunctionCall(
            ferric_rules_parser::FunctionCall {
                name: COMPACT_FACT_SLOT_REF.into(),
                args: vec![
                    ferric_rules_parser::ActionExpr::Variable("f".into(), dummy_span()),
                    ferric_rules_parser::ActionExpr::Literal(ferric_rules_parser::LiteralValue {
                        value: ferric_rules_parser::LiteralKind::Symbol("value".into()),
                        span: dummy_span(),
                    }),
                ],
                span: dummy_span(),
            },
        )];
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            let method = MethodChain {
                generic_name: "test".into(),
                generic_module: ctx.current_module,
                candidate_methods: Vec::new(),
                current_index: 0,
                arg_values: Vec::new(),
            };
            let current_module = ctx.current_module;
            for chain in [None, Some(method)] {
                assert!(matches!(
                    execute_callable_body(ctx, &VarMap::new(), &BindingSet::new(), &body, current_module, chain),
                    Err(EvalError::UnboundVariable { ref name, .. }) if name == "f"
                ));
            }
            assert!(ctx.compact_fact_bindings.is_some());
        });
    }

    #[test]
    fn typed_fact_addresses_reject_numeric_and_string_coercion() {
        let (mut engine, fact) = compact_test_engine();
        let address = address_value(&engine, fact);
        engine
            .load_str(
                "(defmethod address-kind ((?value FACT-ADDRESS)) address)
             (defmethod address-kind ((?value INTEGER)) integer)",
            )
            .unwrap();
        with_compact_context(&mut engine, None, |ctx| {
            for predicate in ["integerp", "numberp"] {
                let value = eval(
                    ctx,
                    &call(predicate, vec![RuntimeExpr::Literal(address.clone())]),
                )
                .unwrap();
                assert!(is_false_symbol(&value, &ctx.engine.symbol_table));
            }
            for (name, args) in [
                ("+", vec![RuntimeExpr::Literal(address.clone()), int(0)]),
                ("integer", vec![RuntimeExpr::Literal(address.clone())]),
                ("str-cat", vec![RuntimeExpr::Literal(address.clone())]),
                ("sym-cat", vec![RuntimeExpr::Literal(address.clone())]),
            ] {
                assert!(matches!(eval(ctx, &call(name, args)),
                    Err(EvalError::TypeError { function, .. }) if function == name));
            }
            let kind = eval(
                ctx,
                &call("address-kind", vec![RuntimeExpr::Literal(address.clone())]),
            )
            .unwrap();
            let Value::Symbol(kind) = kind else {
                panic!("expected kind symbol")
            };
            assert_eq!(
                ctx.engine.symbol_table.resolve_symbol_str(kind),
                Some("address")
            );
            let same = eval(
                ctx,
                &call(
                    "eq",
                    vec![
                        RuntimeExpr::Literal(address.clone()),
                        RuntimeExpr::Literal(address.clone()),
                    ],
                ),
            )
            .unwrap();
            assert!(is_true_symbol(&same, &ctx.engine.symbol_table));
            let different = eval(
                ctx,
                &call("eq", vec![RuntimeExpr::Literal(address.clone()), int(1)]),
            )
            .unwrap();
            assert!(is_false_symbol(&different, &ctx.engine.symbol_table));
            // Host-created nested multifields must not hide a forbidden address.
            let nested = Value::Multifield(Box::new([address.clone()].into_iter().collect()));
            assert!(
                matches!(eval(ctx, &call("str-cat", vec![RuntimeExpr::Literal(nested)])),
                Err(EvalError::TypeError { function, .. }) if function == "str-cat")
            );
        });
    }

    #[test]
    fn fact_addresses_keep_their_spelling_but_do_not_alias_after_reset() {
        let (mut engine, fact) = compact_test_engine();
        let address = test_fact_address(&engine, fact);
        let record = Arc::new(engine.fact_base.get(fact).unwrap().fact.clone());
        let scope = CompactFactBindings::from([(
            "f".into(),
            CompactFactBinding::retained(address.clone(), record),
        )]);
        engine
            .load_str("(deffacts seed (item (value 20) (tags c)))")
            .unwrap();
        engine.reset().unwrap();
        let replacement = engine
            .fact_base
            .facts_by_template(engine.template_ids["item"])
            .next()
            .unwrap();
        assert_eq!(replacement, fact, "reset reuses the slot-map identity");
        assert_eq!(
            address.timestamp(),
            Some(engine.fact_base.get(replacement).unwrap().timestamp)
        );
        assert_eq!(
            live_fact_id(&engine.fact_base, engine.fact_epoch, &address),
            None
        );
        assert_ne!(address, test_fact_address(&engine, replacement));

        let mut printed = String::new();
        let values = Value::Multifield(Box::new(
            [
                Value::FactAddress(address.clone()),
                Value::FactAddress(FactAddress::dummy()),
            ]
            .into_iter()
            .collect(),
        ));
        crate::value_print::append_printout_value(&values, &engine.symbol_table, &mut printed);
        assert_eq!(printed, "(<Fact-1> <Dummy Fact>)");
        let compact = compact_ref(&mut engine, "f", "value");
        let RuntimeExpr::Call { args, .. } = &compact else {
            unreachable!()
        };
        let explicit = call(
            "fact-slot-value",
            vec![
                RuntimeExpr::Literal(Value::FactAddress(address.clone())),
                args[1].clone(),
            ],
        );
        with_compact_context(&mut engine, Some(&scope), |ctx| {
            assert!(eval(ctx, &compact)
                .unwrap()
                .structural_eq(&Value::Integer(10)));
            assert!(is_false_symbol(
                &eval(ctx, &explicit).unwrap(),
                &ctx.engine.symbol_table
            ));
            assert!(eval(
                ctx,
                &call(
                    "fact-index",
                    vec![RuntimeExpr::Literal(Value::FactAddress(address.clone()))]
                )
            )
            .unwrap()
            .structural_eq(&Value::Integer(-1)));
        });
    }

    #[test]
    fn missing_fact_slot_targets_are_false_but_live_invalid_slots_still_error() {
        let (mut engine, fact) = compact_test_engine();
        let address = address_value(&engine, fact);
        let compact = compact_ref(&mut engine, "f", "value");
        let RuntimeExpr::Call { args, .. } = compact else {
            unreachable!()
        };
        let missing = compact_ref(&mut engine, "f", "missing");
        let RuntimeExpr::Call {
            args: missing_args, ..
        } = missing
        else {
            unreachable!()
        };
        with_compact_context(&mut engine, None, |ctx| {
            for target in [
                Value::Integer(-1),
                Value::Integer(99),
                Value::FactAddress(FactAddress::dummy()),
            ] {
                let result = eval(
                    ctx,
                    &call(
                        "fact-slot-value",
                        vec![RuntimeExpr::Literal(target), args[1].clone()],
                    ),
                )
                .unwrap();
                assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
            }
            assert!(
                matches!(eval(ctx, &call("fact-slot-value", vec![RuntimeExpr::Literal(address), missing_args[1].clone()])),
                Err(EvalError::TypeError { actual, .. }) if actual.contains("unknown slot"))
            );
        });
    }

    #[test]
    fn fact_index_keeps_host_facts_stable_when_initial_fact_is_installed_late() {
        let mut engine = crate::Engine::new(EngineConfig::utf8());
        let host_initial = engine
            .assert_ordered("initial-fact", Vec::<Value>::new())
            .unwrap();
        engine.assert_ordered("item", 7_i64).unwrap();
        let mut users: Vec<_> = engine.fact_base.iter().map(|(id, _)| id).collect();
        users.sort_by_key(|id| engine.fact_base.get(*id).unwrap().timestamp);
        for (id, expected) in users.iter().zip([1, 2]) {
            assert_eq!(
                eval_index(address_value(&engine, *id), &mut engine).unwrap(),
                expected
            );
        }

        engine.ensure_initial_fact().unwrap();
        let protected = engine.initial_fact_id.unwrap();
        assert!(!users.contains(&protected));
        assert!(engine.get_fact(host_initial).unwrap().is_some());
        assert_eq!(engine.fact_count(), 2);
        for (id, expected) in users.iter().zip([1, 2]) {
            assert_eq!(
                eval_index(address_value(&engine, *id), &mut engine).unwrap(),
                expected
            );
        }
        assert_eq!(
            eval_index(address_value(&engine, protected), &mut engine).unwrap(),
            0
        );
        engine.ensure_initial_fact().unwrap();
        assert_eq!(engine.fact_base.len(), 3);

        let removed_address = address_value(&engine, users[0]);
        engine.retract(host_initial).unwrap();
        engine.assert_ordered("new-item", 8_i64).unwrap();
        assert_eq!(eval_index(removed_address, &mut engine).unwrap(), -1);
        let (new_id, _) = engine
            .fact_base
            .iter()
            .max_by_key(|(_, fact)| fact.timestamp)
            .unwrap();
        assert_eq!(
            eval_index(address_value(&engine, new_id), &mut engine).unwrap(),
            3
        );
        engine.debug_assert_consistency();
    }

    #[test]
    fn fact_index_returns_minus_one_for_nonaddresses_without_normalizing_them_into_live_keys() {
        let mut engine = crate::Engine::with_rules("").unwrap();
        engine.assert_ordered("item", 7_i64).unwrap();
        for value in [
            Value::Integer(0),
            Value::Integer(1),
            Value::Integer(-1),
            Value::Integer(i64::MIN),
            Value::Integer((2_i64 << 32) | 1), // Even generation.
            Value::Integer(1_i64 << 32),       // Reserved slot zero.
            Value::Integer((1_i64 << 32) | i64::from(u32::MAX)), // Null key.
            Value::Float(1.0),
            Value::String(FerricString::new("1", StringEncoding::Utf8).unwrap()),
        ] {
            // CLIPS 6.30 reports a recoverable notice and returns -1.
            let index = eval_index(value.clone(), &mut engine).unwrap();
            assert_eq!(index, -1, "{value:?}");
        }
    }

    #[test]
    fn fact_index_reports_dummy_and_absent_addresses() {
        let mut engine = crate::Engine::with_rules("").unwrap();
        let initial = address_value(&engine, engine.initial_fact_id.unwrap());
        assert_eq!(
            eval_index(initial, &mut crate::Engine::new(EngineConfig::utf8())).unwrap(),
            -1
        );
        assert_eq!(
            eval_index(Value::FactAddress(FactAddress::dummy()), &mut engine).unwrap(),
            -1
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn fact_index_checks_the_integer_boundary_before_initial_fact_installation() {
        let mut engine = crate::Engine::new(EngineConfig::utf8());
        let maximum = u64::try_from(i64::MAX).unwrap();
        for timestamp in [maximum - 1, maximum, u64::MAX - 1] {
            let mut state = serde_json::to_value(&engine.fact_base).unwrap();
            state["next_timestamp"] = serde_json::json!(timestamp);
            engine.fact_base = serde_json::from_value(state).unwrap();
            engine
                .assert_ordered(
                    "item",
                    FerricString::new(&timestamp.to_string(), StringEncoding::Utf8).unwrap(),
                )
                .unwrap();
            let (id, _) = engine
                .fact_base
                .iter()
                .max_by_key(|(_, entry)| entry.timestamp)
                .unwrap();
            let result = eval_index(address_value(&engine, id), &mut engine);
            if timestamp == maximum - 1 {
                assert_eq!(result.unwrap(), i64::MAX);
            } else {
                assert!(
                    matches!(result, Err(EvalError::UnsupportedOperation { ref operation, .. })
                    if operation == "fact-index")
                );
            }
        }
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
        let (mut engine, mut vm, mut bs) = test_ctx();
        let sym = engine
            .symbol_table
            .intern_symbol("x", StringEncoding::Utf8)
            .unwrap();
        let var_id = vm.get_or_create(sym).unwrap();
        bs.set(var_id, ValueRef::new(Value::Integer(99)));

        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, vm, bs) = test_ctx();
        let action = ferric_rules_parser::ActionExpr::Variable("missing".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut engine.symbol_table, &engine.config).unwrap();

        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, vm, bs) = test_ctx();
        let action =
            ferric_rules_parser::ActionExpr::GlobalVariable("count".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut engine.symbol_table, &engine.config).unwrap();

        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, vm, bs) = test_ctx();
        engine.globals.set(
            engine.module_registry.main_module_id(),
            "count",
            Value::Integer(42),
        );
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, vm, bs) = test_ctx();
        engine.globals.set(
            engine.module_registry.main_module_id(),
            "x",
            Value::Integer(0),
        );
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
            .engine
            .globals
            .get(ctx.engine.module_registry.main_module_id(), "x")
            .unwrap()
            .structural_eq(&Value::Integer(99)));
    }

    #[test]
    fn bind_with_literal_target_returns_type_error() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
            body: vec![ferric_rules_parser::ActionExpr::FunctionCall(
                ferric_rules_parser::FunctionCall {
                    name: "*".to_string(),
                    args: vec![
                        ferric_rules_parser::ActionExpr::Variable("x".to_string(), dummy_span()),
                        ferric_rules_parser::ActionExpr::Literal(
                            ferric_rules_parser::LiteralValue {
                                value: ferric_rules_parser::LiteralKind::Integer(2),
                                span: dummy_span(),
                            },
                        ),
                    ],
                    span: dummy_span(),
                },
            )],
        }
    }

    #[test]
    fn user_function_simple_call() {
        let (mut engine, vm, bs) = test_ctx();
        engine
            .functions
            .register(engine.module_registry.main_module_id(), make_double_func());
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
            body: vec![ferric_rules_parser::ActionExpr::FunctionCall(
                ferric_rules_parser::FunctionCall {
                    name: "+".to_string(),
                    args: vec![
                        ferric_rules_parser::ActionExpr::Variable("a".to_string(), dummy_span()),
                        ferric_rules_parser::ActionExpr::Variable("b".to_string(), dummy_span()),
                    ],
                    span: dummy_span(),
                },
            )],
        };
        let (mut engine, vm, bs) = test_ctx();
        engine
            .functions
            .register(engine.module_registry.main_module_id(), func);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("add", vec![int(3), int(7)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(10)));
    }

    #[test]
    fn user_function_wrong_arity_returns_error() {
        let (mut engine, vm, bs) = test_ctx();
        engine
            .functions
            .register(engine.module_registry.main_module_id(), make_double_func());
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
            body: vec![ferric_rules_parser::ActionExpr::Variable(
                "x".to_string(),
                dummy_span(),
            )],
        };
        let (mut engine, vm, bs) = test_ctx();
        engine
            .functions
            .register(engine.module_registry.main_module_id(), func);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
            body: vec![ferric_rules_parser::ActionExpr::FunctionCall(
                ferric_rules_parser::FunctionCall {
                    name: "inf".to_string(),
                    args: vec![ferric_rules_parser::ActionExpr::Variable(
                        "x".to_string(),
                        dummy_span(),
                    )],
                    span: dummy_span(),
                },
            )],
        };
        let (mut engine, vm, bs) = test_ctx();
        engine.config.max_call_depth = 10; // Low limit for the test
        engine
            .functions
            .register(engine.module_registry.main_module_id(), func);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("inf", vec![int(1)]);
        let result = eval(&mut ctx, &expr);
        assert!(matches!(result, Err(EvalError::RecursionLimit { .. })));
    }

    #[cfg(feature = "tracing")]
    #[test]
    fn eval_run_guard_tracks_root_scope() {
        assert!(!EvalRunGuard::is_active());
        {
            let _guard = EvalRunGuard::enter_root();
            assert!(EvalRunGuard::is_active());
        }
        assert!(!EvalRunGuard::is_active());
    }

    #[cfg(feature = "tracing")]
    #[test]
    fn eval_root_guard_resets_after_error() {
        let func = UserFunction {
            name: "inf".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: None,
            body: vec![ferric_rules_parser::ActionExpr::FunctionCall(
                ferric_rules_parser::FunctionCall {
                    name: "inf".to_string(),
                    args: vec![ferric_rules_parser::ActionExpr::Variable(
                        "x".to_string(),
                        dummy_span(),
                    )],
                    span: dummy_span(),
                },
            )],
        };
        let (mut engine, vm, bs) = test_ctx();
        engine.config.max_call_depth = 3;
        engine
            .functions
            .register(engine.module_registry.main_module_id(), func);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let recursive_expr = call("inf", vec![int(1)]);
        let first_result = eval(&mut ctx, &recursive_expr);
        assert!(matches!(
            first_result,
            Err(EvalError::RecursionLimit { .. })
        ));
        assert!(!EvalRunGuard::is_active());

        let literal_expr = int(42);
        let second_result = eval(&mut ctx, &literal_expr);
        assert!(matches!(second_result, Ok(Value::Integer(42))));
        assert!(!EvalRunGuard::is_active());
    }

    #[test]
    fn user_function_calls_builtin() {
        // (deffunction inc (?x) (+ ?x 1))
        let func = UserFunction {
            name: "inc".to_string(),
            parameters: vec!["x".to_string()],
            wildcard_parameter: None,
            body: vec![ferric_rules_parser::ActionExpr::FunctionCall(
                ferric_rules_parser::FunctionCall {
                    name: "+".to_string(),
                    args: vec![
                        ferric_rules_parser::ActionExpr::Variable("x".to_string(), dummy_span()),
                        ferric_rules_parser::ActionExpr::Literal(
                            ferric_rules_parser::LiteralValue {
                                value: ferric_rules_parser::LiteralKind::Integer(1),
                                span: dummy_span(),
                            },
                        ),
                    ],
                    span: dummy_span(),
                },
            )],
        };
        let (mut engine, vm, bs) = test_ctx();
        engine
            .functions
            .register(engine.module_registry.main_module_id(), func);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
            body: vec![ferric_rules_parser::ActionExpr::FunctionCall(
                ferric_rules_parser::FunctionCall {
                    name: "double".to_string(),
                    args: vec![ferric_rules_parser::ActionExpr::FunctionCall(
                        ferric_rules_parser::FunctionCall {
                            name: "double".to_string(),
                            args: vec![ferric_rules_parser::ActionExpr::Variable(
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
        let (mut engine, vm, bs) = test_ctx();
        engine
            .functions
            .register(engine.module_registry.main_module_id(), double);
        engine
            .functions
            .register(engine.module_registry.main_module_id(), quadruple);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call(">", vec![int(5), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_lt_false() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("<", vec![int(5), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_eq_numeric_true() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("=", vec![int(3), int(3)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_neq_numeric() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("!=", vec![int(3), int(4)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_gte_equal() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call(">=", vec![int(5), int(5)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_lte_less() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("<=", vec![int(3), int(5)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    // -------------------------------------------------------------------
    // Value equality: eq / neq
    // -------------------------------------------------------------------

    #[test]
    fn eval_eq_same_integers() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("eq", vec![int(42), int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_neq_different_types() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("neq", vec![int(1), float(1.0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        // Integer(1) and Float(1.0) are different types under structural_eq
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    // -------------------------------------------------------------------
    // Boolean
    // -------------------------------------------------------------------

    #[test]
    fn eval_and_both_true() {
        let (mut engine, vm, bs) = test_ctx();
        let true_sym = clips_true(&mut engine.symbol_table, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call(
            "and",
            vec![
                RuntimeExpr::Literal(true_sym.clone()),
                RuntimeExpr::Literal(true_sym),
            ],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_and_one_false() {
        let (mut engine, vm, bs) = test_ctx();
        let true_sym = clips_true(&mut engine.symbol_table, StringEncoding::Utf8);
        let false_sym = clips_false(&mut engine.symbol_table, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call(
            "and",
            vec![
                RuntimeExpr::Literal(true_sym),
                RuntimeExpr::Literal(false_sym),
            ],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_or_one_true() {
        let (mut engine, vm, bs) = test_ctx();
        let false_sym = clips_false(&mut engine.symbol_table, StringEncoding::Utf8);
        let true_sym = clips_true(&mut engine.symbol_table, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call(
            "or",
            vec![
                RuntimeExpr::Literal(false_sym),
                RuntimeExpr::Literal(true_sym),
            ],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_not_false_returns_true() {
        let (mut engine, vm, bs) = test_ctx();
        let false_sym = clips_false(&mut engine.symbol_table, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("not", vec![RuntimeExpr::Literal(false_sym)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_not_true_returns_false() {
        let (mut engine, vm, bs) = test_ctx();
        let true_sym = clips_true(&mut engine.symbol_table, StringEncoding::Utf8);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("not", vec![RuntimeExpr::Literal(true_sym)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
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
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("integerp", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_integerp_false_on_float() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("integerp", vec![float(3.125)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_floatp_true() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("floatp", vec![float(3.125)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_numberp_integer() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("numberp", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_numberp_float() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("numberp", vec![float(1.0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_symbolp_true() {
        let (mut engine, vm, bs) = test_ctx();
        let sym = engine
            .symbol_table
            .intern_symbol("foo", StringEncoding::Utf8)
            .unwrap();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("symbolp", vec![RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_stringp_true() {
        let fs = FerricString::new("hello", StringEncoding::Utf8).unwrap();
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("stringp", vec![RuntimeExpr::Literal(Value::String(fs))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    // -------------------------------------------------------------------
    // lexemep
    // -------------------------------------------------------------------

    #[test]
    fn eval_lexemep_true_for_symbol() {
        let (mut engine, vm, bs) = test_ctx();
        let sym = engine
            .symbol_table
            .intern_symbol("hello", StringEncoding::Utf8)
            .unwrap();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("lexemep", vec![RuntimeExpr::Literal(Value::Symbol(sym))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_lexemep_true_for_string() {
        let fs = FerricString::new("hi", StringEncoding::Utf8).unwrap();
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("lexemep", vec![RuntimeExpr::Literal(Value::String(fs))]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_lexemep_false_for_integer() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("lexemep", vec![int(42)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
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
        use ferric_rules_core::value::Multifield;
        let mf = Multifield::new();
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call(
            "multifieldp",
            vec![RuntimeExpr::Literal(Value::Multifield(Box::new(mf)))],
        );
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_multifieldp_false_for_integer() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("multifieldp", vec![int(0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
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
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("evenp", vec![int(4)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_evenp_true_for_zero() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("evenp", vec![int(0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_evenp_false_for_odd_integer() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("evenp", vec![int(7)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
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
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("oddp", vec![int(7)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_oddp_false_for_zero() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("oddp", vec![int(0)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn eval_oddp_false_for_even_integer() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("oddp", vec![int(4)]);
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
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
        let (mut engine, vm, bs) = test_ctx();
        let sym = engine
            .symbol_table
            .intern_symbol("foo", StringEncoding::Utf8)
            .unwrap();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, vm, bs) = test_ctx();
        let sym = engine
            .symbol_table
            .intern_symbol("bar", StringEncoding::Utf8)
            .unwrap();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        // An unknown name inside a builtin's arguments keeps its own name.
        let nested = call("abs", vec![expr]);
        assert!(matches!(
            eval_expr(&nested),
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
        let (mut engine, vm, bs) = test_ctx();
        let sym = engine
            .symbol_table
            .intern_symbol("foo", StringEncoding::Utf8)
            .unwrap();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, _, _) = test_ctx();
        let action = ferric_rules_parser::ActionExpr::Literal(ferric_rules_parser::LiteralValue {
            value: ferric_rules_parser::LiteralKind::Integer(42),
            span: dummy_span(),
        });
        let runtime = from_action_expr(&action, &mut engine.symbol_table, &engine.config).unwrap();
        assert!(matches!(runtime, RuntimeExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn translate_action_expr_variable() {
        let (mut engine, _, _) = test_ctx();
        let action = ferric_rules_parser::ActionExpr::Variable("x".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut engine.symbol_table, &engine.config).unwrap();
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
        let (mut engine, _, _) = test_ctx();
        let action =
            ferric_rules_parser::ActionExpr::GlobalVariable("count".to_string(), dummy_span());
        let runtime = from_action_expr(&action, &mut engine.symbol_table, &engine.config).unwrap();
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
        let (mut engine, _, _) = test_ctx();
        let action =
            ferric_rules_parser::ActionExpr::FunctionCall(ferric_rules_parser::FunctionCall {
                name: "+".to_string(),
                args: vec![
                    ferric_rules_parser::ActionExpr::Literal(ferric_rules_parser::LiteralValue {
                        value: ferric_rules_parser::LiteralKind::Integer(1),
                        span: dummy_span(),
                    }),
                    ferric_rules_parser::ActionExpr::Literal(ferric_rules_parser::LiteralValue {
                        value: ferric_rules_parser::LiteralKind::Integer(2),
                        span: dummy_span(),
                    }),
                ],
                span: dummy_span(),
            });
        let runtime = from_action_expr(&action, &mut engine.symbol_table, &engine.config).unwrap();
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
        let (mut engine, _, _) = test_ctx();
        let sexpr =
            ferric_rules_parser::SExpr::Atom(ferric_rules_parser::Atom::Integer(42), dummy_span());
        let runtime = from_sexpr(&sexpr, &mut engine.symbol_table, &engine.config).unwrap();
        assert!(matches!(runtime, RuntimeExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn translate_sexpr_function_call() {
        let (mut engine, _, _) = test_ctx();
        let sexpr = ferric_rules_parser::SExpr::List(
            vec![
                ferric_rules_parser::SExpr::Atom(
                    ferric_rules_parser::Atom::Symbol(">".to_string()),
                    dummy_span(),
                ),
                ferric_rules_parser::SExpr::Atom(
                    ferric_rules_parser::Atom::SingleVar("x".to_string()),
                    dummy_span(),
                ),
                ferric_rules_parser::SExpr::Atom(
                    ferric_rules_parser::Atom::Integer(10),
                    dummy_span(),
                ),
            ],
            dummy_span(),
        );
        let runtime = from_sexpr(&sexpr, &mut engine.symbol_table, &engine.config).unwrap();
        assert!(matches!(
            runtime,
            RuntimeExpr::Call { ref name, ref args, .. } if name == ">" && args.len() == 2
        ));
    }

    // -------------------------------------------------------------------
    // Specificity scoring unit tests
    // -------------------------------------------------------------------

    fn types(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn type_list_compare_empty_lists() {
        assert_eq!(
            type_list_compare(&[], &[]),
            RestrictionPrecedence::Identical
        );
        assert_eq!(
            type_list_compare(&[], &types(&["INTEGER"])),
            RestrictionPrecedence::Lower
        );
        assert_eq!(
            type_list_compare(&types(&["OBJECT"]), &[]),
            RestrictionPrecedence::Higher
        );
    }

    #[test]
    fn type_list_compare_subclass_wins_at_first_related_position() {
        // CLIPS: (INTEGER SYMBOL) outranks (NUMBER), and (SYMBOL INTEGER
        // FLOAT) outranks (LEXEME), despite covering more types.
        assert_eq!(
            type_list_compare(&types(&["INTEGER", "SYMBOL"]), &types(&["NUMBER"])),
            RestrictionPrecedence::Higher
        );
        assert_eq!(
            type_list_compare(&types(&["LEXEME"]), &types(&["SYMBOL", "INTEGER", "FLOAT"])),
            RestrictionPrecedence::Lower
        );
        // An unrelated first position does not stop the scan.
        assert_eq!(
            type_list_compare(&types(&["INTEGER", "LEXEME"]), &types(&["FLOAT", "SYMBOL"])),
            RestrictionPrecedence::Lower
        );
        assert_eq!(
            type_list_compare(&types(&["NUMBER", "SYMBOL"]), &types(&["PRIMITIVE"])),
            RestrictionPrecedence::Higher
        );
        assert_eq!(
            type_list_compare(&types(&["INSTANCE-ADDRESS"]), &types(&["ADDRESS"])),
            RestrictionPrecedence::Higher
        );
    }

    #[test]
    fn type_list_compare_instance_name_is_not_a_symbol() {
        // In CLIPS 6.30, INSTANCE-NAME's superclasses are INSTANCE,
        // PRIMITIVE, and OBJECT, so it is unrelated to LEXEME.
        assert_eq!(
            type_list_compare(&types(&["INSTANCE-NAME", "SYMBOL"]), &types(&["LEXEME"])),
            RestrictionPrecedence::Lower
        );
    }

    #[test]
    fn type_list_compare_shorter_wins_then_different() {
        assert_eq!(
            type_list_compare(&types(&["INTEGER"]), &types(&["INTEGER", "SYMBOL"])),
            RestrictionPrecedence::Higher
        );
        assert_eq!(
            type_list_compare(
                &types(&["INTEGER", "SYMBOL"]),
                &types(&["INTEGER", "STRING"])
            ),
            RestrictionPrecedence::Different
        );
        assert_eq!(
            type_list_compare(
                &types(&["INTEGER", "SYMBOL"]),
                &types(&["SYMBOL", "INTEGER"])
            ),
            RestrictionPrecedence::Different
        );
        assert_eq!(
            type_list_compare(
                &types(&["INTEGER", "SYMBOL"]),
                &types(&["INTEGER", "SYMBOL"])
            ),
            RestrictionPrecedence::Identical
        );
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
            parameter_queries: vec![None; type_restrictions.len()],
            type_restrictions,
            wildcard_type_restrictions: vec![],
            wildcard_query: None,
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
            compare_method_restrictions(&integer_method, &number_method),
            std::cmp::Ordering::Less,
            "INTEGER method should be more specific (Less) than NUMBER method"
        );
        assert_eq!(
            compare_method_restrictions(&number_method, &integer_method),
            std::cmp::Ordering::Greater,
        );
    }

    #[test]
    fn compare_specificity_restricted_more_specific_than_unrestricted() {
        let restricted = make_method(0, vec![vec!["INTEGER".to_string()]], false);
        let unrestricted = make_method(1, vec![vec![]], false);
        assert_eq!(
            compare_method_restrictions(&restricted, &unrestricted),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn compare_specificity_no_wildcard_more_specific_than_wildcard() {
        let fixed = make_method(0, vec![vec!["INTEGER".to_string()]], false);
        let variadic = make_method(1, vec![vec!["INTEGER".to_string()]], true);
        assert_eq!(
            compare_method_restrictions(&fixed, &variadic),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn compare_specificity_identical_restrictions_tie() {
        // Identical restrictions: neither outranks the other, so definition
        // order decides.
        let m0 = make_method(0, vec![vec!["INTEGER".to_string()]], false);
        let m1 = make_method(1, vec![vec!["INTEGER".to_string()]], false);
        assert_eq!(
            compare_method_restrictions(&m0, &m1),
            std::cmp::Ordering::Equal,
        );
        assert!(!method_has_higher_precedence(&m0, &m1));
        assert!(!method_has_higher_precedence(&m1, &m0));
    }

    fn typed_wildcard(
        mut method: crate::functions::RegisteredMethod,
        types: &[&str],
        query: bool,
    ) -> crate::functions::RegisteredMethod {
        method.wildcard_type_restrictions = types.iter().map(|kind| (*kind).to_string()).collect();
        method.wildcard_query = query.then(|| {
            ferric_rules_parser::ActionExpr::Literal(ferric_rules_parser::LiteralValue {
                value: ferric_rules_parser::LiteralKind::Symbol("TRUE".into()),
                span: dummy_span(),
            })
        });
        method
    }

    #[test]
    fn compare_specificity_untyped_fixed_prefix_beats_wildcard() {
        // CLIPS: (?x) outranks (?x $?y), and () outranks ($?y).
        let fixed = make_method(1, vec![vec![]], false);
        let variadic = make_method(0, vec![vec![]], true);
        assert_eq!(
            compare_method_restrictions(&fixed, &variadic),
            std::cmp::Ordering::Less,
        );
        let empty = make_method(1, vec![], false);
        let wildcard = make_method(0, vec![], true);
        assert_eq!(
            compare_method_restrictions(&empty, &wildcard),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn compare_specificity_typed_wildcard_beats_untyped_fixed_wildcard() {
        // c4/p3: (($?xs INTEGER)) outranks (?x $?xs).
        let typed = typed_wildcard(make_method(1, vec![], true), &["INTEGER"], false);
        let untyped = make_method(2, vec![vec![]], true);
        assert_eq!(
            compare_method_restrictions(&typed, &untyped),
            std::cmp::Ordering::Less,
        );
        // p8: (($?xs INTEGER)) outranks (?x ?y $?z).
        let two_fixed_wild = make_method(2, vec![vec![], vec![]], true);
        assert_eq!(
            compare_method_restrictions(&typed, &two_fixed_wild),
            std::cmp::Ordering::Less,
        );
        // p7: a wildcard slot loses to a regular parameter of a method with
        // no wildcard, whatever its types: (?x ?y) outranks (($?xs INTEGER)).
        let two_fixed = make_method(2, vec![vec![], vec![]], false);
        assert_eq!(
            compare_method_restrictions(&typed, &two_fixed),
            std::cmp::Ordering::Greater,
        );
    }

    #[test]
    fn compare_specificity_wildcard_slot_compares_with_later_fixed_slot() {
        // p9: ((?x INTEGER) ($?xs INTEGER)) outranks ((?x INTEGER) ?y $?z).
        let a = typed_wildcard(
            make_method(1, vec![vec!["INTEGER".to_string()]], true),
            &["INTEGER"],
            false,
        );
        let b = make_method(2, vec![vec!["INTEGER".to_string()], vec![]], true);
        assert_eq!(
            compare_method_restrictions(&a, &b),
            std::cmp::Ordering::Less,
        );
        // p5: equal shared slots, so more slots wins:
        // ((?x INTEGER) $?xs) outranks (($?xs INTEGER)).
        let typed = typed_wildcard(make_method(1, vec![], true), &["INTEGER"], false);
        let int_fixed_wild = make_method(2, vec![vec!["INTEGER".to_string()]], true);
        assert_eq!(
            compare_method_restrictions(&int_fixed_wild, &typed),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn compare_specificity_queried_wildcard_beats_untyped_fixed_wildcard() {
        // p10: (($?xs (> 1 0))) outranks (?x $?xs).
        let queried = typed_wildcard(make_method(1, vec![], true), &[], true);
        let untyped = make_method(2, vec![vec![]], true);
        assert_eq!(
            compare_method_restrictions(&queried, &untyped),
            std::cmp::Ordering::Less,
        );
    }

    fn with_query(
        mut method: crate::functions::RegisteredMethod,
        slot: usize,
    ) -> crate::functions::RegisteredMethod {
        method.parameter_queries[slot] = Some(ferric_rules_parser::ActionExpr::Literal(
            ferric_rules_parser::LiteralValue {
                value: ferric_rules_parser::LiteralKind::Symbol("TRUE".into()),
                span: dummy_span(),
            },
        ));
        method
    }

    #[test]
    fn compare_specificity_different_types_stop_before_query() {
        // CLIPS: ((?x INTEGER SYMBOL)) and ((?x INTEGER STRING (eq ?x 1)))
        // differ, so the query is never consulted and neither outranks.
        let plain = make_method(1, vec![types(&["INTEGER", "SYMBOL"])], false);
        let queried = with_query(
            make_method(2, vec![types(&["INTEGER", "STRING"])], false),
            0,
        );
        assert_eq!(
            compare_method_restrictions(&queried, &plain),
            std::cmp::Ordering::Equal,
        );
        assert_eq!(
            compare_method_restrictions(&plain, &queried),
            std::cmp::Ordering::Equal,
        );
        // With identical types, the query decides.
        let queried_same = with_query(
            make_method(2, vec![types(&["INTEGER", "SYMBOL"])], false),
            0,
        );
        assert_eq!(
            compare_method_restrictions(&queried_same, &plain),
            std::cmp::Ordering::Less,
        );
    }

    #[test]
    fn compare_specificity_different_types_stop_before_later_slots() {
        // CLIPS: ((?x INTEGER SYMBOL) ?y) and ((?x INTEGER STRING) (?y
        // INTEGER)) differ in the first slot, so the second is not compared.
        let a = make_method(1, vec![types(&["INTEGER", "SYMBOL"]), vec![]], false);
        let b = make_method(
            2,
            vec![types(&["INTEGER", "STRING"]), types(&["INTEGER"])],
            false,
        );
        assert_eq!(
            compare_method_restrictions(&b, &a),
            std::cmp::Ordering::Equal
        );
        assert!(!method_has_higher_precedence(&b, &a));
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

    #[test]
    fn eval_error_display_action_iteration_limit() {
        let err = EvalError::ActionIterationLimit {
            function: "loop-for-count".to_string(),
            limit: 10,
            span: Some(SourceSpan { line: 4, column: 9 }),
        };
        assert_eq!(
            err.to_string(),
            "action iteration limit exceeded in `loop-for-count` (limit 10) at line 4:9"
        );
    }

    // -------------------------------------------------------------------
    // String/Symbol built-ins
    // -------------------------------------------------------------------

    /// Helper: make a STRING `RuntimeExpr` literal.
    fn str_lit(s: &str) -> RuntimeExpr {
        let fs = FerricString::new(s, StringEncoding::Utf8).unwrap();
        RuntimeExpr::Literal(Value::String(fs))
    }

    #[test]
    fn str_cat_zero_args_rejected() {
        assert!(matches!(
            eval_expr(&call("str-cat", vec![])),
            Err(EvalError::ArityMismatch { .. })
        ));
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
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let expr = call("sym-cat", vec![str_lit("foo"), str_lit("bar")]);
        let result = eval(&mut ctx, &expr).unwrap();
        match result {
            Value::Symbol(sym) => {
                let name = ctx.engine.symbol_table.resolve_symbol_str(sym);
                assert_eq!(name, Some("foobar"), "sym-cat should intern 'foobar'");
            }
            other => panic!("expected SYMBOL, got {other:?}"),
        }
    }

    #[test]
    fn sym_cat_zero_args_rejected() {
        assert!(matches!(
            eval_expr(&call("sym-cat", vec![])),
            Err(EvalError::ArityMismatch { .. })
        ));
    }

    #[test]
    fn gensym_generates_incrementing_symbols() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };

        let first = eval(&mut ctx, &call("gensym", vec![])).unwrap();
        let second = eval(&mut ctx, &call("gensym", vec![])).unwrap();
        let third = eval(&mut ctx, &call("gensym*", vec![])).unwrap();

        let Value::Symbol(first_sym) = first else {
            panic!("expected SYMBOL from gensym")
        };
        let Value::Symbol(second_sym) = second else {
            panic!("expected SYMBOL from gensym")
        };
        let Value::Symbol(third_sym) = third else {
            panic!("expected SYMBOL from gensym*")
        };

        assert_eq!(
            ctx.engine.symbol_table.resolve_symbol_str(first_sym),
            Some("gen1")
        );
        assert_eq!(
            ctx.engine.symbol_table.resolve_symbol_str(second_sym),
            Some("gen2")
        );
        assert_eq!(
            ctx.engine.symbol_table.resolve_symbol_str(third_sym),
            Some("gen3")
        );
    }

    #[test]
    fn setgen_sets_next_generated_symbol() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };

        let set_result = eval(&mut ctx, &call("setgen", vec![int(10)])).unwrap();
        assert!(set_result.structural_eq(&Value::Integer(10)));
        let generated = eval(&mut ctx, &call("gensym", vec![])).unwrap();
        let Value::Symbol(generated_sym) = generated else {
            panic!("expected SYMBOL from gensym")
        };
        assert_eq!(
            ctx.engine.symbol_table.resolve_symbol_str(generated_sym),
            Some("gen10")
        );
    }

    #[test]
    fn setgen_requires_positive_integer() {
        let result = eval_expr(&call("setgen", vec![int(0)]));
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn gensym_requires_no_arguments() {
        let result = eval_expr(&call("gensym", vec![int(1)]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn setgen_requires_one_argument() {
        let result = eval_expr(&call("setgen", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn set_fact_duplication_returns_boolean_symbol() {
        let result = eval_expr(&call("set-fact-duplication", vec![int(1)])).unwrap();
        assert!(matches!(result, Value::Symbol(_)));
    }

    #[test]
    fn refresh_agenda_reports_unsupported_operation() {
        let result = eval_expr(&call("refresh-agenda", vec![]));
        assert!(matches!(
            result,
            Err(EvalError::UnsupportedOperation { .. })
        ));
    }

    #[test]
    fn watch_requires_at_least_one_argument() {
        let result = eval_expr(&call("watch", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn watch_and_unwatch_with_argument_succeed() {
        let mut engine = crate::Engine::new(EngineConfig::default());
        assert!(matches!(
            engine.eval_str("(watch facts)").unwrap(),
            Value::Void
        ));
        assert!(engine.watch_facts());
        assert!(matches!(
            engine.eval_str("(unwatch facts)").unwrap(),
            Value::Void
        ));
        assert!(!engine.watch_facts());
    }

    #[test]
    fn close_returns_boolean_symbol() {
        let result = eval_expr(&call("close", vec![str_lit("t")])).unwrap();
        assert!(matches!(result, Value::Symbol(_)));
    }

    #[test]
    fn printout_queues_deferred_output_event() {
        let (mut engine, vm, bs) = test_ctx();
        let channel_sym = engine
            .symbol_table
            .intern_symbol("t", StringEncoding::Utf8)
            .unwrap();
        let tab_sym = engine
            .symbol_table
            .intern_symbol("tab", StringEncoding::Utf8)
            .unwrap();
        let crlf_sym = engine
            .symbol_table
            .intern_symbol("crlf", StringEncoding::Utf8)
            .unwrap();

        {
            let mut ctx = EvalContext {
                global_module: None,
                bindings: &bs,
                var_map: &vm,
                callable_locals: None,
                call_depth: 0,
                expression_depth: 0,
                current_module: engine.module_registry.main_module_id(),
                method_chain: None,
                compact_fact_bindings: None,
                engine: &mut engine,
                allow_engine_effects: true,
            };
            let expr = call(
                "printout",
                vec![
                    RuntimeExpr::Literal(Value::Symbol(channel_sym)),
                    str_lit("v="),
                    RuntimeExpr::Literal(Value::Symbol(tab_sym)),
                    int(7),
                    RuntimeExpr::Literal(Value::Symbol(crlf_sym)),
                ],
            );
            let result = eval(&mut ctx, &expr).unwrap();
            assert!(matches!(result, Value::Void));
        }

        let events = engine.globals.take_printout_events();
        assert!(events.iter().all(|(channel, _)| channel == "t"));
        let output: String = events.into_iter().map(|(_, text)| text).collect();
        assert_eq!(output, "v=\t7\n");
    }

    #[test]
    fn return_without_argument_is_invalid_outside_callable() {
        let error = eval_expr(&call("return", vec![]))
            .expect_err("top-level return must not escape as an ordinary value");
        assert!(
            error.to_string().contains("not valid outside a callable"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn return_with_argument_is_invalid_outside_callable() {
        let error = eval_expr(&call("return", vec![int(123)]))
            .expect_err("top-level return must not escape as an ordinary value");
        assert!(
            error.to_string().contains("not valid outside a callable"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn return_argument_error_preserves_original_diagnostic() {
        let division = call("/", vec![int(1), int(0)]);
        let result = eval_expr(&call("return", vec![division]));
        assert!(matches!(result, Err(EvalError::DivisionByZero { .. })));
    }

    #[test]
    fn load_returns_false_symbol_for_placeholder_path() {
        let (mut engine, vm, bs) = test_ctx();
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &call("load", vec![str_lit("x.clp")])).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn rules_accepts_zero_or_one_arguments() {
        let no_arg = eval_expr(&call("rules", vec![])).unwrap();
        assert!(matches!(no_arg, Value::Symbol(_)));
        let one_arg = eval_expr(&call("rules", vec![str_lit("MAIN")])).unwrap();
        assert!(matches!(one_arg, Value::Symbol(_)));
        let too_many = eval_expr(&call("rules", vec![str_lit("MAIN"), str_lit("X")]));
        assert!(matches!(too_many, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn undefrule_and_ppdefrule_return_boolean_symbol() {
        let undef = eval_expr(&call("undefrule", vec![str_lit("r1")])).unwrap();
        assert!(matches!(undef, Value::Symbol(_)));
        let pp = eval_expr(&call("ppdefrule", vec![str_lit("r1")])).unwrap();
        assert!(matches!(pp, Value::Symbol(_)));
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
    fn sub_string_zero_start_clips_to_one() {
        // start < 1: include characters starting at position one
        let expr = call("sub-string", vec![int(0), int(3), str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "hel"),
            other => panic!("expected STRING, got {other:?}"),
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
    // Multifield built-ins
    // -------------------------------------------------------------------

    /// Helper: create a MULTIFIELD `RuntimeExpr` literal from a `Vec` of `Value`s.
    fn mf_lit(values: Vec<Value>) -> RuntimeExpr {
        let mf: ferric_rules_core::value::Multifield = values.into_iter().collect();
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
    fn length_alias_returns_string_length() {
        let expr = call("length", vec![str_lit("hello")]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(5)));
    }

    #[test]
    fn length_alias_returns_multifield_length() {
        let expr = call(
            "length",
            vec![mf_lit(vec![Value::Integer(1), Value::Integer(2)])],
        );
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(2)));
    }

    #[test]
    fn length_alias_type_error_on_integer() {
        let result = eval_expr(&call("length", vec![int(42)]));
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn subseq_mf_extracts_inclusive_slice() {
        let expr = call(
            "subseq$",
            vec![
                mf_lit(vec![
                    Value::Integer(10),
                    Value::Integer(20),
                    Value::Integer(30),
                    Value::Integer(40),
                ]),
                int(2),
                int(3),
            ],
        );
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::Multifield(mf) => {
                assert_eq!(mf.len(), 2);
                assert!(mf[0].structural_eq(&Value::Integer(20)));
                assert!(mf[1].structural_eq(&Value::Integer(30)));
            }
            other => panic!("expected MULTIFIELD, got {other:?}"),
        }
    }

    #[test]
    fn subseq_mf_clamps_end_index() {
        let expr = call(
            "subseq$",
            vec![
                mf_lit(vec![
                    Value::Integer(1),
                    Value::Integer(2),
                    Value::Integer(3),
                ]),
                int(2),
                int(99),
            ],
        );
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::Multifield(mf) => {
                assert_eq!(mf.len(), 2);
                assert!(mf[0].structural_eq(&Value::Integer(2)));
                assert!(mf[1].structural_eq(&Value::Integer(3)));
            }
            other => panic!("expected MULTIFIELD, got {other:?}"),
        }
    }

    #[test]
    fn subseq_mf_inverted_range_returns_empty() {
        let expr = call(
            "subseq$",
            vec![
                mf_lit(vec![
                    Value::Integer(1),
                    Value::Integer(2),
                    Value::Integer(3),
                ]),
                int(3),
                int(1),
            ],
        );
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::Multifield(mf) => assert!(mf.is_empty()),
            other => panic!("expected MULTIFIELD, got {other:?}"),
        }
    }

    #[test]
    fn subseq_mf_type_error_on_non_multifield() {
        let result = eval_expr(&call("subseq$", vec![int(1), int(1), int(2)]));
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn implode_mf_joins_with_spaces() {
        let mf = mf_lit(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]);
        let expr = call("implode$", vec![mf]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), "10 20 30"),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn implode_mf_empty_returns_empty_string() {
        let expr = call("implode$", vec![mf_lit(vec![])]);
        let result = eval_expr(&expr).unwrap();
        match result {
            Value::String(s) => assert_eq!(s.as_str(), ""),
            other => panic!("expected STRING, got {other:?}"),
        }
    }

    #[test]
    fn implode_mf_arity_error() {
        let result = eval_expr(&call("implode$", vec![]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn implode_mf_type_error_non_multifield() {
        let result = eval_expr(&call("implode$", vec![int(42)]));
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
    fn nth_mf_out_of_range_returns_nil() {
        let mf = mf_lit(vec![Value::Integer(10), Value::Integer(20)]);
        let expr = call("nth$", vec![int(5), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Ok(Value::Symbol(_))));
    }

    #[test]
    fn nth_mf_zero_index_returns_nil() {
        let mf = mf_lit(vec![Value::Integer(10)]);
        let expr = call("nth$", vec![int(0), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Ok(Value::Symbol(_))));
    }

    #[test]
    fn nth_mf_negative_index_returns_nil() {
        let mf = mf_lit(vec![Value::Integer(10)]);
        let expr = call("nth$", vec![int(-1), mf]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Ok(Value::Symbol(_))));
    }

    #[test]
    fn nth_mf_arity_error() {
        let result = eval_expr(&call("nth$", vec![int(1)]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
    }

    #[test]
    fn nth_mf_runtime_float_index_truncates() {
        let mf = mf_lit(vec![Value::Integer(10)]);
        let expr = call("nth$", vec![float(1.9), mf]);
        assert!(matches!(eval_expr(&expr), Ok(Value::Integer(10))));
    }

    #[test]
    fn nth_mf_type_error_nonnumeric_index() {
        let mf = mf_lit(vec![Value::Integer(10)]);
        let expr = call("nth$", vec![str_lit("1"), mf]);
        assert!(matches!(eval_expr(&expr), Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn nth_mf_unrepresentable_float_positions_are_absent() {
        // Host-created values must not overflow an integer conversion. These
        // safety cases make no claim about CLIPS's out-of-range C casts.
        for index in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.0_f64.powi(63)] {
            let mf = mf_lit(vec![Value::Integer(10)]);
            let expr = call("nth$", vec![float(index), mf]);
            assert!(matches!(eval_expr(&expr), Ok(Value::Symbol(_))));
        }
    }

    #[test]
    fn nth_mf_type_error_non_multifield_second_arg() {
        let expr = call("nth$", vec![int(1), int(99)]);
        let result = eval_expr(&expr);
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
    }

    #[test]
    fn nth_alias_matches_nth_mf_behavior() {
        let mf = mf_lit(vec![
            Value::Integer(10),
            Value::Integer(20),
            Value::Integer(30),
        ]);
        let expr = call("nth", vec![int(2), mf]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(20)));
    }

    #[test]
    fn nth_alias_arity_error() {
        let result = eval_expr(&call("nth", vec![int(1)]));
        assert!(matches!(result, Err(EvalError::ArityMismatch { .. })));
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
        let (mut engine, vm, bs) = test_ctx();
        let mf = mf_lit(vec![Value::Integer(10), Value::Integer(20)]);
        let expr = call("member$", vec![int(99), mf]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn member_mf_empty_multifield_returns_false() {
        let (mut engine, vm, bs) = test_ctx();
        let mf = mf_lit(vec![]);
        let expr = call("member$", vec![int(1), mf]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
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
    fn member_alias_found_returns_index() {
        let mf = mf_lit(vec![Value::Integer(3), Value::Integer(4)]);
        let expr = call("member", vec![int(4), mf]);
        let result = eval_expr(&expr).unwrap();
        assert!(result.structural_eq(&Value::Integer(2)));
    }

    #[test]
    fn member_alias_not_found_returns_false() {
        let (mut engine, vm, bs) = test_ctx();
        let mf = mf_lit(vec![Value::Integer(3), Value::Integer(4)]);
        let expr = call("member", vec![int(9), mf]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn subsetp_true_when_subset() {
        // (subsetp (create$ 1 2) (create$ 1 2 3)) => TRUE
        let (mut engine, vm, bs) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1), Value::Integer(2)]);
        let mf2 = mf_lit(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn subsetp_true_when_equal_sets() {
        // (subsetp (create$ 1 2) (create$ 1 2)) => TRUE
        let (mut engine, vm, bs) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1), Value::Integer(2)]);
        let mf2 = mf_lit(vec![Value::Integer(1), Value::Integer(2)]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn subsetp_true_for_empty_subset() {
        // (subsetp (create$) (create$ 1 2 3)) => TRUE (empty set is subset of any)
        let (mut engine, vm, bs) = test_ctx();
        let mf1 = mf_lit(vec![]);
        let mf2 = mf_lit(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn subsetp_true_for_both_empty() {
        // (subsetp (create$) (create$)) => TRUE
        let (mut engine, vm, bs) = test_ctx();
        let mf1 = mf_lit(vec![]);
        let mf2 = mf_lit(vec![]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn subsetp_false_when_not_subset() {
        // (subsetp (create$ 1 4) (create$ 1 2 3)) => FALSE (4 not in second)
        let (mut engine, vm, bs) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1), Value::Integer(4)]);
        let mf2 = mf_lit(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
    }

    #[test]
    fn subsetp_false_when_superset_is_empty() {
        // (subsetp (create$ 1) (create$)) => FALSE (1 not in empty)
        let (mut engine, vm, bs) = test_ctx();
        let mf1 = mf_lit(vec![Value::Integer(1)]);
        let mf2 = mf_lit(vec![]);
        let expr = call("subsetp", vec![mf1, mf2]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_false_symbol(&result, &ctx.engine.symbol_table));
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
        let (mut engine, vm, bs) = test_ctx();
        let inner_expr = call("create$", vec![int(1), int(2)]);
        let expr = call("multifieldp", vec![inner_expr]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(is_true_symbol(&result, &ctx.engine.symbol_table));
    }

    // -----------------------------------------------------------------------
    // format tests
    // -----------------------------------------------------------------------

    fn logical_name_forms(name: &str) -> [RuntimeExpr; 3] {
        let symbol = call("sym-cat", vec![str_lit(name)]);
        [
            str_lit(name),
            symbol.clone(),
            call("symbol-to-instance-name", vec![symbol]),
        ]
    }

    #[test]
    fn format_routes_and_returns_the_same_completed_string() {
        for name in ["t", "stdout", "wtrace", "application"] {
            for channel in logical_name_forms(name) {
                let (result, events) = eval_expr_with_output(&call(
                    "format",
                    vec![channel, str_lit("value=%d%n"), int(42)],
                ));
                let Value::String(value) = result.unwrap() else {
                    panic!("format must return a string");
                };
                assert_eq!(value.as_str(), "value=42\n");
                assert_eq!(events, [(name.to_owned(), "value=42\n".to_owned())]);
            }
        }
        for (channel, expected) in [(int(23), "23"), (float(2.5), "2.5")] {
            let (result, events) =
                eval_expr_with_output(&call("format", vec![channel, str_lit("number")]));
            assert!(result.is_ok());
            assert_eq!(events, [(expected.to_owned(), "number".to_owned())]);
        }
    }

    #[test]
    fn nil_format_evaluates_operands_without_routing_its_result() {
        for channel in logical_name_forms("nil") {
            let (result, events) = eval_expr_with_output(&call(
                "format",
                vec![
                    channel,
                    str_lit("outer:%s"),
                    call("format", vec![str_lit("t"), str_lit("inner")]),
                ],
            ));
            let Value::String(value) = result.unwrap() else {
                panic!("format nil must return a string");
            };
            assert_eq!(value.as_str(), "outer:inner");
            assert_eq!(events, [("t".to_owned(), "inner".to_owned())]);
        }
    }

    #[test]
    fn nil_printout_does_not_evaluate_operands() {
        for channel in logical_name_forms("nil") {
            let (result, events) = eval_expr_with_output(&call(
                "printout",
                vec![
                    channel,
                    call("format", vec![str_lit("t"), str_lit("unreached")]),
                    call("+", vec![int(1), str_lit("invalid")]),
                ],
            ));
            assert!(matches!(result, Ok(Value::Void)));
            assert!(events.is_empty());
        }
    }

    #[test]
    fn printout_preserves_nested_format_output_order() {
        let (result, events) = eval_expr_with_output(&call(
            "printout",
            vec![
                str_lit("t"),
                str_lit("a "),
                call("format", vec![str_lit("t"), str_lit("b%n")]),
                str_lit("c\n"),
            ],
        ));
        assert!(matches!(result, Ok(Value::Void)));
        assert!(events.iter().all(|(channel, _)| channel == "t"));
        let output: String = events.into_iter().map(|(_, text)| text).collect();
        assert_eq!(output, "a b\nb\nc\n");
    }

    #[test]
    fn printout_preserves_prefix_and_nested_output_when_a_later_operand_fails() {
        let (result, events) = eval_expr_with_output(&call(
            "printout",
            vec![
                str_lit("t"),
                str_lit("prefix "),
                call(
                    "+",
                    vec![
                        int(1),
                        call("format", vec![str_lit("t"), str_lit("nested")]),
                    ],
                ),
                str_lit("unreached"),
            ],
        ));
        assert!(matches!(result, Err(EvalError::TypeError { .. })));
        assert_eq!(
            events,
            [
                ("t".to_owned(), "prefix ".to_owned()),
                ("t".to_owned(), "nested".to_owned()),
            ]
        );
    }

    #[test]
    fn failed_format_keeps_operand_side_effects_but_writes_no_partial_result() {
        let (result, events) = eval_expr_with_output(&call(
            "format",
            vec![
                str_lit("t"),
                str_lit("prefix:%s:%d"),
                call("format", vec![str_lit("t"), str_lit("nested")]),
                str_lit("invalid"),
            ],
        ));
        assert!(matches!(
            result,
            Err(EvalError::TypeError { function, .. }) if function == "format"
        ));
        assert_eq!(events, [("t".to_owned(), "nested".to_owned())]);
    }

    #[test]
    fn format_validates_channel_control_and_arity_before_operand_side_effects() {
        let operand = call("format", vec![str_lit("t"), str_lit("unreached")]);
        for (channel, control) in [
            (RuntimeExpr::Literal(Value::Void), "%s"),
            (str_lit("t"), "%q"),
            (str_lit("t"), "%s %s"),
        ] {
            let (result, events) = eval_expr_with_output(&call(
                "format",
                vec![channel, str_lit(control), operand.clone()],
            ));
            let error = result.unwrap_err();
            assert!(error.to_string().contains("format"), "{error}");
            assert!(events.is_empty());
        }
    }

    #[test]
    fn test_format_basic_string() {
        // (format nil "hello %s" "world") => "hello world"
        let (mut engine, vm, bs) = test_ctx();
        let nil_sym = RuntimeExpr::Literal(Value::Symbol(
            engine
                .symbol_table
                .intern_symbol("nil", ferric_rules_core::StringEncoding::Utf8)
                .unwrap(),
        ));
        let fmt = RuntimeExpr::Literal(Value::String(
            FerricString::new("hello %s", ferric_rules_core::StringEncoding::Utf8).unwrap(),
        ));
        let arg = RuntimeExpr::Literal(Value::String(
            FerricString::new("world", ferric_rules_core::StringEncoding::Utf8).unwrap(),
        ));
        let expr = call("format", vec![nil_sym, fmt, arg]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("count: %d", ferric_rules_core::StringEncoding::Utf8)
                        .unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("val: %f", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%.2f", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%10d", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%-10d", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("a%nb", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("100%%", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%s is %d", ferric_rules_core::StringEncoding::Utf8).unwrap(),
                )),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("age", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
                str_lit("nil"),
                RuntimeExpr::Literal(Value::String(
                    FerricString::new("%e", ferric_rules_core::StringEncoding::Utf8).unwrap(),
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
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        engine.input_buffer.push_back("42".to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_read_float() {
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        engine.input_buffer.push_back("2.5".to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        assert!(matches!(result, Value::Float(f) if (f - 2.5).abs() < 1e-10));
    }

    #[test]
    fn test_read_symbol() {
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        engine.input_buffer.push_back("hello".to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::Symbol(sym) = result else {
            panic!("expected Symbol");
        };
        assert_eq!(
            ctx.engine.symbol_table.resolve_symbol_str(sym),
            Some("hello")
        );
    }

    #[test]
    fn test_read_eof_empty_buffer() {
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::Symbol(sym) = result else {
            panic!("expected Symbol(EOF)");
        };
        assert_eq!(ctx.engine.symbol_table.resolve_symbol_str(sym), Some("EOF"));
    }

    #[test]
    fn test_read_quoted_string() {
        // push `"hello"` (with actual quotes) → String("hello")
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        engine.input_buffer.push_back(r#""hello""#.to_string());
        let expr = call("read", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        engine.input_buffer.push_back("hello world".to_string());
        let expr = call("readline", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::String(s) = result else {
            panic!("expected String");
        };
        assert_eq!(s.as_str(), "hello world");
    }

    #[test]
    fn test_readline_eof_empty() {
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        let expr = call("readline", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        let Value::Symbol(sym) = result else {
            panic!("expected Symbol(EOF)");
        };
        assert_eq!(ctx.engine.symbol_table.resolve_symbol_str(sym), Some("EOF"));
    }

    #[test]
    fn test_readline_multiple_lines() {
        let (mut engine, vm, bs) = test_ctx();
        engine.input_buffer = std::collections::VecDeque::new();
        engine.input_buffer.push_back("first line".to_string());
        engine.input_buffer.push_back("second line".to_string());
        let expr = call("readline", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: engine.module_registry.main_module_id(),
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
    fn dummy_span() -> ferric_rules_parser::Span {
        ferric_rules_parser::Span::new(
            ferric_rules_parser::Position {
                offset: 0,
                line: 1,
                column: 1,
            },
            ferric_rules_parser::Position {
                offset: 1,
                line: 1,
                column: 2,
            },
            ferric_rules_parser::FileId(0),
        )
    }

    // -------------------------------------------------------------------
    // Agenda/focus query builtins
    // -------------------------------------------------------------------

    #[test]
    fn test_get_focus_returns_current_module() {
        // Default focus is MAIN
        let (mut engine, vm, bs) = test_ctx();
        let main_id = engine.module_registry.main_module_id();
        let expr = call("get-focus", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: main_id,
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
        };
        let result = eval(&mut ctx, &expr).unwrap();
        match result {
            Value::Symbol(sym) => {
                let name = ctx.engine.symbol_table.resolve_symbol_str(sym).unwrap();
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
        let (mut engine, vm, bs) = test_ctx();
        let main_id = engine.module_registry.main_module_id();
        let expr = call("get-focus-stack", vec![]);
        let mut ctx = EvalContext {
            global_module: None,
            bindings: &bs,
            var_map: &vm,
            callable_locals: None,
            call_depth: 0,
            expression_depth: 0,
            current_module: main_id,
            method_chain: None,
            compact_fact_bindings: None,
            engine: &mut engine,
            allow_engine_effects: true,
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
                        let name = ctx.engine.symbol_table.resolve_symbol_str(*sym).unwrap();
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

    // -------------------------------------------------------------------
    // Property-based tests
    // -------------------------------------------------------------------

    use proptest::prelude::*;

    /// Strategy for arbitrary literal values that are safe for `structural_eq`
    /// comparison (excludes NaN floats and uses a bounded float range to avoid
    /// precision surprises).
    fn arb_literal_value() -> impl Strategy<Value = Value> {
        prop_oneof![
            any::<i64>().prop_map(Value::Integer),
            // Bounded range avoids NaN/infinity, which break structural_eq
            (-1e10_f64..1e10_f64).prop_map(Value::Float),
            Just(Value::Void),
        ]
    }

    proptest! {
        /// Invariant: evaluating a Literal expression is the identity function —
        /// the returned value must be structurally equal to the original value.
        #[test]
        fn literal_evaluation_is_identity(v in arb_literal_value()) {
            let expr = RuntimeExpr::Literal(v.clone());
            let result = eval_expr(&expr).unwrap();
            // Postcondition: eval(Literal(v)) == v
            prop_assert!(
                result.structural_eq(&v),
                "expected {:?} but got {:?}",
                v,
                result
            );
        }
    }

    proptest! {
        /// Invariant: integer addition is commutative.
        /// (+ a b) must equal (+ b a) for all integers.
        #[test]
        fn integer_addition_commutative(a in -1000_i64..1000, b in -1000_i64..1000) {
            let result_ab = eval_expr(&call("+", vec![int(a), int(b)])).unwrap();
            let result_ba = eval_expr(&call("+", vec![int(b), int(a)])).unwrap();
            // Postcondition: addition order does not affect result
            prop_assert!(
                result_ab.structural_eq(&result_ba),
                "(+ {} {}) = {:?} but (+ {} {}) = {:?}",
                a, b, result_ab, b, a, result_ba
            );
        }
    }

    proptest! {
        /// Invariant: integer addition is associative.
        /// (+ (+ a b) c) must equal (+ a (+ b c)).
        #[test]
        fn integer_addition_associative(
            a in -1000_i64..1000,
            b in -1000_i64..1000,
            c in -1000_i64..1000,
        ) {
            let ab = call("+", vec![int(a), int(b)]);
            let result_left = eval_expr(&call("+", vec![ab, int(c)])).unwrap();

            let bc = call("+", vec![int(b), int(c)]);
            let result_right = eval_expr(&call("+", vec![int(a), bc])).unwrap();

            // Postcondition: grouping does not affect the sum
            prop_assert!(
                result_left.structural_eq(&result_right),
                "(+ (+ {} {}) {}) = {:?} but (+ {} (+ {} {})) = {:?}",
                a, b, c, result_left, a, b, c, result_right
            );
        }
    }

    proptest! {
        /// Invariant: integer multiplication is commutative.
        /// (* a b) must equal (* b a) for all integers.
        #[test]
        fn integer_multiplication_commutative(a in -1000_i64..1000, b in -1000_i64..1000) {
            let result_ab = eval_expr(&call("*", vec![int(a), int(b)])).unwrap();
            let result_ba = eval_expr(&call("*", vec![int(b), int(a)])).unwrap();
            // Postcondition: multiplication order does not affect result
            prop_assert!(
                result_ab.structural_eq(&result_ba),
                "(* {} {}) = {:?} but (* {} {}) = {:?}",
                a, b, result_ab, b, a, result_ba
            );
        }
    }

    proptest! {
        /// Invariant: subtraction is the inverse of addition.
        /// (- (+ n k) k) must equal n for all integers.
        #[test]
        fn subtraction_is_addition_inverse(n in -10_000_i64..10_000, k in -10_000_i64..10_000) {
            let sum = call("+", vec![int(n), int(k)]);
            let result = eval_expr(&call("-", vec![sum, int(k)])).unwrap();
            // Postcondition: (- (+ n k) k) == n
            prop_assert!(
                result.structural_eq(&Value::Integer(n)),
                "(- (+ {} {}) {}) = {:?}, expected {}",
                n, k, k, result, n
            );
        }
    }

    proptest! {
        /// Invariant: integer negation is self-inverse.
        /// (- 0 (- 0 n)) must equal n, i.e. negating twice yields the original.
        #[test]
        fn integer_negation_double_inverse(n in -100_000_i64..100_000) {
            let neg_n = call("-", vec![int(0), int(n)]);
            let result = eval_expr(&call("-", vec![int(0), neg_n])).unwrap();
            // Postcondition: double negation is identity
            prop_assert!(
                result.structural_eq(&Value::Integer(n)),
                "(- 0 (- 0 {})) = {:?}, expected {}",
                n, result, n
            );
        }
    }

    proptest! {
        /// Invariant: is_truthy follows CLIPS semantics.
        /// Only Void and the FALSE symbol are falsy; all other values are truthy.
        /// Notably, integer 0 and float 0.0 ARE truthy in CLIPS (unlike many languages).
        #[test]
        fn truthiness_consistency(n in any::<i64>(), f in -1e10_f64..1e10_f64) {
            let (engine, ..) = test_ctx();
            // Integer invariant: all integers (including 0) are truthy in CLIPS
            prop_assert!(
                is_truthy(&Value::Integer(n), &engine.symbol_table),
                "integer {} should be truthy",
                n
            );
            // Float invariant: all finite floats (including 0.0) are truthy in CLIPS
            prop_assert!(
                is_truthy(&Value::Float(f), &engine.symbol_table),
                "float {} should be truthy",
                f
            );
            // Void is always falsy
            prop_assert!(
                !is_truthy(&Value::Void, &engine.symbol_table),
                "Void should be falsy"
            );
        }
    }

    proptest! {
        /// Invariant: division by zero always returns DivisionByZero error.
        /// This applies to both integer and float numerators with a zero divisor.
        #[test]
        fn division_by_zero_returns_error(
            n in -10_000_i64..10_000,
            fnum in -1e10_f64..1e10_f64,
        ) {
            // Integer numerator, integer zero denominator
            let result_int = eval_expr(&call("/", vec![int(n), int(0)]));
            // Postcondition: dividing by 0 is an error
            prop_assert!(
                matches!(result_int, Err(EvalError::DivisionByZero { .. })),
                "(/ {} 0) should be DivisionByZero, got {:?}",
                n, result_int
            );

            // Float numerator, float zero denominator (rhs == 0.0 check in impl)
            let result_flt = eval_expr(&call("/", vec![float(fnum), float(0.0)]));
            prop_assert!(
                matches!(result_flt, Err(EvalError::DivisionByZero { .. })),
                "(/ {} 0.0) should be DivisionByZero, got {:?}",
                fnum, result_flt
            );
        }
    }

    proptest! {
        /// Invariant: abs always returns a non-negative integer result for integer inputs,
        /// except for i64::MIN which cannot be represented as a positive i64.
        #[test]
        fn abs_integer_is_non_negative(n in i64::MIN + 1..=i64::MAX) {
            let result = eval_expr(&call("abs", vec![int(n)])).unwrap();
            if let Value::Integer(v) = result {
                // Postcondition: abs(n) >= 0 for all representable n
                prop_assert!(v >= 0, "abs({}) = {} which is negative", n, v);
            } else {
                return Err(TestCaseError::fail(format!(
                    "expected Integer from abs({n}), got {result:?}"
                )));
            }
        }
    }

    proptest! {
        /// Invariant: abs always returns a non-negative float result for float inputs.
        #[test]
        fn abs_float_is_non_negative(f in -1e10_f64..1e10_f64) {
            let result = eval_expr(&call("abs", vec![float(f)])).unwrap();
            if let Value::Float(v) = result {
                // Postcondition: abs(f) >= 0.0 for all finite floats
                prop_assert!(v >= 0.0, "abs({}) = {} which is negative", f, v);
            } else {
                return Err(TestCaseError::fail(format!(
                    "expected Float from abs({f}), got {result:?}"
                )));
            }
        }
    }

    proptest! {
        /// Invariant: max(a, b) >= min(a, b) for any two integers.
        /// This is the defining relationship between max and min.
        #[test]
        fn max_min_consistency(a in -1_000_000_i64..1_000_000, b in -1_000_000_i64..1_000_000) {
            let max_result = eval_expr(&call("max", vec![int(a), int(b)])).unwrap();
            let min_result = eval_expr(&call("min", vec![int(a), int(b)])).unwrap();

            if let (Value::Integer(max_v), Value::Integer(min_v)) = (&max_result, &min_result) {
                // Postcondition: max >= min always holds
                prop_assert!(
                    max_v >= min_v,
                    "max({}, {}) = {} < min({}, {}) = {}",
                    a, b, max_v, a, b, min_v
                );
            } else {
                return Err(TestCaseError::fail(format!(
                    "expected Integers, got max={max_result:?} min={min_result:?}"
                )));
            }
        }
    }
}

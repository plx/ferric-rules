# Phase 4 Pass Implementation Notes

## Pass 001: Baseline and Harness Alignment

### RegisteredTemplate Field Structure
`RegisteredTemplate` (in `crates/ferric-runtime/src/templates.rs`) has:
- `name: String` — template name
- `slot_names: Vec<String>` — slot names in declaration order (NOT `slots`)
- `slot_index: HashMap<String, usize>` — name → positional index
- `defaults: Vec<Value>` — default values by position

`TemplateFact` (in `ferric-core/src/fact.rs`) has:
- `template_id: TemplateId`
- `slots: Box<[Value]>` — positional array, NOT a HashMap

To iterate template slots with names:
```rust
for name in &def.slot_names {
    if let Some(&idx) = def.slot_index.get(name) {
        if let Some(val) = tf.slots.get(idx) {
            // val is &Value at this slot position
        }
    }
}
```

### Clippy: needless_raw_string_hashes
`r#"..."#` triggers clippy warning if the string contains no `"` characters.
Use bare `r"..."` for raw strings that don't need hash escaping.
This applies to `format!(r#"..."#)` calls too.

### Fixture Files in Both Locations
Phase 4 fixture files (like all fixture files) must exist in BOTH:
- `tests/fixtures/` (workspace root)
- `crates/ferric-runtime/tests/fixtures/` (crate root)

`cargo test` sets CWD to crate root, so `load_fixture` uses the crate-relative path.
Use `cp` to keep both in sync after creating a fixture.

## Pass 004: Module-Qualified Callable And Global Lookup Diagnostics

### Changes Made
All changes were in `crates/ferric-runtime/src/evaluator.rs`:
1. In `eval()` `RuntimeExpr::Call` arm: added `if name.contains("::")` check at the top that delegates to `dispatch_qualified_call()` and returns early (bypasses builtins).
2. In `eval()` `RuntimeExpr::GlobalVar` arm: added `if name.contains("::")` check at the top that delegates to `resolve_qualified_global()`.
3. Added `dispatch_qualified_call()` function (between the eval function and dispatch_user_function).
4. Added `resolve_qualified_global()` function alongside dispatch_qualified_call.

### Key Design Decision: No Fallback to Builtins
Qualified calls intentionally skip `dispatch_builtin`. `MAIN::+` returns `UnknownFunction`, not the `+` builtin result. This is enforced by the early-return before `dispatch_builtin`.

### Error Mapping
- Unknown module → `EvalError::TypeError` (function/expected/actual fields)
- Function found but belongs to a different module than stated → `EvalError::UnknownFunction`
- Function found, right module, but not visible → `EvalError::NotVisible`
- No function/generic with that local name at all → `EvalError::UnknownFunction`

### Test Count After Pass 004
840 total (1 ferric facade + 271 ferric-core + 174 ferric-parser + 394 ferric-runtime).

## Pass 007: call-next-method Dispatch Chain

### MethodChain struct
Added to `evaluator.rs` near `EvalContext` definition. Contains:
- `generic_name: String`
- `applicable_methods: Vec<RegisteredMethod>` (sorted most-specific-first)
- `current_index: usize` (index of currently executing method)
- `arg_values: Vec<Value>` (original arg values for rebinding next method)

### EvalContext.method_chain field
- Type: `Option<MethodChain>`
- Set to `None` for: test sites, `dispatch_user_function`, `actions.rs`, `loader.rs`
- Set to `Some(chain)` in `dispatch_generic` for the first method
- Updated to `Some(next_chain)` in `dispatch_call_next_method` with `current_index` incremented

### Pattern for updating all EvalContext sites
When adding a new field to `EvalContext`, use `grep -c "field_pattern"` to confirm the count,
then use `replace_all` with the last common field + closing `};` pattern to update all test sites
at once. For `evaluator.rs` tests, every test EvalContext ends with `generic_modules: &em,\n        };`.

### Clippy: redundant_closure in sort_by
`applicable.sort_by(|a, b| compare_method_specificity(a, b))` triggers `redundant_closure`.
Use `applicable.sort_by(compare_method_specificity)` instead.

### dispatch_call_next_method placement
Add between `dispatch_generic` and `bind_callable_arguments`. Takes `&mut EvalContext<'_>`,
`&[RuntimeExpr]`, and `Option<SourceSpan>`. Reuses `bind_callable_arguments` and `from_action_expr`.

### Test Count After Pass 007
871 total (1 ferric facade + 271 ferric-core + 174 ferric-parser + 422 ferric-runtime + 3 doctests).

## Pass 010: Multifield Function Surface

### Multifield API
`Multifield` in ferric-core has: `new()`, `push()`, `len()`, `is_empty()`, `iter()`, `iter_mut()`.
It implements `Deref<Target=[Value]>` so indexing with `mf[i]` works directly.
It implements `FromIterator<Value>` and `Extend<Value>`.

### `let...else` required for Multifield extraction
When extracting `Value::Multifield(mf)` from a value with error fallback, clippy
enforces `let...else` style:
```rust
let Value::Multifield(mf) = &values[1] else {
    return Err(EvalError::TypeError { ... });
};
```

### Safe index conversion for 1-based indexing
For `nth$` (or any 1-based index check), use `usize::try_from` + `filter`:
```rust
let idx = usize::try_from(index - 1).ok().filter(|&i| i < mf.len());
let Some(idx) = idx else {
    return Err(...);
};
```
This avoids both `cast_sign_loss` and `cast_possible_truncation` warnings.
Note: `usize::try_from(index - 1)` handles: negative index (fails), zero index (fails as -1),
positive out-of-range (caught by filter).

### `create$` flattening
`create$` flattens nested multifields: `(create$ 1 (create$ 2 3) 4)` → `[1, 2, 3, 4]`.
Use `mf.iter().cloned()` to iterate-and-clone elements for appending.

### `member$` return convention
Returns the 1-based index as `Value::Integer` when found, or `clips_bool(false, ...)` when not.
NOT a boolean TRUE — returns the position integer.

### Test count after Pass 010
527 ferric-runtime tests (1 ferric + 271 ferric-core + 174 ferric-parser + 527 ferric-runtime + 3 doctests = 976 total).

## Pass 012: Agenda and Focus Query Function Surface

### New builtins (evaluator.rs)
- `get-focus`: returns current focus module as Symbol; uses `ctx.module_registry.current_focus()` + `module_name()`
- `get-focus-stack`: returns focus stack as `Value::Multifield(Box::new(Multifield))` top-first; use `focus_stack().iter().rev()`

### New actions (actions.rs)
- `list-focus-stack`: prints focus stack to "t" channel, one module per line
- `agenda`: iterates `rete.agenda.iter_activations()` + `all_rule_info` map for rule names
- `run`: no-op from RHS

### `all_rule_info` threading
Added `all_rule_info: &HashMap<RuleId, CompiledRuleInfo>` parameter to both `execute_actions` and `execute_single_action`.
In `engine.rs`, must clone `info = self.rule_info.get(&rule_id).cloned()` so that `&self.rule_info` can be passed as `all_rule_info` without double-borrow.

### Clippy
- `execute_list_focus_stack` and `execute_agenda` are private helpers that always return `Ok(())` → add `#[allow(clippy::unnecessary_wraps)]` (they need the signature for consistent match arm types)
- `.map(|x| x.field).unwrap_or(default)` → use `.map_or(default, |x| x.field)`

### Value::Multifield construction
`Value::Multifield` takes `Box<Multifield>` (NOT `Box<[Value]>` or `Vec<Value>`).
Pattern: `let mut mf = ferric_core::value::Multifield::new(); mf.push(val); Ok(Value::Multifield(Box::new(mf)))`

### Cross-module test limitations
Tests involving `(defmodule X)` + module-qualified deffacts may not work as expected because the loader changes current_module after `defmodule`. Prefer:
1. Explicit `MAIN::rule-name` qualification for rules defined before module switch
2. Use `(deffacts startup ...)` AFTER the final defmodule for SENSOR, OR
3. Use the "rule/fact/deffacts in correct order" pattern matching the phase3_defmodule fixture (MAIN rules first, then defmodule, then defmodule's rules/deffacts)

### Test count after Pass 012
569 ferric-runtime tests (total 1018 including 3 doctests + ferric-core + ferric-parser).

## if/then/else Control Flow Implementation

### Key design: action wrapping
- `interpret_action()` intercepts `(if ...)` BEFORE `interpret_function_call()`
- Result: `Action { call: FunctionCall { name: "if", args: [ActionExpr::If{...}], span } }`
- The `ActionExpr::If` is the SOLE arg — NOT the `(> ?x 0) then ...` elements broken into separate args

### RuntimeExpr::If branch type
- Branches are `Vec<(ActionExpr, Option<Box<RuntimeExpr>>)>` — NOT `Vec<RuntimeExpr>`
- Reason: `execute_single_action` for assert/retract/printout needs original `ActionExpr` args
- Pre-compiled `RuntimeExpr` stored as `Box` to avoid size issues

### Two `if_runtime` extraction paths in execute_single_action
1. **Top-level** (loader pre-compilation): `runtime_call = Some(RuntimeExpr::Call { name: "if", args: [RuntimeExpr::If{...}] })` — extract `args.first()`
2. **Nested in branch** (recursive call): `runtime_call = Some(RuntimeExpr::If{...})` directly
- CRITICAL: Without the nested path, nested `if` silently does nothing

### Branch dispatch
- `ActionExpr::FunctionCall(fc)` → use `fc.clone()` as call, `rt_expr.as_deref()` as runtime_call
- `ActionExpr::If { span, .. }` → synthetic `FunctionCall { name: "if", args: [], span }` + `rt_expr.as_deref()` directly as runtime_call (will be `RuntimeExpr::If` for recursive dispatch)
- Always propagate early-exit flags (`halted`, `reset_requested`, `clear_requested`) after each branch item

### Test count after if/then/else
618 ferric-runtime tests (added 5 parser unit tests + 10 integration tests).

## Loop Control Forms: while, loop-for-count, progn$/foreach

### Parser design (stage2.rs)
- Three new `ActionExpr` variants: `While`, `LoopForCount`, `Progn`
- `foreach` parses to `ActionExpr::Progn` (same semantics, different syntax)
- `interpret_action()` intercepts all 5 keywords before `interpret_function_call()`
- Result: `Action { call: FunctionCall { name: "while"|"loop-for-count"|"progn$"|"foreach", args: [ActionExpr::While|LoopForCount|Progn{...}], span } }`
- `interpret_action_expr()` also intercepts loops for nested usage (e.g., in deffunction body)

### while syntax
- `(while <cond> do <action>*)` — `do` is a plain symbol delimiter (not a token type)
- Condition must be the single element between `while` and `do`

### loop-for-count syntax
- `(?var start end)` — 3-element spec
- `(?var end)` — 2-element spec (start defaults to 1)
- `(end)` — 1-element anonymous counter spec
- `do` is required delimiter after the spec

### progn$/foreach syntax
- `progn$`: `(progn$ (?var <expr>) <action>*)` — no `do` delimiter
- `foreach`: `(foreach ?var <expr> do <action>*)` — optional `do` (3rd element check)

### RuntimeExpr variants
- `While { condition, body, span }` — body is `Vec<(ActionExpr, Option<Box<RuntimeExpr>>)>`
- `LoopForCount { var_name: Option<String>, start, end, body, span }`
- `Progn { var_name: String, list_expr, body, span }`
- `from_action_expr()` needs `#[allow(clippy::too_many_lines)]` (128+ lines)

### Loop evaluation in eval() — for pure expression contexts
- `while`: `MAX_LOOP_ITERATIONS` cap; returns last body value or `FALSE`
- `loop-for-count`: evaluates start/end, clones `var_map`+`bindings` per iteration via `EvalContext`
- `progn$`: evaluates `list_expr`, iterates elements, binds `var_name` and `var_name-index`
- Create new `EvalContext` per iteration with cloned `var_map` and `BindingSet`
- `f64 as i64` needs `#[allow(clippy::cast_possible_truncation)]`
- `(end_int - start_int + 1) as usize` needs `#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]`
- `idx as i64` needs `#[allow(clippy::cast_possible_wrap)]`
- Use `body` not `body.iter()` in for loops (clippy: `explicit_iter_loop`)

### Loop execution in execute_single_action() — for side-effecting action contexts
- Three helpers: `execute_loop_body`, `augment_bindings_with_var`, `rule_info_clone_light`
- `augment_bindings_with_var`: clones `token` and `rule_info.var_map`, adds new variable binding
- `rule_info_clone_light`: clones only `var_map`, `fact_address_vars`, `name`, `salience` — leaves actions/test_conditions empty
- `execute_loop_body`: shared body dispatch (mirrors if branch dispatch) — handles all `ActionExpr` variants
- The `else { if ... }` pattern for fallbacks → clippy `collapsible_else_if` → use `else if`

### Variable binding for loops via augmented Token
- `loop-for-count` and `progn$` use `augment_bindings_with_var(token, rule_info, var, value, ...)`
- Returns `(Token, CompiledRuleInfo)` — both cloned with the extra variable added
- `progn$` calls it twice: once for element, once for `var-index`
- `while` uses the original token/rule_info unchanged (no new variable)

### Test considerations
- `$?xs` multifield LHS patterns NOT yet supported — use `(create$ a b c)` in RHS instead
- `defglobal` must be declared for accumulator variables in deffunction bodies

### Test count after loops
199 ferric-parser tests (10 new), 632 ferric-runtime tests (14 new).

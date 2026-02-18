# Rust Code Writer Memory

## Project Structure: ferric-rules

This is a Cargo workspace with four crates:
- `ferric` — facade crate that re-exports from the others
- `ferric-core` — shared types, fact storage, pattern matching (core engine internals)
- `ferric-parser` — parser (Stage 1 S-expression parser complete, Stage 2 construct interpreter added)
- `ferric-runtime` — Engine, EngineConfig, execution environment, and source loader

## Key Architectural Decisions

### Type Ownership and Dependencies
- **Shared primitive types belong in `ferric-core`**: Value, Symbol, FerricString, StringEncoding, EncodingError
- **Engine-level types belong in `ferric-runtime`**: Engine, EngineConfig, EngineError
- This avoids circular dependencies (runtime depends on core, not vice versa)

### Symbol Interning Pattern
**CRITICAL**: When testing with symbols, always use a single SymbolTable instance across all symbols in a test.
```rust
// WRONG - each call gets SymbolId::Ascii(0), all symbols appear equal!
fn test_symbol(s: &str) -> Symbol {
    let mut table = SymbolTable::new();
    table.intern_symbol(s, StringEncoding::Ascii).unwrap()
}

// RIGHT - reuse the same table
let mut table = SymbolTable::new();
let x = table.intern_symbol("x", StringEncoding::Ascii).unwrap();
let y = table.intern_symbol("y", StringEncoding::Ascii).unwrap();
// x and y are now distinct symbols
```

### SlotMap Key Creation in Tests
When testing with `slotmap` keys like `TemplateId`, `FactId`, `TokenId`, or `ActivationId`, avoid using `::default()` repeatedly:
```rust
// WRONG - both get the same default key
let t1 = TemplateId::default();
let t2 = TemplateId::default();

// RIGHT - create distinct keys using a temporary SlotMap
let mut temp: SlotMap<TemplateId, ()> = SlotMap::with_key();
let t1 = temp.insert(());
let t2 = temp.insert(());
```

### BindingSet Requirements
`BindingSet` requires `Clone` and `Debug` derives (added in Pass 006 for Token storage).

### Value Equality in Tests
`Value` intentionally does NOT implement `PartialEq` (due to float semantics). In tests:
- Use `structural_eq()` for value comparisons
- Use `matches!` pattern matching instead of `assert_eq!`
- For optional values, use `.is_none()` or `.is_some()` instead of comparing to None

### SmallVec and Value
`Value` does not implement `Copy`, so avoid using `SmallVec::from_slice()` which requires `Copy`. Instead:
```rust
// WRONG - Value doesn't implement Copy
let fields = SmallVec::from_slice(&[Value::Integer(42)]);

// RIGHT - push individually or clone
let mut fields = SmallVec::new();
fields.push(Value::Integer(42));
```

### SExpr Type (ferric-parser)
`SExpr` does not implement `Copy`. When building raw S-expression vectors:
```rust
// Need to clone when creating vectors from slices
let lhs_raw = elements[idx..arrow_idx].to_vec();  // calls clone on each element
```

## Clippy Fixes

### Derivable Impls
When a Default impl just sets all fields to their defaults (e.g., `bool` to `false`), use `#[derive(Default)]` instead of manual impl:
```rust
// PREFERRED
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub strict: bool,  // defaults to false
}

// NOT - clippy::derivable_impls
impl Default for Config {
    fn default() -> Self { Self { strict: false } }
}
```

### Integer Truncation
When intentionally truncating `i64` to `i32` (e.g., salience values), use `#[allow(clippy::cast_possible_truncation)]`:
```rust
#[allow(clippy::cast_possible_truncation)]
{
    salience = *sal as i32;
}
```

### Boolean to Int Conversion
Prefer `usize::from(boolean)` over if-else for 0/1 conversion:
```rust
// PREFERRED
let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);

// NOT
let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
```

## Phase 3 Patterns

See `phase3-pass-notes.md` for detailed notes on each Phase 3 pass.

### Pass 003: Template Registry (Template-Aware Modify/Duplicate)
- `RegisteredTemplate` lives in `crates/ferric-runtime/src/templates.rs` (pub(crate))
- `Engine` gained: `template_ids: HashMap<String, TemplateId>`, `template_defs: HashMap<TemplateId, RegisteredTemplate>`, `template_id_alloc: slotmap::SlotMap<TemplateId, ()>`
- Must add `slotmap = { workspace = true }` to `ferric-runtime`'s Cargo.toml
- Template registration happens in `load_str` at `Construct::Template`, **before** rules are compiled
- `execute_actions` takes 8 args — needs `#[allow(clippy::too_many_arguments)]`
- `apply_template_slot_overrides` takes `&mut [Value]` not `&mut Vec<Value>` (clippy::ptr_arg)
- `translate_pattern` with template arm is >100 lines — `#[allow(clippy::too_many_lines)]`
- `register_template` returns `Result<(), LoadError>` but never errors — `#[allow(clippy::unnecessary_wraps)]`
- **INFINITE LOOP hazard**: `duplicate` preserves the original; if the duplicate matches the same rule, it fires forever. Tests for `duplicate` must use constant constraints that the duplicate does NOT satisfy.

### Test CE (Pass 002) Pattern
- Test CEs are NOT compiled into Rete; collected in `CompiledRuleInfo::test_conditions`
- Evaluated at firing time in `execute_actions`
- `execute_actions` returns `(bool, Vec<ActionError>)` — `bool` = did rule logically fire?
- `run()` only increments `rules_fired` when bool is `true`
- `step()` always returns `Some(FiredRule)` (agenda pop happened regardless)

### EvalContext Borrow Pattern
`EvalContext` holds `&'a mut SymbolTable`. `from_action_expr` also needs `&mut SymbolTable`.
These cannot coexist — translate first, then construct context:
```rust
// CORRECT
let runtime_expr = from_action_expr(expr, symbol_table, config)?;  // uses &mut
let mut ctx = EvalContext { symbol_table, ... };                    // then borrow
eval(&mut ctx, &runtime_expr)
```

### Conflict Resolution Strategies (Pass 007)
- **Depth**: Higher salience > Higher timestamp (most recent) > Higher seq
- **Breadth**: Higher salience > Lower timestamp (oldest) > Higher seq
- **LEX**: Higher salience > Lexicographic recency (most recent fact first per position) > Higher seq
- **MEA**: Higher salience > First-pattern recency > LEX tiebreak on rest > Higher seq

### Beta Network
- Beta network root node ID starts at 100,000 to avoid conflicts with alpha node IDs

### Pattern Validation
- Validation happens in `compile_rule_construct` BEFORE pattern translation
- Max nesting depth: 2 (for not/exists)
- `exists` containing `not` is rejected (E0005)
- `SourceLocation` is in `ferric-core` (doesn't depend on parser's `Span` type)

### Pass 005: Deffunction / Defglobal Stage 2 Interpretation
- New types (`FunctionConstruct`, `GlobalDefinition`, `GlobalConstruct`) and `Construct::Function`/`Construct::Global` variants live in `ferric-parser/src/stage2.rs`
- Body expressions in `interpret_function` use `interpret_action_expr` (the spec may call it `parse_action_expr` but the real name in this codebase is `interpret_action_expr`)
- `?*name*` tokens are `Atom::GlobalVar(name)` in the AST; `=` in defglobal is `Atom::Connective(Connective::Equals)` from the lexer
- Loader: add `"deffunction" | "defglobal"` to the construct_forms filter; add `functions` and `globals` fields to `LoadResult`; handle `Construct::Function` and `Construct::Global` arms in the processing loop
- Tests that previously expected `UnsupportedForm` errors for `deffunction`/`defglobal` must be replaced with success assertions
- `pedantic` is at `warn` level (not `deny`) in workspace lints, so `too_many_lines` warnings are non-fatal; suppress them with `#[allow(clippy::too_many_lines)]` on the affected functions (`interpret_constructs`, `interpret_function`, `load_str`)

### Pass 007 (parser): Defmodule / Defgeneric / Defmethod Stage 2 Interpretation
- New types (`ModuleSpec`, `ImportSpec`, `ModuleConstruct`, `GenericConstruct`, `MethodParameter`, `MethodConstruct`) and `Construct::Module/Generic/Method` variants live in `ferric-parser/src/stage2.rs` BEFORE `InterpreterConfig`
- When a form transitions from "unsupported" to supported: update BOTH the parser test AND the loader test that expected `UnsupportedForm`/`UnknownConstruct` to use a genuinely unsupported form (e.g., `defclass`)
- Also remove unused imports from integration tests (e.g., `assert_unsupported_form`)
- **Clippy: collapsible_match** — `if let Some(x) = ... { if let Pattern(v) = x { ... } }` must collapse to `if let Some(Pattern(v)) = ...`
- **Clippy: doc-link-with-quotes / doc-markdown** — avoid `construct_type="deftemplate"` style in doc comments; use backtick-wrapped code or rephrase
- `?ALL` / `?NONE` are parsed as `Atom::SingleVar("ALL")` / `Atom::SingleVar("NONE")` (not symbols), since `?foo` is a variable in CLIPS
- `defgeneric` takes only name + optional comment — NO parameter list in the defgeneric itself (params go in defmethod)

### Pass 008: Defmodule / Focus Stack
- `ModuleRegistry` lives in `crates/ferric-runtime/src/modules.rs` (pub)
- Engine gains: `module_registry: ModuleRegistry`, `rule_modules: HashMap<RuleId, ModuleId>`, `template_modules: HashMap<TemplateId, ModuleId>`
- **CRITICAL BUG AVOIDED**: In `load_str`, constructs are collected in a first pass then rules compiled in a second pass. If `defmodule` statements update `current_module` during the first pass, ALL rules get the LAST module's id (not their own). Fix: capture `current_module` alongside each rule during the first pass using `Vec<(RuleConstruct, ModuleId)>`, then restore module before compiling each rule.
- Focus stack initialized with `[MAIN]`; `run()` uses `pop_matching` to find activations for current focus module; pops focus when no matching activations exist for that module
- `focus` RHS action: collects module names in `focus_requests: Vec<String>`, applied in reverse order after all actions complete (so first arg ends up on top)
- `reset()` must call `module_registry.reset_focus()` to restore `[MAIN]` focus stack
- `step()` is NOT focus-aware (uses `agenda.pop()` directly); only `run()` is focus-aware
- `clippy::uninlined_format_args`: use `{var:?}` not `"{:?}", var` in assert messages

### Pass 004: Printout / OutputRouter
- `OutputRouter` lives in `crates/ferric-runtime/src/router.rs` (pub, not pub(crate))
- Engine gains `pub(crate) router: OutputRouter` field; `reset()` calls `router.clear()`
- `execute_actions` and `execute_single_action` both gain `router: &mut OutputRouter` parameter (now 9 args)
- `Multifield` exposes `as_slice()` (not `values()`) for iterating elements
- `clippy::match_same_arms`: merge identical pattern arms with `|` (e.g., `Symbol(s) | String(s)`)
- `clippy::format_push_string`: use `use std::fmt::Write as FmtWrite` + `write!(output, ...)` instead of `push_str(&format!(...))`
- `crlf` / `tab` / `ff` in printout are resolved as symbols at format time, not at parse time

### Pass 009: Defgeneric/Defmethod Dispatch Runtime
- `GenericRegistry` (with `RegisteredMethod`, `GenericFunction`) lives in `functions.rs`; `Engine` gains `pub(crate) generics: GenericRegistry`
- `EvalContext` gains `pub generics: &'a GenericRegistry`; threaded through all call sites in `evaluator.rs`, `actions.rs`, `loader.rs`
- `MethodConstruct.parameters` is `Vec<MethodParameter>` (not `Vec<String>`); extract names/type_restrictions when calling `register_method`
- **CRITICAL design**: method bodies evaluate through `eval()`, NOT through action machinery — `assert`/`retract` do NOT work inside method bodies. Methods must return pure values; RHS actions use those values.
- `clippy::single_match_else`: `match { Some(x) => x, None => { return Err(...) } }` must use `if let Some(x) = ... { x } else { return Err(...) }`
- `clippy::too_many_arguments` on local `eval_expr` (8 params): add `#[allow(clippy::too_many_arguments)]`
- `get_ordered_fields_for_fact(engine, FactId)` added to `test_helpers.rs` for test inspection by fact ID

### Pass 011: Phase 3 Integration and Exit Validation
- **CRITICAL fixture path rule**: Fixture files must exist in BOTH `tests/fixtures/` (workspace root) AND `crates/ferric-runtime/tests/fixtures/` (crate root). `cargo test` sets CWD to the crate root, so `load_fixture` using `"tests/fixtures"` relative path will look in `crates/ferric-runtime/tests/fixtures/`.
- `printout` is an RHS action and is NOT available inside defmethod/deffunction bodies (bodies are pure expression evaluators). Tests for "generic + printout" must call the generic from the rule RHS and do the printing in the rule body.
- When adding new tests, check the `use` imports at the top of the test module — helpers like `find_facts_by_relation` and `load_fixture` must be explicitly imported.
- `cargo fmt` enforces line-length limits: long `assert!(cond, "msg {var}")` calls must be broken into multi-line form.

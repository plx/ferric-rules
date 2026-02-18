# Phase 003 Notes

## Pass 001: Phase 3 Baseline and Harness Alignment

### What was done
- Reconciled crate-level doc comments across ferric-core, ferric-parser, ferric-runtime, and ferric (facade) to reflect Phase 2 completion and Phase 3 starting contracts.
- Extended `test_helpers.rs` with reusable helpers for Phase 3 test patterns:
  - `run_to_completion()`, `load_and_run()` for execution convenience
  - `assert_fact_count()`, `find_facts_by_relation()`, `assert_has_fact_with_relation()`, `assert_no_fact_with_relation()` for fact queries
  - `load_fixture()` for loading `.clp` fixture files
  - `assert_unsupported_form()` for verifying explicit diagnostic errors
- Created `phase3_integration_tests.rs` with active tests for:
  - Unsupported-form diagnostics (5 tests verifying explicit errors for deffunction/defglobal/defmodule/defgeneric/defmethod)
  - Phase 2 behavior preservation (6 "canary" tests covering rule chains, negation, exists, NCC, retract, reset)
  - Commented-out scaffolding for all upcoming Phase 3 passes
- Created 7 fixture `.clp` files in `tests/fixtures/` for Phase 3 constructs.

### Decisions and trade-offs
- `find_facts_by_relation()` uses `resolve_symbol_str()` (read-only) rather than `intern_symbol()` (mutating) to avoid requiring `&mut Engine` in assertion helpers that only take `&Engine`.
- Phase 3 fixture files contain full CLIPS syntax even though most won't parse yet - they serve as executable documentation of the target behavior.
- Unsupported-form tests are active (not commented out) because they verify the current diagnostic behavior and will need to be updated/removed as each construct gains support.

### Remaining TODOs
- None for this pass.

### Lingering questions
- The `forall_vacuous_truth.clp` fixture from Phase 2 is commented out. Pass 010 should decide whether to enable it or replace it with the new `phase3_forall.clp`.
- The invariant harness (`debug_assert_consistency()`) does not yet have extension points for function environments, modules, or global storage. These will need to be added as those subsystems are implemented.

### Process notes
- Used agent team approach: two background agents (fixtures-agent, docs-agent) worked in parallel while team lead handled test helpers and integration tests. The parallel work completed efficiently with no conflicts.

## Pass 002: Expression Evaluation Path for RHS and Test

### What was done
- Created `evaluator.rs` as a shared expression evaluation pipeline used by both RHS actions and test CEs.
- Defined `RuntimeExpr` as a parser-agnostic normalized expression model with `Literal`, `BoundVar`, `GlobalVar`, and `Call` variants.
- Implemented translation functions: `from_action_expr` (ActionExpr → RuntimeExpr) and `from_sexpr` (SExpr → RuntimeExpr for test CEs).
- Implemented 18 built-in functions covering arithmetic, comparison, boolean logic, and type predicates.
- Refactored `actions.rs` to route all expression evaluation through the evaluator pipeline instead of the Phase 2 inline `eval_expr`.
- Added `test_conditions` field to `CompiledRuleInfo` — test CEs are pre-translated during rule compilation and evaluated at firing time.
- Updated `loader.rs` to intercept `Pattern::Test` during rule translation, converting the S-expression to a `RuntimeExpr` without generating any rete network nodes.
- Updated `engine.rs` so `run()` counts only logical firings (test CEs passed).
- Added `connective_to_function_name()` to map parser connectives (`=`, `&`, `|`, `~`) to builtin function names.

### Decisions and trade-offs
- **Test CE evaluation happens at firing time, not in the rete network.** The alternative (adding test nodes to the beta network) would reduce wasted activations but would require significant rete extensions. The pragmatic approach stores test expressions in `CompiledRuleInfo` and evaluates them when an activation is popped. This means extra activations may sit on the agenda that will be filtered at fire time — a future optimization target.
- **Expression translation happens at execution time for RHS actions.** `from_action_expr` is called each time `eval_expr` runs. Pre-translating during compilation would be more efficient but requires storing `RuntimeExpr` versions of all actions, which is a larger refactoring. Revisit if profiling shows it matters.
- **`execute_actions` returns `(bool, Vec<ActionError>)`.** The bool indicates whether the rule logically fired. This was necessary to let `engine.rs`'s `run()` skip the `rules_fired` counter for suppressed activations.
- **`step()` always returns `Some(FiredRule)` regardless of test CE outcome.** This represents "an activation was processed from the agenda." The `run()` method uses the bool to count logical firings. This slight inconsistency is noted for potential future cleanup (a richer `StepResult` type).
- **Connectives mapped to function names.** The parser classifies `=`, `&`, `|`, `~` as `Connective` atoms rather than symbols. The evaluator's `from_sexpr` now maps these to their corresponding builtin function names so they work in test CE expressions.

### Remaining TODOs
- Global variables (`?*name*`) return `UnboundGlobal` errors — will be resolved in Pass 005 (defglobal).
- `ActionExpr` translation at execution time could be optimized by pre-translating during compilation.
- The `step()` vs `run()` semantics around test CE filtering could be unified.

### Lingering questions
- Should `step()` return a richer type that distinguishes "processed but filtered" from "logically fired"? Current behavior is acceptable but worth revisiting.
- The `Connective::Colon` maps to `":"` and `Connective::Assign` to `"<-"` — neither has a corresponding builtin function. If these appear in test CE head position, they'll produce `UnknownFunction` errors, which is the correct behavior.

## Pass 003: Template-Aware Modify and Duplicate Semantics

### What was done
- Created `templates.rs` module with `RegisteredTemplate` struct holding slot names, slot index map, and default values.
- Added template registry to Engine: `template_ids` (name→TemplateId), `template_defs` (TemplateId→RegisteredTemplate), `template_id_alloc` (SlotMap allocator).
- Implemented `register_template()` in loader to allocate TemplateId, build slot metadata, and store in engine's registry.
- Fixed `process_template_fact_body` to create proper `Fact::Template` instead of faking as ordered facts. Uses registry for slot name→index mapping and populates defaults.
- Enabled template pattern compilation: `Pattern::Template` now resolves template name→TemplateId, creates `AlphaEntryType::Template`, and translates slot constraints with `SlotIndex::Template`.
- Made `execute_modify` and `execute_duplicate` template-aware: they now match on `Fact::Ordered` vs `Fact::Template` and handle each with the appropriate slot-update logic.
- Added `apply_template_slot_overrides` helper for resolving slot names to indices via `RegisteredTemplate`.
- Threaded `template_defs` through the `execute_actions` call chain from engine to actions.

### Decisions and trade-offs
- **TemplateId allocation via SlotMap**: Used `SlotMap<TemplateId, ()>` in Engine for ID allocation. This is consistent with the SlotMap key type definition in ferric-core and ensures unique IDs.
- **Templates must be defined before use**: Templates are registered during the construct processing loop, before rules are compiled. A rule referencing an undefined template gets a compile error.
- **Default values**: Slot defaults are stored as `Value::Void` when no explicit default is provided (matching CLIPS ?DERIVE behavior). The `DefaultValue::Value(lit)` case properly converts literals. `DefaultValue::None` (required slots) is not yet enforced — this could be added later.
- **modify retract+assert semantics**: Template modify follows the same retract-then-assert pattern as ordered modify, preserving rete consistency.
- **Action errors are non-fatal**: Unknown slot errors during modify/duplicate produce `ActionError::EvalError` but don't halt execution. The activation is still popped from the agenda.
- **duplicate uses constant constraints to prevent infinite loops**: The integration test for duplicate uses a constant slot value in the pattern so the duplicated fact (with different value) doesn't re-trigger the rule.

### Remaining TODOs
- Default value enforcement for `?NONE` slots (required slots with no default).
- Multi-slot support in templates (`SlotType::Multi`).
- Template facts in top-level `(assert ...)` forms (currently only deffacts).
- Template fact pretty-printing for debugging.

### Lingering questions
- Should `RegisteredTemplate` live in a shared crate-level module or eventually move to ferric-core? Currently in `ferric-runtime/src/templates.rs`.
- How should duplicate slot overrides in modify be handled? Currently each override is applied in order, so the last one wins.

## Pass 004: Printout Runtime and Router Integration

### What was done
- Created `router.rs` with `OutputRouter` type for per-channel output capture.
- Added `router` field to Engine, with `get_output()` public accessor.
- Replaced `printout` no-op stub with full implementation: channel extraction, value evaluation through evaluator, special symbol expansion (`crlf`, `tab`, `ff`), CLIPS-compatible value formatting.
- Threaded `router: &mut OutputRouter` through the execute_actions call chain.

### Decisions and trade-offs
- **Buffer-based capture only**: No external writer support yet. OutputRouter stores `HashMap<String, String>` for captured output. Future phases could add external writer forwarding.
- **reset() clears output**: Consistent with CLIPS behavior. Output is runtime state, not compiled state.
- **Float formatting**: Always shows decimal point via `{f:.1}` format. CLIPS behavior.
- **String values without quotes**: `Value::String` prints raw content, not `"quoted"`.
- **Channel argument not evaluated**: The channel name (first arg to printout) is read as a literal symbol, not evaluated through the expression pipeline. This matches CLIPS behavior where channels are always literal.

### Remaining TODOs
- External writer forwarding (for real stdout/stderr output when not in test mode).
- `read`/`readline` input router (future phase).
- `format` function for CLIPS-style formatting.

### Lingering questions
- Should the router support a "passthrough" mode where output goes to actual stdout? Currently all output is captured in buffers only.

## Pass 005: Stage 2 Deffunction and Defglobal Interpretation

### What was done
- Added `FunctionConstruct`, `GlobalConstruct`, `GlobalDefinition` types to ferric-parser's Stage 2 AST.
- Implemented `interpret_function`: parses name, optional comment, parameter list with wildcard (`$?`) support, body expressions via `ActionExpr`.
- Implemented `interpret_global`: parses repeating `?*name* = expr` triplets, supports both `Connective::Equals` and `Symbol("=")` for the equals sign.
- Extended `Construct` enum with `Function(FunctionConstruct)` and `Global(GlobalConstruct)` variants.
- Updated `interpret_constructs` dispatcher to handle `deffunction` and `defglobal`.
- Extended `LoadResult` with `functions: Vec<FunctionConstruct>` and `globals: Vec<GlobalConstruct>` fields.
- Updated loader to recognize and pass through `deffunction`/`defglobal` constructs.
- Re-exported new types from ferric-parser and ferric-runtime.
- Updated unsupported-form tests: deffunction/defglobal now load successfully; unsupported-form tests use defmodule instead.
- Added 21 parser tests covering deffunction/defglobal interpretation including error cases.
- Added 4 loader tests for deffunction/defglobal loading.

### Decisions and trade-offs
- **Parse-only, no runtime**: Pass 005 is strictly Stage 2 interpretation — it adds AST types and parsing for deffunction/defglobal but does NOT create runtime registries or execution. That's Pass 006's responsibility.
- **Wildcard parameter representation**: Wildcard params (`$?rest`) are stored as a separate `wildcard_param` field on `FunctionConstruct` rather than being mixed into the regular `params` list. This makes arity checking clearer at runtime.
- **Global name format**: Global variables use the `?*name*` convention. The parser validates the `?*...*` format and strips the delimiters for storage in `GlobalDefinition.name`.
- **Equals sign flexibility**: `interpret_global` accepts both `Connective::Equals` and `Symbol("=")` for the assignment operator, since the parser may classify `=` either way depending on context.
- **LoadResult accumulation**: Functions and globals are accumulated into vectors on `LoadResult` rather than being immediately registered. This keeps the loader as a pass-through for Stage 2 AST and defers runtime registration to the engine/loader integration in Pass 006.

### Remaining TODOs
- Runtime function registry and global variable store (Pass 006).
- Function body execution via evaluator (Pass 006).
- Global variable initialization evaluation (Pass 006).

### Lingering questions
- Should `FunctionConstruct` store pre-translated `RuntimeExpr` bodies, or should translation happen at call time? The current approach stores `ActionExpr` bodies (matching how rule actions are stored). Pass 006 will need to decide on the translation strategy.

## Pass 006: User-Defined Function Environment and Execution

### What was done
- Created `functions.rs` with `UserFunction`, `FunctionEnv` (function registry), and `GlobalStore` (global variable storage), each with clean public APIs.
- Added `max_call_depth: usize` to `EngineConfig` (default 256) for recursion protection.
- Extended `EvalContext` with three new fields: `functions: &'a FunctionEnv`, `globals: &'a mut GlobalStore`, `call_depth: usize`.
- Implemented global variable evaluation: `RuntimeExpr::GlobalVar` now looks up from the `GlobalStore`, returning `UnboundGlobal` only when the variable is truly unset.
- Implemented user-defined function dispatch: when `dispatch_builtin` returns `UnknownFunction`, the evaluator checks `FunctionEnv` for a matching user function. If found, it creates a call frame (fresh `VarMap`/`BindingSet`), binds parameters, translates body `ActionExpr` to `RuntimeExpr`, and evaluates body expressions sequentially, returning the last value.
- Implemented `bind` as a special form: first argument is pattern-matched (not evaluated) as a `RuntimeExpr::GlobalVar`, second argument is evaluated and stored in the `GlobalStore`.
- Wildcard parameter support: excess call arguments beyond named parameters are collected into a `Multifield` value and bound to the wildcard parameter name.
- Added `RecursionLimit` error variant to `EvalError` with depth tracking.
- Added `functions`, `globals`, and `registered_globals` fields to Engine; `get_global()` public API.
- Updated `reset()` to reinitialize globals from the registered initial value snapshot.
- Threaded `&FunctionEnv` and `&mut GlobalStore` through the entire `execute_actions` call chain in `actions.rs` (9 functions updated).
- Updated `loader.rs`: `Construct::Function` now registers a `UserFunction` in the engine's `FunctionEnv`; `Construct::Global` evaluates initial value expressions and stores in both `GlobalStore` and `registered_globals`.
- Updated all ~60 existing evaluator unit tests for new `EvalContext` fields.
- Added 13 integration tests and 9 unit tests (functions.rs).

### Decisions and trade-offs
- **Translation at call time**: `UserFunction` stores `ActionExpr` bodies (not pre-translated `RuntimeExpr`). Translation via `from_action_expr` happens each time the function is called. This matches how rule RHS actions work and avoids storing duplicate expression representations. Could be optimized later if profiling shows hot paths.
- **Clone-based function dispatch**: The user function is `.cloned()` from the `FunctionEnv` before dispatch to avoid holding a borrow on `ctx.functions` while `ctx` is mutably borrowed for evaluation. The clone is cheap for small functions and avoids lifetime gymnastics.
- **`bind` as special form**: Unlike regular builtins that evaluate all arguments first, `bind` pattern-matches its first argument as a `RuntimeExpr::GlobalVar` without evaluation. This is necessary because `?*name*` in binding position is an L-value, not an R-value.
- **Global snapshot on load**: Initial global values are evaluated once at load time and stored in `registered_globals` as a `Vec<(String, Value)>`. On `reset()`, globals are restored from this snapshot rather than re-evaluating the expressions. This is simpler and avoids ordering issues with inter-dependent globals. More sophisticated re-evaluation semantics can be added later if needed.
- **`bind` returns the value**: Following CLIPS semantics, `(bind ?*x* 42)` returns 42. This allows chaining like `(+ (bind ?*x* 1) (bind ?*y* 2))`.
- **No local `bind`**: The current `bind` implementation only supports global variables. Local variable rebinding within function bodies (which CLIPS supports) would require mutable bindings, which is a larger change deferred to a future pass.

### Remaining TODOs
- Local variable `bind` within function bodies (requires mutable BindingSet or a separate local-variable store).
- `return` function for early return from function bodies.
- `str-cat`, `sym-cat`, and other string manipulation builtins.
- `nth$`, `length$`, and other multifield access builtins (for wildcard parameter usage).
- Pre-translation optimization for frequently-called functions.

### Lingering questions
- Should `UserFunction` eventually store pre-translated `RuntimeExpr` bodies for performance? The current approach is simple and correct; optimization can be data-driven.
- How should redefinition of a function that's already in use by compiled rules behave? Currently the latest registration wins (HashMap overwrite), which is correct CLIPS behavior.
- The `max_call_depth` default of 256 is somewhat arbitrary. CLIPS doesn't specify a limit. Should this be configurable via a builder pattern?

## Pass 007: Stage 2 Defmodule/Defgeneric/Defmethod Interpretation

### What was done
- Added 6 new AST types to ferric-parser Stage 2: `ModuleSpec`, `ImportSpec`, `ModuleConstruct`, `GenericConstruct`, `MethodParameter`, `MethodConstruct`.
- Extended `Construct` enum with `Module`, `Generic`, `Method` variants.
- Implemented `interpret_module` with helpers `interpret_module_spec` and `interpret_import_spec`: handles `(export ?ALL)`, `(export ?NONE)`, `(export deftemplate name1 name2)`, and corresponding import forms.
- Implemented `interpret_generic`: parses name and optional comment (defgeneric is declaration-only; methods provide implementations).
- Implemented `interpret_method`: parses name, optional explicit index, parameter restriction list with type constraints, optional wildcard parameter, and body expressions.
- Updated `interpret_constructs` dispatch — defmodule/defgeneric/defmethod no longer treated as unsupported.
- Extended `LoadResult` with `modules`, `generics`, `methods` fields.
- Updated loader to recognize and collect all three construct types.
- Re-exported new types from ferric-parser and ferric-runtime.
- Updated unsupported-form tests to use `defclass` instead.
- Added 27 parser tests + 4 loader tests.

### Decisions and trade-offs
- **Parse-only, no runtime**: Like Pass 005 for deffunction/defglobal, this pass is strictly AST interpretation. Runtime module registry and generic dispatch are Passes 008 and 009.
- **`?ALL`/`?NONE` parsed as SingleVar**: The parser treats `?ALL` as `Atom::SingleVar("ALL")` since it follows single-field variable syntax. The module spec interpreter pattern-matches on the variable name to distinguish from regular variables.
- **Method parameter restrictions as list of type names**: Each parameter restriction `(?x INTEGER FLOAT)` is stored as `MethodParameter { name: "x", type_restrictions: ["INTEGER", "FLOAT"] }`. Query restrictions (e.g., `(> ?x 0)`) are not yet supported.
- **Optional method index**: CLIPS allows `(defmethod name 1 ...)` to specify an explicit dispatch index. This is stored as `Option<i32>` and defaults to `None` when omitted.
- **defgeneric has no parameters**: In CLIPS, `defgeneric` is just a declaration (name + optional comment). The parameter restrictions and body belong to `defmethod`.
- **Unsupported form sentinel changed to `defclass`**: With defmodule/defgeneric/defmethod now supported, the unsupported-form test uses `defclass` which is a CLIPS construct not planned for Phase 3.

### Remaining TODOs
- Runtime module registry (Pass 008).
- Module-qualified name resolution (Pass 008).
- Focus stack integration (Pass 008).
- Generic function dispatch via method type matching (Pass 009).
- Query restrictions in method parameters.

### Lingering questions
- Should `ModuleSpec::Specific` names include `?ALL`/`?NONE` as special values, or should those be separate `ModuleSpec` variants? Currently they're stored as string names `"?ALL"`/`"?NONE"` within the Specific variant when used at the construct-type level.
- How should duplicate method indices be handled? Currently the parser just stores what it sees; validation is deferred to the runtime pass.

## Pass 008: Defmodule Import/Export and Focus Semantics

### What was done
- Created `modules.rs` with `ModuleId(u32)` newtype, `RuntimeModule` struct (name, exports, imports), and `ModuleRegistry` (module registry + focus stack + construct visibility checking).
- MAIN module auto-created with `export ?ALL` on `ModuleRegistry::new()`.
- Added `pop_matching(&mut self, predicate)` and `has_matching(&self, predicate)` to `Agenda` in ferric-core for focus-aware activation selection.
- Engine gains `module_registry: ModuleRegistry`, `rule_modules: HashMap<RuleId, ModuleId>`, `template_modules: HashMap<TemplateId, ModuleId>` fields.
- Focus-aware `run()`: uses `pop_matching` to find highest-priority activation belonging to the current focus module. When no activations exist for focus module, pops focus stack. When focus stack empty, returns `AgendaEmpty`.
- Focus RHS action: `(focus A B C)` collects module names as deferred requests in a `Vec<String>`, applied by engine after all actions complete (pushed in reverse order so first argument ends up on top of stack).
- Loader registers `Construct::Module` in `ModuleRegistry`, sets `current_module` during construct processing. Two-pass loading bug identified and fixed: rules captured as `(RuleConstruct, ModuleId)` tuples during collection to preserve correct module association.
- Template visibility checking: `Pattern::Template` compilation verifies `is_construct_visible()` before proceeding; produces clear diagnostic error if template is not visible.
- `reset()` calls `module_registry.reset_focus()` to restore focus stack to `[MAIN]`.
- Added 10 integration tests, 3 agenda unit tests, 8 module registry unit tests.

### Decisions and trade-offs
- **ModuleId in ferric-runtime, not ferric-core**: ModuleId is a runtime concept. Unlike TemplateId/RuleId/FactId (which are SlotMap keys in ferric-core), ModuleId is a simple u32 newtype since module count is low and we don't need slotmap's generational safety.
- **Facts are global**: Following CLIPS semantics, facts belong to the working memory globally. Only rule *firing* is module-scoped via the focus stack. A rule in module FOO can match facts asserted by module BAR.
- **Focus requests are deferred**: The `focus` action can't directly modify the module registry during action execution (borrow conflicts with engine). Solution: collect focus module names in a `Vec<String>`, apply after all actions complete.
- **Focus argument order**: CLIPS `(focus A B C)` means A fires first (on top of stack). Implemented by pushing in reverse order.
- **Backward compatibility**: Without any `(defmodule)`, all rules belong to MAIN, focus stack starts as [MAIN], `pop_matching` matches all activations → identical behavior to the old `pop()`.
- **Two-pass loading fix**: In `load_str`, constructs are collected then rules compiled in separate passes. If `defmodule` updates `current_module` during collection, a subsequent `defmodule` would overwrite it, causing all rules to be assigned to the last module. Fixed by capturing `Vec<(RuleConstruct, ModuleId)>` during collection and using the stored ModuleId during compilation.
- **Disjoint field borrows**: The `run()` closure captures `&self.rule_modules` (immutable) while `self.rete.agenda` is mutably borrowed via `pop_matching`. This works because Rust 2021 edition supports disjoint struct field borrows.
- **`is_construct_visible` is string-based**: Visibility checking compares construct type strings ("deftemplate", etc.) against export/import specs. This is simple and matches the AST representation.

### Remaining TODOs
- Module-qualified names (e.g., `MAIN::template-name`) are not yet supported. Currently all names are unqualified.
- Export/import for constructs other than `deftemplate` (e.g., `deffunction`, `defglobal`) is not enforced.
- `get-focus`, `get-focus-stack`, `list-focus-stack` query functions.
- `pop-focus` RHS action.

### Lingering questions
- Should `pop_matching` be O(1) for the common case? Currently it scans the BTreeMap from highest priority. In practice, single-module programs (the common case) will always match the first entry.
- Should module-qualified template resolution (e.g., `FOO::my-template`) be a separate pass or folded into a later Pass 008 revision?
- How should redefinition of a module be handled? Currently `register()` would panic on duplicate ModuleId but doesn't check for duplicate names.

## Pass 009: Defgeneric/Defmethod Dispatch Runtime

### What was done
- Added `RegisteredMethod`, `GenericFunction`, and `GenericRegistry` types to `functions.rs` for generic function registration and method dispatch.
- Methods sorted by index (ascending) within each `GenericFunction`. Auto-index assignment when no explicit index: increments from max existing + 1.
- Added `NoApplicableMethod { name, actual_types, span }` error variant to `EvalError` with CLIPS type name reporting.
- Added `generics: &'a GenericRegistry` field to `EvalContext`.
- Integrated generic dispatch as the third dispatch tier in `eval()`: builtins → user-defined → generic → `UnknownFunction` error.
- Implemented `dispatch_generic`: eagerly evaluates all arguments, iterates methods in index order, checks applicability (arity + type restrictions), calls first applicable method with fresh binding frame. Same body evaluation pattern as `dispatch_user_function`.
- Implemented `value_matches_type` for CLIPS type checking and `value_type_name` for error messages.
- Added `GenericRegistry` field to Engine, threaded through entire `execute_actions` chain in `actions.rs` (9 functions updated).
- Loader now registers `Construct::Generic` in `GenericRegistry` and extracts method metadata from `Construct::Method` for registration.
- Added 7 unit tests (registry, sorting, auto-index, wildcard) and 11 integration tests.

### Decisions and trade-offs
- **Three-tier dispatch**: The function call chain is now builtins → user-defined → generic. User-defined functions take priority over generics with the same name. This matches CLIPS behavior where `deffunction` shadows `defgeneric`.
- **First-applicable method wins**: Methods are tried in index order (lowest first). The first method whose type restrictions match the evaluated arguments fires. No ambiguity resolution needed — the ordering IS the resolution.
- **Auto-index increments from max**: When no explicit index is provided, the next auto-index is one above the highest existing index. This means methods registered in order get ascending indices. CLIPS does specificity-based auto-indexing (more specific = lower index), but our simpler approach is correct for Phase 3 and matches registration order.
- **Method bodies are pure expressions**: Like user-defined functions, method bodies go through the expression evaluator. RHS actions (`assert`, `retract`, etc.) are NOT available inside method bodies — only from rule RHS. Tests designed accordingly.
- **Generic auto-created by defmethod**: CLIPS allows `(defmethod foo ...)` without a preceding `(defgeneric foo)`. The `register_method` call auto-creates the generic entry.
- **Clone-based dispatch**: Like `dispatch_user_function`, the `GenericFunction` is cloned before dispatch to avoid holding a borrow while mutably borrowing `EvalContext`.
- **CLIPS type hierarchy**: `NUMBER` matches both `INTEGER` and `FLOAT`; `LEXEME` matches both `SYMBOL` and `STRING`. These are the standard CLIPS abstract type classes.

### Remaining TODOs
- Full CLIPS specificity-based auto-index assignment (more specific type restrictions = lower index).
- Method redefinition (replacing a method at an existing index).
- `call-next-method` for method chaining within a generic dispatch.
- Query restrictions in method parameters (e.g., `(is-a PERSON ?x)`).
- `get-method-restrictions` and `list-defmethods` query functions.

### Lingering questions
- Should `dispatch_generic` pre-evaluate arguments once (current approach) or lazily evaluate per method attempt? Pre-evaluation is simpler and correct for CLIPS semantics (no side effects in evaluation).
- Should a `deffunction` and `defgeneric` with the same name coexist, or should one shadow the other? Currently `deffunction` wins (checked first). CLIPS treats this as an error.
- Performance of cloning `GenericFunction` on every dispatch could matter for hot paths. Could be optimized with `Rc<GenericFunction>` if profiling shows it's an issue.

## Pass 010: Forall Limited Semantics and Regression Contract

### What was done
- Added `Pattern::Forall(Vec<Pattern>, Span)` variant to the parser's `Pattern` enum and `"forall"` arm to `interpret_pattern`.
- In the loader, `translate_condition` handles `Pattern::Forall` by:
  1. Validating Phase 3 restrictions (exactly 2 sub-patterns, both simple ordered/template, no nested forall)
  2. Desugaring to `CompilableCondition::Ncc([P, neg(Q)])` — condition as positive, then-clause as negated
- Changed compiler validation to allow negated subpatterns within NCC (`allow_negated: true`), which is correct and necessary for forall desugaring.
- Added initial-fact mechanism: `ensure_initial_fact()` in loader asserts `(initial-fact)` after rules compile. Engine tracks `initial_fact_id: Option<FactId>`, `facts()` filters it out, `reset()` re-asserts it. Loader injects implicit `(initial-fact)` join when first condition is NCC, enabling standalone forall/negation rules without user-visible trigger patterns.
- **Critical bug fix in rete.rs**: `retract_token_cascade` was not calling `ncc_handle_result_retraction`. This meant that when tokens inside an NCC subnetwork were retracted (e.g., a negative node unblocks because a matching Q arrives), the NCC's result count was never decremented. The NCC stayed "blocked" even when all conditions were satisfied. Fixed by threading `fact_base` and `new_activations` through the retraction cascade call chain. This fix was essential for forall to correctly transition between satisfied/unsatisfied states.
- Enabled the commented-out `forall_vacuous_truth_and_retraction_cycle` regression test.
- Added 8 integration tests and 2 parser tests.

### Decisions and trade-offs
- **Desugaring at translate_condition level**: `forall(P, Q)` → `NCC([P, neg(Q)])` at the loader translation level, not at the parser level. The parser preserves `Pattern::Forall` so error messages can reference "forall" specifically. The desugaring happens when translating to `CompilableCondition`.
- **Initial-fact mechanism**: CLIPS always asserts `(initial-fact)` on reset, providing a token for top-level negation/forall rules. We added this to the loader (post-compile) and engine reset. The fact is hidden from `engine.facts()` so users don't see it.
- **Phase 3 restrictions**: Only `(forall P Q)` with exactly two simple fact patterns is supported. Multi-pattern then-clauses, nested forall, and CE sub-patterns (not, exists, etc.) within forall are rejected with clear diagnostics.
- **NCC subnetwork for forall**: The NCC chain is [join(P), neg(Q), NCC-partner]. When no P-without-Q exists, the NCC is unblocked → forall satisfied. When any P-without-Q exists, the NCC is blocked → forall unsatisfied. Vacuous truth (no P facts) works naturally since the NCC subnetwork never produces results.
- **Compiler validation relaxation**: Allowing `negated: true` in NCC subpatterns is both correct (the rete compiler handles it properly) and necessary (for forall desugaring). The old restriction was overly conservative.

### Remaining TODOs
- Multi-pattern forall then-clauses: `(forall P Q1 Q2)` should desugar to `not(and(P, not(and(Q1, Q2))))`.
- Forall with non-simple sub-patterns (e.g., template patterns work, but nested CEs like `not` or `exists` within forall are blocked).
- Truth maintenance / logical support: when a forall becomes unsatisfied, facts asserted by its RHS should ideally be auto-retracted. This is not implemented.
- `(initial-fact)` should ideally be asserted automatically on every `reset()`, not just when rules exist. Currently it's only asserted after `load_str`.

### Lingering questions
- Should `(initial-fact)` be visible to user queries? Currently filtered from `facts()`, matching CLIPS behavior.
- The retract_token_cascade bug fix is significant and may have implications for other NCC scenarios. Should the existing NCC tests be reviewed for correctness in light of this fix?
- How should `forall` interact with modules? Currently, forall sub-patterns follow normal template visibility rules, but the forall CE itself is module-agnostic.

## Pass 011: Phase 3 Integration and Exit Validation

### What was done
- Updated 3 fixture files to match implemented syntax: `phase3_defgeneric.clp` (removed parameter from defgeneric declaration, methods return values), `phase3_defglobal.clp` (replaced unsupported field constraint `?x&:(...)` with `(test ...)` CE), `phase3_defmodule.clp` (replaced module-qualified names with focus-based module isolation).
- Added 7 fixture-driven integration tests that load and execute each Phase 3 fixture file.
- Added 6 cross-feature interaction tests exercising combinations: deffunction+defglobal, generic+printout, forall+template, deffunction calling generic, test CE with deffunction, global bind+printout.
- Added 1 diagnostic test verifying `defclass` produces a clear unsupported-construct error.
- Updated `lib.rs` doc comments to mark Phase 3 as complete with detailed pass references.

### Decisions and trade-offs
- **Method bodies are pure expressions**: The cross-feature test for generic+printout was designed so the generic method returns a value and the rule's RHS calls printout. Method bodies go through the expression evaluator, not the action executor, so they can't call `assert`, `retract`, or `printout` directly.
- **Fixture simplification**: Module fixtures avoid module-qualified names since they're not implemented. Template visibility across modules is tested via integration tests, not fixtures.
- **Exit validation scope**: The pass focuses on verifying that all features work in combination and that quality gates are clean. It does not add new runtime features.

### Phase 3 Definition of Done — Exit Checklist

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 1 | Template-aware modify/duplicate | ✅ | Pass 003: 7 integration tests |
| 2 | printout implemented | ✅ | Pass 004: 6 integration tests, fixture test |
| 3 | Shared expression evaluation | ✅ | Pass 002: evaluator.rs, 60+ unit tests |
| 4 | deffunction/defglobal executable | ✅ | Passes 005-006: 34 tests, fixture tests |
| 5 | defmodule import/export + focus | ✅ | Pass 008: 10 integration tests, fixture test |
| 6 | defgeneric/defmethod dispatch | ✅ | Pass 009: 11 integration tests, fixture test |
| 7 | Limited forall + vacuous truth | ✅ | Pass 010: 8 integration tests, regression test, fixture test |
| 8 | Quality gates clean | ✅ | fmt, clippy, test, check all clean |

### Phase 3 Statistics

- **Test count**: 776 (up from 130 at Phase 2 exit)
- **New modules**: evaluator.rs, functions.rs, modules.rs, router.rs, templates.rs
- **Passes completed**: 11 (001-011)
- **Key infrastructure added**: expression evaluator, function/global/generic registries, module registry with focus stack, output router, initial-fact mechanism
- **Critical bug fixed**: retract_token_cascade NCC result count management (Pass 010)

### Handoff to Phase 4

Phase 3 is complete. The engine now supports the full CLIPS language subset planned for this phase. Phase 4 (Standard Library Breadth) can build on:
- The expression evaluator for adding new built-in functions
- The function/generic registries for adding stdlib functions
- The module system for organizing stdlib constructs
- The output router for `format`, `read`, etc.

### Remaining items NOT in Phase 3 scope (deferred to Phase 4+)
- Module-qualified names (`MODULE::name`)
- Truth maintenance / logical support
- `defclass`/`definstances`/`defmessage-handler` (COOL)
- `call-next-method` for generic dispatch chaining
- Query restrictions in method parameters
- Multi-pattern forall then-clauses
- Local variable `bind` within function bodies
- `return` for early function return
- String/multifield manipulation builtins
- `if`/`while`/`loop-for-count` control flow in function bodies

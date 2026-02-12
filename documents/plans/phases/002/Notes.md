# Phase 002 Notes

## Pass 001: Phase 2 Baseline And Harness Alignment

### What was done

1. **Baseline reconciliation**: Updated module-level docs in all four crates to reflect the actual Phase 1 state per `PlanAdjustments.md`:
   - `ferric-core/lib.rs`: Documented ownership (values in core, not runtime), flat module layout, O(1) retraction cleanup, and consistency check coverage.
   - `ferric-parser/lib.rs`: Documented Stage 1 `ParseResult` API contract and Stage 2 scope.
   - `ferric-runtime/lib.rs`: Documented loader contract (`LoadResult`), `RuleDef` placeholder status, and Phase 1 engine API subset.
   - `ferric-runtime/engine.rs`: Clarified Phase 1 vs Phase 2 API surface in doc comments.
   - `ferric-runtime/loader.rs`: Updated `RuleDef` doc to note Phase 2 transition plan.

2. **Shared test helpers** (`ferric-runtime/src/test_helpers.rs`):
   - Engine helpers: `new_utf8_engine()`, `intern()`
   - Loading helpers: `load_ok()`, `load_err()`
   - Rete construction: `build_single_pattern_rete()`, `build_constant_test_rete()`, `build_two_pattern_rete()`
   - Rete assertion: `assert_facts_into_rete()`, `assert_one_fact()`, `retract_one_fact()`
   - Consistency: `assert_rete_consistent()`, `assert_rete_clean()`
   - Phase 2 pipeline stubs documented as comments for later passes.

3. **Integration tests refactored** to use shared helpers. Added new test: `integration_assert_retract_cycle_with_consistency_checks`.

4. **Consistency check extensions** in `ReteNetwork::debug_assert_consistency()`:
   - Added cross-check: agenda activations reference existing tokens.
   - Added Phase 2 extension point comments for negative, NCC, exists, and strategy checks.
   - Added `Agenda::iter_activations()` method to support the new cross-check.

5. **Skeleton test modules** created for all Phase 2 areas:
   - `ferric-parser/src/stage2.rs`: Stage 2 interpreter scaffold
   - `ferric-core/src/compiler.rs`: Rule compilation pipeline
   - `ferric-core/src/negative.rs`: Negative node and blocker tracking
   - `ferric-core/src/ncc.rs`: NCC (negated conjunctive condition)
   - `ferric-core/src/exists.rs`: Exists (existential quantification)
   - `ferric-core/src/strategy.rs`: Conflict resolution strategies
   - `ferric-core/src/validation.rs`: Pattern restriction validation
   - `ferric-runtime/src/execution.rs`: Run/step/halt/reset loop
   - `ferric-runtime/src/actions.rs`: RHS action execution
   - `ferric-runtime/src/phase2_integration_tests.rs`: End-to-end Phase 2 tests
   - `tests/fixtures/forall_vacuous_truth.clp`: Fixture for Section 7.5 regression shape

### Test results

- **262 tests pass** (148 core + 67 parser + 46 runtime + 1 facade)
- **0 clippy warnings**
- **1 new test** added (assert/retract cycle with consistency checks)
- **0 regressions** from Phase 1

### Remaining TODOs

None — this pass is strictly scaffolding.

### Noteworthy decisions

- Skeleton modules are registered as `pub mod` in their crate's `lib.rs` so they're available for cross-crate testing from the start. Empty modules contribute no binary size.
- `#[allow(dead_code)]` annotations on helpers that aren't used yet (e.g., `build_two_pattern_rete`, `load_err`, `assert_one_fact`) to keep the test helper module warning-free while still providing the API for later passes.
- The `forall_vacuous_truth.clp` fixture contains commented-out rules because the `forall` CE is Phase 3 scope. The fixture establishes the contract shape now so Phase 3 can plug in.

### Suggestions

- None at this point. The scaffolding is clean and ready for Pass 002.

---

## Pass 002: Stage 2 Construct AST And Interpreter Scaffold

### What was done

1. **Stage 2 construct types** (`ferric-parser/src/stage2.rs`):
   - `Construct` enum with `Rule`, `Template`, `Facts` variants
   - `RuleConstruct`: name, span, optional comment, salience (default 0), raw LHS/RHS S-expressions
   - `TemplateConstruct`: name, span, optional comment, raw slot definitions
   - `FactsConstruct`: name, span, optional comment, raw fact bodies

2. **Interpreter configuration**: `InterpreterConfig` with strict/classic modes. Strict stops on first error; classic collects all errors.

3. **Error infrastructure**: `InterpretError` with message, span, kind, and suggestions. `InterpretErrorKind` enum with: `ExpectedConstruct`, `EmptyConstruct`, `ExpectedKeyword`, `UnknownConstruct`, `MissingElement`, `InvalidStructure`. Constructor helpers and `Display` impl.

4. **Interpreter entry point**: `interpret_constructs(sexprs, config) -> InterpretResult`
   - Top-level dispatch on keyword: defrule, deftemplate, deffacts
   - Known unsupported keywords produce helpful "not yet supported" errors
   - Unknown keywords get edit-distance-based suggestions (Levenshtein)
   - Strict vs classic error collection modes

5. **Individual interpret functions** (scaffold):
   - `interpret_rule`: name, optional comment, optional `(declare (salience N))`, `=>` separator split
   - `interpret_template`: name, optional comment, remaining as raw slots
   - `interpret_facts`: name, optional comment, remaining as raw facts

6. **Re-exports**: Stage 2 types re-exported from `ferric-parser` crate root.

7. **Parser-runtime adapter**: `interpret_source()` and `interpret_ok()` helpers in `test_helpers.rs`.

8. **30 unit tests** covering:
   - Empty input, non-list top-level, empty list, non-symbol head
   - Unknown keyword with suggestions, known unsupported keywords
   - Minimal defrule/deftemplate/deffacts
   - Missing name, missing `=>` separator
   - Salience extraction, comment extraction
   - Multiple LHS/RHS patterns, multiple slots/facts
   - Mixed constructs, strict vs classic modes
   - Error display, edit distance, keyword suggestions

### Test results

- **292 tests pass** (148 core + 97 parser + 46 runtime + 1 facade)
- **0 clippy warnings**
- **30 new Stage 2 tests**
- **0 regressions**

### Remaining TODOs

None — deep construct semantics (typed patterns, slot definitions, etc.) are Pass 003 scope.

### Noteworthy decisions

- Raw S-expression bodies (`lhs_raw`, `rhs_raw`, `slots_raw`, `facts_raw`) are the Pass 002 approach. Pass 003 will add typed AST fields alongside or replacing these.
- Edit distance threshold of 2 for keyword suggestions balances helpfulness vs false positives.
- The `InterpretResult` type uses a struct with both `constructs` and `errors` fields rather than `Result<Vec<Construct>, Vec<InterpretError>>`, since classic mode can produce both constructs and errors in the same pass.

### Suggestions

- None. Ready for Pass 003.

---

## Pass 003: Stage 2 Deftemplate Defrule And Deffacts Interpretation

### What was done

1. **Typed AST types** (`ferric-parser/src/stage2.rs`):
   - `Pattern` enum: `Ordered`, `Template`, `Not`, `Test`, `Exists`, `Assigned`
   - `OrderedPattern`: relation name + constraints vector
   - `TemplatePattern`: template name + slot constraints vector
   - `SlotConstraint`: slot name + constraint
   - `Constraint` enum: `Literal`, `Variable`, `MultiVariable`, `Wildcard`, `MultiWildcard`, `Not`, `And`, `Or`
   - `LiteralValue` and `LiteralKind` (Integer, Float, String, Symbol)
   - `Action`, `FunctionCall`, `ActionExpr` for RHS actions
   - `SlotDefinition`, `SlotType` (Single/Multi), `DefaultValue` for deftemplate
   - `FactBody` (Ordered/Template), `OrderedFactBody`, `TemplateFactBody`, `FactSlotValue`, `FactValue` for deffacts

2. **Full interpretation replacing raw fields**:
   - `RuleConstruct`: `lhs_raw`/`rhs_raw` replaced with `patterns: Vec<Pattern>` and `actions: Vec<Action>`
   - `TemplateConstruct`: `slots_raw` replaced with `slots: Vec<SlotDefinition>`
   - `FactsConstruct`: `facts_raw` replaced with `facts: Vec<FactBody>`
   - Pattern interpretation: handles ordered patterns, template patterns, `not`, `test`, `exists`, assigned patterns (`?var <- ...`)
   - Constraint interpretation: handles variables, wildcards, literals, connectives (`&`, `|`, `~`)
   - Action interpretation: function calls with nested arguments
   - Slot interpretation: `(slot ...)`, `(multislot ...)`, `(default ...)`, `(default ?DERIVE)`, `(default ?NONE)`, `(type ...)`, `(range ...)`, `(allowed-values ...)`
   - Fact body interpretation: ordered and template fact bodies with literal/variable values

3. **Loader Stage 2 integration** (`ferric-runtime/src/loader.rs`):
   - `LoadResult` updated: `rules: Vec<RuleConstruct>`, added `templates: Vec<TemplateConstruct>`
   - `load_str()` separates assert forms from constructs, routes constructs through Stage 2
   - Added `LoadError::Interpret(String)` variant
   - New methods: `process_deffacts_construct()`, `process_fact_body()`, `process_ordered_fact_body()`, `process_template_fact_body()`, `fact_value_to_value()`, `literal_to_value()`
   - Kept `(assert ...)` as Phase 1 direct-assertion path
   - `deffunction` goes through loader's unsupported form handling

4. **Re-exports updated**: All new types re-exported from `ferric-parser` and `ferric-runtime` crate roots.

5. **~48 new parser tests** covering:
   - Ordered patterns with variables, literals, wildcards, multi-variables
   - Template patterns with slot constraints
   - Negation, test, and exists patterns
   - Actions with nested function calls
   - Template slots with defaults (`?DERIVE`, `?NONE`, literal)
   - Deffacts with ordered and template fact bodies
   - Comprehensive CLIPS example (full defrule with template patterns, multiple actions)
   - Error cases: empty patterns, empty actions, invalid slot keywords

6. **Existing tests updated**: Integration tests use `.patterns.len()` and `.actions.len()` instead of old raw fields. Loader tests check `LoadError::Interpret(_)` for Stage 2 errors.

### Test results

- **314 tests pass** (148 core + 115 parser + 47 runtime + 1 facade + 3 doctests)
- **0 clippy warnings**
- **~48 new tests** (Stage 2 typed interpretation)
- **1 new loader test** (`load_deftemplate`)
- **0 regressions**

### Remaining TODOs

None — typed construct AST and full interpretation complete for defrule, deftemplate, and deffacts.

### Noteworthy decisions

- Raw `_raw` fields fully replaced by typed fields (not kept alongside).
- `Test` pattern variant keeps a raw `SExpr` body since test expression evaluation is Phase 3+ scope.
- Constraint connectives (`&`, `|`, `~`) are parsed from Stage 1 `Connective` atoms, maintaining the two-stage parse architecture.
- Template fact bodies in deffacts produce warnings (not errors) for unsupported features like variables, allowing graceful degradation.
- `LoadResult` now carries `Vec<RuleConstruct>` and `Vec<TemplateConstruct>` directly, replacing the old `Vec<RuleDef>` for rules.

### Suggestions

- None. Ready for Pass 004.

---

## Pass 004: Rule Compilation Pipeline And Node Sharing

### What was done

1. **`ReteCompiler`** (`ferric-core/src/compiler.rs`):
   - `CompilableRule` and `CompilablePattern` input types (decoupled from parser crate)
   - `CompileResult` and `CompileError` output types
   - `AlphaPathKey` for canonical alpha path caching and node sharing
   - `ReteCompiler::compile_rule()`: builds alpha paths → beta join chain → terminal node
   - `ReteCompiler::ensure_alpha_path()`: find-or-create with cache for alpha memory reuse
   - `ReteCompiler::allocate_rule_id()`: sequential rule ID allocation starting from 1
   - Variable binding tracking: `HashSet<Symbol>` tracks variables bound in previous patterns; first occurrence binds, subsequent occurrences create `JoinTest`

2. **Engine integration** (`ferric-runtime/src/engine.rs`):
   - Added `ReteNetwork` and `ReteCompiler` fields to `Engine`
   - Added `rete()` accessor for test inspection

3. **Loader compilation wiring** (`ferric-runtime/src/loader.rs`):
   - Added `LoadError::Compile` variant
   - `load_str()` now automatically compiles loaded rules into the engine's rete
   - `compile_rule_construct()` orchestrates translation + compilation
   - `translate_rule_construct()` converts `RuleConstruct` → `CompilableRule`
   - `translate_pattern()` handles `Pattern::Ordered` → `CompilablePattern` (other pattern types deferred)
   - `translate_constraint()` handles Literal→ConstantTest, Variable→variable_slot, Wildcard→skip, Not(Literal)→NotEqual, And→recursive
   - `literal_to_atom_key()` converts `LiteralKind` → `AtomKey` for constant tests

4. **15 compiler unit tests** covering:
   - Sequential rule ID allocation
   - Empty rule error
   - Single/multi-pattern compilation
   - Constant test extraction (Equal and NotEqual)
   - Variable binding and join test generation
   - Alpha path sharing (cache reuse for identical patterns)
   - Deterministic compilation output
   - Beta network structure validation (parent chain, alpha memory links)
   - Three-pattern rule with variable binding across all three patterns

5. **10 Phase 2 integration tests** (`phase2_integration_tests.rs`):
   - End-to-end: load rule → assert facts → verify activations
   - Constant test filtering through compiled rete
   - Alpha path sharing across two rules
   - Multi-pattern rule compilation (structure test, binding deferred to Pass 005)
   - Retraction cleanup through compiled rete
   - Multiple facts / multiple activations
   - Deffacts + compiled rules interaction
   - NotEqual constant test via compiler API

### Test results

- **339 tests pass** (163 core + 115 parser + 57 runtime + 1 facade + 3 doctests)
- **0 clippy warnings**
- **25 new tests** (15 compiler + 10 integration)
- **0 regressions**

### Remaining TODOs

- Stage 2 interpreter does not yet combine bare connective tokens (`~red` parsed as `~` + `red`) into `Constraint::Not`. This means `~literal` constraint syntax doesn't work end-to-end through the parser. The compiler-level `NotEqual` path is tested directly. Fix planned for a future pass.

### Noteworthy decisions

- `CompilableRule`/`CompilablePattern` are defined in ferric-core (not parser-dependent). The runtime translates from parser types. This preserves the clean crate dependency graph: parser → core, runtime → core + parser.
- Alpha path sharing uses full-path matching: `(entry_type, Vec<ConstantTest>)` → `AlphaMemoryId`. Partial path sharing (sharing intermediate test nodes across paths with a common prefix) is a future optimization.
- `RuleId` allocation starts from 1 (0 is reserved for potential special use).
- Join tests for variable bindings are correctly emitted by the compiler, but the Phase 1 rete doesn't extract bindings into tokens during right activation. Pass 005 will add binding extraction to complete the join pipeline.
- Template patterns, Or constraints, and multi-field variables are not yet compiled (logged as "not yet supported" — no error, just silently skipped).

### Suggestions

- ~~Pass 005 (Join Binding Extraction) should add binding extraction during token creation in `right_activate`.~~ Done in Pass 005.
- A follow-up pass should fix the Stage 2 interpreter to handle bare connective sequences (`~`, `&`, `|` followed by operands) by looking ahead in the constraint atom list.

---

## Pass 005: Join Binding Extraction And Left Activation Completion

### What was done

1. **Binding field on join nodes** (`ferric-core/src/beta.rs`):
   - Added `bindings: Vec<(SlotIndex, VarId)>` field to `BetaNode::Join`
   - Updated `create_join_node` signature to accept bindings parameter
   - All existing callers updated to pass `vec![]` where no bindings are needed

2. **Compiler binding extraction** (`ferric-core/src/compiler.rs`):
   - Modified compilation loop to distinguish new variables (→ binding extractions) from previously-bound variables (→ join tests)
   - Binding extractions passed to `create_join_node` alongside join tests

3. **Right activation binding extraction** (`ferric-core/src/rete.rs`):
   - When creating tokens in `right_activate`, extract slot values from facts using the join node's `bindings` list
   - Root-parent tokens get fresh `BindingSet` populated from fact slots
   - Non-root tokens inherit parent bindings then add new extractions

4. **Left activation** (`ferric-core/src/rete.rs`):
   - Added `left_activate` method: when a parent token is propagated to a child join node, check all facts in the child's alpha memory against the token
   - Updated `propagate_token` to call `left_activate` for child join nodes (previously skipped with "wait for right activation" comment)
   - Left activation creates child tokens with inherited + new bindings, just like right activation

5. **Test helper updates** (`ferric-runtime/src/test_helpers.rs`):
   - All `build_*_rete` helpers updated for new `create_join_node` signature

6. **New tests**:
   - 4 rete tests: two-pattern binding extraction, left activation order independence, binding value verification, retraction correctness with bindings
   - Updated Phase 2 integration test `multi_pattern_rule_compiles_into_join_chain` to verify actual join filtering (now passes: exactly 1 activation for alice→bob→carol chain)

### Test results

- **343 tests pass** (167 core + 115 parser + 57 runtime + 1 facade + 3 doctests)
- **0 clippy warnings**
- **4 new core rete tests** + 1 updated integration test
- **0 regressions**

### Remaining TODOs

None — positive-pattern join semantics are complete for the Phase 2 subset.

### Noteworthy decisions

- Binding extractions are `Vec<(SlotIndex, VarId)>` on the join node itself, not in a separate data structure. This keeps binding metadata close to where it's used during token creation.
- Left activation symmetry: left and right activation both produce the same tokens. The order of fact assertion doesn't affect the final activation set.
- Borrow checker pattern: clone parent token data and collect alpha memory fact IDs before mutation loops to avoid aliasing issues.
- `Rc<Value>` is used for binding values via `BindingSet::set(var_id, Rc::new(value.clone()))`, consistent with the Phase 1 `ValueRef = Rc<Value>` design.

### Suggestions

- None. Core positive-rule matching semantics are complete. Ready for Pass 006 (Negative Nodes).

---

## Pass 006: Negative Node (Single Pattern) And Blocker Tracking

### What was done

1. **`NegativeMemory`** (`ferric-core/src/negative.rs`):
   - Full implementation of blocker tracking data structure
   - Forward index: `blocked: HashMap<TokenId, HashSet<FactId>>` (parent token → blocking facts)
   - Reverse index: `fact_to_blocked: HashMap<FactId, HashSet<TokenId>>` (fact → blocked tokens)
   - Unblocked tracking: `unblocked: HashMap<TokenId, TokenId>` (parent → pass-through token)
   - Methods: `add_blocker`, `remove_blocker` (returns bool), `is_blocked`, `tokens_blocked_by`, `set_unblocked`, `get_passthrough`, `remove_unblocked`, `is_unblocked`, `remove_parent_token`, `is_empty`, `blocked_count`, `unblocked_count`, `iter_unblocked`, `debug_assert_consistency`
   - 12 unit tests + 2 property-based tests

2. **`BetaNode::Negative` variant** (`ferric-core/src/beta.rs`):
   - New variant: `Negative { parent, alpha_memory, tests, memory, neg_memory, children }`
   - `BetaNetwork` additions: `neg_memories` map, `next_neg_memory_id`, `alpha_to_negatives` index
   - `create_negative_node(parent, alpha_memory, tests)` → `(NodeId, BetaMemoryId, NegativeMemoryId)`
   - Accessors: `get_neg_memory`, `get_neg_memory_mut`, `negative_nodes_for_alpha`, `neg_memory_ids`
   - Updated `attach_child_to_parent` and `debug_assert_consistency` for Negative variant

3. **Runtime negative node handling** (`ferric-core/src/rete.rs`):
   - `negative_left_activate`: parent token arrives at negative node → check alpha memory for blockers. If none, create pass-through token and propagate. If any match, block.
   - `negative_right_activate`: new fact arrives at negative node → check unblocked tokens for matches. If match, block (cascade-retract pass-through, move to blocked state).
   - `negative_handle_retraction`: fact retracted → find blocked tokens via reverse index, remove blocker, if fully unblocked create new pass-through and propagate.
   - `retract_token_cascade`: helper for cascade-retracting a token and all descendants with full cleanup (beta memories, agenda, negative memories).
   - `cleanup_negative_memories_for_token`: scan all negative memories to remove references to a retracted token.
   - Updated `assert_fact` to right-activate negative nodes (step 3, after join nodes).
   - Updated `propagate_token` to handle `BetaNode::Negative` children (calls `negative_left_activate`).
   - Updated `retract_fact` signature to accept `fact_base: &FactBase` (needed for negative unblocking). All callers updated.
   - Updated `find_memory_for_node` to handle `BetaNode::Negative`.

4. **Compiler support** (`ferric-core/src/compiler.rs`):
   - Added `negated: bool` field to `CompilablePattern`
   - `compile_rule` creates `Negative` nodes for `negated: true` patterns, `Join` nodes otherwise
   - 2 new compiler tests: `test_negated_pattern_creates_negative_node`, `test_negated_pattern_with_join_test`

5. **Loader support** (`ferric-runtime/src/loader.rs`):
   - `translate_pattern` handles `Pattern::Not`: unwraps inner pattern, sets `negated = true`
   - Existing ordered pattern compilation gets `negated: false`

6. **Alpha network addition** (`ferric-core/src/alpha.rs`):
   - Added `memories_containing_fact(fact_id)` method (used by `retract_fact` to determine which alpha memories to check for negative unblocking)

7. **New tests** (21 total for this pass):
   - 11 rete unit tests: no-blocking activation, blocking suppression, retract-unblock, assert-reblock, block-unblock cycle, multiple blockers, positive retract cleanup, multiple positive facts, variable-selective blocking, non-matching exclusion, full lifecycle
   - 8 integration tests: end-to-end via CLIPS syntax — fires when no blocker, blocked when exists, unblocked by retraction, block/unblock cycle, shared variable selective blocking, non-matching exclude, multiple blockers, positive retract cleanup
   - 2 compiler tests: negative node creation, negative with join test

### Test results

- **376 tests pass** (193 core + 115 parser + 65 runtime + 3 doctests)
- **0 clippy warnings**
- **21 new tests** (11 rete + 8 integration + 2 compiler)
- **0 regressions**

### Remaining TODOs

- `cleanup_negative_memories_for_token` scans all negative memories linearly. For large networks this could be optimized with a reverse index from `TokenId` to `NegativeMemoryId`, but current cost is acceptable for correctness.
- The `retract_fact` API change (added `fact_base: &FactBase` parameter) is a minor breaking change. All callers within the workspace have been updated.

### Noteworthy decisions

- **Pass-through token architecture**: When a parent token passes through an unblocked negative node, a new "pass-through" token is created that copies the parent's facts and bindings, with `parent = parent_token_id` and `owner_node = negative_node_id`. This enables cascade retraction to work correctly when blocking occurs — the pass-through and all its descendants are cleanly removed.
- **Dual-index blocker tracking**: Forward (`parent → facts`) and reverse (`fact → parents`) indices enable O(1) lookup in both directions. The reverse index is critical for efficient `negative_handle_retraction` (finding which tokens to unblock when a fact is retracted).
- **Retraction ordering**: In `retract_fact`, positive tokens are cascade-removed first (steps 1-4), then negative unblocking is handled (step 6), then alpha memories are cleaned (step 7). This ensures negative nodes see the correct alpha memory state during unblocking.
- **`retract_fact` signature change**: Added `fact_base: &FactBase` parameter, consistent with `assert_fact` which already takes it. Needed because `negative_handle_retraction` must check alpha memories and evaluate join tests to create pass-through tokens.
- **No binding extraction for negated patterns**: Negated patterns create join tests for previously-bound variables but do NOT extract new bindings (variables that appear only in a negated pattern can't contribute bindings to downstream tokens, since the negated fact doesn't exist in the match). The compiler correctly skips binding extractions for negated patterns.

### Suggestions

- None. Ready for Pass 007 (Agenda Strategy Depth And Breadth).

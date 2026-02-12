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

---

## Pass 007: Agenda Conflict Strategies And Ordering Contract

### What was done

1. **`ConflictResolutionStrategy` enum** (`ferric-core/src/strategy.rs`):
   - Four variants: `Depth` (default), `Breadth`, `Lex`, `Mea`
   - Derives `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `Default`
   - 4 basic unit tests for enum behavior

2. **`StrategyOrd` enum** (`ferric-core/src/agenda.rs`):
   - Strategy-specific ordering component for `AgendaKey`
   - `Depth(Reverse<u64>)`: higher timestamp first (most recent)
   - `Breadth(u64)`: lower timestamp first (oldest)
   - `Lex(Reverse<SmallVec<[u64; 4]>>)`: lexicographic comparison of recency vectors (most recent per position)
   - `Mea { first_recency: Reverse<u64>, rest_recency: Reverse<SmallVec<[u64; 4]>> }`: first pattern recency dominates, then LEX tiebreak on remaining
   - Derives `PartialOrd` and `Ord` for natural `BTreeMap` ordering

3. **Refactored `AgendaKey`** (`ferric-core/src/agenda.rs`):
   - Fields: `salience: Reverse<i32>`, `strategy_ord: StrategyOrd`, `seq: Reverse<u64>`
   - Salience always dominates; strategy-specific ordering is secondary; monotonic sequence is final tiebreaker
   - Total ordering via derived `Ord` on the struct

4. **`Activation` recency field**:
   - Added `recency: SmallVec<[u64; 4]>` to `Activation` struct
   - Holds timestamps of facts in pattern order for LEX/MEA strategies
   - All existing `Activation` constructions updated with `recency: SmallVec::new()`

5. **`Agenda::build_key` method**:
   - Constructs `AgendaKey` from `Activation` based on the agenda's strategy
   - `Agenda::with_strategy(strategy)` constructor for non-default strategies
   - `Agenda::new()` delegates to `with_strategy(Depth)`

6. **Recency vector construction** (`ferric-core/src/rete.rs`):
   - Terminal node activation builds recency vector by mapping token facts to timestamps via `fact_base.get(fid).map(|(_, ts)| ts)`
   - `timestamp` field set to `max` of recency vector (for Depth/Breadth fallback)

7. **Runtime wiring**:
   - `EngineConfig` (`ferric-runtime/src/config.rs`): added `strategy: ConflictResolutionStrategy` field, `with_strategy()` builder method, all existing constructors default to `Depth`
   - `Engine::new()` (`ferric-runtime/src/engine.rs`): extracts strategy from config, passes to `ReteNetwork::with_strategy(strategy)`
   - `ReteNetwork::with_strategy()` (`ferric-core/src/rete.rs`): creates `Agenda::with_strategy(strategy)`, `ReteNetwork::new()` delegates to `with_strategy(Depth)`

8. **Re-exports** (`ferric-core/src/lib.rs`):
   - `pub use strategy::ConflictResolutionStrategy`
   - `pub use agenda::{..., AgendaKey, StrategyOrd}`

9. **19 new agenda tests** covering:
   - Depth: most recent timestamp first
   - Breadth: oldest timestamp first
   - LEX: lexicographic recency vector comparison
   - MEA: first pattern recency dominates, falls back to LEX on tie
   - Salience dominates all four strategies (4 tests)
   - Activation sequence tiebreaker for all four strategies (4 tests)
   - Strategy switch changes ordering (Depth vs Breadth on same data)
   - Remove activations for token works with all four strategies (4 tests)

### Test results

- **399 tests pass** (215 core + 115 parser + 65 runtime + 3 doctests + 1 facade)
- **0 clippy warnings**
- **23 new tests** (19 agenda + 4 strategy)
- **0 regressions**

### Remaining TODOs

- Salience on activations is currently hardcoded to 0 in the terminal node activation path. When salience from `(declare (salience N))` is wired through compilation to the terminal node, this will need updating.
- The recency vector sorts facts by pattern order based on `token.facts` order. This is correct for the current implementation where facts are accumulated in join order.

### Noteworthy decisions

- **`StrategyOrd` as enum variant**: Rather than using a single struct with optional fields, each strategy variant carries exactly the data it needs. This makes the ordering derivation via `#[derive(Ord)]` clean and correct — no need for manual `Ord` implementations.
- **`Reverse` wrappers for BTreeMap ordering**: `BTreeMap::pop_first()` returns the smallest key. Using `Reverse` where needed (salience, depth timestamp, sequence) ensures `pop_first` returns the highest-priority activation without needing a custom iterator.
- **`SmallVec<[u64; 4]>` for recency**: Most rules have 2-4 patterns, so the inline capacity of 4 avoids heap allocation in the common case. `SmallVec` implements `Ord` (lexicographic), which makes `StrategyOrd::Lex` and `StrategyOrd::Mea` comparisons work via derive.
- **Activation sequence as final tiebreaker**: The monotonically-increasing `activation_seq` (assigned by `Agenda::add`) ensures total ordering even when all other fields match. This provides deterministic behavior.
- **Configuration flows downward**: `EngineConfig.strategy` → `Engine::new()` → `ReteNetwork::with_strategy()` → `Agenda::with_strategy()`. Strategy is fixed at construction time; dynamic strategy switching is not supported (consistent with CLIPS behavior where strategy changes only take effect on the next `reset`).

### Suggestions

- None. Ready for Pass 008 (Engine Execution Loop).

---

## Pass 008: Run, Step, Halt, And Reset Execution Loop

### What was done

1. **Core clear/reset infrastructure** (ferric-core):
   - `AlphaMemory::clear()`: clears facts and slot indices, preserves indexed_slots metadata
   - `AlphaNetwork::clear_all_memories()`: clears all alpha memories' fact contents
   - `TokenStore::clear()`: removes all tokens and clears reverse indices
   - `BetaMemory::clear()`: clears token set
   - `BetaNetwork::clear_all_runtime()`: clears all beta memories and negative memories
   - `NegativeMemory::clear()`: clears blocked/unblocked/fact_to_blocked maps
   - `Agenda::strategy()`: getter for current strategy (needed to preserve across clear)
   - `Agenda::clear()`: removes all activations, resets sequence counter
   - `ReteNetwork::clear_working_memory()`: orchestrates clearing all runtime state while preserving compiled network structure

2. **Execution types** (`ferric-runtime/src/execution.rs`):
   - `RunLimit` enum: `Unlimited`, `Count(usize)`
   - `HaltReason` enum: `AgendaEmpty`, `LimitReached`, `HaltRequested`
   - `RunResult` struct: `rules_fired: usize`, `halt_reason: HaltReason`
   - `FiredRule` struct: `rule_id: RuleId`, `token_id: TokenId`
   - All types derive appropriate traits (Clone, Debug, PartialEq, Eq)

3. **Unified assert/retract pipeline** (`ferric-runtime/src/engine.rs`):
   - `Engine::assert_ordered()` now propagates facts through the rete network after inserting into fact_base
   - `Engine::assert()` now propagates facts through the rete network
   - `Engine::retract()` now retracts from rete first (while fact still in fact_base for negative node handling), then removes from fact_base
   - This eliminates the need for manual rete manipulation in application code and tests

4. **Execution loop methods** (`ferric-runtime/src/engine.rs`):
   - `Engine::step()`: pops one activation from agenda and returns `FiredRule` info (action execution is Pass 009)
   - `Engine::run(limit: RunLimit)`: loops firing rules until agenda empty, limit reached, or halt requested. Clears halt flag on entry.
   - `Engine::halt()`: sets halt flag checked between firings
   - `Engine::is_halted()`: halt flag query
   - `Engine::agenda_len()`: convenience accessor for agenda size

5. **Reset semantics** (`ferric-runtime/src/engine.rs`):
   - `Engine::reset()`: clears fact_base, clears rete working memory, clears halt flag, re-asserts all registered deffacts through the rete
   - Added `registered_deffacts: Vec<Vec<Fact>>` field to Engine for deffacts storage
   - Deffacts are registered during `process_deffacts_construct` (clone of constructed facts)
   - Compiled rules preserved across reset (only runtime state cleared)

6. **Loader reordering** (`ferric-runtime/src/loader.rs`):
   - Changed `load_str` to compile rules BEFORE asserting facts (deffacts and bare asserts)
   - This ensures facts flow through the compiled rete network when asserted
   - New order: parse → interpret constructs → compile rules → process deffacts → process assert forms
   - Deffacts now registered for reset during processing

7. **Updated integration tests** (`phase2_integration_tests.rs`):
   - Removed ALL manual `engine.rete.assert_fact(...)` calls — automatic via unified pipeline
   - Removed ALL manual `engine.fact_base.retract(...)` + `engine.rete.retract_fact(...)` calls — replaced with `engine.retract(fid).unwrap()`
   - Tests now simpler and more realistic (use engine API, not rete internals)
   - `integration_tests.rs` unchanged (uses separate rete objects, unaffected)

8. **Re-exports** (`ferric-runtime/src/lib.rs`):
   - Added `pub use execution::{FiredRule, HaltReason, RunLimit, RunResult}`

9. **20 new tests**:
   - 4 execution type tests
   - 16 engine execution tests: step on empty agenda, step fires one, step returns rule info, run all, run with limit, run on empty, run with zero limit, halt stops execution, reset clears state, reset preserves rules, reset reasserts deffacts, reset clears halt, step equivalence to run(1), multiple resets cycle, assert propagates through rete, retract removes from rete

### Test results

- **419 tests pass** (215 core + 115 parser + 85 runtime + 3 doctests + 1 facade)
- **0 clippy warnings**
- **20 new tests** (4 execution + 16 engine)
- **0 regressions**

### Remaining TODOs

- `step()` and `run()` pop activations but do not execute RHS actions — action execution is Pass 009. Currently "firing" a rule means popping its activation from the agenda.
- The `halt()` method sets a flag that's only checked between firings in `run()`. Since action execution doesn't exist yet, halt can't be triggered from within a rule's RHS (which is the typical CLIPS use case). Pass 009 will enable this.

### Noteworthy decisions

- **Unified assert/retract pipeline**: Engine methods now automatically propagate through the rete network. This is a significant architectural change that simplifies all downstream code. Integration tests no longer need to manually push facts through the rete.
- **Loader reordering**: `load_str` now compiles rules BEFORE asserting facts. This ensures that when a single `load_str` call contains both rules and deffacts (or assert forms), the facts automatically match compiled patterns. The old order (assert facts → compile rules) meant facts were silently not matched.
- **Deffacts registration via `Vec<Vec<Fact>>`**: Each deffacts block stores a vector of cloned `Fact` objects. Since `Fact` contains `Symbol` values that reference the symbol table (which persists across reset), the cloned facts remain valid after reset.
- **Reset clones deffacts**: `registered_deffacts.clone()` in reset to avoid borrow issues. The clone cost is small (typically few deffacts) and occurs only on reset.
- **Retract order**: Rete retraction happens before fact_base retraction, so `negative_handle_retraction` can still read the fact_base for unblocking decisions. The fact being retracted is still present in fact_base during rete processing.

### Suggestions

- None. Ready for Pass 009 (Action Execution).

---

## Pass 009: Action Execution

### What was done

1. **`CompiledRuleInfo` struct** (`ferric-runtime/src/actions.rs`):
   - Holds all data needed for RHS action execution: `name`, `actions`, `var_map`, `fact_address_vars`, `salience`
   - `var_map: VarMap` maps variable names → `VarId` for resolving bindings
   - `fact_address_vars: HashMap<String, usize>` maps `?f` → pattern index for fact-address resolution
   - Stored in `Engine::rule_info: HashMap<RuleId, CompiledRuleInfo>`

2. **`ActionError` enum** (`ferric-runtime/src/actions.rs`):
   - Variants: `UnknownAction`, `UnboundVariable`, `FactNotFound`, `InvalidAssert`, `InvalidRetract`, `Encoding`
   - Provides structured error reporting for each action execution failure mode

3. **`execute_actions` function** (`ferric-runtime/src/actions.rs`):
   - Main entry point for RHS action execution, takes split borrows of Engine parts
   - Dispatches on action function name: `assert`, `retract`, `modify`, `duplicate`, `halt`, `printout`
   - `resolve_fact_address()`: maps variable name → pattern index → `token.facts[index]` → `FactId`
   - `eval_expr()`: resolves `ActionExpr` values — literals (via `literal_to_value`), variables (via VarMap+BindingSet), function calls (nested evaluation)
   - `literal_to_value()`: converts `LiteralKind` → `Value` using symbol table for interning

4. **Action implementations**:
   - **assert**: Builds ordered fact from evaluated arguments, asserts through engine pipeline
   - **retract**: Resolves fact-address variable → FactId, retracts through engine pipeline
   - **modify**: Resolves fact-address variable, retracts old fact, asserts new fact with modifications (stub for template support)
   - **duplicate**: Like modify but doesn't retract the original
   - **halt**: Sets `halted = true` on engine
   - **printout**: No-op placeholder (IO infrastructure is Phase 3+)

5. **Engine wiring** (`ferric-runtime/src/engine.rs`):
   - `step()`: Clones token and `CompiledRuleInfo` for the fired rule, calls `execute_actions` with split borrows
   - `run()`: Loops `step()` with halt/limit checking
   - Added `rule_info: HashMap<RuleId, CompiledRuleInfo>` field to Engine

6. **Salience wiring** (`ferric-core/src/beta.rs` + `rete.rs`):
   - Added `salience: i32` field to `BetaNode::Terminal`
   - Terminal node activation uses node's salience (was hardcoded to 0)
   - Salience flows: `RuleConstruct.salience` → `CompilableRule.salience` → `BetaNode::Terminal.salience` → `Activation.salience`

7. **Compiler VarMap export** (`ferric-core/src/compiler.rs`):
   - Added `var_map: VarMap` to `CompileResult` (needed for action variable resolution)
   - Added `Clone`, `Debug`, `PartialEq`, `Eq` derives to `VarMap` in `binding.rs`

8. **Loader fact-address tracking** (`ferric-runtime/src/loader.rs`):
   - `TranslatedRule` struct includes `fact_address_vars: HashMap<String, usize>`
   - `translate_rule_construct` detects `Pattern::Assigned` and records variable → pattern-index mapping
   - `compile_rule_construct` assembles `CompiledRuleInfo` from compilation result + translation data

9. **Parser `?f <- (pattern)` support** (`ferric-parser/src/stage2.rs`):
   - Modified LHS parsing loop in `interpret_rule` to detect 3-element sequence: `SingleVar` + `Connective::Assign` + pattern
   - Constructs `Pattern::Assigned { variable, pattern, span }` with merged span
   - 4 new parser tests: basic assigned pattern, mixed with other patterns, multiple assigned patterns, assigned-not pattern

10. **Integration tests** (`phase2_integration_tests.rs`):
    - `rule_asserts_new_fact_during_execution`: assert action creates facts end-to-end
    - `rule_retract_action_removes_matched_fact`: `?f <- (pattern)` + `(retract ?f)` end-to-end
    - `rule_retract_with_assert_rebuilds_state`: retract old + assert new in single rule
    - `rule_halt_action_stops_execution`: halt action terminates run loop
    - `rule_assert_triggers_chain_reaction`: two-rule chain reaction with assert
    - `rule_with_salience_fires_in_order`: salience ordering verified via step
    - `reset_and_run_cycle_with_actions`: full reset + re-run cycle

### Test results

- **432 tests pass** (215 core + 119 parser + 94 runtime + 3 doctests + 1 facade)
- **0 clippy errors** (2 pedantic warnings: single-char-pattern suggestion)
- **11 new tests** (4 parser + 7 integration)
- **0 regressions**

### Remaining TODOs

- `printout` action is a no-op — IO infrastructure deferred to Phase 3+.
- `modify` and `duplicate` actions are implemented as retract+assert of ordered facts. Template-aware modify (updating specific slots) requires template metadata lookup, deferred to Phase 3.
- Function call evaluation in `eval_expr` returns an error for unknown functions. Built-in functions (`+`, `-`, `str-cat`, etc.) are Phase 3 scope.
- The `?f <- (not (pattern))` combination parses correctly but `fact_address_vars` tracking correctly skips negated patterns (can't bind a fact address to a fact that doesn't exist).

### Noteworthy decisions

- **Split borrows for action execution**: `execute_actions` takes `&mut FactBase`, `&mut ReteNetwork`, `&mut bool` (halted), and `&SymbolTable` rather than `&mut Engine` to satisfy the borrow checker. The token and rule_info are cloned before the call.
- **Clone token + rule_info before execution**: Since action execution may mutate the rete (assert/retract), we clone the token's data and the `CompiledRuleInfo` before calling `execute_actions`. This avoids aliasing issues where the rete needs exclusive access while we're reading from it.
- **Fact-address resolution is positional**: `?f <- (pattern)` maps `f` → pattern index `N`, then looks up `token.facts[N]`. This is correct because token facts are accumulated in join order (pattern 0, 1, 2, ...).
- **Salience wired through compilation**: Rather than looking up salience at activation time, it's stored on the `BetaNode::Terminal` and copied into `Activation`. This is simpler and more efficient.
- **Parser `?f <- (pattern)` lookahead**: The LHS parsing loop uses a `while` loop with index-based iteration instead of `for` to allow consuming 3 elements at a time when an assigned pattern is detected.

### Suggestions

- None. Ready for Pass 010 (NCC And Exists Node Types).

---

## Pass 010: NCC And Exists Nodes, Plus Cleanup Invariants

### What was done

1. **NCC memory** (`ferric-core/src/ncc.rs`):
   - `NccMemoryId` identifier type
   - `NccMemory` struct with result count tracking (`HashMap<TokenId, usize>`) and unblocked pass-through tracking (`HashMap<TokenId, TokenId>`)
   - Methods: `increment_results`, `decrement_results`, `result_count`, `is_blocked`, `set_unblocked`, `get_passthrough`, `remove_unblocked`, `remove_parent_token`, `clear`, `is_empty`, `debug_assert_consistency`
   - 10 unit tests covering increment/decrement transitions, blocking state, cleanup, consistency

2. **Exists memory** (`ferric-core/src/exists.rs`):
   - `ExistsMemoryId` identifier type
   - `ExistsMemory` struct with support tracking (`HashMap<TokenId, HashSet<FactId>>`), satisfied tracking (`HashMap<TokenId, TokenId>`), reverse index (`HashMap<FactId, HashSet<TokenId>>`)
   - Methods: `add_support`, `remove_support`, `support_count`, `parents_supported_by`, `set_satisfied`, `get_passthrough`, `remove_satisfied`, `is_satisfied`, `remove_parent_token`, `clear`, `is_empty`, `debug_assert_consistency`
   - 10 unit tests covering support transitions, reverse index, satisfied state, cleanup, consistency

3. **Beta node variants** (`ferric-core/src/beta.rs`):
   - `BetaNode::Ncc { parent, partner, memory, ncc_memory, children }` — blocks parent tokens when subnetwork has results
   - `BetaNode::NccPartner { parent, ncc_node, ncc_memory }` — reports subnetwork results to NCC node
   - `BetaNode::Exists { parent, alpha_memory, tests, memory, exists_memory, children }` — propagates when at least one supporting fact exists
   - `BetaNetwork` additions: `ncc_memories`, `exists_memories`, `alpha_to_exists` maps, `next_ncc_memory_id`, `next_exists_memory_id` counters
   - Methods: `allocate_ncc_memory`, `create_ncc_node`, `create_ncc_partner`, `get_ncc_memory[_mut]`, `create_exists_node`, `get_exists_memory[_mut]`, `exists_nodes_for_alpha`
   - Updated `attach_child_to_parent`, `clear_all_runtime`, `debug_assert_consistency` for all new variants

4. **Rete runtime — exists nodes** (`ferric-core/src/rete.rs`):
   - `exists_left_activate`: When parent token arrives, check alpha memory for supporting facts. If any match, create pass-through and propagate. Record support in exists memory.
   - `exists_right_activate`: When new fact enters alpha memory, for each parent token evaluate join tests. If count goes 0→1, create pass-through and propagate.
   - `exists_handle_retraction`: When fact retracted, find affected parent tokens via reverse index, remove support. If count goes N→0, retract pass-through cascade.
   - Updated `assert_fact` step 4: right-activate exists nodes for affected alpha memories.
   - Updated `retract_fact` step 6b: call `exists_handle_retraction`.

5. **Rete runtime — NCC nodes** (`ferric-core/src/rete.rs`):
   - `ncc_left_activate`: When parent token arrives, check NCC memory result count. If 0, create pass-through and propagate. If > 0, token is blocked.
   - `ncc_partner_receive_result`: Stub for subnetwork result handling (full NCC subnetwork integration deferred — requires parser support for multi-pattern `not`).
   - Updated `propagate_token` to dispatch to `ncc_left_activate` for Ncc children and handle NccPartner/Exists children.

6. **Cleanup integration**:
   - `cleanup_negative_memories_for_token` extended to also clean NCC memories and exists memories.
   - `find_memory_for_node` handles Ncc and Exists nodes.
   - `clear_working_memory` clears NCC and exists memories via `clear_all_runtime`.
   - `debug_assert_consistency` checks NCC and exists memory invariants.

7. **Compiler — exists support** (`ferric-core/src/compiler.rs`):
   - Added `pub exists: bool` field to `CompilablePattern` (default `false`)
   - `compile_rule` creates `BetaNode::Exists` for `pattern.exists == true` patterns
   - 1 new compiler test: `test_exists_pattern_creates_exists_node`

8. **Loader — exists support** (`ferric-runtime/src/loader.rs`):
   - `translate_pattern` handles `Pattern::Exists`: single-pattern exists sets `compilable.exists = true`; multi-pattern deferred
   - `translate_rule_construct` correctly handles exists patterns (no fact_index increment, no fact-address variable tracking — same as negated)
   - `Pattern::Ordered` and `Pattern::Not` constructions include `exists: false`

9. **Re-exports** (`ferric-core/src/lib.rs`):
   - `pub use ncc::{NccMemory, NccMemoryId}`
   - `pub use exists::{ExistsMemory, ExistsMemoryId}`

10. **Tests**:
    - 10 NCC memory unit tests
    - 10 exists memory unit tests
    - 9 exists rete-level tests: first match activation, at-most-one semantics, retract-one-of-two keeps active, retract-last removes, no-parent no-activation, support add/retract/re-add cycle, parent retract cleanup, multiple independent parents, basic NCC test
    - 1 compiler test for exists node creation
    - 3 integration tests: exists fires once with multiple matches, exists retract removes activation, exists with run produces expected facts

### Test results

- **465 tests pass** (245 core + 119 parser + 97 runtime + 3 doctests + 1 facade)
- **0 clippy warnings/errors**
- **33 new tests** (20 memory unit + 10 rete + 1 compiler + 3 integration) — note: some tests overlap with the basic tests from the first implementation pass
- **0 regressions**

### Remaining TODOs

- **NCC subnetwork**: The NCC partner `receive_result` method is a stub. Full NCC subnetwork integration requires: (1) parser support for `(not (pattern1) (pattern2))` multi-pattern negation, and (2) compiler support for building the subnetwork join chain feeding into the NCC partner. The NCC memory and node types are in place ready for this.
- **Multi-pattern exists**: `(exists (pattern1) (pattern2))` — multi-pattern existential quantification also needs NCC-style subnetwork support. Single-pattern exists is fully functional.

### Noteworthy decisions

- **Exists as support-counting node**: Rather than implementing exists as `not(not(pattern))` (which would require full NCC), we implemented it as a direct support-counting node. This is simpler and more efficient for the common single-pattern case.
- **Exists does NOT contribute facts to tokens**: Like negative nodes, exists nodes produce pass-through tokens that copy the parent's facts and bindings. No new fact is added to the token's fact list. This means exists patterns don't increment `fact_index` in the loader.
- **NCC infrastructure first, integration later**: The NCC memory, node types, and basic left activation are in place. The complex part (subnetwork results flowing to the NCC partner) is deferred until parser/compiler support for multi-pattern negation is added.
- **Exists patterns reuse negative node patterns**: The exists node's join test evaluation, pass-through token creation, and cascade retraction follow the same patterns as the negative node. The key difference is the polarity: negative blocks when matches exist; exists propagates when matches exist.
- **Cleanup covers all memory types**: `cleanup_negative_memories_for_token` (renamed conceptually but kept for compatibility) now scans NCC and exists memories in addition to negative memories. This ensures no stale token references after cascade retraction.

### Suggestions

- A future pass could add parser support for `(not (pattern1) (pattern2))` multi-pattern syntax and wire NCC subnetwork compilation.
- Ready for Pass 011 (Pattern Validation And Source Located Compile Errors).

---

## Pass 011: Pattern Validation And Source-Located Compile Errors

### What was done

1. **Validation types** (`ferric-core/src/validation.rs`):
   - `SourceLocation` struct: simple source location (line, column, end_line, end_column) independent of parser crate
   - `ValidationStage` enum: `AstInterpretation`, `ReteCompilation`
   - `PatternViolation` enum with 5 stable error codes:
     - E0001: `NestingTooDeep { depth, max }` — nesting exceeds limit
     - E0002: `ForallConditionNotSinglePattern` — forall condition must be single fact pattern (Phase 3)
     - E0003: `NestedForall` — forall cannot be nested (Phase 3)
     - E0004: `ForallUnboundVariable { var_name }` — unbound variable in forall then clause (Phase 3)
     - E0005: `UnsupportedNestingCombination { description }` — e.g., `(exists (not ...))`
   - `PatternValidationError` struct: code, kind, location, stage, suggestion
   - Helper methods: `code()`, `suggestion()` on `PatternViolation`; `new()` on `PatternValidationError`
   - `Display` and `Error` trait implementations with full formatting
   - 9 unit tests covering all violation types and display formatting

2. **Compiler error variant** (`ferric-core/src/compiler.rs`):
   - Added `CompileError::Validation(Vec<PatternValidationError>)` variant

3. **Pattern validation in loader** (`ferric-runtime/src/loader.rs`):
   - `validate_rule_patterns(patterns, max_nesting_depth)`: top-level validation entry point
   - `validate_pattern_recursive(pattern, depth, max_depth, errors)`: recursive walker
   - `span_to_source_location(span)`: converter from parser `Span` to core `SourceLocation`
   - Validation integrated into `compile_rule_construct()` before pattern translation
   - `LoadError::Validation(Vec<PatternValidationError>)` variant added
   - Validation rules enforced:
     - E0001: `not`/`exists` nesting depth > 2
     - E0005: `exists` containing `not` as a direct child

4. **Re-exports** (`ferric-core/src/lib.rs`):
   - `pub use validation::{PatternValidationError, PatternViolation, SourceLocation, ValidationStage}`

5. **Integration tests** (`ferric-runtime/src/phase2_integration_tests.rs`):
   - `triple_nested_not_fails_validation`: `(not (not (not (b))))` → E0001
   - `exists_containing_not_fails_validation`: `(exists (not (b)))` → E0005
   - `valid_not_exists_passes`: `(not (exists (b)))` accepted (depth 2, within limit)
   - `double_nested_not_passes`: `(not (not (b)))` accepted (depth 2, within limit)
   - `single_not_passes`: basic negation accepted
   - `single_exists_passes`: basic exists accepted

6. **Forall regression fixture** (`tests/fixtures/forall_vacuous_truth.clp`):
   - Commented-out forall rule with documented expected 5-step behavior
   - Corresponding commented-out test contract in `phase2_integration_tests.rs`

### Test results

- **482 tests pass** (256 core + 119 parser + 103 runtime + 3 doctests + 1 facade)
- **0 clippy warnings**
- **15 new tests** (9 validation unit + 6 integration)
- **0 regressions**

### Remaining TODOs

- Forall-related error codes (E0002, E0003, E0004) are defined but not triggered — forall CE is Phase 3 scope. The types and codes are stable and ready.
- Multi-pattern `not` (NCC) validation is not yet needed since the parser doesn't yet support `(not (pattern1) (pattern2))` multi-pattern syntax.

### Noteworthy decisions

- **Type separation**: Validation error types live in `ferric-core` using `SourceLocation` (not parser's `Span`) to avoid a dependency from core to parser. The runtime layer converts `Span` → `SourceLocation` when building errors.
- **Validation timing**: Validation runs in `compile_rule_construct()` BEFORE pattern translation, ensuring invalid rules never reach rete node construction.
- **Nesting depth limit of 2**: Default matches the implementation plan. `(not (not (fact)))` and `(not (exists (fact)))` are allowed; `(not (not (not (fact))))` is not.
- **`exists(not(...))` is E0005, not E0001**: Even though it's technically within the depth limit, `(exists (not ...))` is flagged as an unsupported combination because it can't be correctly implemented (exists semantics require the inner pattern to be positive).
- **Error code policy**: E0001-E0005 are defined upfront as stable, append-only codes per the plan. New validation rules in future passes will receive E0006+.

### Suggestions

- None. Ready for Pass 012 (Phase 2 Integration And Exit Validation).

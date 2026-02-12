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

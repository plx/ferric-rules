# Rust Code Writer Memory

## Project Structure: ferric-rules

This is a Cargo workspace with four crates:
- `ferric` — facade crate that re-exports from the others
- `ferric-core` — shared types, fact storage, pattern matching (core engine internals)
- `ferric-parser` — parser (Stage 1 S-expression parser complete, Stage 2 construct interpreter added in Pass 002)
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

## Pass 003 Implementation

Successfully moved shared types from `ferric-runtime` to `ferric-core` and added:
- `encoding.rs` — StringEncoding, EncodingError
- `string.rs` — FerricString
- `symbol.rs` — Symbol, SymbolId, SymbolTable
- `value.rs` — Value, AtomKey, Multifield, ExternalAddress
- `fact.rs` — Fact, FactBase, FactId, TemplateId, OrderedFact, TemplateFact
- `binding.rs` — VarId, VarMap, BindingSet, ValueRef
- `engine.rs` in ferric-runtime — Engine with thread affinity checking

All 97 tests pass, clippy clean with `-D warnings`.

## Pass 005 Implementation

Successfully implemented a minimal source loader connecting parser to engine:
- `loader.rs` in `ferric-runtime/src/` with `LoadError`, `RuleDef`, `LoadResult` types
- `Engine::load_str()` and `Engine::load_file()` methods for loading CLIPS source
- Support for `(assert ...)` forms — converts S-expressions to facts in working memory
- Support for `(defrule ...)` forms — stores raw S-expression structure in `RuleDef` for later compilation
- Support for `(deffacts ...)` forms — treated like batch assert
- Comprehensive error handling with `LoadError` enum (Parse, UnsupportedForm, InvalidAssert, InvalidDefrule, Engine, Io)
- Atom-to-Value conversion respects encoding settings
- 36 loader tests including property-based tests, all passing
- Clippy clean with `-D warnings`

Key design notes:
- Made Engine fields `pub(crate)` for intra-crate access (fact_base, symbol_table, config)
- Made `check_thread_affinity()` `pub(crate)` for reuse in loader
- `process_defrule` doesn't use `self` but kept as method for API consistency
- Parse errors aggregated and returned as vector for multi-error reporting
- Warnings collected for non-fatal issues (encoding errors, unsupported values)

All 186 tests pass workspace-wide, clippy clean.

## Pass 007 Implementation (Phase 2)

Successfully implemented all four conflict resolution strategies with full ordering contract:
- `strategy.rs` — `ConflictResolutionStrategy` enum (Depth, Breadth, Lex, Mea)
- `agenda.rs` — Refactored to support all strategies with `StrategyOrd` enum, `build_key()` method
- Added `recency: SmallVec<[u64; 4]>` field to `Activation` for LEX/MEA strategies
- `rete.rs` — Builds recency vectors (fact timestamps in pattern order) when creating activations
- `config.rs` — Added `strategy` field to `EngineConfig`, builder method `with_strategy()`
- `engine.rs` — Wires strategy from config to `ReteNetwork::with_strategy()`
- 19 comprehensive tests covering all strategies, salience dominance, seq tiebreaking, removal
- All 399 workspace tests pass (215 in ferric-core), clippy clean with `-D warnings`

Key strategy ordering rules:
- **Depth**: Higher salience > Higher timestamp (most recent) > Higher seq
- **Breadth**: Higher salience > Lower timestamp (oldest) > Higher seq
- **LEX**: Higher salience > Lexicographic recency (most recent fact first per position) > Higher seq
- **MEA**: Higher salience > First-pattern recency > LEX tiebreak on rest > Higher seq

CRITICAL: Recency vector built in `rete.rs` when creating Terminal activations by collecting `fact_base.get(fid).timestamp` for each fact in pattern order

## Pass 008 Implementation

Successfully implemented beta network, join operations, and agenda (second stage of Rete):
- `beta.rs` — BetaNode (Root, Join, Terminal), BetaMemory, BetaNetwork, JoinTest, RuleId
- `agenda.rs` — Agenda, Activation, ActivationId, AgendaKey with depth strategy (most recent first)
- `rete.rs` — ReteNetwork integrating alpha + beta + token store + agenda
- Right activation: when facts enter alpha memories, propagate through join nodes
- Join evaluation: compare left token bindings with right fact slot values
- Retraction: cascade remove tokens, clean up beta memories and agenda
- 12 unit tests for beta and agenda, 5 integration tests for full Rete network
- All 248 workspace tests pass, clippy clean with `-D warnings`

Key design notes:
- Beta network root node ID starts at 100,000 to avoid conflicts with alpha node IDs

## Pass 011 Implementation

Successfully implemented pattern validation and source-located compile errors:
- **validation.rs** in `ferric-core/src/` — `SourceLocation`, `ValidationStage`, `PatternViolation`, `PatternValidationError` types
- Error codes E0001-E0005 defined (E0002-E0004 for forall, Phase 3 scope)
- `LoadError::Validation` variant added to runtime error enum
- `CompileError::Validation` variant added to core compiler error enum
- Pattern validation function in `loader.rs` — checks nesting depth (max 2) and unsupported combinations
- E0001: Nesting depth exceeded for `not`/`exists` CEs
- E0005: Unsupported nesting (exists containing not is rejected)
- 6 integration tests in `phase2_integration_tests.rs` covering validation pass/fail cases
- 9 unit tests in `validation.rs` covering error type construction and Display formatting
- Forall regression fixture created at `tests/fixtures/forall_vacuous_truth.clp` with commented test contract
- All 482 workspace tests pass (256 core + 119 parser + 103 runtime + 1 facade + 3 doc), clippy clean

Key design notes:
- `SourceLocation` is a simple struct in core (doesn't depend on parser's Span type)
- Runtime converts `ferric_parser::Span` → `ferric_core::SourceLocation` when creating errors
- Validation happens in `compile_rule_construct` BEFORE pattern translation
- Pattern validation walks recursively, checking depth and nesting combinations inline

## Pass 009 (Phase 2, Pass 005) Implementation

Successfully implemented join binding extraction and left activation:
- **Beta.rs**: Added `bindings: Vec<(SlotIndex, VarId)>` field to `BetaNode::Join`
- **Compiler.rs**: Modified compilation loop to extract bindings for new variables (not seen before) vs. join tests for previously-bound variables
- **Rete.rs**:
  - Right activation now extracts bindings from facts using `get_slot_value` and stores in tokens via `BindingSet::set`
  - Added `left_activate` method: when a new parent token is created, it triggers left activation on child join nodes by iterating through alpha memory facts
  - Both root-parent and non-root-parent cases extract bindings using `Rc::new(value.clone())`
- Updated all callers of `create_join_node` to pass empty `vec![]` for bindings where not needed
- 4 comprehensive tests in `rete.rs`:
  - Two-pattern rule with variable binding and join test filtering
  - Left activation with reverse assertion order (age fact first, then person fact)
  - Binding extraction verification
  - Retraction correctness with variable bindings
- Updated `multi_pattern_rule_compiles_into_join_chain` integration test to verify actual join filtering (1 activation for alice→bob→carol chain)
- All 343 workspace tests pass (1 + 167 + 115 + 57 + 0 + 0 + 2 + 1), clippy clean with `-D warnings`

Key implementation notes:
- **Borrow checker pattern**: Clone parent token facts/bindings BEFORE the iteration loop, collect alpha memory fact IDs into Vec before loop, get fresh parent token reference on each iteration
- `BindingSet::set` takes `Rc<Value>` (ValueRef), so use `std::rc::Rc::new(value.clone())`

## Pass 010 (Phase 2, Pass 006) Implementation

Successfully implemented exists pattern support in compiler and loader:
- **CompilablePattern**: Added `pub exists: bool` field (after `negated: bool`)
- **Compiler.rs**:
  - Updated compilation loop to check `pattern.exists` and create exists nodes via `rete.beta.create_exists_node()`
  - Added `test_exists_pattern_creates_exists_node` compiler test
  - Updated ALL 14+ test constructions of `CompilablePattern` to include `exists: false`
- **Loader.rs**:
  - Added `Pattern::Exists` handling in `translate_pattern()` for single-pattern exists (multi-pattern deferred)
  - Updated `translate_rule_construct()` fact_index tracking to exclude exists patterns (like negated patterns)
  - Updated `Pattern::Ordered` construction to include `exists: false`
- **Rete.rs**: Added 9 comprehensive exists node tests with helper function `build_trigger_with_exists_person()`
  - First match produces activation, second match does not
  - Retract one of two keeps activation, retract last removes activation
  - No parent = no activation
  - Support add/retract/re-add cycle
  - Retract parent cleans up
  - Multiple parents independent
- **Phase2_integration_tests.rs**: Added 3 integration tests for exists behavior
- All 465 workspace tests pass (1 + 245 + 119 + 97 + 0 + 0 + 2 + 1), clippy clean with `-D warnings`

Key notes:
- Exists patterns do NOT increment fact_index (they pass through parent token facts unchanged)
- Exists patterns do NOT allow fact-address variable assignment (similar to negated patterns)
- Single-pattern exists compiles to `BetaNode::Exists`, multi-pattern exists deferred to NCC subnetwork
- Propagate_token updated to call `left_activate` for child join nodes (not just terminals)
- Join nodes now properly bind variables on first occurrence and test them on subsequent occurrences
- Join nodes subscribe to alpha memories via `alpha_to_joins` index for right activation
- Tokens created during join clone parent bindings (variable binding creation deferred to later pass)
- Beta memory cleanup during retraction is inefficient (iterates all memories) but correct for Phase 1
- AgendaKey uses `std::cmp::Reverse` for salience, timestamp, and seq to achieve proper BTreeMap ordering
- `evaluate_join()` helper compares AtomKey values extracted from fact slots and token bindings

## Pass 009 Implementation

Successfully implemented Phase 1 integration and exit validation:
- `integration_tests.rs` in `ferric-runtime` — 5 full-pipeline integration tests (parser → loader → engine → Rete → activation)
- `debug_assert_consistency()` for `AlphaNetwork` — validates memory references, node children, no duplicates, slot index invariants
- `debug_assert_consistency()` for `BetaNetwork` — validates node children, parent references, memory IDs, alpha_to_joins index, root node
- 2 retraction invariant tests in `rete.rs` — assert/retract cycles with consistency checks
- All 258 workspace tests pass, clippy clean with `-D warnings`

Key design notes:
- Integration tests in `ferric-runtime` crate to access `pub(crate)` fields of Engine
- Tests manually build Rete networks since RuleDef → Rete bridge not yet implemented (deferred to later phase)
- Consistency checks gated behind `#[cfg(any(test, debug_assertions))]`
- Fact cloning required before retraction due to borrow checker (fact ref used after retract call)
- Loader API uses single-arg `load_str(source)` not two-arg version

## Pass 002 Implementation (Phase 2)

Successfully implemented Stage 2 construct AST types and interpreter scaffold:
- `stage2.rs` in `ferric-parser/src/` with complete construct interpretation
- Types: `Construct`, `RuleConstruct`, `TemplateConstruct`, `FactsConstruct`
- `InterpreterConfig` with strict/classic mode (strict stops on first error)
- `InterpretError` with `InterpretErrorKind` and helpful error messages
- `InterpretResult` aggregates constructs and errors
- `interpret_constructs()` entry point dispatches on keyword
- Individual interpret functions: `interpret_rule`, `interpret_template`, `interpret_facts`
- Edit distance helper for keyword suggestions
- 30 comprehensive tests covering all error cases and construct types
- All 292 workspace tests pass, clippy clean with `-D warnings`

Key design notes:
- Rule interpretation extracts salience from `(declare (salience N))` forms
- All constructs support optional doc comment strings as second element after name
- LHS/RHS and slot/fact bodies stored as raw S-expressions for later typing (Pass 003)
- Strict mode stops on first error; classic mode collects all errors
- Known unsupported keywords (deffunction, defglobal, etc.) give helpful "not yet supported" errors
- Unknown keywords suggest similar valid keywords using Levenshtein distance
- Error Display impl includes source location and suggestions

## Pass 004 Implementation (Phase 2)

Successfully implemented the ReteCompiler in `ferric-core/src/compiler.rs`:
- Input types: `CompilableRule`, `CompilablePattern` (constructed by runtime layer from parser types)
- `ReteCompiler` with alpha path caching for node sharing across rules
- `compile_rule()` builds alpha paths, beta join chain with variable binding tests, and terminal nodes
- Variable binding tracking: uses `HashSet<Symbol>` to track bound variables and generate join tests only for previously-bound variables
- Alpha path sharing: rules with identical patterns (`entry_type`, `constant_tests`) share the same alpha memory
- Error types: `CompileError::EmptyRule`, `CompileError::VarMapOverflow`
- Result type: `CompileResult` with `rule_id`, `terminal_node`, `alpha_memories`
- 15 comprehensive tests covering single/multi-pattern rules, variable binding, alpha path sharing, constant tests
- All 329 workspace tests pass, clippy clean with `-D warnings`

Key design notes:
- `BetaNode::Terminal` has two fields: `parent` and `rule` — pattern matching must use `..` for field ignoring
- Rule ID allocation starts from 1 (reserve 0)
- Alpha path cache key: `AlphaPathKey { entry_type, tests }` — uses `HashMap` for fast lookups
- Variable binding logic: first occurrence binds, subsequent occurrences create join tests
- Join tests use `JoinTestType::Equal` for variable equality constraints
- `ensure_alpha_path()` is idempotent: caches alpha paths by (entry_type, constant_tests) key
- Compiler exports added to `ferric-core` lib.rs: `CompilableRule`, `CompilablePattern`, `CompileError`, `CompileResult`, `ReteCompiler`

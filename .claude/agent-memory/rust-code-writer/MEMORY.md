# Rust Code Writer Memory

## Project Structure: ferric-rules

This is a Cargo workspace with four crates:
- `ferric` — facade crate that re-exports from the others
- `ferric-core` — shared types, fact storage, pattern matching (core engine internals)
- `ferric-parser` — parser (Stage 1 S-expression parser complete)
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

## Pass 006 Implementation

Successfully implemented token storage with reverse indices for efficient retraction:
- `token.rs` in `ferric-core/src/` with `NodeId`, `TokenId`, `Token`, `TokenStore`
- Token store with two reverse indices:
  - `fact_to_tokens`: maps FactId → tokens containing that fact (for retraction entry point)
  - `parent_to_children`: maps TokenId → child tokens (for cascading deletes)
- Methods: `insert`, `remove` (non-cascading), `remove_cascade`, `tokens_containing`, `children`, `retraction_roots`
- `debug_assert_consistency()` for invariant checking (gated behind test/debug_assertions)
- Comprehensive tests including property-based tests
- All 104 core tests pass, workspace-wide all 207 tests pass, clippy clean

Key design notes:
- `remove()` orphans children (doesn't cascade) — removes parent_to_children entry for the removed token
- Fact deduplication during insert: same FactId appearing multiple times in token.facts only creates one index entry
- Index cleanup on remove: prunes empty SmallVec entries to prevent unbounded growth
- Consistency check allows orphaned tokens (parent doesn't exist) since `remove()` doesn't cascade
- Added `Clone` and `Debug` to `BindingSet` for Token derive macros

## Pass 007 Implementation

Successfully implemented alpha network and alpha memory (first stage of Rete algorithm):
- `alpha.rs` in `ferric-core/src/` with complete alpha network implementation
- Types: `SlotIndex`, `AlphaEntryType`, `AlphaMemoryId`, `ConstantTest`, `ConstantTestType`, `AlphaNode`, `AlphaMemory`, `AlphaNetwork`
- `AlphaMemory` with slot indexing: can request indices on specific slots for efficient lookup
- `AlphaNetwork` propagates facts through entry nodes and constant test nodes
- Helper: `get_slot_value()` extracts values from facts by slot index
- Constant tests support `Equal` and `NotEqual` comparisons on `AtomKey` values
- 24 unit tests covering memory operations, network creation, fact propagation, and constant test evaluation
- 2 property-based tests for memory insert/remove and fact propagation
- All 234 workspace tests pass, clippy clean with `-D warnings`

Key design notes:
- Entry nodes are idempotent (get-or-create pattern) — same entry type returns same node
- Constant test nodes are children of entry or other test nodes
- Alpha memories can be attached to any node (entry or test)
- `assert_fact` returns all memories that accepted the fact
- `retract_fact` removes from all memories
- Slot indices are lazily created on request and backfill existing facts
- Empty index entries are pruned eagerly to prevent unbounded growth
- `AtomKey::from_value()` returns `None` for `Multifield` and `Void` (not indexable)

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

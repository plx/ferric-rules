# Phase 001 Notes

## Pass 001: Workspace Bootstrap And CI

### What Was Done
- Created root workspace `Cargo.toml` with workspace-level package metadata, dependencies, and lint configuration.
- Created four crate scaffolds under `crates/`:
  - `ferric` — public facade, re-exports from core/parser/runtime
  - `ferric-core` — future home of Rete network, pattern matching, agenda
  - `ferric-parser` — future home of lexer, S-expression parser, AST
  - `ferric-runtime` — future home of engine, execution, value types
- Added baseline workspace dependencies: `thiserror`, `slotmap`, `smallvec`.
- Added `rustfmt.toml` with project formatting conventions.
- Added workspace lint configuration (clippy `all` deny + `pedantic` warn, `unsafe_code` deny).
- Created `.github/workflows/ci.yml` with four jobs: check, fmt, clippy, test.
- Added one smoke test per crate verifying the crate compiles and its placeholder type is accessible.

### Decisions and Trade-offs
- **Crates included:** Only the four Phase 1 crates (`ferric`, `ferric-core`, `ferric-parser`, `ferric-runtime`). `ferric-stdlib`, `ferric-ffi`, and `ferric-cli` are deferred to later phases.
- **Clippy pedantic as warn, not deny:** Pedantic lints are set to `warn` to surface issues without blocking development. The `all` category remains `deny`.
- **Specific pedantic allows:** `module_name_repetitions`, `must_use_candidate`, `missing_errors_doc`, `missing_panics_doc` are allowed workspace-wide since they generate excessive noise during early development.
- **Placeholder types:** Each internal crate exports a `Placeholder` struct as a temporary public type to verify inter-crate wiring. These will be removed as real types are introduced.
- **Minimum Rust version:** Set to 1.75 (edition 2021) for broad compatibility while still having access to modern features.

### Remaining TODOs
- None — pass is clean and ready for Pass 002.

### Verification
All four commands pass cleanly:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check --workspace`

## Pass 002: Runtime Values, Symbols, And Encoding

### What Was Done
- Implemented `Value` enum in `ferric-runtime/src/value.rs` with all CLIPS value types: `Symbol`, `String`, `Integer`, `Float`, `Multifield`, `ExternalAddress`, `Void`.
- Implemented `AtomKey` for hashable value subset used in alpha-memory indexing, with exact float bit semantics via `f64::to_bits()`.
- Implemented `FerricString` in `ferric-runtime/src/string.rs` with encoding-aware construction and exact byte equality/ordering (no Unicode normalization).
- Implemented `Symbol`, `SymbolId`, and `SymbolTable` in `ferric-runtime/src/symbol.rs` with dual ASCII/UTF-8 interning pools.
- Implemented `EngineConfig` and `StringEncoding` in `ferric-runtime/src/config.rs`.
- Implemented encoding-checked `intern_symbol` constructor function.
- Implemented `Multifield` with `SmallVec<[Value; 8]>` inline storage and `FromIterator` trait.
- Added `ExternalAddress` and `ExternalTypeId` types.
- Added `EncodingError` error type using `thiserror`.
- 44 unit tests + 5 property-based tests (using `proptest`), covering:
  - ASCII mode enforcement (reject non-ASCII symbols/strings)
  - UTF-8 mode acceptance
  - Mixed mode (ASCII symbols, UTF-8 strings)
  - `-0.0` vs `+0.0` distinction in `AtomKey`
  - NaN bit pattern distinction in `AtomKey`
  - Float roundtrip preservation through `AtomKey`
  - No implicit Unicode normalization in string equality
  - Cross-variant (Ascii/Utf8) equality and hash consistency
  - Structural equality reflexivity (including NaN)
  - Symbol interning idempotency

### Decisions and Trade-offs
- **`Box<Multifield>` in Value enum:** The plan shows `Value::Multifield(Multifield)` directly, but this creates a recursive type (`Value` → `Multifield` → `SmallVec<[Value; 8]>`). Using `Box<Multifield>` breaks the size recursion while keeping the `SmallVec` optimization for the multifield itself.
- **`SymbolTable` is `pub(crate)`:** The symbol table is an internal implementation detail. External callers will use `Engine::intern_symbol` (future). The encoding-checked `intern_symbol` function is similarly `pub(crate)`.
- **`ExternalAddress` without `Send`/`Sync`:** The plan includes `unsafe impl Send + Sync for ExternalAddress`, but since `unsafe_code` is denied workspace-wide and Engine is `!Send + !Sync` anyway, we omit these impls for now. They'll be added when FFI is implemented.
- **`Value` does not implement `PartialEq`:** Intentional per plan — uses `structural_eq()` method instead, which uses bitwise float comparison for deterministic behavior.
- **`proptest` added as workspace dev-dependency** for property-based testing throughout the project.

### Remaining TODOs
- `Placeholder` types still exist in all crates — will be removed as real types are introduced.
- `ExternalAddress` `Send`/`Sync` impls deferred to FFI phase.

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 49 tests pass (44 unit + 5 property-based)
- `cargo check --workspace` — clean

## Pass 003: Facts, Bindings, And Engine Skeleton

### What Was Done
- **Restructured type ownership** — Moved `Value`, `Symbol`, `FerricString`, `SymbolTable`, `AtomKey`, `Multifield`, `ExternalAddress`, `ExternalTypeId` from `ferric-runtime` to `ferric-core` to resolve a circular dependency (facts in core need value types, but runtime depends on core). Created `ferric-core/src/encoding.rs` for `StringEncoding` and `EncodingError`.
- **Implemented fact types** in `ferric-core/src/fact.rs`:
  - `FactId` (via `slotmap::new_key_type!`), `TemplateId`, `OrderedFact`, `TemplateFact`, `Fact`, `FactEntry`
  - `FactBase` with assert/retract/query, dual indices (`by_relation`, `by_template`), and monotonic timestamps
  - Indices are automatically cleaned up on retraction (empty sets removed)
- **Implemented binding types** in `ferric-core/src/binding.rs`:
  - `VarId(u16)`, `VarMap` (symbol-to-ID mapping with idempotent `get_or_create`), `ValueRef` (`Rc<Value>`), `BindingSet` (dense `SmallVec<[Option<ValueRef>; 16]>`)
- **Created Engine skeleton** in `ferric-runtime/src/engine.rs`:
  - `Engine` struct with `FactBase`, `SymbolTable`, `EngineConfig`, `creator_thread`, `PhantomData<*mut ()>` for `!Send + !Sync`
  - `assert_ordered`, `retract`, `get_fact`, `facts`, `intern_symbol`, `create_string` methods
  - Thread affinity check (`assert_same_thread`) on all entry points
  - `EngineError` type via thiserror
- Updated `ferric-runtime/src/lib.rs` to re-export types from `ferric-core`
- Updated `ferric-runtime/src/config.rs` to import `StringEncoding` from core
- Updated `ferric/src/lib.rs` facade to use actual types instead of removed Placeholder
- Added `proptest` dev-dependency and `smallvec`/`thiserror` dependencies to `ferric-core`
- 82 tests in `ferric-core` (unit + property-based), 13 tests in `ferric-runtime`

### Decisions and Trade-offs
- **Moved shared types to `ferric-core`:** The plan's file layout showed value types in `ferric-runtime` but this created a circular dependency since `ferric-core` (which needs them for facts/bindings) can't depend on `ferric-runtime`. Moving them to core resolves this cleanly.
- **`EngineConfig::default()` returns UTF-8 mode:** Matches the plan's intent for modern internationalization as default.
- **Thread affinity check panics:** The `assert_same_thread` check panics rather than returning an error, since calling from the wrong thread is always a programming error.
- **FactBase index cleanup:** Empty index sets are removed from the HashMap on retraction to prevent unbounded memory growth from churn patterns.
- **`BindingSet` uses `SmallVec<[Option<ValueRef>; 16]>`:** Inline storage for up to 16 variables (covers most rules), with Rc<Value> for cheap sharing during token propagation.

### Remaining TODOs
- `ferric-parser` still has only a Placeholder type — will be populated in Pass 004.
- Engine doesn't have Rete network, agenda, or template support yet — those come in later passes.
- `ValueRef` (Rc<Value>) will be exercised more heavily when token propagation is implemented.

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 97 tests pass (82 in core, 13 in runtime, 1 each in facade/parser)
- `cargo check --workspace` — clean

## Pass 004: Stage 1 Lexer And S-Expression Parser

### What Was Done
- **Lexer** (`ferric-parser/src/lexer.rs`): Full tokenization for CLIPS lexical forms — parentheses, integers, floats (with exponent), string literals (with escape sequences), symbols, single-field variables (`?x`), multi-field variables (`$?rest`), global variables (`?*name*`), connectives (`&`, `|`, `~`, `:`, `=`, `<-`), comments (`;`).
- **Span tracking** (`ferric-parser/src/span.rs`): `FileId`, `Position` (byte offset + line/column), `Span` (start..end + file_id), with `merge` for composite spans.
- **Error types** (`ferric-parser/src/error.rs`): `ParseError`, `LexError`, `ParseErrorKind` with six categories (UnexpectedCharacter, UnterminatedString, InvalidNumber, UnexpectedToken, UnclosedParen, UnexpectedCloseParen).
- **S-expression parser** (`ferric-parser/src/sexpr.rs`): `SExpr` (Atom/List), `Atom` variants, `Connective` enum, `ParseResult` with error recovery. The `parse_sexprs(source, file_id)` function lexes and parses in one call.
- **Error recovery**: Unmatched `)` reported and skipped; unclosed `(` reported with partial list returned; multiple errors collected in one pass.
- **Bug fix**: Fixed infinite loop when lexing `=>` — `=` was missing from `is_symbol_char`/`is_symbol_start`, so `lex_symbol()` couldn't consume it, creating an infinite loop.
- **55 unit tests** covering all token types, span tracking, multiline handling, error cases, and CLIPS rule structures.
- **14 property-based tests** (proptest) for lexer and parser:
  - Lexer: never panics on arbitrary ASCII, spans ordered/non-overlapping, spans within bounds, valid tokens produce no errors, integer roundtrip, string roundtrip
  - Parser: never panics on arbitrary ASCII, balanced parens produce no errors, structure preservation, unmatched `)` always errors, unclosed `(` always errors, root span covers source

### Decisions and Trade-offs
- **`=` as symbol character**: Added `=` to both `is_symbol_start` and `is_symbol_char` so symbols like `=>`, `>=`, `<=`, `==` are correctly lexed as multi-character symbols. The `=` case in the main dispatch still produces `Token::Equals` when `=` is followed by a non-symbol character.
- **Lex errors abort parsing**: When lexing produces errors, the parser returns them immediately without attempting to parse. This is simpler than trying to parse a partial token stream and matches the two-stage architecture intent.
- **`ParseResult` always returned**: The parser returns `ParseResult { exprs, errors }` rather than `Result<_, _>`, enabling partial results alongside errors for better tooling support.

### Remaining TODOs
- `ferric-parser` Placeholder type is now replaced by real parser types.
- Stage 2 (construct interpretation) deferred to pass 005.

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 164 tests pass (82 in core, 13 in runtime, 67 in parser, 2 doc-tests)
- `cargo check --workspace` — clean

## Pass 005: Minimal Source Loader For assert And defrule

### What Was Done
- **Loader module** (`ferric-runtime/src/loader.rs`): Full implementation of source loading:
  - `Engine::load_str(source)` — parses CLIPS source and processes top-level forms
  - `Engine::load_file(path)` — reads file and delegates to `load_str`
  - `LoadError` enum (Parse, UnsupportedForm, InvalidAssert, InvalidDefrule, Engine, Io)
  - `LoadResult` struct with asserted_facts, rules, and warnings
  - `RuleDef` struct for S-expression-level rule storage (name, lhs, rhs)
- **Top-level form dispatcher**:
  - `(assert ...)` — converts atoms to `Value`s and asserts ordered facts
  - `(defrule name <patterns> => <actions>)` — extracts name, splits on `=>`, stores as `RuleDef`
  - `(deffacts name <facts>)` — treated as batch assert
  - All other forms → `LoadError::UnsupportedForm`
- **Atom-to-Value conversion**: Integer, Float, String (encoding-aware), Symbol (encoding-aware); variables/connectives produce warnings
- **Engine field visibility**: Changed `fact_base`, `symbol_table`, `config` to `pub(crate)` for loader access
- 33 unit tests + 3 property-based tests covering all paths

### Decisions and Trade-offs
- **`RuleDef` stores raw S-expressions**: Full Stage 2 rule parsing/compilation deferred to later passes. Rules are stored at S-expression level, which is sufficient for Phase 1 integration testing.
- **Error aggregation**: `load_str` returns `Vec<LoadError>` rather than stopping at first error, supporting multi-error diagnostics.
- **Warnings for unsupported atoms**: Variables and connectives in assert forms produce warnings rather than errors, since the loader should be lenient during Phase 1.
- **`deffacts` support**: Added opportunistically since it's trivially similar to `assert` processing.

### Remaining TODOs
- `RuleDef` is not yet compiled into Rete network nodes — that comes in passes 007-008.
- No template fact support yet (only ordered facts).
- File-level `FileId` tracking is minimal (always `FileId(0)` for strings).

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 189 tests pass (82 in core, 36 in runtime, 67 in parser, 1 facade, 3 doc-tests)
- `cargo check --workspace` — clean

## Pass 006: Token Store, Retraction Indices, And Invariant Harness

### What Was Done
- **Token module** (`ferric-core/src/token.rs`): Full implementation of token storage:
  - `NodeId(u32)` — simple identifier for Rete network nodes
  - `TokenId` — slotmap key type for stable token identity
  - `Token` — partial match struct with facts, bindings, parent, owner_node
  - `TokenStore` — central storage with two reverse indices:
    - `fact_to_tokens: HashMap<FactId, SmallVec<[TokenId; 4]>>` — for retraction entry point
    - `parent_to_children: HashMap<TokenId, SmallVec<[TokenId; 4]>>` — for cascading deletes
- **Key operations**:
  - `insert()` with fact deduplication and debug_assert no-duplicate invariant
  - `remove()` for single token removal (non-cascading), with index pruning
  - `remove_cascade()` using iterative stack-based traversal of the parent-children tree
  - `retraction_roots()` to filter affected tokens to minimal set (no ancestors in set)
  - `tokens_containing()` and `children()` for index lookup
- **Invariant harness**: `debug_assert_consistency()` method gated behind `#[cfg(any(test, debug_assertions))]` that verifies:
  - All TokenIds in fact_to_tokens exist in the tokens SlotMap
  - All TokenIds in parent_to_children exist in the tokens SlotMap
  - For every token, its facts are correctly reflected in fact_to_tokens
  - For every token with a living parent, the parent's children list contains it
  - No empty SmallVecs exist in index maps
- Added `Clone` and `Debug` derives to `BindingSet` (needed by Token)
- 22 unit tests + 3 property-based tests

### Decisions and Trade-offs
- **Non-cascading `remove()`**: Orphans children rather than cascading, per spec. Use `remove_cascade()` for subtree deletion.
- **Fact deduplication on insert**: Same FactId appearing multiple times in a token's facts list produces only one index entry.
- **SmallVec<[_; 4]>** for reverse index values: inline storage for typical case (most facts participate in few tokens), spills to heap for rare high-fanout cases.
- **`retain()` for removals**: Linear scan + compaction, correct by construction, fast for k ≤ 4.

### Remaining TODOs
- `owner_node` is stored but not yet used for beta memory cleanup (comes in Pass 008).
- `debug_assert_consistency` will be extended as more structures are added.

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 211 tests pass (104 in core, 36 in runtime, 67 in parser, 1 facade, 3 doc-tests)
- `cargo check --workspace` — clean

## Pass 007: Alpha Network And Alpha Memory

### What Was Done
- **Alpha module** (`ferric-core/src/alpha.rs`): Full alpha-side implementation:
  - `SlotIndex` enum (Ordered/Template) for referencing fact fields by position
  - `AlphaEntryType` enum for first-level discrimination (Template/OrderedRelation)
  - `AlphaMemoryId(u32)` — identifier for alpha memories
  - `ConstantTest` and `ConstantTestType` (Equal/NotEqual) for slot value tests
  - `AlphaNode` enum (Entry/ConstantTest) with children and optional memory
  - `AlphaMemory` with:
    - `HashSet<FactId>` primary storage
    - Optional slot indices `HashMap<SlotIndex, HashMap<AtomKey, HashSet<FactId>>>`
    - Lazy index creation with backfilling via `request_index()`
    - Eager pruning of empty index entries
  - `AlphaNetwork` managing nodes, memories, and propagation:
    - Idempotent entry node creation
    - Constant test node creation as children
    - `assert_fact()` — propagates through network, returns affected memory IDs
    - `retract_fact()` — removes from all matching memories
  - `get_slot_value()` helper for extracting values from facts
- 24 unit tests + 2 property-based tests

### Decisions and Trade-offs
- **Entry nodes are idempotent**: Same `AlphaEntryType` always returns the same `NodeId`.
- **Slot indices are lazy**: Only created when `request_index()` is called, with backfilling of existing facts. This matches the plan's intent for index creation during rule compilation.
- **Constant tests use `AtomKey`**: Multifield and Void values can't be indexed/tested.
- **Phase 1 test types**: Only Equal and NotEqual. LessThan/GreaterThan/etc. deferred to later phases.

### Remaining TODOs
- Alpha network is not yet wired to beta network — that comes in Pass 008.
- No node sharing yet (each `create_constant_test_node` creates a new node).
- LessThan/GreaterThan constant test types deferred.

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 237 tests pass (130 in core, 36 in runtime, 67 in parser, 1 facade, 3 doc-tests)
- `cargo check --workspace` — clean

## Pass 008: Beta Network, Simple Joins, And Agenda Plumbing

### What Was Done
- **Beta module** (`ferric-core/src/beta.rs`): Full beta-side implementation:
  - `JoinTest` and `JoinTestType` (Equal/NotEqual) for comparing token bindings against fact slot values
  - `BetaMemoryId(u32)` identifier, `BetaMemory` with `HashSet<TokenId>` storage
  - `BetaNode` enum (Root/Join/Terminal) — Phase 1 subset
  - `BetaNetwork` managing nodes, memories, and the `alpha_to_joins` subscription index
  - `create_join_node()` with parent linking, memory allocation, and alpha subscription registration
  - `create_terminal_node()` for activation-producing nodes
  - `join_nodes_for_alpha()` reverse lookup for right activation dispatch
- **Agenda module** (`ferric-core/src/agenda.rs`): Full activation lifecycle:
  - `ActivationId` (slotmap key), `Activation` struct with rule, token, salience, timestamp, sequence
  - `AgendaKey` with `Reverse` wrappers: higher salience → higher timestamp (depth strategy) → higher sequence
  - `Agenda` with `BTreeMap<AgendaKey, ActivationId>` ordering, four indices:
    - `activations` SlotMap for storage
    - `id_to_key` for O(1) removal by ID
    - `token_to_activations` for retraction cleanup
  - `add()`, `pop()`, `remove_activations_for_token()` methods
- **Rete module** (`ferric-core/src/rete.rs`): Full integration layer:
  - `ReteNetwork` combining alpha, beta, token_store, and agenda
  - `assert_fact()` — full pipeline: alpha propagation → collect affected memories → right activation on subscribed joins → join evaluation → token creation → terminal activation
  - `retract_fact()` — token cascade removal via `retraction_roots()` → `remove_cascade()`, then beta memory cleanup, agenda cleanup, and alpha memory cleanup
  - `right_activate()` — handles both root-parent (no existing tokens) and non-root-parent (iterate parent tokens) cases
  - `evaluate_join()` free function comparing fact slot values against token bindings via `AtomKey`
  - `propagate_token()` dispatching to terminal (creates activation) vs join/root children
- Updated `ferric-core/src/lib.rs` with new module declarations and re-exports
- 13 new unit tests (4 beta + 4 agenda + 5 rete integration)

### Decisions and Trade-offs
- **Beta root at `NodeId(100_000)`**: Avoids conflict with alpha network node IDs by using a high offset. A proper shared ID allocator would be better long-term, but this is sufficient for Phase 1.
- **`alpha_to_joins` index**: Pre-computed reverse lookup from alpha memories to their subscribed join nodes, avoiding linear scan during right activation.
- **Depth strategy ordering**: `AgendaKey` uses `Reverse` wrappers so that `BTreeMap`'s natural ordering (ascending) produces the correct depth-first behavior (higher salience, then most recent, then highest sequence first).
- **Root-parent special case**: When a join node's parent is the beta root, there are no parent tokens to iterate over. Instead, a fresh token is created with just the new fact. This avoids needing a dummy root memory.
- **Retraction cleanup iterates all beta memories**: Rather than tracking which memory owns each token, `retract_fact` iterates all beta memories for each removed token. This is O(memories × removed_tokens) — correct but inefficient. A per-token owner index would be better for Phase 2.
- **No variable binding during join**: Phase 1 join evaluation checks existing bindings against fact slots, but does not create new bindings during the join. This limits what can be tested but keeps the join logic straightforward.

### Remaining TODOs
- Variable binding creation during join (binding extraction from patterns → token propagation) deferred to Phase 2.
- Node sharing in beta network (multiple rules sharing common prefix) not implemented.
- Retraction beta memory cleanup is O(all memories) per removed token — needs per-token owner tracking.
- Left activation (re-propagation when a new token arrives in a parent memory) not implemented — only right activation works.

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 251 tests pass (144 in core, 36 in runtime, 67 in parser, 1 facade, 3 doc-tests)
- `cargo check --workspace` — clean

## Pass 009: Phase 1 Integration And Exit Validation

### What Was Done
- **Cross-cutting integration tests** (`ferric-runtime/src/integration_tests.rs`): 5 integration tests exercising the full pipeline — parser → loader → engine → Rete network → activation:
  - `integration_parse_load_assert_match` — load fact via `load_str()`, build Rete, assert, verify activation
  - `integration_retract_removes_activation` — load, assert, verify activation, retract, verify cleanup (agenda empty, no orphan tokens)
  - `integration_multiple_facts_multiple_activations` — 3 facts produce 3 activations
  - `integration_constant_test_filters_facts` — alpha constant test filters `(color red)` from `(color blue)` and `(color green)`
  - `integration_loader_and_rete_roundtrip` — realistic CLIPS source with `deffacts` + `defrule`, verifies 3 facts, 1 rule def, 3 activations, pop/drain behavior
- **Alpha network invariant harness** (`ferric-core/src/alpha.rs`): `debug_assert_consistency()` gated behind `#[cfg(any(test, debug_assertions))]` verifying:
  - All alpha memory IDs referenced by nodes exist in the memories map
  - All node IDs in children fields exist in the nodes map
  - No duplicate children in any node
  - All facts in slot indices are also in the main facts set of that memory
- **Beta network invariant harness** (`ferric-core/src/beta.rs`): `debug_assert_consistency()` gated behind `#[cfg(any(test, debug_assertions))]` verifying:
  - All node IDs in children fields exist in the nodes map
  - All parent references point to existing nodes
  - All memory IDs in join nodes exist in the memories map
  - All join nodes in `alpha_to_joins` index exist and are actually Join variant
  - Root node exists and is a Root variant
- **Extended retraction invariant tests** (`ferric-core/src/rete.rs`): 2 new tests exercising `debug_assert_consistency()` across token store and alpha network after every assert/retract operation:
  - `retraction_invariants_after_assert_retract_cycle` — assert 3, retract 1, assert 1, retract all; consistency checked at every step
  - `retraction_invariants_with_constant_tests` — assert 5 facts through constant test filter, verify consistency at each step, retract all, verify clean state
- **Audit**: No dead stubs, no Placeholder types, no TODO/FIXME in Rust code. Public API surface is minimal and appropriate for Phase 1.
- **No technical debt to clean**: All scaffolding from earlier passes was integrated cleanly.

### Phase 1 Exit Criteria Verification
1. Basic `.clp` files are parseable into Stage 1 S-expressions — **satisfied** (Pass 004)
2. Minimal loader handles top-level `(assert ...)` and `(defrule ...)` — **satisfied** (Pass 005)
3. Facts can be asserted/retracted through engine APIs — **satisfied** (Pass 003)
4. Simple rule matching through alpha + beta produces activations — **satisfied** (Pass 008, verified by Pass 009 integration tests)
5. Retraction-invariant test harness exists and is wired into tests — **satisfied** (TokenStore in Pass 006, AlphaNetwork + BetaNetwork in Pass 009)
6. Workspace CI checks pass consistently — **satisfied** (all 258 tests pass)

### Decisions and Trade-offs
- **Integration tests in `ferric-runtime`**: Placed in the runtime crate (not the facade) because they need `pub(crate)` access to `engine.fact_base` and `engine.symbol_table` for Rete network setup.
- **Manual Rete network setup in integration tests**: Since there's no automatic `RuleDef` → Rete compilation yet (Phase 2), integration tests manually build the Rete network to demonstrate the pipeline works.
- **No API changes**: Pass 009 added only tests and invariant harnesses — no new public API surface.

### Remaining TODOs
- Phase 1 is complete. Next work starts at Phase 2 ("Core Engine") scope.
- Key Phase 2 prerequisites: Stage 2 parser (pattern compilation), automatic rule → Rete compilation, variable binding during joins, left activation, node sharing.

### Verification
- `cargo fmt --all --check` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 258 tests pass (146 in core, 41 in runtime, 67 in parser, 1 facade, 3 doc-tests)
- `cargo check --workspace` — clean

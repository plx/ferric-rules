# Property Test Coverage Plan

This document catalogs the property-testable components of `ferric-rules` and their
testable properties. It is organized component-by-component, where "component" is
deliberately flexible — it may refer to an individual type, a module, or an abstract
grouping of related functionality. Types may appear in multiple components when they
participate in distinct subsystems with distinct properties to verify.

Each component section includes:
- **What it is** and where to find it
- **Properties** worth testing, described in terms of preconditions, postconditions,
  and invariants, with enough detail for an implementing agent to know which code
  entities are involved and what must hold

The components are organized in three tiers:
1. **Tier 1 — Individual Types & Data Structures**: Properties of isolated types
2. **Tier 2 — Subsystems**: Properties of cooperating modules
3. **Tier 3 — Cross-Cutting Concerns**: Properties spanning multiple subsystems

Within each tier, components are ordered roughly by priority (most impactful first).

---

## Table of Contents

### Tier 1 — Individual Types & Data Structures
- [1.1 Value System](#11-value-system)
- [1.2 Fact Storage](#12-fact-storage)
- [1.3 Token Store](#13-token-store)
- [1.4 Variable Bindings](#14-variable-bindings)
- [1.5 Negative Node Memory](#15-negative-node-memory)
- [1.6 Exists Node Memory](#16-exists-node-memory)
- [1.7 NCC Node Memory](#17-ncc-node-memory)
- [1.8 Agenda & Conflict Resolution](#18-agenda--conflict-resolution)
- [1.9 Alpha Memory](#19-alpha-memory)
- [1.10 Beta Memory](#110-beta-memory)

### Tier 2 — Subsystems
- [2.1 Alpha Network](#21-alpha-network)
- [2.2 Beta Network](#22-beta-network)
- [2.3 Rete Network (Integrated)](#23-rete-network-integrated)
- [2.4 Compiler](#24-compiler)
- [2.5 Parser Pipeline](#25-parser-pipeline)
- [2.6 Expression Evaluator](#26-expression-evaluator)
- [2.7 Module System & Visibility](#27-module-system--visibility)
- [2.8 FFI Boundary](#28-ffi-boundary)

### Tier 3 — Cross-Cutting Concerns
- [3.1 Fact Lifecycle](#31-fact-lifecycle)
- [3.2 Rule Compilation-to-Execution Pipeline](#32-rule-compilation-to-execution-pipeline)
- [3.3 Reset & Clear Semantics](#33-reset--clear-semantics)
- [3.4 Thread Affinity](#34-thread-affinity)

---

## Tier 1 — Individual Types & Data Structures

### 1.1 Value System

**Location:** `crates/ferric-core/src/value.rs`, `crates/ferric-core/src/symbol.rs`,
`crates/ferric-core/src/string.rs`, `crates/ferric-core/src/encoding.rs`

**Types involved:** `Value`, `AtomKey`, `Multifield`, `FerricString`, `Symbol`,
`SymbolTable`, `StringEncoding`, `ExternalTypeId`

**Description:** The foundational runtime value representation. `Value` is a tagged
union of all CLIPS value types. `AtomKey` is a hashable/equatable subset used for
indexing. `FerricString` is an encoding-aware immutable string. `Symbol` is an interned
string handle valid only within its owning `SymbolTable`.

#### Properties

**P1.1.1 — AtomKey roundtrip preservation**
- _Entities:_ `AtomKey::from_value`, `AtomKey::to_value`, `Value::structural_eq`
- _Property:_ For any `Value` v where `AtomKey::from_value(&v)` returns `Some(k)`,
  `v.structural_eq(&k.to_value())` must be `true`.
- _Generator:_ Arbitrary `Value` variants excluding `Multifield` and `Void` (the
  non-atom types).

**P1.1.2 — AtomKey hashing consistency**
- _Entities:_ `AtomKey`, `Hash`, `Eq`
- _Property:_ For any two `AtomKey` values `a` and `b`, if `a == b` then
  `hash(a) == hash(b)`. (Standard hash-eq contract.)
- _Generator:_ Pairs of `AtomKey` values, including edge cases like `-0.0`/`+0.0`,
  NaN bit patterns, empty strings.

**P1.1.3 — Value::structural_eq reflexivity for atoms**
- _Entities:_ `Value::structural_eq`
- _Property:_ For any `Value` v that is not `Float(NaN)`, `v.structural_eq(&v)` is
  `true`. For `Float(NaN)`, `structural_eq` comparing the same NaN bit pattern to
  itself must also be `true` (since it compares via `to_bits()`).
- _Note:_ This is stronger than IEEE 754 reflexivity — the implementation deliberately
  uses bitwise comparison, so even NaN is reflexive.
- _Generator:_ Arbitrary `Value` instances.

**P1.1.4 — Value::structural_eq symmetry**
- _Entities:_ `Value::structural_eq`
- _Property:_ For any `Value` pair `(a, b)`, `a.structural_eq(&b) == b.structural_eq(&a)`.
- _Generator:_ Pairs of arbitrary `Value` instances, including cross-variant pairs.

**P1.1.5 — Float bitwise identity preservation**
- _Entities:_ `Value::Float`, `AtomKey::FloatBits`, `f64::to_bits`, `f64::from_bits`
- _Property:_ For any `f64` value f (including NaN, infinities, negative zero),
  `f64::from_bits(f64::to_bits(f)) == f` at the bit level (i.e., `to_bits` of both
  sides are identical).
- _Generator:_ Arbitrary `f64` values, with explicit inclusion of special values.

**P1.1.6 — FerricString encoding enforcement**
- _Entities:_ `FerricString::new`, `StringEncoding`
- _Property:_ In `Ascii` mode, `FerricString::new(s, Ascii)` returns `Err` for any
  string `s` containing a non-ASCII byte. In `Utf8` and `AsciiSymbolsUtf8Strings`
  modes, it returns `Ok` for any valid `&str`.
- _Generator:_ Arbitrary `&str` values paired with all three `StringEncoding` variants.

**P1.1.7 — FerricString cross-variant equality**
- _Entities:_ `FerricString`, `PartialEq`, `Hash`
- _Property:_ For any ASCII-only string content `s`, `FerricString::Ascii(s)` and
  `FerricString::Utf8(s)` must be equal and hash identically.
- _Generator:_ Arbitrary ASCII-only strings.

**P1.1.8 — FerricString ordering consistency**
- _Entities:_ `FerricString`, `Ord`
- _Property:_ The ordering is a total order: antisymmetric, transitive, and total. It
  must be consistent with `Eq` (i.e., `cmp` returns `Equal` iff `==` is `true`).
- _Generator:_ Triples of arbitrary `FerricString` values for transitivity checks.

**P1.1.9 — Symbol interning idempotency**
- _Entities:_ `SymbolTable::intern_symbol`
- _Property:_ For any string `s` and encoding `e`, calling `intern_symbol(s, e)` twice
  returns the same `Symbol` both times.
- _Generator:_ Arbitrary `(String, StringEncoding)` pairs.

**P1.1.10 — Symbol interning distinctness**
- _Entities:_ `SymbolTable::intern_symbol`
- _Property:_ For any two distinct strings `s1 != s2` interned in the same encoding
  mode, the resulting `Symbol` values are not equal.
- _Generator:_ Pairs of distinct strings.

**P1.1.11 — Symbol resolution roundtrip**
- _Entities:_ `SymbolTable::intern_symbol`, `SymbolTable::resolve_symbol`
- _Property:_ For any string `s` successfully interned as `sym`,
  `resolve_symbol(sym)` returns bytes equal to `s.as_bytes()`.
- _Generator:_ Arbitrary strings that pass encoding validation.

**P1.1.12 — Multifield structural equality element-wise**
- _Entities:_ `Multifield`, `PartialEq`
- _Property:_ Two `Multifield` values are equal iff they have the same length and
  every corresponding pair of elements satisfies `Value::structural_eq`.
- _Generator:_ Pairs of `Multifield` values with varying lengths and element types.

**P1.1.13 — AtomKey excludes non-atom types**
- _Entities:_ `AtomKey::from_value`
- _Property:_ `AtomKey::from_value` returns `None` for `Value::Multifield` and
  `Value::Void`, and `Some` for all other variants.
- _Generator:_ Arbitrary `Value` instances.

---

### 1.2 Fact Storage

**Location:** `crates/ferric-core/src/fact.rs`

**Types involved:** `FactBase`, `FactEntry`, `FactId`, `Timestamp`, `Fact`,
`OrderedFact`, `TemplateFact`

**Description:** The working memory store. Facts are identified by `FactId` (slotmap
key) and carry monotonically increasing `Timestamp` values. The `FactBase` maintains
two secondary indices: by relation name (for ordered facts) and by template ID (for
template facts).

#### Properties

**P1.2.1 — Timestamp strict monotonicity**
- _Entities:_ `FactBase::assert_ordered`, `FactBase::assert_template`, `Timestamp`
- _Property:_ For any sequence of N assertions on a `FactBase`, the resulting
  timestamps form a strictly increasing sequence: `t_1 < t_2 < ... < t_N`.
- _Generator:_ Arbitrary interleaving of `assert_ordered` and `assert_template` calls.

**P1.2.2 — Assert-then-get roundtrip**
- _Entities:_ `FactBase::assert_ordered`, `FactBase::get`
- _Property:_ After `let id = fb.assert_ordered(rel, fields)`, `fb.get(id)` returns
  `Some(entry)` where `entry.fact` matches the asserted relation and fields.
- _Generator:_ Arbitrary relation symbols and field vectors.

**P1.2.3 — Retract removes from all indices**
- _Entities:_ `FactBase::retract`, `FactBase::get`, `FactBase::facts_by_relation`,
  `FactBase::facts_by_template`
- _Property:_ After `fb.retract(id)`:
  - `fb.get(id)` returns `None`
  - `id` does not appear in any `facts_by_relation` or `facts_by_template` iterator
  - `fb.len()` decreased by 1
- _Generator:_ Sequences of assert/retract operations.

**P1.2.4 — Retract idempotency**
- _Entities:_ `FactBase::retract`
- _Property:_ A second call to `fb.retract(id)` with the same `id` returns `None`
  and does not change `fb.len()`.
- _Generator:_ Arbitrary fact IDs.

**P1.2.5 — Index consistency after arbitrary operations**
- _Entities:_ `FactBase`, all index fields
- _Property:_ After any sequence of `assert_ordered`, `assert_template`, and
  `retract` operations:
  - Every `FactId` in `by_relation[R]` has `get(id).fact` with relation `R`
  - Every `FactId` in `by_template[T]` has `get(id).fact` with template ID `T`
  - No empty sets exist in either index (pruning invariant)
  - `len()` equals the number of facts retrievable via `get()`
- _Generator:_ Arbitrary sequences of mixed assert/retract operations.

**P1.2.6 — Timestamp uniqueness**
- _Entities:_ `FactBase`, `Timestamp`
- _Property:_ No two facts in a `FactBase` share the same `Timestamp`.
- _Generator:_ Sequences of assertions (timestamps should never collide).

---

### 1.3 Token Store

**Location:** `crates/ferric-core/src/token.rs`

**Types involved:** `TokenStore`, `Token`, `TokenId`, `NodeId`

**Description:** Storage for partial-match tokens in the Rete beta network. Tokens
form a forest (trees via parent pointers). Two reverse indices enable efficient
retraction: `fact_to_tokens` maps facts to containing tokens, and
`parent_to_children` maps parents to direct children.

#### Properties

**P1.3.1 — Insert-then-get roundtrip**
- _Entities:_ `TokenStore::insert`, `TokenStore::get`
- _Property:_ After `let id = ts.insert(token)`, `ts.get(id)` returns the token
  with the same facts, bindings, parent, and owner_node.
- _Generator:_ Arbitrary `Token` values.

**P1.3.2 — Fact-to-token index consistency**
- _Entities:_ `TokenStore`, `fact_to_tokens` index
- _Property:_ After any sequence of insert/remove operations, for every token `t`
  with ID `tid` in the store, and for every distinct `FactId` in `t.facts`,
  `tokens_containing(fact_id)` includes `tid`. Conversely, every `tid` in the index
  for a fact `fid` is a live token whose `facts` field contains `fid`.
- _Generator:_ Arbitrary insert/remove sequences.

**P1.3.3 — Parent-to-children index consistency**
- _Entities:_ `TokenStore`, `parent_to_children` index
- _Property:_ After any sequence of operations, for every token `t` with
  `t.parent == Some(pid)` where `pid` is a live token, `children(pid)` includes
  `t`'s ID. No stale entries: every child listed under a parent is live and has
  that parent.
- _Generator:_ Arbitrary tree-structured token insertions and removals.

**P1.3.4 — Cascade removal completeness**
- _Entities:_ `TokenStore::remove_cascade`
- _Property:_ After `remove_cascade(root)`, `root` and all of its descendants
  (transitively via `parent_to_children`) are removed. No orphaned subtrees remain.
  All index entries for removed tokens are cleaned.
- _Generator:_ Arbitrary tree-structured token insertions, then cascade from arbitrary root.

**P1.3.5 — Retraction roots minimality**
- _Entities:_ `TokenStore::retraction_roots`
- _Property:_ Given a set of affected token IDs, `retraction_roots(affected)` returns
  a subset such that:
  - Every token in `affected` is either in the result or is a descendant of a token
    in the result
  - No token in the result is a descendant of another token in the result
- _Generator:_ Arbitrary token trees with arbitrary subsets marked as affected.

**P1.3.6 — Remove orphans children (non-cascade)**
- _Entities:_ `TokenStore::remove` (non-cascade)
- _Property:_ After `remove(id)`, children of `id` remain in the store but their
  `parent` field points to a non-existent token. The parent-to-children index no
  longer contains `id` as a key.
- _Generator:_ Token trees where a non-leaf is removed.

**P1.3.7 — Empty-index pruning**
- _Entities:_ `TokenStore`, index maps
- _Property:_ After any sequence of operations, no key in `fact_to_tokens` or
  `parent_to_children` maps to an empty list/vec.
- _Generator:_ Arbitrary operation sequences including insertions and removals that
  empty out index entries.

---

### 1.4 Variable Bindings

**Location:** `crates/ferric-core/src/binding.rs`

**Types involved:** `VarMap`, `VarId`, `BindingSet`, `ValueRef`

**Description:** `VarMap` provides bidirectional symbol-to-ID mapping for pattern
variables. `BindingSet` is a sparse vector of optional `Rc<Value>` indexed by `VarId`.

#### Properties

**P1.4.1 — VarMap idempotent get_or_create**
- _Entities:_ `VarMap::get_or_create`
- _Property:_ For any symbol `s`, calling `get_or_create(s)` N times always returns
  the same `VarId`.
- _Generator:_ Arbitrary sequences of `get_or_create` calls with repeated symbols.

**P1.4.2 — VarMap bidirectional consistency**
- _Entities:_ `VarMap::get_or_create`, `VarMap::lookup`, `VarMap::name`
- _Property:_ After any sequence of `get_or_create` calls:
  - `lookup(s) == Some(id)` iff `get_or_create(s)` was called with `s`
  - `name(id) == s` for the symbol that produced `id`
  - The number of distinct IDs equals `len()`
- _Generator:_ Arbitrary sequences of symbol interning operations.

**P1.4.3 — VarMap sequential ID assignment**
- _Entities:_ `VarMap::get_or_create`
- _Property:_ The first N distinct symbols produce VarIds 0, 1, 2, ..., N-1 in the
  order they were first seen.
- _Generator:_ Arbitrary sequences of get_or_create with varying repeat patterns.

**P1.4.4 — BindingSet set-then-get roundtrip**
- _Entities:_ `BindingSet::set`, `BindingSet::get`
- _Property:_ After `bs.set(var, value)`, `bs.get(var)` returns `Some(v)` where
  `v` points to the set value.
- _Generator:_ Arbitrary `(VarId, Value)` pairs.

**P1.4.5 — BindingSet extend_from non-overwriting merge**
- _Entities:_ `BindingSet::extend_from`
- _Property:_ After `child.extend_from(&parent)`:
  - For every `VarId` where `child.get(var)` was `Some` before the call, the value
    is unchanged
  - For every `VarId` where `child.get(var)` was `None` and `parent.get(var)` was
    `Some`, `child.get(var)` is now `Some` with the parent's value
  - `bound_count` of child >= original `bound_count` of child
- _Generator:_ Pairs of `BindingSet` with overlapping and non-overlapping VarId ranges.

**P1.4.6 — BindingSet capacity auto-expansion**
- _Entities:_ `BindingSet::set`
- _Property:_ `set(var, value)` succeeds for any `VarId` regardless of current
  capacity. After the call, `capacity() >= var.0 + 1`.
- _Generator:_ Arbitrary VarIds including large values.

---

### 1.5 Negative Node Memory

**Location:** `crates/ferric-core/src/negative.rs`

**Types involved:** `NegativeMemory`, `NegativeMemoryId`, `TokenId`, `FactId`

**Description:** Tracks which parent tokens are blocked by which matching facts. A
token is unblocked (and its pass-through propagated) only when ALL blockers are
removed. Maintains bidirectional indices between tokens and blocking facts.

#### Properties

**P1.5.1 — Bidirectional index consistency**
- _Entities:_ `NegativeMemory`, `blocked`, `fact_to_blocked`
- _Property:_ After any sequence of `add_blocker`/`remove_blocker`/`remove_parent_token`
  operations:
  - Token `T` is in `blocked[T]` with fact `F` iff `fact_to_blocked[F]` contains `T`
  - No empty sets in either index
- _Generator:_ Arbitrary operation sequences.

**P1.5.2 — Mutual exclusivity of blocked and unblocked**
- _Entities:_ `NegativeMemory`, `blocked`, `unblocked`
- _Property:_ No `TokenId` appears as a key in both `blocked` and `unblocked`
  simultaneously.
- _Generator:_ Arbitrary operation sequences including add/remove blocker and
  set/remove unblocked transitions.

**P1.5.3 — Unblocking transition correctness**
- _Entities:_ `NegativeMemory::remove_blocker`
- _Property:_ `remove_blocker(token, fact)` returns `true` iff the token's blocker
  set becomes empty (i.e., last blocker removed). When it returns `true`, the token
  is no longer in `blocked`.
- _Generator:_ Sequences that add multiple blockers then remove them one by one.

**P1.5.4 — remove_parent_token complete cleanup**
- _Entities:_ `NegativeMemory::remove_parent_token`
- _Property:_ After `remove_parent_token(tid)`, `tid` does not appear anywhere in
  `blocked`, `fact_to_blocked`, or `unblocked`. The memory passes
  `debug_assert_consistency()`.
- _Generator:_ Memories with multiple blocked/unblocked tokens, removal of one.

---

### 1.6 Exists Node Memory

**Location:** `crates/ferric-core/src/exists.rs`

**Types involved:** `ExistsMemory`, `ExistsMemoryId`

**Description:** Tracks support for parent tokens. A token is "satisfied" (and its
pass-through propagated) when it has at least one supporting fact. Maintains
bidirectional indices between tokens and supporting facts.

#### Properties

**P1.6.1 — Bidirectional index consistency**
- _Entities:_ `ExistsMemory`, `support`, `fact_to_parents`
- _Property:_ After any operation sequence:
  - Token `T` has fact `F` in `support[T]` iff `fact_to_parents[F]` contains `T`
  - No empty sets in either index
- _Generator:_ Arbitrary operation sequences.

**P1.6.2 — Satisfaction implies positive support**
- _Entities:_ `ExistsMemory`, `satisfied`, `support_count`
- _Property:_ If `is_satisfied(tid)` is true, then `support_count(tid) > 0`.
- _Generator:_ Arbitrary operation sequences.

**P1.6.3 — Support count accuracy**
- _Entities:_ `ExistsMemory::support_count`, `ExistsMemory::add_support`,
  `ExistsMemory::remove_support`
- _Property:_ `support_count(tid)` always equals the number of distinct `FactId`s in
  the support set for `tid`.
- _Generator:_ Sequences of add/remove support operations.

**P1.6.4 — Transition detection on add/remove**
- _Entities:_ `ExistsMemory::add_support`, `ExistsMemory::remove_support`
- _Property:_ `add_support` returns the new count. When the return goes from 0→1
  (first support), the caller should create a pass-through. `remove_support` returns
  `(new_count, was_removed)` where `new_count == 0` signals the last support was
  removed.
- _Generator:_ Sequences that build up and tear down support.

---

### 1.7 NCC Node Memory

**Location:** `crates/ferric-core/src/ncc.rs`

**Types involved:** `NccMemory`, `NccMemoryId`

**Description:** Tracks subnetwork result counts per parent token. A parent is blocked
when result count > 0 and unblocked when count == 0. Maintains result-to-parent
ownership tracking.

#### Properties

**P1.7.1 — Mutual exclusivity of blocked and unblocked**
- _Entities:_ `NccMemory`, `result_count`, `unblocked`
- _Property:_ No `TokenId` has both `result_count[tid] > 0` and
  `unblocked.contains_key(tid)` simultaneously.
- _Generator:_ Arbitrary add_result/remove_result/set_unblocked sequences.

**P1.7.2 — Result count accuracy**
- _Entities:_ `NccMemory`, `result_count`, `result_owner`
- _Property:_ For every parent token `P`, `result_count[P]` equals the number of
  entries in `result_owner` whose value is `P`.
- _Generator:_ Arbitrary operation sequences.

**P1.7.3 — Non-zero count invariant**
- _Entities:_ `NccMemory::result_count`
- _Property:_ The `result_count` map never contains an entry with value 0. When
  `decrement_results` reaches 0, the entry is removed.
- _Generator:_ Sequences that increment and decrement counts.

**P1.7.4 — Duplicate result token detection**
- _Entities:_ `NccMemory::add_result`
- _Property:_ If `add_result(parent, result)` is called when `result` is already
  tracked in `result_owner`, it returns no-op counts (old == new) and does not
  double-count.
- _Generator:_ Sequences with repeated add_result calls for the same result token.

**P1.7.5 — remove_parent_token complete cleanup**
- _Entities:_ `NccMemory::remove_parent_token`
- _Property:_ After `remove_parent_token(tid)`, `tid` does not appear in
  `result_count`, `unblocked`, or as a value in `result_owner`.
- _Generator:_ Memories with results and parent tokens, selective parent removal.

---

### 1.8 Agenda & Conflict Resolution

**Location:** `crates/ferric-core/src/agenda.rs`, `crates/ferric-core/src/strategy.rs`

**Types involved:** `Agenda`, `Activation`, `ActivationId`, `ActivationSeq`,
`AgendaKey`, `StrategyOrd`, `ConflictResolutionStrategy`

**Description:** The agenda maintains a priority-ordered collection of rule activations.
Priority is determined by: (1) salience (higher first), (2) strategy-specific ordering,
(3) activation sequence number (tiebreaker). The agenda maintains three synchronized
indices: `ordering` (BTreeMap for priority), `id_to_key` (reverse lookup), and
`token_to_activations` (token reverse index).

#### Properties

**P1.8.1 — Salience dominance across all strategies**
- _Entities:_ `Agenda::pop`, `Activation::salience`, all `ConflictResolutionStrategy` values
- _Property:_ For any two activations A and B where `A.salience > B.salience`, A is
  always popped before B regardless of strategy, timestamps, or recency vectors.
- _Generator:_ Pairs of activations with different salience values, tested across all
  four strategy variants.

**P1.8.2 — Depth strategy: most recent first**
- _Entities:_ `Agenda` with `Depth` strategy
- _Property:_ Among activations with equal salience, the activation with the highest
  (most recent) timestamp is popped first.
- _Generator:_ Sets of activations with same salience but different timestamps.

**P1.8.3 — Breadth strategy: least recent first**
- _Entities:_ `Agenda` with `Breadth` strategy
- _Property:_ Among activations with equal salience, the activation with the lowest
  (oldest) timestamp is popped first.
- _Generator:_ Sets of activations with same salience but different timestamps.

**P1.8.4 — LEX strategy: lexicographic recency comparison**
- _Entities:_ `Agenda` with `Lex` strategy
- _Property:_ Among activations with equal salience, position-by-position comparison
  of recency vectors determines order: at each position, the activation with the more
  recent (higher) timestamp at that position wins. If all positions match, longer
  recency vector wins.
- _Generator:_ Sets of activations with same salience, varying recency vectors.

**P1.8.5 — MEA strategy: first-pattern recency dominates**
- _Entities:_ `Agenda` with `Mea` strategy
- _Property:_ Among activations with equal salience, the activation whose first
  recency element is highest wins. On tie of first element, falls back to LEX ordering
  on remaining elements.
- _Generator:_ Sets of activations with same salience, varying recency vectors.

**P1.8.6 — Sequence number tiebreaker**
- _Entities:_ `Agenda`, `ActivationSeq`
- _Property:_ When salience and strategy ordering are identical, the activation added
  most recently (highest `ActivationSeq`) is popped first.
- _Generator:_ Multiple activations with identical salience, timestamps, and recency vectors.

**P1.8.7 — Three-index synchronization**
- _Entities:_ `Agenda`, `ordering`, `id_to_key`, `token_to_activations`
- _Property:_ After any sequence of `add`/`pop`/`remove_activations_for_token`
  operations:
  - Every `ActivationId` in `ordering` has a matching entry in `id_to_key` and
    vice versa
  - Every `ActivationId` in `ordering` references a live activation
  - Every token in `token_to_activations` maps to live activations whose `token`
    field matches the key
  - No empty entries in `token_to_activations`
- _Note:_ This is essentially the existing `debug_assert_consistency()` as a property test.
- _Generator:_ Arbitrary interleaving of add/pop/remove operations.

**P1.8.8 — pop returns highest priority**
- _Entities:_ `Agenda::pop`
- _Property:_ The activation returned by `pop()` has the highest `AgendaKey` according
  to the BTreeMap ordering (which means lowest Reverse<Salience>, then strategy, then
  Reverse<Seq>). No remaining activation in the agenda has higher priority.
- _Generator:_ Agendas with multiple activations, then pop and verify.

**P1.8.9 — remove_activations_for_token completeness**
- _Entities:_ `Agenda::remove_activations_for_token`
- _Property:_ After `remove_activations_for_token(tid)`, no activation with
  `token == tid` remains in the agenda. All other activations are undisturbed.
- _Generator:_ Agendas with activations across multiple tokens, removal of one token's activations.

**P1.8.10 — clear resets sequence counter**
- _Entities:_ `Agenda::clear`
- _Property:_ After `clear()`, `is_empty()` is true AND the next activation added
  receives `ActivationSeq(0)` (sequence counter reset).
- _Generator:_ Agendas with content, clear, then add.

---

### 1.9 Alpha Memory

**Location:** `crates/ferric-core/src/alpha.rs`

**Types involved:** `AlphaMemory`, `AlphaMemoryId`, `SlotIndex`, `AtomKey`

**Description:** Storage for facts that pass alpha-network filtering. Maintains a
main fact set plus per-slot secondary indices for efficient join lookups.

#### Properties

**P1.9.1 — Insert idempotency**
- _Entities:_ `AlphaMemory::insert`
- _Property:_ Inserting the same `FactId` twice does not change the memory's content
  or size.
- _Generator:_ Arbitrary FactIds with repeated insertions.

**P1.9.2 — Remove idempotency**
- _Entities:_ `AlphaMemory::remove`
- _Property:_ Removing a `FactId` not in the memory is a no-op.
- _Generator:_ Arbitrary FactIds against empty and populated memories.

**P1.9.3 — Slot index subset invariant**
- _Entities:_ `AlphaMemory`, `slot_indices`
- _Property:_ Every `FactId` appearing in any slot index entry is also in the main
  `facts` set.
- _Generator:_ Arbitrary insert/remove sequences with slot value extraction.

**P1.9.4 — Index backfill on request_index**
- _Entities:_ `AlphaMemory::request_index`
- _Property:_ After `request_index(slot, fact_base)`, the slot index contains entries
  for all existing facts' values at that slot. Subsequent inserts also maintain the
  new index.
- _Generator:_ Memories with existing facts, then request_index on a new slot.

**P1.9.5 — Clear preserves indexed_slots structure**
- _Entities:_ `AlphaMemory::clear`
- _Property:_ After `clear()`, `facts` and all slot index entries are empty, but the
  set of indexed slots is preserved (so subsequent inserts continue indexing).
- _Generator:_ Memories with indexed slots and facts, then clear and re-insert.

---

### 1.10 Beta Memory

**Location:** `crates/ferric-core/src/beta.rs`

**Types involved:** `BetaMemory`, `BetaMemoryId`

**Description:** Simple set of `TokenId`s owned by a beta network node. Tracks which
tokens are stored at each join/negative/exists/NCC node.

#### Properties

**P1.10.1 — Insert/remove idempotency**
- _Entities:_ `BetaMemory::insert`, `BetaMemory::remove`
- _Property:_ Inserting the same `TokenId` twice does not change the set. Removing a
  `TokenId` not present is a no-op.
- _Generator:_ Arbitrary TokenId sequences.

**P1.10.2 — Membership consistency**
- _Entities:_ `BetaMemory::contains`, `BetaMemory::insert`, `BetaMemory::remove`
- _Property:_ `contains(tid)` returns `true` iff `tid` was inserted and not
  subsequently removed.
- _Generator:_ Arbitrary insert/remove sequences with membership queries.

---

## Tier 2 — Subsystems

### 2.1 Alpha Network

**Location:** `crates/ferric-core/src/alpha.rs`

**Types involved:** `AlphaNetwork`, `AlphaNode`, `AlphaMemory`, `AlphaEntryType`,
`ConstantTest`

**Description:** The alpha network performs per-fact filtering. Facts enter via entry
nodes (dispatched by `AlphaEntryType` — ordered relation or template), pass through
constant test nodes (slot-value equality/inequality checks), and land in alpha
memories. A reverse index (`fact_to_memories`) enables efficient retraction.

#### Properties

**P2.1.1 — Assert-then-retract roundtrip**
- _Entities:_ `AlphaNetwork::assert_fact`, `AlphaNetwork::retract_fact`
- _Property:_ For any fact F asserted into the alpha network, after retraction:
  - F does not appear in any `AlphaMemory`
  - `fact_to_memories` has no entry for F's FactId
  - All memories pass internal consistency checks
- _Generator:_ Arbitrary facts (varying relation, slot values).

**P2.1.2 — Constant test correctness**
- _Entities:_ `AlphaNode::ConstantTest`, `ConstantTest`
- _Property:_ A fact passes through a constant test node iff the fact's value at the
  tested slot satisfies the test (equality or inequality against the target `AtomKey`).
  The fact lands in exactly the set of alpha memories whose paths it satisfies.
- _Generator:_ Facts with arbitrary slot values paired with networks containing
  various constant test configurations.

**P2.1.3 — Entry node idempotent creation**
- _Entities:_ `AlphaNetwork::create_entry_node`
- _Property:_ Multiple calls with the same `AlphaEntryType` return the same `NodeId`.
- _Generator:_ Repeated creation requests with same and different entry types.

**P2.1.4 — Reverse index completeness**
- _Entities:_ `AlphaNetwork::fact_to_memories`, `AlphaNetwork::memories_containing_fact`
- _Property:_ After asserting fact F, `memories_containing_fact(F.id)` returns exactly
  the set of `AlphaMemoryId`s that contain F. This set matches the memories reached
  by traversing the network from the entry node.
- _Generator:_ Networks with multiple paths; facts that match some but not all paths.

**P2.1.5 — Alpha network consistency under arbitrary operations**
- _Entities:_ `AlphaNetwork::debug_assert_consistency`
- _Property:_ After any sequence of node creation, fact assertion, and fact retraction
  operations, the network passes all consistency checks (all memory IDs valid, all
  child nodes exist, no duplicate children, slot indices subset of main facts).
- _Generator:_ Arbitrary operation sequences.

---

### 2.2 Beta Network

**Location:** `crates/ferric-core/src/beta.rs`

**Types involved:** `BetaNetwork`, `BetaNode`, `BetaMemory`, `NegativeMemory`,
`ExistsMemory`, `NccMemory`, `JoinTest`, `BindingExtraction`

**Description:** The beta network performs inter-pattern joins. Tokens flow left
(from parent nodes carrying partial matches) and facts flow right (from alpha
memories). Join nodes create new tokens when join tests pass. Negative, exists, and
NCC nodes implement specialized blocking/support semantics.

This is the **highest-priority subsystem for expanded property testing** given its
complexity (978 lines) and current under-testing (only 4 unit tests).

#### Properties

**P2.2.1 — Join node produces correct token combinations**
- _Entities:_ `BetaNode::Join`, `JoinTest`, `BindingExtraction`
- _Property:_ When a join node is right-activated with fact F and has parent tokens
  in its parent's beta memory, a new child token is created for each parent token P
  where all `JoinTest`s pass (the alpha slot value matches the beta binding value).
  The child token's bindings are the merge of P's bindings plus new extractions from F.
- _Generator:_ Arbitrary parent tokens with bindings, facts with varying slot values,
  join tests requiring specific slot-to-binding matches.

**P2.2.2 — Join test evaluation correctness**
- _Entities:_ `JoinTest`, `Value::structural_eq`
- _Property:_ A join test passes iff the fact's value at `alpha_slot` is
  `structural_eq` to the parent token's binding at `beta_var`.
- _Generator:_ Arbitrary `(Value, Value)` pairs for the join comparison.

**P2.2.3 — Negative node blocking semantics**
- _Entities:_ `BetaNode::Negative`, `NegativeMemory`
- _Property:_ When a parent token enters a negative node:
  - If any fact in the associated alpha memory passes all join tests, the parent
    token is blocked (each matching fact registered as a blocker)
  - If no facts match, a pass-through token is created and propagated downstream
  - When all blockers are subsequently retracted, the parent becomes unblocked and
    a new pass-through is created
- _Generator:_ Networks with negative nodes, varying alpha memory contents.

**P2.2.4 — Exists node support semantics**
- _Entities:_ `BetaNode::Exists`, `ExistsMemory`
- _Property:_ When a parent token enters an exists node:
  - If at least one fact in the alpha memory passes join tests, a pass-through is
    created (support count transitions 0→1)
  - Additional matching facts increase support count but do not create additional
    pass-throughs
  - Only when the last supporting fact is retracted (count N→0) is the pass-through
    retracted
- _Generator:_ Networks with exists nodes, adding and removing supporting facts.

**P2.2.5 — NCC node blocking semantics**
- _Entities:_ `BetaNode::Ncc`, `BetaNode::NccPartner`, `NccMemory`
- _Property:_ An NCC node's parent token is:
  - Unblocked (pass-through created) when the subnetwork produces 0 result tokens
  - Blocked (pass-through retracted) when the subnetwork produces >= 1 result tokens
  - Transitions occur on the 0→1 and N→0 boundaries only
- _Generator:_ Networks with NCC patterns, varying subnetwork match results.

**P2.2.6 — Beta network consistency after arbitrary operations**
- _Entities:_ `BetaNetwork::debug_assert_consistency`
- _Property:_ After any sequence of node creation, left/right activation, and
  retraction operations:
  - All child/parent references point to existing nodes
  - All memory IDs reference existing memories
  - All reverse indices (alpha_to_joins, alpha_to_negatives, alpha_to_exists) are
    consistent with node registrations
  - Root node exists and is the Root variant
  - All subordinate memories pass their own consistency checks
- _Generator:_ Arbitrary operation sequences.

**P2.2.7 — Binding extraction correctness**
- _Entities:_ `BindingExtraction`, `BindingSet`
- _Property:_ When a join node extracts bindings from a fact, for each
  `BindingExtraction { alpha_slot, beta_var }`, the fact's value at `alpha_slot` is
  stored in the child token's `BindingSet` at `beta_var`.
- _Generator:_ Facts with arbitrary slot values, extraction specs with varying slot
  indices and VarIds.

**P2.2.8 — Token tree structure preservation**
- _Entities:_ `Token::parent`, `TokenStore`
- _Property:_ After any sequence of join activations:
  - Every token has at most one parent
  - The parent chain is acyclic
  - Each token is in exactly one beta memory (determined by `owner_node`)
- _Generator:_ Multi-level join networks producing deep token chains.

---

### 2.3 Rete Network (Integrated)

**Location:** `crates/ferric-core/src/rete.rs`

**Types involved:** `ReteNetwork`, `AlphaNetwork`, `BetaNetwork`, `TokenStore`,
`Agenda`, `FactBase`

**Description:** The top-level Rete network coordinates alpha filtering, beta joining,
token management, and agenda maintenance. This is the primary subsystem for verifying
end-to-end invariants of the pattern matching engine.

#### Properties

**P2.3.1 — Assert produces correct activations**
- _Entities:_ `ReteNetwork::assert_fact`
- _Property:_ After asserting a fact, the set of new activations on the agenda
  corresponds exactly to the rules whose patterns are fully satisfied by the current
  working memory contents. No spurious activations, no missing activations.
- _Generator:_ Simple rule networks (1-3 patterns) with facts that match or don't
  match various pattern positions.

**P2.3.2 — Retract removes exactly dependent activations**
- _Entities:_ `ReteNetwork::retract_fact`
- _Property:_ After retracting a fact, every activation whose token chain included
  that fact is removed from the agenda. Activations not depending on the retracted
  fact are preserved.
- _Generator:_ Networks with multiple rules, shared and non-shared facts.

**P2.3.3 — Assert-retract-assert consistency**
- _Entities:_ `ReteNetwork::assert_fact`, `ReteNetwork::retract_fact`
- _Property:_ After asserting fact F, retracting F, then asserting F again (possibly
  with a different FactId), the network state is equivalent to having only the second
  assertion active. No residual state from the first assertion.
- _Generator:_ Arbitrary facts against compiled rule networks.

**P2.3.4 — Cross-structure consistency**
- _Entities:_ `ReteNetwork::debug_assert_consistency`
- _Property:_ After any sequence of fact assertion and retraction operations:
  - Every token in every beta memory exists in the TokenStore
  - Every activation's token exists in the TokenStore
  - Alpha, beta, token store, and agenda all pass individual consistency checks
  - No dangling references across subsystems
- _Generator:_ Arbitrary sequences of assert/retract operations against compiled networks.

**P2.3.5 — Clear working memory completeness**
- _Entities:_ `ReteNetwork::clear_working_memory`
- _Property:_ After `clear_working_memory()`:
  - All alpha memories are empty
  - All beta memories are empty
  - Token store is empty
  - Agenda is empty
  - All negative/exists/NCC memories are empty
  - Network structure (nodes, connections) is preserved
- _Generator:_ Networks with content, then clear, then verify emptiness.

**P2.3.6 — Negative node interaction with assertion/retraction**
- _Entities:_ `ReteNetwork`, negative node processing
- _Property:_ For a rule with a negative pattern `(not (foo ?x))`:
  - When no `(foo ...)` fact exists, the rule activates
  - When a matching `(foo ...)` fact is asserted, the activation is removed
  - When that fact is retracted, the activation reappears
  - The net effect is correct regardless of operation order
- _Generator:_ Sequences of assert/retract for positive and negative patterns.

**P2.3.7 — Multi-pattern join correctness**
- _Entities:_ `ReteNetwork`, join nodes with variable bindings
- _Property:_ For a rule with patterns `(a ?x)(b ?x)`, the rule activates only
  when both facts exist AND the shared variable `?x` binds to the same value in both
  patterns.
- _Generator:_ Pairs of facts with matching and non-matching variable values.

---

### 2.4 Compiler

**Location:** `crates/ferric-core/src/compiler.rs`, `crates/ferric-core/src/validation.rs`

**Types involved:** `ReteCompiler`, `CompilableRule`, `CompilablePattern`,
`CompilableCondition`, `CompileResult`, `CompileError`, `PatternValidationError`

**Description:** Translates parsed rule representations into Rete network nodes. The
compiler shares alpha paths and join nodes when possible (via caching), and validates
patterns for structural correctness.

#### Properties

**P2.4.1 — Alpha path sharing**
- _Entities:_ `ReteCompiler`, `alpha_path_cache`
- _Property:_ Two rules whose first pattern has the same `AlphaEntryType` and
  `constant_tests` share the same `AlphaMemoryId`. Different entry types or test
  configurations produce different alpha memories.
- _Generator:_ Pairs of rules with identical and differing first-pattern specs.

**P2.4.2 — Join node sharing**
- _Entities:_ `ReteCompiler`, `join_node_cache`
- _Property:_ Two rules whose first two patterns produce the same
  `(parent_node, alpha_memory, join_tests, binding_extractions)` tuple share the same
  join node.
- _Generator:_ Pairs of rules with identical and differing two-pattern specs.

**P2.4.3 — Variable binding consistency**
- _Entities:_ `ReteCompiler`, `VarMap`, compilation pipeline
- _Property:_ After compiling a rule:
  - Every variable appearing in a pattern is in the `VarMap`
  - The first occurrence of a variable in pattern order creates a `BindingExtraction`
  - Subsequent occurrences of the same variable create `JoinTest`s
  - Variables in negated/exists patterns that are already bound create join tests
    (no new bindings inside negation/exists)
- _Generator:_ Rules with varying variable usage patterns.

**P2.4.4 — Compilation rejects invalid rules**
- _Entities:_ `ReteCompiler::compile_rule`, `CompileError`
- _Property:_ Compiling a rule with zero patterns returns `CompileError::EmptyRule`.
  Compiling a rule that violates validation constraints returns
  `CompileError::Validation` with appropriate error codes.
- _Generator:_ Deliberately invalid rules (empty, too-deep nesting, unsupported
  combinations).

**P2.4.5 — Compiled network structural correctness**
- _Entities:_ `CompileResult`, `BetaNetwork`, `AlphaNetwork`
- _Property:_ After compiling a rule:
  - `terminal_node` exists in the beta network and is a `Terminal` variant
  - All `alpha_memories` exist in the alpha network
  - The path from root to terminal follows the pattern structure (join nodes for
    positive patterns, negative nodes for negated patterns, etc.)
- _Generator:_ Rules with varying pattern types (positive, negative, exists, NCC).

---

### 2.5 Parser Pipeline

**Location:** `crates/ferric-parser/src/lexer.rs`, `crates/ferric-parser/src/sexpr.rs`,
`crates/ferric-parser/src/stage2.rs`

**Types involved:** `Token`, `SpannedToken`, `SExpr`, `Atom`, `Construct`,
`RuleConstruct`, `Pattern`, `Constraint`

**Description:** Three-stage pipeline: lexer (characters → tokens), S-expression
parser (tokens → tree), and Stage 2 interpreter (tree → typed constructs). Each stage
collects errors and attempts recovery.

#### Properties

**P2.5.1 — Lexer token span correctness**
- _Entities:_ `lex`, `SpannedToken`, `Span`
- _Property:_ For every token produced by lexing, the span's byte range when sliced
  from the original source produces text consistent with the token's value.
- _Generator:_ Arbitrary valid CLIPS source strings.

**P2.5.2 — Lexer roundtrip for literals**
- _Entities:_ `lex`, `Token::Integer`, `Token::Float`, `Token::String`
- _Property:_ For integer tokens, the `i64` value when formatted matches the source
  text (ignoring leading `+`). For string tokens, the content matches the source text
  with escape sequences resolved.
- _Generator:_ Arbitrary integer, float, and string literals.

**P2.5.3 — S-expression parenthesis matching**
- _Entities:_ `parse_sexprs`, `SExpr::List`
- _Property:_ Every `SExpr::List` in a successful parse (no unclosed-paren errors)
  corresponds to a matched pair of parentheses in the source.
- _Generator:_ Arbitrary well-formed parenthesized expressions.

**P2.5.4 — S-expression span coverage**
- _Entities:_ `parse_sexprs`, `Span`
- _Property:_ For every `SExpr`, its span covers from the first character of the
  expression to the last character (inclusive of closing paren for lists, inclusive
  of the token for atoms).
- _Generator:_ Arbitrary well-formed CLIPS expressions.

**P2.5.5 — Stage 2 construct completeness**
- _Entities:_ `interpret_constructs`, `Construct`
- _Property:_ Every `(defXXX ...)` form in the input produces a corresponding
  `Construct` variant in the output (when the form is valid). The construct's name
  matches the name in the source.
- _Generator:_ Arbitrary valid `defrule`, `deftemplate`, `deffacts`, `deffunction`,
  `defglobal`, `defmodule` forms.

**P2.5.6 — Error recovery does not lose constructs**
- _Entities:_ `interpret_constructs`, `InterpretResult`
- _Property:_ When one construct in a multi-construct source has an error, the other
  valid constructs are still successfully interpreted and returned.
- _Generator:_ Sources with mixtures of valid and invalid constructs.

---

### 2.6 Expression Evaluator

**Location:** `crates/ferric-runtime/src/evaluator.rs`

**Types involved:** `RuntimeExpr`, `EvalContext`, `EvalError`, `Value`

**Description:** Evaluates runtime expressions (literals, variable references, function
calls) in the context of variable bindings, globals, and function definitions. Handles
arithmetic, comparison, string operations, and user-defined function dispatch.

#### Properties

**P2.6.1 — Literal evaluation is identity**
- _Entities:_ `eval`, `RuntimeExpr::Literal`
- _Property:_ Evaluating `RuntimeExpr::Literal(v)` always returns `Ok(v)` regardless
  of context.
- _Generator:_ Arbitrary `Value` instances wrapped in `Literal`.

**P2.6.2 — Arithmetic operation correctness**
- _Entities:_ `eval`, builtin arithmetic functions (`+`, `-`, `*`, `/`)
- _Property:_ For integer operands: `(+ a b)` returns `Value::Integer(a + b)` when no
  overflow occurs. When any operand is float, result is float. Division by zero
  returns `DivisionByZero` error.
- _Generator:_ Arbitrary numeric values as operands.

**P2.6.3 — Comparison operation correctness**
- _Entities:_ `eval`, comparison functions (`=`, `<>`, `<`, `>`, `<=`, `>=`)
- _Property:_ Comparison results match the expected mathematical ordering for numeric
  values. String comparisons follow lexicographic byte ordering.
- _Generator:_ Pairs of numeric and string values.

**P2.6.4 — Recursion limit enforcement**
- _Entities:_ `eval`, `EvalContext::call_depth`, `EngineConfig::max_call_depth`
- _Property:_ A recursive function that exceeds `max_call_depth` returns
  `EvalError::RecursionLimit`. The error includes the function name and depth.
- _Generator:_ Recursive function definitions with varying depth limits.

**P2.6.5 — Variable lookup correctness**
- _Entities:_ `eval`, `RuntimeExpr::BoundVar`, `RuntimeExpr::GlobalVar`
- _Property:_ `BoundVar { name }` returns the value from `bindings` via `var_map`.
  `GlobalVar { name }` returns the value from `globals`. Both return appropriate
  errors when the variable is unbound.
- _Generator:_ Contexts with varying bound/unbound variables.

**P2.6.6 — Truthiness follows CLIPS convention**
- _Entities:_ `is_truthy`
- _Property:_ Only `Value::Void` and the symbol `FALSE` are falsy. Everything else
  is truthy, including `0`, `0.0`, empty string, and empty multifield.
- _Generator:_ All `Value` variants.

---

### 2.7 Module System & Visibility

**Location:** `crates/ferric-runtime/src/modules.rs`

**Types involved:** `ModuleRegistry`, `ModuleId`, `RuntimeModule`, `ModuleSpec`,
`ImportSpec`

**Description:** Manages module registration, export/import declarations, construct
visibility, and the focus stack for rule execution.

#### Properties

**P2.7.1 — MAIN module always exists**
- _Entities:_ `ModuleRegistry`
- _Property:_ After construction and after any sequence of operations, the MAIN module
  (ID 0) exists and its name is "MAIN".
- _Generator:_ Arbitrary module registration/removal sequences.

**P2.7.2 — Module registration idempotency for names**
- _Entities:_ `ModuleRegistry::register`
- _Property:_ Registering a module with the same name twice returns the same `ModuleId`
  both times (the second call updates exports/imports but reuses the ID).
- _Generator:_ Repeated registration with same name, varying exports/imports.

**P2.7.3 — Visibility is symmetric with export/import**
- _Entities:_ `ModuleRegistry::is_construct_visible`
- _Property:_ A construct in module A is visible from module B iff:
  - A == B (same-module always visible), OR
  - A exports the construct AND B imports from A (matching construct type and name)
- _Generator:_ Module pairs with varying export/import specs, construct names.

**P2.7.4 — Focus stack never empty**
- _Entities:_ `ModuleRegistry::focus_stack`
- _Property:_ After construction and after any sequence of `push_focus`/`pop_focus`/
  `set_focus`/`reset_focus` operations, `focus_stack` is never empty.
- _Generator:_ Arbitrary focus stack operations.

**P2.7.5 — Name-to-ID bidirectional consistency**
- _Entities:_ `ModuleRegistry`, `name_to_id`, `modules`
- _Property:_ `name_to_id[name] == id` iff `modules[id].name == name`. Every module
  in `modules` has a corresponding entry in `name_to_id` and vice versa.
- _Generator:_ Arbitrary module registration sequences.

---

### 2.8 FFI Boundary

**Location:** `crates/ferric-ffi/src/engine.rs`, `crates/ferric-ffi/src/error.rs`,
`crates/ferric-ffi/src/types.rs`

**Types involved:** `FerricEngine`, `FerricError`, `FerricValue`, `FerricConfig`,
`FerricStringEncoding`, `FerricConflictStrategy`, `FerricValueType`,
`EngineErrorState`

**Description:** C-ABI interface layer. All interactions go through `extern "C"`
functions with explicit pointer validation, thread affinity checks, and error code
returns. Values are converted between Rust (`Value`) and C (`FerricValue`)
representations.

#### Properties

**P2.8.1 — FerricStringEncoding roundtrip**
- _Entities:_ `FerricStringEncoding::as_raw`, `FerricStringEncoding::from_raw`
- _Property:_ For every valid `FerricStringEncoding` variant `e`,
  `FerricStringEncoding::from_raw(e.as_raw()) == Some(e)`.
- _Generator:_ All three encoding variants.

**P2.8.2 — FerricConflictStrategy roundtrip**
- _Entities:_ `FerricConflictStrategy::as_raw`, `FerricConflictStrategy::from_raw`
- _Property:_ For every valid `FerricConflictStrategy` variant `s`,
  `FerricConflictStrategy::from_raw(s.as_raw()) == Some(s)`.
- _Generator:_ All four strategy variants.

**P2.8.3 — Invalid discriminant rejection**
- _Entities:_ `FerricStringEncoding::from_raw`, `FerricConflictStrategy::from_raw`
- _Property:_ `from_raw(x)` returns `None` for any `u32` value `x` outside the valid
  discriminant range.
- _Generator:_ Arbitrary `u32` values including boundary values (3 for encoding,
  4 for strategy, `u32::MAX`).

**P2.8.4 — FerricConfig validation**
- _Entities:_ `TryFrom<&FerricConfig> for EngineConfig`
- _Property:_ Conversion succeeds iff both `string_encoding` and `strategy` fields
  contain valid discriminant values. On success, the resulting `EngineConfig` reflects
  the correct encoding and strategy.
- _Generator:_ `FerricConfig` with valid and invalid field combinations.

**P2.8.5 — Error code stability**
- _Entities:_ `FerricError`
- _Property:_ All error code numeric values match their documented constants:
  `Ok=0, NullPointer=1, ThreadViolation=2, NotFound=3, ParseError=4, CompileError=5,
  RuntimeError=6, IoError=7, BufferTooSmall=8, InvalidArgument=9, InternalError=99`.
- _Note:_ This is an ABI contract — values must never change.
- _Generator:_ Enumerate all variants and check discriminant values.

**P2.8.6 — Error mapping completeness**
- _Entities:_ `map_engine_error`, `map_load_error`
- _Property:_ Every `EngineError` variant maps to a specific `FerricError` (not
  just `InternalError` for known variants). Every `LoadError` variant maps to a
  specific `FerricError`.
- _Generator:_ All `EngineError` and `LoadError` variants.

**P2.8.7 — Null pointer safety**
- _Entities:_ All `extern "C"` functions
- _Property:_ Passing a null pointer for the `engine` parameter returns
  `FerricError::NullPointer` (not a crash). Passing null for optional output pointers
  (like `out_fired`, `out_status`) is a no-op (output simply not written).
- _Generator:_ All FFI entry points called with null engine pointers.

**P2.8.8 — Buffer copy contract**
- _Entities:_ `ferric_engine_last_error_copy`, `ferric_last_error_global_copy`,
  `ferric_engine_action_diagnostic_copy`
- _Property:_ The copy functions follow a precise contract:
  - `buf=null, buf_len=0`: size query, returns needed size in `out_len`, returns `Ok`
  - `buf=valid, buf_len>=needed`: full copy, writes `out_len = bytes_written`, returns `Ok`
  - `buf=valid, buf_len<needed`: truncated copy with NUL terminator, writes
    `out_len = full_needed_size`, returns `BufferTooSmall`
  - `out_len=null`: returns `InvalidArgument`
- _Generator:_ All combinations of null/non-null buf, various buf_len values, and
  error messages of varying lengths.

**P2.8.9 — Value conversion roundtrip**
- _Entities:_ `value_to_ferric`, `FerricValue`, `Value`
- _Property:_ For every `Value` variant (except `ExternalAddress`, which has special
  ownership semantics), converting to `FerricValue` and inspecting the result
  preserves the original value:
  - Integer: `fv.integer == i`
  - Float: `fv.float.to_bits() == f.to_bits()`
  - Symbol/String: `CStr::from_ptr(fv.string_ptr) == original_text`
  - Multifield: recursive element-wise preservation
  - Void: `fv.value_type == FerricValueType::Void`
- _Generator:_ Arbitrary `Value` instances.

---

## Tier 3 — Cross-Cutting Concerns

### 3.1 Fact Lifecycle

**Types involved:** `FactBase`, `AlphaNetwork`, `BetaNetwork`, `TokenStore`, `Agenda`,
`ReteNetwork`

**Description:** The complete lifecycle of a fact from assertion through alpha
filtering, beta joining, activation creation, to retraction and cascading cleanup.
This is the most critical cross-component property testing area.

#### Properties

**P3.1.1 — Assertion propagation completeness**
- _Entities:_ `ReteNetwork::assert_fact`, `AlphaNetwork`, `BetaNetwork`, `Agenda`
- _Property:_ After asserting a set of facts that fully satisfies a compiled rule's
  patterns, the agenda contains an activation for that rule. The activation's token
  chain contains exactly the facts matching each pattern position.
- _Generator:_ Compiled rules with 1-3 patterns, fact sets that satisfy all patterns.

**P3.1.2 — Retraction cascading completeness**
- _Entities:_ `ReteNetwork::retract_fact`, `TokenStore`, `Agenda`
- _Property:_ After retracting a fact:
  - No token in any beta memory or token store references the retracted fact
  - No activation on the agenda has a token chain containing the retracted fact
  - All negative nodes that were blocked by the retracted fact have been unblocked
    (and their pass-throughs created)
  - All exists nodes that lost their last support from the retracted fact have their
    pass-throughs removed
- _Generator:_ Networks with mixed positive/negative/exists patterns, retraction of
  various facts.

**P3.1.3 — Working memory state determinism**
- _Entities:_ `ReteNetwork`, `FactBase`
- _Property:_ For any set of facts S asserted in any order, the final set of
  activations on the agenda is the same (order of activations may differ, but the
  set is identical).
- _Note:_ This tests that assertion order does not affect which rules are activated.
- _Generator:_ Permutations of fact assertion order for the same fact set.

**P3.1.4 — Empty working memory after full retraction**
- _Entities:_ `ReteNetwork`, `FactBase`
- _Property:_ If all asserted facts are retracted (in any order), the network returns
  to its initial state: no tokens, no activations, all memories empty.
- _Generator:_ Assert N facts, then retract all N in arbitrary order.

---

### 3.2 Rule Compilation-to-Execution Pipeline

**Types involved:** `ReteCompiler`, `ReteNetwork`, `Agenda`, `CompiledRuleInfo`,
`Engine`

**Description:** The full path from CLIPS source text through parsing, compilation,
fact assertion, activation, and rule firing.

#### Properties

**P3.2.1 — Compiled rule fires with correct bindings**
- _Entities:_ `Engine::load_str`, `Engine::run`, `CompiledRuleInfo`
- _Property:_ When a rule fires, the bindings available to its RHS actions match the
  variable values determined by the LHS patterns. For a rule like
  `(defrule r (a ?x) (b ?x) => (assert (c ?x)))`, after firing, the asserted fact
  `(c ?x)` carries the value that `?x` was bound to.
- _Generator:_ Simple rules with shared variables between LHS and RHS.

**P3.2.2 — Test CE prevents firing**
- _Entities:_ `execute_actions`, `is_truthy`, test CE evaluation
- _Property:_ If a rule has a `(test ...)` CE that evaluates to falsy, the rule's
  RHS actions are not executed even though the rule was popped from the agenda.
- _Generator:_ Rules with test CEs that are true/false for different binding values.

**P3.2.3 — Rule firing consumes activation**
- _Entities:_ `Engine::step`, `Agenda`
- _Property:_ After `step()` fires a rule, that exact activation is no longer on the
  agenda (it was popped). If the same fact combination still matches, a new activation
  would need to be re-created (which doesn't happen automatically — facts must be
  re-asserted).
- _Generator:_ Single-rule networks, verify agenda emptiness after firing.

---

### 3.3 Reset & Clear Semantics

**Types involved:** `Engine`

**Description:** `reset()` returns the engine to its initial runtime state while
preserving compiled constructs. `clear()` wipes everything.

#### Properties

**P3.3.1 — Reset preserves compiled rules**
- _Entities:_ `Engine::reset`
- _Property:_ After `reset()`:
  - All compiled rules still exist (`rule_info` is preserved)
  - All registered templates still exist
  - All registered functions still exist
  - Working memory is empty except for `(initial-fact)` and deffacts
  - Globals are re-initialized to their registered initial values
  - Focus stack is `[MAIN]`
- _Generator:_ Engines with loaded rules/templates/functions/globals, then reset.

**P3.3.2 — Reset re-asserts deffacts**
- _Entities:_ `Engine::reset`
- _Property:_ After `reset()`, all facts from registered `deffacts` constructs are
  re-asserted and present in working memory.
- _Generator:_ Engines with deffacts, assert additional facts, reset, verify only
  deffacts facts remain.

**P3.3.3 — Clear removes everything**
- _Entities:_ `Engine::clear`
- _Property:_ After `clear()`:
  - No rules, templates, functions, or globals remain
  - Working memory is empty
  - Rete network is empty (no nodes beyond root)
  - Module registry contains only MAIN
- _Generator:_ Fully populated engines, then clear.

**P3.3.4 — Post-reset execution equivalence**
- _Entities:_ `Engine::reset`, `Engine::run`
- _Property:_ An engine that loads source S, runs to completion, calls `reset()`, and
  runs again produces the same execution trace (same rules fired in same order with
  same bindings) as a fresh engine that loads S and runs once.
- _Note:_ This is a strong property that may need careful scoping (e.g., deterministic
  conflict resolution strategy, no I/O side effects).
- _Generator:_ Deterministic CLIPS programs with known execution traces.

---

### 3.4 Thread Affinity

**Types involved:** `Engine`, `FerricEngine` (FFI)

**Description:** Both the Rust `Engine` and the FFI `FerricEngine` enforce thread
affinity: all operations must occur on the creating thread.

#### Properties

**P3.4.1 — Cross-thread operation rejection**
- _Entities:_ `Engine::check_thread_affinity`
- _Property:_ Any public method called on an `Engine` from a different thread than the
  one that created it returns `EngineError::WrongThread`.
- _Generator:_ Create engine on thread A, call methods from thread B.

**P3.4.2 — Same-thread operation acceptance**
- _Entities:_ `Engine::check_thread_affinity`
- _Property:_ Any public method called on an `Engine` from the same thread that
  created it does not return `EngineError::WrongThread`.
- _Generator:_ Create engine and call methods on the same thread.

**P3.4.3 — FFI thread violation returns correct error code**
- _Entities:_ `ferric_engine_*` FFI functions, `FerricError::ThreadViolation`
- _Property:_ Calling any mutating FFI function from a different thread returns
  `FerricError::ThreadViolation` (value 2).
- _Generator:_ Create FFI engine on thread A, call from thread B.

---

## Implementation Notes

### Recommended Tooling

All property tests should use the [`proptest`](https://docs.rs/proptest) crate, which
is already a dependency in `ferric-core`. The existing `Arbitrary` implementations for
core types (e.g., `Value`, `AtomKey`, `ConstantTest`) should be reused and extended.

### Priority Order for Implementation

1. **Tier 2.2 (Beta Network)** — Highest impact. Only 4 existing tests for 978 lines
   of complex join/blocking logic. Start with P2.2.1 through P2.2.5.
2. **Tier 2.3 (Rete Network Integrated)** — Second highest. Validates cross-component
   interactions. P2.3.1 through P2.3.4 are the most important.
3. **Tier 1.8 (Agenda)** — Third. The ordering properties (P1.8.1-P1.8.6) are
   critical for correctness and highly amenable to property testing.
4. **Tier 1.5-1.7 (Specialized Memories)** — The bidirectional index and mutual
   exclusivity invariants are prime property test material.
5. **Tier 1.2-1.3 (Fact/Token Storage)** — Index consistency properties under
   arbitrary operation sequences.
6. **Tier 3.1 (Fact Lifecycle)** — Cross-cutting lifecycle properties. These are
   the most complex to implement but provide the highest confidence.
7. **Tier 1.1 (Value System)** — Some properties already tested; expand coverage
   for edge cases (NaN, encoding boundaries).
8. **Remaining Tiers** — Fill in based on development needs.

### Arbitrary Instance Generation Strategy

Many properties require generating valid inputs for complex types. Key generators
to build:

- **Arbitrary `CompilableRule`**: Generate 1-4 patterns with plausible slot counts,
  variable distributions, and constant tests.
- **Arbitrary `FactBase` state**: Generate sequences of assert/retract operations
  to produce realistic fact base states.
- **Arbitrary `TokenStore` state**: Generate tree-structured token insertions with
  varying depths and branching factors.
- **Arbitrary `Agenda` state**: Generate activations with varying salience, timestamps,
  and recency vectors.
- **Arbitrary `NegativeMemory`/`ExistsMemory`/`NccMemory` state**: Generate sequences
  of blocking/support operations to produce realistic memory states.

### Existing Proptest Infrastructure

The codebase already has `proptest` regression files in
`crates/ferric-core/proptest-regressions/`. The existing `Arbitrary` implementations
and strategies should be consulted and extended rather than rewritten.

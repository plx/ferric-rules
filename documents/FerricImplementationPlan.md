# Ferric: A CLIPS-Inspired Rules Engine in Rust

## Implementation Plan

**Version:** 10.1  
**Date:** February 2026  
**Status:** Final

---

## Table of Contents

1. [Project Overview](#1-project-overview)
2. [Design Principles](#2-design-principles)
3. [Architecture](#3-architecture)
4. [Crate Structure](#4-crate-structure)
5. [Core Data Structures](#5-core-data-structures)
6. [Rete Network Implementation](#6-rete-network-implementation)
7. [Negation and Existential Semantics](#7-negation-and-existential-semantics)
8. [Parser and Language](#8-parser-and-language)
9. [Runtime Environment](#9-runtime-environment)
10. [Standard Library](#10-standard-library)
11. [C FFI Layer](#11-c-ffi-layer)
12. [CLI and REPL](#12-cli-and-repl)
13. [Testing Strategy](#13-testing-strategy)
14. [Performance Considerations](#14-performance-considerations)
15. [Implementation Phases](#15-implementation-phases)
16. [Compatibility Documentation](#16-compatibility-documentation)
17. [Appendix A: Dropped Features](#appendix-a-dropped-features)

---

## 1. Project Overview

### 1.1 What is Ferric?

Ferric is a modern, embeddable forward-chaining rules engine implemented in Rust. It is inspired by CLIPS (C Language Integrated Production System) and aims for high compatibility with CLIPS rule syntax and semantics while benefiting from Rust's safety guarantees, modern tooling, and cross-platform support.

The name "Ferric" references both Rust (ferric oxide) and the iron-clad reliability we aim to provide.

### 1.2 Goals

1. **Semantic Compatibility:** Rules written for CLIPS (within the supported subset) should execute identically in Ferric without modification.

2. **Embeddability:** Provide a C-compatible FFI layer enabling integration into applications written in C, C++, Swift, Kotlin (Android NDK), and other languages with C FFI support.

3. **Safety:** Leverage Rust's ownership model to eliminate memory safety bugs that plague C codebases.

4. **Modernization:** Update the architecture for contemporary development practices while preserving the proven Rete algorithm at its core.

5. **Independence:** Multiple engine instances must coexist within a single process without interference.

### 1.3 Non-Goals

- Full CLIPS compatibility (COOL object system, certainty factors, and other features are explicitly out of scope)
- Being a drop-in replacement for the CLIPS binary
- Providing distributed or networked rule evaluation
- Supporting probabilistic or fuzzy logic (these can be separate projects)

### 1.4 Licensing

Ferric will be dual-licensed under MIT and Apache 2.0, allowing users to choose the license that best fits their needs. All dependencies must be compatible with this dual-license approach.

---

## 2. Design Principles

### 2.1 No Global State

Ferric avoids process-wide mutable state for engine behavior. There are only two tightly-scoped process-level facilities:

1. A one-time `ferric::init()` initialization hook
2. Thread-local last-error storage used by the C FFI for pre-engine failures

Neither facility stores rule network state, facts, agenda state, templates, or runtime configuration. Every engine instance remains fully independent:

```rust
// One-time process setup (optional unless embedding requires it)
ferric::init();

// All rule execution state is instance-local
let engine_a = Engine::new(EngineConfig::default())?;
let engine_b = Engine::new(EngineConfig::strict())?;

// Engines share no mutable runtime state.
// Each engine is thread-affine and must be used on its owning thread.
```

**Threading model:** Engine instances are **thread-affine** (`!Send + !Sync`). This means:

- An engine **cannot** be shared between threads.
- Cross-thread transfer is only possible via explicit `unsafe` handoff (`move_to_current_thread`), with strict invariants.
- Multiple independent engines can exist on different threads simultaneously—they share nothing.

This is enforced at the type level, not just by documentation:

```rust
pub struct Engine {
    // ... engine fields ...

    /// Prevents Engine from being Send or Sync.
    /// This is deliberate: engines are thread-affine because they use Rc<Value>
    /// internally (cheaper than Arc) and rely on thread-local error storage.
    /// See §2.1 for rationale.
    _not_send: PhantomData<Rc<()>>,
    
    /// Thread ID of the thread that created this engine.
    /// Checked on every public entry point (debug: assert!, release: error return).
    /// See §11.2 for FFI-level enforcement.
    creator_thread_id: std::thread::ThreadId,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        Ok(Engine {
            // ... field initialization ...
            _not_send: PhantomData,
            creator_thread_id: std::thread::current().id(),
        })
    }
    
    /// Verify we're on the creating thread. Called at the start of every
    /// public method. Panics in debug builds; returns Err in release.
    #[inline]
    fn check_thread(&self) -> Result<(), EngineError> {
        let current = std::thread::current().id();
        if current != self.creator_thread_id {
            #[cfg(debug_assertions)]
            panic!(
                "Ferric engine accessed from wrong thread \
                 (created on {:?}, called from {:?})",
                self.creator_thread_id, current
            );
            #[cfg(not(debug_assertions))]
            return Err(EngineError::ThreadViolation {
                creator: self.creator_thread_id,
                caller: current,
            });
        }
        Ok(())
    }
}

// For the rare case where a caller needs to move an engine across threads
// (e.g., spawn_blocking), use the official transfer API:

impl Engine {
    /// Transfer ownership of this engine to the current (new) thread.
    ///
    /// # Safety
    /// The caller must ensure:
    ///   1. The engine is not accessed from the original thread after this call.
    ///   2. No `Rc<Value>` references (e.g., from `get_fact()` or `query()`)
    ///      have escaped to code that will continue to run on the original thread.
    ///      See "common violations" below.
    ///   3. This method resets the engine's per-instance error state.
    ///      Thread-local (TLS) error storage (`ferric_last_error_global`) is
    ///      per-thread by nature and does not need explicit clearing — the new
    ///      thread has its own independent TLS slot.
    ///
    /// This method updates `creator_thread_id` so that subsequent `check_thread()`
    /// calls pass on the new thread.
    ///
    /// # FFI note
    /// This escape hatch is designed for the Rust API. It is **not** exposed
    /// through the C FFI. C callers who need cross-thread transfer must create
    /// a new engine on the target thread. (Exposing `move_to_current_thread`
    /// through C would require the caller to reason about `Rc<Value>` lifetimes,
    /// which is not practical from C.)
    ///
    /// # Common violations (do NOT do these)
    /// - Storing a `ValueRef` (which wraps `Rc<Value>`) in a `Vec` on thread A,
    ///   then moving the engine to thread B while thread A still holds the Vec.
    /// - Passing a fact query result (`&Fact` borrowing engine internals) to a
    ///   closure that outlives the transfer.
    /// - Calling `get_fact()` before the transfer and using the returned reference
    ///   after the engine has moved — even if the reference is "just reading."
    pub unsafe fn move_to_current_thread(&mut self) {
        self.creator_thread_id = std::thread::current().id();
        self.clear_last_error();
    }
}

// A minimal Send wrapper is still needed to satisfy Rust's type system for
// the cross-thread move itself. This is intentionally kept minimal — all
// safety-critical state updates happen inside `move_to_current_thread()`.
struct SendEngine(Engine);
unsafe impl Send for SendEngine {}

// Typical usage with an async runtime:
//   let mut send_engine = SendEngine(engine);
//   let result = tokio::task::spawn_blocking(move || {
//       unsafe { send_engine.0.move_to_current_thread(); }
//       send_engine.0.run(RunLimit::Unlimited)
//   }).await??;
//
// Prefer "one engine per thread" — treat cross-thread transfer as a niche
// escape hatch, not a primary usage pattern.
```

**Rationale for !Send (vs. the Arc alternative):**

The v3 review identified that `Rc<Value>` makes `Engine` `!Send`. Two options were considered:

- **Option A:** Switch to `Arc<Value>` — makes `Engine` `Send`, but adds atomic overhead on every refcount bump in hot paths (token propagation, join evaluation).
- **Option B (chosen):** Keep `Rc<Value>`, make `Engine` `!Send + !Sync` by construction.

Option B is preferred because:

1. **Performance:** `Rc` is measurably cheaper than `Arc` for refcount-heavy workloads (no atomic operations).
2. **Correctness by construction:** The compiler prevents accidental cross-thread use. With `Arc + Send`, the compiler would *allow* concurrent access from multiple threads if someone wraps the engine in `Arc<Mutex<Engine>>` — which is technically safe but operationally confusing (mutex contention, thread-local error confusion).
3. **Escape hatch exists:** The `unsafe Send` wrapper pattern is standard Rust and lets advanced users opt in explicitly.

### 2.2 Synchronous Execution Model

Individual engine instances are purely synchronous. This simplifies the internal implementation and matches CLIPS' execution model. Users who need async integration should wrap engine instances appropriately:

```rust
// Sync usage (native)
engine.run(RunLimit::Unlimited)?;

// Async usage: requires explicit unsafe thread transfer
struct SendEngine(Engine);
unsafe impl Send for SendEngine {}

let mut send_engine = SendEngine(engine);
let result = tokio::task::spawn_blocking(move || {
    // Safety: engine is not accessed on the original thread after this point,
    // and no Rc<Value> references have escaped.
    unsafe { send_engine.0.move_to_current_thread(); }
    send_engine.0.run(RunLimit::Unlimited)
}).await??;
```

This design avoids infecting the core engine with async complexity while remaining fully compatible with async runtimes.

### 2.3 Configurable Strictness

Engine behavior is configurable at initialization time via two primary modes:

| Mode | Behavior |
|------|----------|
| **Classic** | Matches CLIPS behavior: warnings for minor issues, continues execution where possible. Maximizes backward compatibility. |
| **Strict** | Fails fast on ambiguous or questionable constructs. Better for new development and catching errors early. |

```rust
let config = EngineConfig::new()
    .error_mode(ErrorMode::Classic)  // or ErrorMode::Strict
    .string_encoding(StringEncoding::Utf8);

let engine = Engine::new(config)?;
```

**Critical clarification — unsupported constructs always fail compilation:**

In **both** modes, rules containing unsupported pattern constructs (e.g., triple-nested negation, `exists(not ...)`, nested `forall`) fail compilation. The rule is **never** silently dropped or compiled in a degraded/inert form. This is an explicit design choice to avoid "why didn't my rule fire?" mysteries.

The difference between modes is **severity and scope**, not whether errors are surfaced:

| Situation | Classic Mode | Strict Mode |
|-----------|-------------|-------------|
| Unsupported pattern construct | Compilation **fails**, diagnostic at `Warning` level | Compilation **fails**, diagnostic at `Error` level |
| Ambiguous variable scope | Compilation succeeds with warning | Compilation **fails** |
| Deprecated CLIPS syntax | Compilation succeeds with warning | Compilation **fails** |
| Type mismatch in constraint | Runtime warning, best-effort | Compilation **fails** |

The `Warning` vs `Error` severity distinction in classic mode exists for downstream tooling (IDEs, linters) that may want to differentiate "hard unsupported" from "soft ambiguous." In both cases, unsupported constructs produce a `CompileError` and the rule is not added to the network.

### 2.4 Explicit Text Encoding

Ferric supports multiple text encoding modes, configured at engine initialization:

| Mode | Symbols | Strings | Use Case |
|------|---------|---------|----------|
| `Ascii` | ASCII only | ASCII only | Maximum CLIPS compatibility |
| `Utf8` | UTF-8 | UTF-8 | Full internationalization |
| `AsciiSymbolsUtf8Strings` | ASCII only | UTF-8 | Compromise: identifiers remain ASCII, text data is modern |

**Encoding Invariants:**

The engine configuration governs all symbol and string creation. To prevent illegal states:

1. **Centralized Construction:** `Symbol` and `FerricString` cannot be created directly outside the runtime crate. All creation goes through `Engine::intern_symbol()` and `Engine::create_string()`, which enforce the configured encoding mode.

2. **Mode Enforcement:** In `AsciiSymbolsUtf8Strings` mode, attempting to intern a non-ASCII symbol returns `Err(EncodingError::NonAsciiSymbol)`.

3. **Equality and Ordering Semantics:** Within a single engine, symbols and strings are always comparable. Semantics are defined precisely per encoding mode (see Section 2.4.1). Cross-engine comparison is explicitly unsupported (symbols from different engines are not comparable).

```rust
impl Engine {
    /// Intern a symbol, enforcing the engine's encoding mode.
    /// Returns Err if the input violates encoding constraints.
    pub fn intern_symbol(&mut self, s: &str) -> Result<Symbol, EncodingError> {
        match self.config.string_encoding {
            StringEncoding::Ascii | StringEncoding::AsciiSymbolsUtf8Strings => {
                if !s.is_ascii() {
                    return Err(EncodingError::NonAsciiSymbol(s.to_string()));
                }
                Ok(Symbol(self.symbol_table.intern_ascii(s.as_bytes())))
            }
            StringEncoding::Utf8 => {
                Ok(Symbol(self.symbol_table.intern_utf8(s)))
            }
        }
    }

    /// Create a string value, enforcing the engine's encoding mode.
    pub fn create_string(&self, s: &str) -> Result<FerricString, EncodingError> {
        match self.config.string_encoding {
            StringEncoding::Ascii => {
                if !s.is_ascii() {
                    return Err(EncodingError::NonAsciiString(s.to_string()));
                }
                Ok(FerricString::Ascii(s.as_bytes().into()))
            }
            StringEncoding::Utf8 | StringEncoding::AsciiSymbolsUtf8Strings => {
                Ok(FerricString::Utf8(s.into()))
            }
        }
    }
}
```

#### 2.4.1 String and Symbol Comparison Semantics

To avoid ambiguity and cross-platform behavioral differences, Ferric defines comparison semantics precisely:

**Equality:** Exact byte equality. Two strings or symbols are equal if and only if their underlying byte sequences are identical. No Unicode normalization (NFC, NFD, NFKC, NFKD) is performed. This matches Rust's built-in `str` equality and avoids hidden costs. Users who need normalization-aware equality should normalize their inputs before asserting facts.

**Ordering:** Lexicographic comparison by UTF-8 byte values (equivalently, by Unicode scalar values, since UTF-8 preserves scalar value ordering). This is identical to Rust's `Ord` implementation for `str` and `[u8]`.

| Encoding Mode | Equality | Ordering | Notes |
|---------------|----------|----------|-------|
| `Ascii` | Byte-identical | Lexicographic by byte value | ASCII bytes only; identical to codepoint ordering |
| `Utf8` | Byte-identical | Lexicographic by UTF-8 byte value | No locale collation; same as Rust `str::cmp` |
| `AsciiSymbolsUtf8Strings` | Byte-identical (per type) | Lexicographic by byte value (per type) | Symbols are ASCII-only; strings are UTF-8 |

**What is explicitly NOT supported:**

- Unicode normalization (NFC/NFD)
- Locale-sensitive collation (e.g., `ä` sorting near `a` in German)
- Case-insensitive comparison (users should normalize case before interning)

**Compatibility note:** CLIPS uses byte comparison for all strings and symbols. Ferric's behavior is identical for ASCII content. For UTF-8 content, the behavior is the natural extension (byte-order comparison) and matches what most users expect. The compatibility documentation will include examples demonstrating these semantics.

### 2.5 Retraction-First Design

A key architectural principle: **design for retraction from day one**. Many Rete implementations optimize for assertion and treat retraction as an afterthought, leading to O(n) scans and painful rewrites later.

Ferric's data structures are designed with efficient retraction as a primary constraint:

- All tokens have stable identities (`TokenId`)
- Reverse indices map `FactId → TokenId set` for O(1) lookup of affected tokens
- Parent→children indices enable O(subtree-size) cascading deletes (see Section 5.5.1)
- Negative nodes maintain explicit blocker relationships
- Alpha memories support indexed lookup, not just membership testing

This principle influences nearly every data structure decision in sections 5-7.

---

## 3. Architecture

### 3.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         User Application                         │
└─────────────────────────────────────────────────────────────────┘
                                  │
                    ┌─────────────┴─────────────┐
                    ▼                           ▼
         ┌──────────────────┐        ┌──────────────────┐
         │   Rust API       │        │    C FFI API     │
         │  (ferric crate)  │        │ (ferric-ffi)     │
         └────────┬─────────┘        └────────┬─────────┘
                  │                           │
                  └─────────────┬─────────────┘
                                ▼
         ┌──────────────────────────────────────────────┐
         │              Engine Instance                  │
         │  ┌─────────────────────────────────────────┐ │
         │  │           Rete Network                  │ │
         │  │  ┌─────────────┐  ┌─────────────────┐   │ │
         │  │  │ Alpha Net   │  │   Beta Network  │   │ │
         │  │  │ (patterns)  │  │   (joins)       │   │ │
         │  │  └─────────────┘  └─────────────────┘   │ │
         │  └─────────────────────────────────────────┘ │
         │  ┌──────────────┐  ┌────────────────────┐   │
         │  │  Fact Base   │  │      Agenda        │   │
         │  │  (working    │  │  (activated rules) │   │
         │  │   memory)    │  │                    │   │
         │  └──────────────┘  └────────────────────┘   │
         │  ┌──────────────────────────────────────┐   │
         │  │         Module System                │   │
         │  └──────────────────────────────────────┘   │
         └──────────────────────────────────────────────┘
                                │
                                ▼
         ┌──────────────────────────────────────────────┐
         │              Standard Library                 │
         │  (math, string, multifield, I/O, etc.)       │
         └──────────────────────────────────────────────┘
```

### 3.2 Data Flow

1. **Input:** Rules and facts enter via parser (`.clp` files) or programmatic API
2. **Compilation:** Rules are compiled into Rete network nodes
3. **Assertion:** Facts are asserted into working memory, propagating through alpha network
4. **Matching:** Beta network performs joins, creating partial matches
5. **Activation:** Complete matches create activations on the agenda
6. **Execution:** Conflict resolution selects next rule; RHS actions execute
7. **Iteration:** Actions may assert/retract facts, triggering further matching

### 3.3 Immutability Boundaries

To keep retraction logic clean and avoid "who mutates what" complexity:

**Immutable after construction:**
- Compiled Rete network topology (nodes, edges, test definitions)
- Rule definitions
- Template definitions

**Mutable during execution:**
- Alpha memories (fact sets)
- Beta memories (token sets)
- Negative node blocker maps
- Agenda
- Fact base
- Global variables

This separation means retraction never needs to modify the network structure—only the memories attached to nodes.

---

## 4. Crate Structure

### 4.1 Workspace Layout

The layout below reflects the **current baseline after Phase 1**. The earlier
nested `ferric-core/src/rete/*` sketch has been flattened into top-level core
modules to match implementation reality.

```
ferric/
├── Cargo.toml                 # Workspace definition
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md
│
├── crates/
│   ├── ferric/                # Main public API (re-exports from other crates)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   ├── ferric-core/           # Rete network, pattern matching, agenda
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── agenda.rs          # Conflict resolution, activation management
│   │       ├── alpha.rs           # Alpha network implementation
│   │       ├── beta.rs            # Beta network and join nodes
│   │       ├── token.rs           # Token representation and storage
│   │       ├── rete.rs            # Rete integration (alpha+beta+agenda+tokens)
│   │       ├── fact.rs            # Fact representation
│   │       ├── binding.rs         # Variable bindings with VarId
│   │       ├── value.rs           # Value enum and operations
│   │       ├── symbol.rs          # Symbol interning (encoding-aware)
│   │       ├── string.rs          # FerricString (encoding-aware)
│   │       └── encoding.rs        # StringEncoding and EncodingError
│   │
│   ├── ferric-parser/         # Lexer, parser, AST
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── lexer.rs           # Tokenization
│   │       ├── sexpr.rs           # Stage 1: S-expression parsing
│   │       ├── span.rs            # Source location tracking
│   │       └── error.rs           # Parse errors with spans
│   │
│   ├── ferric-runtime/        # Engine, execution, modules
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── engine.rs          # Main Engine type
│   │       ├── config.rs          # EngineConfig (Phase 1 subset)
│   │       └── loader.rs          # Minimal source loader
│   │
│   ├── ferric-stdlib/         # Built-in functions (Phase 2+)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── math.rs
│   │       ├── string.rs
│   │       ├── multifield.rs
│   │       ├── predicate.rs
│   │       ├── io.rs
│   │       ├── fact_ops.rs
│   │       └── agenda_ops.rs
│   │
│   ├── ferric-ffi/            # C API (Phase 5+)
│   │   ├── Cargo.toml
│   │   ├── build.rs               # cbindgen header generation
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── engine.rs          # Engine lifecycle
│   │   │   ├── error.rs           # Error handling (TLS + per-engine)
│   │   │   ├── value.rs           # Value conversion
│   │   │   └── ownership.rs       # Ownership documentation
│   │   └── include/
│   │       └── ferric.h           # Generated C header
│   │
│   └── ferric-cli/            # Command-line interface and REPL (Phase 5+)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── repl.rs
│           └── batch.rs
│
├── tests/                     # Integration tests
│   ├── clips_compat/          # CLIPS compatibility test suite
│   │   ├── basic_rules.clp
│   │   ├── templates.clp
│   │   └── ...
│   └── regression/
│
├── benches/                   # Benchmarks
│   ├── rete_bench.rs
│   ├── waltz.rs
│   └── manners.rs
│
└── docs/
    ├── compatibility.md       # What is/isn't supported from CLIPS
    ├── migration.md           # Guide for CLIPS users
    └── architecture.md        # Internal documentation
```

Phase 2+ expected additions to this layout include parser Stage-2 files
(`ast.rs`, `construct.rs`) and runtime module/eval surfaces.

### 4.2 Crate Dependencies

The graph below reflects the **current implemented dependency baseline**.
Additional edges involving `ferric-stdlib`, `ferric-ffi`, and `ferric-cli`
are phased in later.

```
ferric (public facade)
├── ferric-core
├── ferric-parser
├── ferric-runtime
│   ├── ferric-core
│   └── ferric-parser

ferric-ffi
└── ferric

ferric-cli
├── ferric                  # planned Phase 5+
└── rustyline (for REPL)    # planned Phase 5+
```

### 4.3 External Dependencies (Preliminary)

| Crate | Purpose | License |
|-------|---------|---------|
| `thiserror` | Error type derivation | MIT/Apache-2.0 |
| `slotmap` | Stable-identity storage for tokens/facts | Zlib |
| `rustc-hash` | Fast hashing (FxHash) | MIT/Apache-2.0 |
| `bumpalo` | Arena allocation | MIT/Apache-2.0 |
| `smallvec` | Small vector optimization | MIT/Apache-2.0 |
| `nom` | S-expression parsing | MIT |
| `rustyline` | REPL line editing | MIT |
| `tracing` | Observability | MIT |
| `cbindgen` | C header generation (build) | MPL-2.0 |

All dependencies are compatible with MIT/Apache-2.0 dual licensing.

---

## 5. Core Data Structures

### 5.1 Values

`Value`, `FerricString`, `SymbolTable`, and encoding primitives are currently
implemented in `ferric-core` (not `ferric-runtime`) to avoid crate cycles
between facts/rete internals and runtime APIs.

The `Value` type represents all runtime values in Ferric:

```rust
/// Runtime value in Ferric.
/// 
/// Value intentionally does NOT implement Eq or Hash because it contains
/// Float (IEEE 754 equality is not reflexive) and Multifield (deep hashing
/// would be expensive and leak into hot paths). For contexts that need
/// hashing/equality — alpha-memory indexing, constant tests — use AtomKey
/// (see Section 5.1.1).
#[derive(Clone, Debug)]
pub enum Value {
    /// A symbolic atom (interned, always Copy)
    Symbol(Symbol),
    
    /// A string value
    String(FerricString),
    
    /// A 64-bit signed integer
    Integer(i64),
    
    /// A 64-bit floating point number
    Float(f64),
    
    /// An ordered collection of values.
    /// Boxed to avoid recursive-size issues (`Value` -> `Multifield` -> `Value`).
    Multifield(Box<Multifield>),
    
    /// An opaque pointer for embedding (with type tag)
    ExternalAddress(ExternalAddress),
    
    /// The void/nil value
    Void,
}

/// Interned symbol - always cheap to copy and compare.
/// The SymbolId is only valid within the engine that created it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Symbol(pub(crate) SymbolId);

/// Internal symbol identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SymbolId {
    Ascii(u32),
    Utf8(u32),
}

/// String with encoding awareness.
/// Not interned; comparison is by value.
/// Equality is exact byte equality (see Section 2.4.1).
/// Ordering is lexicographic by byte value (see Section 2.4.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FerricString {
    Ascii(Box<[u8]>),
    Utf8(Box<str>),
}

/// Multifield (ordered collection).
/// Uses SmallVec for common small cases.
#[derive(Clone, Debug, PartialEq)]
pub struct Multifield {
    values: SmallVec<[Value; 8]>,
}

/// External address for embedding.
/// The type_id allows the embedding application to safely downcast.
#[derive(Clone, Debug)]
pub struct ExternalAddress {
    pub type_id: ExternalTypeId,
    pub pointer: *mut c_void,
}

// ExternalAddress is Send+Sync if the embedding code ensures thread safety
unsafe impl Send for ExternalAddress {}
unsafe impl Sync for ExternalAddress {}
```

#### 5.1.1 AtomKey — Hashable Value Subset for Indexing

Several Rete data structures need values as hash keys (alpha-memory slot indices, constant test identity). Full `Value` is unsuitable because `f64` lacks `Eq`/`Hash` and deep-hashing `Multifield` is expensive.

`AtomKey` captures the subset of values that can appear as constant-test operands or index keys. It is `Eq + Hash + Clone` and covers the types that CLIPS allows in pattern constants.

```rust
/// A value that can be used as a hash key in alpha-memory indices
/// and constant-test definitions. Covers the "atomic" value types.
///
/// This type exists because full `Value` intentionally does not implement
/// Eq or Hash (due to Float semantics and Multifield cost).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AtomKey {
    Symbol(Symbol),
    String(FerricString),
    Integer(i64),
    /// Float stored as raw bits via f64::to_bits().
    /// This means -0.0 != +0.0 and distinct NaN bit patterns are distinct keys.
    /// This matches CLIPS behavior (bitwise comparison) and avoids IEEE 754 edge cases.
    FloatBits(u64),
    /// External address keyed by (type_id, pointer).
    ExternalAddress { type_id: ExternalTypeId, pointer: usize },
}

impl AtomKey {
    /// Convert from a Value, if the value is an atom (not Multifield or Void).
    pub fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Symbol(s) => Some(AtomKey::Symbol(*s)),
            Value::String(s) => Some(AtomKey::String(s.clone())),
            Value::Integer(i) => Some(AtomKey::Integer(*i)),
            Value::Float(f) => Some(AtomKey::FloatBits(f.to_bits())),
            Value::ExternalAddress(ea) => Some(AtomKey::ExternalAddress {
                type_id: ea.type_id,
                pointer: ea.pointer as usize,
            }),
            Value::Multifield(_) | Value::Void => None,
        }
    }

    /// Convert back to a Value for runtime use.
    pub fn to_value(&self) -> Value {
        match self {
            AtomKey::Symbol(s) => Value::Symbol(*s),
            AtomKey::String(s) => Value::String(s.clone()),
            AtomKey::Integer(i) => Value::Integer(*i),
            AtomKey::FloatBits(bits) => Value::Float(f64::from_bits(*bits)),
            AtomKey::ExternalAddress { type_id, pointer } => {
                Value::ExternalAddress(ExternalAddress {
                    type_id: *type_id,
                    pointer: *pointer as *mut c_void,
                })
            }
        }
    }
}
```

**Design rationale:**

- `FloatBits(u64)` stores the raw IEEE 754 bit pattern via `f64::to_bits()`. This means `-0.0 ≠ +0.0` and each NaN bit pattern is a distinct key. This is intentional: it matches CLIPS' bitwise float comparison, avoids the `Eq` reflexivity problem, and provides consistent `Hash` behavior.
- `Multifield` and `Void` are excluded: they don't appear as constant-test operands in CLIPS and would add cost to hashing.
- `AtomKey` is used in `ConstantTestType` (§6.3), alpha-memory slot indices (§6.4), and node-sharing keys (§6.2). The API boundary between `Value` (general runtime) and `AtomKey` (indexing/tests) is narrow and explicit.

**Value Cloning Policy:**

To minimize allocation overhead during pattern matching:

1. `Symbol` is `Copy` (just a u32 index)
2. `Integer`, `Float`, `Void` are `Copy`
3. `FerricString` and `Multifield` are cloned only when escaping from token context to user code
4. Within the Rete network, values are reference-counted or arena-backed (see Token section)

### 5.2 Facts

```rust
/// A fact identifier - stable handle into fact storage.
/// Uses slotmap for O(1) access and stable identity across insertions/deletions.
///
/// Defined via slotmap::new_key_type! to ensure it implements the slotmap::Key
/// trait, which is required for use as a SlotMap key. This also provides
/// a distinct type (not just DefaultKey) for type safety.
slotmap::new_key_type! {
    pub struct FactId;
}

/// An ordered fact (simple list of values)
#[derive(Clone, Debug)]
pub struct OrderedFact {
    pub relation: Symbol,
    pub fields: SmallVec<[Value; 8]>,
}

/// A template-based fact (named slots)
#[derive(Clone, Debug)]
pub struct TemplateFact {
    pub template_id: TemplateId,
    /// Slots indexed by position (order matches Template.slots)
    pub slots: Box<[Value]>,
}

/// Either kind of fact
#[derive(Clone, Debug)]
pub enum Fact {
    Ordered(OrderedFact),
    Template(TemplateFact),
}

/// Metadata stored alongside facts in the fact base
pub struct FactEntry {
    pub fact: Fact,
    pub id: FactId,
    /// Monotonic timestamp for recency-based conflict resolution
    pub timestamp: u64,
}

/// The fact base with indexing support
pub struct FactBase {
    /// Primary storage: FactId → FactEntry
    facts: SlotMap<FactId, FactEntry>,
    
    /// Index by template/relation for fast pattern entry
    by_template: HashMap<TemplateId, HashSet<FactId>>,
    by_relation: HashMap<Symbol, HashSet<FactId>>,
    
    /// Monotonic timestamp counter
    next_timestamp: u64,
}
```

### 5.3 Templates

```rust
/// A template definition (immutable after registration)
#[derive(Clone, Debug)]
pub struct Template {
    pub id: TemplateId,
    pub name: Symbol,
    pub module: ModuleId,
    pub slots: Vec<SlotDefinition>,
    /// Precomputed: slot name → index for fast lookup
    slot_index: HashMap<Symbol, usize>,
}

/// A slot within a template
#[derive(Clone, Debug)]
pub struct SlotDefinition {
    pub name: Symbol,
    pub slot_type: SlotType,
    pub default: Option<Value>,
    pub constraints: Vec<SlotConstraint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotType {
    Single,
    Multi,
}

#[derive(Clone, Debug)]
pub enum SlotConstraint {
    Type(ValueType),
    Range { min: Option<f64>, max: Option<f64> },
    AllowedValues(Vec<Value>),
}
```

### 5.4 Variable Bindings

**Design Decision:** Bindings use dense `VarId` indexing rather than symbol lookup.

During rule compilation, each variable is assigned a unique `VarId` (small integer). At runtime, bindings are a dense `Vec<Option<Value>>` indexed by `VarId`. This provides O(1) lookup during joins.

```rust
/// Compiler-assigned variable identifier (dense, 0-based)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VarId(pub u16);

/// Mapping from source-level variable names to VarIds.
/// Stored per-rule, used during compilation and RHS evaluation.
#[derive(Clone, Debug)]
pub struct VarMap {
    /// Symbol → VarId
    by_name: HashMap<Symbol, VarId>,
    /// VarId → Symbol (for error messages and debugging)
    by_id: Vec<Symbol>,
}

impl VarMap {
    pub fn get_or_create(&mut self, name: Symbol) -> Result<VarId, VarMapError> {
        if let Some(&id) = self.by_name.get(&name) {
            return Ok(id);
        }

        let attempted = self.by_id.len() + 1;
        if attempted > u16::MAX as usize {
            return Err(VarMapError::TooManyVariables {
                attempted,
                max: u16::MAX as usize,
            });
        }

        let id = VarId(self.by_id.len() as u16);
        self.by_name.insert(name, id);
        self.by_id.push(name);
        Ok(id)
    }

    pub fn lookup(&self, name: Symbol) -> Option<VarId> {
        self.by_name.get(&name).copied()
    }

    pub fn name(&self, id: VarId) -> Symbol {
        self.by_id[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VarMapError {
    #[error("rule declares too many variables: attempted {attempted}, max {max}")]
    TooManyVariables { attempted: usize, max: usize },
}

/// Runtime binding set: VarId → Value
/// 
/// Uses a dense vector. Unbound variables are None.
/// Values are reference-counted to avoid cloning during join propagation.
#[derive(Clone, Debug)]
pub struct BindingSet {
    /// Indexed by VarId. Length equals rule's variable count.
    bindings: SmallVec<[Option<ValueRef>; 16]>,
}

/// Reference-counted value for efficient sharing in tokens.
/// 
/// Uses Rc (not Arc) because Engine is !Send+!Sync by design (see §2.1).
/// Rc avoids atomic overhead on every refcount bump, which matters in
/// hot paths (token propagation, join evaluation).
pub type ValueRef = std::rc::Rc<Value>;

impl BindingSet {
    pub fn new(var_count: usize) -> Self {
        Self {
            bindings: smallvec![None; var_count],
        }
    }

    #[inline]
    pub fn get(&self, var: VarId) -> Option<&Value> {
        self.bindings.get(var.0 as usize)?.as_ref().map(|rc| rc.as_ref())
    }

    #[inline]
    pub fn set(&mut self, var: VarId, value: ValueRef) {
        self.bindings[var.0 as usize] = Some(value);
    }

    /// Extend bindings from another set (for join operations)
    pub fn extend_from(&mut self, other: &BindingSet) {
        for (i, binding) in other.bindings.iter().enumerate() {
            if let Some(value) = binding {
                if self.bindings[i].is_none() {
                    self.bindings[i] = Some(value.clone());
                }
            }
        }
    }
}
```

### 5.5 Tokens

Tokens represent partial matches through the beta network. They have stable identities to support efficient retraction.

```rust
/// Stable token identifier.
/// Defined via slotmap::new_key_type! for type safety and Key trait compliance.
slotmap::new_key_type! {
    pub struct TokenId;
}

/// A partial match through the beta network.
/// Tokens are stored in a SlotMap for stable identity.
#[derive(Clone, Debug)]
pub struct Token {
    /// Facts matched so far (in pattern order)
    pub facts: SmallVec<[FactId; 4]>,
    
    /// Variable bindings accumulated so far
    pub bindings: BindingSet,
    
    /// Parent token (for tree-structured token memory)
    pub parent: Option<TokenId>,
    
    /// The beta memory (or negative/NCC/exists memory) that owns this token.
    /// Stored here so retraction can remove the token from its owning memory
    /// in O(1) without scanning all memories.
    pub owner_node: NodeId,
}
```

#### 5.5.1 Token Storage and Indexing

Token storage provides both the `FactId → TokenId` reverse index (for retraction entry point) and a `parent → children` index (for efficient cascading deletes down the token tree).

```rust
/// Token storage with reverse indexing for retraction
pub struct TokenStore {
    /// Primary storage
    tokens: SlotMap<TokenId, Token>,
    
    /// Reverse index: FactId → set of TokenIds containing that fact.
    /// Critical for efficient retraction.
    /// 
    /// Uses SmallVec because most facts participate in few tokens at any time.
    /// For the rare high-fanout case, SmallVec spills to heap transparently.
    fact_to_tokens: HashMap<FactId, SmallVec<[TokenId; 4]>>,
    
    /// Parent → children index for efficient subtree deletion.
    /// Maintained on every insert/remove.
    /// 
    /// Without this index, cascading deletes require either an expensive
    /// full scan or fragile mark-and-sweep logic.
    parent_to_children: HashMap<TokenId, SmallVec<[TokenId; 4]>>,
}

impl TokenStore {
    /// Insert a token, consuming it by value (no unnecessary clone).
    /// Returns the assigned TokenId.
    pub fn insert(&mut self, token: Token) -> TokenId {
        // Capture facts, parent, and owner before moving the token into storage.
        let facts: SmallVec<[FactId; 4]> = token.facts.clone();
        let parent = token.parent;
        let _owner_node = token.owner_node; // stored in token for O(1) cleanup
        
        // Insert by value — no clone of the full token.
        let id = self.tokens.insert(token);
        
        // Update fact → token reverse index.
        // De-dup fact_ids for index maintenance: a token's fact list may contain
        // the same FactId more than once if the same fact satisfies multiple patterns
        // in the rule. Without dedup, we'd insert duplicate entries in fact_to_tokens,
        // causing extra work on removal and potentially confusing downstream code
        // that assumes one index entry per (fact, token) pair.
        //
        // We use a local SmallVec + contains() for the dedup because:
        //   - token.facts is typically ≤4 elements (SmallVec<[FactId; 4]>)
        //   - O(n²) contains-check is cheaper than sorting for n ≤ ~8
        //   - the token.facts field itself is kept as-is (preserving pattern order)
        let mut seen_facts: SmallVec<[FactId; 4]> = SmallVec::new();
        for &fact_id in &facts {
            if !seen_facts.contains(&fact_id) {
                seen_facts.push(fact_id);
                self.fact_to_tokens
                    .entry(fact_id)
                    .or_default()
                    .push(id);
            }
        }
        
        // Update parent → children index
        if let Some(parent_id) = parent {
            self.parent_to_children
                .entry(parent_id)
                .or_default()
                .push(id);
        }
        
        id
    }

    /// Remove a single token (does NOT cascade to children).
    /// Use `remove_cascade` for subtree deletion.
    pub fn remove(&mut self, id: TokenId) -> Option<Token> {
        let token = self.tokens.remove(id)?;
        
        // Update fact → token reverse index (dedup: same fact may appear multiple times)
        let mut seen_facts: SmallVec<[FactId; 4]> = SmallVec::new();
        for &fact_id in &token.facts {
            if !seen_facts.contains(&fact_id) {
                seen_facts.push(fact_id);
                if let Some(set) = self.fact_to_tokens.get_mut(&fact_id) {
                    set.retain(|&tid| tid != id);
                    if set.is_empty() {
                        self.fact_to_tokens.remove(&fact_id);
                    }
                }
            }
        }
        
        // Update parent → children index
        if let Some(parent_id) = token.parent {
            if let Some(children) = self.parent_to_children.get_mut(&parent_id) {
                children.retain(|&cid| cid != id);
                if children.is_empty() {
                    self.parent_to_children.remove(&parent_id);
                }
            }
        }
        
        // Clean up our own children entry (children are now orphaned if not also removed)
        self.parent_to_children.remove(&id);
        
        Some(token)
    }

    /// Remove a token and all its descendants in one pass.
    /// Returns all removed (TokenId, Token) pairs for downstream cleanup
    /// (callers need Token.owner_node for O(1) beta-memory cleanup).
    /// Complexity: O(size of subtree).
    pub fn remove_cascade(&mut self, root_id: TokenId) -> Vec<(TokenId, Token)> {
        // Precondition: root_id should exist in the TokenStore. Calling on an
        // already-removed or nonexistent token is a logic bug. We debug_assert
        // to catch this in development, and defensively return empty in release
        // to avoid compounding a single bug into corrupted indices.
        debug_assert!(
            self.tokens.contains_key(root_id),
            "remove_cascade called on nonexistent root {:?}",
            root_id
        );
        if !self.tokens.contains_key(root_id) {
            return Vec::new();
        }

        let mut removed = Vec::new();
        let mut stack = vec![root_id];
        
        while let Some(id) = stack.pop() {
            // Push children onto the stack before removing
            if let Some(children) = self.parent_to_children.remove(&id) {
                stack.extend(children.iter().copied());
            }
            
            if let Some(token) = self.tokens.remove(id) {
                // Update fact → token reverse index (dedup for same reason as insert)
                let mut seen_facts: SmallVec<[FactId; 4]> = SmallVec::new();
                for &fact_id in &token.facts {
                    if !seen_facts.contains(&fact_id) {
                        seen_facts.push(fact_id);
                        if let Some(set) = self.fact_to_tokens.get_mut(&fact_id) {
                            set.retain(|&tid| tid != id);
                            if set.is_empty() {
                                self.fact_to_tokens.remove(&fact_id);
                            }
                        }
                    }
                }
                
                // Update parent → children (for root only; children are removed in-order)
                if let Some(parent_id) = token.parent {
                    if let Some(siblings) = self.parent_to_children.get_mut(&parent_id) {
                        siblings.retain(|&cid| cid != id);
                        if siblings.is_empty() {
                            self.parent_to_children.remove(&parent_id);
                        }
                    }
                }
                
                removed.push((id, token));
            }
        }
        
        removed
    }

    /// Find all tokens containing a given fact (for retraction)
    pub fn tokens_containing(&self, fact_id: FactId) -> impl Iterator<Item = TokenId> + '_ {
        self.fact_to_tokens
            .get(&fact_id)
            .into_iter()
            .flat_map(|set| set.iter().copied())
    }

    /// Get direct children of a token
    pub fn children(&self, id: TokenId) -> impl Iterator<Item = TokenId> + '_ {
        self.parent_to_children
            .get(&id)
            .into_iter()
            .flat_map(|children| children.iter().copied())
    }

    /// Given a set of affected tokens, return only roots whose ancestors are not
    /// in the same set. This prevents double-cascade during fact retraction.
    pub fn retraction_roots(&self, affected: &HashSet<TokenId>) -> Vec<TokenId> {
        let mut roots = Vec::new();
        for &token_id in affected {
            let mut has_affected_ancestor = false;
            let mut current = self.get(token_id).and_then(|t| t.parent);
            while let Some(parent_id) = current {
                if affected.contains(&parent_id) {
                    has_affected_ancestor = true;
                    break;
                }
                current = self.get(parent_id).and_then(|t| t.parent);
            }
            if !has_affected_ancestor {
                roots.push(token_id);
            }
        }
        roots
    }

    pub fn get(&self, id: TokenId) -> Option<&Token> {
        self.tokens.get(id)
    }
}
```

**Future optimization note (scratch buffer reuse):** The current `remove_cascade` allocates a fresh `Vec` and internal `stack` on every call. For workloads with heavy churn (many cascades per cycle), a `remove_cascade_into(root, removed_out: &mut Vec<_>, stack: &mut Vec<_>)` variant — or scratch buffers stored on the `ReteNetwork` — would allow capacity reuse across cascades. This is an allowed future optimization behind the existing API; no design change is needed to support it.

**Design notes on reverse index representation:**

The `fact_to_tokens` and `parent_to_children` maps use `SmallVec<[TokenId; 4]>` rather than `HashSet<TokenId>`. In typical rule sets, most facts participate in a small number of tokens and most tokens have few children. `SmallVec<[_; 4]>` stores up to 4 elements inline (no heap allocation) and gracefully spills to heap for rare high-fanout cases. This avoids the per-entry overhead of `HashSet` (hashing, load factor, pointer chasing) for the common case.

**Deletion method for SmallVec-backed indices:**

All reverse-index removals use `retain(|&x| x != target)` (linear scan + compaction). This is O(k) where k is the SmallVec length, which is typically ≤4. This is intentionally chosen over `swap_remove` because:

1. `retain` never leaves duplicates or gaps — it is correct by construction.
2. For k ≤ 4, the constant factor is negligible (no branching advantage from `swap_remove`).
3. Order preservation (while not required) aids debugging.

**No-duplicates invariant:** The insert paths (`push`) are only called for freshly-created TokenIds (from `SlotMap::insert`), which are guaranteed unique. Therefore duplicates cannot occur and `retain` will remove exactly zero or one element. This invariant is asserted in debug builds:

```rust
// Debug-mode invariant check (in insert paths):
debug_assert!(
    !self.fact_to_tokens.get(&fact_id).map_or(false, |v| v.contains(&id)),
    "duplicate token in fact_to_tokens reverse index"
);
```

The API surface does not expose the backing collection type, so this representation can be swapped later (e.g., to a hybrid `SmallVec` + `HashSet` above a threshold) without changing callers.

### 5.6 Rules

```rust
/// A compiled rule (immutable after compilation)
pub struct Rule {
    pub id: RuleId,
    pub name: Symbol,
    pub module: ModuleId,
    pub salience: i32,
    pub patterns: Vec<SpannedPattern>,
    pub actions: Vec<Action>,
    pub declaration: RuleDeclaration,
    
    /// Variable mapping for this rule
    pub var_map: VarMap,
    
    /// Compiled Rete entry point (beta node)
    pub(crate) rete_entry: NodeId,
}

/// Rule declaration properties
#[derive(Clone, Debug, Default)]
pub struct RuleDeclaration {
    pub salience: i32,
    pub auto_focus: bool,
}

/// A pattern in the LHS of a rule (bare structure, no source location).
/// Always wrapped in `SpannedPattern` when produced by the parser (see below).
#[derive(Clone, Debug)]
pub enum Pattern {
    /// Match a fact
    Fact(FactPattern),
    /// Test a condition without matching a fact
    Test(Expression),
    /// Negation (see Section 7 for semantics)
    Not(Box<SpannedPattern>),
    /// Conjunction
    And(Vec<SpannedPattern>),
    /// Disjunction
    Or(Vec<SpannedPattern>),
    /// Existential (see Section 7 for semantics)
    Exists(Box<SpannedPattern>),
    /// Universal (see Section 7 for supported subset)
    Forall { condition: Box<SpannedPattern>, then: Box<SpannedPattern> },
}

/// A pattern paired with its source location.
///
/// This is the primary representation used throughout the compiler and validator.
/// Spans are attached at the AST level (during parsing) rather than being
/// "filled in later" — this ensures validation errors always have source
/// locations without requiring error-prone caller cooperation.
///
/// For programmatically-constructed patterns (e.g., generated by macros or tests),
/// use `SourceSpan::generated()` as a sentinel.
#[derive(Clone, Debug)]
pub struct SpannedPattern {
    pub kind: Pattern,
    pub span: SourceSpan,
}

/// A pattern matching a fact
#[derive(Clone, Debug)]
pub struct FactPattern {
    /// Optional fact binding: ?fact <- (...)
    pub binding: Option<VarId>,
    /// Template or relation to match
    pub template: PatternTemplate,
    /// Slot/field constraints
    pub constraints: Vec<(SlotIndex, PatternConstraint)>,
}

/// Which template or relation this pattern matches
#[derive(Clone, Debug)]
pub enum PatternTemplate {
    Template(TemplateId),
    OrderedRelation(Symbol),
}

/// Constraint within a pattern slot
#[derive(Clone, Debug)]
pub enum PatternConstraint {
    /// Variable binding: ?x
    Variable(VarId),
    /// Literal match
    Literal(Value),
    /// Negation: ~value
    Not(Box<PatternConstraint>),
    /// Disjunction: value1 | value2
    Or(Vec<PatternConstraint>),
    /// Conjunction: value1 & value2
    And(Vec<PatternConstraint>),
    /// Predicate: ?x&:(> ?x 10)
    Predicate { var: Option<VarId>, expr: Expression },
    /// Return value constraint: =(function-call)
    ReturnValue(Expression),
}
```

### 5.7 Actions

```rust
/// An action in the RHS of a rule
#[derive(Clone, Debug)]
pub enum Action {
    /// Assert a new fact
    Assert(FactConstruction),
    /// Retract a fact
    Retract(Expression),
    /// Modify a fact's slots
    Modify { fact: Expression, modifications: Vec<SlotModification> },
    /// Duplicate a fact with modifications
    Duplicate { fact: Expression, modifications: Vec<SlotModification> },
    /// Bind a variable
    Bind { variable: VarId, value: Expression },
    /// Conditional execution
    If { condition: Expression, then: Vec<Action>, else_: Option<Vec<Action>> },
    /// Loop constructs
    While { condition: Expression, body: Vec<Action> },
    Loop { variable: VarId, range: Expression, body: Vec<Action> },
    /// Focus on a module
    Focus(Vec<Symbol>),
    /// Halt execution
    Halt,
    /// Function call (for side effects)
    Call(Expression),
}
```

---

## 6. Rete Network Implementation

### 6.1 Overview

The Rete algorithm is the heart of Ferric's pattern matching. It compiles rules into a discrimination network that efficiently matches facts against patterns.

### 6.2 Node Identity and Sharing

**Design Principle:** Nodes are identified by their semantic content, enabling automatic sharing.

Current sharing guarantees are explicit: alpha paths and positive join nodes are
canonicalized and reused across rules. Specialized control-flow nodes
(`Negative`, `Ncc`, `Exists`) use semantic runtime memories and are not treated
as interchangeable with positive joins.

```rust
/// Unique identifier for a node in the network
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

/// Canonical key for alpha test nodes (used for sharing)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AlphaTestKey {
    /// What we're testing (template/relation)
    pub entry_type: AlphaEntryType,
    /// Sequence of tests from root to this node
    pub test_path: Vec<ConstantTest>,
}

/// Canonical key for join nodes (used for sharing)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JoinNodeKey {
    /// Parent beta node
    pub parent: NodeId,
    /// Alpha memory being joined
    pub alpha_memory: AlphaMemoryId,
    /// Join tests performed
    pub tests: Vec<JoinTest>,
    /// Newly-bound variables extracted from the right fact
    pub bindings: Vec<(SlotIndex, VarId)>,
}
```

### 6.3 Alpha Network

```rust
/// Entry type for alpha network (first discrimination)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AlphaEntryType {
    Template(TemplateId),
    OrderedRelation(Symbol),
}

/// Alpha network node (immutable structure, mutable memory)
pub enum AlphaNode {
    /// Type/relation test - first node after root
    Entry {
        entry_type: AlphaEntryType,
        /// Children: further constant tests
        children: Vec<NodeId>,
        /// Memory for facts passing this test (if any pattern terminates here)
        memory: Option<AlphaMemoryId>,
    },
    
    /// Constant test on a specific slot
    ConstantTest {
        test: ConstantTest,
        /// Children: further tests
        children: Vec<NodeId>,
        /// Memory for patterns terminating at this test sequence
        memory: Option<AlphaMemoryId>,
    },
}

/// A single constant test
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConstantTest {
    pub slot: SlotIndex,
    pub test_type: ConstantTestType,
}

/// Constant test type using AtomKey (not Value) so the type is Eq + Hash.
/// See Section 5.1.1 for the AtomKey design rationale.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConstantTestType {
    Equal(AtomKey),
    NotEqual(AtomKey),
    LessThan(AtomKey),
    GreaterThan(AtomKey),
    LessOrEqual(AtomKey),
    GreaterOrEqual(AtomKey),
}
```

### 6.4 Alpha Memory

Alpha memories store facts passing a particular test sequence. They support indexed lookup for common join patterns.

```rust
/// Identifier for an alpha memory
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AlphaMemoryId(u32);

/// Alpha memory with optional indexing
pub struct AlphaMemory {
    pub id: AlphaMemoryId,
    
    /// Primary storage: all facts in this memory
    facts: HashSet<FactId>,
    
    /// Optional index by slot value (for common join patterns).
    /// Key: (slot_index, AtomKey) → set of matching FactIds.
    /// Uses AtomKey (not Value) because only atomic values are indexable.
    /// Built lazily when a join node requests indexing on a slot.
    slot_indices: HashMap<SlotIndex, HashMap<AtomKey, HashSet<FactId>>>,
    
    /// Which slots are indexed
    indexed_slots: HashSet<SlotIndex>,
}

impl AlphaMemory {
    /// Add a fact to the memory
    pub fn insert(&mut self, fact_id: FactId, fact: &Fact) {
        self.facts.insert(fact_id);
        
        // Update indices (only for slots whose values are atomic/indexable)
        for &slot in &self.indexed_slots {
            if let Some(value) = get_slot_value(fact, slot) {
                if let Some(key) = AtomKey::from_value(value) {
                    self.slot_indices
                        .entry(slot)
                        .or_default()
                        .entry(key)
                        .or_default()
                        .insert(fact_id);
                }
            }
        }
    }

    /// Remove a fact from the memory
    pub fn remove(&mut self, fact_id: FactId, fact: &Fact) {
        self.facts.remove(&fact_id);

        // Update indices and prune empty entries eagerly to avoid quiet memory growth.
        for &slot in &self.indexed_slots {
            if let Some(value) = get_slot_value(fact, slot) {
                if let Some(key) = AtomKey::from_value(value) {
                    if let Some(index) = self.slot_indices.get_mut(&slot) {
                        let mut remove_slot_index = false;
                        if let Some(set) = index.get_mut(&key) {
                            set.remove(&fact_id);
                            if set.is_empty() {
                                index.remove(&key);
                            }
                        }
                        if index.is_empty() {
                            remove_slot_index = true;
                        }
                        if remove_slot_index {
                            self.slot_indices.remove(&slot);
                        }
                    }
                }
            }
        }
    }

    /// Request indexing on a slot.
    /// 
    /// If the memory already contains facts, the index is built immediately
    /// from existing facts. This ensures correctness when rules are loaded
    /// dynamically after facts have already been asserted.
    /// 
    /// Cost: O(n) where n is the number of facts currently in the memory.
    /// This cost is paid once per index request and is acceptable because
    /// index requests happen during rule compilation, not during matching.
    pub fn request_index(&mut self, slot: SlotIndex, fact_base: &FactBase) {
        if !self.indexed_slots.insert(slot) {
            return; // Already indexed
        }
        
        // Build the index from existing facts (if any)
        if !self.facts.is_empty() {
            let index = self.slot_indices.entry(slot).or_default();
            for &fact_id in &self.facts {
                if let Some(fact) = fact_base.get(fact_id) {
                    if let Some(value) = get_slot_value(fact, slot) {
                        if let Some(key) = AtomKey::from_value(value) {
                            index.entry(key).or_default().insert(fact_id);
                        }
                    }
                }
            }
        }
    }

    /// Lookup facts by indexed slot value
    pub fn lookup_by_slot(&self, slot: SlotIndex, key: &AtomKey) -> Option<&HashSet<FactId>> {
        self.slot_indices.get(&slot)?.get(key)
    }

    /// Iterate all facts (when index isn't available)
    pub fn iter(&self) -> impl Iterator<Item = FactId> + '_ {
        self.facts.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}
```

### 6.5 Beta Network

```rust
/// Beta network node types (immutable structure)
pub enum BetaNode {
    /// Root of beta network (empty partial match)
    Root {
        children: Vec<NodeId>,
    },
    
    /// Standard join node
    Join {
        /// Parent beta memory to join against
        parent: NodeId,
        /// Alpha memory providing right-hand facts
        alpha_memory: AlphaMemoryId,
        /// Tests comparing left (token) and right (fact) values
        tests: Vec<JoinTest>,
        /// Output beta memory
        memory: BetaMemoryId,
        /// Children (further joins or terminals)
        children: Vec<NodeId>,
    },
    
    /// Negative node (see Section 7)
    Negative {
        parent: NodeId,
        alpha_memory: AlphaMemoryId,
        tests: Vec<JoinTest>,
        memory: NegativeMemoryId,
        children: Vec<NodeId>,
    },
    
    /// NCC (Negated Conjunctive Condition) node (see Section 7)
    Ncc {
        parent: NodeId,
        /// The subnetwork producing matches to be negated
        subnetwork: NccSubnetwork,
        memory: NccMemoryId,
        children: Vec<NodeId>,
    },
    
    /// NCC partner node (collector at end of subnetwork)
    NccPartner {
        /// The NCC node this feeds into
        ncc_node: NodeId,
        /// Number of conditions in the subnetwork
        condition_count: usize,
    },
    
    /// Terminal node: complete match, creates activation
    Terminal {
        rule: RuleId,
    },
}

/// A test performed at a join node
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JoinTest {
    /// Slot in the incoming fact (right side)
    pub alpha_slot: SlotIndex,
    /// Variable from earlier pattern (left side)
    pub beta_var: VarId,
    /// Comparison type
    pub test_type: JoinTestType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinTestType {
    Equal,
    NotEqual,
    LessThan,
    GreaterThan,
    LessOrEqual,
    GreaterOrEqual,
}
```

### 6.6 Beta Memory and Retraction Cleanup Indices

```rust
/// Identifier for a beta memory
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BetaMemoryId(u32);

/// Beta memory: stores tokens (partial matches)
pub struct BetaMemory {
    pub id: BetaMemoryId,
    
    /// Primary storage (uses global TokenStore for stable IDs)
    tokens: HashSet<TokenId>,
}

impl BetaMemory {
    pub fn insert(&mut self, token_id: TokenId) {
        self.tokens.insert(token_id);
    }

    pub fn remove(&mut self, token_id: TokenId) {
        self.tokens.remove(&token_id);
    }

    pub fn iter(&self) -> impl Iterator<Item = TokenId> + '_ {
        self.tokens.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}
```

#### 6.6.1 O(1) Token Cleanup During Retraction

When a token is removed during retraction, two downstream structures must be updated:

1. **Beta memory removal:** The token must be removed from its owning beta memory (or negative/NCC/exists memory).
2. **Activation removal:** Any agenda activations derived from the token must be removed.

Without reverse pointers, both operations would require scanning all beta memories or the entire agenda — O(total memories) and O(total activations) respectively.

Ferric avoids these scans via two reverse maps:

**Token → owning node (for beta memory cleanup):**

Each `Token` stores its `owner_node: NodeId` (see §5.5). When a token is removed, the engine looks up the node's memory and calls `remove(token_id)` directly — O(1).

Phase 1 baseline status: this owner-node-directed cleanup path is now the
implemented default; the temporary all-memories scan used during early bring-up
has been removed.

**Token → activations (for agenda cleanup):**

```rust
/// Activation identifier for the agenda
slotmap::new_key_type! {
    pub struct ActivationId;
}

/// An activation on the agenda
pub struct Activation {
    pub id: ActivationId,
    pub rule: RuleId,
    pub token: TokenId,
    pub salience: i32,
    pub timestamp: u64,
    /// Monotonically increasing sequence number, assigned on creation.
    /// Used as the final tiebreaker in agenda ordering instead of ActivationId,
    /// because SlotMap keys are NOT monotonic (they reuse indices with
    /// incremented generations), so using them for recency produces
    /// surprising ordering shifts across insertion/removal churn.
    pub activation_seq: u64,
    /// For MEA/LEX strategies: per-fact recency values from the token
    pub recency: SmallVec<[u64; 4]>,
}

/// Composite key that determines activation ordering on the agenda.
/// All fields participate in comparison; the final activation_seq serves
/// as a stable, monotonic tiebreaker to ensure a total order within a run.
///
/// Ord is derived in field order (salience first, then strategy fields, then seq).
/// Higher salience fires first (Reverse wrapper), then strategy-specific,
/// then higher activation_seq fires first.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgendaKey {
    /// Primary: higher salience fires first (wrapped in Reverse for BTreeMap ordering)
    pub salience: std::cmp::Reverse<i32>,
    /// Secondary: strategy-specific ordering value(s)
    pub strategy_ord: StrategyOrd,
    /// Final tiebreaker: higher activation_seq (more recent) fires first.
    /// This is a monotonic counter, NOT an ActivationId (SlotMap keys are
    /// not monotonic and would produce non-deterministic ordering).
    pub seq: std::cmp::Reverse<u64>,
}

/// Strategy-specific ordering component of AgendaKey.
/// Each variant defines what "higher priority" means for that strategy.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrategyOrd {
    /// Depth: most recent activation first (CLIPS default).
    /// Higher timestamp = higher priority.
    Depth(std::cmp::Reverse<u64>),
    /// Breadth: oldest activation first.
    /// Lower timestamp = higher priority.
    Breadth(u64),
    /// LEX (lexicographic recency): compare per-fact recency values
    /// left-to-right, most recent first.
    Lex(std::cmp::Reverse<SmallVec<[u64; 4]>>),
    /// MEA (means-ends analysis): first-pattern recency first,
    /// then remaining recency values lexicographically.
    Mea {
        first_recency: std::cmp::Reverse<u64>,
        rest_recency: std::cmp::Reverse<SmallVec<[u64; 4]>>,
    },
}
```

**Agenda ordering strategy: `BTreeMap<AgendaKey, ActivationId>`**

The agenda uses `BTreeMap<AgendaKey, ActivationId>` as its ordering structure. This was chosen over alternatives because it provides honest O(log n) for all operations — including arbitrary deletion by key (required for retraction cleanup) and pop with identity (required for firing).

```rust
/// The agenda with reverse indexing for efficient retraction cleanup
pub struct Agenda {
    /// Primary ordering: BTreeMap from key → ActivationId.
    /// pop_first() yields the highest-priority (key, act_id) pair.
    /// remove() by key is O(log n) — no lazy tombstones needed.
    /// Using BTreeMap (not BTreeSet) so that pop_first returns the
    /// ActivationId directly — avoids O(n) reverse lookups.
    ordering: BTreeMap<AgendaKey, ActivationId>,
    
    /// Activation data keyed by ActivationId.
    activations: SlotMap<ActivationId, Activation>,
    
    /// AgendaKey lookup by ActivationId (needed for BTreeMap removal).
    id_to_key: HashMap<ActivationId, AgendaKey>,
    
    /// Reverse index: TokenId → set of ActivationIds derived from that token.
    /// Enables O(k log n) activation removal during retraction, where k is the
    /// number of activations for the affected token (typically 0 or 1).
    token_to_activations: HashMap<TokenId, SmallVec<[ActivationId; 2]>>,
    
    /// The active conflict resolution strategy
    strategy: ConflictResolutionStrategy,
    
    /// Monotonically increasing counter for activation sequence numbers.
    /// Assigned to each activation on insertion. Never reused, never decremented.
    /// Provides a stable tiebreaker for agenda ordering within one run.
    next_seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictResolutionStrategy {
    Depth,
    Breadth,
    Lex,
    Mea,
}

impl Agenda {
    /// Remove all activations derived from a given token.
    /// O(k log n) where k is the number of activations for this token.
    pub fn remove_activations_for_token(&mut self, token_id: TokenId) -> Vec<Activation> {
        let mut removed = Vec::new();
        if let Some(act_ids) = self.token_to_activations.remove(&token_id) {
            for act_id in act_ids {
                if let Some(key) = self.id_to_key.remove(&act_id) {
                    self.ordering.remove(&key);
                }
                if let Some(activation) = self.activations.remove(act_id) {
                    removed.push(activation);
                }
            }
        }
        removed
    }
    
    /// Add an activation, maintaining all indices.
    /// Returns the assigned ActivationId.
    pub fn add(&mut self, mut activation: Activation) -> ActivationId {
        let token_id = activation.token;
        let seq = self.next_seq;
        self.next_seq += 1;
        activation.activation_seq = seq;
        let id = self.activations.insert(activation);
        let key = self.build_key(id, seq);
        self.ordering.insert(key.clone(), id);
        self.id_to_key.insert(id, key);
        self.token_to_activations
            .entry(token_id)
            .or_default()
            .push(id);
        id
    }
    
    /// Pop the highest-priority activation. O(log n), no heap allocation.
    pub fn pop(&mut self) -> Option<Activation> {
        let (key, act_id) = self.ordering.pop_first()?;
        self.id_to_key.remove(&act_id);
        let activation = self.activations.remove(act_id)?;
        // Clean up token reverse index
        if let Some(ids) = self.token_to_activations.get_mut(&activation.token) {
            ids.retain(|&aid| aid != act_id);
            if ids.is_empty() {
                self.token_to_activations.remove(&activation.token);
            }
        }
        Some(activation)
    }
}
```

**Strategy ordering table:**

All strategies share salience as the primary key. The table below defines the secondary and tertiary ordering. "Higher priority fires first" in all cases.

| Strategy | Primary | Secondary | Tertiary | Final Tiebreak |
|----------|---------|-----------|----------|----------------|
| **Depth** (default) | Higher salience | Higher timestamp (newer first) | — | Higher activation_seq |
| **Breadth** | Higher salience | Lower timestamp (older first) | — | Higher activation_seq |
| **LEX** | Higher salience | Lexicographic recency of matched facts (most-recent-fact-first per position, left-to-right) | — | Higher activation_seq |
| **MEA** | Higher salience | Higher first-pattern recency | Remaining recency (LEX order) | Higher activation_seq |

**Design notes:**

- **Monotonic `activation_seq` as tiebreaker (not `ActivationId`):** SlotMap keys are *not* monotonic — they reuse indices with incremented generations, so `ActivationId` ordering can shift unpredictably across insertion/removal churn. The `activation_seq: u64` counter is strictly monotonic (incremented on every `add()`, never decremented or reused), so tie handling never depends on key-reuse artifacts. `ActivationId` is used only for identity/lookup (in `id_to_key`, `token_to_activations`, etc.), never for ordering.
- **`BTreeMap` (not `BTreeSet`):** The ordering structure is `BTreeMap<AgendaKey, ActivationId>` rather than `BTreeSet<AgendaKey>`. This is because `pop_first()` must return both the key *and* the corresponding `ActivationId` in O(log n). With a `BTreeSet`, recovering the `ActivationId` from a popped key would require an O(n) reverse scan through `id_to_key`. `BTreeMap::pop_first()` returns `(AgendaKey, ActivationId)` directly.
- **`BTreeMap` over `BinaryHeap`:** `BinaryHeap` does not support efficient arbitrary deletion (only `pop`). Retraction cleanup requires `remove(act_id)`, which in a `BinaryHeap` requires either a positions map + custom sift (fragile) or lazy tombstones (memory leak risk in long-running engines). `BTreeMap` provides honest O(log n) remove.
- If profiling later reveals that the `BTreeMap` overhead is significant (unlikely given typical agenda sizes of 10–1000 activations), it can be replaced with a custom indexed heap without changing the API. The `Agenda` struct encapsulates the ordering strategy entirely.
- `token_to_activations` uses `SmallVec<[ActivationId; 2]>` rather than `[; 1]` because a token that completes a terminal node may produce activations for multiple rules sharing the same beta prefix. Two inline slots covers this common case without heap allocation.
- **LEX/MEA recency vector length invariant:** The recency vector used for LEX and MEA ordering has a fixed length per rule (equal to the number of positive patterns in the rule's LHS). This length is determined at compile time and stored in the rule metadata. The ordering comparison must not depend on vector length (no prefix-wins or length-wins semantics) — two activations for the same rule always have same-length vectors, and two activations for different rules are never compared by recency (salience + activation_seq suffice).
```

**Summary of retraction cleanup cost per token:**

| Operation | Cost | Mechanism |
|-----------|------|-----------|
| Remove from TokenStore | O(1) | SlotMap removal + reverse index updates |
| Remove from owning beta memory | O(1) | `token.owner_node` → node's memory → `HashSet::remove` |
| Remove from agenda | O(k log n), k≈1 | `token_to_activations` reverse index + `BTreeMap::remove` |
| Cascade to children | O(subtree) | `parent_to_children` index |

No global scans are required at any step.
```

### 6.6.2 Determinism Contract

Ferric guarantees that agenda comparisons define a **total order** (no ambiguous comparisons), but it does **not** promise cross-run or cross-platform replay-identical firing order as part of the public API contract.

- Within a single run, agenda operations are stable and well-defined.
- Across runs, equivalent-priority activations may fire in different order due to factors such as hash iteration order and dynamic rule/fact loading order.
- Correctness for Ferric is therefore defined in terms of semantic outcomes (final working-memory state) for rule sets that are order-insensitive at equal priority.

If a deployment requires replay-identical ordering, rule authors must encode explicit precedence (salience/module focus/phase facts) rather than relying on incidental tie order.

### 6.7 Network Compilation

The compilation pipeline is intentionally layered to keep `ferric-core`
parser-agnostic while still preserving rich source diagnostics:

1. `ferric-parser` Stage 2 produces typed constructs (`RuleConstruct`, etc.).
2. `ferric-runtime` translates those constructs into
   `ferric-core` compile models (`CompilableRule` / `CompilableCondition`).
3. `ferric-core::ReteCompiler` performs authoritative validation and network
   construction, sharing common substructure across rules.

```rust
pub struct ReteCompiler {
    /// Existing alpha paths keyed by canonical structure.
    alpha_path_cache: HashMap<AlphaPathKey, AlphaMemoryId>,
    /// Existing positive join nodes keyed by canonical structure.
    join_node_cache: HashMap<JoinNodeKey, NodeId>,
    /// Rule ID allocator.
    next_rule_id: u32,
}

impl ReteCompiler {
    /// Compile a translated rule into the network.
    pub fn compile_rule(
        &mut self,
        rete: &mut ReteNetwork,
        rule: &CompilableRule,
    ) -> Result<CompileResult, CompileError> {
        Self::validate_rule_patterns(&rule.patterns)?;
        let conditions = Self::patterns_as_conditions(&rule.patterns);
        self.compile_conditions_unchecked(rete, rule.rule_id, rule.salience, &conditions)
    }

    /// Compile translated conditions (used by runtime translation path).
    pub fn compile_conditions(
        &mut self,
        rete: &mut ReteNetwork,
        rule_id: RuleId,
        salience: i32,
        conditions: &[CompilableCondition],
    ) -> Result<CompileResult, CompileError> {
        Self::validate_conditions(conditions)?;
        self.compile_conditions_unchecked(rete, rule_id, salience, conditions)
    }
}
```

### 6.8 Network Operations

```rust
/// Delta produced by a fact retraction.
pub struct RetractionDelta {
    pub removed_activations: Vec<Activation>,
    pub added_activations: Vec<ActivationId>,
}

impl ReteNetwork {
    /// Assert a fact: propagate through network, return new activations
    pub fn assert_fact(
        &mut self,
        fact_id: FactId,
        fact: &Fact,
        token_store: &mut TokenStore,
    ) -> Vec<Activation> {
        let mut activations = Vec::new();
        
        // 1. Find entry point (template or relation)
        let entry_type = get_entry_type(fact);
        let entry_node = match self.entry_nodes.get(&entry_type) {
            Some(&node) => node,
            None => return activations, // No rules match this fact type
        };
        
        // 2. Propagate through alpha network
        let alpha_memories = self.propagate_alpha(entry_node, fact_id, fact);
        
        // 3. For each affected alpha memory, propagate to beta network
        for alpha_mem_id in alpha_memories {
            self.propagate_beta_right(
                alpha_mem_id,
                fact_id,
                fact,
                token_store,
                &mut activations,
            );
        }
        
        activations
    }

    /// Retract a fact: update memories and report agenda delta.
    pub fn retract_fact(
        &mut self,
        fact_id: FactId,
        fact: &Fact,
        token_store: &mut TokenStore,
        agenda: &mut Agenda,
    ) -> RetractionDelta {
        let mut removed_activations = Vec::new();

        // 1. Find all directly affected tokens.
        let affected_tokens: HashSet<TokenId> = token_store.tokens_containing(fact_id).collect();

        // 2. Collapse to roots only, so each subtree is removed exactly once.
        let root_tokens = token_store.retraction_roots(&affected_tokens);

        // 3. Remove affected tokens and descendants (using parent→children index).
        for token_id in root_tokens {
            self.remove_token_cascade(token_id, token_store, agenda, &mut removed_activations);
        }

        // 4. Remove from alpha memories.
        self.remove_from_alpha(fact_id, fact);

        // 5. Update negative-side nodes and enqueue any newly unblocked activations.
        let mut added_activations = Vec::new();
        for activation in self.update_negative_on_retract(fact_id, fact, token_store) {
            let id = agenda.add(activation);
            added_activations.push(id);
        }

        RetractionDelta {
            removed_activations,
            added_activations,
        }
    }

    /// Remove a token and all its descendants using TokenStore::remove_cascade.
    /// Uses owner_node for O(1) beta memory cleanup and token_to_activations
    /// for O(k log n) agenda cleanup (see §6.6.1).
    fn remove_token_cascade(
        &mut self,
        token_id: TokenId,
        token_store: &mut TokenStore,
        agenda: &mut Agenda,
        removed_activations: &mut Vec<Activation>,
    ) {
        // Use the TokenStore's cascading delete, which is O(subtree size)
        // thanks to the parent→children index.
        // We need the removed tokens' data (owner_node) for cleanup,
        // so remove_cascade returns (TokenId, Token) pairs.
        let removed_entries = token_store.remove_cascade(token_id);
        
        for (id, token) in &removed_entries {
            self.dispatch_token_retracted(*id, token.owner_node, agenda, removed_activations);
        }
    }

    fn dispatch_token_retracted(
        &mut self,
        token_id: TokenId,
        owner_node: NodeId,
        agenda: &mut Agenda,
        removed_activations: &mut Vec<Activation>,
    ) {
        // 1) Remove from owning memory.
        if let Some(memory) = self.get_node_memory_mut(owner_node) {
            memory.remove(token_id);
        }

        // 2) Notify every side memory that retains TokenIds.
        self.notify_negative_memories_token_retracted(token_id);
        self.notify_ncc_memories_token_retracted(token_id);
        self.notify_exists_memories_token_retracted(token_id);

        // 3) Remove any agenda activations derived from this token.
        removed_activations.extend(agenda.remove_activations_for_token(token_id));
    }
}
```

---

## 7. Negation and Existential Semantics

This section details the handling of `not`, `exists`, and `forall` patterns, which require additional Rete machinery beyond simple joins.

### 7.1 Supported Constructs

| Construct | Support Level | Notes |
|-----------|---------------|-------|
| `(not <single-pattern>)` | Full | Negation of one fact pattern |
| `(not (and <patterns>...))` | Full | Negation of conjunction (NCC) |
| `(exists <pattern>)` | Full | At least one match exists |
| `(forall <cond> <then>)` | Limited | See 7.5 for restrictions |
| Nested `not`/`exists` | Limited | Max depth 2 (see 7.6) |

### 7.2 Negative Node (Single Pattern Negation)

For `(not <single-pattern>)`, we use a Negative node that tracks which tokens are "blocked" by matching facts.

```rust
/// Memory for a Negative node
pub struct NegativeMemory {
    pub id: NegativeMemoryId,
    
    /// Tokens that have passed through (no blocking facts)
    unblocked_tokens: HashSet<TokenId>,
    
    /// Blocker tracking: which facts block which tokens
    /// Key: TokenId, Value: set of FactIds currently blocking it
    blockers: HashMap<TokenId, HashSet<FactId>>,
    
    /// Reverse: which tokens are blocked by each fact
    blocked_by: HashMap<FactId, HashSet<TokenId>>,
}

impl NegativeMemory {
    /// Called when a token arrives from the parent
    pub fn token_arrived(
        &mut self,
        token_id: TokenId,
        blocking_facts: HashSet<FactId>,
    ) -> bool {
        if blocking_facts.is_empty() {
            // No blockers: token passes through
            self.unblocked_tokens.insert(token_id);
            true
        } else {
            // Blocked: record blockers
            for &fact_id in &blocking_facts {
                self.blocked_by.entry(fact_id).or_default().insert(token_id);
            }
            self.blockers.insert(token_id, blocking_facts);
            false
        }
    }

    /// Called when a fact is asserted that might block tokens
    pub fn fact_asserted(
        &mut self,
        fact_id: FactId,
        newly_blocked_tokens: Vec<TokenId>,
    ) -> Vec<TokenId> {
        let mut removed = Vec::new();
        
        for token_id in newly_blocked_tokens {
            if self.unblocked_tokens.remove(&token_id) {
                // Token was passing, now blocked
                removed.push(token_id);
            }
            self.blockers.entry(token_id).or_default().insert(fact_id);
            self.blocked_by.entry(fact_id).or_default().insert(token_id);
        }
        
        removed // These tokens' activations should be removed
    }

    /// Called when a blocking fact is retracted
    pub fn fact_retracted(&mut self, fact_id: FactId) -> Vec<TokenId> {
        let mut newly_unblocked = Vec::new();
        
        if let Some(tokens) = self.blocked_by.remove(&fact_id) {
            for token_id in tokens {
                if let Some(blockers) = self.blockers.get_mut(&token_id) {
                    blockers.remove(&fact_id);
                    if blockers.is_empty() {
                        // No more blockers: token can pass
                        self.blockers.remove(&token_id);
                        self.unblocked_tokens.insert(token_id);
                        newly_unblocked.push(token_id);
                    }
                }
            }
        }
        
        newly_unblocked // These should propagate to children
    }

    /// Called when a parent token is retracted
    pub fn token_retracted(&mut self, token_id: TokenId) {
        self.unblocked_tokens.remove(&token_id);
        if let Some(blockers) = self.blockers.remove(&token_id) {
            for fact_id in blockers {
                if let Some(tokens) = self.blocked_by.get_mut(&fact_id) {
                    tokens.remove(&token_id);
                    // Prune empty sets to prevent quiet memory growth
                    // in long-running engines with high churn.
                    if tokens.is_empty() {
                        self.blocked_by.remove(&fact_id);
                    }
                }
            }
        }
    }
}
```

**Token retraction propagation invariant:**

> **Invariant:** Every node that stores `TokenId`s in any internal data structure (beta memories, negative memories, NCC memories, exists memories, agenda) **must** receive a `token_retracted(token_id)` callback for every token removed via cascade, regardless of the reason for removal.

This invariant prevents stale `TokenId` references from accumulating in side indices. It applies to:

| Node Type | TokenId-bearing structures | Cleanup method |
|-----------|---------------------------|----------------|
| Beta memory | `tokens: HashSet<TokenId>` | `remove(token_id)` via `owner_node` |
| Negative memory | `unblocked_tokens`, `blockers`, `blocked_by` | `token_retracted(token_id)` |
| NCC memory | `unblocked_owners`, `owner_to_results`, `result_to_owner` | `token_retracted(token_id)` |
| Exists memory | `support_count`, `satisfied` | `token_retracted(token_id)` |
| Agenda | `activations`, `ordering`, `id_to_key`, `token_to_activations` | `remove_activations_for_token(token_id)` |

The `remove_token_cascade` method in `ReteNetwork` (§6.8) is responsible for dispatching these callbacks for every token in the removed subtree. Implementation code must not add new TokenId-storing structures without also adding cleanup to this dispatch path.

**Cascade callback ordering policy:**

> **Invariant:** Callbacks dispatched during cascade retraction are **order-independent**. Every cleanup callback must be **idempotent** and **tolerant of missing entries** — i.e., calling `token_retracted(id)` on a node that has already cleaned up `id` (or never stored it) must be a no-op, not a panic or error.

This is preferred over defining a fixed callback ordering because:

1. **Reduced coupling:** Nodes need not reason about which other nodes have already been notified.
2. **Future-proofing:** New node types can be added without auditing ordering constraints.
3. **Simpler testing:** Each callback can be tested in isolation without reproducing a specific dispatch sequence.

In practice, the current `remove_token_cascade` implementation dispatches callbacks in subtree traversal order (parent before children). This is a natural consequence of the implementation, not a guaranteed contract — callers must not depend on it.

**Implementation guidance for cleanup methods:** All `token_retracted`-style cleanup methods must be **total functions** — they must not call `unwrap()`, `expect()`, or use any other panicking accessor on internal lookups. Use `if let Some(…)` or `.get_mut()` checks instead. This follows directly from the idempotence requirement (the entry may already have been cleaned up or may never have existed), but is called out explicitly because `unwrap` in cleanup paths is a common failure mode during refactoring.

**Debug-mode consistency checker:**

To catch "forgot to dispatch callback" regressions early, the engine provides a consistency verification method that can be called in tests and (optionally) after every retraction in debug builds:

```rust
impl Engine {
    /// Verify cross-structure consistency of all token/activation indices.
    /// Intended for use in tests and debug builds. Cost: O(total index entries).
    ///
    /// Panics with a diagnostic message on the first inconsistency found.
    #[cfg(any(test, debug_assertions))]
    pub fn debug_assert_consistency(&self) {
        // 1. Every TokenId in any side index exists in TokenStore.
        //    Covers: fact_to_tokens, parent_to_children, beta memories,
        //    negative memory (unblocked_tokens, blockers, blocked_by values),
        //    NCC memory (unblocked_owners, owner_to_results, result_to_owner),
        //    exists memory (support_count, satisfied).

        // 2. Every ActivationId in the agenda ordering exists in the
        //    activations SlotMap.

        // 3. Every ActivationId in token_to_activations exists in the
        //    activations SlotMap.

        // 4. No empty sets/maps exist where forbidden:
        //    - blocked_by values are non-empty (prune-on-empty invariant)
        //    - owner_to_results values are non-empty

        // 5. id_to_key and ordering are consistent (every key in one
        //    exists in the other).
    }
}
```

This method is called automatically in the retraction invariants test suite (§15.0) after every assert/retract operation. Test authors are encouraged to call it liberally in new tests as a cheap structural regression check.

### 7.3 NCC Node (Negated Conjunctive Condition)

For `(not (and <p1> <p2> ...))`, we need an NCC (Negated Conjunctive Condition) structure. This requires a subnetwork that produces matches for the conjunction, which then block the parent tokens.

Phase 2 establishes full NCC runtime semantics (partner result accounting plus
assert/retract unblock-reblock transitions). Phase 3+ features that rely on
conjunction negation (notably limited `forall`) build on this implementation,
not on alternate lowering strategies.

```rust
/// Subnetwork definition for NCC
pub struct NccSubnetwork {
    /// Entry point of the subnetwork (joins from parent token)
    pub entry: NodeId,
    /// The partner node that collects subnetwork matches
    pub partner: NodeId,
    /// Number of conditions in the conjunction
    pub condition_count: usize,
}

/// Memory for NCC node
pub struct NccMemory {
    pub id: NccMemoryId,
    
    /// Owner tokens (from parent) that are unblocked
    unblocked_owners: HashSet<TokenId>,
    
    /// Owner tokens → subnetwork result tokens blocking them
    owner_to_results: HashMap<TokenId, HashSet<TokenId>>,
    
    /// Subnetwork result token → owner token it blocks
    result_to_owner: HashMap<TokenId, TokenId>,
}

impl NccMemory {
    /// Called when an owner token arrives from parent
    pub fn owner_arrived(&mut self, owner_id: TokenId) -> bool {
        // Check if any subnetwork results block this owner
        if self.owner_to_results.get(&owner_id).map_or(true, |s| s.is_empty()) {
            self.unblocked_owners.insert(owner_id);
            true
        } else {
            false
        }
    }

    /// Called when subnetwork produces a result (potential blocker)
    pub fn subnetwork_match(&mut self, owner_id: TokenId, result_id: TokenId) -> Option<TokenId> {
        self.result_to_owner.insert(result_id, owner_id);
        self.owner_to_results.entry(owner_id).or_default().insert(result_id);
        
        // If owner was unblocked, it's now blocked
        if self.unblocked_owners.remove(&owner_id) {
            Some(owner_id) // Return for activation removal
        } else {
            None
        }
    }

    /// Called when a subnetwork result is retracted
    pub fn subnetwork_retract(&mut self, result_id: TokenId) -> Option<TokenId> {
        if let Some(owner_id) = self.result_to_owner.remove(&result_id) {
            if let Some(results) = self.owner_to_results.get_mut(&owner_id) {
                results.remove(&result_id);
                if results.is_empty() {
                    // Owner is now unblocked
                    self.unblocked_owners.insert(owner_id);
                    return Some(owner_id); // Return for propagation
                }
            }
        }
        None
    }

    /// Cleanup callback for cascade retraction.
    /// Must be idempotent and tolerate missing entries.
    pub fn token_retracted(&mut self, token_id: TokenId) {
        self.unblocked_owners.remove(&token_id);

        // token_id may be an owner
        if let Some(results) = self.owner_to_results.remove(&token_id) {
            for result_id in results {
                self.result_to_owner.remove(&result_id);
            }
        }

        // token_id may be a result
        if let Some(owner_id) = self.result_to_owner.remove(&token_id) {
            if let Some(results) = self.owner_to_results.get_mut(&owner_id) {
                results.remove(&token_id);
                if results.is_empty() {
                    self.owner_to_results.remove(&owner_id);
                    self.unblocked_owners.insert(owner_id);
                }
            }
        }
    }
}
```

### 7.4 Exists Pattern

`(exists <pattern>)` is implemented as a dedicated support-counting Exists node.
While it is semantically equivalent to `(not (not <pattern>))`, Ferric treats
this dedicated node as the canonical implementation path (not merely an
optimization), and later features must interoperate with this memory model.

- When the first matching fact appears, the exists condition is satisfied
- Additional matches don't create additional activations
- When all matches are retracted, the condition becomes unsatisfied

```rust
/// Memory for Exists node (optimized not-not)
pub struct ExistsMemory {
    pub id: ExistsMemoryId,
    
    /// Token → count of supporting facts
    support_count: HashMap<TokenId, usize>,
    
    /// Tokens with at least one supporting fact
    satisfied: HashSet<TokenId>,
    
    /// Fact → tokens it supports
    fact_to_tokens: HashMap<FactId, HashSet<TokenId>>,
}

impl ExistsMemory {
    /// A fact arrived that could satisfy the exists for some tokens
    pub fn fact_supports(&mut self, fact_id: FactId, tokens: Vec<TokenId>) -> Vec<TokenId> {
        let mut newly_satisfied = Vec::new();
        
        for token_id in tokens {
            let count = self.support_count.entry(token_id).or_insert(0);
            *count += 1;
            
            self.fact_to_tokens.entry(fact_id).or_default().insert(token_id);
            
            if *count == 1 {
                // First support: becomes satisfied
                self.satisfied.insert(token_id);
                newly_satisfied.push(token_id);
            }
        }
        
        newly_satisfied
    }

    /// A supporting fact was retracted
    pub fn fact_unsupports(&mut self, fact_id: FactId) -> Vec<TokenId> {
        let mut newly_unsatisfied = Vec::new();
        
        if let Some(tokens) = self.fact_to_tokens.remove(&fact_id) {
            for token_id in tokens {
                if let Some(count) = self.support_count.get_mut(&token_id) {
                    *count -= 1;
                    if *count == 0 {
                        self.support_count.remove(&token_id);
                        self.satisfied.remove(&token_id);
                        newly_unsatisfied.push(token_id);
                    }
                }
            }
        }
        
        newly_unsatisfied
    }

    /// Cleanup callback for cascade retraction.
    /// Must be idempotent and tolerate missing entries.
    pub fn token_retracted(&mut self, token_id: TokenId) {
        self.satisfied.remove(&token_id);
        self.support_count.remove(&token_id);

        // Remove token from every fact support set, pruning empties.
        let mut empty_facts = Vec::new();
        for (&fact_id, tokens) in self.fact_to_tokens.iter_mut() {
            tokens.remove(&token_id);
            if tokens.is_empty() {
                empty_facts.push(fact_id);
            }
        }
        for fact_id in empty_facts {
            self.fact_to_tokens.remove(&fact_id);
        }
    }
}
```

### 7.5 Forall Pattern Restrictions

The `(forall <condition> <then>)` pattern in CLIPS asserts: "for all facts matching `<condition>`, they must also match `<then>`."

**Supported subset:**

```clips
;; Simple forall: all items must be checked
(defrule all-items-checked
    (forall (item ?id)
            (item-checked ?id))
    =>
    (assert (all-items-complete)))
```

**Restrictions:**

1. The `<condition>` must be a single fact pattern (no conjunctions)
2. The `<then>` must be a single fact pattern
3. Variables in `<then>` must be bound in `<condition>` or earlier patterns
4. Nested forall is not supported

These restrictions allow forall to be implemented as a combination of NCC without full subnetwork complexity.

**Vacuous truth semantics:**

If the `<condition>` matches **zero** facts, the `forall` is satisfied (vacuously true), and the rule proceeds as if the constraint were met. This matches CLIPS behavior and standard universal quantifier semantics: "for all X in ∅, P(X)" is true.

This is a common implementation pitfall — modeling forall as "at least one condition match exists and all match the then clause" would be incorrect. The Rete implementation must ensure that when no condition-matching facts exist, the forall node propagates its token as unblocked.

```clips
;; Example: this rule fires even if no (item ...) facts exist,
;; because "all zero items are checked" is vacuously true.
(defrule all-items-checked
    (forall (item ?id)
            (item-checked ?id))
    =>
    (assert (all-items-complete)))
```

**Required regression contract (fixture scaffolded in Phase 2; fully enabled in Phase 3 before forall sign-off):**

```
Test: forall_vacuous_truth_and_retraction_cycle

Step 1: Load rule:
  (defrule all-checked
    (forall (item ?id) (checked ?id))
    => (assert (all-complete)))

Step 2: Run engine with empty working memory.
  EXPECT: rule fires (vacuously true — zero items, all zero are checked).
  EXPECT: (all-complete) is asserted.

Step 3: Assert (item 1). Run engine.
  EXPECT: (all-complete) retracted or rule no longer satisfied
          (item 1 exists but (checked 1) does not).

Step 4: Assert (checked 1). Run engine.
  EXPECT: forall is satisfied again (all items are checked).
  EXPECT: (all-complete) is re-asserted.

Step 5: Retract (checked 1). Run engine.
  EXPECT: forall becomes unsatisfied again.

Step 6: Retract (item 1). Run engine.
  EXPECT: forall becomes vacuously true again (zero items).
```

This test flushes out three common forall implementation bugs: (a) failing to fire on empty condition set, (b) failing to retract on partial mismatch, and (c) failing to re-satisfy after retraction returns to vacuous state. It should be implemented early (Phase 2) before any features are built on top of forall.

**Implementation note (Phase 3 closure):** Ferric uses an internal hidden
`(initial-fact)` to enable standalone negation/forall activation behavior
without requiring user-authored trigger facts.

**Not supported (with clear error messages):**

```clips
;; NOT SUPPORTED: conjunction in condition
(forall (and (item ?id) (item-priority ?id high))
        (item-checked ?id))

;; NOT SUPPORTED: nested forall
(forall (category ?cat)
        (forall (item ?id ?cat) (item-checked ?id)))
```

### 7.6 Nesting Restrictions

To avoid combinatorial complexity, Ferric limits nesting of negation/existential constructs:

| Pattern | Allowed |
|---------|---------|
| `(not <fact>)` | ✓ |
| `(not (and <fact> <fact>))` | ✓ |
| `(not (not <fact>))` | ✓ (use `exists` instead) |
| `(exists <fact>)` | ✓ |
| `(not (exists <fact>))` | ✓ |
| `(exists (not <fact>))` | ✗ Not supported |
| `(not (not (not <fact>)))` | ✗ Not supported |
| `(forall ... (not ...))` | ✗ Not supported |

When unsupported patterns are encountered:
- In **both modes**: Compilation **fails** — the rule is not added to the network. The engine never silently drops or degrades a rule (see §2.3).
- In **strict mode**: The error is reported at `Error` severity.
- In **classic mode**: The error is reported at `Warning` severity (for tooling differentiation), but the `CompileError` return still prevents the rule from being loaded.

The compatibility documentation will include examples of how to refactor unsupported patterns into equivalent supported forms where possible.

### 7.7 Compile-Time Validation of Pattern Restrictions

All restrictions from Sections 7.5 and 7.6 are enforced before network nodes
are constructed. The authoritative gate is the core compiler entry points
(`ReteCompiler::compile_rule` / `compile_conditions`), while runtime-level
pre-validation may be performed for earlier source-located diagnostics. This
ensures violations are caught as explicit errors rather than silently producing
incorrect runtime behavior ("why didn't my rule ever fire?").

```rust
/// Validates pattern restrictions before Rete node construction.
pub struct PatternValidator {
    /// Maximum allowed nesting depth for not/exists
    max_nesting_depth: usize,  // default: 2
}

/// Where in the pipeline a validation error was detected
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationStage {
    /// Detected during AST interpretation (Stage 2 parsing)
    AstInterpretation,
    /// Detected during Rete compilation
    ReteCompilation,
}

/// A pattern restriction violation
#[derive(Debug)]
pub struct PatternValidationError {
    /// Stable, machine-readable error code for programmatic matching.
    /// Format: "E" + 4-digit number (e.g., E0001). Codes are never reused.
    /// See error code table below.
    pub code: &'static str,
    /// Categorized violation kind with structured data.
    pub kind: PatternViolation,
    /// Source location where the violation was detected.
    /// Always present when the parser has produced spans (i.e., when loading
    /// from source text). May be None for programmatically-constructed patterns.
    /// Both the Phase 1 minimal S-expr loader and the later full grammar must
    /// populate this field — validation error quality depends on it.
    pub span: Option<SourceSpan>,
    /// Pipeline stage where the violation was caught.
    pub stage: ValidationStage,
    /// Human-readable suggestion for how to fix the issue.
    pub suggestion: Option<String>,
}

/// Source location for error reporting.
/// Designed to be populated by both the minimal S-expr loader (Phase 1)
/// and the full grammar parser (Phase 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    /// Source file path (or "<repl>" / "<string>" for non-file sources)
    pub file: String,
    /// 1-based line number
    pub line: u32,
    /// 1-based column number (byte offset within line)
    pub col: u32,
    /// Byte offset from start of source
    pub offset: usize,
    /// Length in bytes of the span
    pub len: usize,
}

impl SourceSpan {
    /// Sentinel span for programmatically-constructed patterns (tests, macros).
    /// Clearly distinguishable in error messages.
    pub fn generated() -> Self {
        SourceSpan { file: "<generated>".into(), line: 0, col: 0, offset: 0, len: 0 }
    }
}

/// Stable error codes for pattern validation errors.
/// These codes are part of the public API contract and will not be reused.
///
/// | Code  | Violation |
/// |-------|-----------|
/// | E0001 | Nesting depth exceeded |
/// | E0002 | Forall condition is not a single fact pattern |
/// | E0003 | Nested forall |
/// | E0004 | Unbound variable in forall <then> clause |
/// | E0005 | Unsupported nesting combination (including invalid forall <then>) |
///
/// **Error code policy (stable contract):**
/// - Codes are **append-only**: new validation rules receive the next unused
///   code (E0006, E0007, ...). Existing codes are **never renumbered or reused**.
/// - A code's meaning is fixed once assigned. If a validation rule's semantics
///   change substantially, it receives a new code; the old code is retired
///   (documented as "deprecated — see Exxxx") but never reassigned.
/// - Refined or more specific error messages for an existing code are permitted
///   as long as the underlying violation category is unchanged.
/// - This policy exists because external tools and test suites will match on
///   these codes programmatically.

#[derive(Debug)]
pub enum PatternViolation {
    /// e.g., (exists (not <fact>)) or triple-nested not
    NestingTooDeep { depth: usize, max: usize },
    /// e.g., (forall (and ...) ...)
    ForallConditionNotSinglePattern,
    /// e.g., (forall ... (forall ...))
    NestedForall,
    /// Variables in forall <then> not bound in <condition> or earlier
    ForallUnboundVariable { var_name: String },
    /// Unsupported nesting combination
    UnsupportedNestingCombination { description: String },
}

impl PatternValidator {
    /// Validate all patterns in a rule's LHS.
    /// Called before any Rete nodes are built.
    ///
    /// Accepts `SpannedPattern` (not bare `Pattern`) so that spans are always
    /// available at validation time. This avoids the "filled in by caller later"
    /// anti-pattern which tends to result in span: None in practice.
    pub fn validate_patterns(&self, patterns: &[SpannedPattern]) -> Result<(), Vec<PatternValidationError>> {
        let mut errors = Vec::new();
        for pattern in patterns {
            self.validate_pattern(pattern, 0, &mut errors);
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    fn validate_pattern(
        &self,
        pattern: &SpannedPattern,
        nesting_depth: usize,
        errors: &mut Vec<PatternValidationError>,
    ) {
        match &pattern.kind {
            Pattern::Not(inner) => {
                let new_depth = nesting_depth + 1;
                if new_depth > self.max_nesting_depth {
                    errors.push(PatternValidationError {
                        code: "E0001",
                        kind: PatternViolation::NestingTooDeep {
                            depth: new_depth,
                            max: self.max_nesting_depth,
                        },
                        span: Some(pattern.span.clone()),
                        stage: ValidationStage::ReteCompilation,
                        suggestion: Some(
                            "Consider using (exists ...) instead of (not (not ...))".into()
                        ),
                    });
                }
                self.validate_pattern(inner, new_depth, errors);
            }
            Pattern::Exists(inner) => {
                let new_depth = nesting_depth + 1;
                if new_depth > self.max_nesting_depth {
                    errors.push(PatternValidationError {
                        code: "E0001",
                        kind: PatternViolation::NestingTooDeep {
                            depth: new_depth,
                            max: self.max_nesting_depth,
                        },
                        span: Some(pattern.span.clone()),
                        stage: ValidationStage::ReteCompilation,
                        suggestion: None,
                    });
                }
                self.validate_pattern(inner, new_depth, errors);
            }
            Pattern::Forall { condition, then } => {
                // Validate: condition must be a single fact pattern.
                if !matches!(condition.kind, Pattern::Fact(_)) {
                    errors.push(PatternValidationError {
                        code: "E0002",
                        kind: PatternViolation::ForallConditionNotSinglePattern,
                        span: Some(condition.span.clone()),
                        stage: ValidationStage::ReteCompilation,
                        suggestion: Some(
                            "The forall condition must be a single fact pattern. \
                             Consider restructuring with multiple rules.".into()
                        ),
                    });
                }

                // Validate: then must also be a single fact pattern.
                if !matches!(then.kind, Pattern::Fact(_)) {
                    errors.push(PatternValidationError {
                        code: "E0005",
                        kind: PatternViolation::UnsupportedNestingCombination {
                            description: "forall <then> must be a single fact pattern".into(),
                        },
                        span: Some(then.span.clone()),
                        stage: ValidationStage::ReteCompilation,
                        suggestion: Some(
                            "Move additional conjunction/negation logic into separate rules.".into()
                        ),
                    });
                }

                // Validate variable scope when both sides are fact patterns.
                if let (Pattern::Fact(cond), Pattern::Fact(then_fact)) = (&condition.kind, &then.kind) {
                    self.check_forall_variable_scope(cond, then_fact, then.span.clone(), errors);
                }

                // No nested forall.
                self.check_no_nested_forall(then, errors);
            }
            Pattern::And(children) => {
                for child in children {
                    self.validate_pattern(child, nesting_depth, errors);
                }
            }
            Pattern::Or(children) => {
                for child in children {
                    self.validate_pattern(child, nesting_depth, errors);
                }
            }
            Pattern::Fact(_) | Pattern::Test(_) => {
                // Leaf patterns: always valid
            }
        }
    }

    fn check_no_nested_forall(&self, pattern: &SpannedPattern, errors: &mut Vec<PatternValidationError>) {
        if matches!(pattern.kind, Pattern::Forall { .. }) {
            errors.push(PatternValidationError {
                code: "E0003",
                kind: PatternViolation::NestedForall,
                span: Some(pattern.span.clone()),
                stage: ValidationStage::ReteCompilation,
                suggestion: Some(
                    "Nested forall is not supported. Consider using multiple \
                     rules or restructuring with exists/not.".into()
                ),
            });
        }
    }

    fn check_forall_variable_scope(
        &self,
        condition: &FactPattern,
        then_fact: &FactPattern,
        then_span: SourceSpan,
        errors: &mut Vec<PatternValidationError>,
    ) {
        let mut bound = HashSet::new();
        if let Some(v) = condition.binding {
            bound.insert(v);
        }
        for (_, c) in &condition.constraints {
            if let PatternConstraint::Variable(v) = c {
                bound.insert(*v);
            }
        }

        for (_, c) in &then_fact.constraints {
            if let PatternConstraint::Variable(v) = c {
                if !bound.contains(v) {
                    errors.push(PatternValidationError {
                        code: "E0004",
                        kind: PatternViolation::ForallUnboundVariable {
                            var_name: format!("{:?}", v),
                        },
                        span: Some(then_span.clone()),
                        stage: ValidationStage::ReteCompilation,
                        suggestion: Some(
                            "Bind this variable in the forall condition or an earlier LHS pattern."
                                .into(),
                        ),
                    });
                }
            }
        }
    }
}
```

**Validation pipeline summary:**

| Stage | What is validated | Effect of violation |
|-------|-------------------|---------------------|
| Stage 2 (AST interpretation) | Syntactic validity: correct keyword usage, balanced parens, valid variable references | `InterpretError` with source span |
| Runtime translation (loader) | Construct-shape checks and source-aware pre-validation before translation into compiler models | Load fails with explicit validation/compile errors |
| Core compilation | Semantic restrictions: nesting depth, NCC/exists/forall constraints, variable scoping across patterns | `CompileError::Validation` / `PatternValidationError` with stable codes |

All stages produce errors with source locations where available. No unsupported
pattern ever silently enters the Rete network.

---

## 8. Parser and Language

### 8.1 Two-Stage Parsing Architecture

Ferric uses a two-stage parsing approach for better error recovery and faster iteration:

**Stage 1: S-Expression Parsing**
- Tokenize input into atoms, strings, numbers, and delimiters
- Build tree of S-expressions with source spans
- No semantic knowledge—just structural parsing

**Stage 2: Construct Interpretation**
- Walk S-expression tree
- Interpret `(defrule ...)`, `(deftemplate ...)`, etc. into typed AST
- Produce detailed error messages referencing source spans

```
Source Code
    │
    ▼
┌─────────────────────┐
│   Stage 1: Lexer    │
│   + S-expr Parser   │
└─────────────────────┘
    │
    ▼
  SExpr Tree (with Spans)
    │
    ▼
┌─────────────────────┐
│   Stage 2:          │
│   Construct         │
│   Interpreter       │
└─────────────────────┘
    │
    ▼
  Typed AST (Vec<Construct>)
    │
    ▼
  Runtime Translation
  (CompilableRule / CompilableCondition)
    │
    ▼
  Core ReteCompiler
  (parser-agnostic validation + compilation)
```

### 8.2 Stage 1: Lexer and S-Expressions

```rust
/// Token from lexer
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    LeftParen,
    RightParen,
    Integer(i64),
    Float(f64),
    String(String),
    Symbol(String),
    SingleVar(String),    // ?name
    MultiVar(String),     // $?name
    GlobalVar(String),    // ?*MODULE::name* (MODULE optional for current-module lookup)
    Ampersand,            // &
    Pipe,                 // |
    Tilde,                // ~
    Colon,                // :
    Equals,               // =
    LeftArrow,            // <-
}

/// S-expression with source location
#[derive(Clone, Debug)]
pub enum SExpr {
    Atom(Atom, Span),
    List(Vec<SExpr>, Span),
}

#[derive(Clone, Debug)]
pub enum Atom {
    Integer(i64),
    Float(f64),
    String(String),
    Symbol(String),
    SingleVar(String),
    MultiVar(String),
    GlobalVar(String),     // ?*MODULE::name* (MODULE optional for current-module lookup)
    Connective(Connective),  // & | ~ : = <-
}

#[derive(Clone, Copy, Debug)]
pub enum Connective {
    And,        // &
    Or,         // |
    Not,        // ~
    Colon,      // :
    Equals,     // =
    Assign,     // <-
}

/// Source location
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub start: Position,
    pub end: Position,
    pub file_id: FileId,
}

#[derive(Clone, Copy, Debug)]
pub struct Position {
    pub offset: usize,
    pub line: u32,
    pub column: u32,
}
```

```rust
/// Parse source into S-expressions
pub struct ParseResult {
    pub exprs: Vec<SExpr>,
    pub errors: Vec<ParseError>,
}

pub fn parse_sexprs(source: &str, file_id: FileId) -> ParseResult {
    // Lex first. Current implementation returns early with converted parse
    // errors if lexing fails (no partial token-stream parsing).
    let tokens = match lex(source, file_id) {
        Ok(tokens) => tokens,
        Err(lex_errors) => {
            return ParseResult {
                exprs: Vec::new(),
                errors: lex_errors
                    .into_iter()
                    .map(|e| ParseError::new(e.message, e.span, e.kind))
                    .collect(),
            };
        }
    };

    parse_sexpr_list(&tokens)
}
```

**Error Recovery:**

The S-expression parser implements recovery strategies:
- On unexpected token, skip to next `)` at same depth
- On unclosed `(`, report error with opening location
- Continue parsing to find multiple errors

### 8.3 Stage 2: Construct Interpretation

Phase 2 baseline note:

- Stage 2 interpretation is the default path for `defrule`, `deftemplate`,
  and `deffacts` in runtime loading.
- Additional top-level constructs (`deffunction`, `defglobal`, `defmodule`,
  `defgeneric`, `defmethod`) are now interpreted and executable in runtime
  load/execute flows (Phase 3 closure); unsupported sentinel coverage for this
  area moved to out-of-scope constructs such as `defclass`.
- `Engine::load_str` / `Engine::load_file` return
  `Result<LoadResult, Vec<LoadError>>`, where `LoadResult` carries asserted
  facts, typed constructs, and warnings.
- Runtime translates Stage 2 constructs into parser-agnostic compiler models
  before calling `ferric-core::ReteCompiler`.
- Unsupported pattern/constraint forms fail with explicit validation/compile
  diagnostics; they are never silently dropped.

```rust
/// Top-level construct
#[derive(Clone, Debug)]
pub enum Construct {
    Rule(RuleConstruct),
    Template(TemplateConstruct),
    Facts(FactsConstruct),
}

/// Interpret S-expressions into constructs
pub fn interpret_constructs(
    sexprs: &[SExpr],
    config: &InterpreterConfig,
) -> InterpretResult {
    let mut out = InterpretResult::default();
    for sexpr in sexprs {
        match interpret_one(sexpr, config) {
            Ok(c) => out.constructs.push(c),
            Err(e) => {
                out.errors.push(e);
                if config.strict {
                    break;
                }
            }
        }
    }
    out
}

fn interpret_one(sexpr: &SExpr, config: &InterpreterConfig) -> Result<Construct, InterpretError> {
    let list = sexpr.as_list().ok_or_else(|| {
        InterpretError::expected("construct", sexpr.span())
    })?;

    let head = list.first().ok_or_else(|| {
        InterpretError::empty_construct(sexpr.span())
    })?;

    let keyword = head.as_symbol().ok_or_else(|| {
        InterpretError::expected("construct keyword", head.span())
    })?;

    match keyword {
        "defrule" => interpret_rule(&list[1..], sexpr.span()),
        "deftemplate" => interpret_template(&list[1..], sexpr.span()),
        "deffacts" => interpret_facts(&list[1..], sexpr.span()),
        _ => Err(InterpretError::unknown_construct(keyword, head.span())),
    }
}
```

### 8.4 Error Messages

Errors include source context for helpful diagnostics:

```rust
#[derive(Debug)]
pub struct InterpretError {
    pub message: String,
    pub span: Span,
    pub kind: ErrorKind,
    pub suggestions: Vec<Suggestion>,
}

impl InterpretError {
    pub fn format(&self, source: &str) -> String {
        let line = get_line(source, self.span.start.line);
        let pointer = " ".repeat(self.span.start.column as usize) + "^";

        format!(
            "error: {}\n  --> {}:{}:{}\n   |\n{:3} | {}\n   | {}\n",
            self.message,
            self.span.file_id,
            self.span.start.line,
            self.span.start.column,
            self.span.start.line,
            line,
            pointer
        )
    }
}
```

Example output:

```
error: expected pattern or '=>', found 'unknown-keyword'
  --> rules.clp:15:5
   |
15 |     (unknown-keyword ?x)
   |     ^
   |
help: did you mean to use 'test' for a conditional check?
```

### 8.5 Supported Grammar

```ebnf
(* Top-level constructs *)
construct = defrule | deftemplate | deffacts | deffunction
          | defglobal | defmodule | defgeneric | defmethod ;

(* Rule definition *)
defrule = "(" "defrule" rule-name [comment] [declaration]
          pattern* "=>" action* ")" ;

rule-name = symbol ;
declaration = "(" "declare" declaration-item* ")" ;
declaration-item = "(" "salience" integer ")"
                 | "(" "auto-focus" boolean ")" ;

(* Pattern matching *)
pattern = "(" pattern-ce ")" | assigned-pattern | not-ce | and-ce | or-ce
        | exists-ce | forall-ce | test-ce ;

assigned-pattern = single-variable "<-" pattern ;
pattern-ce = template-name constraint* ;
constraint = literal | single-variable | multi-variable
           | connected-constraint | predicate-constraint ;

connected-constraint = constraint "&" constraint
                     | constraint "|" constraint
                     | "~" constraint ;

predicate-constraint = constraint "&:" "(" function-call ")" ;

(* Actions *)
action = "(" action-name expression* ")" ;
action-name = "assert" | "retract" | "modify" | "duplicate"
            | "bind" | "if" | "while" | "loop-for-count"
            | "focus" | "halt" | function-name ;

(* Templates *)
deftemplate = "(" "deftemplate" template-name [comment]
              slot-definition* ")" ;

slot-definition = "(" "slot" slot-name slot-attribute* ")"
                | "(" "multislot" slot-name slot-attribute* ")" ;

slot-attribute = "(" "default" expression ")"
               | "(" "default-dynamic" expression ")"
               | "(" "type" type-spec+ ")"
               | "(" "allowed-values" value+ ")"
               | "(" "range" range-spec range-spec ")" ;

(* Functions *)
deffunction = "(" "deffunction" function-name [comment]
              "(" parameter* [wildcard-parameter] ")"
              action* ")" ;

(* Globals *)
defglobal = "(" "defglobal" [module-name]
            global-assignment* ")" ;

global-assignment = global-variable "=" expression ;
(* global-variable uses canonical '?*MODULE::name*' syntax; omitting MODULE
   binds/reads the current module's local global namespace. *)

(* Modules *)
defmodule = "(" "defmodule" module-name [comment]
            [export-spec] [import-spec] ")" ;
```

---

## 9. Runtime Environment

### 9.1 Engine Configuration

```rust
/// Configuration for creating an engine
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// How to handle errors and edge cases
    pub error_mode: ErrorMode,

    /// Text encoding rules
    pub string_encoding: StringEncoding,

    /// Initial conflict resolution strategy
    pub strategy: ConflictResolutionStrategy,

    /// Whether to track statistics
    pub enable_statistics: bool,

    /// Tracing/logging configuration
    pub tracing: TracingConfig,

    /// Maximum callable depth (`deffunction` / `defmethod` / `call-next-method`)
    /// before recursion-limit diagnostics are raised.
    pub max_call_depth: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum ErrorMode {
    /// Match CLIPS behavior: warn and continue
    #[default]
    Classic,
    /// Fail fast on errors
    Strict,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum StringEncoding {
    /// ASCII only (CLIPS compatible)
    Ascii,
    /// Full UTF-8 support
    #[default]
    Utf8,
    /// ASCII symbols, UTF-8 strings
    AsciiSymbolsUtf8Strings,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum ConflictResolutionStrategy {
    #[default]
    Depth,
    Breadth,
    Lex,
    Mea,
}

// Additional CLIPS strategies (Simplicity/Complexity/Random) are explicitly
// deferred until they are fully specified with ordering semantics and tests.

impl EngineConfig {
    pub fn new() -> Self { ... }
    pub fn classic() -> Self { ... }  // CLIPS-compatible defaults
    pub fn strict() -> Self { ... }   // Modern strict defaults
}

// Constructor defaults:
// - new() / classic() / strict() all initialize max_call_depth = 64.
```

### 9.2 Engine API

The API below combines the long-term target surface with explicit post-Phase-2
baseline notes:

- `run` / `step` / `halt` / `reset` are implemented.
- `reset` / `clear` are deferred until the current action batch completes;
  once applied, the current `run()` invocation returns so callers observe the
  new post-reset/clear state before any subsequent firing cycle.
- RHS action execution is live for `assert`, `retract`, and `halt`.
- `modify` / `duplicate` are template-aware for ordered and template facts.
- `printout` is implemented with per-channel output routing.
- `printout` channel contract: first argument must be a literal symbol/string;
  non-literal channel forms emit a diagnostic.
- Focus query/set contract: `set_focus` replaces the stack with one module;
  `get_focus` returns the top module name; `get_focus_stack` returns names
  bottom-to-top. `run` pops transient focus frames when empty but preserves
  the final baseline focus frame across runs; `reset` restores `[MAIN]`.
- Generic dispatch baseline is deterministic by method index order. Auto-index
  assignment is registration-order-based in Phase 3; CLIPS-style specificity
  ranking and `call-next-method` are explicit Phase 4 compatibility work.
- Name-collision policy transition: Phase 3 currently allows `deffunction`
  precedence over same-name `defgeneric`; Phase 4 tightens this to CLIPS-style
  definition-time conflict diagnostics.
- Function-call expressions route through the runtime function registry;
  broad built-in coverage is phased in via Sections 10 and 15.

```rust
/// The main Ferric engine
pub struct Engine {
    // Core components
    rete: ReteNetwork,
    fact_base: FactBase,
    token_store: TokenStore,
    agenda: Agenda,

    // Definitions
    templates: HashMap<TemplateId, Template>,
    rules: HashMap<RuleId, Rule>,
    functions: HashMap<Symbol, Function>,
    globals: HashMap<Symbol, Value>,
    modules: HashMap<ModuleId, Module>,

    // State
    current_module: ModuleId,
    symbol_table: SymbolTable,
    config: EngineConfig,
    statistics: Option<Statistics>,

    // Execution state
    halt_requested: bool,
    timestamp: u64,
}

impl Engine {
    /// Create a new engine with the given configuration
    pub fn new(config: EngineConfig) -> Self;

    // === Loading ===

    /// Load constructs from a string
    /// Phase 1 return shape: successful facts/rules + warnings, or collected errors.
    pub fn load_str(&mut self, source: &str) -> Result<LoadResult, Vec<LoadError>>;

    /// Load constructs from a file
    pub fn load_file(&mut self, path: &Path) -> Result<LoadResult, Vec<LoadError>>;

    // === Facts ===

    /// Assert an ordered fact by relation name and positional fields.
    /// Implemented in Phase 1 as a convenience surface.
    pub fn assert_ordered(
        &mut self,
        relation: &str,
        fields: Vec<Value>,
    ) -> Result<FactId, AssertError>;

    /// Assert a fact from a string
    /// Phase 2+
    pub fn assert_str(&mut self, fact_str: &str) -> Result<FactId, AssertError>;

    /// Assert a programmatically constructed fact
    pub fn assert(&mut self, fact: Fact) -> Result<FactId, AssertError>;

    /// Retract a fact by ID
    pub fn retract(&mut self, fact_id: FactId) -> Result<(), RetractError>;

    /// Get a fact by ID
    pub fn get_fact(&self, fact_id: FactId) -> Option<&Fact>;

    /// Iterate over all facts
    pub fn facts(&self) -> impl Iterator<Item = (FactId, &Fact)>;

    /// Query facts matching a pattern
    /// Phase 2+
    pub fn query(&self, pattern: &str) -> Result<Vec<FactId>, QueryError>;

    // === Execution ===

    /// Run rules until completion or limit
    pub fn run(&mut self, limit: RunLimit) -> Result<RunResult, RunError>;

    /// Execute a single rule activation
    pub fn step(&mut self) -> Result<StepResult, RunError>;

    /// Reset the engine (retract all facts, reassert deffacts)
    pub fn reset(&mut self) -> Result<(), ResetError>;

    /// Clear the engine (remove all constructs)
    pub fn clear(&mut self);

    /// Request halt (checked between rule firings)
    pub fn halt(&mut self);

    // === Thread Transfer ===

    /// Explicitly rebind engine ownership to the current thread.
    ///
    /// # Safety
    /// Caller must guarantee no references into engine internals are used
    /// after transfer from the old owning thread.
    pub unsafe fn move_to_current_thread(&mut self);

    // === Symbols and Strings (encoding-aware) ===

    /// Intern a symbol, enforcing encoding constraints
    pub fn intern_symbol(&mut self, s: &str) -> Result<Symbol, EncodingError>;

    /// Create a string value, enforcing encoding constraints
    pub fn create_string(&self, s: &str) -> Result<FerricString, EncodingError>;

    // === Functions ===

    /// Call a function by name
    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, CallError>;

    /// Register a Rust function as callable
    pub fn register_function<F>(&mut self, name: &str, func: F) -> Result<(), RegisterError>
    where
        F: Fn(&mut Engine, &[Value]) -> Result<Value, String> + 'static;

    // === Globals ===

    /// Get a global variable value
    pub fn get_global(&self, name: &str) -> Option<&Value>;

    /// Set a global variable value
    ///
    /// `bind` follows the same visibility contract as `get_global`: it may
    /// only target globals visible from the current module and must report an
    /// error for undeclared globals (no implicit global creation).
    pub fn set_global(&mut self, name: &str, value: Value) -> Result<(), GlobalError>;

    // === Modules ===

    /// Set the current module focus
    pub fn set_focus(&mut self, module: &str) -> Result<(), ModuleError>;

    /// Get the current focus module name (top of focus stack)
    pub fn get_focus(&self) -> Option<&str>;

    /// Get the full focus stack (bottom to top)
    pub fn get_focus_stack(&self) -> Vec<&str>;

    /// Get the current module
    pub fn current_module(&self) -> &str;

    // === Introspection ===

    /// List all defined rules
    pub fn rules(&self) -> impl Iterator<Item = &Rule>;

    /// List all defined templates
    pub fn templates(&self) -> impl Iterator<Item = &Template>;

    /// Get engine statistics
    pub fn statistics(&self) -> Option<&Statistics>;

    /// Get the current agenda
    pub fn agenda(&self) -> &Agenda;
}

/// Limit for run operations
pub enum RunLimit {
    /// Run until agenda is empty or halt
    Unlimited,
    /// Run at most N rules
    Rules(usize),
}

/// Result of a run operation
pub struct RunResult {
    pub rules_fired: usize,
    pub halted: bool,
}

/// Result of a single step
pub enum StepResult {
    /// A rule was fired
    Fired { rule_name: String },
    /// No activations available
    Empty,
    /// Engine was halted
    Halted,
}
```

### 9.3 Symbol Table

Implementation note: `SymbolTable` currently lives in `ferric-core` and is
re-exported into runtime-facing APIs.

```rust
/// Symbol interning with encoding awareness
pub(crate) struct SymbolTable {
    /// ASCII symbols (used in Ascii and AsciiSymbolsUtf8Strings modes)
    ascii_to_id: HashMap<Box<[u8]>, u32>,
    ascii_strings: Vec<Box<[u8]>>,

    /// UTF-8 symbols (used in Utf8 mode)
    utf8_to_id: HashMap<Box<str>, u32>,
    utf8_strings: Vec<Box<str>>,
}

impl SymbolTable {
    pub fn intern_ascii(&mut self, s: &[u8]) -> SymbolId {
        if let Some(&id) = self.ascii_to_id.get(s) {
            return SymbolId::Ascii(id);
        }
        let id = self.ascii_strings.len() as u32;
        let boxed: Box<[u8]> = s.into();
        self.ascii_to_id.insert(boxed.clone(), id);
        self.ascii_strings.push(boxed);
        SymbolId::Ascii(id)
    }

    pub fn intern_utf8(&mut self, s: &str) -> SymbolId {
        if let Some(&id) = self.utf8_to_id.get(s) {
            return SymbolId::Utf8(id);
        }
        let id = self.utf8_strings.len() as u32;
        let boxed: Box<str> = s.into();
        self.utf8_to_id.insert(boxed.clone(), id);
        self.utf8_strings.push(boxed);
        SymbolId::Utf8(id)
    }

    pub fn resolve(&self, id: SymbolId) -> &[u8] {
        match id {
            SymbolId::Ascii(i) => &self.ascii_strings[i as usize],
            SymbolId::Utf8(i) => self.utf8_strings[i as usize].as_bytes(),
        }
    }

    pub fn resolve_str(&self, id: SymbolId) -> Option<&str> {
        match id {
            SymbolId::Ascii(i) => std::str::from_utf8(&self.ascii_strings[i as usize]).ok(),
            SymbolId::Utf8(i) => Some(&self.utf8_strings[i as usize]),
        }
    }
}
```

---

## 10. Standard Library

### 10.1 Function Registration

All standard library functions are registered at engine initialization:

```rust
fn register_stdlib(engine: &mut Engine) -> Result<(), RegisterError> {
    // Predicate functions
    engine.register_function("eq", stdlib::eq)?;
    engine.register_function("neq", stdlib::neq)?;
    engine.register_function("numberp", stdlib::numberp)?;
    // ... etc

    // Math functions
    engine.register_function("+", stdlib::add)?;
    engine.register_function("-", stdlib::subtract)?;
    // ... etc

    Ok(())
}
```

### 10.2 Function Categories

The standard library is implemented in phases. v10 locks a concrete minimum surface so this plan is standalone.

| Category | Minimum v1 Function Set | Notes |
|----------|--------------------------|-------|
| Predicate | `eq`, `neq`, `=`, `!=`, `>`, `<`, `>=`, `<=`, `numberp`, `integerp`, `floatp`, `symbolp`, `stringp`, `multifieldp` | Type/introspection and comparisons |
| Math | `+`, `-`, `*`, `/`, `mod`, `abs`, `min`, `max` | Numeric ops only; overflow/NaN semantics documented |
| String/Symbol | `str-cat`, `str-length`, `sub-string`, `sym-cat` | Must follow encoding semantics from §2.4.1 |
| Multifield | `create$`, `length$`, `nth$`, `member$`, `subsetp` | No implicit flattening beyond CLIPS behavior |
| Fact Ops | `assert`, `retract`, `modify`, `duplicate` | `assert`/`retract` are fully operational; template-aware `modify`/`duplicate` implemented in Phase 3 |
| I/O | `printout` | Implemented in Phase 3 runtime; channel argument is literal symbol/string only (non-literal yields diagnostic). Phase 4 extends broader I/O surface (`format`, `read`, `readline`) and propagates `read`/`readline` input buffering through nested `deffunction` / `defmethod` / `call-next-method` frames |
| Agenda Ops | `run`, `halt`, `focus`, `get-focus`, `get-focus-stack`, `list-focus-stack`, `agenda` | Must not bypass agenda invariants; query-surface parity is completed in Phase 4 |
| Environment | `reset`, `clear` | Administrative controls |

Functions outside this table are explicitly deferred until they are listed in this document with tests and compatibility notes.

---

## 11. C FFI Layer

### 11.1 Design Goals

1. **API Compatibility:** Mirror CLIPS 6.4 C API where sensible
2. **Safety:** Rust safety guarantees preserved; invalid usage returns errors
3. **Simplicity:** Opaque handles, clear ownership semantics
4. **Portability:** Works across all target platforms
5. **Consistent Error Handling:** Unified approach for all failure modes

### 11.2 Thread Safety Contract

**⚠️ IMPORTANT: Ferric engine instances are thread-affine (NOT thread-safe).**

A `FerricEngine*` must be used exclusively on the thread that created it. It must not be accessed from other threads, even with external synchronization. The Rust-only `unsafe` transfer escape hatch from §2.1 is intentionally not exposed through C.

This contract is documented in three places to minimize the chance of misuse:

1. **This plan** (here)
2. **The generated C header** (`ferric.h`) — as a prominent block comment at the top of the file
3. **The Rust-side `Engine` documentation** — on the type itself (and mechanically enforced via `!Send + !Sync`)

**Runtime enforcement:**

In addition to compile-time `!Send + !Sync` (which only protects Rust callers), the FFI layer performs a runtime thread-ID check on every `ferric_engine_*` entry point:

- The engine stores the `std::thread::ThreadId` of the thread that called `ferric_engine_new()`.
- On each subsequent `ferric_engine_*` call, the current thread ID is compared to the stored creator.
- **Debug builds:** mismatch triggers an immediate `assert!` failure (loud, unmissable).
- **Release builds:** mismatch returns `FERRIC_ERROR_THREAD_VIOLATION` and sets the thread-local error message to a diagnostic string including both thread IDs.

This check costs one `thread::current().id()` call per entry point (a cheap TLS read on all major platforms). It catches the most common C/FFI misuse scenario — accidentally passing an engine handle to a callback running on a thread pool — at the point of misuse rather than at the point of corruption.

**ABI contract:** The thread-ID check is performed **before any mutation or mutable borrows of internal fields** in every `ferric_engine_*` entry point. This is part of the stable ABI — a `FERRIC_ERROR_THREAD_VIOLATION` return guarantees that no engine state was modified by the call. This prevents "partial mutation then error" scenarios if entry points grow more complex over time.

**Canonical implementation pattern:** Every `extern "C"` entry point that takes a `*mut FerricEngine` must follow this two-step cast sequence:

```rust
// Step 1: Cast to shared reference — safe for check_thread() (reads only).
let engine: &Engine = unsafe { &*engine_ptr };
engine.check_thread()?;

// Step 2: Only NOW cast to mutable reference for real work.
let engine: &mut Engine = unsafe { &mut *engine_ptr };
engine.do_actual_work()
```

This makes "doing it right" the default: `check_thread()` takes `&self`, so the mutable borrow (which would allow accidental field mutation) is not available until after the check passes. All `ferric_engine_*` entry points must use this pattern.

```c
/*
 * ============================================================================
 * THREAD SAFETY WARNING
 * ============================================================================
 *
 * FerricEngine instances are thread-affine. Each engine must be used
 * exclusively on the thread that created it. Do NOT pass a FerricEngine*
 * to another thread.
 *
 * Violation in debug builds: assertion failure (immediate abort).
 * Violation in release builds: FERRIC_ERROR_THREAD_VIOLATION return code.
 *
 * Multiple independent FerricEngine instances may be created and used on
 * different threads simultaneously (they share no state).
 *
 * The only exception is ferric_last_error_global(), which is thread-local
 * and safe to call from any thread.
 * ============================================================================
 */
```

### 11.3 FFI Panic Policy

**Rule: No Rust panic may unwind across the FFI boundary.** Unwinding from Rust into C is undefined behavior per the Rust reference, regardless of whether the C caller uses exception handling.

**Enforcement:** Ferric uses a profile matrix to keep Rust developer ergonomics while ensuring FFI artifacts never unwind across C boundaries:

```toml
# In workspace root Cargo.toml
[profile.dev]
panic = "unwind"

[profile.release]
panic = "unwind"

[profile.ffi-dev]
inherits = "dev"
panic = "abort"

[profile.ffi-release]
inherits = "release"
panic = "abort"
```

Build commands for FFI artifacts:

- Debug-style FFI build: `cargo build -p ferric-ffi --profile ffi-dev`
- Release FFI build: `cargo build -p ferric-ffi --profile ffi-release`

This means:
- Rust API development/tests keep unwind semantics (`dev`/`test`), preserving standard panic diagnostics.
- Shipped FFI artifacts (`ffi-dev`/`ffi-release`) abort on panic, so no unwind can cross `extern "C"`.

**Rationale for `panic = "abort"` over `catch_unwind`:** Wrapping every `extern "C"` entry point in `std::panic::catch_unwind` is error-prone (easy to forget on new entry points, interacts poorly with non-`UnwindSafe` types like `&mut Engine`). Profile-level enforcement is simpler and more reliable for published FFI artifacts.

**Implication for the Rust API:** The core Rust-facing crates continue using unwind semantics in normal development/test profiles. Rust callers who want panic boundaries can use `catch_unwind` at their own call sites.

**Testing ergonomics note:** `cargo test -p ferric-ffi` runs under `profile.test` (unwind). To verify abort-on-panic behavior of the produced FFI library, use subprocess tests that invoke binaries built with `--profile ffi-dev` or `--profile ffi-release`.

### 11.4 Error Handling Strategy

**Problem:** Engine creation can fail, but there's no engine instance to hold the error.

**Solution:** Two-tier error system:

1. **Thread-local error:** For failures before/during engine creation
2. **Per-engine error:** For failures during engine operations

```c
/* === Error Handling === */

/**
 * Error codes returned by Ferric functions.
 * All functions that can fail return FerricError.
 */
typedef enum {
    FERRIC_OK = 0,
    FERRIC_ERROR_INVALID_ARGUMENT = 1,
    FERRIC_ERROR_PARSE = 2,
    FERRIC_ERROR_RUNTIME = 3,
    FERRIC_ERROR_NOT_FOUND = 4,
    FERRIC_ERROR_OUT_OF_MEMORY = 5,
    FERRIC_ERROR_ENCODING = 6,
    FERRIC_ERROR_COMPILATION = 7,
    FERRIC_ERROR_THREAD_VIOLATION = 8,
    FERRIC_ERROR_BUFFER_TOO_SMALL = 9,
} FerricError;

/**
 * Get the last error message from thread-local storage.
 * Use this when a function returns an error but there's no engine
 * (e.g., engine creation failed).
 *
 * Returns NULL if no error. String is valid until next Ferric call on this thread.
 */
const char* ferric_last_error_global(void);

/**
 * Get the last error message for a specific engine.
 * Use this when an operation on an existing engine fails.
 *
 * Returns NULL if no error. String is valid until next operation on this engine.
 */
const char* ferric_engine_last_error(const FerricEngine* engine);

/**
 * Clear the thread-local error.
 */
void ferric_clear_error_global(void);

/**
 * Clear the per-engine error.
 */
void ferric_engine_clear_error(FerricEngine* engine);
```

#### 11.4.1 Copy-to-Buffer Error APIs

For language wrappers where the "valid until next call" lifetime of error strings is inconvenient, Ferric also provides copy-to-buffer variants. These coexist with the `const char*` functions above.

**Buffer sizing convention:**

The copy-to-buffer functions first check whether an error is present. If no error is available, the function returns `FERRIC_ERROR_NOT_FOUND` immediately, sets `*out_len` to `0` (if `out_len` is non-NULL), and does not inspect `buf` or `buf_len`. This check takes priority over all other logic below.

When an error **is** present, the `buf` and `buf_len` parameters control behavior:

- If `buf` is `NULL` **and** `buf_len` is `0`, the function writes the required buffer size (including the NUL terminator) to `*out_len` and returns `FERRIC_OK`. No data is copied. This is the "query size" path.
- If `buf` is non-NULL and `buf_len` is `0`, the function returns `FERRIC_ERROR_INVALID_ARGUMENT` (nothing can be written safely).
- If `buf` is non-NULL but `buf_len` is too small, the message is **truncated** to fit. Exactly `buf_len - 1` bytes of the message are copied, followed by a NUL byte at `buf[buf_len - 1]`. `*out_len` receives the **full** message length (excluding NUL). The function returns `FERRIC_ERROR_BUFFER_TOO_SMALL`.
- If `buf` is non-NULL and `buf_len` is sufficient, the full message is copied (NUL-terminated), `*out_len` receives the message length (excluding NUL), and the function returns `FERRIC_OK`.
- If `buf_len` is `1`, only the NUL terminator is written (zero message bytes).

**Detecting truncation (for wrapper authors):**

Define `required_len = *out_len + 1` (message length plus NUL). The message was truncated if and only if `required_len > buf_len`. This avoids off-by-one errors from comparing values with different NUL-inclusion conventions.

```c
// Example: detect and retry on truncation
char small_buf[64];
size_t msg_len = 0;
FerricError err = ferric_last_error_global_copy(small_buf, sizeof(small_buf), &msg_len);
if (err == FERRIC_ERROR_BUFFER_TOO_SMALL) {
    size_t required_len = msg_len + 1;  // +1 for NUL
    char* big_buf = malloc(required_len);
    ferric_last_error_global_copy(big_buf, required_len, NULL);
    // ... use big_buf ...
    free(big_buf);
}
```

This two-call pattern (query size, then copy) is standard in C APIs and avoids both over-allocation and silent truncation:

```c
// Alternative: query-then-allocate pattern
size_t needed = 0;
FerricError qerr = ferric_last_error_global_copy(NULL, 0, &needed);
if (qerr == FERRIC_OK && needed > 0) {
    char* buf = malloc(needed);    // needed includes NUL
    ferric_last_error_global_copy(buf, needed, NULL);  // exact fit
    // ... use buf ...
    free(buf);
} else {
    // No error present (qerr == FERRIC_ERROR_NOT_FOUND), or needed == 0
}
```

```c
/**
 * Copy the last global (thread-local) error message into a caller-provided buffer.
 *
 * @param buf       Caller-provided buffer, or NULL to query required size.
 * @param buf_len   Size of the buffer in bytes (including space for null terminator).
 *                  Pass 0 with buf=NULL to query required size.
 * @param out_len   Receives the actual length of the error message (excluding null terminator).
 *                  When buf=NULL and buf_len=0, receives the required buffer size
 *                  (including null terminator).
 *                  Set to 0 when no error is available.
 *                  May be NULL if the caller doesn't need the length.
 * @return FERRIC_ERROR_NOT_FOUND if no error is available (checked first, before
 *             inspecting buf/buf_len; *out_len set to 0),
 *         FERRIC_OK if an error message was copied or size was queried successfully,
 *         FERRIC_ERROR_BUFFER_TOO_SMALL if the buffer was too small (message truncated).
 */
FerricError ferric_last_error_global_copy(char* buf, size_t buf_len, size_t* out_len);

/**
 * Copy the last per-engine error message into a caller-provided buffer.
 *
 * @param engine    The engine whose error message to copy.
 * @param buf       Caller-provided buffer, or NULL to query required size.
 * @param buf_len   Size of the buffer in bytes (0 with buf=NULL to query size).
 * @param out_len   Receives the actual message length, or required size when querying.
 *                  Set to 0 when no error is available. May be NULL.
 * @return FERRIC_ERROR_NOT_FOUND if no error is available (checked first),
 *         FERRIC_OK if copied/queried successfully,
 *         FERRIC_ERROR_BUFFER_TOO_SMALL if truncated,
 *         FERRIC_ERROR_INVALID_ARGUMENT if engine is NULL.
 */
FerricError ferric_engine_last_error_copy(
    const FerricEngine* engine,
    char* buf,
    size_t buf_len,
    size_t* out_len
);
```

### 11.5 Unified Return Convention

**All functions that can fail return `FerricError`.** Output values are passed via pointers.

```c
/* === Engine Lifecycle === */

/**
 * Create a new engine with default configuration.
 *
 * @param out_engine  Pointer to receive the new engine. Set to NULL on failure.
 * @return FERRIC_OK on success, error code on failure.
 *         On failure, call ferric_last_error_global() for details.
 */
FerricError ferric_engine_new(FerricEngine** out_engine);

/**
 * Create a new engine with custom configuration.
 *
 * @param config      Configuration options. May be NULL for defaults.
 * @param out_engine  Pointer to receive the new engine.
 * @return FERRIC_OK on success, error code on failure.
 */
FerricError ferric_engine_new_with_config(
    const FerricConfig* config,
    FerricEngine** out_engine
);

/**
 * Destroy an engine and free all resources.
 * Safe to call with NULL (no-op).
 */
void ferric_engine_free(FerricEngine* engine);

/* === Execution === */

/**
 * Run rules until agenda is empty or limit reached.
 *
 * @param engine      The engine.
 * @param limit       Maximum rules to fire, or -1 for unlimited.
 * @param out_fired   Pointer to receive count of rules fired.
 * @return FERRIC_OK on success (including normal completion),
 *         error code on runtime error.
 */
FerricError ferric_run(
    FerricEngine* engine,
    int64_t limit,
    int64_t* out_fired
);

/**
 * Execute a single rule activation.
 *
 * @param engine     The engine.
 * @param out_status Receives: 1 if rule fired, 0 if agenda empty, -1 if halted.
 * @return FERRIC_OK on success, error code on failure.
 */
FerricError ferric_step(FerricEngine* engine, int* out_status);

/* === Facts === */

/**
 * Assert a fact from string representation.
 *
 * @param engine      The engine.
 * @param fact_str    Fact in CLIPS syntax, e.g., "(person (name \"Alice\"))".
 * @param out_fact_id Pointer to receive the new fact's ID (may be NULL if not needed).
 * @return FERRIC_OK on success, error code on failure.
 */
FerricError ferric_assert_string(
    FerricEngine* engine,
    const char* fact_str,
    FerricFactId* out_fact_id
);

/**
 * Retract a fact by ID.
 *
 * @param engine   The engine.
 * @param fact_id  ID of the fact to retract.
 * @return FERRIC_OK on success, FERRIC_ERROR_NOT_FOUND if fact doesn't exist.
 */
FerricError ferric_retract(FerricEngine* engine, FerricFactId fact_id);
```

### 11.6 Ownership Model

Clear documentation of ownership for all pointer-returning functions:

```c
/*
 * OWNERSHIP MODEL
 * ===============
 *
 * Engine-owned (caller must NOT free):
 * - Strings returned by ferric_last_error_global(), ferric_engine_last_error()
 *   Valid until: next Ferric call on same thread/engine
 *
 * - Fact data accessed via ferric_get_fact_*() functions
 *   Valid until: fact is retracted or engine is freed
 *
 * Caller-owned (caller MUST free):
 * - FerricEngine* from ferric_engine_new() → free with ferric_engine_free()
 *
 * - Strings from ferric_value_to_string() → free with ferric_string_free()
 *
 * - FerricValueArray from ferric_get_multifield() → free with ferric_value_array_free()
 *
 * Copy semantics (no ownership transfer):
 * - ferric_last_error_global_copy() copies into caller-provided buffer
 * - ferric_engine_last_error_copy() copies into caller-provided buffer
 * - ferric_get_global_string() copies into caller-provided buffer
 * - ferric_call() copies result into caller-provided buffer
 */

/**
 * Free a string allocated by Ferric.
 */
void ferric_string_free(char* str);

/**
 * Free a value array allocated by Ferric.
 */
void ferric_value_array_free(FerricValueArray* array);
```

### 11.7 Rust Implementation

```rust
// ferric-ffi/src/error.rs

use std::cell::RefCell;
use std::ffi::CString;

thread_local! {
    /// Thread-local error message for failures without an engine context
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

pub fn set_global_error(msg: &str) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
}

pub fn get_global_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        e.borrow().as_ref().map_or(ptr::null(), |s| s.as_ptr())
    })
}

pub fn clear_global_error() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
}

/// Copy the global error into a caller-provided buffer.
/// Returns NOT_FOUND (with *out_len = 0) if no error is present — checked first,
/// before inspecting buf/buf_len. See §11.4.1 for full semantics.
pub fn copy_global_error(buf: *mut c_char, buf_len: usize, out_len: *mut usize) -> FerricError {
    LAST_ERROR.with(|e| {
        let borrow = e.borrow();
        match borrow.as_ref() {
            None => {
                // No error present — return NOT_FOUND before inspecting buf/buf_len.
                if !out_len.is_null() {
                    unsafe { *out_len = 0; }
                }
                FerricError::NotFound
            }
            Some(cstr) => {
                let bytes = cstr.as_bytes(); // without null terminator
                // Size-query path: buf=NULL, buf_len=0
                if buf.is_null() && buf_len == 0 {
                    if !out_len.is_null() {
                        unsafe { *out_len = bytes.len() + 1; } // includes NUL
                    }
                    return FerricError::Ok;
                }
                if buf.is_null() {
                    return FerricError::InvalidArgument;
                }
                if buf_len == 0 {
                    return FerricError::InvalidArgument;
                }
                let copy_len = bytes.len().min(buf_len - 1);
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, copy_len);
                    *buf.add(copy_len) = 0; // null terminate
                    if !out_len.is_null() {
                        *out_len = bytes.len();
                    }
                }
                if bytes.len() + 1 > buf_len {
                    FerricError::BufferTooSmall
                } else {
                    FerricError::Ok
                }
            }
        }
    })
}

// ferric-ffi/src/engine.rs

#[repr(C)]
pub struct FerricEngine {
    engine: Engine,
    last_error: Option<CString>,
}

#[no_mangle]
pub extern "C" fn ferric_engine_new(out_engine: *mut *mut FerricEngine) -> FerricError {
    if out_engine.is_null() {
        set_global_error("out_engine pointer is null");
        return FerricError::InvalidArgument;
    }

    match Engine::new(EngineConfig::default()) {
        Ok(engine) => {
            let boxed = Box::new(FerricEngine {
                engine,
                last_error: None,
            });
            unsafe { *out_engine = Box::into_raw(boxed) };
            FerricError::Ok
        }
        Err(e) => {
            unsafe { *out_engine = ptr::null_mut() };
            set_global_error(&e.to_string());
            FerricError::Runtime
        }
    }
}

#[no_mangle]
pub extern "C" fn ferric_run(
    engine: *mut FerricEngine,
    limit: i64,
    out_fired: *mut i64,
) -> FerricError {
    // Step 1: shared borrow for thread check (no mutation yet).
    let shared = match unsafe { engine.as_ref() } {
        Some(e) => e,
        None => {
            set_global_error("engine pointer is null");
            return FerricError::InvalidArgument;
        }
    };
    if let Err(e) = shared.engine.check_thread() {
        set_global_error(&e.to_string());
        return FerricError::ThreadViolation;
    }

    // Step 2: mutable borrow only after check passes.
    let engine = unsafe { &mut *engine };

    let run_limit = if limit < 0 {
        RunLimit::Unlimited
    } else {
        RunLimit::Rules(limit as usize)
    };

    match engine.engine.run(run_limit) {
        Ok(result) => {
            if !out_fired.is_null() {
                unsafe { *out_fired = result.rules_fired as i64 };
            }
            FerricError::Ok
        }
        Err(e) => {
            engine.last_error = CString::new(e.to_string()).ok();
            FerricError::Runtime
        }
    }
}

#[no_mangle]
pub extern "C" fn ferric_engine_last_error(engine: *const FerricEngine) -> *const c_char {
    match unsafe { engine.as_ref() } {
        Some(e) => e.last_error.as_ref().map_or(ptr::null(), |s| s.as_ptr()),
        None => ptr::null(),
    }
}
```

### 11.8 Build Profiles and Platform Artifacts

```toml
# crates/ferric-ffi/Cargo.toml

[lib]
crate-type = ["cdylib", "staticlib"]

[target.'cfg(windows)'.dependencies]
# Windows-specific deps if needed

[target.'cfg(target_os = "android")'.dependencies]
# Android-specific deps if needed
```

FFI panic behavior is controlled by workspace-level `ffi-dev` / `ffi-release` profiles (see §11.3).

Build artifacts:
- `libferric.so` (Linux)
- `libferric.dylib` (macOS)
- `ferric.dll` + `ferric.lib` (Windows)
- `libferric.a` (all platforms, static)

---

## 12. CLI and REPL

Ferric provides a CLI for batch execution and an interactive REPL for debugging rule sets.

### 12.1 CLI Goals

- Execute `.clp` files non-interactively with predictable exit codes
- Support embedding-style diagnostics (parse/compile/runtime errors with spans)
- Provide machine-friendly output modes for CI

### 12.2 CLI Commands

| Command | Purpose | Exit Behavior |
|---------|---------|---------------|
| `ferric run <file.clp>` | Load and run until agenda empty (or error) | `0` on success, non-zero on load/runtime error |
| `ferric check <file.clp>` | Parse/compile validation only (no execution) | `0` if valid, non-zero if diagnostics emitted |
| `ferric repl` | Start interactive session | Non-zero only on initialization failure |
| `ferric version` | Print version/build metadata | Always `0` unless IO failure |

### 12.3 REPL Requirements

- Multi-line input with balanced-paren continuation
- Commands: `reset`, `run [n]`, `facts`, `agenda`, `clear`, `exit`
- Source spans in error output when evaluating entered forms
- Command history and line editing via `rustyline`

### 12.4 Non-Goals (v1)

- Networked/distributed REPL
- Time-travel debugging
- Deterministic replay tooling

---

## 13. Testing Strategy

Testing is layered to catch logic errors early, especially around retraction and negation where bugs are subtle and expensive later.

### 13.1 Test Layers

1. Unit tests for values, parser primitives, symbol table, agenda ordering, and index maintenance
2. Integration tests for load/assert/retract/run behavior across multiple constructs
3. Regression tests for previously fixed bugs (append-only suite)
4. CLIPS compatibility tests for supported subset semantics
5. Property-style tests for invariants (idempotent cleanup, no stale IDs)

### 13.2 Required Invariant Suites

- Retraction invariant suite from §15.0 (must pass before Phase 2 exits)
- Negative/NCC/exists cleanup idempotence suite
- Forall vacuous-truth cycle suite (`forall_vacuous_truth_and_retraction_cycle`)
- FFI error-API contract suite (including copy-to-buffer edge cases)

### 13.3 CI Gates

- `cargo test --workspace` for unit/integration/regression suites
- Dedicated CLIPS compatibility job
- Benchmark smoke job (non-blocking early, blocking in Phase 6)
- FFI subprocess tests for abort-policy verification in `ffi-*` profiles

### 13.4 Test Philosophy

- Add a regression test for every bug fixed in core matching/retraction logic
- Prefer deterministic assertions on state/invariants over brittle timing assertions
- Keep fixture rule sets small and targeted to isolate semantic failures quickly

---

## 14. Performance Considerations

### 14.1 Performance Goals

Based on project requirements, Ferric should perform within 10x of CLIPS:

| Metric | CLIPS Baseline | Ferric Target |
|--------|----------------|---------------|
| Rules fired/sec | 100,000 | ≥10,000 |
| Fact assertions/sec | 500,000 | ≥50,000 |
| Fact retractions/sec | 200,000 | ≥20,000 |
| Waltz benchmark | 1.0s | ≤10s |
| Manners 64 | 0.5s | ≤5s |

### 14.2 Retraction-Specific Optimizations

Given the retraction-first design principle:

1. **Reverse indices:** `FactId → TokenIds` mapping ensures O(affected) not O(total) retraction
2. **Parent→children index:** `TokenStore.parent_to_children` enables O(subtree) cascading deletes without global scans
3. **Owner node tracking:** Each token stores its `owner_node`, enabling O(1) removal from the owning beta memory without scanning all memories
4. **Token→activation index:** `Agenda.token_to_activations` enables O(k log n) activation cleanup per token (k≈1 typically) via `BTreeMap::remove`, without scanning the full agenda
5. **Stable token IDs:** SlotMap provides O(1) token lookup and removal
6. **Blocker tracking:** Negative nodes maintain explicit blocker relationships for O(1) updates
7. **Eager empty-entry pruning:** Empty index entries (e.g., in `fact_to_tokens`, `blocked_by`) are pruned on removal in the structures already touched during retraction. Avoid periodic sweeps unless profiling shows benefit
8. **SmallVec-backed indices:** `SmallVec<[TokenId; 4]>` avoids heap allocation for low-fanout entries (the common case), reducing memory pressure and improving cache locality

### 14.3 Join Optimization

1. **VarId indexing:** O(1) variable lookup during joins (vs O(n) linear scan)
2. **Alpha memory indexing:** Hash-based lookup via `AtomKey` when joining on equality
3. **Right-unlinking:** Empty alpha memories are skipped during left activation
4. **Reference-counted values:** `Rc<Value>` minimizes cloning during token propagation; no atomic overhead (Engine is `!Send` by design, see §2.1)
5. **Zero-clone token insert:** `TokenStore::insert` takes tokens by value; only the small `facts` slice is cloned for index updates, not the full token

### 14.4 Hot-Path Budget

In addition to big-O complexity, the following operational targets constrain the constant factors in the engine's inner loop. These targets exist to keep future refactors honest — any change to an operation listed here must not violate the budget or must justify the regression.

| Operation | Time Budget | Allocation Budget | Notes |
|-----------|-------------|-------------------|-------|
| **Agenda insert** | O(log A) | No incidental allocations beyond container-internal node allocation | A = agenda size. `BTreeMap::insert` + `HashMap::insert` + `SmallVec::push`. SmallVec inline capacity (2) avoids alloc for typical fanout. `AgendaKey` is `Clone` but contains no heap data (all `Copy` fields + `SmallVec` that clones inline). `BTreeMap` may allocate a B-tree node internally; this is unavoidable container growth, not incidental. |
| **Agenda pop** | O(log A) | No incidental allocations | `BTreeMap::pop_first` returns `(AgendaKey, ActivationId)` directly — no reverse lookup needed. |
| **Agenda remove (retraction)** | O(log A) per activation | No incidental allocations | `id_to_key` lookup → `BTreeMap::remove`. |
| **Token insert** | O(k) | One SlotMap insert (amortized O(1)), k SmallVec pushes | k = number of facts in token (typically ≤4). SmallVec pushes are inline for k ≤ 4. |
| **Token cascade removal** | O(subtree) | No incidental allocations per node | `SlotMap::remove` is O(1). Reverse-index cleanup via `retain()` is in-place. |
| **Alpha memory lookup** | O(1) amortized | None | `HashMap::get` on `AtomKey`. |
| **Thread-ID check (FFI)** | One TLS read | None | `std::thread::current().id()` — a cheap platform TLS access. |

**"No incidental allocations" means:** No `Vec::clone()`, no `format!()`, no `HashMap` full-scan, and no application-level heap allocation on the common path. Container-internal allocations (e.g., `BTreeMap` node splits on insert, `HashMap` resizing on growth) are unavoidable and acceptable — these are amortized O(1) and cannot be eliminated without replacing the container. If truly zero-allocation agenda operations are needed later, the `Agenda` struct encapsulates the ordering strategy and can be backed by a pre-sized slab + index structure without API changes.

**Key anti-patterns to avoid (for implementors):**

- No `Vec::clone()` or `BTreeMap::clone()` in any code path that runs per-activation or per-token.
- No `AgendaKey` construction that allocates (all fields must be `Copy` or inline-`SmallVec`).
- No `format!()` or string allocation on the success path of any engine operation (only on error paths).
- No `HashMap::iter()` full-scan in any per-fact or per-token operation (that's what the reverse indices are for).

---

## 15. Implementation Phases

### 15.0 Pre-Implementation Checklist

Before writing implementation code, the following design decisions must be locked down and verified against this plan. Each item is a "cheap to fix now, expensive to fix later" edge case.

| # | Decision | Status | Plan Reference |
|---|----------|--------|----------------|
| 1 | **AgendaKey definition per strategy** — salience, then strategy-specific fields, then `activation_seq` tiebreak (not ActivationId). All four strategies (Depth/Breadth/LEX/MEA) have defined ordering. | ✅ Defined | §6.6.1 |
| 2 | **Classic-mode unsupported constructs fail compilation** — never silently dropped or compiled inert. Both modes return `CompileError`. | ✅ Defined | §2.3, §7.6 |
| 3 | **Engine is `!Send + !Sync`** — enforced via `PhantomData<Rc<()>>`. `unsafe Send` escape hatch documented for advanced use. Runtime `creator_thread_id` check on every FFI entry point. | ✅ Defined | §2.1, §11.2 |
| 4 | **Forall vacuous truth** — `forall` is satisfied when `<condition>` matches zero facts. 6-step regression test specified. | ✅ Defined | §7.5 |
| 5 | **SmallVec deletion method** — `retain()` (linear scan + compaction). No-duplicates invariant enforced by `debug_assert!`. | ✅ Defined | §5.5.1 |
| 6 | **FFI buffer sizing convention** — `buf=NULL, buf_len=0` queries size; too-small buffers truncate exactly `buf_len-1` bytes + NUL, return `BUFFER_TOO_SMALL`. Truncation check: `required_len = *out_len + 1 > buf_len`. | ✅ Defined | §11.4.1 |
| 7 | **SlotMap key types** — `FactId`, `TokenId`, `ActivationId` all use `slotmap::new_key_type!`. | ✅ Defined | §5.2, §5.5, §6.6.1 |
| 8 | **AtomKey float semantics** — `f64::to_bits()`, so `-0.0 ≠ +0.0` and NaN bit patterns are distinct. | ✅ Defined | §5.1.1 |
| 9 | **PatternValidationError contract** — stable error codes (`E0001`–`E0005`), `SourceSpan` always attached via `SpannedPattern`. | ✅ Defined | §5.6, §7.7 |
| 10 | **String comparison** — exact byte equality, lexicographic byte ordering, no normalization. | ✅ Defined | §2.4.1 |
| 11 | **Token fact dedup for index maintenance** — `TokenStore::insert` deduplicates `fact_to_tokens` entries when `token.facts` contains repeated FactIds. | ✅ Defined | §5.5.1 |
| 12 | **Token retraction propagation invariant** — every node storing TokenIds must receive cleanup callbacks for all cascaded removals. | ✅ Defined | §7.2 |
| 13 | **Hot-path budget** — agenda insert/pop/remove are O(log A) with no incidental heap allocations (container-internal node allocation is acceptable). | ✅ Defined | §14.4 |
| 14 | **Cascade callback ordering** — order-independent; all cleanup callbacks are idempotent and tolerant of missing entries. No dependence on dispatch sequence. | ✅ Defined | §7.2 |
| 15 | **Thread-transfer API** — official `unsafe fn move_to_current_thread(&mut self)` handles `creator_thread_id` update and error state reset. Ad-hoc `SendEngine` wrapper only needed for the type-system move, not for state management. | ✅ Defined | §2.1 |
| 16 | **FFI thread-check ABI contract** — `check_thread()` runs before any state mutation in every `ferric_engine_*` entry point. `FERRIC_ERROR_THREAD_VIOLATION` guarantees no state was modified. | ✅ Defined | §11.2 |
| 17 | **Error code policy** — `PatternValidationError` codes are append-only, never renumbered or reused. New validation rules get new codes. | ✅ Defined | §7.7 |
| 18 | **Debug consistency checker** — `debug_assert_consistency()` verifies cross-structure TokenId/ActivationId integrity. Available in tests and debug builds. | ✅ Defined | §7.2 |
| 19 | **FFI panic policy matrix** — workspace `dev/release/test` use unwind ergonomics; shipped FFI artifacts use dedicated `ffi-dev`/`ffi-release` profiles with `panic = "abort"`. | ✅ Defined | §11.3 |
| 20 | **Copy-to-buffer "no error" semantics** — `FERRIC_ERROR_NOT_FOUND` returned (with `*out_len = 0`) before inspecting `buf`/`buf_len`. Size-query path (`buf=NULL, buf_len=0` → `FERRIC_OK`) only applies when an error is present. | ✅ Defined | §11.4.1 |
| 21 | **LEX/MEA recency vector length** — fixed per rule (equal to positive pattern count), determined at compile time. Ordering comparison never depends on vector length. | ✅ Defined | §6.6.1 |
| 22 | **`remove_cascade` precondition** — `root_id` must exist in TokenStore. `debug_assert!` in development; defensive early return (empty `Vec`) in release. Double-remove does not corrupt indices. | ✅ Defined | §5.5.1 |
| 23 | **FFI canonical entry-point pattern** — every `extern "C"` entry point casts to `&Engine` (shared) for `check_thread()`, then to `&mut Engine` only after the check passes. No mutable borrow exists during the thread-ID check. | ✅ Defined | §11.2 |
| 24 | **Retraction root selection** — if multiple affected tokens are ancestor/descendant, only roots are cascaded to avoid double-remove behavior divergence between debug/release. | ✅ Defined | §5.5.1, §6.8 |
| 25 | **Retraction activation delta** — activations created during negative/existential re-satisfaction are enqueued and returned in retraction results (not dropped). | ✅ Defined | §6.8 |
| 26 | **Standalone plan rule** — no normative section can depend on “previous version” text for implementation scope. | ✅ Defined | §10.2, §12, §13, Appendix A |
| 27 | **Activation ordering contract** — total order within run, no replay-identical cross-run guarantee unless encoded by explicit rule precedence. | ✅ Defined | §6.6.2, §16.6 |

**Go / no-go gate for implementation:**

The following items are **must-haves** before writing core logic beyond basic project scaffolding. They are the minimum conditions under which implementation can proceed without accumulating avoidable technical debt:

1. ✅ The retraction invariants test suite skeleton exists (even if not all tests are passing yet) and the harness can create small networks and exercise assert/retract. (See test suite spec below.)
2. ✅ Cascade callback idempotence / ordering policy is decided: order-independent, all callbacks idempotent and tolerant of missing entries. (§7.2)
3. ✅ Thread-affinity story is complete: compile-time `!Send + !Sync` enforcement, runtime `check_thread()` on every entry point, and the official `unsafe fn move_to_current_thread()` escape hatch. (§2.1, §11.2)
4. ✅ FFI panic policy matrix is locked: default Rust dev/test profiles use unwind, while shipped FFI artifacts are built with `ffi-*` abort profiles so no Rust unwind crosses the FFI boundary. (§11.3)

**Retraction invariants test suite (implement in Phase 1):**

Before any feature work beyond basic assert/retract, the following invariants must have automated tests:

1. After retraction, no `TokenId` in any reverse index (`fact_to_tokens`, `parent_to_children`, `token_to_activations`) references a token that has been removed from the `TokenStore`.
2. After retraction, no `ActivationId` in the `BTreeMap` ordering references an activation that has been removed from the `SlotMap`.
3. After retraction, no beta memory contains a `TokenId` that has been removed from the `TokenStore`.
4. After retract-all (clearing all facts), all reverse indices, beta memories, and the agenda are empty. This includes negative/NCC/exists memory side-indices (`blockers`, `blocked_by`, `owner_to_results`, `support_count`, etc.).
5. The `remove_cascade` function removes exactly the subtree rooted at the target token — no more, no less.
6. `debug_assert!` checks for the no-duplicates invariant fire correctly when a duplicate insertion is attempted (test via `#[should_panic]`).
7. When a token whose `facts` list contains the same FactId twice is retracted, the `fact_to_tokens` reverse index contains no stale entries (dedup correctness).
8. Negative memory `blocked_by` map contains no empty sets after token retraction (prune-on-empty invariant).
9. `debug_assert_consistency()` passes after every assert and retract operation in all retraction invariant tests (structural regression check).

Phase 1 baseline status: consistency checks now cover token, alpha, beta, and
agenda internals, plus rete-level cross-structure integrity checks exercised in
retraction-oriented tests. Negative/NCC/exists-specific invariants remain
Phase 2+ as those structures are introduced.
Phase 3 extension status: `debug_assert_consistency()` also checks module/focus
registry integrity, function/global/generic registries, and rule/template
module mappings.

### Phase 1: Foundation (Weeks 1-10)

**Goal:** Minimal working engine with basic rules AND minimal parser

| Week | Deliverables |
|------|--------------|
| 1-2 | Project setup, crate structure, CI pipeline |
| 3-4 | Value types, symbol interning (encoding-aware), basic fact representation |
| 5-6 | S-expression lexer and parser (Stage 1) — including a minimal source loader capable of parsing `.clp` files into S-expression trees, sufficient for integration testing before the full grammar lands |
| 7-8 | Alpha network (type tests, constant tests) |
| 9-10 | Beta network (simple joins), token storage with reverse indices and parent→children index |

**Exit Criteria:**
- Can parse basic `.clp` files into S-expressions
- Minimal source loader supports `(assert ...)` and `(defrule ...)` at the S-expression level
- Can assert facts
- Can retain minimal rule definitions and demonstrate simple pattern
  propagation via programmatic alpha/beta/agenda network construction
- Alpha-beta propagation works
- Unit tests pass

Automatic compilation from parsed rule definitions into rete networks is
explicitly Phase 2 scope.

### Phase 2: Core Engine (Weeks 11-20)

**Goal:** Complete Rete implementation with retraction support

| Week | Deliverables |
|------|--------------|
| 11-12 | Construct interpreter (Stage 2): deftemplate, defrule basics |
| 13-14 | Negative nodes with blocker tracking |
| 15-16 | Agenda, conflict resolution strategies |
| 17-18 | Rule firing, action execution, fact modification |
| 19-20 | NCC nodes for `(not (and ...))`, exists nodes; compile-time pattern validation (Section 7.7) |

**Exit Criteria:**
- Can load `.clp` files with deftemplate, defrule, deffacts
- Retraction works correctly with proper token cleanup (cascading deletes via parent→children)
- Negative patterns work (single and conjunction)
- Pattern restriction violations are caught at compile time with source-located errors
- Integration tests pass with real CLIPS files

Carry-forward baseline for remaining phases:
- Parser Stage 2 produces typed constructs; runtime owns translation into
  parser-agnostic core compile models.
- NCC conjunction negation semantics are complete and are the baseline for
  future `forall` work.
- `exists` uses dedicated support-counting memory as the canonical
  implementation.
- Unsupported constructs must fail with explicit diagnostics (never silently
  dropped).
- Core compiler entry points are the authoritative validation gate.
- RHS actions are live with a narrowed subset (`assert`/`retract`/`halt` fully
  operational; template-aware `modify`/`duplicate` and `printout` deferred).

### Phase 3: Language Completion (Weeks 21-26)

**Goal:** Complete language/runtime semantics deferred from Phase 2 and land the remaining supported construct set.

| Week | Deliverables |
|------|--------------|
| 21-22 | Runtime carryover closure: template-aware `modify`/`duplicate`, non-placeholder `printout`, function-call evaluation path for RHS/test expressions |
| 23-24 | `deffunction`, `defglobal`; function environment wiring for user-defined calls |
| 25-26 | `defmodule` import/export, `defgeneric`/`defmethod`, `forall` (limited) built on existing NCC/exists semantics and vacuous-truth contract |

**Exit Criteria:**
- Phase 2 carryover action semantics are complete (`modify`/`duplicate` template-aware, `printout` implemented)
- `forall` limited subset is implemented with regression coverage (including vacuous-truth + retraction cycle)
- Remaining supported constructs (`deffunction`, `defglobal`, `defmodule`, `defgeneric`, `defmethod`) are implemented
- Good error messages with source locations
- Unsupported constructs rejected with stable, source-located diagnostics (no silent degradation)

**Phase 3 remediation closure (2026-02-19):**
- R1 resolved: forall vacuous-truth/retraction-cycle regression contract is fully enforced by active 6-step integration tests.
- R2 resolved: RHS `focus` now emits explicit diagnostics for unknown modules (no silent drops).
- R3 resolved: duplicate-definition checks with source-located diagnostics cover `defglobal`, `defmodule`, `defgeneric`, and duplicate explicit `defmethod` indices.
- R4 resolved: evaluator carries source spans for variable/global references and surfaces them in unbound-variable/global diagnostics.
- R5 explicitly deferred to Phase 4: enforce module visibility for cross-module `deffunction` and `defglobal` resolution paths, including module-qualified `MODULE::name` references.
- R6 resolved: public focus APIs (`set_focus`, `get_focus`, `get_focus_stack`) match runtime behavior; `run` preserves a baseline focus frame across calls and `reset` restores `[MAIN]`.
- R8 resolved: `printout` channel remains literal-only (`symbol`/`string`) with diagnostics for non-literal forms.
- R9 resolved: consistency checks include Phase 3 registries/mappings (module/focus, function/global/generic, rule/template module links).
- Phase 3 post-review carryover: generic dispatch remains index-order deterministic with registration-order auto-indexing; Phase 4 finalizes CLIPS specificity ranking and `call-next-method`.
- Phase 3 post-review carryover: same-name `deffunction`/`defgeneric` currently uses precedence behavior; Phase 4 replaces this with explicit definition-time conflict diagnostics.

### Phase 4: Standard Library (Weeks 27-32)

**Goal:** Fill out built-in function breadth and close remaining language-compatibility carryovers discovered during Phase 3.

| Week | Deliverables |
|------|--------------|
| 27-28 | Module-resolution completion: enforce `defmodule` import/export visibility for cross-module `deffunction` calls and `defglobal` reads/writes, and add module-qualified `MODULE::name` resolution with source-located diagnostics |
| 29-30 | Generic-dispatch compatibility closure: CLIPS-style specificity ranking, `call-next-method`, and finalized definition-time conflict diagnostics for same-name `deffunction`/`defgeneric` |
| 31-32 | Standard-library breadth: predicate/math/string/symbol/multifield and I/O/environment/fact/agenda surfaces (`format`, `read`, `readline`, focus query functions), including full `printout` behavior validation |

**Exit Criteria:**
- All documented functions implemented
- Callable/global registries are keyed by `(ModuleId, local-name)` so identical local names can coexist across modules without clobbering
- Module-qualified and cross-module callable/global resolution paths honor import/export visibility with source-located diagnostics (`?*MODULE::name*` canonical form for qualified globals)
- Unqualified resolution is caller-module-first, then visible imports; ambiguous multiple-visible matches emit explicit diagnostics (no arbitrary fallback)
- Generic dispatch behavior matches documented specificity/`call-next-method` contract
- Same-name `deffunction`/`defgeneric` definitions fail with explicit conflict diagnostics
- Function tests pass through both direct calls and RHS expression execution paths
- Can run standard CLIPS examples

### Phase 5: FFI & CLI (Weeks 33-38)

**Goal:** External interfaces built on Phase 4's finalized module/global resolution and diagnostic contracts.

| Week | Deliverables |
|------|--------------|
| 33-34 | C FFI core API with unified error handling, including copy-to-buffer error APIs |
| 35-36 | FFI extended API, header generation (with thread safety warning block), ownership documentation |
| 37-38 | CLI, REPL |

**Exit Criteria:**
- C programs can embed Ferric
- Error handling works correctly (thread-local + per-engine + copy-to-buffer variants)
- Thread safety contract is documented prominently in the generated C header
- Validation and action-execution diagnostics are surfaced consistently through FFI and CLI
- Phase 4 module/generic diagnostics (visibility, module-qualified names, dispatch/conflict errors) are surfaced through FFI and CLI without loss of source context
- Phase 4 module/global ambiguity and visibility diagnostics are treated as stable external contract (no CLI/FFI-layer reinterpretation)
- CLI runs on all platforms
- REPL is functional

### Phase 6: Polish (Weeks 39-44)

**Goal:** Production readiness

| Week | Deliverables |
|------|--------------|
| 39-40 | CLIPS compatibility test suite |
| 41-42 | Performance optimization, benchmarking |
| 43-44 | Documentation, examples, compatibility doc (including string comparison semantics examples) |

**Exit Criteria:**
- CLIPS compatibility tests pass
- Performance within target range
- Documentation complete (including string comparison semantics, pattern restriction rationale, canonical `?*MODULE::name*` global syntax, and `bind` non-creation semantics)
- Ready for release

Phase 4 follow-through requirements for subsequent phases:
- Phase 5 surface design must keep Phase 4 module/visibility/ambiguity diagnostics intact and source-located.
- Phase 6 compatibility documentation must reflect canonical qualified global syntax and bind write semantics exactly as implemented.
- Regression coverage for module namespace collisions and qualified-global paths remains mandatory for future refactors.

### Timeline Summary

| Phase | Duration | Cumulative |
|-------|----------|------------|
| Foundation | 10 weeks | 10 weeks |
| Core Engine | 10 weeks | 20 weeks |
| Language Completion | 6 weeks | 26 weeks |
| Standard Library | 6 weeks | 32 weeks |
| FFI & CLI | 6 weeks | 38 weeks |
| Polish | 6 weeks | 44 weeks |

**Total: ~11 months** for 1-2 developers

---

## 16. Compatibility Documentation

Ferric will maintain a living document (`docs/compatibility.md`) detailing:

### 16.1 Supported Constructs

Full list of supported CLIPS constructs with notes on any behavioral differences.

### 16.2 Unsupported Features

Each unsupported feature will be documented with:
- Feature name and CLIPS documentation reference
- Reason for exclusion
- Workarounds (if any)
- Whether future support is planned

### 16.3 Pattern Nesting Restrictions

Documented restrictions on negation/existential nesting with:
- Clear examples of what is/isn't supported
- Refactoring suggestions for unsupported patterns
- Rationale for restrictions

### 16.4 String and Symbol Comparison Semantics

Documented comparison behavior with:
- Precise definitions of equality and ordering per encoding mode
- Examples demonstrating byte-equality behavior (e.g., NFC vs. NFD representations of the same character are NOT equal)
- Explicit statement of what is not supported (normalization, collation, case-insensitive comparison)
- Guidance for users who need normalization-aware behavior (normalize inputs before asserting)
- Comparison with CLIPS behavior for ASCII content (identical)

### 16.5 Migration Guide

Step-by-step guide for migrating CLIPS applications to Ferric.

### 16.6 Activation Ordering Contract

Documentation must explicitly state:

- Ferric guarantees total ordering of activations at runtime but does not guarantee cross-run replay-identical order.
- Semantic compatibility expectations should focus on final working-memory outcomes for order-insensitive rule sets.
- For order-sensitive side effects, users must encode precedence explicitly (salience, focus, phase facts).

---

## Appendix A: Dropped Features

The following features are explicitly out of scope for the current implementation plan and are not assumed by any phase exit criteria:

1. COOL object system
2. Certainty factors / probabilistic reasoning
3. Distributed or networked rule evaluation
4. Replay-identical deterministic scheduling guarantees across runs/platforms
5. Conflict strategies `Simplicity`, `Complexity`, and `Random` (deferred until fully specified)

Any future proposal to add one of these features must include:
- Semantics specification
- Data-structure impact analysis (especially retraction paths)
- Migration/compatibility impact
- Required regression tests

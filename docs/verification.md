# Formal verification

ferric-rules uses two verifiers, starting from the Rete core and working
outward.

- **[Kani](https://model-checking.github.io/kani/)** is a bounded model
  checker that runs on the production Rust code. A Kani harness proves a
  property for *every* input within its stated bounds, such as string length,
  number of operations or loop iterations. It also checks arithmetic overflow,
  out-of-bounds access, pointer validity in `unsafe` code, and panics.
- **[Verus](https://github.com/verus-lang/verus)** is an SMT-based deductive
  verifier. Verus proofs here are *models*: transcriptions of production
  algorithms, written function by function in Verus syntax, with unbounded
  theorems about them. They hold for any number of facts, tokens, operations
  or input lengths.

The two meet in **refinement harnesses**. A refinement harness is a Kani
harness that checks a plain-Rust copy of a Verus model's executable code
against the production code on bounded inputs. Where one exists, the
unbounded theorem is tied to the shipped code, not only to the model.

## Running the proofs

Install the pinned tools with `scripts/install-verification-tools.sh`. It
installs Kani and downloads a Verus release; each tool pins its own Rust
toolchain, independent of `rust-toolchain.toml`. Then:

| Command | What it runs |
| --- | --- |
| `just verify` | Every Kani harness and every Verus proof |
| `just verify-kani [crate]` | All Kani harnesses, or one crate's |
| `just verify-kani-harness <crate> <harness>` | One Kani harness, e.g. `just verify-kani-harness ferric-rules-core ordered_set::kani_refinement::promotion_refines_spec` |
| `just verify-verus` | Every file under `verification/verus/` |

Verus checks every file in a few seconds. Kani harnesses range from under a
second to roughly 20 minutes, and the heaviest need about 7 GB of memory. Run
them one at a time on small machines: `cargo kani --harness <path> --exact`
under `ulimit -v`.

Ordinary builds never compile proof code. Harness modules are gated by
`#[cfg(kani)]`, and the workspace declares `cfg(kani)` as an expected
configuration, so clippy and rustc stay warning-free.

## Layout and conventions

- `src/<module>/kani_proofs.rs` holds Kani harnesses for `<module>`. Each is
  declared at the end of the production file as `#[cfg(kani)] mod
  kani_proofs;`. As a child module it can reach private items, so no
  production visibility changes for verification.
- `src/<module>/kani_refinement.rs` holds refinement harnesses linking a
  module to its Verus model.
- `verification/verus/<name>.rs` holds standalone Verus files (`use
  vstd::prelude::*; verus! { ... }`). Each is checked with `verus
  --crate-type=lib <file>`.

Every proof file opens with a module comment. It lists what is proved, the
bounds (Kani) or trusted base (Verus), and known gaps. Treat that comment as
the authoritative description of the file. The rules all proof files follow:

- **Independent oracles.** Expected results come from documentation:
  [`compatibility.md`](compatibility.md), module and function docs, and CLIPS
  6.30 behaviour. They are never copied from the implementation.
- **Non-vacuity.** Every harness that constrains its inputs (`kani::assume`,
  `kani::any_where`) also has `kani::cover!` statements, which Kani must
  report as satisfied, showing its interesting paths are reachable. Verus
  files carry concrete witness lemmas for the same purpose.
- **Fidelity.** Every Verus model function cites the production function it
  follows (`file.rs:LINE`) and keeps its branch structure. The file's header
  documents every abstraction. Reviewers checked fidelity line by line and
  ran mutation tests: they planted plausible bugs in production code or model
  code and confirmed a proof failed.
- **No proof shortcuts.** Verus files use no `assume`, `admit`,
  `external_body` or `assume_specification` for anything with proof weight.
  The single trusted wrapper is marked `// TRUSTED:` (see
  [Trusted base](#trusted-base)).

## What is verified

### Core (`ferric-rules-core`)

| Area | Kani, on production code (bounded) | Verus, on a model (unbounded) |
| --- | --- | --- |
| Values, strings, symbols, encodings | `Value`/`AtomKey` equality is exact per kind (float bit patterns, integer vs float, fact-address identity ignoring the public index). `Eq`/`Hash` are consistent. `FerricString` equality and order are byte equality and lexicographic order, and its hash matches the borrowed `str`. Symbol-table interning and resolution are exact. Encoding modes accept exactly the documented inputs. | `symbol_table.rs`, `ferric_string_order.rs` (byte order is a total order and equals Unicode scalar order on UTF-8), `value_structural_eq.rs` |
| Fact base, tokens, `OrderedSet` | Timestamp exhaustion has no side effects. Retracting a missing fact is a no-op. Structural fingerprints ignore the public index. Token lifecycle and stale-ID safety. `OrderedSet` refines an insertion-ordered sequence. | `core_stores_fact_base.rs` (index invariants under every operation), `core_stores_token_store.rs` (an invariant stronger than `validate_consistency`; cascade removal removes exactly the descendants), `core_stores_ordered_set.rs`, `ordered_set.rs` (+ refinement harnesses) |
| Alpha tests, bindings, join evaluation | Every constant and slot test against a reference written from the docs, including NaN, `-0.0`, integer/float promotion and checked offsets. Join tests and the index lemma: index lookup plus residual tests select the same facts as evaluating every test. `BindingSet`/`VarMap` against an abstract map. | `core_matching.rs` (alpha slot indexes, indexed candidate collection, bindings) |
| Negative, exists and NCC memories, beta memory | Empty-memory and lifecycle operations; `BetaMemory` refines an insertion-ordered list. | `negative_memory.rs`, `exists_memory.rs`, `ncc_memory.rs`, `beta_memory.rs`, `support_counting.rs`: incremental state equals the from-scratch value of the ground truth, and propagate/retract signals fire exactly on empty↔non-empty transitions, for any operation history |
| Rete propagation | Not tractable on production code (see [Gaps](#known-gaps)). | `rete_incremental.rs`: for any chain length and operation sequence, every beta memory holds exactly the declarative matches of its prefix of patterns, including one negative node. `rete_e2e.rs`: for single, join, `not` and `exists` rules, the agenda equals a naive oracle after any assert/retract/reset sequence. |
| Sequence (multifield) matching | `segment_extra` and the split search against a brute-force reference of every way to split the fields; `validate` characterized exactly. | `sequence_match.rs` (+ refinement harnesses), `core_sequence_split_search.rs`, `core_sequence_validate.rs`: sound and complete for patterns and inputs of any length |
| Agenda and conflict strategies | Every ordering type is a lawful total order. `build_key` implements Depth, Breadth, LEX and MEA exactly as documented. Sequence counters never overflow. | `conflict_strategy.rs` (+ refinement harnesses): total orders, the documented strategy semantics, `pop` returns the maximum, and operations preserve the agenda invariant. `agenda_sequence_rebase.rs`: rebasing preserves relative order. |
| Compiler, validation, snapshots | Limit checks are exact; condition validation accepts per spec. Diagnostic codes are the documented stable codes. Snapshot invariant checks never panic on malformed input and accept every normal state. | `core_compiler.rs`: validation accepts exactly the specified condition trees and reports the real demand, beta-node budgeting is exact and bounds every installation, and alpha/join nodes are shared exactly when their keys are equal. `pattern_restrictions.rs`: the loader's pattern-restriction validator reports E0001–E0003/E0005 exactly for the documented violations. |

### Parser, runtime and C ABI

<!-- Filled in by the outward pass. -->

## Trusted base

Results rest on:

- **The tools.** Kani 0.68 with CBMC, Verus with Z3 and `vstd`, and the Rust
  standard library and compiler. Kani treats atomic operations sequentially
  and cannot model unwinding, threads or `RandomState` seeding (`getrandom`).
- **Model fidelity**, for results proved only on a Verus model with no
  refinement harness. The model follows the production function cited in it.
  Reviewers checked this by reading it line by line and by mutation testing,
  not mechanically.
- **One trusted wrapper.** In `sequence_match.rs`, `slice_from_ref` states the
  documented behaviour of `std::slice::from_ref`, for which `vstd` has no
  specification.
- **Stated preconditions.** Where a harness constrains its inputs, the
  constraint is a documented or enforced precondition, and the harness cites
  where it is documented or enforced. An example is the ASCII invariant of
  `FerricString::Ascii`, enforced by `new` and by snapshot loading.

## Known gaps

- **Hash-table-heavy production code is out of Kani's reach.** This covers
  `ReteNetwork` end to end, `AlphaNetwork`, populated beta, negative, exists
  and NCC memories, and the variable indexes. Kani either cannot construct
  them (std `RandomState`) or cannot finish within memory once a few
  hash-table insertions happen. `rete_incremental.rs`, `rete_e2e.rs`,
  `support_counting.rs` and the memory models prove these algorithms
  unboundedly, but on models.
- **Bounds.** Kani results hold only within each harness's stated bounds,
  typically up to three facts or tokens, strings of up to 2–4 bytes, and
  operation sequences of up to five steps.
- **Not modelled:** the `Any` disjunction test's branch semantics, whole-fact
  `Sequence` tests inside the alpha network, NCC parent-rank insertion on
  production code, and "compiled networks pass `validate_consistency`". Each
  proof file lists its own gaps in detail.

## Adding or changing a proof

- Put harnesses in the module's `kani_proofs.rs` (declare it if absent), and
  write the oracle from the documentation.
- Keep symbolic data out of hash keys and out of the lengths of strings that
  get UTF-8-validated. Use concrete lengths per harness (const generics plus a
  macro) and short operation sequences. Tight `#[kani::unwind]` bounds matter,
  because one high bound multiplies the cost of every standard-library loop.
- After changing production code that a proof covers, re-run the harnesses in
  that module's proof files and the Verus file that cites it. A Verus model
  that no longer matches the code it cites is a stale proof: update the model
  in the same change.

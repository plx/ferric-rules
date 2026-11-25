//! Kani end-to-end Rete harnesses for `rete` (see docs/verification.md).
//!
//! Proved here: nothing. End-to-end bounded model checking of the production
//! `ReteNetwork` (compile a rule with `ReteCompiler`, then symbolic
//! `assert_fact` / `retract_fact` steps checked against a naive activation
//! oracle and `validate_consistency`) was attempted and measured as
//! intractable with Kani 0.68 / CBMC 6.11 under a 7 GB address-space limit,
//! so no harness is registered. The unbounded Verus model in
//! `verification/verus/rete_e2e.rs` proves the oracle property for the same
//! four rule shapes (`(a ?x)`, `(a ?x) (b ?x)`, `(a ?x) (not (b ?x))`,
//! `(a ?x) (exists (b ?x))`) on a model of the production algorithm.
//!
//! Measurements (each a single harness run, wall clock):
//! - `ReteNetwork::new()` alone: CBMC out of memory after about 420 s.
//!   hashbrown's group probing on a freshly allocated table is not resolved
//!   by symbolic execution, and `disabled_rules` is a
//!   `std::collections::HashSet` whose `RandomState::new()` reaches the
//!   `getrandom` system call (an unsupported foreign function inside an
//!   unbounded retry loop).
//! - With a verification-only `Vec`-backed substitute for every
//!   `FxHashMap` / `FxHashSet` / `SparseSecondaryMap` / `OrderedSet` on the
//!   path (import-line changes only, since reverted) and the network built
//!   field by field: one-pattern rule, two facts, three symbolic steps with
//!   checks after every step: timeout at 1200 s; two steps without
//!   `validate_consistency`: timeout at 600 s; rule compilation plus two
//!   concrete `FactBase` insertions, with no symbolic input at all: still in
//!   symbolic execution after 9 min at `unwind(8)` and after 9.5 min (5.4 GB)
//!   at `unwind(4)`.
//! - Root cause, isolated with concrete micro-probes at `unwind(3)`: CBMC
//!   does not bound loops over `Vec`s stored inside heap buffers (iterating an
//!   empty `Vec<u32>` held in a `Vec<Vec<u32>>` unwinds to the bound), and
//!   loses pointer-valued fields read back from heap buffers. One
//!   `BetaMemory` pushed into a `Vec` plus one `insert` gave a 110k-step
//!   formula and "solver ran out of memory during propositional reduction"
//!   after 16 s, because `ncc_parent_ranks: Option<Box<_>>` is no longer
//!   known to be `None`; the same memory as a local verifies in about 5 s.
//!   `ReteNetwork` keeps every memory, node and token in such heap
//!   containers, and the compiler's recursive `ConstantTest` lists make each
//!   unbounded loop expand deeply, so even concrete compilation diverges.
//!
//! Known gaps: the production code itself is not model checked end to end;
//! the Verus model abstracts token identities, block chronology, reverse
//! indices and duplicate activations (see that file's header).

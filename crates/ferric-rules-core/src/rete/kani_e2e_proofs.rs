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
//!   by symbolic execution. Review measurement: independently of cost,
//!   unmodified `new()` cannot pass: `disabled_rules` is a
//!   `std::collections::HashSet`, and a harness
//!   whose body is only `std::collections::HashSet::<RuleId>::new()` fails
//!   after about 1 s of CBMC time because `RandomState::new()` reaches
//!   unsupported constructs (a foreign `syscall` in
//!   `std::sys::sync::futex::unix::futex_wake_all`, reported as a reachable
//!   unsupported construct, and the missing definition of
//!   `std::sys::random::linux::getrandom::getrandom`). Any harness that calls
//!   `ReteNetwork::new()` / `with_strategy` (rete.rs:164) therefore fails
//!   unless `disabled_rules` is built another way.
//! - Review measurement: with `disabled_rules` built from a fixed
//!   `RandomState` (transmuted zero keys) and everything else unmodified,
//!   constructing an empty network field by field and seeding its root token
//!   (no rule, no fact, no symbolic input, `unwind(4)`) ran 377 s of CBMC
//!   time and reported VERIFICATION FAILED with 47 failed checks, all in
//!   library code: NULL / dangling dereferences in hashbrown's
//!   `RawTableInner::data_end`, pointer `offset` safety checks, an
//!   unsupported `same_allocation` query on unallocated memory, and again the
//!   foreign `syscall` and `getrandom`, although no `RandomState::new()` call
//!   remains on the path. The harness has no symbolic input and
//!   the same construction runs in every `rete` unit test, so these are
//!   artifacts of CBMC losing pointer precision on heap-held hash tables, not
//!   production defects; but they mean that on unmodified code even the empty
//!   network cannot be verified, let alone a compiled rule.
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
//!   A review re-run of the same pair measured 8 s for the local memory and
//!   a 600 s timeout (still in CBMC) for the pushed one.
//!   `ReteNetwork` keeps every memory, node and token in such heap
//!   containers, and the compiler's recursive `ConstantTest` lists make each
//!   unbounded loop expand deeply, so even concrete compilation diverges.
//!
//! Known gaps: the production code itself is not model checked end to end;
//! the Verus model abstracts token identities, block chronology, reverse
//! indices and activation ordering (it does count duplicate activations: its
//! agenda is a multiset), and has no mechanical link to this code (see that
//! file's header).

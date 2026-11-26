//! Verus model of the rule compiler's validation, beta-node budgeting and
//! alpha-path sharing in `crates/ferric-rules-core/src/compiler.rs`, plus the
//! alpha-cache part of the compiler snapshot check in `snapshot.rs`.
//!
//! Check with `verus --crate-type=lib verification/verus/core_compiler.rs`.
//!
//! # What is proved (unbounded: condition trees of any shape and depth,
//! any number of patterns, tests and compiled rules)
//!
//! * `validate_conditions` (compiler.rs:536) — iterative limit scan and
//!   structural diagnostics, `theorem_validate_conditions_exact` and the
//!   postconditions of `validate_conditions` / `validate_conditions_accepts_exactly`:
//!   - the worklist scan counts every condition node exactly once, nested NCC
//!     nodes and their contents included (`sizes`), so the rule is rejected
//!     with `ResourceLimit { required: 65, limit: 64 }` exactly when it has
//!     more than 64 nodes and all pattern test checks pass; the reported
//!     demand is always 65, never the rule's real size;
//!   - it returns `Ok` exactly when the rule has at most 64 nodes, every
//!     pattern passes its test checks, and the declarative structure
//!     predicate holds: every top-level NCC is non-empty; inside any NCC,
//!     patterns may be negated but not `exists`, nested NCCs are non-empty
//!     and satisfy the same rule recursively; no pattern binds one variable
//!     at two different slots (`top_member_ok`, `ncc_ok`, `pattern_ok`);
//!   - the error list is exactly the in-order list of per-node diagnostics
//!     (`top_errors`), all of kind E0005, and acceptance implies every NCC at
//!     every depth is non-empty (the `debug_assert!` that
//!     `compile_ncc_condition` relies on, compiler.rs:958-961).
//! * `validate_pattern_structure` (compiler.rs:738): the variable-reuse loop
//!   over a hash map reports a diagnostic exactly when the most recent earlier
//!   binding of the same variable used a different slot, and reports none at
//!   all exactly when no variable is bound at two different slots
//!   (`lemma_reuse_errors_exact`).
//! * `maximum_new_beta_nodes` (compiler.rs:122) equals
//!   `1 + nodes + nccs` (terminal, one node per condition, one partner per
//!   NCC) and is at most 129 for validated rules; the modeled installation
//!   (`compile_conditions_unchecked` / `compile_pattern` /
//!   `compile_ncc_condition` node allocations) allocates at most that many
//!   node IDs whatever the join-cache hits, so after `validate_conditions`
//!   and `ensure_node_capacity(maximum_new_beta_nodes(..))` succeed the
//!   `allocate_node_id` `expect` (beta.rs:1429-1435) can never fire:
//!   installation cannot partially fail (`compile_conditions`).
//! * Alpha-path sharing (`ensure_alpha_path`, compiler.rs:781, over
//!   `create_entry_node` / `create_constant_test_node` / `create_memory`,
//!   alpha.rs:483/508/533): from `ReteCompiler::new()` /
//!   `AlphaNetwork::new()` every call preserves `state_wf`, which states that
//!   the cache maps distinct keys to distinct memories, has one entry per
//!   memory, and every cached memory's owner node walks back through its
//!   parents spelling the key's tests in reverse and ending at the key's
//!   entry node; children lists and parent links agree.
//!   - `theorem_alpha_sharing_exact`: two patterns receive the same alpha
//!     memory if and only if their keys (entry type and full test list) are
//!     equal;
//!   - `theorem_alpha_recompile_reuses`: compiling the same key again returns
//!     the same memory and leaves the alpha network and the cache unchanged.
//! * Snapshot acceptance (`theorem_snapshot_accepts_compiled_alpha_paths`):
//!   the model of the alpha-cache part of
//!   `ReteCompiler::validate_snapshot_checked` (snapshot.rs:2022-2076:
//!   cache size, `parents`/`owners` reconstruction, duplicate cached memory,
//!   owner lookup, test-by-test walk, entry match) returns `Ok` on every state
//!   produced by any sequence of `ensure_alpha_path` calls.
//!
//! # Abstractions and simplifications (each also noted at its site)
//!
//! * A pattern (`CompilablePattern`, compiler.rs:41) keeps `negated`,
//!   `exists` and its variable bindings (symbol and slot as `u64` codes);
//!   `validate_pattern_tests` (compiler.rs:582) is abstracted to its outcome
//!   `tests_ok`, and its error to `CompileErrorM::PatternTests` (Kani checks
//!   that function on production code). Predicates keep their index.
//! * Diagnostic strings are abstracted to `ViolationM` tags; every one of
//!   them is built by `push_unsupported_structure_error` as an E0005
//!   `UnsupportedNestingCombination` (compiler.rs:768). `ResourceLimit`
//!   keeps `required`/`limit` (its resource name is "rule conditions").
//! * `HashMap::insert`'s returned previous value is read with `get` just
//!   before the insert (this vstd's `insert` returns `()`), which the
//!   `HashMap` contract makes identical.
//! * The two caches are modeled by what their keys determine. Entry types and
//!   constant tests are `u64` codes compared by equality (the production keys
//!   derive `PartialEq`/`Hash`); `alpha_path_cache` is an association list
//!   with map semantics (lookup by key equality, insert only when absent);
//!   `tests` passed to `ensure_alpha_path` already includes the appended
//!   sequence test (compiler.rs:787-799). The fact base is empty, so
//!   `backfill_memory` (which only fills memories) is omitted.
//! * Node-allocation modeling keeps only the node-ID counter; join-cache
//!   hits are a free input (`hits`), so the bound holds for every cache
//!   state. `prepare_var_map` (`VarMapOverflow`) is not modeled.
//! * In-place `Vec` element updates (`node_mut(..)`) are written as
//!   `Vec::remove` + `Vec::insert` at the same index, which yields the same
//!   vector. `AlphaNetwork` keeps `memory_count` for `memories.len()` /
//!   `next_memory_id` (the production asserts they agree). A ghost sequence
//!   `par` records which node's `children` received each node; it is erased
//!   and only states the invariant (it is what the snapshot check
//!   reconstructs as `parents`).
//! * The snapshot model omits the work budget (`Work::spend`, a resource
//!   limit, not a structural check), the join-cache part of the same
//!   function, and the rule-ID checks; the 32-bit index conversion is
//!   implied by `nodes.len() == next_node_id: u32`.
//! * Unsigned counters: `create_entry_node`/`create_constant_test_node`
//!   require `next_node_id < u32::MAX` and `create_memory` requires
//!   `memory_count < u32::MAX` (production uses unchecked `+= 1`, alpha.rs:489,
//!   510, 535); callers carry these as preconditions.
//!
//! # Trusted base
//!
//! Verus, Z3, and vstd (`Vec`, `HashMapWithView`, `HashSetWithView`
//! specifications and vstd's hash-table key-model axioms for `u32`/`u64`).
//! This file contains no `assume`, `admit`, `external_body`, `external`, or
//! `assume_specification`. `#[verifier::opaque]` only hides definitions for
//! performance; `#[verifier::rlimit(..)]` only raises the solver budget.
//!
//! # Known gaps
//!
//! * Hand transcription: fidelity rests on line-by-line comparison (each
//!   model function cites its production counterpart), not on compiling the
//!   production code. The Kani harnesses in compiler/kani_proofs.rs and
//!   snapshot/kani_proofs.rs check the production code on bounded inputs.
//! * Join-node sharing (`ensure_join_node`, the join part of the snapshot
//!   check) and the content of compiled beta nodes are not modeled beyond the
//!   allocation count; nor are `remove_rules` cache remapping, backfill,
//!   memory indexes, or the runtime's `validate_pattern_tests` details.
//! * The source-level validator that emits E0001, E0002, E0003 and E0005
//!   (ferric-rules-runtime loader.rs:7028-7171) is modeled separately in
//!   verification/verus/pattern_restrictions.rs.
use vstd::prelude::*;
use vstd::hash_map::HashMapWithView;
use vstd::hash_set::HashSetWithView;

verus! {

/// `MAX_RULE_CONDITIONS` (compiler.rs:26).
pub const MAX_RULE_CONDITIONS: usize = 64;

/// Models `CompilablePattern` (compiler.rs:41): the flags and variable
/// bindings `(symbol, slot)` as `u64` codes; `tests_ok` is the outcome of
/// `validate_pattern_tests` (compiler.rs:582) for this pattern.
pub struct PatternM {
    pub negated: bool,
    pub exists: bool,
    pub tests_ok: bool,
    pub variable_slots: Vec<(u64, u64)>,
}

/// Models `CompilableCondition` (compiler.rs:63).
pub enum CondM {
    Pattern(PatternM),
    Predicate { condition_index: u32 },
    Ncc(Vec<CondM>),
}

/// The E0005 diagnostics built by `push_unsupported_structure_error`
/// (compiler.rs:768), one tag per message: "cannot be negated" (compiler.rs:746),
/// "cannot be exists" (749), variable reuse (759), empty NCC (565), empty
/// nested NCC (697).
pub enum ViolationM {
    CannotBeNegated,
    CannotBeExists,
    VariableReuse,
    EmptyNcc,
    EmptyNestedNcc,
}

/// Models `CompileError` (compiler.rs:136) as far as validation produces it.
pub enum CompileErrorM {
    ResourceLimit { required: usize, limit: usize },
    PatternTests,
    Validation(Vec<ViolationM>),
}

// ---------------------------------------------------------------------------
// Sizes
// ---------------------------------------------------------------------------

pub open spec fn size(c: CondM) -> nat
    decreases c, 1nat,
{
    match c {
        CondM::Ncc(children) => 1 + sizes(children@),
        _ => 1,
    }
}

pub open spec fn sizes(s: Seq<CondM>) -> nat
    decreases s, 0nat,
{
    if s.len() == 0 {
        0
    } else {
        sizes(s.drop_last()) + size(s.last())
    }
}

pub open spec fn tests_ok(c: CondM) -> bool
    decreases c, 1nat,
{
    match c {
        CondM::Pattern(p) => p.tests_ok,
        CondM::Predicate { .. } => true,
        CondM::Ncc(children) => all_tests_ok(children@),
    }
}

pub open spec fn all_tests_ok(s: Seq<CondM>) -> bool
    decreases s, 0nat,
{
    s.len() == 0 || (all_tests_ok(s.drop_last()) && tests_ok(s.last()))
}

proof fn lemma_append(a: Seq<CondM>, b: Seq<CondM>)
    ensures
        sizes(a + b) == sizes(a) + sizes(b),
        all_tests_ok(a + b) == (all_tests_ok(a) && all_tests_ok(b)),
    decreases b.len(),
{
    if b.len() == 0 {
        assert(a + b =~= a);
    } else {
        lemma_append(a, b.drop_last());
        assert((a + b).drop_last() =~= a + b.drop_last());
        assert((a + b).last() == b.last());
    }
}

pub open spec fn dv(s: Seq<&CondM>) -> Seq<CondM> {
    s.map_values(|c: &CondM| *c)
}

// ---------------------------------------------------------------------------
// Variable reuse
// ---------------------------------------------------------------------------

pub open spec fn bindings(v: Seq<(u64, u64)>) -> Map<u64, u64>
    decreases v.len(),
{
    if v.len() == 0 {
        Map::empty()
    } else {
        bindings(v.drop_last()).insert(v.last().0, v.last().1)
    }
}

pub open spec fn reuse_errors(v: Seq<(u64, u64)>) -> Seq<ViolationM>
    decreases v.len(),
{
    if v.len() == 0 {
        seq![]
    } else {
        let previous = bindings(v.drop_last());
        reuse_errors(v.drop_last()) + if previous.contains_key(v.last().0) && previous[v.last().0]
            != v.last().1 {
            seq![ViolationM::VariableReuse]
        } else {
            seq![]
        }
    }
}

pub open spec fn reuses_variable(v: Seq<(u64, u64)>) -> bool {
    exists|i: int, j: int| 0 <= i < j < v.len() && v[i].0 == v[j].0 && v[i].1 != v[j].1
}

proof fn lemma_bindings(v: Seq<(u64, u64)>, k: u64)
    ensures
        bindings(v).contains_key(k) <==> exists|i: int| 0 <= i < v.len() && v[i].0 == k,
        bindings(v).contains_key(k) ==> exists|i: int|
            0 <= i < v.len() && v[i].0 == k && v[i].1 == bindings(v)[k],
        bindings(v).contains_key(k) && !reuses_variable(v) ==> forall|i: int|
            0 <= i < v.len() && v[i].0 == k ==> v[i].1 == bindings(v)[k],
    decreases v.len(),
{
    if v.len() > 0 {
        let w = v.drop_last();
        lemma_bindings(w, k);
        if !reuses_variable(v) {
            assert(!reuses_variable(w)) by {
                if reuses_variable(w) {
                    let (i, j) = choose|i: int, j: int|
                        0 <= i < j < w.len() && w[i].0 == w[j].0 && w[i].1 != w[j].1;
                    assert(v[i] == w[i] && v[j] == w[j]);
                }
            }
        }
        if exists|i: int| 0 <= i < v.len() && v[i].0 == k {
            let i = choose|i: int| 0 <= i < v.len() && v[i].0 == k;
            if i < w.len() {
                assert(w[i] == v[i]);
            }
        }
        if exists|i: int| 0 <= i < w.len() && w[i].0 == k {
            let i = choose|i: int| 0 <= i < w.len() && w[i].0 == k;
            assert(v[i] == w[i]);
        }
        if v.last().0 == k {
            assert(v[v.len() - 1].0 == k);
            if !reuses_variable(v) {
                assert forall|i: int| 0 <= i < v.len() && v[i].0 == k implies v[i].1
                    == bindings(v)[k] by {
                    if i < v.len() - 1 {
                        assert(!(v[i].0 == v[v.len() - 1].0 && v[i].1 != v[v.len() - 1].1));
                    }
                }
            }
        } else if bindings(v).contains_key(k) {
            let i = choose|i: int| 0 <= i < w.len() && w[i].0 == k && w[i].1 == bindings(w)[k];
            assert(v[i] == w[i]);
            if !reuses_variable(v) {
                assert forall|i: int| 0 <= i < v.len() && v[i].0 == k implies v[i].1
                    == bindings(v)[k] by {
                    assert(i < w.len());
                    assert(v[i] == w[i]);
                }
            }
        }
    }
}

/// The error list of the reuse loop is empty exactly when no variable is
/// bound at two different slots.
proof fn lemma_reuse_errors_exact(v: Seq<(u64, u64)>)
    ensures
        (reuse_errors(v).len() == 0) == !reuses_variable(v),
    decreases v.len(),
{
    if v.len() > 0 {
        let w = v.drop_last();
        let (k, s) = v.last();
        lemma_reuse_errors_exact(w);
        lemma_bindings(w, k);
        let previous = bindings(w);
        let tail_error = previous.contains_key(k) && previous[k] != s;
        assert(reuse_errors(v).len() == reuse_errors(w).len() + if tail_error {
            1int
        } else {
            0int
        });
        if reuses_variable(w) {
            let (i, j) = choose|i: int, j: int|
                0 <= i < j < w.len() && w[i].0 == w[j].0 && w[i].1 != w[j].1;
            assert(v[i] == w[i] && v[j] == w[j]);
            assert(reuses_variable(v));
        } else {
            if tail_error {
                let i = choose|i: int|
                    0 <= i < w.len() && w[i].0 == k && w[i].1 == previous[k];
                assert(v[i] == w[i]);
                assert(v[v.len() - 1] == (k, s));
                assert(reuses_variable(v));
            } else {
                assert(!reuses_variable(v)) by {
                    if reuses_variable(v) {
                        let (i, j) = choose|i: int, j: int|
                            0 <= i < j < v.len() && v[i].0 == v[j].0 && v[i].1 != v[j].1;
                        if j < v.len() - 1 {
                            assert(v[i] == w[i] && v[j] == w[j]);
                        } else {
                            assert(v[j] == (k, s));
                            assert(v[i] == w[i]);
                            assert(previous.contains_key(k));
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Structural diagnostics
// ---------------------------------------------------------------------------

pub open spec fn pattern_errors(p: PatternM, allow_negated: bool, allow_exists: bool) -> Seq<
    ViolationM,
> {
    (if p.negated && !allow_negated {
        seq![ViolationM::CannotBeNegated]
    } else {
        seq![]
    }) + (if p.exists && !allow_exists {
        seq![ViolationM::CannotBeExists]
    } else {
        seq![]
    }) + reuse_errors(p.variable_slots@)
}

pub open spec fn ncc_member_errors(c: CondM) -> Seq<ViolationM>
    decreases c, 1nat,
{
    match c {
        CondM::Pattern(p) => pattern_errors(p, true, false),
        CondM::Predicate { .. } => seq![],
        CondM::Ncc(inner) => if inner.len() == 0 {
            seq![ViolationM::EmptyNestedNcc]
        } else {
            ncc_errors(inner@)
        },
    }
}

pub open spec fn ncc_errors(s: Seq<CondM>) -> Seq<ViolationM>
    decreases s, 0nat,
{
    if s.len() == 0 {
        seq![]
    } else {
        ncc_errors(s.drop_last()) + ncc_member_errors(s.last())
    }
}

pub open spec fn top_member_errors(c: CondM) -> Seq<ViolationM> {
    match c {
        CondM::Pattern(p) => pattern_errors(p, true, true),
        CondM::Predicate { .. } => seq![],
        CondM::Ncc(sub) => if sub.len() == 0 {
            seq![ViolationM::EmptyNcc]
        } else {
            ncc_errors(sub@)
        },
    }
}

pub open spec fn top_errors(s: Seq<CondM>) -> Seq<ViolationM>
    decreases s.len(),
{
    if s.len() == 0 {
        seq![]
    } else {
        top_errors(s.drop_last()) + top_member_errors(s.last())
    }
}

proof fn lemma_size_member(s: Seq<CondM>, i: int)
    requires
        0 <= i < s.len(),
    ensures
        size(s[i]) <= sizes(s),
    decreases s.len(),
{
    if i < s.len() - 1 {
        lemma_size_member(s.drop_last(), i);
        assert(s.drop_last()[i] == s[i]);
    }
}

/// Models `ReteCompiler::validate_pattern_structure` (compiler.rs:738).
fn validate_pattern_structure(
    pattern: &PatternM,
    allow_negated: bool,
    allow_exists: bool,
    errors: &mut Vec<ViolationM>,
)
    ensures
        final(errors)@ == old(errors)@ + pattern_errors(*pattern, allow_negated, allow_exists),
{
    let ghost base = errors@;
    if pattern.negated && !allow_negated {
        errors.push(ViolationM::CannotBeNegated);
    }
    if pattern.exists && !allow_exists {
        errors.push(ViolationM::CannotBeExists);
    }
    let ghost flags = errors@;
    let mut variable_bindings: HashMapWithView<u64, u64> = HashMapWithView::new();
    let mut i: usize = 0;
    let ghost v = pattern.variable_slots@;
    while i < pattern.variable_slots.len()
        invariant
            v == pattern.variable_slots@,
            i <= v.len(),
            variable_bindings@ == bindings(v.take(i as int)),
            errors@ == flags + reuse_errors(v.take(i as int)),
        decreases v.len() - i,
    {
        let (var_sym, slot) = pattern.variable_slots[i];
        let ghost prefix = v.take(i as int);
        // `HashMap::insert` returns the previous value; this vstd version's
        // `insert` returns `()`, so the previous value is read just before.
        let previous = match variable_bindings.get(&var_sym) {
            Some(previous_slot) => Some(*previous_slot),
            None => None,
        };
        variable_bindings.insert(var_sym, slot);
        proof {
            assert(v.take(i + 1).drop_last() =~= prefix);
            assert(v.take(i + 1).last() == (var_sym, slot));
        }
        if let Some(previous_slot) = previous {
            if previous_slot != slot {
                errors.push(ViolationM::VariableReuse);
            }
        }
        i += 1;
        proof {
            assert(errors@ =~= flags + reuse_errors(v.take(i as int)));
        }
    }
    proof {
        assert(v.take(v.len() as int) =~= v);
        assert(errors@ =~= base + pattern_errors(*pattern, allow_negated, allow_exists));
    }
}

/// Models `ReteCompiler::validate_ncc_conditions` (compiler.rs:677).
fn validate_ncc_conditions(subconditions: &Vec<CondM>, errors: &mut Vec<ViolationM>)
    ensures
        final(errors)@ == old(errors)@ + ncc_errors(subconditions@),
    decreases sizes(subconditions@),
{
    let ghost base = errors@;
    let ghost s = subconditions@;
    let mut idx: usize = 0;
    while idx < subconditions.len()
        invariant
            s == subconditions@,
            idx <= s.len(),
            errors@ == base + ncc_errors(s.take(idx as int)),
        decreases s.len() - idx,
    {
        proof {
            assert(s.take(idx + 1).drop_last() =~= s.take(idx as int));
            assert(s.take(idx + 1).last() == s[idx as int]);
        }
        let ghost before = errors@;
        match &subconditions[idx] {
            CondM::Pattern(pattern) => {
                validate_pattern_structure(pattern, true, false, errors);
            },
            CondM::Predicate { .. } => {},
            CondM::Ncc(inner) => {
                if inner.len() == 0 {
                    errors.push(ViolationM::EmptyNestedNcc);
                } else {
                    proof {
                        lemma_size_member(s, idx as int);
                    }
                    validate_ncc_conditions(inner, errors);
                }
            },
        }
        proof {
            assert(errors@ == before + ncc_member_errors(s[idx as int]));
            assert(ncc_errors(s.take(idx + 1)) == ncc_errors(s.take(idx as int)) + ncc_member_errors(
                s[idx as int],
            ));
        }
        idx += 1;
        proof {
            assert(errors@ =~= base + ncc_errors(s.take(idx as int)));
        }
    }
    proof {
        assert(s.take(s.len() as int) =~= s);
    }
}

/// Models `ReteCompiler::finish_validation` (compiler.rs:729).
fn finish_validation(errors: Vec<ViolationM>) -> (r: Result<(), CompileErrorM>)
    ensures
        r matches Ok(()) <==> errors@.len() == 0,
        r matches Err(CompileErrorM::Validation(e)) ==> e@ == errors@,
        !(r matches Err(CompileErrorM::ResourceLimit { .. })),
        !(r matches Err(CompileErrorM::PatternTests)),
{
    if errors.len() == 0 {
        Ok(())
    } else {
        Err(CompileErrorM::Validation(errors))
    }
}

/// Models `ReteCompiler::validate_conditions` (compiler.rs:536).
fn validate_conditions(conditions: &Vec<CondM>) -> (r: Result<(), CompileErrorM>)
    ensures
        r matches Ok(()) ==> sizes(conditions@) <= 64 && all_tests_ok(conditions@) && top_errors(
            conditions@,
        ).len() == 0,
        r matches Err(CompileErrorM::ResourceLimit { required, limit }) ==> required == 65 && limit
            == 64 && sizes(conditions@) > 64,
        all_tests_ok(conditions@) && sizes(conditions@) > 64 ==> r matches Err(
            CompileErrorM::ResourceLimit { .. },
        ),
        r matches Err(CompileErrorM::PatternTests) ==> !all_tests_ok(conditions@),
        sizes(conditions@) <= 64 && !all_tests_ok(conditions@) ==> r matches Err(
            CompileErrorM::PatternTests,
        ),
        sizes(conditions@) <= 64 && all_tests_ok(conditions@) ==> match r {
            Ok(()) => top_errors(conditions@).len() == 0,
            Err(CompileErrorM::Validation(e)) => e@ == top_errors(conditions@) && e@.len() > 0,
            _ => false,
        },
{
    let mut pending: Vec<&CondM> = Vec::new();
    let mut i: usize = 0;
    while i < conditions.len()
        invariant
            i <= conditions.len(),
            dv(pending@) == conditions@.take(i as int),
        decreases conditions.len() - i,
    {
        pending.push(&conditions[i]);
        i += 1;
        proof {
            assert(conditions@.take(i as int) =~= conditions@.take(i - 1).push(conditions@[i - 1]));
        }
    }
    proof {
        assert(conditions@.take(conditions.len() as int) =~= conditions@);
    }
    let mut count: usize = 0;
    while pending.len() > 0
        invariant
            count <= MAX_RULE_CONDITIONS,
            count + sizes(dv(pending@)) == sizes(conditions@),
            all_tests_ok(conditions@) == all_tests_ok(dv(pending@)),
        decreases sizes(dv(pending@)),
    {
        let ghost before = dv(pending@);
        let condition = pending.pop().unwrap();
        proof {
            assert(before.drop_last() =~= dv(pending@));
            assert(before.last() == *condition);
            assert(sizes(before) == sizes(dv(pending@)) + size(*condition));
            assert(all_tests_ok(before) == (all_tests_ok(dv(pending@)) && tests_ok(*condition)));
        }
        count += 1;
        // `check_limit("rule conditions", count, MAX_RULE_CONDITIONS)?`
        if count > MAX_RULE_CONDITIONS {
            return Err(CompileErrorM::ResourceLimit { required: count, limit: MAX_RULE_CONDITIONS });
        }
        match condition {
            CondM::Pattern(pattern) => {
                // `validate_pattern_tests(pattern)?`, abstracted to its outcome.
                if !pattern.tests_ok {
                    return Err(CompileErrorM::PatternTests);
                }
            },
            CondM::Ncc(children) => {
                let ghost rest = dv(pending@);
                let mut j: usize = 0;
                while j < children.len()
                    invariant
                        j <= children.len(),
                        dv(pending@) == rest + children@.take(j as int),
                    decreases children.len() - j,
                {
                    pending.push(&children[j]);
                    j += 1;
                    proof {
                        assert(rest + children@.take(j as int) =~= (rest + children@.take(j - 1)).push(
                            children@[j - 1],
                        ));
                    }
                }
                proof {
                    assert(children@.take(children.len() as int) =~= children@);
                    lemma_append(rest, children@);
                }
            },
            CondM::Predicate { .. } => {},
        }
    }
    let mut errors: Vec<ViolationM> = Vec::new();
    let ghost s = conditions@;
    let mut condition_idx: usize = 0;
    while condition_idx < conditions.len()
        invariant
            s == conditions@,
            condition_idx <= s.len(),
            errors@ == top_errors(s.take(condition_idx as int)),
        decreases s.len() - condition_idx,
    {
        proof {
            assert(s.take(condition_idx + 1).drop_last() =~= s.take(condition_idx as int));
            assert(s.take(condition_idx + 1).last() == s[condition_idx as int]);
        }
        match &conditions[condition_idx] {
            CondM::Pattern(pattern) => {
                validate_pattern_structure(pattern, true, true, &mut errors);
            },
            CondM::Predicate { .. } => {},
            CondM::Ncc(subconditions) => {
                if subconditions.len() == 0 {
                    errors.push(ViolationM::EmptyNcc);
                } else {
                    validate_ncc_conditions(subconditions, &mut errors);
                }
            },
        }
        condition_idx += 1;
        proof {
            assert(errors@ =~= top_errors(s.take(condition_idx as int)));
        }
    }
    proof {
        assert(s.take(s.len() as int) =~= s);
    }
    finish_validation(errors)
}


// ---------------------------------------------------------------------------
// Declarative characterization of the structural diagnostics
// ---------------------------------------------------------------------------

/// A pattern needs no diagnostic: negation/exists only where allowed, and no
/// variable bound at two different slots.
pub open spec fn pattern_ok(p: PatternM, allow_negated: bool, allow_exists: bool) -> bool {
    (!p.negated || allow_negated) && (!p.exists || allow_exists) && !reuses_variable(
        p.variable_slots@,
    )
}

/// Inside an NCC: patterns may be negated (forall desugaring) but not
/// exists, and nested NCCs must be non-empty and well formed themselves.
pub open spec fn ncc_member_ok(c: CondM) -> bool
    decreases c, 1nat,
{
    match c {
        CondM::Pattern(p) => pattern_ok(p, true, false),
        CondM::Predicate { .. } => true,
        CondM::Ncc(inner) => inner.len() > 0 && ncc_ok(inner@),
    }
}

pub open spec fn ncc_ok(s: Seq<CondM>) -> bool
    decreases s, 0nat,
{
    s.len() == 0 || (ncc_ok(s.drop_last()) && ncc_member_ok(s.last()))
}

/// At the top level: patterns may be negated or exists; NCCs must be
/// non-empty and well formed.
pub open spec fn top_member_ok(c: CondM) -> bool {
    match c {
        CondM::Pattern(p) => pattern_ok(p, true, true),
        CondM::Predicate { .. } => true,
        CondM::Ncc(sub) => sub.len() > 0 && ncc_ok(sub@),
    }
}

/// Every NCC at every depth has at least one subcondition (the
/// `debug_assert!` at compiler.rs:957 that `compile_ncc_condition` relies on).
pub open spec fn nccs_nonempty(c: CondM) -> bool
    decreases c, 1nat,
{
    match c {
        CondM::Ncc(children) => children.len() > 0 && all_nccs_nonempty(children@),
        _ => true,
    }
}

pub open spec fn all_nccs_nonempty(s: Seq<CondM>) -> bool
    decreases s, 0nat,
{
    s.len() == 0 || (all_nccs_nonempty(s.drop_last()) && nccs_nonempty(s.last()))
}

proof fn lemma_pattern_errors_exact(p: PatternM, allow_negated: bool, allow_exists: bool)
    ensures
        (pattern_errors(p, allow_negated, allow_exists).len() == 0) == pattern_ok(
            p,
            allow_negated,
            allow_exists,
        ),
{
    lemma_reuse_errors_exact(p.variable_slots@);
}

proof fn lemma_ncc_errors_exact(s: Seq<CondM>)
    ensures
        (ncc_errors(s).len() == 0) == ncc_ok(s),
        ncc_ok(s) ==> all_nccs_nonempty(s),
    decreases s, 0nat,
{
    if s.len() > 0 {
        lemma_ncc_errors_exact(s.drop_last());
        lemma_ncc_member_errors_exact(s.last());
    }
}

proof fn lemma_ncc_member_errors_exact(c: CondM)
    ensures
        (ncc_member_errors(c).len() == 0) == ncc_member_ok(c),
        ncc_member_ok(c) ==> nccs_nonempty(c),
    decreases c, 1nat,
{
    match c {
        CondM::Pattern(p) => lemma_pattern_errors_exact(p, true, false),
        CondM::Predicate { .. } => {},
        CondM::Ncc(inner) => {
            if inner.len() > 0 {
                lemma_ncc_errors_exact(inner@);
            }
        },
    }
}

proof fn lemma_top_errors_exact(s: Seq<CondM>)
    ensures
        (top_errors(s).len() == 0) == (forall|i: int| 0 <= i < s.len() ==> top_member_ok(#[trigger] s[i])),
        (top_errors(s).len() == 0) ==> all_nccs_nonempty(s),
    decreases s.len(),
{
    if s.len() > 0 {
        let w = s.drop_last();
        lemma_top_errors_exact(w);
        let c = s.last();
        match c {
            CondM::Pattern(p) => lemma_pattern_errors_exact(p, true, true),
            CondM::Predicate { .. } => {},
            CondM::Ncc(sub) => {
                if sub.len() > 0 {
                    lemma_ncc_errors_exact(sub@);
                }
            },
        }
        assert(top_errors(s).len() == top_errors(w).len() + top_member_errors(c).len());
        assert((top_member_errors(c).len() == 0) == top_member_ok(c));
        if forall|i: int| 0 <= i < s.len() ==> top_member_ok(#[trigger] s[i]) {
            assert forall|i: int| 0 <= i < w.len() implies top_member_ok(#[trigger] w[i]) by {
                assert(w[i] == s[i]);
            }
            assert(top_member_ok(s[s.len() - 1]));
        }
        if (forall|i: int| 0 <= i < w.len() ==> top_member_ok(#[trigger] w[i])) && top_member_ok(c) {
            assert forall|i: int| 0 <= i < s.len() implies top_member_ok(#[trigger] s[i]) by {
                if i < w.len() {
                    assert(w[i] == s[i]);
                }
            }
        }
        if top_errors(s).len() == 0 {
            assert(top_errors(w).len() == 0);
            assert(nccs_nonempty(c)) by {
                match c {
                    CondM::Ncc(sub) => {
                        assert(sub.len() > 0);
                        assert(ncc_ok(sub@));
                    },
                    _ => {},
                }
            }
            assert(all_nccs_nonempty(s));
        }
    }
}

/// The NCC well-formedness predicate in quantified form.
proof fn lemma_ncc_ok_forall(s: Seq<CondM>)
    ensures
        ncc_ok(s) == (forall|i: int| 0 <= i < s.len() ==> ncc_member_ok(#[trigger] s[i])),
    decreases s.len(),
{
    if s.len() > 0 {
        let w = s.drop_last();
        lemma_ncc_ok_forall(w);
        if forall|i: int| 0 <= i < s.len() ==> ncc_member_ok(#[trigger] s[i]) {
            assert forall|i: int| 0 <= i < w.len() implies ncc_member_ok(#[trigger] w[i]) by {
                assert(w[i] == s[i]);
            }
            assert(ncc_member_ok(s[s.len() - 1]));
        }
        if ncc_ok(s) {
            assert forall|i: int| 0 <= i < s.len() implies ncc_member_ok(#[trigger] s[i]) by {
                if i < w.len() {
                    assert(w[i] == s[i]);
                }
            }
        }
    }
}

/// Theorem (validate_conditions accepts exactly the specified rules): the
/// modeled validator returns `Ok` exactly when the rule has at most 64
/// condition nodes (counting nested NCC nodes and their contents), every
/// pattern passes its test checks, every top-level NCC is non-empty, and
/// every member of every NCC is a non-exists pattern, a predicate, or a
/// non-empty NCC of the same kind, and no pattern binds one variable at two
/// slots. Acceptance implies every NCC at every depth is non-empty.
proof fn theorem_validate_conditions_exact(conditions: Seq<CondM>, accepted: bool)
    requires
        accepted == (sizes(conditions) <= 64 && all_tests_ok(conditions) && top_errors(
            conditions,
        ).len() == 0),
    ensures
        accepted == (sizes(conditions) <= 64 && all_tests_ok(conditions) && forall|i: int|
            0 <= i < conditions.len() ==> top_member_ok(#[trigger] conditions[i])),
        accepted ==> all_nccs_nonempty(conditions),
{
    lemma_top_errors_exact(conditions);
}

/// Exec-level wrapper: the result of the modeled `validate_conditions`.
fn validate_conditions_accepts_exactly(conditions: &Vec<CondM>) -> (r: Result<(), CompileErrorM>)
    ensures
        r is Ok == (sizes(conditions@) <= 64 && all_tests_ok(conditions@) && forall|i: int|
            0 <= i < conditions@.len() ==> top_member_ok(#[trigger] conditions@[i])),
        r is Ok ==> all_nccs_nonempty(conditions@),
{
    let r = validate_conditions(conditions);
    proof {
        lemma_top_errors_exact(conditions@);
    }
    r
}


// ---------------------------------------------------------------------------
// Beta-node demand (`maximum_new_beta_nodes`) and installation
// ---------------------------------------------------------------------------

pub open spec fn nccs(c: CondM) -> nat
    decreases c, 1nat,
{
    match c {
        CondM::Ncc(children) => 1 + nccs_s(children@),
        _ => 0,
    }
}

pub open spec fn nccs_s(s: Seq<CondM>) -> nat
    decreases s, 0nat,
{
    if s.len() == 0 {
        0
    } else {
        nccs_s(s.drop_last()) + nccs(s.last())
    }
}

proof fn lemma_nccs_le_sizes(s: Seq<CondM>)
    ensures
        nccs_s(s) <= sizes(s),
    decreases s, 0nat,
{
    if s.len() > 0 {
        lemma_nccs_le_sizes(s.drop_last());
        lemma_nccs_le_size(s.last());
    }
}

proof fn lemma_nccs_le_size(c: CondM)
    ensures
        nccs(c) <= size(c),
    decreases c, 1nat,
{
    if let CondM::Ncc(children) = c {
        lemma_nccs_le_sizes(children@);
    }
}

proof fn lemma_nccs_append(a: Seq<CondM>, b: Seq<CondM>)
    ensures
        nccs_s(a + b) == nccs_s(a) + nccs_s(b),
    decreases b.len(),
{
    if b.len() == 0 {
        assert(a + b =~= a);
    } else {
        lemma_nccs_append(a, b.drop_last());
        assert((a + b).drop_last() =~= a + b.drop_last());
        assert((a + b).last() == b.last());
    }
}

proof fn lemma_take_step(s: Seq<CondM>, i: int)
    requires
        0 <= i < s.len(),
    ensures
        sizes(s.take(i + 1)) == sizes(s.take(i)) + size(s[i]),
        nccs_s(s.take(i + 1)) == nccs_s(s.take(i)) + nccs(s[i]),
        sizes(s.take(i + 1)) + nccs_s(s.take(i + 1)) <= sizes(s) + nccs_s(s),
        sizes(s.take(i + 1)) <= sizes(s),
{
    assert(s.take(i + 1).drop_last() =~= s.take(i));
    assert(s.take(i + 1).last() == s[i]);
    lemma_append(s.take(i + 1), s.skip(i + 1));
    lemma_nccs_append(s.take(i + 1), s.skip(i + 1));
    assert(s.take(i + 1) + s.skip(i + 1) =~= s);
}

/// Models `maximum_new_beta_nodes` (compiler.rs:122). Every call site runs
/// after validation (compiler.rs:296-304, 323-325, and plans built by
/// `plan_conditions` at compiler.rs:341), which bounds the rule at 64
/// condition nodes; the `requires` records that.
fn maximum_new_beta_nodes(conditions: &Vec<CondM>) -> (r: usize)
    requires
        sizes(conditions@) <= 64,
    ensures
        r == 1 + sizes(conditions@) + nccs_s(conditions@),
        r <= 1 + 2 * 64,
    decreases sizes(conditions@),
{
    let ghost s = conditions@;
    let mut sum: usize = 0;
    let mut i: usize = 0;
    while i < conditions.len()
        invariant
            s == conditions@,
            sizes(s) <= 64,
            i <= s.len(),
            sum == sizes(s.take(i as int)) + nccs_s(s.take(i as int)),
            sizes(s.take(i as int)) <= sizes(s),
        decreases s.len() - i,
    {
        proof {
            lemma_take_step(s, i as int);
            lemma_nccs_le_sizes(s.take(i + 1));
        }
        let term: usize = match &conditions[i] {
            CondM::Pattern(_) | CondM::Predicate { .. } => 1,
            CondM::Ncc(children) => {
                proof {
                    lemma_size_member(s, i as int);
                }
                1 + maximum_new_beta_nodes(children)
            },
        };
        sum = sum + term;
        i += 1;
    }
    proof {
        assert(s.take(s.len() as int) =~= s);
        lemma_nccs_le_sizes(s);
    }
    1 + sum
}

/// Models the node-ID counter of `BetaNetwork` (beta.rs:506).
pub struct BetaM {
    pub next_node_id: u32,
}

/// Models `BetaNetwork::allocate_node_id` (beta.rs:1429). Production's
/// `checked_add(1).expect(..)` panics exactly when the counter is
/// `u32::MAX`; the `requires` makes every modeled caller prove it is not.
fn allocate_node_id(beta: &mut BetaM) -> (id: u32)
    requires
        old(beta).next_node_id < u32::MAX,
    ensures
        id == old(beta).next_node_id,
        final(beta).next_node_id == old(beta).next_node_id + 1,
{
    let id = beta.next_node_id;
    beta.next_node_id = beta.next_node_id + 1;
    id
}

/// Models `BetaNetwork::ensure_node_capacity` (beta.rs:1407).
fn ensure_node_capacity(beta: &BetaM, required: usize) -> (ok: bool)
    ensures
        ok == (required <= u32::MAX - beta.next_node_id),
{
    let available = (u32::MAX - beta.next_node_id) as usize;
    !(required > available)
}

/// Models the node allocations of `compile_pattern` (compiler.rs:854):
/// `create_negative_node` / `create_exists_node` allocate one node; a
/// positive pattern allocates one only when `ensure_join_node`
/// (compiler.rs:826) misses its cache, which `join_cache_hit` stands for.
fn compile_pattern_nodes(beta: &mut BetaM, pattern: &PatternM, join_cache_hit: bool)
    requires
        old(beta).next_node_id < u32::MAX,
    ensures
        old(beta).next_node_id <= final(beta).next_node_id <= old(beta).next_node_id + 1,
{
    if pattern.negated {
        allocate_node_id(beta);
    } else if pattern.exists {
        allocate_node_id(beta);
    } else if !join_cache_hit {
        allocate_node_id(beta);
    }
}

/// Models the node allocations of `compile_ncc_condition` (compiler.rs:946):
/// the NCC node, each subcondition in order, then the partner.
fn compile_ncc_condition_nodes(beta: &mut BetaM, subconditions: &Vec<CondM>, hits: &Vec<bool>)
    requires
        old(beta).next_node_id as int + 2 + sizes(subconditions@) + nccs_s(subconditions@)
            <= u32::MAX,
    ensures
        old(beta).next_node_id <= final(beta).next_node_id,
        final(beta).next_node_id as int <= old(beta).next_node_id as int + 2 + sizes(
            subconditions@,
        ) + nccs_s(subconditions@),
    decreases sizes(subconditions@),
{
    let ghost start = beta.next_node_id as int;
    let _ncc_id = allocate_node_id(beta);
    let ghost s = subconditions@;
    let mut i: usize = 0;
    while i < subconditions.len()
        invariant
            s == subconditions@,
            i <= s.len(),
            start + 2 + sizes(s) + nccs_s(s) <= u32::MAX,
            start + 1 <= beta.next_node_id as int <= start + 1 + sizes(s.take(i as int)) + nccs_s(
                s.take(i as int),
            ),
        decreases s.len() - i,
    {
        proof {
            lemma_take_step(s, i as int);
        }
        match &subconditions[i] {
            CondM::Pattern(pattern) => {
                let hit = i < hits.len() && hits[i];
                compile_pattern_nodes(beta, pattern, hit);
            },
            CondM::Predicate { .. } => {
                allocate_node_id(beta);
            },
            CondM::Ncc(inner) => {
                proof {
                    lemma_size_member(s, i as int);
                }
                compile_ncc_condition_nodes(beta, inner, hits);
            },
        }
        i += 1;
    }
    proof {
        assert(s.take(s.len() as int) =~= s);
    }
    let _partner = allocate_node_id(beta);
}

/// Models the node allocations of `compile_conditions_unchecked`
/// (compiler.rs:434): each condition in order, then the terminal.
fn compile_conditions_nodes(beta: &mut BetaM, conditions: &Vec<CondM>, hits: &Vec<bool>)
    requires
        old(beta).next_node_id as int + 1 + sizes(conditions@) + nccs_s(conditions@) <= u32::MAX,
    ensures
        old(beta).next_node_id <= final(beta).next_node_id,
        final(beta).next_node_id as int - old(beta).next_node_id as int <= 1 + sizes(conditions@)
            + nccs_s(conditions@),
{
    let ghost start = beta.next_node_id as int;
    let ghost s = conditions@;
    let mut i: usize = 0;
    while i < conditions.len()
        invariant
            s == conditions@,
            i <= s.len(),
            start + 1 + sizes(s) + nccs_s(s) <= u32::MAX,
            start <= beta.next_node_id as int <= start + sizes(s.take(i as int)) + nccs_s(
                s.take(i as int),
            ),
        decreases s.len() - i,
    {
        proof {
            lemma_take_step(s, i as int);
        }
        match &conditions[i] {
            CondM::Pattern(pattern) => {
                let hit = i < hits.len() && hits[i];
                compile_pattern_nodes(beta, pattern, hit);
            },
            CondM::Predicate { .. } => {
                allocate_node_id(beta);
            },
            CondM::Ncc(subconditions) => {
                proof {
                    lemma_size_member(s, i as int);
                }
                compile_ncc_condition_nodes(beta, subconditions, hits);
            },
        }
        i += 1;
    }
    proof {
        assert(s.take(s.len() as int) =~= s);
    }
    let _terminal = allocate_node_id(beta);
}

/// Theorem (installation cannot partially fail): after `validate_conditions`
/// accepts a rule and `ensure_node_capacity(maximum_new_beta_nodes(..))`
/// succeeds — the sequence in `compile_conditions` (compiler.rs:317-337) —
/// the modeled installation never reaches the node-ID `expect` panic,
/// whatever the join cache hits are, and allocates at most the demand.
fn compile_conditions(beta: &mut BetaM, conditions: &Vec<CondM>, hits: &Vec<bool>) -> (installed: bool)
    ensures
        installed ==> final(beta).next_node_id as int - old(beta).next_node_id as int <= 1 + sizes(
            conditions@,
        ) + nccs_s(conditions@) <= 1 + 2 * 64,
        !installed ==> final(beta).next_node_id == old(beta).next_node_id,
{
    if validate_conditions(conditions).is_err() {
        return false;
    }
    let demand = maximum_new_beta_nodes(conditions);
    if !ensure_node_capacity(beta, demand) {
        return false;
    }
    compile_conditions_nodes(beta, conditions, hits);
    true
}


// ---------------------------------------------------------------------------
// Alpha path sharing (`ensure_alpha_path`) and the snapshot cache check
// ---------------------------------------------------------------------------

/// Models `AlphaNode` (alpha.rs:118): an entry node for a fact type, or a
/// constant-test node. Entry types and tests are abstracted to `u64` codes
/// compared by equality (the production keys derive `Eq`/`Hash`).
pub enum AlphaKindM {
    Entry(u64),
    Test(u64),
}

pub struct AlphaNodeM {
    pub kind: AlphaKindM,
    pub children: Vec<u32>,
    pub memory: Option<u32>,
}

/// Models `AlphaNetwork` (alpha.rs:366): `nodes`, the number of memories
/// (`memories.len()` = `next_memory_id`), `entry_nodes`, `next_node_id`.
pub struct AlphaM {
    pub nodes: Vec<AlphaNodeM>,
    pub memory_count: u32,
    pub entry_nodes: HashMapWithView<u64, u32>,
    pub next_node_id: u32,
}

/// Models `AlphaPathKey` (compiler.rs:154).
pub struct AlphaKeyM {
    pub entry: u64,
    pub tests: Vec<u64>,
}

pub open spec fn kv(k: AlphaKeyM) -> (u64, Seq<u64>) {
    (k.entry, k.tests@)
}

/// `alpha_path_cache: HashMap<AlphaPathKey, AlphaMemoryId>` (compiler.rs:232),
/// modeled as an association list with map semantics (lookup by key
/// equality; new keys are added only when absent).
pub struct CompilerM {
    pub alpha_path_cache: Vec<(AlphaKeyM, u32)>,
}

pub open spec fn kind_of(nodes: Seq<AlphaNodeM>, i: int) -> AlphaKindM {
    nodes[i].kind
}

/// Walking from node `o` through the (ghost) parent links spells `tests`
/// backwards and ends at an entry node for `entry`: exactly what
/// `ReteCompiler::validate_snapshot_checked` checks for each cached key
/// (snapshot.rs:2061-2076: "cached alpha test mismatch", "cached alpha path
/// lacks ancestor", "cached alpha entry mismatch").
pub open spec fn spells(
    nodes: Seq<AlphaNodeM>,
    par: Seq<Option<u32>>,
    o: int,
    entry: u64,
    tests: Seq<u64>,
) -> bool
    decreases tests.len(),
{
    0 <= o < nodes.len() && o < par.len() && if tests.len() == 0 {
        nodes[o].kind == AlphaKindM::Entry(entry)
    } else {
        &&& nodes[o].kind == AlphaKindM::Test(tests.last())
        &&& par[o] is Some
        &&& spells(nodes, par, par[o]->Some_0 as int, entry, tests.drop_last())
    }
}

#[verifier::opaque]
pub open spec fn alpha_wf_parts(
    nodes: Seq<AlphaNodeM>,
    entries: Map<u64, u32>,
    memory_count: u32,
    next_node_id: int,
    par: Seq<Option<u32>>,
) -> bool {
    &&& nodes.len() == next_node_id
    &&& par.len() == nodes.len()
    &&& forall|i: int| 0 <= i < nodes.len() ==> ((#[trigger] nodes[i]).kind is Entry <==> par[i] is None)
    &&& forall|i: int|
        0 <= i < nodes.len() && (#[trigger] par[i]) is Some ==> par[i]->Some_0 < i && nodes[par[i]->Some_0 as int].children@.contains(i as u32)
    &&& forall|p: int, k: int|
        0 <= p < nodes.len() && 0 <= k < nodes[p].children@.len() ==> (#[trigger] nodes[p].children@[k]) < nodes.len()
            && par[nodes[p].children@[k] as int] == Some(p as u32)
    &&& forall|e: u64| #[trigger] entries.contains_key(e) ==> entries[e] < nodes.len()
        && nodes[entries[e] as int].kind == AlphaKindM::Entry(e)
    &&& forall|i: int| 0 <= i < nodes.len() && (#[trigger] nodes[i]).memory is Some ==> nodes[i].memory->Some_0 < memory_count
    &&& forall|i: int, j: int|
        0 <= i < nodes.len() && 0 <= j < nodes.len() && (#[trigger] nodes[i]).memory is Some && nodes[i].memory == (#[trigger] nodes[j]).memory ==> i == j
}

/// Structural invariant of the alpha network built by the compiler. `par`
/// is a ghost record of the node whose `children` list received each node;
/// it is what the snapshot check reconstructs as `parents` (snapshot.rs:2023, 2041).
pub open spec fn alpha_wf(a: AlphaM, par: Seq<Option<u32>>) -> bool {
    alpha_wf_parts(a.nodes@, a.entry_nodes@, a.memory_count, a.next_node_id as int, par)
}

#[verifier::opaque]
pub open spec fn cache_wf(c: Seq<(AlphaKeyM, u32)>, a: AlphaM, par: Seq<Option<u32>>) -> bool {
    let nodes = a.nodes@;
    &&& c.len() == a.memory_count
    &&& forall|j: int| 0 <= j < c.len() ==> (#[trigger] c[j]).1 < a.memory_count
    &&& forall|j: int, l: int| 0 <= j < l < c.len() ==> kv((#[trigger] c[j]).0) != kv((#[trigger] c[l]).0)
    &&& forall|j: int, l: int| 0 <= j < l < c.len() ==> (#[trigger] c[j]).1 != (#[trigger] c[l]).1
    &&& forall|j: int| 0 <= j < c.len() ==> #[trigger] has_owner(c, a, par, j)
    &&& forall|o: int| 0 <= o < nodes.len() && (#[trigger] nodes[o]).memory is Some ==> owned_by_cache(c, a, par, o)
}

/// Cached memory `j` has an owner node whose walk spells its key
/// ("cached alpha memory lacks owner", snapshot.rs:2055-2059).
pub open spec fn has_owner(c: Seq<(AlphaKeyM, u32)>, a: AlphaM, par: Seq<Option<u32>>, j: int) -> bool {
    exists|o: int|
        0 <= o < a.nodes@.len() && (#[trigger] a.nodes@[o]).memory == Some(c[j].1)
            && spells(a.nodes@, par, o, c[j].0.entry, c[j].0.tests@)
}

/// Every memory attached to a node is cached under the key its node spells.
pub open spec fn owned_by_cache(c: Seq<(AlphaKeyM, u32)>, a: AlphaM, par: Seq<Option<u32>>, o: int) -> bool {
    exists|j: int|
        0 <= j < c.len() && (#[trigger] c[j]).1 == a.nodes@[o].memory->Some_0
            && spells(a.nodes@, par, o, c[j].0.entry, c[j].0.tests@)
}

/// The walk from a node is deterministic, so a node spells at most one key.
proof fn lemma_spells_unique(
    nodes: Seq<AlphaNodeM>,
    par: Seq<Option<u32>>,
    o: int,
    e1: u64,
    t1: Seq<u64>,
    e2: u64,
    t2: Seq<u64>,
)
    requires
        spells(nodes, par, o, e1, t1),
        spells(nodes, par, o, e2, t2),
    ensures
        e1 == e2 && t1 == t2,
    decreases t1.len(),
{
    if t1.len() == 0 {
        if t2.len() > 0 {
            assert(nodes[o].kind == AlphaKindM::Entry(e1));
        }
    } else {
        if t2.len() == 0 {
            assert(nodes[o].kind == AlphaKindM::Test(t1.last()));
        } else {
            lemma_spells_unique(
                nodes,
                par,
                par[o]->Some_0 as int,
                e1,
                t1.drop_last(),
                e2,
                t2.drop_last(),
            );
            assert(t1 =~= t1.drop_last().push(t1.last()));
            assert(t2 =~= t2.drop_last().push(t2.last()));
        }
    }
}

/// Appending nodes and extending children lists preserves existing walks.
proof fn lemma_spells_frame(
    n1: Seq<AlphaNodeM>,
    p1: Seq<Option<u32>>,
    n2: Seq<AlphaNodeM>,
    p2: Seq<Option<u32>>,
    o: int,
    e: u64,
    t: Seq<u64>,
)
    requires
        spells(n1, p1, o, e, t),
        n1.len() <= n2.len(),
        n1.len() <= p1.len() <= p2.len(),
        forall|i: int| 0 <= i < n1.len() ==> (#[trigger] n2[i]).kind == n1[i].kind,
        forall|i: int| 0 <= i < n1.len() ==> (#[trigger] p2[i]) == p1[i],
    ensures
        spells(n2, p2, o, e, t),
    decreases t.len(),
{
    assert(n2[o].kind == n1[o].kind);
    assert(p2[o] == p1[o]);
    if t.len() > 0 {
        lemma_spells_frame(n1, p1, n2, p2, p1[o]->Some_0 as int, e, t.drop_last());
    }
}

/// Models `AlphaNetwork::create_entry_node` (alpha.rs:483).
fn create_entry_node(a: &mut AlphaM, entry_type: u64, Ghost(par): Ghost<Seq<Option<u32>>>) -> (r: (
    u32,
    Ghost<Seq<Option<u32>>>,
))
    requires
        alpha_wf(*old(a), par),
        old(a).next_node_id < u32::MAX,
    ensures
        alpha_wf(*final(a), r.1@),
        r.0 < final(a).nodes@.len(),
        final(a).nodes@[r.0 as int].kind == AlphaKindM::Entry(entry_type),
        final(a).nodes@[r.0 as int].memory is None || old(a).nodes@.len() > r.0,
        old(a).nodes@.len() <= final(a).nodes@.len() <= old(a).nodes@.len() + 1,
        final(a).next_node_id <= old(a).next_node_id + 1,
        final(a).memory_count == old(a).memory_count,
        par.len() <= r.1@.len(),
        forall|i: int| 0 <= i < old(a).nodes@.len() ==> #[trigger] final(a).nodes@[i] == old(a).nodes@[i],
        forall|i: int| 0 <= i < par.len() ==> #[trigger] r.1@[i] == par[i],
        r.0 >= old(a).nodes@.len() ==> final(a).nodes@[r.0 as int].memory is None,
        forall|k: int| old(a).nodes@.len() <= k < final(a).nodes@.len() ==> (#[trigger] final(a).nodes@[k]).memory is None && k == r.0,
        r.0 < old(a).nodes@.len() ==> final(a).nodes@ == old(a).nodes@ && r.1@ == par,
{
    proof {
        reveal(alpha_wf_parts);
    }
    if let Some(node_id) = a.entry_nodes.get(&entry_type) {
        return (*node_id, Ghost(par));
    }
    let node_id = a.next_node_id;
    a.next_node_id = a.next_node_id + 1;
    let node = AlphaNodeM { kind: AlphaKindM::Entry(entry_type), children: Vec::new(), memory: None };
    let ghost old_nodes = a.nodes@;
    a.nodes.push(node);
    a.entry_nodes.insert(entry_type, node_id);
    let ghost new_par = par.push(None);
    proof {
        let nodes = a.nodes@;
        assert(nodes == old_nodes.push(node));
        assert forall|i: int| 0 <= i < nodes.len() && (#[trigger] new_par[i]) is Some implies new_par[i]->Some_0
            < i && nodes[new_par[i]->Some_0 as int].children@.contains(i as u32) by {
            assert(i < old_nodes.len());
            assert(nodes[new_par[i]->Some_0 as int] == old_nodes[new_par[i]->Some_0 as int]);
        }
        assert forall|p: int, k: int|
            0 <= p < nodes.len() && 0 <= k < nodes[p].children@.len() implies (#[trigger] nodes[p].children@[k]) < nodes.len()
                && new_par[nodes[p].children@[k] as int] == Some(p as u32) by {
            assert(p < old_nodes.len());
            assert(nodes[p] == old_nodes[p]);
        }
        assert forall|i: int, j: int|
            0 <= i < nodes.len() && 0 <= j < nodes.len() && (#[trigger] nodes[i]).memory is Some && nodes[i].memory == (#[trigger] nodes[j]).memory implies i == j by {
            assert(i < old_nodes.len() && j < old_nodes.len());
            assert(nodes[i] == old_nodes[i] && nodes[j] == old_nodes[j]);
        }
    }
    (node_id, Ghost(new_par))
}

proof fn lemma_add_test_node(
    old_nodes: Seq<AlphaNodeM>,
    nodes: Seq<AlphaNodeM>,
    entries: Map<u64, u32>,
    memory_count: u32,
    par: Seq<Option<u32>>,
    parent: int,
    test: u64,
)
    requires
        alpha_wf_parts(old_nodes, entries, memory_count, old_nodes.len() as int, par),
        0 <= parent < old_nodes.len(),
        old_nodes.len() < u32::MAX,
        nodes.len() == old_nodes.len() + 1,
        nodes[old_nodes.len() as int].kind == AlphaKindM::Test(test),
        nodes[old_nodes.len() as int].memory is None,
        nodes[old_nodes.len() as int].children@.len() == 0,
        nodes[parent].kind == old_nodes[parent].kind,
        nodes[parent].memory == old_nodes[parent].memory,
        nodes[parent].children@ == old_nodes[parent].children@.push(old_nodes.len() as u32),
        forall|i: int| 0 <= i < old_nodes.len() && i != parent ==> #[trigger] nodes[i] == old_nodes[i],
    ensures
        alpha_wf_parts(nodes, entries, memory_count, nodes.len() as int, par.push(Some(parent as u32))),
{
    reveal(alpha_wf_parts);
    let new_par = par.push(Some(parent as u32));
    let n = old_nodes.len() as int;
    assert forall|i: int| 0 <= i < nodes.len() implies ((#[trigger] nodes[i]).kind is Entry <==> new_par[i] is None) by {
        if i < n && i != parent {
            assert(nodes[i] == old_nodes[i]);
        }
    }
    assert forall|i: int| 0 <= i < nodes.len() && (#[trigger] new_par[i]) is Some implies new_par[i]->Some_0
        < i && nodes[new_par[i]->Some_0 as int].children@.contains(i as u32) by {
        let p = new_par[i]->Some_0 as int;
        if i < n {
            assert(new_par[i] == par[i]);
            let cs = old_nodes[p].children@;
            let k = choose|k: int| 0 <= k < cs.len() && cs[k] == i as u32;
            if p == parent {
                assert(nodes[p].children@[k] == i as u32);
            } else {
                assert(nodes[p] == old_nodes[p]);
            }
        } else {
            assert(p == parent);
            assert(nodes[p].children@[nodes[p].children@.len() - 1] == i as u32);
        }
    }
    assert forall|p: int, k: int|
        0 <= p < nodes.len() && 0 <= k < nodes[p].children@.len() implies (#[trigger] nodes[p].children@[k]) < nodes.len()
            && new_par[nodes[p].children@[k] as int] == Some(p as u32) by {
        if p == parent {
            if k < old_nodes[p].children@.len() {
                assert(nodes[p].children@[k] == old_nodes[p].children@[k]);
            }
        } else if p < n {
            assert(nodes[p] == old_nodes[p]);
        }
    }
    assert forall|e: u64| #[trigger] entries.contains_key(e) implies entries[e] < nodes.len()
        && nodes[entries[e] as int].kind == AlphaKindM::Entry(e) by {
        let i = entries[e] as int;
        if i != parent {
            assert(nodes[i] == old_nodes[i]);
        }
    }
    assert forall|i: int| 0 <= i < nodes.len() && (#[trigger] nodes[i]).memory is Some implies nodes[i].memory->Some_0
        < memory_count by {
        if i != parent && i < n {
            assert(nodes[i] == old_nodes[i]);
        }
    }
    assert forall|i: int, j: int|
        0 <= i < nodes.len() && 0 <= j < nodes.len() && (#[trigger] nodes[i]).memory is Some && nodes[i].memory == (#[trigger] nodes[j]).memory implies i == j by {
        assert(i < n && j < n);
        if i != parent {
            assert(nodes[i] == old_nodes[i]);
        }
        if j != parent {
            assert(nodes[j] == old_nodes[j]);
        }
    }
}

/// Models `AlphaNetwork::create_constant_test_node` (alpha.rs:508). The
/// parent's `children.push` is done by taking the node out and putting it
/// back (`Vec::remove`/`insert`), which has the same effect on the vector as
/// production's in-place `node_mut(parent)` update.
fn create_constant_test_node(a: &mut AlphaM, parent: u32, test: u64, Ghost(par): Ghost<Seq<Option<u32>>>) -> (r: (
    u32,
    Ghost<Seq<Option<u32>>>,
))
    requires
        alpha_wf(*old(a), par),
        old(a).next_node_id < u32::MAX,
        parent < old(a).nodes@.len(),
    ensures
        alpha_wf(*final(a), r.1@),
        r.0 == old(a).nodes@.len(),
        final(a).nodes@.len() == old(a).nodes@.len() + 1,
        final(a).next_node_id == old(a).next_node_id + 1,
        final(a).memory_count == old(a).memory_count,
        final(a).nodes@[r.0 as int].kind == AlphaKindM::Test(test),
        final(a).nodes@[r.0 as int].memory is None,
        r.1@ == par.push(Some(parent)),
        final(a).entry_nodes@ == old(a).entry_nodes@,
        forall|i: int| 0 <= i < old(a).nodes@.len() ==> (#[trigger] final(a).nodes@[i]).kind == old(a).nodes@[i].kind
            && final(a).nodes@[i].memory == old(a).nodes@[i].memory,
{
    proof {
        reveal(alpha_wf_parts);
    }
    let node_id = a.next_node_id;
    a.next_node_id = a.next_node_id + 1;
    let ghost old_nodes = a.nodes@;
    a.nodes.push(AlphaNodeM { kind: AlphaKindM::Test(test), children: Vec::new(), memory: None });
    let ghost mid = a.nodes@;
    // `if let Some(parent_node) = self.node_mut(parent) { parent_node.children_mut().push(node_id) }`
    let mut parent_node = a.nodes.remove(parent as usize);
    parent_node.children.push(node_id);
    a.nodes.insert(parent as usize, parent_node);
    proof {
        let nodes = a.nodes@;
        assert(nodes =~= mid.update(parent as int, nodes[parent as int]));
        assert forall|i: int| 0 <= i < old_nodes.len() && i != parent implies #[trigger] nodes[i] == old_nodes[i] by {
            assert(nodes[i] == mid[i]);
        }
        assert(nodes[old_nodes.len() as int] == mid[old_nodes.len() as int]);
        lemma_add_test_node(old_nodes, nodes, a.entry_nodes@, a.memory_count, par, parent as int, test);
        assert forall|i: int| 0 <= i < old_nodes.len() implies (#[trigger] nodes[i]).kind == old_nodes[i].kind
            && nodes[i].memory == old_nodes[i].memory by {
            if i != parent {
                assert(nodes[i] == old_nodes[i]);
            }
        }
    }
    (node_id, Ghost(par.push(Some(parent))))
}

/// Models `AlphaNetwork::create_memory` (alpha.rs:533) on a node that has no
/// memory yet (the only way `ensure_alpha_path` calls it, proved there).
fn create_memory(a: &mut AlphaM, node_id: u32, Ghost(par): Ghost<Seq<Option<u32>>>) -> (memory_id: u32)
    requires
        alpha_wf(*old(a), par),
        old(a).memory_count < u32::MAX,
        node_id < old(a).nodes@.len(),
        old(a).nodes@[node_id as int].memory is None,
    ensures
        alpha_wf(*final(a), par),
        memory_id == old(a).memory_count,
        final(a).memory_count == old(a).memory_count + 1,
        final(a).next_node_id == old(a).next_node_id,
        final(a).entry_nodes@ == old(a).entry_nodes@,
        final(a).nodes@.len() == old(a).nodes@.len(),
        final(a).nodes@[node_id as int].memory == Some(memory_id),
        forall|i: int| 0 <= i < old(a).nodes@.len() ==> (#[trigger] final(a).nodes@[i]).kind == old(a).nodes@[i].kind,
        forall|i: int| 0 <= i < old(a).nodes@.len() && i != node_id ==> (#[trigger] final(a).nodes@[i]) == old(a).nodes@[i],
{
    proof {
        reveal(alpha_wf_parts);
    }
    let memory_id = a.memory_count;
    a.memory_count = a.memory_count + 1;
    let ghost old_nodes = a.nodes@;
    // `if let Some(node) = self.node_mut(node_id) { *node.memory_mut() = Some(memory_id) }`
    let mut node = a.nodes.remove(node_id as usize);
    node.memory = Some(memory_id);
    a.nodes.insert(node_id as usize, node);
    proof {
        let nodes = a.nodes@;
        assert(nodes =~= old_nodes.update(node_id as int, nodes[node_id as int]));
        assert forall|i: int| 0 <= i < old_nodes.len() && i != node_id implies #[trigger] nodes[i] == old_nodes[i] by {}
        assert forall|i: int| 0 <= i < nodes.len() implies ((#[trigger] nodes[i]).kind is Entry <==> par[i] is None) by {
            assert(nodes[i].kind == old_nodes[i].kind);
        }
        assert forall|i: int| 0 <= i < nodes.len() && (#[trigger] par[i]) is Some implies par[i]->Some_0
            < i && nodes[par[i]->Some_0 as int].children@.contains(i as u32) by {
            assert(nodes[par[i]->Some_0 as int].children@ == old_nodes[par[i]->Some_0 as int].children@);
        }
        assert forall|p: int, k: int|
            0 <= p < nodes.len() && 0 <= k < nodes[p].children@.len() implies (#[trigger] nodes[p].children@[k]) < nodes.len()
                && par[nodes[p].children@[k] as int] == Some(p as u32) by {
            assert(nodes[p].children@ == old_nodes[p].children@);
        }
        assert forall|e: u64| #[trigger] a.entry_nodes@.contains_key(e) implies a.entry_nodes@[e] < nodes.len()
            && nodes[a.entry_nodes@[e] as int].kind == AlphaKindM::Entry(e) by {
            assert(nodes[a.entry_nodes@[e] as int].kind == old_nodes[a.entry_nodes@[e] as int].kind);
        }
        assert forall|i: int| 0 <= i < nodes.len() && (#[trigger] nodes[i]).memory is Some implies nodes[i].memory->Some_0
            < a.memory_count by {
            if i != node_id {
                assert(nodes[i] == old_nodes[i]);
            }
        }
        assert forall|i: int, j: int|
            0 <= i < nodes.len() && 0 <= j < nodes.len() && (#[trigger] nodes[i]).memory is Some && nodes[i].memory == (#[trigger] nodes[j]).memory implies i == j by {
            if i != node_id {
                assert(nodes[i] == old_nodes[i]);
            }
            if j != node_id {
                assert(nodes[j] == old_nodes[j]);
            }
        }
    }
    memory_id
}

/// `AlphaPathKey` equality (derived `PartialEq`, compiler.rs:152).
fn key_eq(a: &AlphaKeyM, b: &AlphaKeyM) -> (r: bool)
    ensures
        r == (kv(*a) == kv(*b)),
{
    if a.entry != b.entry || a.tests.len() != b.tests.len() {
        return false;
    }
    let mut i: usize = 0;
    while i < a.tests.len()
        invariant
            a.tests.len() == b.tests.len(),
            i <= a.tests.len(),
            forall|k: int| 0 <= k < i ==> a.tests@[k] == b.tests@[k],
        decreases a.tests.len() - i,
    {
        if a.tests[i] != b.tests[i] {
            return false;
        }
        i += 1;
    }
    assert(a.tests@ =~= b.tests@);
    true
}

/// `alpha_path_cache.get(&key)` (compiler.rs:805) under map semantics.
fn cache_get(cache: &Vec<(AlphaKeyM, u32)>, key: &AlphaKeyM) -> (r: Option<u32>)
    ensures
        r matches Some(m) ==> exists|j: int| 0 <= j < cache@.len() && kv((#[trigger] cache@[j]).0) == kv(*key) && cache@[j].1 == m,
        r is None ==> forall|j: int| 0 <= j < cache@.len() ==> kv((#[trigger] cache@[j]).0) != kv(*key),
{
    let mut j: usize = 0;
    while j < cache.len()
        invariant
            j <= cache@.len(),
            forall|l: int| 0 <= l < j ==> kv((#[trigger] cache@[l]).0) != kv(*key),
        decreases cache@.len() - j,
    {
        if key_eq(&cache[j].0, key) {
            return Some(cache[j].1);
        }
        j += 1;
    }
    None
}

/// `pattern.constant_tests.clone()` (compiler.rs:787).
fn clone_tests(tests: &Vec<u64>) -> (r: Vec<u64>)
    ensures
        r@ == tests@,
{
    let mut r: Vec<u64> = Vec::new();
    let mut i: usize = 0;
    while i < tests.len()
        invariant
            i <= tests.len(),
            r@ == tests@.take(i as int),
        decreases tests.len() - i,
    {
        r.push(tests[i]);
        i += 1;
        proof {
            assert(tests@.take(i as int) =~= tests@.take(i - 1).push(tests@[i - 1]));
        }
    }
    assert(tests@.take(tests.len() as int) =~= tests@);
    r
}

pub open spec fn state_wf(c: CompilerM, a: AlphaM, par: Seq<Option<u32>>) -> bool {
    alpha_wf(a, par) && cache_wf(c.alpha_path_cache@, a, par)
}

/// Models `ReteCompiler::ensure_alpha_path` (compiler.rs:781) on an empty
/// fact base (backfill does not change the network's structure). `tests`
/// is the pattern's constant tests with the sequence test already appended
/// (compiler.rs:791-799).
fn ensure_alpha_path(
    compiler: &mut CompilerM,
    alpha: &mut AlphaM,
    entry_type: u64,
    tests: &Vec<u64>,
    Ghost(par): Ghost<Seq<Option<u32>>>,
) -> (r: (u32, Ghost<Seq<Option<u32>>>))
    requires
        state_wf(*old(compiler), *old(alpha), par),
        old(alpha).next_node_id as int + 1 + tests@.len() <= u32::MAX,
        old(alpha).memory_count < u32::MAX,
    ensures
        state_wf(*final(compiler), *final(alpha), r.1@),
        // The returned memory is the one cached under exactly this key.
        exists|j: int| 0 <= j < final(compiler).alpha_path_cache@.len() && kv((#[trigger] final(compiler).alpha_path_cache@[j]).0) == (entry_type, tests@)
            && final(compiler).alpha_path_cache@[j].1 == r.0,
        // Growth: at most one entry node, one node per test, one memory.
        final(alpha).next_node_id as int <= old(alpha).next_node_id as int + 1 + tests@.len(),
        final(alpha).memory_count as int <= old(alpha).memory_count as int + 1,
        // The cache only grows.
        old(compiler).alpha_path_cache@.len() <= final(compiler).alpha_path_cache@.len(),
        forall|j: int| 0 <= j < old(compiler).alpha_path_cache@.len() ==> #[trigger] final(compiler).alpha_path_cache@[j] == old(compiler).alpha_path_cache@[j],
        // A cached key reuses its path: nothing in the network changes.
        (exists|j: int| 0 <= j < old(compiler).alpha_path_cache@.len() && kv((#[trigger] old(compiler).alpha_path_cache@[j]).0) == (entry_type, tests@))
            ==> final(alpha).nodes@ == old(alpha).nodes@ && final(alpha).memory_count == old(alpha).memory_count
                && final(compiler).alpha_path_cache@ == old(compiler).alpha_path_cache@,
{
    let key = AlphaKeyM { entry: entry_type, tests: clone_tests(tests) };
    if let Some(mem_id) = cache_get(&compiler.alpha_path_cache, &key) {
        return (mem_id, Ghost(par));
    }
    let ghost c0 = compiler.alpha_path_cache@;
    let ghost a0 = alpha.nodes@;
    let (entry_node, Ghost(par1)) = create_entry_node(alpha, entry_type, Ghost(par));
    proof {
        lemma_alpha_facts(*alpha, par1);
        lemma_spells_entry(alpha.nodes@, par1, entry_node as int, entry_type);
    }
    let mut current_node = entry_node;
    let ghost mut cur_par = par1;
    let mut i: usize = 0;
    while i < key.tests.len()
        invariant
            key.entry == entry_type,
            key.tests@ == tests@,
            i <= tests@.len(),
            alpha_wf(*alpha, cur_par),
            alpha.next_node_id as int + tests@.len() - i <= u32::MAX,
            alpha.next_node_id as int <= old(alpha).next_node_id as int + 1 + i,
            alpha.memory_count == old(alpha).memory_count,
            a0.len() <= alpha.nodes@.len(),
            par.len() <= cur_par.len(),
            forall|k: int| 0 <= k < a0.len() ==> (#[trigger] alpha.nodes@[k]).kind == a0[k].kind && alpha.nodes@[k].memory == a0[k].memory,
            forall|k: int| 0 <= k < par.len() ==> #[trigger] cur_par[k] == par[k],
            spells(alpha.nodes@, cur_par, current_node as int, entry_type, tests@.take(i as int)),
            forall|k: int| a0.len() <= k < alpha.nodes@.len() ==> (#[trigger] alpha.nodes@[k]).memory is None,
            current_node < a0.len() ==> i == 0 && alpha.nodes@[current_node as int].kind == AlphaKindM::Entry(entry_type),
        decreases tests@.len() - i,
    {
        proof {
            lemma_alpha_facts(*alpha, cur_par);
        }
        let ghost n_before = alpha.nodes@;
        let ghost p_before = cur_par;
        let (next, Ghost(p2)) = create_constant_test_node(alpha, current_node, key.tests[i], Ghost(cur_par));
        proof {
            let t = tests@.take(i + 1);
            assert(t.drop_last() =~= tests@.take(i as int));
            assert(t.last() == tests@[i as int]);
            lemma_alpha_facts(*alpha, p2);
            lemma_spells_frame(n_before, p_before, alpha.nodes@, p2, current_node as int, entry_type, tests@.take(i as int));
            assert(spells(alpha.nodes@, p2, next as int, entry_type, t));
            cur_par = p2;
        }
        current_node = next;
        i += 1;
    }
    proof {
        assert(tests@.take(tests@.len() as int) =~= tests@);
        // An entry node reached with no tests has no memory: otherwise its
        // memory would be cached under (entry_type, []) = this key.
        lemma_cache_facts(c0, *old(alpha), par);
        if current_node < a0.len() {
            let o = current_node as int;
            if alpha.nodes@[o].memory is Some {
                assert(a0[o].memory is Some);
                assert(owned_by_cache(c0, *old(alpha), par, o));
                let j = choose|j: int| 0 <= j < c0.len() && (#[trigger] c0[j]).1 == a0[o].memory->Some_0
                    && spells(a0, par, o, c0[j].0.entry, c0[j].0.tests@);
                lemma_spells_entry(a0, par, o, entry_type);
                lemma_spells_unique(a0, par, o, c0[j].0.entry, c0[j].0.tests@, entry_type, seq![]);
                assert(tests@ =~= seq![]);
                assert(kv(c0[j].0) == kv(key));
            }
        }
    }
    let ghost n_before = alpha.nodes@;
    let mem_id = create_memory(alpha, current_node, Ghost(cur_par));
    compiler.alpha_path_cache.push((key, mem_id));
    proof {
        let c1 = compiler.alpha_path_cache@;
        assert(c1.len() == c0.len() + 1);
        assert(forall|j: int| 0 <= j < c0.len() ==> #[trigger] c1[j] == c0[j]);
        assert(kv(c1[c0.len() as int].0) == (entry_type, tests@));
        assert(c1[c0.len() as int].1 == mem_id);
        lemma_cache_after_insert(c0, c1, *old(alpha), par, n_before, *alpha, cur_par, current_node as int, entry_type, tests@);
    }
    (mem_id, Ghost(cur_par))
}

proof fn lemma_spells_entry(nodes: Seq<AlphaNodeM>, par: Seq<Option<u32>>, o: int, e: u64)
    requires
        0 <= o < nodes.len(),
        o < par.len(),
        nodes[o].kind == AlphaKindM::Entry(e),
    ensures
        spells(nodes, par, o, e, seq![]),
{
}

#[verifier::rlimit(60)]
proof fn lemma_cache_after_insert(
    c0: Seq<(AlphaKeyM, u32)>,
    c1: Seq<(AlphaKeyM, u32)>,
    a_old: AlphaM,
    par: Seq<Option<u32>>,
    n_before: Seq<AlphaNodeM>,
    a_final: AlphaM,
    cur_par: Seq<Option<u32>>,
    cur: int,
    entry: u64,
    tests: Seq<u64>,
)
    requires
        cache_wf(c0, a_old, par),
        alpha_wf(a_old, par),
        alpha_wf(a_final, cur_par),
        c1.len() == c0.len() + 1,
        forall|j: int| 0 <= j < c0.len() ==> #[trigger] c1[j] == c0[j],
        kv(c1[c0.len() as int].0) == (entry, tests),
        c1[c0.len() as int].1 == a_old.memory_count,
        forall|j: int| 0 <= j < c0.len() ==> kv((#[trigger] c0[j]).0) != (entry, tests),
        a_final.memory_count == a_old.memory_count + 1,
        a_old.nodes@.len() <= n_before.len(),
        n_before.len() == a_final.nodes@.len(),
        forall|k: int| 0 <= k < a_old.nodes@.len() ==> (#[trigger] n_before[k]).kind == a_old.nodes@[k].kind
            && n_before[k].memory == a_old.nodes@[k].memory,
        forall|k: int| 0 <= k < n_before.len() ==> (#[trigger] a_final.nodes@[k]).kind == n_before[k].kind,
        forall|k: int| 0 <= k < n_before.len() && k != cur ==> (#[trigger] a_final.nodes@[k]) == n_before[k],
        forall|k: int| a_old.nodes@.len() <= k < n_before.len() ==> (#[trigger] n_before[k]).memory is None,
        0 <= cur < n_before.len(),
        a_final.nodes@[cur].memory == Some(a_old.memory_count),
        cur < a_old.nodes@.len() ==> a_old.nodes@[cur].memory is None,
        spells(n_before, cur_par, cur, entry, tests),
        par.len() <= cur_par.len(),
        forall|k: int| 0 <= k < par.len() ==> #[trigger] cur_par[k] == par[k],
    ensures
        cache_wf(c1, a_final, cur_par),
{
    reveal(alpha_wf_parts);
    reveal(cache_wf);
    let a0 = a_old.nodes@;
    let nf = a_final.nodes@;
    let n = c0.len() as int;
    assert forall|k: int| 0 <= k < a0.len() implies (#[trigger] nf[k]).kind == a0[k].kind by {
        assert(nf[k].kind == n_before[k].kind);
    }
    lemma_spells_frame(n_before, cur_par, nf, cur_par, cur, entry, tests);
    assert forall|j: int| 0 <= j < c1.len() implies (#[trigger] c1[j]).1 < a_final.memory_count by {
        if j < n {
            assert(c1[j] == c0[j]);
        }
    }
    assert forall|j: int, l: int| 0 <= j < l < c1.len() implies kv((#[trigger] c1[j]).0) != kv((#[trigger] c1[l]).0) by {
        assert(c1[j] == c0[j]);
        if l < n {
            assert(c1[l] == c0[l]);
        }
    }
    assert forall|j: int, l: int| 0 <= j < l < c1.len() implies (#[trigger] c1[j]).1 != (#[trigger] c1[l]).1 by {
        assert(c1[j] == c0[j]);
        if l < n {
            assert(c1[l] == c0[l]);
        }
    }
    assert forall|j: int| 0 <= j < c1.len() implies #[trigger] has_owner(c1, a_final, cur_par, j) by {
        if j < n {
            assert(c1[j] == c0[j]);
            assert(has_owner(c0, a_old, par, j));
            let o = choose|o: int|
                0 <= o < a0.len() && (#[trigger] a0[o]).memory == Some(c0[j].1)
                    && spells(a0, par, o, c0[j].0.entry, c0[j].0.tests@);
            assert(o != cur);
            assert(nf[o] == n_before[o]);
            lemma_spells_frame(a0, par, nf, cur_par, o, c0[j].0.entry, c0[j].0.tests@);
            assert(nf[o].memory == Some(c1[j].1));
        } else {
            assert(nf[cur].memory == Some(c1[j].1));
        }
    }
    assert forall|o: int| 0 <= o < nf.len() && (#[trigger] nf[o]).memory is Some implies owned_by_cache(c1, a_final, cur_par, o) by {
        if o == cur {
            assert(c1[n].1 == nf[o].memory->Some_0);
        } else {
            assert(nf[o] == n_before[o]);
            assert(o < a0.len());
            assert(a0[o].memory is Some);
            assert(owned_by_cache(c0, a_old, par, o));
            let j = choose|j: int|
                0 <= j < c0.len() && (#[trigger] c0[j]).1 == a0[o].memory->Some_0
                    && spells(a0, par, o, c0[j].0.entry, c0[j].0.tests@);
            lemma_spells_frame(a0, par, nf, cur_par, o, c0[j].0.entry, c0[j].0.tests@);
            assert(c1[j] == c0[j]);
        }
    }
}

/// The empty network and cache satisfy the invariants
/// (`AlphaNetwork::new`, alpha.rs:383; `ReteCompiler::new`, compiler.rs:241).
fn new_alpha_state() -> (r: (CompilerM, AlphaM, Ghost<Seq<Option<u32>>>))
    ensures
        state_wf(r.0, r.1, r.2@),
        r.1.next_node_id == 0,
        r.1.memory_count == 0,
{
    let compiler = CompilerM { alpha_path_cache: Vec::new() };
    let alpha = AlphaM {
        nodes: Vec::new(),
        memory_count: 0,
        entry_nodes: HashMapWithView::new(),
        next_node_id: 0,
    };
    proof {
        reveal(alpha_wf_parts);
        reveal(cache_wf);
    }
    (compiler, alpha, Ghost(Seq::empty()))
}

proof fn lemma_cache_distinct(c: Seq<(AlphaKeyM, u32)>, a: AlphaM, par: Seq<Option<u32>>)
    requires
        cache_wf(c, a, par),
    ensures
        forall|j: int, l: int| 0 <= j < l < c.len() ==> kv((#[trigger] c[j]).0) != kv((#[trigger] c[l]).0),
        forall|j: int, l: int| 0 <= j < l < c.len() ==> (#[trigger] c[j]).1 != (#[trigger] c[l]).1,
{
    reveal(cache_wf);
}

/// Facts of the invariant that the snapshot check relies on.
proof fn lemma_snapshot_facts(c: Seq<(AlphaKeyM, u32)>, a: AlphaM, par: Seq<Option<u32>>)
    requires
        alpha_wf(a, par),
        cache_wf(c, a, par),
    ensures
        par.len() == a.nodes@.len(),
        a.nodes@.len() == a.next_node_id,
        c.len() == a.memory_count,
        forall|p: int, k: int|
            0 <= p < a.nodes@.len() && 0 <= k < a.nodes@[p].children@.len() ==> (#[trigger] a.nodes@[p].children@[k]) < a.nodes@.len()
                && par[a.nodes@[p].children@[k] as int] == Some(p as u32),
        forall|i: int|
            0 <= i < a.nodes@.len() && (#[trigger] par[i]) is Some ==> par[i]->Some_0 < i && a.nodes@[par[i]->Some_0 as int].children@.contains(i as u32),
        forall|i: int| 0 <= i < a.nodes@.len() && (#[trigger] a.nodes@[i]).memory is Some ==> a.nodes@[i].memory->Some_0 < a.memory_count,
        forall|i: int, j: int|
            0 <= i < a.nodes@.len() && 0 <= j < a.nodes@.len() && (#[trigger] a.nodes@[i]).memory is Some && a.nodes@[i].memory == (#[trigger] a.nodes@[j]).memory ==> i == j,
        forall|j: int, l: int| 0 <= j < l < c.len() ==> (#[trigger] c[j]).1 != (#[trigger] c[l]).1,
        forall|j: int| 0 <= j < c.len() ==> #[trigger] has_owner(c, a, par, j),
        forall|j: int| 0 <= j < c.len() ==> (#[trigger] c[j]).1 < a.memory_count,
{
    lemma_alpha_facts(a, par);
    lemma_cache_facts(c, a, par);
}

proof fn lemma_alpha_facts(a: AlphaM, par: Seq<Option<u32>>)
    requires
        alpha_wf(a, par),
    ensures
        par.len() == a.nodes@.len(),
        a.nodes@.len() == a.next_node_id,
        forall|p: int, k: int|
            0 <= p < a.nodes@.len() && 0 <= k < a.nodes@[p].children@.len() ==> (#[trigger] a.nodes@[p].children@[k]) < a.nodes@.len()
                && par[a.nodes@[p].children@[k] as int] == Some(p as u32),
        forall|i: int|
            0 <= i < a.nodes@.len() && (#[trigger] par[i]) is Some ==> par[i]->Some_0 < i && a.nodes@[par[i]->Some_0 as int].children@.contains(i as u32),
        forall|i: int| 0 <= i < a.nodes@.len() && (#[trigger] a.nodes@[i]).memory is Some ==> a.nodes@[i].memory->Some_0 < a.memory_count,
        forall|i: int, j: int|
            0 <= i < a.nodes@.len() && 0 <= j < a.nodes@.len() && (#[trigger] a.nodes@[i]).memory is Some && a.nodes@[i].memory == (#[trigger] a.nodes@[j]).memory ==> i == j,
{
    reveal(alpha_wf_parts);
}

proof fn lemma_cache_facts(c: Seq<(AlphaKeyM, u32)>, a: AlphaM, par: Seq<Option<u32>>)
    requires
        cache_wf(c, a, par),
    ensures
        c.len() == a.memory_count,
        forall|j: int, l: int| 0 <= j < l < c.len() ==> (#[trigger] c[j]).1 != (#[trigger] c[l]).1,
        forall|j: int| 0 <= j < c.len() ==> #[trigger] has_owner(c, a, par, j),
        forall|o: int| 0 <= o < a.nodes@.len() && (#[trigger] a.nodes@[o]).memory is Some ==> owned_by_cache(c, a, par, o),
        forall|j: int| 0 <= j < c.len() ==> (#[trigger] c[j]).1 < a.memory_count,
{
    reveal(cache_wf);
}

/// Theorem (alpha sharing is exact): two patterns, compiled one after the
/// other from any reachable state, receive the same alpha memory if and only
/// if their alpha path keys (entry type and full test list) are equal.
fn theorem_alpha_sharing_exact(
    compiler: &mut CompilerM,
    alpha: &mut AlphaM,
    Ghost(par): Ghost<Seq<Option<u32>>>,
    e1: u64,
    t1: &Vec<u64>,
    e2: u64,
    t2: &Vec<u64>,
) -> (r: (u32, u32, Ghost<Seq<Option<u32>>>))
    requires
        state_wf(*old(compiler), *old(alpha), par),
        old(alpha).next_node_id as int + 2 + t1@.len() + t2@.len() <= u32::MAX,
        old(alpha).memory_count as int + 2 <= u32::MAX,
    ensures
        state_wf(*final(compiler), *final(alpha), r.2@),
        (r.0 == r.1) == ((e1, t1@) == (e2, t2@)),
{
    let (m1, Ghost(p1)) = ensure_alpha_path(compiler, alpha, e1, t1, Ghost(par));
    let ghost c1 = compiler.alpha_path_cache@;
    let (m2, Ghost(p2)) = ensure_alpha_path(compiler, alpha, e2, t2, Ghost(p1));
    proof {
        let c2 = compiler.alpha_path_cache@;
        let j1 = choose|j: int| 0 <= j < c1.len() && kv((#[trigger] c1[j]).0) == (e1, t1@) && c1[j].1 == m1;
        let j2 = choose|j: int| 0 <= j < c2.len() && kv((#[trigger] c2[j]).0) == (e2, t2@) && c2[j].1 == m2;
        lemma_cache_distinct(c2, *alpha, p2);
        assert(c2[j1] == c1[j1]);
        if j1 != j2 {
            if j1 < j2 {
                assert(kv(c2[j1].0) != kv(c2[j2].0));
                assert(c2[j1].1 != c2[j2].1);
            } else {
                assert(kv(c2[j2].0) != kv(c2[j1].0));
                assert(c2[j2].1 != c2[j1].1);
            }
        }
    }
    (m1, m2, Ghost(p2))
}

/// Theorem (recompilation reuses the alpha path): compiling the same alpha
/// key a second time returns the same memory and changes neither the alpha
/// network nor the cache.
fn theorem_alpha_recompile_reuses(
    compiler: &mut CompilerM,
    alpha: &mut AlphaM,
    Ghost(par): Ghost<Seq<Option<u32>>>,
    entry: u64,
    tests: &Vec<u64>,
) -> (r: (u32, u32, Ghost<Seq<Option<u32>>>))
    requires
        state_wf(*old(compiler), *old(alpha), par),
        old(alpha).next_node_id as int + 2 + 2 * tests@.len() <= u32::MAX,
        old(alpha).memory_count as int + 2 <= u32::MAX,
    ensures
        state_wf(*final(compiler), *final(alpha), r.2@),
        r.0 == r.1,
{
    let (m1, Ghost(p1)) = ensure_alpha_path(compiler, alpha, entry, tests, Ghost(par));
    let ghost nodes1 = alpha.nodes@;
    let ghost c1 = compiler.alpha_path_cache@;
    let (m2, Ghost(p2)) = ensure_alpha_path(compiler, alpha, entry, tests, Ghost(p1));
    proof {
        assert(alpha.nodes@ == nodes1);
        assert(compiler.alpha_path_cache@ == c1);
        let j1 = choose|j: int| 0 <= j < c1.len() && kv((#[trigger] c1[j]).0) == (entry, tests@) && c1[j].1 == m1;
        let j2 = choose|j: int| 0 <= j < c1.len() && kv((#[trigger] c1[j]).0) == (entry, tests@) && c1[j].1 == m2;
        lemma_cache_distinct(c1, *alpha, p2);
        if j1 != j2 {
            if j1 < j2 {
                assert(kv(c1[j1].0) != kv(c1[j2].0));
            } else {
                assert(kv(c1[j2].0) != kv(c1[j1].0));
            }
        }
    }
    (m1, m2, Ghost(p2))
}

// ---------------------------------------------------------------------------
// Snapshot check of the alpha path cache
// ---------------------------------------------------------------------------

pub open spec fn parents_upto(nodes: Seq<AlphaNodeM>, par: Seq<Option<u32>>, parents: Seq<Option<u32>>, index: int, k: int) -> bool {
    &&& parents.len() == nodes.len()
    &&& forall|c: int| 0 <= c < nodes.len() ==> #[trigger] parents[c] == if par[c] is Some && (par[c]->Some_0 < index
        || (par[c]->Some_0 == index && nodes[index].children@.take(k).contains(c as u32))) {
        par[c]
    } else {
        None
    }
}

pub open spec fn owners_upto(nodes: Seq<AlphaNodeM>, owners: Seq<Option<u32>>, index: int) -> bool {
    forall|m: int| 0 <= m < owners.len() ==> match #[trigger] owners[m] {
        Some(o) => o < index && nodes[o as int].memory == Some(m as u32),
        None => forall|o: int| 0 <= o < index ==> (#[trigger] nodes[o]).memory != Some(m as u32),
    }
}

/// Models the alpha-cache part of `ReteCompiler::validate_snapshot_checked`
/// (snapshot.rs:2022-2076): the cache size check, the `parents`/`owners`
/// reconstruction from children lists and memory fields, and, for every
/// cached key, the duplicate-memory check and the walk that matches the
/// key's tests backwards and ends at the key's entry node. The work budget
/// (`Work::spend`, a resource limit) is omitted. Returns `true` for `Ok`.
fn validate_alpha_cache_snapshot(
    compiler: &CompilerM,
    alpha: &AlphaM,
    Ghost(par): Ghost<Seq<Option<u32>>>,
) -> (ok: bool)
    requires
        state_wf(*compiler, *alpha, par),
    ensures
        ok,
{
    let ghost nodes = alpha.nodes@;
    let ghost cache = compiler.alpha_path_cache@;
    proof {
        lemma_snapshot_facts(cache, *alpha, par);
    }
    // require_eq!(self.alpha_path_cache.len(), rete.alpha.memories.len());
    if compiler.alpha_path_cache.len() != alpha.memory_count as usize {
        return false;
    }
    let mut parents: Vec<Option<u32>> = Vec::new();
    let mut i: usize = 0;
    while i < alpha.nodes.len()
        invariant
            i <= alpha.nodes.len(),
            parents@ == Seq::new(i as nat, |c: int| None::<u32>),
        decreases alpha.nodes.len() - i,
    {
        parents.push(None);
        i += 1;
        proof {
            assert(parents@ =~= Seq::new(i as nat, |c: int| None::<u32>));
        }
    }
    let mut owners: Vec<Option<u32>> = Vec::new();
    let mut i: usize = 0;
    while i < alpha.memory_count as usize
        invariant
            i <= alpha.memory_count,
            owners@ == Seq::new(i as nat, |m: int| None::<u32>),
        decreases alpha.memory_count - i,
    {
        owners.push(None);
        i += 1;
        proof {
            assert(owners@ =~= Seq::new(i as nat, |m: int| None::<u32>));
        }
    }
    let mut index: usize = 0;
    while index < alpha.nodes.len()
        invariant
            nodes == alpha.nodes@,
            cache == compiler.alpha_path_cache@,
            state_wf(*compiler, *alpha, par),
            index <= nodes.len(),
            parents_upto(nodes, par, parents@, index as int, 0),
            owners@.len() == alpha.memory_count,
            owners_upto(nodes, owners@, index as int),
        decreases nodes.len() - index,
    {
        proof {
            lemma_snapshot_facts(cache, *alpha, par);
        }
        let id = index as u32;
        let mut k: usize = 0;
        while k < alpha.nodes[index].children.len()
            invariant
                nodes == alpha.nodes@,
                state_wf(*compiler, *alpha, par),
                cache == compiler.alpha_path_cache@,
                index < nodes.len(),
                id == index,
                k <= nodes[index as int].children@.len(),
                parents_upto(nodes, par, parents@, index as int, k as int),
            decreases nodes[index as int].children@.len() - k,
        {
            proof {
                lemma_snapshot_facts(cache, *alpha, par);
            }
            let child = alpha.nodes[index].children[k];
            // `*parents.get_mut(child.0 as usize).ok_or("dangling alpha child")? = Some(id);`
            if child as usize >= parents.len() {
                return false;
            }
            parents.set(child as usize, Some(id));
            k += 1;
            proof {
                let cs = nodes[index as int].children@;
                assert(par[child as int] == Some(id));
                assert(cs.take(k as int) =~= cs.take(k - 1).push(child));
                assert forall|c: int| 0 <= c < nodes.len() implies #[trigger] parents@[c] == if par[c] is Some && (par[c]->Some_0 < index
                    || (par[c]->Some_0 == index && cs.take(k as int).contains(c as u32))) {
                    par[c]
                } else {
                    None
                } by {
                    if c != child as int {
                        if cs.take(k as int).contains(c as u32) {
                            let w = choose|w: int| 0 <= w < k && cs.take(k as int)[w] == c as u32;
                            assert(w < k - 1);
                            assert(cs.take(k - 1)[w] == c as u32);
                        }
                    } else {
                        assert(cs.take(k as int)[k - 1] == c as u32);
                    }
                }
            }
        }
        proof {
            let cs = nodes[index as int].children@;
            assert(cs.take(cs.len() as int) =~= cs);
            assert forall|c: int| 0 <= c < nodes.len() implies #[trigger] parents@[c] == if par[c] is Some && (par[c]->Some_0 < index + 1
                || (par[c]->Some_0 == index + 1 && nodes[index + 1].children@.take(0).contains(c as u32))) {
                par[c]
            } else {
                None
            } by {
                if par[c] is Some && par[c]->Some_0 == index {
                    assert(nodes[par[c]->Some_0 as int].children@.contains(c as u32));
                }
            }
        }
        // `if let Some(memory) = memory { *owners.get_mut(memory.0 as usize).ok_or(..)? = Some(id) }`
        if let Some(memory) = alpha.nodes[index].memory {
            if memory as usize >= owners.len() {
                return false;
            }
            owners.set(memory as usize, Some(id));
        }
        index += 1;
        proof {
            assert forall|m: int| 0 <= m < owners@.len() implies match #[trigger] owners@[m] {
                Some(o) => o < index && nodes[o as int].memory == Some(m as u32),
                None => forall|o: int| 0 <= o < index ==> (#[trigger] nodes[o]).memory != Some(m as u32),
            } by {
                if nodes[index - 1].memory == Some(m as u32) {
                    assert(owners@[m] == Some(id));
                }
            }
        }
    }
    let mut cached_memories: HashSetWithView<u32> = HashSetWithView::new();
    let mut j: usize = 0;
    while j < compiler.alpha_path_cache.len()
        invariant
            nodes == alpha.nodes@,
            cache == compiler.alpha_path_cache@,
            state_wf(*compiler, *alpha, par),
            j <= cache.len(),
            parents_upto(nodes, par, parents@, nodes.len() as int, 0),
            owners@.len() == alpha.memory_count,
            owners_upto(nodes, owners@, nodes.len() as int),
            forall|m: u32| #[trigger] cached_memories@.contains(m) <==> exists|l: int| 0 <= l < j && (#[trigger] cache[l]).1 == m,
        decreases cache.len() - j,
    {
        proof {
            lemma_snapshot_facts(cache, *alpha, par);
        }
        let memory = compiler.alpha_path_cache[j].1;
        let ghost key = cache[j as int].0;
        // require!(cached_memories.insert(*memory), "duplicate cached alpha memory");
        proof {
            if cached_memories@.contains(memory) {
                let l = choose|l: int| 0 <= l < j && (#[trigger] cache[l]).1 == memory;
                assert(cache[l].1 != cache[j as int].1);
            }
        }
        if !cached_memories.insert(memory) {
            return false;
        }
        proof {
            assert forall|m: u32| #[trigger] cached_memories@.contains(m) <==> exists|l: int| 0 <= l < j + 1 && (#[trigger] cache[l]).1 == m by {
                if m == memory {
                    assert(cache[j as int].1 == m);
                }
            }
        }
        // let mut id = owners.get(memory).copied().flatten().ok_or(..)?;
        proof {
            assert(cache[j as int].1 < alpha.memory_count);
        }
        if memory as usize >= owners.len() {
            return false;
        }
        let mut id = match owners[memory as usize] {
            Some(id) => id,
            None => {
                proof {
                    assert(has_owner(cache, *alpha, par, j as int));
                    let o = choose|o: int| 0 <= o < nodes.len() && (#[trigger] nodes[o]).memory == Some(cache[j as int].1)
                        && spells(nodes, par, o, cache[j as int].0.entry, cache[j as int].0.tests@);
                    assert(nodes[o].memory == Some(memory));
                }
                return false;
            },
        };
        proof {
            assert(has_owner(cache, *alpha, par, j as int));
            let o = choose|o: int| 0 <= o < nodes.len() && (#[trigger] nodes[o]).memory == Some(cache[j as int].1)
                && spells(nodes, par, o, cache[j as int].0.entry, cache[j as int].0.tests@);
            assert(nodes[id as int].memory == Some(memory));
            assert(id as int == o);
        }
        let tests = &compiler.alpha_path_cache[j].0.tests;
        let mut remaining: usize = tests.len();
        proof {
            assert(has_owner(cache, *alpha, par, j as int));
            let o = choose|o: int| 0 <= o < nodes.len() && (#[trigger] nodes[o]).memory == Some(cache[j as int].1)
                && spells(nodes, par, o, cache[j as int].0.entry, cache[j as int].0.tests@);
            assert(id as int == o);
            assert(tests@.take(remaining as int) =~= tests@);
        }
        // for expected in key.tests.iter().rev()
        while remaining > 0
            invariant
                nodes == alpha.nodes@,
                state_wf(*compiler, *alpha, par),
                cache == compiler.alpha_path_cache@,
                j < cache.len(),
                key == cache[j as int].0,
                tests@ == key.tests@,
                remaining <= tests@.len(),
                parents_upto(nodes, par, parents@, nodes.len() as int, 0),
                spells(nodes, par, id as int, key.entry, tests@.take(remaining as int)),
            decreases remaining,
        {
            proof {
                lemma_snapshot_facts(cache, *alpha, par);
            }
            let expected = tests[remaining - 1];
            let ghost t = tests@.take(remaining as int);
            proof {
                assert(t.last() == expected);
                assert(t.drop_last() =~= tests@.take(remaining - 1));
            }
            // require!(matches!(nodes.get(id), Some(ConstantTest { test, .. }) if test == expected))
            if id as usize >= alpha.nodes.len() {
                return false;
            }
            match &alpha.nodes[id as usize].kind {
                AlphaKindM::Test(test) => {
                    if *test != expected {
                        return false;
                    }
                },
                AlphaKindM::Entry(_) => {
                    return false;
                },
            }
            // id = parents.get(id).copied().flatten().ok_or("cached alpha path lacks ancestor")?;
            id = match parents[id as usize] {
                Some(parent) => parent,
                None => {
                    return false;
                },
            };
            remaining -= 1;
        }
        // require!(matches!(nodes.get(id), Some(Entry { entry_type, .. }) if *entry_type == key.entry_type))
        proof {
            assert(tests@.take(0) =~= Seq::<u64>::empty());
        }
        if id as usize >= alpha.nodes.len() {
            return false;
        }
        match &alpha.nodes[id as usize].kind {
            AlphaKindM::Entry(entry) => {
                if *entry != compiler.alpha_path_cache[j].0.entry {
                    return false;
                }
            },
            AlphaKindM::Test(_) => {
                return false;
            },
        }
        j += 1;
    }
    true
}

/// Theorem (snapshot accepts compiled states): from `ReteCompiler::new()`,
/// after any sequence of `ensure_alpha_path` calls, the snapshot's alpha
/// cache check succeeds. (`state_wf` holds initially and is preserved by
/// every call, and `validate_alpha_cache_snapshot` requires only it.)
fn theorem_snapshot_accepts_compiled_alpha_paths(entries: &Vec<u64>, tests: &Vec<Vec<u64>>) -> (ok: bool)
    requires
        entries.len() == tests.len(),
        entries.len() <= 1000,
        forall|i: int| 0 <= i < tests.len() ==> (#[trigger] tests@[i])@.len() <= 1000,
    ensures
        ok,
{
    let (mut compiler, mut alpha, Ghost(par)) = new_alpha_state();
    let ghost mut cur = par;
    let mut i: usize = 0;
    while i < entries.len()
        invariant
            entries.len() == tests.len(),
            entries.len() <= 1000,
            forall|k: int| 0 <= k < tests.len() ==> (#[trigger] tests@[k])@.len() <= 1000,
            i <= entries.len(),
            state_wf(compiler, alpha, cur),
            alpha.next_node_id <= i * 1001,
            alpha.memory_count <= i,
        decreases entries.len() - i,
    {
        let (_m, Ghost(next)) = ensure_alpha_path(&mut compiler, &mut alpha, entries[i], &tests[i], Ghost(cur));
        proof {
            cur = next;
        }
        i += 1;
    }
    validate_alpha_cache_snapshot(&compiler, &alpha, Ghost(cur))
}

}

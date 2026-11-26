//! Verus model of the sequence (multifield) split search of
//! crates/ferric-rules-core/src/sequence.rs, with its constant-test pruning.
//!
//! # What is proved (unbounded: any number of segments, fields, values, tests)
//!
//! Declarative spec (`is_match`): a vector `lens` of capture lengths is a
//! match of a pattern against a fact when splitting every segment's source
//! values into consecutive runs, one run per pattern field (exactly one value
//! for a single field, `lens[k]` values for the k-th capture, the whole source
//! consumed, segments split independently and their capture lengths
//! concatenated in written order), yields a projection on which every
//! constant test holds (`project`, `all_pass`). No part of the spec mentions
//! the search, its offsets, `extra` budgets, `needs` or `checked`.
//!
//! * `search_all` (the exec transcription of `search` with a visitor that
//!   records every match) — soundness: every reported vector is a match;
//!   completeness: every match is reported; order: reports are strictly
//!   decreasing lexicographically (the CLIPS order), hence duplicate-free
//!   (`lemma_sorted_distinct`).
//! * `admits` returns `true` exactly when some match exists
//!   (`exists |lens| is_match(..)`).
//! * `search` with an arbitrary visitor of the modelled kind (`visit_ok`):
//!   reports only matches, each once, in decreasing order; when the visitor
//!   never breaks, all of them; when it does break, the break came from the
//!   visitor (a match with `stop_on_match`, or an exhausted step budget).
//! * Pruning soundness: a test evaluated on a placed prefix gives the same
//!   result on every completion (`lemma_eval_local`, from `extent` being one
//!   past the largest selector a test reads, including nested `Any`
//!   branches); so tests that pass at a capture never need re-checking
//!   (`lemma_checked_advance`), a failing ready test excludes every
//!   completion (`lemma_prune`), and the final check accepts exactly the
//!   splits passing all tests (`lemma_complete_check`).
//! * Work bound (`search`, `place`): the search runs at most
//!   `(Step events + 1) * (logical width + segments)` iterations of
//!   `place`'s loop, over all recursive calls. An iteration places one
//!   single field, moves to the next segment, or runs the ready tests once
//!   (at a capture, before trying its lengths, or at a complete split);
//!   each capture length tried starts a recursive call whose iterations
//!   are counted, so this bounds all placements and test runs. This is the
//!   premise of callers that bound the search by charging a fixed cost per
//!   `Step` (snapshot.rs `any_sequence_match` charges `split_cost`, which
//!   includes `logical_width() + segments.len()`, up front and per step).
//!   "Step events" counts the steps the visitor accepted (`Visitor::steps`;
//!   a step that breaks the search is not counted). Equality
//!   holds for a one-capture pattern over no values (2 iterations, no
//!   step), and the bound is false if a capture that is not its segment's
//!   last stops emitting `Step`, or if only the last one emits it.
//! * Termination of every exec function (Verus `decreases` checks), and
//!   absence of arithmetic overflow and out-of-bounds indexing in the model
//!   (Verus checks every exec operation).
//!
//! # Fidelity
//!
//! Each exec function cites the production function it transcribes and
//! keeps its control structure (same loop, branches, early exits, `mark` /
//! `truncate`, push/recurse/pop, longest-length-first capture loop).
//! crates/ferric-rules-core/src/sequence/kani_refinement.rs holds a
//! plain-Rust copy of this exec code and Kani harnesses comparing parts of it
//! with the production code on bounded inputs: `segment_extra`, `test_extent`
//! (`Equal`/`EqualSlot`) and production `evaluate_field_test` for `Equal`
//! over the values `SplitView::get` is documented to return (supplied by an
//! oracle closure there, not by production `SplitView::get`).
//! The search itself (`search`/`place`/`ready_tests_pass`) is beyond the
//! Kani budget there (measurements in that file), so it is linked to
//! production only by the line-by-line fidelity of this transcription.
//!
//! Abstractions and simplifications:
//! * Values are integers (`Val::Atom`, for `Value::Integer`) and multifields
//!   (`Val::Mf`); other `Value` variants are not modelled. Tests are
//!   `Equal(k)` (for `ConstantTestType::Equal(AtomKey::Integer(k))`),
//!   `EqualSlot` (for `ConstantTestType::EqualSlot`, standing for the
//!   two-selector family), `Any` (arbitrarily nested) and `WholeFact` (for
//!   `OrderedFieldCount`/`Sequence`, which never match as field tests).
//!   Ordering and offset comparisons are not modelled; they read the same
//!   selectors as `EqualSlot`.
//! * `SplitView::get` returns the borrowed field instead of a copied
//!   multifield `Value`; `field_eq` gives the `structural_eq` result for the
//!   copy. The `OnceCell` copy cache is omitted (it only memoizes `get`).
//! * The `prefix` filter of `search_with_prefix` is fixed to "always true",
//!   i.e. this models `search` (and `admits`). Join-level prefix filters
//!   (rete.rs `sequence_join_prefix_matches`) are not modelled.
//! * The `visit` callback is a concrete `Visitor` that records matches and
//!   step positions, breaking on a match when `stop_on_match` and on a step
//!   once `step_budget` steps were taken; `ControlFlow<B>` is a `bool`.
//! * A single lifetime replaces production's `'p`/`'a`; `(shortest..=extra)
//!   .rev()` is written as a countdown loop; `place`'s mutable parameters are
//!   copied into locals (`segment0` → `segment`, ...); the facts' relation
//!   and template id are omitted (the search never reads them).
//! * Ghost instrumentation with no production counterpart (erased, and
//!   dropped from the Kani copy): `SplitSearch::work` counts `place`'s loop
//!   iterations, and `search` returns it as the `Ghost` second component of
//!   its result, next to production's `is_break()`.
//! * Precondition `width(..) < usize::MAX`: the logical width
//!   (`logical_width`, sequence.rs:64) is a sum of in-memory `Vec` lengths;
//!   it bounds every placed prefix so a saturated `test_extent` is exact
//!   whenever a test is evaluated early.
//!
//! # Trusted base
//!
//! Verus, Z3 and vstd (including its specifications of `Vec`, slices,
//! `slice_subrange`, `saturating_add` and `checked_sub`), plus one item marked
//! `TRUSTED` below: `slice_from_ref`, a specification of
//! `core::slice::from_ref`, which vstd lacks. No `assume`, `admit` or other
//! `external_body` is used.
//!
//! # Known gaps
//!
//! * This is a model: no theorem here is about the production code. The Kani
//!   refinement in kani_refinement.rs links only the helpers listed above,
//!   on bounded inputs; the search loop itself is not machine-linked.
//! * `validate`, `project` (sequence.rs:302), `physical_selector`/
//!   `physical_test` and the copy-on-read capture cache are not modelled.
//! * `Step` events are specified only through the work bound, not by their
//!   exact positions; when the visitor breaks, the matches already reported
//!   are sound and ordered but are not proved to be the greatest ones.
use vstd::prelude::*;
use vstd::slice::slice_subrange;

verus! {

// ===================================================================
// Data model
// ===================================================================

#[derive(PartialEq, Eq, Structural, Clone, Copy)]
pub enum Field {
    Single,
    Multi,
}

pub enum Source {
    Ordered,
    TemplateSlot(usize),
    TemplateScalar(usize),
}

pub struct Segment {
    pub source: Source,
    pub fields: Vec<Field>,
}

pub enum Slot {
    Ordered(usize),
    Template(usize),
}

pub enum Val {
    Atom(i64),
    Mf(Vec<Val>),
}

pub enum Fact {
    Ordered(Vec<Val>),
    Template(Vec<Val>),
}

pub enum TestType {
    Equal(i64),
    EqualSlot(Slot),
    Any(Vec<Vec<Test>>),
    WholeFact,
}

pub struct Test {
    pub slot: Slot,
    pub test_type: TestType,
}

pub struct Pattern {
    pub segments: Vec<Segment>,
    pub tests: Vec<Test>,
}

// ===================================================================
// Spec: logical fields, value equality, test semantics
// ===================================================================

pub ghost enum LVal {
    One(Val),
    Many(Seq<Val>),
}

pub open spec fn val_eq(a: Val, b: Val) -> bool
    decreases a,
{
    match (a, b) {
        (Val::Atom(x), Val::Atom(y)) => x == y,
        (Val::Mf(xs), Val::Mf(ys)) => xs@.len() == ys@.len() && forall|i: int|
            0 <= i < xs@.len() ==> val_eq(#[trigger] xs@[i], ys@[i]),
        _ => false,
    }
}

pub open spec fn seq_eq(xs: Seq<Val>, ys: Seq<Val>) -> bool {
    xs.len() == ys.len() && forall|i: int| 0 <= i < xs.len() ==> val_eq(#[trigger] xs[i], ys[i])
}

pub open spec fn lval_eq(a: LVal, b: LVal) -> bool {
    match (a, b) {
        (LVal::One(x), LVal::One(y)) => val_eq(x, y),
        (LVal::Many(xs), LVal::Many(ys)) => seq_eq(xs, ys),
        (LVal::One(Val::Mf(xs)), LVal::Many(ys)) => seq_eq(xs@, ys),
        (LVal::Many(xs), LVal::One(Val::Mf(ys))) => seq_eq(xs, ys@),
        _ => false,
    }
}

pub open spec fn slot_index(slot: Slot) -> usize {
    match slot {
        Slot::Ordered(i) => i,
        Slot::Template(i) => i,
    }
}

pub open spec fn field_at(ordered: bool, p: Seq<LVal>, slot: Slot) -> Option<LVal> {
    match slot {
        Slot::Ordered(i) => if ordered && i < p.len() {
            Some(p[i as int])
        } else {
            None
        },
        Slot::Template(i) => if !ordered && i < p.len() {
            Some(p[i as int])
        } else {
            None
        },
    }
}

pub open spec fn eval(t: Test, ordered: bool, p: Seq<LVal>) -> bool
    decreases t,
{
    match field_at(ordered, p, t.slot) {
        None => false,
        Some(v) => match t.test_type {
            TestType::WholeFact => false,
            TestType::Any(bs) => exists|i: int|
                0 <= i < bs@.len() && #[trigger] trigger_branch(bs@[i]) && forall|j: int|
                    0 <= j < bs@[i]@.len() ==> eval(#[trigger] bs@[i]@[j], ordered, p),
            TestType::Equal(k) => v == LVal::One(Val::Atom(k)),
            TestType::EqualSlot(other) => match field_at(ordered, p, other) {
                Some(w) => lval_eq(v, w),
                None => false,
            },
        },
    }
}

pub open spec fn trigger_branch(b: Vec<Test>) -> bool {
    true
}

pub open spec fn branch_passes(b: Seq<Test>, ordered: bool, p: Seq<LVal>) -> bool {
    forall|j: int| 0 <= j < b.len() ==> eval(#[trigger] b[j], ordered, p)
}

pub open spec fn max_nat(a: nat, b: nat) -> nat {
    if a >= b {
        a
    } else {
        b
    }
}

pub open spec fn extent(t: Test) -> nat
    decreases t,
{
    let own = slot_index(t.slot) + 1;
    let other: nat = match t.test_type {
        TestType::EqualSlot(s) => (slot_index(s) + 1) as nat,
        TestType::Any(bs) => ext_branches(bs@, bs@.len() as int),
        _ => 0,
    };
    max_nat(own as nat, other)
}

pub open spec fn ext_branches(bs: Seq<Vec<Test>>, n: int) -> nat
    decreases bs, n,
{
    if n <= 0 || n > bs.len() {
        0
    } else {
        max_nat(ext_branches(bs, n - 1), ext_tests(bs[n - 1]@, bs[n - 1]@.len() as int))
    }
}

pub open spec fn ext_tests(ts: Seq<Test>, n: int) -> nat
    decreases ts, n,
{
    if n <= 0 || n > ts.len() {
        0
    } else {
        max_nat(ext_tests(ts, n - 1), extent(ts[n - 1]))
    }
}

pub open spec fn sat(n: nat) -> usize {
    if n > usize::MAX {
        usize::MAX
    } else {
        n as usize
    }
}

// ===================================================================
// Locality of test evaluation
// ===================================================================

proof fn lemma_ext_tests_ge(ts: Seq<Test>, n: int, j: int)
    requires
        0 <= j < n <= ts.len(),
    ensures
        extent(ts[j]) <= ext_tests(ts, n),
    decreases n,
{
    if j < n - 1 {
        lemma_ext_tests_ge(ts, n - 1, j);
    }
}

proof fn lemma_ext_branches_ge(bs: Seq<Vec<Test>>, n: int, i: int, j: int)
    requires
        0 <= i < n <= bs.len(),
        0 <= j < bs[i]@.len(),
    ensures
        extent(bs[i]@[j]) <= ext_branches(bs, n),
    decreases n,
{
    if i < n - 1 {
        lemma_ext_branches_ge(bs, n - 1, i, j);
    } else {
        lemma_ext_tests_ge(bs[i]@, bs[i]@.len() as int, j);
    }
}

pub open spec fn agree(a: Seq<LVal>, b: Seq<LVal>, n: int) -> bool {
    &&& 0 <= n <= a.len()
    &&& n <= b.len()
    &&& forall|i: int| 0 <= i < n ==> a[i] == b[i]
}

/// The pruning premise: a test reads only fields below its extent.
proof fn lemma_eval_local(t: Test, ordered: bool, a: Seq<LVal>, b: Seq<LVal>, n: int)
    requires
        extent(t) <= n,
        agree(a, b, n),
    ensures
        eval(t, ordered, a) == eval(t, ordered, b),
    decreases t,
{
    assert(field_at(ordered, a, t.slot) == field_at(ordered, b, t.slot));
    match t.test_type {
        TestType::Any(bs) => {
            assert forall|i: int, j: int|
                0 <= i < bs@.len() && 0 <= j < bs@[i]@.len() implies eval(
                #[trigger] bs@[i]@[j],
                ordered,
                a,
            ) == eval(bs@[i]@[j], ordered, b) by {
                lemma_ext_branches_ge(bs@, bs@.len() as int, i, j);
                assert(decreases_to!(t => t.test_type));
                assert(decreases_to!(t.test_type => t.test_type->Any_0));
                assert(decreases_to!(bs => bs@));
                assert(decreases_to!(bs@ => bs@[i]));
                assert(decreases_to!(bs@[i] => bs@[i]@));
                assert(decreases_to!(bs@[i]@ => bs@[i]@[j]));
                lemma_eval_local(bs@[i]@[j], ordered, a, b, n);
            }
            if eval(t, ordered, a) {
                let i = choose|i: int|
                    0 <= i < bs@.len() && #[trigger] trigger_branch(bs@[i]) && forall|j: int|
                        0 <= j < bs@[i]@.len() ==> eval(#[trigger] bs@[i]@[j], ordered, a);
                assert(trigger_branch(bs@[i]));
            }
            if eval(t, ordered, b) {
                let i = choose|i: int|
                    0 <= i < bs@.len() && #[trigger] trigger_branch(bs@[i]) && forall|j: int|
                        0 <= j < bs@[i]@.len() ==> eval(#[trigger] bs@[i]@[j], ordered, b);
                assert(trigger_branch(bs@[i]));
            }
        },
        TestType::EqualSlot(other) => {
            assert(field_at(ordered, a, other) == field_at(ordered, b, other));
        },
        _ => {},
    }
}

// ===================================================================
// Exec: split view, field equality and test evaluation
// ===================================================================

#[derive(Clone, Copy)]
pub enum FieldRef<'a> {
    Single(&'a Val),
    Multi(&'a [Val]),
}

impl<'a> FieldRef<'a> {
    pub open spec fn lval(self) -> LVal {
        match self {
            FieldRef::Single(v) => LVal::One(*v),
            FieldRef::Multi(s) => LVal::Many(s@),
        }
    }
}

pub struct SplitView<'a> {
    pub fact: &'a Fact,
    pub fields: Vec<FieldRef<'a>>,
    pub lengths: Vec<usize>,
}

pub open spec fn lvals<'a>(fields: Seq<FieldRef<'a>>) -> Seq<LVal> {
    fields.map_values(|f: FieldRef<'a>| f.lval())
}

impl<'a> SplitView<'a> {
    pub open spec fn ordered(&self) -> bool {
        *self.fact is Ordered
    }

    pub open spec fn view_fields(&self) -> Seq<LVal> {
        lvals(self.fields@)
    }

    // Models crates/ferric-rules-core/src/sequence.rs:544 fn get (minus the
    // capture copy: the borrowed field is returned and compared in place).
    pub fn get(&self, slot: &Slot) -> (r: Option<FieldRef<'a>>)
        ensures
            r is None ==> field_at(self.ordered(), self.view_fields(), *slot) is None,
            r matches Some(f) ==> field_at(self.ordered(), self.view_fields(), *slot) == Some(
                f.lval(),
            ),
    {
        let index = match (slot, self.fact) {
            (Slot::Ordered(index), Fact::Ordered(_)) => *index,
            (Slot::Template(index), Fact::Template(_)) => *index,
            _ => return None,
        };
        if index < self.fields.len() {
            Some(self.fields[index])
        } else {
            None
        }
    }
}

// Models crates/ferric-rules-core/src/value.rs:310 fn structural_eq, on
// integers and multifields.
fn value_eq(a: &Val, b: &Val) -> (r: bool)
    ensures
        r == val_eq(*a, *b),
    decreases *a,
{
    match a {
        Val::Atom(x) => match b {
            Val::Atom(y) => *x == *y,
            _ => false,
        },
        Val::Mf(xs) => match b {
            Val::Mf(ys) => {
                if xs.len() != ys.len() {
                    return false;
                }
                let mut i = 0;
                while i < xs.len()
                    invariant
                        0 <= i <= xs.len(),
                        xs.len() == ys.len(),
                        *a == Val::Mf(*xs),
                        *b == Val::Mf(*ys),
                        forall|k: int| 0 <= k < i ==> val_eq(#[trigger] xs@[k], ys@[k]),
                    decreases xs.len() - i,
                {
                    proof {
                        assert(decreases_to!(*a => (*a)->Mf_0));
                        assert(decreases_to!(*xs => xs@));
                        assert(decreases_to!(xs@ => xs@[i as int]));
                    }
                    if !value_eq(&xs[i], &ys[i]) {
                        assert(!val_eq(xs@[i as int], ys@[i as int]));
                        return false;
                    }
                    i += 1;
                }
                true
            },
            _ => false,
        },
    }
}

// Models crates/ferric-rules-core/src/value.rs:260 fn eq (`impl PartialEq for
// Multifield`): equal lengths and pairwise `structural_eq`.
fn values_eq(xs: &[Val], ys: &[Val]) -> (r: bool)
    ensures
        r == seq_eq(xs@, ys@),
{
    if xs.len() != ys.len() {
        return false;
    }
    let mut i = 0;
    while i < xs.len()
        invariant
            0 <= i <= xs.len(),
            xs.len() == ys.len(),
            forall|k: int| 0 <= k < i ==> val_eq(#[trigger] xs@[k], ys@[k]),
        decreases xs.len() - i,
    {
        if !value_eq(&xs[i], &ys[i]) {
            return false;
        }
        i += 1;
    }
    true
}

// Models the `slot_value.structural_eq(other_value)` comparison of the
// `EqualSlot` arm, crates/ferric-rules-core/src/alpha.rs:881 (in fn
// evaluate_field_test_inner), on the values crates/ferric-rules-core/src/
// sequence.rs:544 fn get returns: a capture reads as the multifield of its
// values (sequence.rs:602 fn capture).
fn field_eq(a: FieldRef, b: FieldRef) -> (r: bool)
    ensures
        r == lval_eq(a.lval(), b.lval()),
{
    match (a, b) {
        (FieldRef::Single(x), FieldRef::Single(y)) => value_eq(x, y),
        (FieldRef::Multi(xs), FieldRef::Multi(ys)) => values_eq(xs, ys),
        (FieldRef::Single(Val::Mf(xs)), FieldRef::Multi(ys)) => values_eq(xs.as_slice(), ys),
        (FieldRef::Multi(xs), FieldRef::Single(Val::Mf(ys))) => values_eq(xs, ys.as_slice()),
        _ => false,
    }
}

// Models crates/ferric-rules-core/src/alpha.rs:844 fn evaluate_field_test_inner
// for the modelled test types, with `field` = `split.get`.
fn evaluate_field_test(test: &Test, split: &SplitView) -> (r: bool)
    ensures
        r == eval(*test, split.ordered(), split.view_fields()),
    decreases test,
{
    let value = match split.get(&test.slot) {
        Some(value) => value,
        None => return false,
    };
    match &test.test_type {
        TestType::WholeFact => false,
        TestType::Any(branches) => {
            let ghost p = split.view_fields();
            let ghost ordered = split.ordered();
            let mut i = 0;
            while i < branches.len()
                invariant
                    0 <= i <= branches.len(),
                    test.test_type == TestType::Any(*branches),
                    field_at(ordered, p, test.slot) is Some,
                    p == split.view_fields(),
                    ordered == split.ordered(),
                    forall|i0: int|
                        0 <= i0 < i ==> !branch_passes(#[trigger] branches@[i0]@, ordered, p),
                decreases branches.len() - i,
            {
                let branch = &branches[i];
                let mut all = true;
                let mut j = 0;
                while j < branch.len()
                    invariant_except_break
                        all,
                    invariant
                        0 <= j <= branch.len(),
                        *branch == branches@[i as int],
                        test.test_type == TestType::Any(*branches),
                        0 <= i < branches.len(),
                        field_at(ordered, p, test.slot) is Some,
                        forall|i0: int|
                            0 <= i0 < i ==> !branch_passes(#[trigger] branches@[i0]@, ordered, p),
                        p == split.view_fields(),
                        ordered == split.ordered(),
                        forall|j0: int| 0 <= j0 < j ==> eval(#[trigger] branch@[j0], ordered, p),
                    ensures
                        all ==> forall|j0: int|
                            0 <= j0 < branch@.len() ==> eval(#[trigger] branch@[j0], ordered, p),
                        !all ==> !eval(branch@[j as int], ordered, p) && j < branch.len(),
                    decreases branch.len() - j,
                {
                    proof {
                        assert(decreases_to!(*test => test.test_type));
                        assert(decreases_to!(test.test_type => test.test_type->Any_0));
                        assert(decreases_to!(*branches => branches@));
                        assert(decreases_to!(branches@ => branches@[i as int]));
                        assert(decreases_to!(branches@[i as int] => branches@[i as int]@));
                        assert(decreases_to!(branches@[i as int]@ => branches@[i as int]@[j as int]));
                    }
                    if !evaluate_field_test(&branch[j], split) {
                        all = false;
                        break;
                    }
                    j += 1;
                }
                if all {
                    assert(trigger_branch(branches@[i as int]));
                    assert(forall|j: int|
                        0 <= j < branches@[i as int]@.len() ==> eval(
                            #[trigger] branches@[i as int]@[j],
                            ordered,
                            p,
                        ));
                    assert(field_at(ordered, p, test.slot) is Some);
                    assert(test.test_type->Any_0 == *branches);
                    assert(0 <= i < branches@.len());
                    assert(exists|i0: int|
                        0 <= i0 < branches@.len() && #[trigger] trigger_branch(branches@[i0]) && forall|j: int|
                            0 <= j < branches@[i0]@.len() ==> eval(#[trigger] branches@[i0]@[j], ordered, p));
                    assert(eval(*test, ordered, p));
                    return true;
                }
                assert(!eval(branches@[i as int]@[j as int], ordered, p));
                assert(!branch_passes(branches@[i as int]@, ordered, p));
                i += 1;
            }
            proof {
                if eval(*test, ordered, p) {
                    let i0 = choose|i0: int|
                        0 <= i0 < branches@.len() && #[trigger] trigger_branch(branches@[i0])
                            && forall|j: int|
                            0 <= j < branches@[i0]@.len() ==> eval(
                                #[trigger] branches@[i0]@[j],
                                ordered,
                                p,
                            );
                    assert(false);
                }
            }
            false
        },
        TestType::Equal(key) => match value {
            FieldRef::Single(Val::Atom(x)) => *x == *key,
            _ => false,
        },
        TestType::EqualSlot(other) => match split.get(other) {
            Some(other_value) => field_eq(value, other_value),
            None => false,
        },
    }
}

// Models the `index` closure of crates/ferric-rules-core/src/sequence.rs:381
// (in fn test_extent).
fn index(slot: &Slot) -> (r: usize)
    ensures
        r == slot_index(*slot),
{
    match slot {
        Slot::Ordered(index) => *index,
        Slot::Template(index) => *index,
    }
}

// Models crates/ferric-rules-core/src/sequence.rs:380 fn test_extent.
fn test_extent(test: &Test) -> (r: usize)
    ensures
        r == sat(extent(*test)),
    decreases test,
{
    let other = match &test.test_type {
        TestType::EqualSlot(slot) => index(slot).saturating_add(1),
        TestType::Any(branches) => {
            let mut best: usize = 0;
            let mut i = 0;
            while i < branches.len()
                invariant
                    0 <= i <= branches.len(),
                    test.test_type == TestType::Any(*branches),
                    best == sat(ext_branches(branches@, i as int)),
                decreases branches.len() - i,
            {
                let branch = &branches[i];
                let mut j = 0;
                while j < branch.len()
                    invariant
                        0 <= j <= branch.len(),
                        0 <= i < branches.len(),
                        *branch == branches@[i as int],
                        test.test_type == TestType::Any(*branches),
                        best == sat(
                            max_nat(
                                ext_branches(branches@, i as int),
                                ext_tests(branch@, j as int),
                            ),
                        ),
                    decreases branch.len() - j,
                {
                    proof {
                        assert(decreases_to!(*test => test.test_type));
                        assert(decreases_to!(test.test_type => test.test_type->Any_0));
                        assert(decreases_to!(*branches => branches@));
                        assert(decreases_to!(branches@ => branches@[i as int]));
                        assert(decreases_to!(branches@[i as int] => branches@[i as int]@));
                        assert(decreases_to!(branches@[i as int]@ => branches@[i as int]@[j as int]));
                    }
                    let extent = test_extent(&branch[j]);
                    if extent > best {
                        best = extent;
                    }
                    j += 1;
                }
                i += 1;
            }
            best
        },
        _ => 0,
    };
    let own = index(&test.slot).saturating_add(1);
    if own >= other {
        own
    } else {
        other
    }
}


// ===================================================================
// Spec: the declarative meaning of a split
// ===================================================================

// What `segment_values` (sequence.rs:606) yields for a source.
pub open spec fn seg_values(source: Source, fact: Fact) -> Option<Seq<Val>> {
    match source {
        Source::Ordered => match fact {
            Fact::Ordered(fs) => Some(fs@),
            Fact::Template(_) => None,
        },
        Source::TemplateSlot(i) => match fact {
            Fact::Template(ss) => if i < ss@.len() {
                match ss@[i as int] {
                    Val::Mf(vs) => Some(vs@),
                    Val::Atom(_) => Some(seq![ss@[i as int]]),
                }
            } else {
                None
            },
            Fact::Ordered(_) => None,
        },
        Source::TemplateScalar(i) => match fact {
            Fact::Template(ss) => if i < ss@.len() {
                Some(seq![ss@[i as int]])
            } else {
                None
            },
            Fact::Ordered(_) => None,
        },
    }
}

pub open spec fn singles_from(fields: Seq<Field>, f: int) -> nat
    decreases fields.len() - f,
{
    if f < 0 || f >= fields.len() {
        0
    } else {
        (if fields[f] == Field::Single {
            1nat
        } else {
            0nat
        }) + singles_from(fields, f + 1)
    }
}

pub open spec fn multis_from(fields: Seq<Field>, f: int) -> nat
    decreases fields.len() - f,
{
    if f < 0 || f >= fields.len() {
        0
    } else {
        (if fields[f] == Field::Multi {
            1nat
        } else {
            0nat
        }) + multis_from(fields, f + 1)
    }
}

/// Logical fields `fields[f..]` of one segment, read consecutively from
/// `vals[o..]`: a single field takes exactly one value, a capture takes the
/// next `lens[0]` values. All of `vals` must be consumed. Returns the
/// projected fields and the unconsumed capture lengths.
pub open spec fn proj_seg_from(fields: Seq<Field>, vals: Seq<Val>, f: int, o: int, lens: Seq<usize>) -> Option<
    (Seq<LVal>, Seq<usize>),
>
    decreases fields.len() - f,
{
    if f < 0 || o < 0 {
        None
    } else if f >= fields.len() {
        if o == vals.len() {
            Some((Seq::empty(), lens))
        } else {
            None
        }
    } else if fields[f] == Field::Single {
        if o < vals.len() {
            match proj_seg_from(fields, vals, f + 1, o + 1, lens) {
                Some(pr) => Some((seq![LVal::One(vals[o])] + pr.0, pr.1)),
                None => None,
            }
        } else {
            None
        }
    } else {
        if lens.len() > 0 && o + lens[0] <= vals.len() {
            match proj_seg_from(fields, vals, f + 1, o + lens[0], lens.drop_first()) {
                Some(pr) => Some((seq![LVal::Many(vals.subrange(o, o + lens[0]))] + pr.0, pr.1)),
                None => None,
            }
        } else {
            None
        }
    }
}

/// Segments `segs[s..]`, each split independently, consuming `lens` exactly.
pub open spec fn proj_segs_from(segs: Seq<Segment>, fact: Fact, s: int, lens: Seq<usize>) -> Option<
    Seq<LVal>,
>
    decreases segs.len() - s,
{
    if s < 0 {
        None
    } else if s >= segs.len() {
        if lens.len() == 0 {
            Some(Seq::empty())
        } else {
            None
        }
    } else {
        match seg_values(segs[s].source, fact) {
            None => None,
            Some(vals) => match proj_seg_from(segs[s].fields@, vals, 0, 0, lens) {
                None => None,
                Some(pr) => match proj_segs_from(segs, fact, s + 1, pr.1) {
                    None => None,
                    Some(q) => Some(pr.0 + q),
                },
            },
        }
    }
}

/// The logical projection of `fact` under the capture lengths `lens`, or
/// `None` when `lens` is not a split of `fact` (the meaning of
/// sequence.rs:302 fn project).
pub open spec fn project(pat: Pattern, fact: Fact, lens: Seq<usize>) -> Option<Seq<LVal>> {
    if pat.segments@.len() == 0 {
        None
    } else {
        proj_segs_from(pat.segments@, fact, 0, lens)
    }
}

pub open spec fn all_pass(tests: Seq<Test>, ordered: bool, p: Seq<LVal>) -> bool {
    forall|i: int| 0 <= i < tests.len() ==> #[trigger] eval(tests[i], ordered, p)
}

/// THE SPEC: `lens` splits every segment of `fact` into consecutive runs,
/// one per pattern field (one value per single field, `lens[k]` values for
/// the k-th capture), and the projected fields pass every test.
pub open spec fn is_match(pat: Pattern, fact: Fact, lens: Seq<usize>) -> bool {
    match project(pat, fact, lens) {
        Some(p) => all_pass(pat.tests@, fact is Ordered, p),
        None => false,
    }
}

/// The rest of the projection from segment `s`, field `f`, offset `o`.
pub open spec fn proj_state(segs: Seq<Segment>, fact: Fact, s: int, f: int, o: int, rest: Seq<usize>) -> Option<
    Seq<LVal>,
> {
    if 0 <= s < segs.len() {
        match seg_values(segs[s].source, fact) {
            None => None,
            Some(vals) => match proj_seg_from(segs[s].fields@, vals, f, o, rest) {
                None => None,
                Some(pr) => match proj_segs_from(segs, fact, s + 1, pr.1) {
                    None => None,
                    Some(q) => Some(pr.0 + q),
                },
            },
        }
    } else {
        None
    }
}

pub open spec fn prepend(p: Seq<LVal>, o: Option<Seq<LVal>>) -> Option<Seq<LVal>> {
    match o {
        Some(q) => Some(p + q),
        None => None,
    }
}

/// The search invariant tying the placed prefix to the declarative
/// projection: with capture lengths `lens` chosen and fields `p` placed,
/// the projection of every extension `lens + rest` is `p` followed by the
/// rest of the projection from the current position.
pub open spec fn linked(pat: Pattern, fact: Fact, lens: Seq<usize>, p: Seq<LVal>, s: int, f: int, o: int) -> bool {
    forall|rest: Seq<usize>|
        #![trigger project(pat, fact, lens + rest)]
        #![trigger proj_state(pat.segments@, fact, s, f, o, rest)]
        project(pat, fact, lens + rest) == prepend(p, proj_state(pat.segments@, fact, s, f, o, rest))
}

pub open spec fn fields_upto(segs: Seq<Segment>, n: int) -> nat
    decreases n,
{
    if n <= 0 || n > segs.len() {
        0
    } else {
        fields_upto(segs, n - 1) + segs[n - 1].fields@.len()
    }
}

/// `SequencePattern::logical_width` (sequence.rs:64).
pub open spec fn width(segs: Seq<Segment>) -> nat {
    fields_upto(segs, segs.len() as int)
}

pub open spec fn lex_at(a: Seq<usize>, b: Seq<usize>, k: int) -> bool {
    &&& 0 <= k < a.len()
    &&& k < b.len()
    &&& a.take(k) == b.take(k)
    &&& a[k] > b[k]
}

/// Strict lexicographic order on capture-length vectors.
pub open spec fn lex_gt(a: Seq<usize>, b: Seq<usize>) -> bool {
    exists|k: int| #[trigger] lex_at(a, b, k)
}

pub open spec fn sorted_desc(m: Seq<Vec<usize>>, lo: int, hi: int) -> bool {
    forall|j1: int, j2: int| lo <= j1 < j2 < hi ==> #[trigger] lex_gt(m[j1]@, m[j2]@)
}

pub open spec fn emitted(m: Seq<Vec<usize>>, lo: int, hi: int, x: Seq<usize>) -> bool {
    exists|j: int| lo <= j < hi && #[trigger] m[j]@ == x
}

pub open spec fn has_prefix(x: Seq<usize>, lens: Seq<usize>) -> bool {
    x.len() >= lens.len() && x.take(lens.len() as int) == lens
}

pub open spec fn sound_from(pat: Pattern, fact: Fact, m: Seq<Vec<usize>>, lo: int, hi: int, lens: Seq<usize>) -> bool {
    forall|j: int| #![trigger m[j]] lo <= j < hi ==> has_prefix(m[j]@, lens) && is_match(pat, fact, m[j]@)
}

pub open spec fn complete_from(pat: Pattern, fact: Fact, m: Seq<Vec<usize>>, lo: int, hi: int, lens: Seq<usize>) -> bool {
    forall|rest: Seq<usize>| #[trigger] is_match(pat, fact, lens + rest) ==> emitted(m, lo, hi, lens + rest)
}

// ===================================================================
// Lemmas about the declarative projection
// ===================================================================

proof fn lemma_counts(fields: Seq<Field>, f: int)
    requires
        0 <= f <= fields.len(),
    ensures
        singles_from(fields, f) + multis_from(fields, f) == fields.len() - f,
    decreases fields.len() - f,
{
    if f < fields.len() {
        lemma_counts(fields, f + 1);
    }
}

/// A segment split consumes exactly the values its fields can hold.
proof fn lemma_budget(fields: Seq<Field>, vals: Seq<Val>, f: int, o: int, lens: Seq<usize>)
    requires
        proj_seg_from(fields, vals, f, o, lens) is Some,
    ensures
        o + singles_from(fields, f) <= vals.len(),
        multis_from(fields, f) == 0 ==> o + singles_from(fields, f) == vals.len(),
    decreases fields.len() - f,
{
    if f < fields.len() {
        if fields[f] == Field::Single {
            lemma_budget(fields, vals, f + 1, o + 1, lens);
        } else {
            lemma_budget(fields, vals, f + 1, o + lens[0], lens.drop_first());
        }
    }
}

proof fn lemma_fields_upto_mono(segs: Seq<Segment>, a: int, b: int)
    requires
        0 <= a <= b <= segs.len(),
    ensures
        fields_upto(segs, a) <= fields_upto(segs, b),
    decreases b - a,
{
    if a < b {
        lemma_fields_upto_mono(segs, a, b - 1);
    }
}

/// A segment with no source, or whose single fields cannot fit (or, without
/// captures, do not fill) its source, admits no split of the whole fact.
proof fn lemma_bad_segment(segs: Seq<Segment>, fact: Fact, s: int, k: int, lens: Seq<usize>)
    requires
        0 <= s <= k < segs.len(),
        seg_values(segs[k].source, fact) is None || ({
            let n = seg_values(segs[k].source, fact).unwrap().len();
            let singles = singles_from(segs[k].fields@, 0);
            n < singles || (n > singles && multis_from(segs[k].fields@, 0) == 0)
        }),
    ensures
        proj_segs_from(segs, fact, s, lens) is None,
    decreases k - s,
{
    if s == k {
        if seg_values(segs[k].source, fact) is Some {
            let vals = seg_values(segs[k].source, fact).unwrap();
            if proj_seg_from(segs[k].fields@, vals, 0, 0, lens) is Some {
                lemma_budget(segs[k].fields@, vals, 0, 0, lens);
            }
        }
    } else {
        if seg_values(segs[s].source, fact) is Some {
            let vals = seg_values(segs[s].source, fact).unwrap();
            if proj_seg_from(segs[s].fields@, vals, 0, 0, lens) is Some {
                let pr = proj_seg_from(segs[s].fields@, vals, 0, 0, lens).unwrap();
                lemma_bad_segment(segs, fact, s + 1, k, pr.1);
            }
        }
    }
}

proof fn lemma_link_start(pat: Pattern, fact: Fact)
    requires
        pat.segments@.len() > 0,
    ensures
        linked(pat, fact, Seq::empty(), Seq::empty(), 0, 0, 0),
{
    assert forall|rest: Seq<usize>|
        project(pat, fact, Seq::<usize>::empty() + rest) == prepend(
            Seq::empty(),
            proj_state(pat.segments@, fact, 0, 0, 0, rest),
        ) by {
        assert(Seq::<usize>::empty() + rest =~= rest);
        match proj_state(pat.segments@, fact, 0, 0, 0, rest) {
            Some(q) => {
                assert(Seq::<LVal>::empty() + q =~= q);
            },
            None => {},
        }
    }
}

proof fn lemma_link_single(pat: Pattern, fact: Fact, lens: Seq<usize>, p: Seq<LVal>, s: int, f: int, o: int)
    requires
        linked(pat, fact, lens, p, s, f, o),
        0 <= s < pat.segments@.len(),
        seg_values(pat.segments@[s].source, fact) is Some,
        0 <= f < pat.segments@[s].fields@.len(),
        pat.segments@[s].fields@[f] == Field::Single,
        0 <= o < seg_values(pat.segments@[s].source, fact).unwrap().len(),
    ensures
        linked(
            pat,
            fact,
            lens,
            p.push(LVal::One(seg_values(pat.segments@[s].source, fact).unwrap()[o])),
            s,
            f + 1,
            o + 1,
        ),
{
    let segs = pat.segments@;
    let fields = segs[s].fields@;
    let vals = seg_values(segs[s].source, fact).unwrap();
    let x = LVal::One(vals[o]);
    assert forall|rest: Seq<usize>|
        project(pat, fact, lens + rest) == prepend(
            p.push(x),
            proj_state(segs, fact, s, f + 1, o + 1, rest),
        ) by {
        assert(project(pat, fact, lens + rest) == prepend(p, proj_state(segs, fact, s, f, o, rest)));
        match proj_seg_from(fields, vals, f + 1, o + 1, rest) {
            Some(pr) => {
                assert(proj_seg_from(fields, vals, f, o, rest) == Some((seq![x] + pr.0, pr.1)));
                match proj_segs_from(segs, fact, s + 1, pr.1) {
                    Some(q) => {
                        assert(p + ((seq![x] + pr.0) + q) =~= p.push(x) + (pr.0 + q));
                    },
                    None => {},
                }
            },
            None => {
                assert(proj_seg_from(fields, vals, f, o, rest) is None);
            },
        }
    }
}

proof fn lemma_link_multi(
    pat: Pattern,
    fact: Fact,
    lens: Seq<usize>,
    p: Seq<LVal>,
    s: int,
    f: int,
    o: int,
    len: usize,
)
    requires
        linked(pat, fact, lens, p, s, f, o),
        0 <= s < pat.segments@.len(),
        seg_values(pat.segments@[s].source, fact) is Some,
        0 <= f < pat.segments@[s].fields@.len(),
        pat.segments@[s].fields@[f] == Field::Multi,
        0 <= o,
        o + len <= seg_values(pat.segments@[s].source, fact).unwrap().len(),
    ensures
        linked(
            pat,
            fact,
            lens.push(len),
            p.push(
                LVal::Many(seg_values(pat.segments@[s].source, fact).unwrap().subrange(o, o + len)),
            ),
            s,
            f + 1,
            o + len,
        ),
{
    let segs = pat.segments@;
    let fields = segs[s].fields@;
    let vals = seg_values(segs[s].source, fact).unwrap();
    let x = LVal::Many(vals.subrange(o, o + len));
    assert forall|rest: Seq<usize>|
        project(pat, fact, lens.push(len) + rest) == prepend(
            p.push(x),
            proj_state(segs, fact, s, f + 1, o + len, rest),
        ) by {
        let rest0 = seq![len] + rest;
        assert(lens.push(len) + rest =~= lens + rest0);
        assert(project(pat, fact, lens + rest0) == prepend(p, proj_state(segs, fact, s, f, o, rest0)));
        assert(rest0.drop_first() =~= rest);
        assert(rest0[0] == len);
        match proj_seg_from(fields, vals, f + 1, o + len, rest) {
            Some(pr) => {
                assert(proj_seg_from(fields, vals, f, o, rest0) == Some((seq![x] + pr.0, pr.1)));
                match proj_segs_from(segs, fact, s + 1, pr.1) {
                    Some(q) => {
                        assert(p + ((seq![x] + pr.0) + q) =~= p.push(x) + (pr.0 + q));
                    },
                    None => {},
                }
            },
            None => {
                assert(proj_seg_from(fields, vals, f, o, rest0) is None);
            },
        }
    }
}

proof fn lemma_link_advance(pat: Pattern, fact: Fact, lens: Seq<usize>, p: Seq<LVal>, s: int)
    requires
        0 <= s,
        s + 1 < pat.segments@.len(),
        seg_values(pat.segments@[s].source, fact) is Some,
        linked(
            pat,
            fact,
            lens,
            p,
            s,
            pat.segments@[s].fields@.len() as int,
            seg_values(pat.segments@[s].source, fact).unwrap().len() as int,
        ),
    ensures
        linked(pat, fact, lens, p, s + 1, 0, 0),
{
    let segs = pat.segments@;
    let n = segs[s].fields@.len() as int;
    let vals = seg_values(segs[s].source, fact).unwrap();
    assert forall|rest: Seq<usize>|
        project(pat, fact, lens + rest) == prepend(p, proj_state(segs, fact, s + 1, 0, 0, rest)) by {
        assert(project(pat, fact, lens + rest) == prepend(
            p,
            proj_state(segs, fact, s, n, vals.len() as int, rest),
        ));
        assert(proj_seg_from(segs[s].fields@, vals, n, vals.len() as int, rest) == Some(
            (Seq::<LVal>::empty(), rest),
        ));
        match proj_segs_from(segs, fact, s + 1, rest) {
            Some(q) => {
                assert(Seq::<LVal>::empty() + q =~= q);
            },
            None => {},
        }
    }
}

proof fn lemma_link_end(pat: Pattern, fact: Fact, lens: Seq<usize>, p: Seq<LVal>, s: int)
    requires
        0 <= s,
        s + 1 == pat.segments@.len(),
        seg_values(pat.segments@[s].source, fact) is Some,
        linked(
            pat,
            fact,
            lens,
            p,
            s,
            pat.segments@[s].fields@.len() as int,
            seg_values(pat.segments@[s].source, fact).unwrap().len() as int,
        ),
    ensures
        forall|rest: Seq<usize>| #[trigger]
            project(pat, fact, lens + rest) == if rest.len() == 0 {
                Some(p)
            } else {
                None
            },
{
    let segs = pat.segments@;
    let n = segs[s].fields@.len() as int;
    let vals = seg_values(segs[s].source, fact).unwrap();
    assert forall|rest: Seq<usize>|
        #[trigger] project(pat, fact, lens + rest) == if rest.len() == 0 {
            Some(p)
        } else {
            None
        } by {
        assert(project(pat, fact, lens + rest) == prepend(
            p,
            proj_state(segs, fact, s, n, vals.len() as int, rest),
        ));
        assert(proj_seg_from(segs[s].fields@, vals, n, vals.len() as int, rest) == Some(
            (Seq::<LVal>::empty(), rest),
        ));
        assert(p + (Seq::<LVal>::empty() + Seq::<LVal>::empty()) =~= p);
    }
}

/// Every split extending the capture state at a capture of field `f` gives
/// that capture a length in `[shortest, e]`, where the segment's last
/// capture (no captures after it) must take exactly `e`.
proof fn lemma_capture_range(
    pat: Pattern,
    fact: Fact,
    lens: Seq<usize>,
    p: Seq<LVal>,
    s: int,
    f: int,
    o: int,
    e: int,
    rest: Seq<usize>,
)
    requires
        linked(pat, fact, lens, p, s, f, o),
        0 <= s < pat.segments@.len(),
        seg_values(pat.segments@[s].source, fact) is Some,
        0 <= f < pat.segments@[s].fields@.len(),
        pat.segments@[s].fields@[f] == Field::Multi,
        0 <= o,
        e + o + singles_from(pat.segments@[s].fields@, f) == seg_values(
            pat.segments@[s].source,
            fact,
        ).unwrap().len(),
        project(pat, fact, lens + rest) is Some,
    ensures
        rest.len() > 0,
        rest[0] <= e,
        multis_from(pat.segments@[s].fields@, f + 1) == 0 ==> rest[0] == e,
{
    let segs = pat.segments@;
    let fields = segs[s].fields@;
    let vals = seg_values(segs[s].source, fact).unwrap();
    assert(project(pat, fact, lens + rest) == prepend(p, proj_state(segs, fact, s, f, o, rest)));
    assert(proj_seg_from(fields, vals, f, o, rest) is Some);
    assert(rest.len() > 0);
    assert(proj_seg_from(fields, vals, f + 1, o + rest[0], rest.drop_first()) is Some);
    lemma_budget(fields, vals, f + 1, o + rest[0], rest.drop_first());
}

/// A test that already passed on the checked prefix passes on every
/// sequence that agrees with it there.
proof fn lemma_checked_passes(
    tests: Seq<Test>,
    needs: Seq<usize>,
    ordered: bool,
    p: Seq<LVal>,
    c: int,
    q: Seq<LVal>,
    i: int,
)
    requires
        checked_ok(tests, needs, ordered, p, c),
        0 <= i < tests.len(),
        needs.len() == tests.len(),
        needs[i] == sat(extent(tests[i])),
        needs[i] <= c,
        c < usize::MAX,
        c <= p.len(),
        agree(p, q, c),
    ensures
        eval(tests[i], ordered, q),
{
    assert(eval(tests[i], ordered, p.take(c)));
    lemma_eval_local(tests[i], ordered, p.take(c), q, c);
}

pub open spec fn checked_ok(tests: Seq<Test>, needs: Seq<usize>, ordered: bool, p: Seq<LVal>, c: int) -> bool {
    forall|i: int|
        0 <= i < tests.len() && needs[i] <= c ==> #[trigger] eval(tests[i], ordered, p.take(c))
}

pub open spec fn ready_ok(
    t: Test,
    need: usize,
    checked: int,
    placed: int,
    complete: bool,
    ordered: bool,
    p: Seq<LVal>,
) -> bool {
    need <= checked || (need > placed && !complete) || eval(t, ordered, p)
}

pub open spec fn all_ready(
    tests: Seq<Test>,
    needs: Seq<usize>,
    checked: int,
    complete: bool,
    ordered: bool,
    p: Seq<LVal>,
) -> bool {
    forall|i: int|
        0 <= i < tests.len() ==> #[trigger] ready_ok(
            tests[i],
            needs[i],
            checked,
            p.len() as int,
            complete,
            ordered,
            p,
        )
}

pub open spec fn needs_ok(tests: Seq<Test>, needs: Seq<usize>) -> bool {
    &&& needs.len() == tests.len()
    &&& forall|i: int| 0 <= i < tests.len() ==> #[trigger] needs[i] == sat(extent(tests[i]))
}

/// After the tests ready at a capture pass, every test whose fields are all
/// placed has passed on the placed prefix.
proof fn lemma_checked_advance(tests: Seq<Test>, needs: Seq<usize>, ordered: bool, p: Seq<LVal>, c: int)
    requires
        checked_ok(tests, needs, ordered, p, c),
        needs_ok(tests, needs),
        all_ready(tests, needs, c, false, ordered, p),
        0 <= c <= p.len(),
        p.len() < usize::MAX,
    ensures
        checked_ok(tests, needs, ordered, p, p.len() as int),
{
    assert(p.take(p.len() as int) =~= p);
    assert forall|i: int| 0 <= i < tests.len() && needs[i] <= p.len() implies #[trigger] eval(
        tests[i],
        ordered,
        p.take(p.len() as int),
    ) by {
        assert(ready_ok(tests[i], needs[i], c, p.len() as int, false, ordered, p));
        if needs[i] <= c {
            lemma_checked_passes(tests, needs, ordered, p, c, p, i);
        }
    }
}

/// At a complete split the ready check accepts exactly when every test passes.
proof fn lemma_complete_check(tests: Seq<Test>, needs: Seq<usize>, ordered: bool, p: Seq<LVal>, c: int)
    requires
        checked_ok(tests, needs, ordered, p, c),
        needs_ok(tests, needs),
        0 <= c <= p.len(),
        p.len() < usize::MAX,
    ensures
        all_ready(tests, needs, c, true, ordered, p) == all_pass(tests, ordered, p),
{
    if all_ready(tests, needs, c, true, ordered, p) {
        assert forall|i: int| 0 <= i < tests.len() implies #[trigger] eval(tests[i], ordered, p) by {
            assert(ready_ok(tests[i], needs[i], c, p.len() as int, true, ordered, p));
            if needs[i] <= c {
                lemma_checked_passes(tests, needs, ordered, p, c, p, i);
            }
        }
    }
    if all_pass(tests, ordered, p) {
        assert forall|i: int| 0 <= i < tests.len() implies #[trigger] ready_ok(
            tests[i],
            needs[i],
            c,
            p.len() as int,
            true,
            ordered,
            p,
        ) by {
            assert(eval(tests[i], ordered, p));
        }
    }
}

/// A ready test that fails on the placed prefix fails on every extension.
proof fn lemma_prune(tests: Seq<Test>, needs: Seq<usize>, ordered: bool, p: Seq<LVal>, c: int, q: Seq<LVal>)
    requires
        needs_ok(tests, needs),
        !all_ready(tests, needs, c, false, ordered, p),
        p.len() < usize::MAX,
        agree(p, q, p.len() as int),
    ensures
        !all_pass(tests, ordered, q),
{
    let i = choose|i: int|
        0 <= i < tests.len() && !#[trigger] ready_ok(
            tests[i],
            needs[i],
            c,
            p.len() as int,
            false,
            ordered,
            p,
        );
    assert(extent(tests[i]) == needs[i]);
    lemma_eval_local(tests[i], ordered, p, q, p.len() as int);
}

// ===================================================================
// Exec: segment sources, visitor, and the split search
// ===================================================================

// TRUSTED: vstd has no specification for `core::slice::from_ref`. This
// wrapper only states its documented std behaviour (a one-element slice
// aliasing `value`); production uses it at sequence.rs:612 and :616.
#[verifier::external_body]
fn slice_from_ref<'a>(value: &'a Val) -> (r: &'a [Val])
    ensures
        r@ == seq![*value],
{
    std::slice::from_ref(value)
}

// Models crates/ferric-rules-core/src/sequence.rs:606 fn segment_values.
fn segment_values<'a>(source: &Source, fact: &'a Fact) -> (r: Option<&'a [Val]>)
    ensures
        r matches Some(values) ==> seg_values(*source, *fact) == Some(values@),
        r is None ==> seg_values(*source, *fact) is None,
{
    match (source, fact) {
        (Source::Ordered, Fact::Ordered(fields)) => Some(fields.as_slice()),
        (Source::TemplateSlot(index), Fact::Template(slots)) => {
            if *index >= slots.len() {
                return None;
            }
            let value = &slots[*index];
            Some(
                match value {
                    Val::Mf(values) => values.as_slice(),
                    _ => slice_from_ref(value),
                },
            )
        },
        (Source::TemplateScalar(index), Fact::Template(slots)) => {
            if *index >= slots.len() {
                return None;
            }
            Some(slice_from_ref(&slots[*index]))
        },
        _ => None,
    }
}

// Models crates/ferric-rules-core/src/sequence.rs:406 fn segment_extra.
fn segment_extra(fields: &Vec<Field>, values: &[Val]) -> (r: Option<usize>)
    ensures
        r matches Some(e) ==> e + singles_from(fields@, 0) == values@.len() && (multis_from(
            fields@,
            0,
        ) == 0 ==> e == 0),
        r is None ==> values@.len() < singles_from(fields@, 0) || (values@.len() > singles_from(
            fields@,
            0,
        ) && multis_from(fields@, 0) == 0),
{
    let mut singles: usize = 0;
    let mut k: usize = 0;
    while k < fields.len()
        invariant
            0 <= k <= fields.len(),
            singles <= k,
            singles + singles_from(fields@, k as int) == singles_from(fields@, 0),
        decreases fields.len() - k,
    {
        if fields[k] == Field::Single {
            singles += 1;
        }
        k += 1;
    }
    proof {
        lemma_counts(fields@, 0);
    }
    let extra = match values.len().checked_sub(singles) {
        Some(extra) => extra,
        None => return None,
    };
    if extra == 0 || singles < fields.len() {
        Some(extra)
    } else {
        None
    }
}

// Models the `last` computation of crates/ferric-rules-core/src/sequence.rs:487
// (in fn place): `fields[start..].iter().all(|field| *field == Single)`.
fn only_singles_after(fields: &Vec<Field>, start: usize) -> (r: bool)
    requires
        start <= fields.len(),
    ensures
        r == (multis_from(fields@, start as int) == 0),
{
    let mut k = start;
    while k < fields.len()
        invariant
            start <= k <= fields.len(),
            multis_from(fields@, start as int) == multis_from(fields@, k as int),
        decreases fields.len() - k,
    {
        if fields[k] != Field::Single {
            return false;
        }
        k += 1;
    }
    true
}

// Models the `split.lengths.clone()` a recording visitor makes (e.g.
// crates/ferric-rules-core/src/rete.rs:2914, in fn sequence_matches).
fn copy_lengths(lengths: &Vec<usize>) -> (r: Vec<usize>)
    ensures
        r@ == lengths@,
{
    let mut out: Vec<usize> = Vec::new();
    let mut k = 0;
    while k < lengths.len()
        invariant
            0 <= k <= lengths.len(),
            out@ == lengths@.take(k as int),
        decreases lengths.len() - k,
    {
        out.push(lengths[k]);
        k += 1;
        assert(out@ =~= lengths@.take(k as int));
    }
    assert(out@ =~= lengths@);
    out
}

/// A concrete `visit` callback (the `visit` argument of sequence.rs:265
/// fn search_with_prefix): records every match's capture lengths and, at
/// every `Step`, how many matches preceded it. It breaks on a match when
/// `stop_on_match` (as `admits` does) and on a step once `step_budget`
/// steps were taken (as a work budget does).
pub struct Visitor {
    pub matches: Vec<Vec<usize>>,
    pub steps: Vec<usize>,
    pub stop_on_match: bool,
    pub step_budget: Option<usize>,
}

pub open spec fn budget_hit(v: Visitor) -> bool {
    match v.step_budget {
        Some(b) => v.steps@.len() >= b,
        None => false,
    }
}

pub open spec fn visit_ok(pat: Pattern, fact: Fact, v0: Visitor, v1: Visitor, lens: Seq<usize>, broke: bool) -> bool {
    let lo = v0.matches@.len() as int;
    let hi = v1.matches@.len() as int;
    &&& lo <= hi
    &&& forall|j: int| 0 <= j < lo ==> #[trigger] v1.matches@[j] == v0.matches@[j]
    &&& v1.stop_on_match == v0.stop_on_match
    &&& v1.step_budget == v0.step_budget
    &&& sound_from(pat, fact, v1.matches@, lo, hi, lens)
    &&& sorted_desc(v1.matches@, lo, hi)
    &&& (!broke ==> complete_from(pat, fact, v1.matches@, lo, hi, lens))
    &&& (broke ==> (v1.stop_on_match && lo < hi) || budget_hit(v1))
    &&& (!broke && v1.stop_on_match ==> lo == hi)
}

impl Visitor {
    // Models the `SplitEvent::Match` arm of a caller's `visit` closure, e.g.
    // crates/ferric-rules-core/src/sequence.rs:343 (in fn admits) and
    // crates/ferric-rules-core/src/rete.rs:2907 (in fn sequence_matches).
    pub fn on_match(&mut self, lengths: &Vec<usize>) -> (stop: bool)
        ensures
            final(self).matches@.len() == old(self).matches@.len() + 1,
            forall|j: int|
                0 <= j < old(self).matches@.len() ==> #[trigger] final(self).matches@[j] == old(
                    self,
                ).matches@[j],
            final(self).matches@[old(self).matches@.len() as int]@ == lengths@,
            final(self).stop_on_match == old(self).stop_on_match,
            final(self).step_budget == old(self).step_budget,
            final(self).steps@ == old(self).steps@,
            stop == old(self).stop_on_match,
    {
        let copy = copy_lengths(lengths);
        self.matches.push(copy);
        self.stop_on_match
    }

    // Models the `SplitEvent::Step` arm of a caller's `visit` closure, e.g.
    // the work budget of crates/ferric-rules-core/src/snapshot.rs:464.
    pub fn on_step(&mut self) -> (stop: bool)
        ensures
            final(self).matches@ == old(self).matches@,
            final(self).stop_on_match == old(self).stop_on_match,
            final(self).step_budget == old(self).step_budget,
            stop ==> budget_hit(*final(self)),
            stop ==> final(self).steps@.len() == old(self).steps@.len(),
            !stop ==> final(self).steps@.len() == old(self).steps@.len() + 1,
    {
        if let Some(budget) = self.step_budget {
            if self.steps.len() >= budget {
                return true;
            }
        }
        let marker = self.matches.len();
        self.steps.push(marker);
        false
    }
}

pub open spec fn source_ok<'a>(seg: Segment, fact: Fact, src: (&'a [Val], usize)) -> bool {
    &&& seg_values(seg.source, fact) == Some(src.0@)
    &&& src.1 + singles_from(seg.fields@, 0) == src.0@.len()
    &&& (multis_from(seg.fields@, 0) == 0 ==> src.1 == 0)
}

pub open spec fn measure(segs: Seq<Segment>, s: int, f: int) -> int {
    width(segs) - fields_upto(segs, s) - f + segs.len() - s
}

// Models crates/ferric-rules-core/src/sequence.rs:415 struct SplitSearch
// (without the `prefix` filter, i.e. as used by `search`). `work` is ghost
// instrumentation with no production counterpart: it counts the iterations
// of `place`'s loop, i.e. the search effort that the work-bound theorem
// relates to the number of `Step` events.
pub struct SplitSearch<'a> {
    pub plan: &'a Pattern,
    pub sources: Vec<(&'a [Val], usize)>,
    pub needs: Vec<usize>,
    pub split: SplitView<'a>,
    pub work: Ghost<nat>,
}

impl<'a> SplitSearch<'a> {
    pub open spec fn segs(&self) -> Seq<Segment> {
        self.plan.segments@
    }

    pub open spec fn tests(&self) -> Seq<Test> {
        self.plan.tests@
    }

    pub open spec fn wf(&self) -> bool {
        &&& self.segs().len() > 0
        &&& self.sources@.len() == self.segs().len()
        &&& forall|k: int|
            0 <= k < self.segs().len() ==> #[trigger] source_ok(
                self.segs()[k],
                *self.split.fact,
                self.sources@[k],
            )
        &&& needs_ok(self.tests(), self.needs@)
        &&& width(self.segs()) < usize::MAX
    }

    pub open spec fn same_frame(&self, other: &Self) -> bool {
        &&& self.plan == other.plan
        &&& self.sources@ == other.sources@
        &&& self.needs@ == other.needs@
        &&& self.split.fact == other.split.fact
    }

    pub open spec fn state_ok(&self, s: int, f: int, o: int, e: int, c: int) -> bool {
        let segs = self.segs();
        let p = self.split.view_fields();
        &&& 0 <= s < segs.len()
        &&& 0 <= f <= segs[s].fields@.len()
        &&& 0 <= o
        &&& 0 <= e
        &&& e + o + singles_from(segs[s].fields@, f) == self.sources@[s].0@.len()
        &&& (multis_from(segs[s].fields@, f) == 0 ==> e == 0)
        &&& 0 <= c <= p.len()
        &&& p.len() == fields_upto(segs, s) + f
        &&& checked_ok(self.tests(), self.needs@, self.split.ordered(), p, c)
        &&& linked(*self.plan, *self.split.fact, self.split.lengths@, p, s, f, o)
    }

    // Models crates/ferric-rules-core/src/sequence.rs:432 fn ready_tests_pass
    // (the `prefix` filter is absent, i.e. always true, as in `search`).
    fn ready_tests_pass(&self, checked: usize, complete: bool) -> (r: bool)
        requires
            self.needs@.len() == self.tests().len(),
        ensures
            r == all_ready(
                self.tests(),
                self.needs@,
                checked as int,
                complete,
                self.split.ordered(),
                self.split.view_fields(),
            ),
    {
        let placed = self.split.fields.len();
        let ghost p = self.split.view_fields();
        let mut i = 0;
        while i < self.plan.tests.len()
            invariant
                0 <= i <= self.tests().len(),
                self.needs@.len() == self.tests().len(),
                placed == p.len(),
                p == self.split.view_fields(),
                forall|i0: int|
                    0 <= i0 < i ==> #[trigger] ready_ok(
                        self.tests()[i0],
                        self.needs@[i0],
                        checked as int,
                        placed as int,
                        complete,
                        self.split.ordered(),
                        p,
                    ),
            decreases self.tests().len() - i,
        {
            let need = self.needs[i];
            if !(need <= checked || (need > placed && !complete) || evaluate_field_test(
                &self.plan.tests[i],
                &self.split,
            )) {
                assert(!ready_ok(
                    self.tests()[i as int],
                    self.needs@[i as int],
                    checked as int,
                    placed as int,
                    complete,
                    self.split.ordered(),
                    p,
                ));
                return false;
            }
            i += 1;
        }
        true
    }
}


// ===================================================================
// Visitor bookkeeping lemmas
// ===================================================================

pub open spec fn above(m: Seq<Vec<usize>>, lo: int, hi: int, k: int, bound: int) -> bool {
    forall|j: int| #![trigger m[j]] lo <= j < hi ==> m[j]@.len() > k && m[j]@[k] > bound
}

pub open spec fn complete_above(
    pat: Pattern,
    fact: Fact,
    m: Seq<Vec<usize>>,
    lo: int,
    hi: int,
    lens: Seq<usize>,
    bound: int,
) -> bool {
    forall|rest: Seq<usize>|
        #[trigger] is_match(pat, fact, lens + rest) ==> (rest.len() > 0 && rest[0] > bound
            ==> emitted(m, lo, hi, lens + rest))
}

pub open spec fn extends(m1: Seq<Vec<usize>>, m0: Seq<Vec<usize>>) -> bool {
    &&& m0.len() <= m1.len()
    &&& forall|j: int| 0 <= j < m0.len() ==> #[trigger] m1[j] == m0[j]
}

/// Appending the block of matches for capture length `len` after blocks for
/// longer lengths keeps the run sound and strictly descending.
proof fn lemma_merge_block(
    pat: Pattern,
    fact: Fact,
    mb: Seq<Vec<usize>>,
    ma: Seq<Vec<usize>>,
    lo: int,
    lens: Seq<usize>,
    len: usize,
)
    requires
        0 <= lo <= mb.len(),
        extends(ma, mb),
        sound_from(pat, fact, mb, lo, mb.len() as int, lens),
        sorted_desc(mb, lo, mb.len() as int),
        above(mb, lo, mb.len() as int, lens.len() as int, len as int),
        sound_from(pat, fact, ma, mb.len() as int, ma.len() as int, lens.push(len)),
        sorted_desc(ma, mb.len() as int, ma.len() as int),
    ensures
        sound_from(pat, fact, ma, lo, ma.len() as int, lens),
        sorted_desc(ma, lo, ma.len() as int),
        above(ma, lo, ma.len() as int, lens.len() as int, len as int - 1),
{
    let k = lens.len() as int;
    let mid = mb.len() as int;
    let hi = ma.len() as int;
    assert forall|j: int| #![trigger ma[j]] lo <= j < hi implies has_prefix(ma[j]@, lens) && is_match(
        pat,
        fact,
        ma[j]@,
    ) && ma[j]@.len() > k && ma[j]@[k] > len as int - 1 by {
        if j < mid {
            assert(ma[j] == mb[j]);
            assert(mb[j]@.len() > k && mb[j]@[k] > len);
        } else {
            let x = ma[j]@;
            assert(has_prefix(x, lens.push(len)));
            assert(x.take(k + 1)[k] == len);
            assert(x.take(k) =~= x.take(k + 1).take(k));
            assert(lens.push(len).take(k) =~= lens);
        }
    }
    assert forall|j1: int, j2: int| lo <= j1 < j2 < hi implies #[trigger] lex_gt(ma[j1]@, ma[j2]@) by {
        if j2 < mid {
            assert(ma[j1] == mb[j1]);
            assert(ma[j2] == mb[j2]);
            assert(lex_gt(mb[j1]@, mb[j2]@));
        } else if j1 >= mid {
            assert(lex_gt(ma[j1]@, ma[j2]@));
        } else {
            assert(ma[j1] == mb[j1]);
            let x = ma[j1]@;
            let y = ma[j2]@;
            assert(x.len() > k && x[k] > len);
            assert(has_prefix(x, lens));
            assert(has_prefix(y, lens.push(len)));
            assert(y.take(k + 1)[k] == len);
            assert(y.take(k) =~= y.take(k + 1).take(k));
            assert(lens.push(len).take(k) =~= lens);
            assert(lex_at(x, y, k));
        }
    }
}

/// Completeness for lengths above `len` plus the block for `len` gives
/// completeness for lengths from `len` up.
proof fn lemma_merge_complete(
    pat: Pattern,
    fact: Fact,
    mb: Seq<Vec<usize>>,
    ma: Seq<Vec<usize>>,
    lo: int,
    lens: Seq<usize>,
    len: usize,
)
    requires
        0 <= lo <= mb.len(),
        extends(ma, mb),
        complete_above(pat, fact, mb, lo, mb.len() as int, lens, len as int),
        complete_from(pat, fact, ma, mb.len() as int, ma.len() as int, lens.push(len)),
    ensures
        complete_above(pat, fact, ma, lo, ma.len() as int, lens, len as int - 1),
{
    let mid = mb.len() as int;
    let hi = ma.len() as int;
    assert forall|rest: Seq<usize>| #[trigger]
        is_match(pat, fact, lens + rest) && rest.len() > 0 && rest[0] > len as int - 1 implies emitted(
        ma,
        lo,
        hi,
        lens + rest,
    ) by {
        if rest[0] > len {
            assert(emitted(mb, lo, mid, lens + rest));
            let j = choose|j: int| lo <= j < mid && #[trigger] mb[j]@ == lens + rest;
            assert(ma[j] == mb[j]);
        } else {
            let tail = rest.drop_first();
            assert(lens.push(len) + tail =~= lens + rest);
            assert(is_match(pat, fact, lens.push(len) + tail));
            assert(emitted(ma, mid, hi, lens.push(len) + tail));
            let j = choose|j: int| mid <= j < hi && #[trigger] ma[j]@ == lens.push(len) + tail;
            assert(ma[j]@ == lens + rest);
        }
    }
}

proof fn lemma_extends_keeps(
    pat: Pattern,
    fact: Fact,
    mb: Seq<Vec<usize>>,
    ma: Seq<Vec<usize>>,
    lo: int,
    lens: Seq<usize>,
    bound: int,
)
    requires
        0 <= lo <= mb.len(),
        extends(ma, mb),
        ma.len() == mb.len(),
        sound_from(pat, fact, mb, lo, mb.len() as int, lens),
        sorted_desc(mb, lo, mb.len() as int),
        above(mb, lo, mb.len() as int, lens.len() as int, bound),
    ensures
        sound_from(pat, fact, ma, lo, ma.len() as int, lens),
        sorted_desc(ma, lo, ma.len() as int),
        above(ma, lo, ma.len() as int, lens.len() as int, bound),
{
    assert(ma =~= mb);
}

// ===================================================================
// Work-bound arithmetic
// ===================================================================

/// Adding the iterations `wi <= (si + 1) * (mp - 1)` of one capture
/// length's recursive call (`si` steps inside it) to `w <= wc + d0 * (mp -
/// 1)` (`d0` steps before it) gives the bound with `d0 + si + 1` steps: the
/// step count after the call when the capture emitted a `Step` for this
/// length, one more than it otherwise.
proof fn lemma_work_add(w: int, wc: int, d0: int, wi: int, si: int, mp: int)
    requires
        w <= wc + d0 * (mp - 1),
        wi <= (si + 1) * (mp - 1),
    ensures
        w + wi <= wc + (d0 + si + 1) * (mp - 1),
{
    assert((d0 + si + 1) * (mp - 1) == d0 * (mp - 1) + (si + 1) * (mp - 1)) by (nonlinear_arith);
}

/// `wc + (ds + 1) * (mp - 1) <= (ds + 1) * (wc + mp - 1)`: closing a call
/// that spent `wc` iterations reaching a capture with measure `mp`.
proof fn lemma_work_close(w: int, wc: int, ds: int, mp: int)
    requires
        0 <= wc,
        0 <= ds,
        w <= wc + (ds + 1) * (mp - 1),
    ensures
        w <= (ds + 1) * (wc + mp - 1),
{
    assert((ds + 1) * (wc + mp - 1) == (ds + 1) * wc + (ds + 1) * (mp - 1)) by (nonlinear_arith);
    assert(wc <= (ds + 1) * wc) by (nonlinear_arith)
        requires
            0 <= wc,
            0 <= ds,
    ;
}

/// A measure is at least 1 at every valid search position.
proof fn lemma_measure_pos(segs: Seq<Segment>, s: int, f: int)
    requires
        0 <= s < segs.len(),
        0 <= f <= segs[s].fields@.len(),
    ensures
        measure(segs, s, f) >= 1,
{
    lemma_fields_upto_mono(segs, s + 1, segs.len() as int);
    assert(fields_upto(segs, s + 1) == fields_upto(segs, s) + segs[s].fields@.len());
}

// ===================================================================
// Exec: the split search
// ===================================================================

impl<'a> SplitSearch<'a> {
    // Models crates/ferric-rules-core/src/sequence.rs:450 fn place.
    fn place(
        &mut self,
        segment0: usize,
        field0: usize,
        offset0: usize,
        extra0: usize,
        checked: usize,
        visitor: &mut Visitor,
    ) -> (broke: bool)
        requires
            old(self).wf(),
            old(self).state_ok(
                segment0 as int,
                field0 as int,
                offset0 as int,
                extra0 as int,
                checked as int,
            ),
        ensures
            final(self).same_frame(old(self)),
            final(self).wf(),
            !broke ==> final(self).split.fields@ == old(self).split.fields@,
            !broke ==> final(self).split.lengths@ == old(self).split.lengths@,
            visit_ok(
                *old(self).plan,
                *old(self).split.fact,
                *old(visitor),
                *final(visitor),
                old(self).split.lengths@,
                broke,
            ),
            // Work bound: the loop iterations of this call and its
            // recursive calls are at most (Step events + 1) times the
            // remaining fields and segments.
            old(visitor).steps@.len() <= final(visitor).steps@.len(),
            old(self).work@ <= final(self).work@,
            final(self).work@ - old(self).work@ <= (final(visitor).steps@.len()
                - old(visitor).steps@.len() + 1) * measure(
                old(self).segs(),
                segment0 as int,
                field0 as int,
            ),
        decreases measure(self.segs(), segment0 as int, field0 as int),
    {
        let ghost pat = *self.plan;
        let ghost fact = *self.split.fact;
        let ghost lens = self.split.lengths@;
        let ghost fields0 = self.split.fields@;
        let ghost v0 = *visitor;
        let ghost m0 = visitor.matches@.len() as int;
        let ghost ordered = self.split.ordered();
        let ghost segs = self.segs();
        let ghost s0 = segment0 as int;
        let ghost f0 = field0 as int;
        let ghost w0 = self.work@;
        let ghost m_all = measure(segs, s0, f0);
        let mark = self.split.fields.len();
        let plan = self.plan;
        let mut segment = segment0;
        let mut field = field0;
        let mut offset = offset0;
        let mut extra = extra0;
        loop
            invariant_except_break
                self.state_ok(
                    segment as int,
                    field as int,
                    offset as int,
                    extra as int,
                    checked as int,
                ),
                *visitor == v0,
                measure(segs, segment as int, field as int) <= measure(segs, s0, f0),
                self.work@ - w0 + measure(segs, segment as int, field as int) == m_all,
            invariant
                w0 <= self.work@,
                w0 == old(self).work@,
                m_all == measure(segs, s0, f0),
                self.same_frame(old(self)),
                self.wf(),
                plan == self.plan,
                pat == *self.plan,
                fact == *self.split.fact,
                segs == self.segs(),
                ordered == self.split.ordered(),
                v0 == *old(visitor),
                m0 == v0.matches@.len(),
                pat == *old(self).plan,
                fact == *old(self).split.fact,
                s0 == segment0 as int,
                f0 == field0 as int,
                segs == old(self).segs(),
                lens == old(self).split.lengths@,
                fields0 == old(self).split.fields@,
                self.split.lengths@ == lens,
                mark == fields0.len(),
                mark <= self.split.fields@.len(),
                self.split.fields@.take(mark as int) == fields0,
            ensures
                visit_ok(pat, fact, v0, *visitor, lens, false),
                v0.steps@.len() <= visitor.steps@.len(),
                self.work@ - w0 <= (visitor.steps@.len() - v0.steps@.len() + 1) * m_all,
            decreases measure(segs, segment as int, field as int),
        {
            proof {
                lemma_measure_pos(segs, segment as int, field as int);
            }
            self.work = Ghost(self.work@ + 1);
            let fields = &plan.segments[segment].fields;
            let values = self.sources[segment].0;
            let ghost s = segment as int;
            let ghost f = field as int;
            let ghost vals = values@;
            let ghost p = self.split.view_fields();
            proof {
                assert(source_ok(segs[s], fact, self.sources@[s]));
                assert(seg_values(segs[s].source, fact) == Some(vals));
                assert(fields_upto(segs, s + 1) == fields_upto(segs, s) + fields@.len());
                lemma_fields_upto_mono(segs, s + 1, segs.len() as int);
            }
            if field == fields.len() {
                proof {
                    assert(segment < plan.segments.len());
                }
                segment += 1;
                if segment < plan.segments.len() {
                    proof {
                        lemma_link_advance(pat, fact, lens, p, s);
                        assert(source_ok(
                            segs[segment as int],
                            fact,
                            self.sources@[segment as int],
                        ));
                    }
                    field = 0;
                    offset = 0;
                    extra = self.sources[segment].1;
                    continue;
                }
                let ok = self.ready_tests_pass(checked, true);
                proof {
                    lemma_link_end(pat, fact, lens, p, s);
                    lemma_complete_check(self.tests(), self.needs@, ordered, p, checked as int);
                    assert(lens + Seq::<usize>::empty() =~= lens);
                    assert(project(pat, fact, lens + Seq::<usize>::empty()) == Some(p));
                }
                if ok {
                    let stop = visitor.on_match(&self.split.lengths);
                    proof {
                        let m = visitor.matches@;
                        assert(m[m0]@ == lens);
                        assert(lens.take(lens.len() as int) =~= lens);
                        assert(is_match(pat, fact, m[m0]@));
                        assert(sound_from(pat, fact, m, m0, m0 + 1, lens));
                        assert forall|rest: Seq<usize>| #[trigger]
                            is_match(pat, fact, lens + rest) implies emitted(
                            m,
                            m0,
                            m0 + 1,
                            lens + rest,
                        ) by {
                            assert(project(pat, fact, lens + rest) is Some);
                            assert(rest.len() == 0);
                            assert(lens + rest =~= lens);
                            assert(m[m0]@ == lens + rest);
                        }
                    }
                    if stop {
                        return true;
                    }
                } else {
                    proof {
                        assert forall|rest: Seq<usize>| #[trigger]
                            is_match(pat, fact, lens + rest) implies emitted(
                            visitor.matches@,
                            m0,
                            m0,
                            lens + rest,
                        ) by {
                            assert(project(pat, fact, lens + rest) is Some);
                            assert(rest.len() == 0);
                        }
                    }
                }
                break;
            }
            if fields[field] == Field::Single {
                self.split.fields.push(FieldRef::Single(&values[offset]));
                proof {
                    lemma_link_single(pat, fact, lens, p, s, f, offset as int);
                    let p2 = self.split.view_fields();
                    assert(p2 =~= p.push(LVal::One(vals[offset as int])));
                    assert(p2.take(checked as int) =~= p.take(checked as int));
                    assert(self.split.fields@.take(mark as int) =~= fields0);
                }
                field += 1;
                offset += 1;
                continue;
            }
            let ok = self.ready_tests_pass(checked, false);
            if !ok {
                proof {
                    assert forall|rest: Seq<usize>| #[trigger]
                        is_match(pat, fact, lens + rest) implies emitted(
                        visitor.matches@,
                        m0,
                        m0,
                        lens + rest,
                    ) by {
                        let q = proj_state(segs, fact, s, f, offset as int, rest).unwrap();
                        assert(project(pat, fact, lens + rest) == Some(p + q));
                        assert(agree(p, p + q, p.len() as int));
                        lemma_prune(self.tests(), self.needs@, ordered, p, checked as int, p + q);
                    }
                }
                break;
            }
            proof {
                lemma_checked_advance(self.tests(), self.needs@, ordered, p, checked as int);
            }
            let ghost raw_cap = self.split.fields@;
            let checked = self.split.fields.len();
            // The segment's last capture takes whatever the others leave.
            let last = only_singles_after(fields, field + 1);
            let shortest = if last {
                extra
            } else {
                0
            };
            proof {
                assert forall|rest: Seq<usize>| #[trigger]
                    is_match(pat, fact, lens + rest) && rest.len() > 0 && rest[0]
                    > extra as int implies emitted(visitor.matches@, m0, m0, lens + rest) by {
                    lemma_capture_range(
                        pat,
                        fact,
                        lens,
                        p,
                        s,
                        f,
                        offset as int,
                        extra as int,
                        rest,
                    );
                }
            }
            let ghost wc = self.work@ - w0;
            let ghost mp = measure(segs, s, f);
            let mut length = extra;
            loop
                invariant_except_break
                    self.work@ - w0 <= wc + (visitor.steps@.len() - v0.steps@.len()) * (mp - 1),
                    shortest <= length,
                    above(
                        visitor.matches@,
                        m0,
                        visitor.matches@.len() as int,
                        lens.len() as int,
                        length as int,
                    ),
                    complete_above(
                        pat,
                        fact,
                        visitor.matches@,
                        m0,
                        visitor.matches@.len() as int,
                        lens,
                        length as int,
                    ),
                invariant
                    length <= extra,
                    shortest <= extra,
                    shortest == (if last {
                        extra
                    } else {
                        0
                    }),
                    last == (multis_from(fields@, f + 1) == 0),
                    0 <= f < fields@.len(),
                    fields@[f] == Field::Multi,
                    fields@ == segs[s].fields@,
                    s == segment,
                    f == field,
                    values@ == vals,
                    self.sources@[s].0@ == vals,
                    seg_values(segs[s].source, fact) == Some(vals),
                    fields_upto(segs, s) + fields@.len() <= width(segs),
                    p == self.split.view_fields(),
                    p.len() < usize::MAX,
                    checked == p.len(),
                    self.split.fields@ == raw_cap,
                    self.split.lengths@ == lens,
                    self.state_ok(s, f, offset as int, extra as int, checked as int),
                    self.same_frame(old(self)),
                    self.wf(),
                    plan == self.plan,
                    pat == *self.plan,
                    fact == *self.split.fact,
                    segs == self.segs(),
                    ordered == self.split.ordered(),
                    v0 == *old(visitor),
                    m0 == v0.matches@.len(),
                    pat == *old(self).plan,
                    fact == *old(self).split.fact,
                    s0 == segment0 as int,
                    f0 == field0 as int,
                    segs == old(self).segs(),
                    lens == old(self).split.lengths@,
                    fields0 == old(self).split.fields@,
                    mark <= raw_cap.len(),
                    raw_cap.take(mark as int) == fields0,
                    measure(segs, s, f) <= measure(segs, s0, f0),
                    extends(visitor.matches@, v0.matches@),
                    visitor.stop_on_match == v0.stop_on_match,
                    visitor.step_budget == v0.step_budget,
                    sound_from(pat, fact, visitor.matches@, m0, visitor.matches@.len() as int, lens),
                    sorted_desc(visitor.matches@, m0, visitor.matches@.len() as int),
                    visitor.stop_on_match ==> visitor.matches@.len() == m0,
                    w0 <= self.work@,
                    w0 == old(self).work@,
                    m_all == measure(segs, s0, f0),
                    0 <= wc,
                    wc + mp == m_all + 1,
                    mp == measure(segs, s, f),
                    mp >= 2,
                    v0.steps@.len() <= visitor.steps@.len(),
                ensures
                    self.work@ - w0 <= wc + (visitor.steps@.len() - v0.steps@.len() + 1) * (mp
                        - 1),
                    complete_above(
                        pat,
                        fact,
                        visitor.matches@,
                        m0,
                        visitor.matches@.len() as int,
                        lens,
                        shortest as int - 1,
                    ),
                decreases length,
            {
                let ghost d0 = visitor.steps@.len() - v0.steps@.len();
                if !last {
                    if visitor.on_step() {
                        proof {
                            assert(wc + d0 * (mp - 1) <= wc + (d0 + 1) * (mp - 1)) by (nonlinear_arith)
                                requires
                                    mp >= 1,
                            ;
                            lemma_work_close(self.work@ - w0, wc, d0, mp);
                        }
                        return true;
                    }
                }
                let ghost vb = *visitor;
                let ghost wb = self.work@;
                proof {
                    assert(offset as int + length as int <= vals.len());
                    assert(vals.len() == values.len());
                }
                self.split.fields.push(FieldRef::Multi(slice_subrange(values, offset, offset + length)));
                self.split.lengths.push(length);
                proof {
                    lemma_link_multi(pat, fact, lens, p, s, f, offset as int, length);
                    let p2 = self.split.view_fields();
                    assert(p2 =~= p.push(
                        LVal::Many(vals.subrange(offset as int, offset as int + length as int)),
                    ));
                    assert(p2.take(checked as int) =~= p);
                    assert(p.take(checked as int) =~= p);
                    assert(singles_from(fields@, f) == singles_from(fields@, f + 1));
                    assert(measure(segs, s, f + 1) == measure(segs, s, f) - 1);
                    assert(measure(segs, s, f + 1) >= 0);
                    assert(self.segs() == old(self).segs());
                }
                let broke = self.place(
                    segment,
                    field + 1,
                    offset + length,
                    extra - length,
                    checked,
                    visitor,
                );
                proof {
                    lemma_merge_block(
                        pat,
                        fact,
                        vb.matches@,
                        visitor.matches@,
                        m0,
                        lens,
                        length,
                    );
                    // Work: this length's subtree adds at most
                    // (its steps + 1) * (mp - 1) iterations.
                    let si = visitor.steps@.len() - vb.steps@.len();
                    let ds = visitor.steps@.len() - v0.steps@.len();
                    lemma_work_add(wb - w0, wc, d0, self.work@ - wb, si, mp);
                    if !last {
                        assert(ds == d0 + 1 + si);
                        assert(wc + ds * (mp - 1) <= wc + (ds + 1) * (mp - 1)) by (nonlinear_arith)
                            requires
                                mp >= 1,
                        ;
                    } else {
                        assert(ds == d0 + si);
                    }
                    assert(self.work@ - w0 <= wc + (ds + 1) * (mp - 1));
                    lemma_work_close(self.work@ - w0, wc, ds, mp);
                }
                if broke {
                    return true;
                }
                self.split.fields.pop();
                self.split.lengths.pop();
                proof {
                    assert(self.split.fields@ =~= raw_cap);
                    assert(self.split.lengths@ =~= lens);
                    lemma_merge_complete(
                        pat,
                        fact,
                        vb.matches@,
                        visitor.matches@,
                        m0,
                        lens,
                        length,
                    );
                }
                if length == shortest {
                    break;
                }
                length -= 1;
            }
            proof {
                assert forall|rest: Seq<usize>| #[trigger]
                    is_match(pat, fact, lens + rest) implies emitted(
                    visitor.matches@,
                    m0,
                    visitor.matches@.len() as int,
                    lens + rest,
                ) by {
                    lemma_capture_range(
                        pat,
                        fact,
                        lens,
                        p,
                        s,
                        f,
                        offset as int,
                        extra as int,
                        rest,
                    );
                }
                lemma_work_close(
                    self.work@ - w0,
                    wc,
                    visitor.steps@.len() - v0.steps@.len(),
                    mp,
                );
            }
            break;
        }
        self.split.fields.truncate(mark);
        proof {
            assert(self.split.fields@ =~= fields0);
        }
        false
    }
}

// ===================================================================
// Entry points and theorems
// ===================================================================

// Models crates/ferric-rules-core/src/sequence.rs:265 fn search_with_prefix
// with the always-true prefix filter, i.e. sequence.rs:250 fn search. The
// result is production's `ControlFlow::is_break()` paired with a ghost
// component (erased; no production counterpart): the number of `place`
// loop iterations the search ran.
pub fn search(plan: &Pattern, fact: &Fact, visitor: &mut Visitor) -> (r: (bool, Ghost<nat>))
    requires
        width(plan.segments@) < usize::MAX,
    ensures
        visit_ok(*plan, *fact, *old(visitor), *final(visitor), Seq::empty(), r.0),
        // Work bound: the search runs at most (Step events + 1) * (logical
        // width + segments) loop iterations, so a caller charging a fixed
        // cost per `Step` (snapshot.rs `any_sequence_match`) bounds it.
        old(visitor).steps@.len() <= final(visitor).steps@.len(),
        r.1@ <= (final(visitor).steps@.len() - old(visitor).steps@.len() + 1) * (width(
            plan.segments@,
        ) + plan.segments@.len()),
{
    let ghost v0 = *visitor;
    let mut sources: Vec<(&[Val], usize)> = Vec::new();
    let mut k: usize = 0;
    while k < plan.segments.len()
        invariant
            0 <= k <= plan.segments@.len(),
            sources@.len() == k,
            forall|k0: int|
                0 <= k0 < k ==> #[trigger] source_ok(plan.segments@[k0], *fact, sources@[k0]),
            *visitor == v0,
        decreases plan.segments@.len() - k,
    {
        let segment = &plan.segments[k];
        let values = match segment_values(&segment.source, fact) {
            Some(values) => values,
            None => {
                proof {
                    assert forall|rest: Seq<usize>| #[trigger]
                        is_match(*plan, *fact, Seq::<usize>::empty() + rest) implies emitted(
                        visitor.matches@,
                        v0.matches@.len() as int,
                        v0.matches@.len() as int,
                        Seq::<usize>::empty() + rest,
                    ) by {
                        lemma_bad_segment(plan.segments@, *fact, 0, k as int, Seq::<usize>::empty() + rest);
                    }
                }
                return (false, Ghost(0));
            },
        };
        let extra = match segment_extra(&segment.fields, values) {
            Some(extra) => extra,
            None => {
                proof {
                    assert forall|rest: Seq<usize>| #[trigger]
                        is_match(*plan, *fact, Seq::<usize>::empty() + rest) implies emitted(
                        visitor.matches@,
                        v0.matches@.len() as int,
                        v0.matches@.len() as int,
                        Seq::<usize>::empty() + rest,
                    ) by {
                        lemma_bad_segment(plan.segments@, *fact, 0, k as int, Seq::<usize>::empty() + rest);
                    }
                }
                return (false, Ghost(0));
            },
        };
        sources.push((values, extra));
        k += 1;
    }
    if sources.len() == 0 {
        return (false, Ghost(0));
    }
    let extra = sources[0].1;
    let mut needs: Vec<usize> = Vec::new();
    let mut i: usize = 0;
    while i < plan.tests.len()
        invariant
            0 <= i <= plan.tests@.len(),
            needs@.len() == i,
            forall|i0: int|
                0 <= i0 < i ==> #[trigger] needs@[i0] == sat(extent(plan.tests@[i0])),
        decreases plan.tests@.len() - i,
    {
        needs.push(test_extent(&plan.tests[i]));
        i += 1;
    }
    let mut search = SplitSearch {
        plan,
        sources,
        needs,
        split: SplitView { fact, fields: Vec::new(), lengths: Vec::new() },
        work: Ghost(0),
    };
    proof {
        lemma_link_start(*plan, *fact);
        assert(search.split.view_fields() =~= Seq::<LVal>::empty());
        assert(source_ok(plan.segments@[0], *fact, search.sources@[0]));
        assert forall|i0: int|
            0 <= i0 < search.tests().len() && search.needs@[i0] <= 0 implies #[trigger] eval(
            search.tests()[i0],
            search.split.ordered(),
            search.split.view_fields().take(0),
        ) by {
            assert(search.needs@[i0] == sat(extent(search.tests()[i0])));
        }
    }
    let broke = search.place(0, 0, 0, extra, 0, visitor);
    proof {
        assert(fields_upto(plan.segments@, 0) == 0);
        assert(measure(plan.segments@, 0, 0) == width(plan.segments@) + plan.segments@.len());
    }
    (broke, Ghost(search.work@))
}

/// Every matching split, in the order the search reports them.
// Models crates/ferric-rules-core/src/sequence.rs:359 fn matches (the
// collect-everything visitor over `search`).
pub fn search_all(plan: &Pattern, fact: &Fact) -> (r: Vec<Vec<usize>>)
    requires
        width(plan.segments@) < usize::MAX,
    ensures
// Soundness: every reported vector is a split that passes every test.

        forall|j: int| 0 <= j < r@.len() ==> #[trigger] is_match(*plan, *fact, r@[j]@),
        // Completeness: every split that passes every test is reported.
        forall|lens: Seq<usize>| #[trigger]
            is_match(*plan, *fact, lens) ==> exists|j: int| 0 <= j < r@.len() && r@[j]@ == lens,
        // Order: strictly descending lexicographically (CLIPS order), so no
        // split is reported twice.
        forall|j1: int, j2: int| 0 <= j1 < j2 < r@.len() ==> #[trigger] lex_gt(r@[j1]@, r@[j2]@),
{
    let mut visitor = Visitor {
        matches: Vec::new(),
        steps: Vec::new(),
        stop_on_match: false,
        step_budget: None,
    };
    let (broke, _work) = search(plan, fact, &mut visitor);
    proof {
        assert(!broke);
        assert forall|lens: Seq<usize>| #[trigger]
            is_match(*plan, *fact, lens) implies exists|j: int|
            0 <= j < visitor.matches@.len() && visitor.matches@[j]@ == lens by {
            assert(Seq::<usize>::empty() + lens =~= lens);
            assert(emitted(visitor.matches@, 0, visitor.matches@.len() as int, Seq::<usize>::empty() + lens));
        }
    }
    visitor.matches
}

// Models crates/ferric-rules-core/src/sequence.rs:341 fn admits.
pub fn admits(plan: &Pattern, fact: &Fact) -> (r: bool)
    requires
        width(plan.segments@) < usize::MAX,
    ensures
        r == exists|lens: Seq<usize>| is_match(*plan, *fact, lens),
{
    let mut visitor = Visitor {
        matches: Vec::new(),
        steps: Vec::new(),
        stop_on_match: true,
        step_budget: None,
    };
    let (broke, _work) = search(plan, fact, &mut visitor);
    proof {
        if broke {
            assert(is_match(*plan, *fact, visitor.matches@[0]@));
        } else {
            assert forall|lens: Seq<usize>| !is_match(*plan, *fact, lens) by {
                assert(Seq::<usize>::empty() + lens =~= lens);
                if is_match(*plan, *fact, Seq::<usize>::empty() + lens) {
                    assert(emitted(visitor.matches@, 0, 0, Seq::<usize>::empty() + lens));
                }
            }
        }
    }
    broke
}

/// Strict lexicographic order is irreflexive, so a descending run has no
/// duplicate splits.
proof fn lemma_sorted_distinct(m: Seq<Vec<usize>>, lo: int, hi: int)
    requires
        sorted_desc(m, lo, hi),
    ensures
        forall|j1: int, j2: int| lo <= j1 < j2 < hi ==> m[j1]@ != m[j2]@,
{
    assert forall|j1: int, j2: int| lo <= j1 < j2 < hi implies m[j1]@ != m[j2]@ by {
        assert(lex_gt(m[j1]@, m[j2]@));
        let k = choose|k: int| #[trigger] lex_at(m[j1]@, m[j2]@, k);
    }
}


// ===================================================================
// Non-vacuity: the spec and the exec search accept and reject real input
// ===================================================================

/// The pattern `(row 7 $?)`: a single field that must equal 7, then a
/// capture. `admits` accepts `(row 7 8)` (the split `[1]`) and rejects
/// `(row 9 8)`, so neither the spec nor the theorems are vacuous.
fn example_admits() {
    let mut fields: Vec<Field> = Vec::new();
    fields.push(Field::Single);
    fields.push(Field::Multi);
    let mut segments: Vec<Segment> = Vec::new();
    segments.push(Segment { source: Source::Ordered, fields });
    let mut tests: Vec<Test> = Vec::new();
    tests.push(Test { slot: Slot::Ordered(0), test_type: TestType::Equal(7) });
    let pat = Pattern { segments, tests };
    let mut values: Vec<Val> = Vec::new();
    values.push(Val::Atom(7));
    values.push(Val::Atom(8));
    let fact = Fact::Ordered(values);
    let ghost segs = pat.segments@;
    let ghost fs = segs[0].fields@;
    let ghost vals = values@;
    proof {
        assert(fields_upto(segs, 0) == 0);
        assert(fields_upto(segs, 1) == 2);
        assert(width(segs) == 2);
        let lens = seq![1usize];
        assert(lens.drop_first() =~= Seq::<usize>::empty());
        assert(seg_values(segs[0].source, fact) == Some(vals));
        assert(proj_seg_from(fs, vals, 2, 2, Seq::empty()) == Some(
            (Seq::<LVal>::empty(), Seq::<usize>::empty()),
        ));
        let many = LVal::Many(vals.subrange(1, 2));
        assert(proj_seg_from(fs, vals, 1, 1, lens) == Some(
            (seq![many] + Seq::<LVal>::empty(), Seq::<usize>::empty()),
        ));
        let p = seq![LVal::One(vals[0])] + (seq![many] + Seq::<LVal>::empty());
        assert(proj_seg_from(fs, vals, 0, 0, lens) == Some((p, Seq::<usize>::empty())));
        assert(proj_segs_from(segs, fact, 1, Seq::empty()) == Some(Seq::<LVal>::empty()));
        assert(project(pat, fact, lens) == Some(p + Seq::<LVal>::empty()));
        assert((p + Seq::<LVal>::empty())[0] == LVal::One(Val::Atom(7)));
        assert(eval(pat.tests@[0], true, p + Seq::<LVal>::empty()));
        assert(is_match(pat, fact, lens));
    }
    let accepted = admits(&pat, &fact);
    assert(accepted);

    let mut other: Vec<Val> = Vec::new();
    other.push(Val::Atom(9));
    other.push(Val::Atom(8));
    let rejected = Fact::Ordered(other);
    let ghost others = other@;
    proof {
        assert forall|lens: Seq<usize>| !is_match(pat, rejected, lens) by {
            assert(seg_values(segs[0].source, rejected) == Some(others));
            if proj_seg_from(fs, others, 0, 0, lens) is Some {
                let tail = proj_seg_from(fs, others, 1, 1, lens).unwrap();
                assert(proj_seg_from(fs, others, 0, 0, lens).unwrap().0 == seq![
                    LVal::One(others[0]),
                ] + tail.0);
                if project(pat, rejected, lens) is Some {
                    let q = proj_segs_from(segs, rejected, 1, tail.1).unwrap();
                    let p = (seq![LVal::One(others[0])] + tail.0) + q;
                    assert(project(pat, rejected, lens) == Some(p));
                    assert(p[0] == LVal::One(Val::Atom(9)));
                    assert(!eval(pat.tests@[0], true, p));
                }
            }
        }
    }
    let admitted = admits(&pat, &rejected);
    assert(!admitted);
}

} // verus!

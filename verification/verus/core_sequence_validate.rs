//! Verus model of `SequencePattern::validate`
//! (crates/ferric-rules-core/src/sequence.rs:169): its source checks, with
//! `is_ordered` (sequence.rs:73), and its selector loop over the tests.
//!
//! What is proved (unbounded: any number of segments and fields, any slot
//! indices, any number and nesting depth of tests):
//!
//! * `validate_sources` (an executable model of sequence.rs:170-197)
//!   returns `true` exactly when `sources_valid` holds: the plan has a
//!   segment, and either it is a single ordered segment, or every segment
//!   is a template multislot source or a scalar template source projecting
//!   exactly one single field, with no template slot used twice.
//! * `theorem_check_pending`: the selector loop (sequence.rs:201-226,
//!   modelled by the spec function `check_pending`, which pops the last
//!   pending test and queues a disjunction's branches in order) accepts a
//!   list of tests exactly when every test, nested ones included, reads a
//!   selector of the projection's kind below its width (both selectors of a
//!   slot pair) and is not a whole-fact constraint (`all_ok`).
//!
//! `validate` returns `Ok` exactly when the source checks pass,
//! `constant_test_count` is at most 64, and the selector loop passes; the
//! count is not modelled here (it is `alpha::constant_test_count`).
//!
//! Kani cannot reach either part on the production code: every harness
//! that reached the duplicate check's hash-set insertion, or that iterated
//! over a heap-allocated list of `ConstantTest`s (whose `Any` variant makes
//! symbolic execution explore nested heap data), ran for more than ten
//! minutes or out of memory (see
//! crates/ferric-rules-core/src/sequence/kani_proofs.rs).
//!
//! Abstractions: `FxHashSet<usize>` is modelled by std `HashSet<usize>`
//! (vstd specification): only set semantics matter (`insert` returns
//! whether the key was absent); the hasher is not modelled. `Err` results
//! are modelled as `false`. A `ConstantTest` is modelled by what `validate`
//! reads (`TestT`: its slot and variant group); the selector validator is
//! `valid_slot`, the logic of `logical_slot_validator` (sequence.rs:84),
//! which Kani proves on production (`projection_helpers_are_exact`).
//!
//! Trusted base: vstd, including its `HashSet` specifications. No `assume`,
//! `admit`, `external_body` or other trusted glue is used.
use vstd::prelude::*;
use std::collections::HashSet;

verus! {

broadcast use vstd::std_specs::hash::group_hash_axioms;

/// Models `SequenceField` (crates/ferric-rules-core/src/sequence.rs:19).
pub enum Field {
    Single,
    Multi,
}

/// Models `SequenceSource` (crates/ferric-rules-core/src/sequence.rs:27).
pub enum Source {
    Ordered,
    TemplateSlot(usize),
    TemplateScalar(usize),
}

/// Models `SequenceSegment` (crates/ferric-rules-core/src/sequence.rs:40).
pub struct Segment {
    pub source: Source,
    pub fields: Vec<Field>,
}

pub ghost struct SegmentV {
    pub source: Source,
    pub fields: Seq<Field>,
}

pub open spec fn view_segments(segments: Seq<Segment>) -> Seq<SegmentV> {
    segments.map_values(|s: Segment| SegmentV { source: s.source, fields: s.fields@ })
}

pub open spec fn spec_is_ordered(segs: Seq<SegmentV>) -> bool {
    segs.len() == 1 && segs[0].source is Ordered
}

/// The slot a well-formed template segment names, if it is one.
pub open spec fn template_slot(seg: SegmentV) -> Option<usize> {
    match seg.source {
        Source::TemplateSlot(index) => Some(index),
        Source::TemplateScalar(index) => if seg.fields.len() == 1 && seg.fields[0] is Single {
            Some(index)
        } else {
            None
        },
        Source::Ordered => None,
    }
}

/// The documented well-formedness of a plan's sources.
pub open spec fn sources_valid(segs: Seq<SegmentV>) -> bool {
    &&& segs.len() > 0
    &&& (spec_is_ordered(segs) || {
        &&& forall|s: int| 0 <= s < segs.len() ==> #[trigger] template_slot(segs[s]) is Some
        &&& forall|s: int, t: int|
            0 <= s < t < segs.len() ==> template_slot(#[trigger] segs[s])->0 != template_slot(
                #[trigger] segs[t],
            )->0
    })
}

/// Models `SequencePattern::is_ordered` (crates/ferric-rules-core/src/sequence.rs:73).
pub fn is_ordered(segments: &Vec<Segment>) -> (r: bool)
    ensures
        r == spec_is_ordered(view_segments(segments@)),
{
    if segments.len() != 1 {
        return false;
    }
    match segments[0].source {
        Source::Ordered => true,
        _ => false,
    }
}

/// Models the source checks of `SequencePattern::validate`
/// (crates/ferric-rules-core/src/sequence.rs:170-197); `true` stands for
/// reaching the test checks, `false` for returning an `Err`.
pub fn validate_sources(segments: &Vec<Segment>) -> (r: bool)
    ensures
        r == sources_valid(view_segments(segments@)),
{
    let ghost segs = view_segments(segments@);
    if segments.len() == 0 {
        return false;
    }
    if !is_ordered(segments) {
        let mut sources = HashSet::<usize>::new();
        let mut i: usize = 0;
        while i < segments.len()
            invariant
                segs == view_segments(segments@),
                !spec_is_ordered(segs),
                i <= segments.len(),
                forall|s: int| 0 <= s < i ==> #[trigger] template_slot(segs[s]) is Some,
                forall|s: int, t: int|
                    0 <= s < t < i ==> template_slot(#[trigger] segs[s])->0 != template_slot(
                        #[trigger] segs[t],
                    )->0,
                forall|k: usize|
                    sources@.contains(k) <==> exists|s: int|
                        0 <= s < i && #[trigger] template_slot(segs[s]) == Some(k),
            decreases segments.len() - i,
        {
            let segment = &segments[i];
            let index = match segment.source {
                Source::TemplateSlot(index) => index,
                Source::TemplateScalar(index) => {
                    let single = segment.fields.len() == 1 && match segment.fields[0] {
                        Field::Single => true,
                        Field::Multi => false,
                    };
                    if !single {
                        // "scalar template sequence source must project one single field"
                        proof {
                            assert(template_slot(segs[i as int]) is None);
                        }
                        return false;
                    }
                    index
                },
                Source::Ordered => {
                    // "ordered sequence source must be the only segment"
                    proof {
                        assert(template_slot(segs[i as int]) is None);
                    }
                    return false;
                },
            };
            proof {
                assert(template_slot(segs[i as int]) == Some(index));
            }
            if !sources.insert(index) {
                // "sequence pattern repeats a template slot source"
                proof {
                    let s = choose|s: int| 0 <= s < i && #[trigger] template_slot(segs[s]) == Some(index);
                    assert(template_slot(segs[s])->0 == template_slot(segs[i as int])->0);
                }
                return false;
            }
            i += 1;
        }
    }
    true
}

// ===========================================================================
// Test checks
// ===========================================================================

/// Models `SlotIndex` (crates/ferric-rules-core/src/alpha.rs:21).
pub enum Slot {
    Ordered(usize),
    Template(usize),
}

/// Models a `ConstantTest` (crates/ferric-rules-core/src/alpha.rs:44) by
/// what `validate` inspects: its slot, and its variant grouped as atom tests
/// (`Equal`, `NotEqual`, `EqualAny` and the four comparisons against a
/// key), slot pairs (`EqualSlot`, `NotEqualSlot` and the six offset
/// comparisons, with the other slot), whole-fact constraints
/// (`OrderedFieldCount`, `Sequence`) and disjunctions (`Any`).
pub enum TestT {
    Atom(Slot),
    Pair(Slot, Slot),
    Whole(Slot),
    Any(Slot, Seq<Seq<TestT>>),
}

pub open spec fn slot_of(t: TestT) -> Slot {
    match t {
        TestT::Atom(s) => s,
        TestT::Pair(s, _) => s,
        TestT::Whole(s) => s,
        TestT::Any(s, _) => s,
    }
}

/// Models `logical_slot_validator` (crates/ferric-rules-core/src/sequence.rs:84).
pub open spec fn valid_slot(ordered: bool, width: nat, s: Slot) -> bool {
    match s {
        Slot::Ordered(i) => ordered && i < width,
        Slot::Template(i) => !ordered && i < width,
    }
}

pub open spec fn size(t: TestT) -> nat
    decreases t,
{
    match t {
        TestT::Any(_, b) => 1 + sizes2(b),
        _ => 1,
    }
}

pub open spec fn sizes(ts: Seq<TestT>) -> nat
    decreases ts,
{
    if ts.len() == 0 {
        0
    } else {
        sizes(ts.drop_last()) + size(ts.last())
    }
}

pub open spec fn sizes2(b: Seq<Seq<TestT>>) -> nat
    decreases b,
{
    if b.len() == 0 {
        0
    } else {
        sizes2(b.drop_last()) + sizes(b.last())
    }
}

/// `branches.iter().flatten()`: the branches' tests, in order.
pub open spec fn flatten(b: Seq<Seq<TestT>>) -> Seq<TestT>
    decreases b.len(),
{
    if b.len() == 0 {
        seq![]
    } else {
        flatten(b.drop_last()) + b.last()
    }
}

pub proof fn lemma_sizes_concat(a: Seq<TestT>, b: Seq<TestT>)
    ensures
        sizes(a + b) == sizes(a) + sizes(b),
    decreases b.len(),
{
    if b.len() == 0 {
        assert(a + b =~= a);
    } else {
        lemma_sizes_concat(a, b.drop_last());
        assert((a + b).drop_last() =~= a + b.drop_last());
    }
}

pub proof fn lemma_sizes_flatten(b: Seq<Seq<TestT>>)
    ensures
        sizes(flatten(b)) == sizes2(b),
    decreases b.len(),
{
    if b.len() > 0 {
        lemma_sizes_flatten(b.drop_last());
        lemma_sizes_concat(flatten(b.drop_last()), b.last());
    }
}

pub proof fn lemma_check_decreases(pending: Seq<TestT>)
    requires
        pending.len() > 0,
    ensures
        sizes(pending.drop_last()) < sizes(pending),
        pending.last() is Any ==> sizes(pending.drop_last() + flatten(pending.last()->Any_1))
            < sizes(pending),
{
    assert(sizes(pending) == sizes(pending.drop_last()) + size(pending.last()));
    assert(size(pending.last()) >= 1);
    if pending.last() is Any {
        let b = pending.last()->Any_1;
        assert(size(pending.last()) == 1 + sizes2(b));
        lemma_sizes_concat(pending.drop_last(), flatten(b));
        lemma_sizes_flatten(b);
    }
}

/// The selector loop of `validate` (crates/ferric-rules-core/src/sequence.rs:201-226):
/// pop the last pending test; reject an invalid selector; queue the tests
/// of every branch of a disjunction; reject whole-fact constraints and slot
/// pairs whose other selector is invalid. `true` stands for `Ok(())`.
pub open spec fn check_pending(ordered: bool, width: nat, pending: Seq<TestT>) -> bool
    decreases sizes(pending),
{
    if pending.len() == 0 {
        true
    } else {
        let t = pending.last();
        let rest = pending.drop_last();
        if !valid_slot(ordered, width, slot_of(t)) {
            false
        } else {
            match t {
                TestT::Any(_, b) => {
                    proof {
                        lemma_check_decreases(pending);
                    }
                    check_pending(ordered, width, rest + flatten(b))
                },
                TestT::Whole(_) => false,
                TestT::Pair(_, other) => if !valid_slot(ordered, width, other) {
                    false
                } else {
                    proof {
                        lemma_check_decreases(pending);
                    }
                    check_pending(ordered, width, rest)
                },
                TestT::Atom(_) => {
                    proof {
                        lemma_check_decreases(pending);
                    }
                    check_pending(ordered, width, rest)
                },
            }
        }
    }
}

/// The documented validity of a test: it reads a selector of the
/// projection's kind below its width (both selectors of a slot pair), it is
/// not a whole-fact constraint, and every test nested in a disjunction is
/// valid.
pub open spec fn test_ok(ordered: bool, width: nat, t: TestT) -> bool
    decreases t,
{
    &&& valid_slot(ordered, width, slot_of(t))
    &&& match t {
        TestT::Atom(_) => true,
        TestT::Pair(_, other) => valid_slot(ordered, width, other),
        TestT::Whole(_) => false,
        TestT::Any(_, b) => forall|i: int, j: int|
            0 <= i < b.len() && 0 <= j < b[i].len() ==> test_ok(ordered, width, #[trigger] b[i][j]),
    }
}

pub open spec fn all_ok(ordered: bool, width: nat, ts: Seq<TestT>) -> bool {
    forall|k: int| 0 <= k < ts.len() ==> test_ok(ordered, width, #[trigger] ts[k])
}

pub proof fn lemma_flatten_members(b: Seq<Seq<TestT>>, ordered: bool, width: nat)
    ensures
        all_ok(ordered, width, flatten(b)) <==> forall|i: int, j: int|
            0 <= i < b.len() && 0 <= j < b[i].len() ==> test_ok(ordered, width, #[trigger] b[i][j]),
    decreases b.len(),
{
    if b.len() > 0 {
        let init = b.drop_last();
        lemma_flatten_members(init, ordered, width);
        let f = flatten(init);
        let l = b.last();
        assert(flatten(b) == f + l);
        if all_ok(ordered, width, flatten(b)) {
            assert forall|i: int, j: int|
                0 <= i < b.len() && 0 <= j < b[i].len() implies test_ok(
                ordered,
                width,
                #[trigger] b[i][j],
            ) by {
                if i < b.len() - 1 {
                    assert(init[i] == b[i]);
                    assert forall|k: int| 0 <= k < f.len() implies test_ok(
                        ordered,
                        width,
                        #[trigger] f[k],
                    ) by {
                        assert((f + l)[k] == f[k]);
                    }
                } else {
                    assert((f + l)[f.len() + j] == l[j]);
                }
            }
        }
        if forall|i: int, j: int|
            0 <= i < b.len() && 0 <= j < b[i].len() ==> test_ok(ordered, width, #[trigger] b[i][j]) {
            assert forall|i: int, j: int|
                0 <= i < init.len() && 0 <= j < init[i].len() implies test_ok(
                ordered,
                width,
                #[trigger] init[i][j],
            ) by {
                assert(init[i] == b[i]);
            }
            assert forall|k: int| 0 <= k < flatten(b).len() implies test_ok(
                ordered,
                width,
                #[trigger] flatten(b)[k],
            ) by {
                if k < f.len() {
                    assert((f + l)[k] == f[k]);
                } else {
                    assert((f + l)[k] == l[k - f.len()]);
                    assert(b[b.len() - 1] == l);
                }
            }
        }
    }
}

/// The selector loop accepts exactly the test lists whose every test,
/// nested ones included, is valid — whatever the nesting depth.
pub proof fn theorem_check_pending(ordered: bool, width: nat, pending: Seq<TestT>)
    ensures
        check_pending(ordered, width, pending) == all_ok(ordered, width, pending),
    decreases sizes(pending),
{
    if pending.len() > 0 {
        let t = pending.last();
        let rest = pending.drop_last();
        lemma_check_decreases(pending);
        assert(all_ok(ordered, width, pending) == (all_ok(ordered, width, rest) && test_ok(
            ordered,
            width,
            t,
        ))) by {
            if all_ok(ordered, width, rest) && test_ok(ordered, width, t) {
                assert forall|k: int| 0 <= k < pending.len() implies test_ok(
                    ordered,
                    width,
                    #[trigger] pending[k],
                ) by {
                    if k < rest.len() {
                        assert(rest[k] == pending[k]);
                    }
                }
            }
            if all_ok(ordered, width, pending) {
                assert forall|k: int| 0 <= k < rest.len() implies test_ok(
                    ordered,
                    width,
                    #[trigger] rest[k],
                ) by {
                    assert(rest[k] == pending[k]);
                }
                assert(pending[pending.len() - 1] == t);
            }
        }
        if valid_slot(ordered, width, slot_of(t)) {
            match t {
                TestT::Any(_, b) => {
                    let next = rest + flatten(b);
                    theorem_check_pending(ordered, width, next);
                    lemma_flatten_members(b, ordered, width);
                    let f = flatten(b);
                    assert(all_ok(ordered, width, next) == (all_ok(ordered, width, rest) && all_ok(
                        ordered,
                        width,
                        f,
                    ))) by {
                        if all_ok(ordered, width, next) {
                            assert forall|k: int| 0 <= k < rest.len() implies test_ok(
                                ordered,
                                width,
                                #[trigger] rest[k],
                            ) by {
                                assert(next[k] == rest[k]);
                            }
                            assert forall|k: int| 0 <= k < f.len() implies test_ok(
                                ordered,
                                width,
                                #[trigger] f[k],
                            ) by {
                                assert(next[rest.len() + k] == f[k]);
                            }
                        }
                        if all_ok(ordered, width, rest) && all_ok(ordered, width, f) {
                            assert forall|k: int| 0 <= k < next.len() implies test_ok(
                                ordered,
                                width,
                                #[trigger] next[k],
                            ) by {
                                if k < rest.len() {
                                    assert(next[k] == rest[k]);
                                } else {
                                    assert(next[k] == f[k - rest.len()]);
                                }
                            }
                        }
                    }
                },
                TestT::Pair(_, other) => {
                    if valid_slot(ordered, width, other) {
                        theorem_check_pending(ordered, width, rest);
                    }
                },
                TestT::Atom(_) => {
                    theorem_check_pending(ordered, width, rest);
                },
                TestT::Whole(_) => {},
            }
        }
    }
}
} // verus!

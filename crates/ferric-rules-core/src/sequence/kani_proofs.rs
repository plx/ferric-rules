//! Kani proof harnesses for `sequence` (see docs/verification.md).
//!
//! What is proved, on the production code (references are written in this
//! file from the documented semantics, not copied from `sequence.rs`):
//!
//! * `segment_extra_is_exact`: for every field list of up to six fields and
//!   every source of up to eight values, `segment_extra` is `Some(e)`
//!   exactly when some vector of capture lengths (any `usize` values) tiles
//!   the source, and then every tiling's captures sum to `e`.
//! * `test_extent_is_exact`: for every non-`Any` test variant and arbitrary
//!   selectors (any kind, any `usize` index), `test_extent` is one past the
//!   largest selector the variant reads, saturating at `usize::MAX`.
//! * `atom_tests_read_below_extent`: evaluating `Equal`, `NotEqual` and
//!   `EqualAny` through `evaluate_field_test` reads no field at or past the
//!   test's extent. With `test_extent_is_exact` this is the locality premise
//!   of the Verus split-search theorem for these tests.
//! * `projection_helpers_are_exact`: `logical_width`, `is_ordered`,
//!   `valid_logical_slot` and `logical_slot_validator` agree with their
//!   references on every plan of up to three segments of up to three fields
//!   (symbolic sources and fields) and every selector up to index 10.
//! * `physical_selector_is_exact_and_sound`: `physical_selector` agrees with
//!   its reference (an ordered field preceded only by single fields keeps
//!   its position; a field of a scalar template source is that slot) on the
//!   same plans, and on ordered plans the selected field is a single field
//!   at that physical position in every split of a fact of up to eight
//!   values.
//! * `physical_test_is_exact`: for `Equal`, `EqualSlot`, `LessThanSlotOffset`,
//!   `OrderedFieldCount` and `Sequence` tests over those plans,
//!   `physical_test` maps a test exactly when every selector it reads has a
//!   fixed physical position, rewriting exactly those selectors, and never
//!   maps a whole-fact constraint.
//! * `physical_test_maps_every_slot_pair` and
//!   `physical_test_maps_every_key_test`: the same exactness for every
//!   other non-`Any` variant (`NotEqualSlot` and the other five offset
//!   comparisons; `NotEqual`, `EqualAny` with one key and the four ordered
//!   key comparisons, with integer keys): the variant, offset and key are
//!   kept, both selectors of a slot pair are rewritten, and the test is
//!   mapped exactly when every selector it reads has a fixed position.
//! * `segment_values_is_exact`: `segment_values` returns exactly (same
//!   address and length) the documented physical sequence for every source
//!   kind and slot (present, multislot, scalar slot, missing), on an
//!   ordered fact of up to four fields and a template fact.
//!
//! The central property of the module — the split search reports exactly
//! the splits a brute-force enumeration accepts, once each, in CLIPS order,
//! with sound early test pruning, handing the visitor exactly each split's
//! projection — is proved for all sizes on a Verus model of
//! `search_with_prefix`/`place`/`ready_tests_pass`/`segment_extra`
//! (verification/verus/core_sequence_split_search.rs), not on the
//! production code (see below). `validate` is modelled in
//! verification/verus/core_sequence_validate.rs (source checks, the
//! nested-test selector loop as an executable worklist, and their
//! composition). The harnesses above discharge, on production code, the
//! facts those models take as premises or abstract.
//!
//! Harness technique: values with recursive drop glue (`Value`,
//! `ConstantTest`, facts, plans) are leaked with `mem::forget` where their
//! drop is not part of the property; test variants are chosen per call
//! (never a symbolic variant tag), because a symbolic `ConstantTestType`
//! tag makes symbolic execution explore `Any`'s heap-allocated branches.
//!
//! Not covered by Kani, with the measured reason (Kani 0.68, 7 GB limit):
//!
//! * `search`, `search_with_prefix`, `admits`, `accepts`, `SplitView::get`:
//!   `place` pops `(FieldRef, OnceCell<Value>)` from a `SmallVec`; symbolic
//!   execution cannot resolve the popped `OnceCell<Value>`'s variant and
//!   explores the recursive drop glue of `Value`. A three-statement harness
//!   (two pushes and a pop) ran out of memory in 140 s at unwind 3 (13 s at
//!   unwind 2); a fully concrete search of `[$?, ?, $?]` over three values
//!   ran out of memory in 212 s; a capture-free search (no `pop`) ran out of
//!   memory in 466 s through the `capture` path of `SplitView::get`. The
//!   smallest search, one single field over a one-value ordered fact with
//!   no tests and no captures, all concrete and with a visitor that only
//!   counts matches, ran out of memory in 187 s (cause not isolated; the
//!   `truncate` in `place` and the drop of the `SplitView` both reach the
//!   drop glue of `OnceCell<Value>`).
//! * `project`: the early returns drop a partly built `SplitView`; out of
//!   memory in 302-397 s even with concrete field shapes.
//! * `validate`: any template source reaches the `FxHashSet` insertion
//!   (still in symbolic execution after 11-16 minutes, even for one
//!   insertion), and the test checks iterate a heap `Vec<ConstantTest>`
//!   (6.8 GB after 14 minutes; the 64-test limit harness timed out at 10
//!   minutes). Even a harness restricted to single ordered segments (up to
//!   three symbolic fields, at most one test of a concrete variant), where
//!   the insertion is unreachable, was still in symbolic execution at
//!   6.5 GB after 15 minutes: symbolic execution does not prune the
//!   infeasible `FxHashSet` branch and unwinds hashbrown's probe loop.
//! * Evaluation of slot-offset and key-comparison tests (`compare_offset`,
//!   `compare_test_key`) and of `Any` (`test_extent`, `physical_test` on
//!   `Any` ran over 10 minutes).

use super::*;
use crate::fact::{OrderedFact, TemplateFact, TemplateId};
use crate::symbol::{Symbol, SymbolId};
use crate::value::{AtomKey, Multifield};
use std::cell::Cell;

fn any_field() -> SequenceField {
    if kani::any() {
        SequenceField::Multi
    } else {
        SequenceField::Single
    }
}

fn relation() -> Symbol {
    Symbol(SymbolId::Ascii(0))
}

/// An ordered fact with `len` integer fields. The inline buffer is filled
/// with integers beyond `len` too, so every slot has the same variant.
fn ordered_fact(len: usize, values: [i64; 4]) -> Fact {
    let buf = [
        Value::Integer(values[0]),
        Value::Integer(values[1]),
        Value::Integer(values[2]),
        Value::Integer(values[3]),
        Value::Integer(0),
        Value::Integer(0),
        Value::Integer(0),
        Value::Integer(0),
    ];
    Fact::Ordered(OrderedFact {
        relation: relation(),
        fields: SmallVec::from_buf_and_len(buf, len),
    })
}

/// Reference tiling: the fields, in order, consume exactly `n` values when
/// each single field takes one value and the `i`-th capture takes
/// `lengths[i]`; `lengths` must name exactly one length per capture.
fn ref_tiles(fields: &[SequenceField], n: usize, lengths: &[usize]) -> bool {
    let mut total = 0_usize;
    let mut capture = 0_usize;
    for field in fields {
        match field {
            SequenceField::Single => total = total.saturating_add(1),
            SequenceField::Multi => {
                if capture == lengths.len() {
                    return false;
                }
                total = total.saturating_add(lengths[capture]);
                capture += 1;
            }
        }
    }
    capture == lengths.len() && total == n
}

fn ref_count(fields: &[SequenceField], field: SequenceField) -> usize {
    let mut count = 0;
    for f in fields {
        if *f == field {
            count += 1;
        }
    }
    count
}

/// `segment_extra` is `Some(e)` exactly when some choice of capture lengths
/// tiles the source, and then every tiling's captures share exactly `e`
/// values (`e = n - singles`).
///
/// Soundness quantifies over every vector of capture lengths (any `usize`);
/// completeness builds a tiling (all of `e` in the first capture) from the
/// result.
#[kani::proof]
#[kani::unwind(9)]
fn segment_extra_is_exact() {
    let k: usize = kani::any_where(|k| *k <= 6);
    let shapes: [SequenceField; 6] = std::array::from_fn(|_| any_field());
    let fields = &shapes[..k];
    let n: usize = kani::any_where(|n| *n <= 8);
    let buf: [Value; 8] = std::array::from_fn(|_| Value::Void);
    let got = segment_extra(fields, &buf[..n]);

    let singles = ref_count(fields, SequenceField::Single);
    let captures = ref_count(fields, SequenceField::Multi);
    // Every tiling is reported, with the captures sharing exactly `extra`.
    let lengths: [usize; 6] = kani::any();
    if ref_tiles(fields, n, &lengths[..captures]) {
        let mut shared = 0_usize;
        for length in &lengths[..captures] {
            shared += length;
        }
        assert_eq!(got, Some(shared));
        kani::cover!(
            captures >= 2 && shared >= 2,
            "a tiling with shared captures"
        );
    }
    // Every report has a tiling.
    if let Some(extra) = got {
        let mut witness = [0_usize; 6];
        if captures > 0 {
            witness[0] = extra;
        }
        assert!(ref_tiles(fields, n, &witness[..captures]));
        assert_eq!(extra + singles, n);
    }
    kani::cover!(got.is_none() && n < singles, "too few values");
    kani::cover!(got.is_none() && n > singles, "fixed fields, extra values");
    kani::cover!(got == Some(0) && captures == 0, "exact fixed fields");
    kani::cover!(got == Some(2) && captures == 3, "shared captures");
}

// ---------------------------------------------------------------------------
// Constant-test extents.
// ---------------------------------------------------------------------------

/// The index a selector names, whatever its fact kind.
fn slot_number(slot: SlotIndex) -> usize {
    match slot {
        SlotIndex::Ordered(index) | SlotIndex::Template(index) => index,
    }
}

fn any_selector() -> SlotIndex {
    let index: usize = kani::any();
    if kani::any() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    }
}

/// Reference extent: one past the largest selector the variant documents it
/// reads (the inspected slot, and the other slot of a slot pair), saturating.
fn ref_extent(slot: SlotIndex, other: Option<SlotIndex>) -> usize {
    slot_number(slot)
        .saturating_add(1)
        .max(other.map_or(0, |other| slot_number(other).saturating_add(1)))
}

/// `test_extent` of every variant except `Any` is the reference extent.
/// (`Any` recursion over heap-allocated branches is not tractable here.)
#[kani::proof]
#[kani::unwind(3)]
fn test_extent_is_exact() {
    let slot = any_selector();
    let other = any_selector();
    let key = AtomKey::Integer(kani::any());
    let offset: i64 = kani::any();
    let check = |test_type: ConstantTestType, other: Option<SlotIndex>| {
        let test = ConstantTest { slot, test_type };
        assert_eq!(test_extent(&test), ref_extent(slot, other));
    };
    match kani::any::<u8>() {
        0 => check(ConstantTestType::Equal(key), None),
        1 => check(ConstantTestType::NotEqual(key), None),
        2 => check(ConstantTestType::EqualAny(vec![key]), None),
        3 => check(ConstantTestType::GreaterThan(key), None),
        4 => check(ConstantTestType::LessThan(key), None),
        5 => check(ConstantTestType::GreaterOrEqual(key), None),
        6 => check(ConstantTestType::LessOrEqual(key), None),
        7 => check(ConstantTestType::EqualSlot(other), Some(other)),
        8 => check(ConstantTestType::NotEqualSlot(other), Some(other)),
        9 => check(
            ConstantTestType::EqualSlotOffset(other, offset),
            Some(other),
        ),
        10 => check(
            ConstantTestType::NotEqualSlotOffset(other, offset),
            Some(other),
        ),
        11 => check(
            ConstantTestType::GreaterThanSlotOffset(other, offset),
            Some(other),
        ),
        12 => check(
            ConstantTestType::LessThanSlotOffset(other, offset),
            Some(other),
        ),
        13 => check(
            ConstantTestType::GreaterOrEqualSlotOffset(other, offset),
            Some(other),
        ),
        14 => check(
            ConstantTestType::LessOrEqualSlotOffset(other, offset),
            Some(other),
        ),
        15 => check(
            ConstantTestType::OrderedFieldCount {
                min: kani::any(),
                max: kani::any(),
            },
            None,
        ),
        _ => check(
            ConstantTestType::Sequence(Box::new(SequencePattern {
                segments: vec![],
                tests: vec![],
            })),
            None,
        ),
    }
}

/// Evaluate `test` over three integer fields and check every read lies
/// below its extent (a saturated extent only bounds a read of the largest
/// selector).
fn check_reads_below_extent(test: &ConstantTest) {
    let need = test_extent(test);
    // Separate locals (not an array) keep each field's variant concrete for
    // symbolic execution.
    let (v0, v1, v2) = (
        Value::Integer(kani::any()),
        Value::Integer(kani::any()),
        Value::Integer(kani::any()),
    );
    let max_read = Cell::new(None::<usize>);
    let passed = evaluate_field_test(test, |slot| {
        let index = slot_number(slot);
        max_read.set(Some(max_read.get().map_or(index, |m| m.max(index))));
        match (slot, index) {
            (SlotIndex::Ordered(_), 0) => Some(&v0),
            (SlotIndex::Ordered(_), 1) => Some(&v1),
            (SlotIndex::Ordered(_), 2) => Some(&v2),
            _ => None,
        }
    });
    if let Some(read) = max_read.get() {
        assert!(read.saturating_add(1) <= need);
    }
    kani::cover!(passed, "the test passes");
    kani::cover!(!passed && need == 2, "the test fails on a present field");
    std::mem::forget((v0, v1, v2));
}

/// Evaluating an atom test (`Equal`, `NotEqual`, `EqualAny`) reads only
/// selectors below its extent, so a prefix holding `need` fields decides it.
#[kani::proof]
#[kani::unwind(3)]
fn atom_tests_read_below_extent() {
    let slot = any_selector();
    let key = AtomKey::Integer(kani::any());
    let test = |test_type| ConstantTest { slot, test_type };
    match kani::any::<u8>() {
        0 => check_reads_below_extent(&test(ConstantTestType::Equal(key))),
        1 => check_reads_below_extent(&test(ConstantTestType::NotEqual(key))),
        _ => check_reads_below_extent(&test(ConstantTestType::EqualAny(vec![key]))),
    }
}

// ---------------------------------------------------------------------------
// Symbolic pattern shapes (up to 3 segments of up to 3 fields).
// ---------------------------------------------------------------------------

const MAX_SEGMENTS: usize = 3;
const MAX_SEGMENT_FIELDS: usize = 3;

/// The harness-side description of a pattern, read by the references.
#[derive(Clone, Copy)]
struct Shape {
    count: usize,
    sources: [SequenceSource; MAX_SEGMENTS],
    lens: [usize; MAX_SEGMENTS],
    fields: [[SequenceField; MAX_SEGMENT_FIELDS]; MAX_SEGMENTS],
}

fn any_source(max_index: usize) -> SequenceSource {
    let index: usize = kani::any_where(|i| *i <= max_index);
    any_source_at(index)
}

fn any_source_at(index: usize) -> SequenceSource {
    if kani::any() {
        SequenceSource::Ordered
    } else if kani::any() {
        SequenceSource::TemplateSlot(index)
    } else {
        SequenceSource::TemplateScalar(index)
    }
}

/// A symbolic shape whose template sources name slots up to `max_index`.
fn any_shape(max_index: usize) -> Shape {
    let count: usize = kani::any_where(|c| *c <= MAX_SEGMENTS);
    let lens: [usize; MAX_SEGMENTS] =
        std::array::from_fn(|_| kani::any_where(|l| *l <= MAX_SEGMENT_FIELDS));
    Shape {
        count,
        sources: std::array::from_fn(|_| any_source(max_index)),
        lens,
        fields: std::array::from_fn(|_| [any_field(), any_field(), any_field()]),
    }
}

impl Shape {
    fn pattern(&self, tests: Vec<ConstantTest>) -> SequencePattern {
        let mut segments: Vec<SequenceSegment> = (0..MAX_SEGMENTS)
            .map(|s| {
                let mut fields = self.fields[s].to_vec();
                fields.truncate(self.lens[s]);
                SequenceSegment {
                    source: self.sources[s],
                    fields,
                }
            })
            .collect();
        segments.truncate(self.count);
        SequencePattern { segments, tests }
    }

    fn width(&self) -> usize {
        let mut width = 0;
        for s in 0..self.count {
            width += self.lens[s];
        }
        width
    }

    /// Exactly one segment, which projects an ordered fact.
    fn ordered(&self) -> bool {
        self.count == 1 && self.sources[0] == SequenceSource::Ordered
    }

    /// The selector kind matches the projection and the index is in range.
    fn valid_slot(&self, slot: SlotIndex) -> bool {
        match slot {
            SlotIndex::Ordered(index) => self.ordered() && index < self.width(),
            SlotIndex::Template(index) => !self.ordered() && index < self.width(),
        }
    }

    /// The segment holding logical template field `index`, and the
    /// position of that field within the segment.
    fn locate(&self, index: usize) -> Option<(usize, usize)> {
        let mut start = 0;
        for s in 0..self.count {
            if start <= index && index < start + self.lens[s] {
                return Some((s, index - start));
            }
            start += self.lens[s];
        }
        None
    }

    /// Reference for `physical_selector`: an ordered field preceded only by
    /// single fields keeps its physical position; a field projected by a
    /// scalar template source is that physical slot.
    fn physical(&self, slot: SlotIndex) -> Option<SlotIndex> {
        match slot {
            SlotIndex::Ordered(index) => {
                if !self.ordered() || index >= self.lens[0] {
                    return None;
                }
                for j in 0..=index {
                    if self.fields[0][j] != SequenceField::Single {
                        return None;
                    }
                }
                Some(slot)
            }
            SlotIndex::Template(index) => match self.locate(index) {
                Some((s, _)) => match self.sources[s] {
                    SequenceSource::TemplateScalar(physical) => Some(SlotIndex::Template(physical)),
                    _ => None,
                },
                None => None,
            },
        }
    }
}

fn any_small_selector(max: usize) -> SlotIndex {
    let index: usize = kani::any_where(|i| *i <= max);
    if kani::any() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    }
}

/// `logical_width`, `is_ordered`, `valid_logical_slot` and
/// `logical_slot_validator` agree with their references on every shape.
#[kani::proof]
#[kani::unwind(4)]
fn projection_helpers_are_exact() {
    let shape = any_shape(2);
    let pattern = shape.pattern(vec![]);
    assert_eq!(pattern.logical_width(), shape.width());
    assert_eq!(pattern.is_ordered(), shape.ordered());
    let slot = any_small_selector(MAX_SEGMENTS * MAX_SEGMENT_FIELDS + 1);
    assert_eq!(pattern.valid_logical_slot(slot), shape.valid_slot(slot));
    assert_eq!(
        pattern.logical_slot_validator()(slot),
        shape.valid_slot(slot)
    );
    kani::cover!(
        shape.ordered() && shape.valid_slot(slot),
        "ordered selector"
    );
    kani::cover!(
        shape.count == 3 && shape.valid_slot(slot),
        "template selector"
    );
    kani::cover!(shape.width() == 9, "widest projection");
}

/// `physical_selector` agrees with its reference, and is sound: for every
/// split of an ordered fact (any capture lengths that tile it), a logical
/// field it maps is a single field at that same physical position.
#[kani::proof]
#[kani::unwind(4)]
fn physical_selector_is_exact_and_sound() {
    let shape = any_shape(2);
    let pattern = shape.pattern(vec![]);
    let slot = any_small_selector(MAX_SEGMENTS * MAX_SEGMENT_FIELDS + 1);
    let got = pattern.physical_selector(slot);
    assert_eq!(got, shape.physical(slot));
    if let (true, Some(SlotIndex::Ordered(physical))) = (shape.ordered(), got) {
        let lengths: [usize; MAX_SEGMENT_FIELDS] = kani::any();
        let fields = &shape.fields[0][..shape.lens[0]];
        let captures = ref_count(fields, SequenceField::Multi);
        let n: usize = kani::any_where(|n| *n <= 8);
        if ref_tiles(fields, n, &lengths[..captures]) {
            let mut offset = 0_usize;
            let mut capture = 0;
            for field in &fields[..slot_number(slot)] {
                match field {
                    SequenceField::Single => offset += 1,
                    SequenceField::Multi => {
                        offset += lengths[capture];
                        capture += 1;
                    }
                }
            }
            assert_eq!(fields[slot_number(slot)], SequenceField::Single);
            assert_eq!(offset, physical);
            assert!(physical < n);
            kani::cover!(
                physical == 1 && captures == 1,
                "fixed field before a capture"
            );
        }
    }
    kani::cover!(
        matches!(got, Some(SlotIndex::Template(_))) && shape.count == 3,
        "scalar template slot"
    );
    kani::cover!(
        got.is_none() && shape.valid_slot(slot),
        "a valid selector without a fixed position"
    );
}

/// `physical_test` maps exactly the tests whose every read selector has a
/// fixed physical position, rewriting those selectors and nothing else; it
/// never maps whole-fact constraints.
#[kani::proof]
#[kani::unwind(4)]
fn physical_test_is_exact() {
    let shape = any_shape(2);
    let pattern = shape.pattern(vec![]);
    let slot = any_small_selector(MAX_SEGMENTS * MAX_SEGMENT_FIELDS);
    let other = any_small_selector(MAX_SEGMENTS * MAX_SEGMENT_FIELDS);
    let key = AtomKey::Integer(kani::any());
    let offset: i64 = kani::any();
    let mapped_slot = shape.physical(slot);
    let mapped_other = shape.physical(other);
    let test = |test_type| ConstantTest { slot, test_type };
    // The result is decomposed by pattern matching (not `PartialEq`, whose
    // recursion through `Any` symbolic execution cannot bound) and leaked
    // (its drop glue is not part of the property).
    match kani::any::<u8>() {
        0 => {
            let got = pattern.physical_test(&test(ConstantTestType::Equal(key.clone())));
            match (&got, mapped_slot) {
                (
                    Some(ConstantTest {
                        slot: s,
                        test_type: ConstantTestType::Equal(k),
                    }),
                    Some(m),
                ) => {
                    assert_eq!(*s, m);
                    assert!(*k == key);
                }
                (None, None) => {}
                _ => panic!("physical_test differs from the reference"),
            }
            kani::cover!(got.is_some(), "mapped constant test");
            std::mem::forget(got);
        }
        1 => {
            let got = pattern.physical_test(&test(ConstantTestType::EqualSlot(other)));
            match (&got, mapped_slot, mapped_other) {
                (
                    Some(ConstantTest {
                        slot: s,
                        test_type: ConstantTestType::EqualSlot(o),
                    }),
                    Some(ms),
                    Some(mo),
                ) => {
                    assert_eq!(*s, ms);
                    assert_eq!(*o, mo);
                }
                (None, None, _) | (None, _, None) => {}
                _ => panic!("physical_test differs from the reference"),
            }
            kani::cover!(got.is_some(), "mapped slot pair");
            kani::cover!(
                got.is_none() && mapped_slot.is_some(),
                "unmapped other slot"
            );
            std::mem::forget(got);
        }
        2 => {
            let got =
                pattern.physical_test(&test(ConstantTestType::LessThanSlotOffset(other, offset)));
            match (&got, mapped_slot, mapped_other) {
                (
                    Some(ConstantTest {
                        slot: s,
                        test_type: ConstantTestType::LessThanSlotOffset(o, k),
                    }),
                    Some(ms),
                    Some(mo),
                ) => {
                    assert_eq!(*s, ms);
                    assert_eq!(*o, mo);
                    assert_eq!(*k, offset);
                }
                (None, None, _) | (None, _, None) => {}
                _ => panic!("physical_test differs from the reference"),
            }
            kani::cover!(got.is_some(), "mapped offset pair");
            std::mem::forget(got);
        }
        3 => {
            let got = pattern.physical_test(&test(ConstantTestType::OrderedFieldCount {
                min: kani::any(),
                max: kani::any(),
            }));
            assert!(got.is_none());
            kani::cover!(mapped_slot.is_some(), "field count on a fixed field");
            std::mem::forget(got);
        }
        _ => {
            let got = pattern.physical_test(&test(ConstantTestType::Sequence(Box::new(
                SequencePattern {
                    segments: vec![],
                    tests: vec![],
                },
            ))));
            assert!(got.is_none());
            kani::cover!(mapped_slot.is_some(), "nested plan on a fixed field");
            std::mem::forget(got);
        }
    }
}

// ---------------------------------------------------------------------------
// Physical sources and recorded splits.
// ---------------------------------------------------------------------------

fn template_fact(slots: Vec<Value>) -> Fact {
    Fact::Template(TemplateFact {
        template_id: TemplateId::default(),
        slots: slots.into_boxed_slice(),
    })
}

fn multifield(values: [i64; 2]) -> Value {
    let mut multifield = Multifield::new();
    multifield.push(Value::Integer(values[0]));
    multifield.push(Value::Integer(values[1]));
    Value::Multifield(Box::new(multifield))
}

/// `segment_values` gives exactly the documented physical sequence: all
/// fields of an ordered fact for an ordered source; the stored multifield
/// of a template multislot (a non-multifield slot value as a one-value
/// sequence); the one stored value of a scalar template slot; and nothing
/// for a missing slot or a source of the other fact kind. Checked by
/// identity (same address and length) on an ordered fact of up to four
/// fields and a template fact with a scalar slot and a multifield slot.
#[kani::proof]
#[kani::unwind(4)]
fn segment_values_is_exact() {
    let n: usize = kani::any_where(|n| *n <= 4);
    let ordered = ordered_fact(n, kani::any());
    let template = template_fact(vec![Value::Integer(kani::any()), multifield(kani::any())]);
    let index: usize = kani::any_where(|i| *i <= 2);
    let source = any_source_at(index);
    let (Fact::Ordered(o), Fact::Template(t)) = (&ordered, &template) else {
        unreachable!()
    };
    let Value::Multifield(stored) = &t.slots[1] else {
        unreachable!()
    };
    let same = |got: Option<&[Value]>, expected: Option<&[Value]>| match (got, expected) {
        (Some(got), Some(expected)) => std::ptr::eq(got, expected),
        (None, None) => true,
        _ => false,
    };
    // Ordered fact.
    let expected = match source {
        SequenceSource::Ordered => Some(o.fields.as_slice()),
        _ => None,
    };
    assert!(same(segment_values(source, &ordered), expected));
    // Template fact.
    let expected = match (source, index) {
        (SequenceSource::TemplateSlot(_), 0) | (SequenceSource::TemplateScalar(_), 0) => {
            Some(std::slice::from_ref(&t.slots[0]))
        }
        (SequenceSource::TemplateSlot(_), 1) => Some(stored.as_slice()),
        (SequenceSource::TemplateScalar(_), 1) => Some(std::slice::from_ref(&t.slots[1])),
        _ => None,
    };
    assert!(same(segment_values(source, &template), expected));
    kani::cover!(
        matches!(source, SequenceSource::TemplateSlot(1)),
        "multislot contents"
    );
    kani::cover!(
        index == 2 && source != SequenceSource::Ordered,
        "missing slot"
    );
    std::mem::forget((ordered, template));
}

// ---------------------------------------------------------------------------
// `physical_test` on every remaining non-`Any` variant (review addition).
// ---------------------------------------------------------------------------

/// The variant of a slot-pair test, its other selector and its offset (0
/// for the plain slot comparisons); `None` for every other variant. Written
/// from the variant list of `alpha::ConstantTestType`.
fn pair_parts(test_type: &ConstantTestType) -> Option<(u8, SlotIndex, i64)> {
    match test_type {
        ConstantTestType::EqualSlot(other) => Some((0, *other, 0)),
        ConstantTestType::NotEqualSlot(other) => Some((1, *other, 0)),
        ConstantTestType::EqualSlotOffset(other, k) => Some((2, *other, *k)),
        ConstantTestType::NotEqualSlotOffset(other, k) => Some((3, *other, *k)),
        ConstantTestType::GreaterThanSlotOffset(other, k) => Some((4, *other, *k)),
        ConstantTestType::LessThanSlotOffset(other, k) => Some((5, *other, *k)),
        ConstantTestType::GreaterOrEqualSlotOffset(other, k) => Some((6, *other, *k)),
        ConstantTestType::LessOrEqualSlotOffset(other, k) => Some((7, *other, *k)),
        _ => None,
    }
}

/// `physical_test` of a slot-pair test is defined exactly when both of its
/// selectors have a fixed physical position, and then rewrites both and
/// keeps the variant and offset.
fn check_physical_pair(shape: &Shape, slot: SlotIndex, test_type: ConstantTestType) {
    let (tag, other, offset) = pair_parts(&test_type).unwrap();
    let pattern = shape.pattern(vec![]);
    let got = pattern.physical_test(&ConstantTest { slot, test_type });
    match (&got, shape.physical(slot), shape.physical(other)) {
        (Some(mapped), Some(mapped_slot), Some(mapped_other)) => {
            assert_eq!(mapped.slot, mapped_slot);
            assert_eq!(
                pair_parts(&mapped.test_type),
                Some((tag, mapped_other, offset))
            );
        }
        (None, None, _) | (None, _, None) => {}
        _ => panic!("physical_test differs from the reference"),
    }
    kani::cover!(got.is_some(), "mapped slot pair");
    kani::cover!(
        got.is_none() && shape.physical(slot).is_some(),
        "fixed slot, unfixed other slot"
    );
    std::mem::forget((got, pattern));
}

/// Every slot-pair variant (`EqualSlot`, `NotEqualSlot` and the six offset
/// comparisons) is rewritten exactly as the reference says: both selectors
/// must have a fixed position, both are rewritten, nothing else changes.
#[kani::proof]
#[kani::unwind(4)]
fn physical_test_maps_every_slot_pair() {
    let shape = any_shape(2);
    let slot = any_small_selector(MAX_SEGMENTS * MAX_SEGMENT_FIELDS);
    let other = any_small_selector(MAX_SEGMENTS * MAX_SEGMENT_FIELDS);
    let offset: i64 = kani::any();
    match kani::any::<u8>() {
        0 => check_physical_pair(&shape, slot, ConstantTestType::EqualSlot(other)),
        1 => check_physical_pair(&shape, slot, ConstantTestType::NotEqualSlot(other)),
        2 => check_physical_pair(
            &shape,
            slot,
            ConstantTestType::EqualSlotOffset(other, offset),
        ),
        3 => check_physical_pair(
            &shape,
            slot,
            ConstantTestType::NotEqualSlotOffset(other, offset),
        ),
        4 => check_physical_pair(
            &shape,
            slot,
            ConstantTestType::GreaterThanSlotOffset(other, offset),
        ),
        5 => check_physical_pair(
            &shape,
            slot,
            ConstantTestType::LessThanSlotOffset(other, offset),
        ),
        6 => check_physical_pair(
            &shape,
            slot,
            ConstantTestType::GreaterOrEqualSlotOffset(other, offset),
        ),
        _ => check_physical_pair(
            &shape,
            slot,
            ConstantTestType::LessOrEqualSlotOffset(other, offset),
        ),
    }
}

/// The variant and integer key of a test against constant keys; `None`
/// for every other variant or key kind. (Only integer keys are compared:
/// `AtomKey` equality over every key kind exhausts the solver's memory.)
fn key_parts(test_type: &ConstantTestType) -> Option<(u8, i64)> {
    let (tag, key) = match test_type {
        ConstantTestType::Equal(key) => (0, key),
        ConstantTestType::NotEqual(key) => (1, key),
        ConstantTestType::EqualAny(keys) if keys.len() == 1 => (2, &keys[0]),
        ConstantTestType::GreaterThan(key) => (3, key),
        ConstantTestType::LessThan(key) => (4, key),
        ConstantTestType::GreaterOrEqual(key) => (5, key),
        ConstantTestType::LessOrEqual(key) => (6, key),
        _ => return None,
    };
    match key {
        AtomKey::Integer(value) => Some((tag, *value)),
        _ => None,
    }
}

/// `physical_test` of a test against constant keys is defined exactly when
/// its selector has a fixed physical position, and then rewrites only it.
fn check_physical_key(shape: &Shape, slot: SlotIndex, test_type: ConstantTestType) {
    let parts = key_parts(&test_type);
    assert!(parts.is_some());
    let pattern = shape.pattern(vec![]);
    let got = pattern.physical_test(&ConstantTest { slot, test_type });
    match (&got, shape.physical(slot)) {
        (Some(mapped), Some(mapped_slot)) => {
            assert_eq!(mapped.slot, mapped_slot);
            assert_eq!(key_parts(&mapped.test_type), parts);
        }
        (None, None) => {}
        _ => panic!("physical_test differs from the reference"),
    }
    kani::cover!(got.is_some(), "mapped key test");
    kani::cover!(
        got.is_none() && shape.valid_slot(slot),
        "a valid selector without a fixed position"
    );
    std::mem::forget((got, pattern));
}

/// Every test against constant keys (`NotEqual`, `EqualAny` and the four
/// ordered comparisons; `Equal` is in `physical_test_is_exact`) is rewritten
/// exactly when its selector has a fixed position, keeping its keys.
#[kani::proof]
#[kani::unwind(4)]
fn physical_test_maps_every_key_test() {
    let shape = any_shape(2);
    let slot = any_small_selector(MAX_SEGMENTS * MAX_SEGMENT_FIELDS);
    let key = AtomKey::Integer(kani::any());
    match kani::any::<u8>() {
        0 => check_physical_key(&shape, slot, ConstantTestType::NotEqual(key)),
        1 => check_physical_key(&shape, slot, ConstantTestType::EqualAny(vec![key])),
        2 => check_physical_key(&shape, slot, ConstantTestType::GreaterThan(key)),
        3 => check_physical_key(&shape, slot, ConstantTestType::LessThan(key)),
        4 => check_physical_key(&shape, slot, ConstantTestType::GreaterOrEqual(key)),
        _ => check_physical_key(&shape, slot, ConstantTestType::LessOrEqual(key)),
    }
}

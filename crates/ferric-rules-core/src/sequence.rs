//! Sequence matching with independent zero-or-more captures in ordered facts and template slots.

use std::cell::OnceCell;
use std::ops::ControlFlow;

use smallvec::SmallVec;

use crate::alpha::{
    evaluate_field_test, AlphaEntryType, ConstantTest, ConstantTestType, SlotIndex,
};
use crate::fact::Fact;
#[cfg(test)]
use crate::fact::{OrderedFact, TemplateFact};
use crate::value::Value;

/// Number of physical fields consumed by one logical pattern field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SequenceField {
    Single,
    Multi,
}

/// Physical sequence supplied to a segment, before positional matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SequenceSource {
    /// All fields of an ordered fact.
    Ordered,
    /// The stored multifield of a template multislot.
    TemplateSlot(usize),
    /// A single-field template slot. It projects exactly one logical field
    /// holding the physical slot value, so its selector stays indexable.
    TemplateScalar(usize),
}

/// One independently matched sequence, kept in written constraint order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SequenceSegment {
    pub source: SequenceSource,
    pub fields: Vec<SequenceField>,
}

/// A sequence projection and the constant constraints on its flattened logical fields.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SequencePattern {
    pub segments: Vec<SequenceSegment>,
    pub tests: Vec<ConstantTest>,
}

/// One positional match. Lengths identify captures even when none are named.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct SequenceMatch {
    pub fact: Fact,
    pub lengths: SmallVec<[usize; 2]>,
}

impl SequencePattern {
    /// Number of scalar values or multifield captures in the projected fact.
    #[must_use]
    pub fn logical_width(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.fields.len())
            .sum()
    }

    /// Whether this plan projects the fields of an ordered fact.
    #[must_use]
    pub fn is_ordered(&self) -> bool {
        matches!(
            self.segments.as_slice(),
            [SequenceSegment {
                source: SequenceSource::Ordered,
                ..
            }]
        )
    }

    /// Cache the projection width once when checking a collection of selectors.
    pub fn logical_slot_validator(&self) -> impl Fn(SlotIndex) -> bool {
        let ordered = self.is_ordered();
        let width = self.logical_width();
        move |slot| match slot {
            SlotIndex::Ordered(index) => ordered && index < width,
            SlotIndex::Template(index) => !ordered && index < width,
        }
    }

    /// Check one selector against the kind and width of the logical projection.
    /// Use `logical_slot_validator` to amortize width lookup across many selectors.
    #[must_use]
    pub fn valid_logical_slot(&self, slot: SlotIndex) -> bool {
        self.logical_slot_validator()(slot)
    }

    /// The physical fact selector for a logical field whose position never
    /// depends on a split: an ordered single field before the first capture,
    /// or a scalar template slot. Such selectors can key alpha/beta indexes.
    #[must_use]
    pub fn physical_selector(&self, slot: SlotIndex) -> Option<SlotIndex> {
        match (self.segments.as_slice(), slot) {
            (
                [SequenceSegment {
                    source: SequenceSource::Ordered,
                    fields,
                }],
                SlotIndex::Ordered(index),
            ) => (index < fields.len()
                && fields[..=index]
                    .iter()
                    .all(|field| *field == SequenceField::Single))
            .then_some(slot),
            (segments, SlotIndex::Template(mut index)) => {
                for segment in segments {
                    if index < segment.fields.len() {
                        return match segment.source {
                            SequenceSource::TemplateScalar(physical) => {
                                Some(SlotIndex::Template(physical))
                            }
                            _ => None,
                        };
                    }
                    index -= segment.fields.len();
                }
                None
            }
            _ => None,
        }
    }

    /// Rewrite a constant test to physical selectors when every field it reads
    /// has a fixed physical position, so it can filter facts in the alpha
    /// network before any split is enumerated.
    #[must_use]
    pub fn physical_test(&self, test: &ConstantTest) -> Option<ConstantTest> {
        let mut mapped = test.clone();
        mapped.slot = self.physical_selector(test.slot)?;
        match &mut mapped.test_type {
            ConstantTestType::EqualSlot(other)
            | ConstantTestType::NotEqualSlot(other)
            | ConstantTestType::EqualSlotOffset(other, _)
            | ConstantTestType::NotEqualSlotOffset(other, _)
            | ConstantTestType::GreaterThanSlotOffset(other, _)
            | ConstantTestType::LessThanSlotOffset(other, _)
            | ConstantTestType::GreaterOrEqualSlotOffset(other, _)
            | ConstantTestType::LessOrEqualSlotOffset(other, _) => {
                *other = self.physical_selector(*other)?;
            }
            ConstantTestType::Any(branches) => {
                for branch in branches {
                    for test in branch {
                        *test = self.physical_test(test)?;
                    }
                }
            }
            ConstantTestType::OrderedFieldCount { .. } | ConstantTestType::Sequence(_) => {
                return None
            }
            _ => {}
        }
        Some(mapped)
    }

    /// Check the sources and logical selectors before installing or restoring a pattern.
    pub fn validate(&self) -> Result<(), String> {
        if self.segments.is_empty() {
            return Err("sequence pattern has no source segments".to_string());
        }
        if !self.is_ordered() {
            let mut sources = rustc_hash::FxHashSet::default();
            for segment in &self.segments {
                let index = match segment.source {
                    SequenceSource::TemplateSlot(index) => index,
                    SequenceSource::TemplateScalar(index)
                        if segment.fields.as_slice() == [SequenceField::Single] =>
                    {
                        index
                    }
                    SequenceSource::TemplateScalar(_) => {
                        return Err(
                            "scalar template sequence source must project one single field"
                                .to_string(),
                        );
                    }
                    SequenceSource::Ordered => {
                        return Err("ordered sequence source must be the only segment".to_string());
                    }
                };
                if !sources.insert(index) {
                    return Err("sequence pattern repeats a template slot source".to_string());
                }
            }
        }
        if crate::alpha::constant_test_count(&self.tests) > crate::compiler::MAX_ALPHA_TESTS {
            return Err("sequence pattern exceeds 64 tests".to_string());
        }
        let valid_slot = self.logical_slot_validator();
        let mut pending: Vec<_> = self.tests.iter().collect();
        while let Some(test) = pending.pop() {
            if !valid_slot(test.slot) {
                return Err("sequence test has an invalid logical field".to_string());
            }
            match &test.test_type {
                ConstantTestType::Any(branches) => pending.extend(branches.iter().flatten()),
                ConstantTestType::OrderedFieldCount { .. } | ConstantTestType::Sequence(_) => {
                    return Err("sequence test contains a whole-fact constraint".to_string());
                }
                ConstantTestType::EqualSlot(slot)
                | ConstantTestType::NotEqualSlot(slot)
                | ConstantTestType::EqualSlotOffset(slot, _)
                | ConstantTestType::NotEqualSlotOffset(slot, _)
                | ConstantTestType::GreaterThanSlotOffset(slot, _)
                | ConstantTestType::LessThanSlotOffset(slot, _)
                | ConstantTestType::GreaterOrEqualSlotOffset(slot, _)
                | ConstantTestType::LessOrEqualSlotOffset(slot, _)
                    if !valid_slot(*slot) =>
                {
                    return Err("sequence test references an invalid logical field".to_string());
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Require an alpha source of the same fact kind as the projection.
    pub fn validate_entry(&self, entry: &AlphaEntryType) -> Result<(), String> {
        self.validate()?;
        if self.is_ordered() != matches!(entry, AlphaEntryType::OrderedRelation(_)) {
            return Err("sequence plan and alpha source have different fact kinds".to_string());
        }
        Ok(())
    }

    /// Search the splits of `fact` depth first and report each one that passes
    /// every test of the plan.
    ///
    /// Captures are placed in written order, the earlier segments outermost,
    /// and each capture tries its longest length first, so splits arrive in
    /// the order CLIPS inserts them. A test runs as soon as every field it
    /// reads is placed, so a failing constant prunes every split sharing that
    /// prefix. `visit` sees [`SplitEvent::Step`] before each length a capture
    /// tries, which lets callers bound the work (a segment's last capture takes
    /// the remaining fields without a step), and [`SplitEvent::Match`] for each
    /// accepted split. Returning `Break` stops the search.
    pub(crate) fn search<'a, B>(
        &'a self,
        fact: &'a Fact,
        visit: &mut impl FnMut(SplitEvent<'_, 'a>) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        self.search_with_prefix(fact, &mut |_, _, _| true, visit)
    }

    /// Search with an additional predicate over each newly placed prefix.
    ///
    /// The filter runs after the plan's ready constant tests, immediately before
    /// another capture choice or a complete match. `checked` is the number of
    /// leading fields checked in the parent prefix; fields through
    /// `split.placed_fields()` are now available. A complete split must also
    /// check selectors beyond its width, preserving invalid-selector rejection.
    pub(crate) fn search_with_prefix<'a, B>(
        &'a self,
        fact: &'a Fact,
        prefix: &mut impl FnMut(&SplitView<'a>, usize, bool) -> bool,
        visit: &mut impl FnMut(SplitEvent<'_, 'a>) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        let mut sources = SmallVec::<[(&'a [Value], usize); 2]>::new();
        for segment in &self.segments {
            let Some(values) = segment_values(segment.source, fact) else {
                return ControlFlow::Continue(());
            };
            let Some(extra) = segment_extra(&segment.fields, values) else {
                return ControlFlow::Continue(());
            };
            sources.push((values, extra));
        }
        let Some(&(_, extra)) = sources.first() else {
            return ControlFlow::Continue(());
        };
        let width = self.logical_width();
        let mut search = SplitSearch {
            plan: self,
            prefix,
            needs: self.tests.iter().map(test_extent).collect(),
            sources,
            split: SplitView {
                fact,
                fields: SmallVec::with_capacity(width),
                lengths: SmallVec::new(),
            },
        };
        search.place(0, 0, 0, extra, 0, visit)
    }

    /// Rebuild the view of one recorded split in `O(width)`.
    /// Returns `None` when `lengths` is not a valid split of `fact`.
    #[must_use]
    pub(crate) fn project<'a>(
        &'a self,
        fact: &'a Fact,
        lengths: &[usize],
    ) -> Option<SplitView<'a>> {
        let mut split = SplitView {
            fact,
            fields: SmallVec::with_capacity(self.logical_width()),
            lengths: SmallVec::from_slice(lengths),
        };
        let mut lengths = lengths.iter();
        for segment in &self.segments {
            let values = segment_values(segment.source, fact)?;
            let mut offset = 0_usize;
            for field in &segment.fields {
                let length = match field {
                    SequenceField::Single => 1,
                    SequenceField::Multi => *lengths.next()?,
                };
                let end = offset.checked_add(length)?;
                let values = values.get(offset..end)?;
                split.fields.push((
                    match field {
                        SequenceField::Single => FieldRef::Single(&values[0]),
                        SequenceField::Multi => FieldRef::Multi(values),
                    },
                    OnceCell::new(),
                ));
                offset = end;
            }
            if offset != values.len() {
                return None;
            }
        }
        (lengths.next().is_none() && !self.segments.is_empty()).then_some(split)
    }

    /// Whether some split of `fact` passes every test of the plan.
    #[must_use]
    pub fn admits(&self, fact: &Fact) -> bool {
        self.search(fact, &mut |event| match event {
            SplitEvent::Match(_) => ControlFlow::Break(()),
            SplitEvent::Step => ControlFlow::Continue(()),
        })
        .is_break()
    }

    /// Evaluate constant constraints against a complete split.
    #[must_use]
    pub(crate) fn accepts(&self, split: &SplitView<'_>) -> bool {
        self.tests
            .iter()
            .all(|test| evaluate_field_test(test, |slot| split.get(slot)))
    }

    /// Every matching split, including empty and anonymous captures.
    #[cfg(test)]
    pub(crate) fn matches(&self, fact: &Fact) -> impl Iterator<Item = SequenceMatch> {
        let mut matches = Vec::new();
        let _ = self.search(fact, &mut |event| {
            if let SplitEvent::Match(split) = event {
                matches.push(split.to_match());
            }
            ControlFlow::<()>::Continue(())
        });
        matches.into_iter()
    }
}

/// What a split search reports to its visitor.
pub(crate) enum SplitEvent<'s, 'a> {
    /// A capture with a choice of lengths is about to try one.
    Step,
    /// A complete split that passes every test of the plan.
    Match(&'s SplitView<'a>),
}

/// The number of leading logical fields a test reads.
fn test_extent(test: &ConstantTest) -> usize {
    let index = |slot: SlotIndex| match slot {
        SlotIndex::Ordered(index) | SlotIndex::Template(index) => index,
    };
    let other = match &test.test_type {
        ConstantTestType::EqualSlot(slot)
        | ConstantTestType::NotEqualSlot(slot)
        | ConstantTestType::EqualSlotOffset(slot, _)
        | ConstantTestType::NotEqualSlotOffset(slot, _)
        | ConstantTestType::GreaterThanSlotOffset(slot, _)
        | ConstantTestType::LessThanSlotOffset(slot, _)
        | ConstantTestType::GreaterOrEqualSlotOffset(slot, _)
        | ConstantTestType::LessOrEqualSlotOffset(slot, _) => index(*slot).saturating_add(1),
        ConstantTestType::Any(branches) => branches
            .iter()
            .flatten()
            .map(test_extent)
            .max()
            .unwrap_or(0),
        _ => 0,
    };
    index(test.slot).saturating_add(1).max(other)
}

/// The values a segment's captures share, or `None` when its single fields
/// cannot fit (or, without captures, do not fill) the source.
fn segment_extra(fields: &[SequenceField], values: &[Value]) -> Option<usize> {
    let singles = fields
        .iter()
        .filter(|field| **field == SequenceField::Single)
        .count();
    let extra = values.len().checked_sub(singles)?;
    (extra == 0 || singles < fields.len()).then_some(extra)
}

struct SplitSearch<'p, 'a, F> {
    plan: &'p SequencePattern,
    prefix: F,
    /// Each segment's values and the length its captures share.
    sources: SmallVec<[(&'a [Value], usize); 2]>,
    /// Per test, how many leading logical fields must be placed to run it.
    needs: SmallVec<[usize; 8]>,
    split: SplitView<'a>,
}

impl<'a, F> SplitSearch<'_, 'a, F>
where
    F: FnMut(&SplitView<'a>, usize, bool) -> bool,
{
    /// Run the tests that became ready since `checked` fields were placed.
    /// A complete split runs every remaining test, so a selector past the
    /// projection still runs, and fails, there.
    fn ready_tests_pass(&mut self, checked: usize, complete: bool) -> bool {
        let placed = self.split.fields.len();
        let constants_pass = self
            .plan
            .tests
            .iter()
            .zip(&self.needs)
            .all(|(test, &need)| {
                need <= checked
                    || (need > placed && !complete)
                    || evaluate_field_test(test, |slot| self.split.get(slot))
            });
        constants_pass && (self.prefix)(&self.split, checked, complete)
    }

    /// Place the fields from `field` of `segment` on, at physical `offset`,
    /// with `extra` values left for that segment's remaining captures.
    /// Tests reading only the first `checked` fields have already passed.
    fn place<B>(
        &mut self,
        mut segment: usize,
        mut field: usize,
        mut offset: usize,
        mut extra: usize,
        checked: usize,
        visit: &mut impl FnMut(SplitEvent<'_, 'a>) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        let mark = self.split.fields.len();
        let plan = self.plan;
        loop {
            let fields = &plan.segments[segment].fields;
            let values = self.sources[segment].0;
            if field == fields.len() {
                segment += 1;
                if segment < plan.segments.len() {
                    (field, offset, extra) = (0, 0, self.sources[segment].1);
                    continue;
                }
                if self.ready_tests_pass(checked, true) {
                    visit(SplitEvent::Match(&self.split))?;
                }
                break;
            }
            if fields[field] == SequenceField::Single {
                self.split
                    .fields
                    .push((FieldRef::Single(&values[offset]), OnceCell::new()));
                (field, offset) = (field + 1, offset + 1);
                continue;
            }
            if !self.ready_tests_pass(checked, false) {
                break;
            }
            let checked = self.split.fields.len();
            // The segment's last capture takes whatever the others leave.
            let last = fields[field + 1..]
                .iter()
                .all(|field| *field == SequenceField::Single);
            let shortest = if last { extra } else { 0 };
            for length in (shortest..=extra).rev() {
                if !last {
                    visit(SplitEvent::Step)?;
                }
                self.split.fields.push((
                    FieldRef::Multi(&values[offset..offset + length]),
                    OnceCell::new(),
                ));
                self.split.lengths.push(length);
                self.place(
                    segment,
                    field + 1,
                    offset + length,
                    extra - length,
                    checked,
                    visit,
                )?;
                self.split.fields.pop();
                self.split.lengths.pop();
            }
            break;
        }
        self.split.fields.truncate(mark);
        ControlFlow::Continue(())
    }
}

/// One logical field of a split, borrowed from the physical fact.
#[derive(Clone, Copy, Debug)]
enum FieldRef<'a> {
    Single(&'a Value),
    Multi(&'a [Value]),
}

/// A split that borrows the physical fact. Tests and bindings read logical
/// fields through [`SplitView::get`]; a capture is copied into a multifield
/// value only when something reads it.
#[derive(Debug)]
pub(crate) struct SplitView<'a> {
    fact: &'a Fact,
    fields: SmallVec<[(FieldRef<'a>, OnceCell<Value>); 8]>,
    /// Capture lengths, which identify this split among those of the fact.
    pub lengths: SmallVec<[usize; 2]>,
}

impl SplitView<'_> {
    /// Number of leading logical fields available to a prefix predicate.
    pub(crate) fn placed_fields(&self) -> usize {
        self.fields.len()
    }

    /// The value of a logical field, as the projected fact would hold it.
    #[must_use]
    pub fn get(&self, slot: SlotIndex) -> Option<&Value> {
        let ((SlotIndex::Ordered(index), Fact::Ordered(_))
        | (SlotIndex::Template(index), Fact::Template(_))) = (slot, self.fact)
        else {
            return None;
        };
        let (field, copy) = self.fields.get(index)?;
        Some(match *field {
            FieldRef::Single(value) => value,
            FieldRef::Multi(values) => copy.get_or_init(|| capture(values)),
        })
    }

    /// Whether `get` has copied any capture of this split.
    #[must_use]
    pub fn copied_capture(&self) -> bool {
        self.fields.iter().any(|(_, copy)| copy.get().is_some())
    }

    /// Materialize the projected fact.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn to_match(&self) -> SequenceMatch {
        let fields = self.fields.iter().map(|(field, copy)| match field {
            FieldRef::Single(value) => (*value).clone(),
            FieldRef::Multi(values) => copy.get().cloned().unwrap_or_else(|| capture(values)),
        });
        let fact = match self.fact {
            Fact::Ordered(original) => Fact::Ordered(OrderedFact {
                relation: original.relation,
                fields: fields.collect(),
            }),
            Fact::Template(original) => Fact::Template(TemplateFact {
                template_id: original.template_id,
                slots: fields.collect(),
            }),
        };
        SequenceMatch {
            fact,
            lengths: self.lengths.clone(),
        }
    }
}

fn capture(values: &[Value]) -> Value {
    Value::Multifield(Box::new(values.iter().cloned().collect()))
}

fn segment_values(source: SequenceSource, fact: &Fact) -> Option<&[Value]> {
    match (source, fact) {
        (SequenceSource::Ordered, Fact::Ordered(fact)) => Some(fact.fields.as_slice()),
        (SequenceSource::TemplateSlot(index), Fact::Template(fact)) => {
            Some(match fact.slots.get(index)? {
                Value::Multifield(values) => values.as_slice(),
                value => std::slice::from_ref(value),
            })
        }
        (SequenceSource::TemplateScalar(index), Fact::Template(fact)) => {
            fact.slots.get(index).map(std::slice::from_ref)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtomKey, StringEncoding, SymbolTable};
    use std::collections::HashSet;

    fn fact(width: usize) -> Fact {
        let mut symbols = SymbolTable::new();
        Fact::Ordered(OrderedFact {
            relation: symbols.intern_symbol("row", StringEncoding::Ascii).unwrap(),
            fields: (0..width)
                .map(|value| Value::Integer(i64::try_from(value).unwrap()))
                .collect(),
        })
    }

    #[test]
    fn sequence_splits_enumerate_every_composition_once() {
        for width in 0..=5 {
            for captures in 1..=5 {
                let pattern = SequencePattern {
                    segments: vec![SequenceSegment {
                        source: SequenceSource::Ordered,
                        fields: vec![SequenceField::Multi; captures],
                    }],
                    tests: vec![],
                };
                let source = fact(width);
                let matches: Vec<_> = pattern.matches(&source).collect();
                // Stars and bars: C(width + captures - 1, captures - 1).
                let expected = (1..captures).fold(1, |count, k| count * (width + k) / k);
                assert_eq!(
                    matches.len(),
                    expected,
                    "width={width}, captures={captures}"
                );
                let unique: HashSet<_> = matches
                    .iter()
                    .map(|matched| matched.lengths.clone())
                    .collect();
                assert_eq!(unique.len(), expected);
                for matched in &matches {
                    assert_eq!(matched.lengths.iter().sum::<usize>(), width);
                    let Fact::Ordered(projected) = &matched.fact else {
                        panic!("ordered projection");
                    };
                    let flattened: Vec<_> = projected
                        .fields
                        .iter()
                        .flat_map(|value| {
                            let Value::Multifield(values) = value else {
                                panic!("multifield capture");
                            };
                            values.iter().cloned()
                        })
                        .collect();
                    let Fact::Ordered(original) = &source else {
                        unreachable!()
                    };
                    assert!(flattened
                        .iter()
                        .zip(&original.fields)
                        .all(|(a, b)| a.structural_eq(b)));
                }
                assert!(matches
                    .windows(2)
                    .all(|pair| pair[0].lengths > pair[1].lengths));
            }
        }
    }

    #[test]
    fn sequence_fixed_fields_constrain_middle_and_empty_captures() {
        let pattern = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: vec![
                    SequenceField::Single,
                    SequenceField::Multi,
                    SequenceField::Single,
                ],
            }],
            tests: vec![
                ConstantTest {
                    slot: SlotIndex::Ordered(0),
                    test_type: ConstantTestType::Equal(AtomKey::Integer(0)),
                },
                ConstantTest {
                    slot: SlotIndex::Ordered(2),
                    test_type: ConstantTestType::Equal(AtomKey::Integer(3)),
                },
            ],
        };
        let source = fact(4);
        let matched = pattern.matches(&source).next().unwrap();
        assert_eq!(matched.lengths.as_slice(), &[2]);
        assert_eq!(pattern.matches(&fact(3)).count(), 0);
        let empty = SequencePattern {
            tests: vec![],
            ..pattern
        };
        assert_eq!(
            empty.matches(&fact(2)).next().unwrap().lengths.as_slice(),
            &[0]
        );
        assert_eq!(empty.matches(&fact(1)).count(), 0);
    }

    #[test]
    fn sequence_validation_rejects_physical_and_out_of_bounds_tests() {
        let mut pattern = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: vec![SequenceField::Multi],
            }],
            tests: vec![],
        };
        assert!(pattern.validate().is_ok());
        pattern.tests.push(ConstantTest {
            slot: SlotIndex::Ordered(1),
            test_type: ConstantTestType::Equal(AtomKey::Integer(0)),
        });
        assert!(pattern.validate().is_err());
        pattern.tests[0].slot = SlotIndex::Ordered(0);
        pattern.tests[0].test_type = ConstantTestType::EqualSlot(SlotIndex::Ordered(1));
        assert!(pattern.validate().is_err());
        pattern.tests[0].test_type = ConstantTestType::OrderedFieldCount { min: 0, max: None };
        assert!(pattern.validate().is_err());
    }

    #[test]
    fn sequence_disjunction_waits_for_every_referenced_field() {
        let pattern = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: vec![
                    SequenceField::Single,
                    SequenceField::Multi,
                    SequenceField::Single,
                ],
            }],
            tests: vec![ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::Any(vec![
                    vec![ConstantTest {
                        slot: SlotIndex::Ordered(0),
                        test_type: ConstantTestType::EqualSlotOffset(SlotIndex::Ordered(2), -3),
                    }],
                    vec![ConstantTest {
                        slot: SlotIndex::Ordered(0),
                        test_type: ConstantTestType::Equal(AtomKey::Integer(99)),
                    }],
                ]),
            }],
        };
        assert!(pattern.validate().is_ok());
        assert_eq!(
            pattern.matches(&fact(4)).next().unwrap().lengths.as_slice(),
            &[2]
        );
        assert_eq!(pattern.matches(&fact(3)).count(), 0);
        assert!(pattern.physical_test(&pattern.tests[0]).is_none());
    }

    #[test]
    fn sequence_disjunction_remaps_every_nested_selector() {
        let pattern = SequencePattern {
            segments: vec![
                SequenceSegment {
                    source: SequenceSource::TemplateScalar(3),
                    fields: vec![SequenceField::Single],
                },
                SequenceSegment {
                    source: SequenceSource::TemplateScalar(1),
                    fields: vec![SequenceField::Single],
                },
            ],
            tests: vec![],
        };
        let test = ConstantTest {
            slot: SlotIndex::Template(0),
            test_type: ConstantTestType::Any(vec![vec![ConstantTest {
                slot: SlotIndex::Template(1),
                test_type: ConstantTestType::EqualSlotOffset(SlotIndex::Template(0), 2),
            }]]),
        };
        assert_eq!(
            pattern.physical_test(&test),
            Some(ConstantTest {
                slot: SlotIndex::Template(3),
                test_type: ConstantTestType::Any(vec![vec![ConstantTest {
                    slot: SlotIndex::Template(1),
                    test_type: ConstantTestType::EqualSlotOffset(SlotIndex::Template(3), 2),
                }]]),
            })
        );
    }

    #[test]
    fn sequence_disjunction_validates_nested_selectors_and_constraints() {
        let mut pattern = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: vec![SequenceField::Single],
            }],
            tests: vec![],
        };
        for test in [
            ConstantTest {
                slot: SlotIndex::Ordered(1),
                test_type: ConstantTestType::Equal(AtomKey::Integer(0)),
            },
            ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::EqualSlot(SlotIndex::Ordered(1)),
            },
            ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::OrderedFieldCount { min: 0, max: None },
            },
        ] {
            pattern.tests = vec![ConstantTest {
                slot: SlotIndex::Ordered(0),
                test_type: ConstantTestType::Any(vec![vec![test]]),
            }];
            assert!(pattern.validate().is_err());
        }
        pattern.tests = vec![ConstantTest {
            slot: SlotIndex::Ordered(0),
            test_type: ConstantTestType::Any(vec![vec![
                ConstantTest {
                    slot: SlotIndex::Ordered(0),
                    test_type: ConstantTestType::Equal(AtomKey::Integer(0)),
                };
                crate::compiler::MAX_ALPHA_TESTS
            ]]),
        }];
        assert!(pattern.validate().unwrap_err().contains("exceeds 64 tests"));
    }

    fn multifield(values: &[i64]) -> Value {
        Value::Multifield(Box::new(
            values.iter().copied().map(Value::Integer).collect(),
        ))
    }

    fn template(slots: Vec<Value>) -> Fact {
        let mut ids: slotmap::SlotMap<crate::fact::TemplateId, ()> = slotmap::SlotMap::with_key();
        Fact::Template(TemplateFact {
            template_id: ids.insert(()),
            slots: slots.into_boxed_slice(),
        })
    }

    #[test]
    fn template_segments_project_scalar_and_multifield_values() {
        let source = template(vec![
            Value::Integer(42),
            multifield(&[10, 20]),
            multifield(&[]),
        ]);
        let plan = SequencePattern {
            segments: vec![
                SequenceSegment {
                    source: SequenceSource::TemplateSlot(1),
                    fields: vec![SequenceField::Single, SequenceField::Multi],
                },
                SequenceSegment {
                    source: SequenceSource::TemplateSlot(0),
                    fields: vec![SequenceField::Single],
                },
                SequenceSegment {
                    source: SequenceSource::TemplateSlot(2),
                    fields: vec![SequenceField::Multi],
                },
            ],
            tests: vec![ConstantTest {
                slot: SlotIndex::Template(2),
                test_type: ConstantTestType::Equal(AtomKey::Integer(42)),
            }],
        };
        assert!(plan.validate().is_ok());
        assert_eq!(plan.logical_width(), 4);
        let matched = plan.matches(&source).next().unwrap();
        assert_eq!(matched.lengths.as_slice(), &[1, 0]);
        let Fact::Template(projected) = matched.fact else {
            panic!("template projection");
        };
        let expected = [
            Value::Integer(10),
            multifield(&[20]),
            Value::Integer(42),
            multifield(&[]),
        ];
        assert!(projected
            .slots
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.structural_eq(&expected)));
    }

    #[test]
    fn split_views_copy_captures_only_when_read() {
        let source = fact(4);
        let plan = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: vec![
                    SequenceField::Multi,
                    SequenceField::Single,
                    SequenceField::Multi,
                ],
            }],
            tests: vec![ConstantTest {
                slot: SlotIndex::Ordered(1),
                test_type: ConstantTestType::Equal(AtomKey::Integer(2)),
            }],
        };
        let first = plan.search(&source, &mut |event| match event {
            SplitEvent::Match(split) => {
                assert_eq!(split.lengths.as_slice(), &[2, 1]);
                assert!(!split.copied_capture());
                let captured = split.get(SlotIndex::Ordered(0)).unwrap();
                assert!(captured.structural_eq(&multifield(&[0, 1])));
                assert!(split.copied_capture());
                assert!(split.get(SlotIndex::Template(0)).is_none());
                assert!(split.get(SlotIndex::Ordered(3)).is_none());
                ControlFlow::Break(())
            }
            SplitEvent::Step => ControlFlow::Continue(()),
        });
        assert!(first.is_break());
    }

    #[test]
    fn split_search_prunes_on_placed_constants() {
        // (row $? 0 $? 1 $? 2 $?) against a row with no 0 fails on the first
        // capture's lengths alone, instead of enumerating every composition.
        let plan = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::Ordered,
                fields: [SequenceField::Multi, SequenceField::Single]
                    .repeat(3)
                    .into_iter()
                    .chain([SequenceField::Multi])
                    .collect(),
            }],
            tests: (0..3)
                .map(|value| ConstantTest {
                    slot: SlotIndex::Ordered(2 * value + 1),
                    test_type: ConstantTestType::Equal(AtomKey::Integer(
                        i64::try_from(value).unwrap(),
                    )),
                })
                .collect(),
        };
        let count_steps = |source: &Fact| {
            let (mut steps, mut matches) = (0, 0);
            let _ = plan.search(source, &mut |event| {
                match event {
                    SplitEvent::Step => steps += 1,
                    SplitEvent::Match(_) => matches += 1,
                }
                ControlFlow::<()>::Continue(())
            });
            (steps, matches)
        };
        let Fact::Ordered(mut row) = fact(200) else {
            unreachable!()
        };
        row.fields[0] = Value::Integer(7);
        assert_eq!(count_steps(&Fact::Ordered(row)), (198, 0));
        // Fields 0, 1 and 2 are adjacent: one match, found without a full
        // enumeration of the C(200, 3) compositions.
        let (steps, matches) = count_steps(&fact(200));
        assert_eq!(matches, 1);
        assert!(steps < 1_000, "{steps} steps");
    }

    #[test]
    fn recorded_splits_project_without_enumeration() {
        let source = template(vec![multifield(&[1, 2, 3]), multifield(&[4])]);
        let plan = SequencePattern {
            segments: vec![
                SequenceSegment {
                    source: SequenceSource::TemplateSlot(0),
                    fields: vec![
                        SequenceField::Multi,
                        SequenceField::Single,
                        SequenceField::Multi,
                    ],
                },
                SequenceSegment {
                    source: SequenceSource::TemplateSlot(1),
                    fields: vec![SequenceField::Multi, SequenceField::Multi],
                },
            ],
            tests: vec![],
        };
        let mut count = 0;
        for matched in plan.matches(&source) {
            let projected = plan.project(&source, &matched.lengths).unwrap().to_match();
            let (Fact::Template(expected), Fact::Template(actual)) =
                (&matched.fact, &projected.fact)
            else {
                panic!("template projection");
            };
            assert_eq!(projected.lengths, matched.lengths);
            assert!(expected
                .slots
                .iter()
                .zip(actual.slots.iter())
                .all(|(expected, actual)| expected.structural_eq(actual)));
            count += 1;
        }
        assert_eq!(count, 6);
        for lengths in [
            &[][..],
            &[2, 1, 1, 0],
            &[1, 1, 1],
            &[1, 1, 1, 0, 0],
            &[usize::MAX, 3, 0, 1],
        ] {
            assert!(plan.project(&source, lengths).is_none(), "{lengths:?}");
        }
    }

    #[test]
    fn template_segments_require_independent_exact_lengths() {
        let source = template(vec![multifield(&[1, 2]), multifield(&[])]);
        let mut plan = SequencePattern {
            segments: vec![SequenceSegment {
                source: SequenceSource::TemplateSlot(1),
                fields: vec![],
            }],
            tests: vec![],
        };
        // Unmentioned slot 0 does not affect the explicit empty slot 1.
        assert_eq!(plan.matches(&source).count(), 1);
        plan.segments[0].source = SequenceSource::TemplateSlot(0);
        assert_eq!(plan.matches(&source).count(), 0);
        plan.segments[0].fields.push(SequenceField::Single);
        assert_eq!(plan.matches(&source).count(), 0);
        plan.segments[0].fields.push(SequenceField::Single);
        assert_eq!(plan.matches(&source).count(), 1);
        plan.segments[0].source = SequenceSource::TemplateSlot(100);
        assert_eq!(plan.matches(&source).count(), 0);
    }

    #[test]
    fn template_cartesian_splits_follow_written_segment_order() {
        let source = template(vec![
            multifield(&[1, 9, 2, 9, 3]),
            multifield(&[10, 9, 20, 9, 30]),
        ]);
        let fields = vec![
            SequenceField::Multi,
            SequenceField::Single,
            SequenceField::Multi,
        ];
        let plan = SequencePattern {
            segments: vec![
                SequenceSegment {
                    source: SequenceSource::TemplateSlot(1),
                    fields: fields.clone(),
                },
                SequenceSegment {
                    source: SequenceSource::TemplateSlot(0),
                    fields,
                },
            ],
            tests: [1, 4]
                .into_iter()
                .map(|index| ConstantTest {
                    slot: SlotIndex::Template(index),
                    test_type: ConstantTestType::Equal(AtomKey::Integer(9)),
                })
                .collect(),
        };
        let matches: Vec<_> = plan.matches(&source).collect();
        let cuts: Vec<_> = matches
            .iter()
            .map(|matched| matched.lengths.as_slice())
            .collect();
        assert_eq!(
            cuts,
            vec![
                &[3, 1, 3, 1][..],
                &[3, 1, 1, 3],
                &[1, 3, 3, 1],
                &[1, 3, 1, 3]
            ]
        );
        let Fact::Template(first) = &matches[0].fact else {
            panic!("template projection");
        };
        assert!(first.slots[0].structural_eq(&multifield(&[10, 9, 20])));
        assert!(first.slots[3].structural_eq(&multifield(&[1, 9, 2])));
    }

    #[test]
    fn sequence_validation_rejects_mixed_duplicate_and_wrong_kind_sources() {
        let segment = SequenceSegment {
            source: SequenceSource::TemplateSlot(0),
            fields: vec![SequenceField::Multi],
        };
        let mut plan = SequencePattern {
            segments: vec![segment.clone()],
            tests: vec![],
        };
        let Fact::Template(template) = template(vec![]) else {
            unreachable!()
        };
        assert!(plan
            .validate_entry(&AlphaEntryType::Template(template.template_id))
            .is_ok());
        plan.segments.push(segment);
        assert!(plan.validate().is_err());
        plan.segments[1].source = SequenceSource::Ordered;
        assert!(plan.validate().is_err());
        plan.segments.remove(0);
        assert!(plan
            .validate_entry(&AlphaEntryType::Template(template.template_id))
            .is_err());
        plan.segments.clear();
        assert!(plan.validate().is_err());
    }
}

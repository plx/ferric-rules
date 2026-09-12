//! Sequence matching with independent zero-or-more captures in ordered facts and template slots.

use smallvec::SmallVec;

use crate::alpha::{evaluate_test, AlphaEntryType, ConstantTest, ConstantTestType, SlotIndex};
use crate::fact::{Fact, OrderedFact, TemplateFact};
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
    Ordered,
    TemplateSlot(usize),
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
#[derive(Clone, Debug)]
pub struct SequenceMatch {
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

    /// Check the sources and logical selectors before installing or restoring a pattern.
    pub fn validate(&self) -> Result<(), String> {
        if self.segments.is_empty() {
            return Err("sequence pattern has no source segments".to_string());
        }
        if !self.is_ordered() {
            let mut sources = rustc_hash::FxHashSet::default();
            for segment in &self.segments {
                let SequenceSource::TemplateSlot(index) = segment.source else {
                    return Err("ordered sequence source must be the only segment".to_string());
                };
                if !sources.insert(index) {
                    return Err("sequence pattern repeats a template slot source".to_string());
                }
            }
        }
        if self.tests.len() > crate::compiler::MAX_ALPHA_TESTS {
            return Err("sequence pattern exceeds 64 tests".to_string());
        }
        let valid_slot = self.logical_slot_validator();
        for test in &self.tests {
            if !valid_slot(test.slot) {
                return Err("sequence test has an invalid logical field".to_string());
            }
            match test.test_type {
                ConstantTestType::OrderedFieldCount { .. } => {
                    return Err(
                        "sequence test contains a physical field-count constraint".to_string()
                    );
                }
                ConstantTestType::EqualSlot(slot)
                | ConstantTestType::NotEqualSlot(slot)
                | ConstantTestType::EqualSlotOffset(slot, _)
                | ConstantTestType::NotEqualSlotOffset(slot, _)
                | ConstantTestType::GreaterThanSlotOffset(slot, _)
                | ConstantTestType::LessThanSlotOffset(slot, _)
                | ConstantTestType::GreaterOrEqualSlotOffset(slot, _)
                | ConstantTestType::LessOrEqualSlotOffset(slot, _)
                    if !valid_slot(slot) =>
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

    /// Enumerate all positional split combinations lazily, before constant tests.
    /// Each step yields one candidate, including those rejected by constraints.
    pub fn candidates<'a>(&'a self, fact: &'a Fact) -> SequenceCandidates<'a> {
        let cursors: Option<Vec<_>> = self
            .segments
            .iter()
            .map(|segment| {
                let values = match (segment.source, fact) {
                    (SequenceSource::Ordered, Fact::Ordered(fact)) => fact.fields.as_slice(),
                    (SequenceSource::TemplateSlot(index), Fact::Template(fact)) => {
                        match fact.slots.get(index)? {
                            Value::Multifield(values) => values.as_slice(),
                            value => std::slice::from_ref(value),
                        }
                    }
                    _ => return None,
                };
                SegmentCursor::new(&segment.fields, values)
            })
            .collect();
        let done = self.segments.is_empty() || cursors.is_none();
        SequenceCandidates {
            fact,
            cursors: cursors.unwrap_or_default(),
            width: self.logical_width(),
            done,
        }
    }

    /// Evaluate constant constraints against an already projected candidate.
    #[must_use]
    pub fn accepts(&self, fact: &Fact) -> bool {
        self.tests.iter().all(|test| evaluate_test(fact, test))
    }

    /// Enumerate every matching split, including empty and anonymous captures.
    pub fn matches<'a>(&'a self, fact: &'a Fact) -> impl Iterator<Item = SequenceMatch> + 'a {
        self.candidates(fact)
            .filter(|candidate| self.accepts(&candidate.fact))
    }
}

/// Lazy Cartesian product in written source order; the final segment advances fastest.
/// Within each segment, larger earlier captures are inserted first, as in CLIPS.
pub struct SequenceCandidates<'a> {
    fact: &'a Fact,
    cursors: Vec<SegmentCursor<'a>>,
    width: usize,
    done: bool,
}

struct SegmentCursor<'a> {
    fields: &'a [SequenceField],
    values: &'a [Value],
    lengths: SmallVec<[usize; 2]>,
    extra: usize,
}

impl<'a> SegmentCursor<'a> {
    fn new(fields: &'a [SequenceField], values: &'a [Value]) -> Option<Self> {
        let multi = fields
            .iter()
            .filter(|field| **field == SequenceField::Multi)
            .count();
        let extra = values.len().checked_sub(fields.len() - multi)?;
        if multi == 0 && extra != 0 {
            return None;
        }
        let mut cursor = Self {
            fields,
            values,
            lengths: smallvec::smallvec![0; multi],
            extra,
        };
        cursor.reset();
        Some(cursor)
    }

    fn reset(&mut self) {
        self.lengths.fill(0);
        if let Some(first) = self.lengths.first_mut() {
            *first = self.extra;
        }
    }

    fn advance(&mut self) -> bool {
        if let Some(&last) = self.lengths.last() {
            let mut available = last;
            for index in (0..self.lengths.len() - 1).rev() {
                if self.lengths[index] > 0 {
                    self.lengths[index] -= 1;
                    self.lengths[index + 1..].fill(0);
                    self.lengths[index + 1] = available + 1;
                    return true;
                }
                available += self.lengths[index];
            }
        }
        false
    }

    fn append_fields(&self, output: &mut SmallVec<[Value; 8]>) {
        let mut offset = 0;
        let mut ranges = self.lengths.iter();
        for field in self.fields {
            match field {
                SequenceField::Single => {
                    output.push(self.values[offset].clone());
                    offset += 1;
                }
                SequenceField::Multi => {
                    let length = *ranges.next().expect("one length for every capture");
                    let captured = self.values[offset..offset + length]
                        .iter()
                        .cloned()
                        .collect();
                    output.push(Value::Multifield(Box::new(captured)));
                    offset += length;
                }
            }
        }
    }
}

impl Iterator for SequenceCandidates<'_> {
    type Item = SequenceMatch;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut fields = SmallVec::with_capacity(self.width);
        let mut lengths = SmallVec::new();
        for cursor in &self.cursors {
            cursor.append_fields(&mut fields);
            lengths.extend_from_slice(&cursor.lengths);
        }
        let fact = match self.fact {
            Fact::Ordered(original) => Fact::Ordered(OrderedFact {
                relation: original.relation,
                fields,
            }),
            Fact::Template(original) => Fact::Template(TemplateFact {
                template_id: original.template_id,
                slots: fields.into_vec().into_boxed_slice(),
            }),
        };
        self.done = true;
        for cursor in self.cursors.iter_mut().rev() {
            if cursor.advance() {
                self.done = false;
                break;
            }
            cursor.reset();
        }
        Some(SequenceMatch { fact, lengths })
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

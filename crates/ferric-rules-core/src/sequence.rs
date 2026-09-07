//! Ordered sequence matching with zero-or-more field captures.

use smallvec::SmallVec;

use crate::alpha::{evaluate_test, ConstantTest, ConstantTestType, SlotIndex};
use crate::fact::{Fact, OrderedFact};
use crate::value::Value;

/// Number of physical fields consumed by one logical pattern field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SequenceField {
    Single,
    Multi,
}

/// A sequence projection and the constant constraints on its logical fields.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SequencePattern {
    pub fields: Vec<SequenceField>,
    pub tests: Vec<ConstantTest>,
}

/// One positional match. Lengths identify captures even when none are named.
#[derive(Clone, Debug)]
pub struct SequenceMatch {
    pub fact: Fact,
    pub lengths: SmallVec<[usize; 2]>,
}

impl SequencePattern {
    /// Check the logical selectors before installing or restoring a pattern.
    pub fn validate(&self) -> Result<(), String> {
        if self.tests.len() > crate::compiler::MAX_ALPHA_TESTS {
            return Err("sequence pattern exceeds 64 tests".to_string());
        }
        let valid_slot =
            |slot| matches!(slot, SlotIndex::Ordered(index) if index < self.fields.len());
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

    /// Enumerate all possible positional splits lazily, before constant tests.
    /// Each step yields one candidate, including those rejected by constraints.
    pub fn candidates<'a>(&'a self, fact: &'a Fact) -> SequenceCandidates<'a> {
        let ordered = match fact {
            Fact::Ordered(ordered) => Some(ordered),
            Fact::Template(_) => None,
        };
        let multi = self
            .fields
            .iter()
            .filter(|field| **field == SequenceField::Multi)
            .count();
        let single = self.fields.len() - multi;
        let remaining = ordered.and_then(|fact| fact.fields.len().checked_sub(single));
        let mut lengths = smallvec::smallvec![0; multi];
        if let (Some(remaining), Some(first)) = (remaining, lengths.first_mut()) {
            *first = remaining;
        }
        let done = remaining.is_none() || (multi == 0 && remaining != Some(0));
        SequenceCandidates {
            pattern: self,
            ordered,
            lengths,
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

/// Lazy weak-composition enumeration; storage is linear in the pattern width.
/// Larger earlier captures are inserted first, matching CLIPS activation order.
pub struct SequenceCandidates<'a> {
    pattern: &'a SequencePattern,
    ordered: Option<&'a OrderedFact>,
    lengths: SmallVec<[usize; 2]>,
    done: bool,
}

impl Iterator for SequenceCandidates<'_> {
    type Item = SequenceMatch;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let ordered = self.ordered?;
        let mut fields = SmallVec::with_capacity(self.pattern.fields.len());
        let mut offset = 0;
        let mut ranges = self.lengths.iter();
        for field in &self.pattern.fields {
            match field {
                SequenceField::Single => {
                    fields.push(ordered.fields[offset].clone());
                    offset += 1;
                }
                SequenceField::Multi => {
                    let length = *ranges.next().expect("one length for every capture");
                    let captured = ordered.fields[offset..offset + length]
                        .iter()
                        .cloned()
                        .collect();
                    fields.push(Value::Multifield(Box::new(captured)));
                    offset += length;
                }
            }
        }
        let candidate = SequenceMatch {
            fact: Fact::Ordered(OrderedFact {
                relation: ordered.relation,
                fields,
            }),
            lengths: self.lengths.clone(),
        };
        self.done = true;
        if let Some(&last) = self.lengths.last() {
            let mut available = last;
            for index in (0..self.lengths.len() - 1).rev() {
                if self.lengths[index] > 0 {
                    self.lengths[index] -= 1;
                    self.lengths[index + 1..].fill(0);
                    self.lengths[index + 1] = available + 1;
                    self.done = false;
                    break;
                }
                available += self.lengths[index];
            }
        }
        Some(candidate)
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
                    fields: vec![SequenceField::Multi; captures],
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
            fields: vec![
                SequenceField::Single,
                SequenceField::Multi,
                SequenceField::Single,
            ],
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
            fields: vec![SequenceField::Multi],
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
}

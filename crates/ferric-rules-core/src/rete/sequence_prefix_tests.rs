//! Prefix joins must retain the exhaustive search's values, multiplicity and order.

use super::*;
use crate::alpha::{ConstantTest, ConstantTestType};
use crate::fact::{OrderedFact, TemplateFact};
use crate::sequence::{SequenceField, SequenceSegment, SequenceSource};
use crate::{FerricString, StringEncoding, SymbolTable};

fn ordered(values: impl IntoIterator<Item = Value>) -> Fact {
    let mut symbols = SymbolTable::new();
    Fact::Ordered(OrderedFact {
        relation: symbols.intern_symbol("row", StringEncoding::Ascii).unwrap(),
        fields: values.into_iter().collect(),
    })
}

fn multifield(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Multifield(Box::new(values.into_iter().collect()))
}

fn ordered_plan(fields: &[SequenceField]) -> SequencePattern {
    SequencePattern {
        segments: vec![SequenceSegment {
            source: SequenceSource::Ordered,
            fields: fields.to_vec(),
        }],
        tests: vec![],
    }
}

fn pair_plan() -> SequencePattern {
    ordered_plan(&[
        SequenceField::Multi,
        SequenceField::Single,
        SequenceField::Multi,
        SequenceField::Single,
        SequenceField::Multi,
    ])
}

fn parent(values: impl IntoIterator<Item = Value>) -> BindingSet {
    let mut bindings = BindingSet::new();
    for (index, value) in values.into_iter().enumerate() {
        bindings.set(VarId(u16::try_from(index).unwrap()), ValueRef::new(value));
    }
    bindings
}

fn equality(slot: SlotIndex, variable: u16) -> JoinTest {
    JoinTest {
        alpha_slot: slot,
        beta_var: VarId(variable),
        test_type: JoinTestType::Equal,
    }
}

fn logical_slot(plan: &SequencePattern, index: usize) -> SlotIndex {
    if plan.is_ordered() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    }
}

/// Compare to the old algorithm: constant-only enumeration followed by a full
/// join check. Read all result fields too, so binding extraction is covered.
fn assert_equivalent(
    fact: &Fact,
    plan: &SequencePattern,
    parent: &BindingSet,
    tests: &[JoinTest],
) -> Vec<SmallVec<[usize; 2]>> {
    let mut expected = Vec::new();
    let _ = plan.search(fact, &mut |event| {
        if let SplitEvent::Match(split) = event {
            if evaluate_join_fields(|slot| split.get(slot), Some(parent), tests) {
                let values: Vec<_> = (0..plan.logical_width())
                    .map(|index| split.get(logical_slot(plan, index)).unwrap().clone())
                    .collect();
                expected.push((split.lengths.clone(), values));
            }
        }
        ControlFlow::<()>::Continue(())
    });
    let extracted: Vec<_> = (0..plan.logical_width())
        .map(|index| {
            (
                logical_slot(plan, index),
                VarId(u16::try_from(index + 8).unwrap()),
            )
        })
        .collect();
    let actual = sequence_matches(fact, parent, tests, &extracted, plan);
    assert_eq!(actual.len(), expected.len(), "{fact:?}, {tests:?}");
    for ((bindings, lengths), (expected_lengths, values)) in actual.iter().zip(&expected) {
        assert_eq!(lengths, expected_lengths, "split order changed");
        for ((_, variable), expected) in extracted.iter().zip(values) {
            assert!(bindings.get(*variable).unwrap().structural_eq(expected));
        }
        for (index, expected) in parent.as_ref().iter().enumerate() {
            let actual = bindings.get(VarId(u16::try_from(index).unwrap()));
            match (actual, expected) {
                (Some(actual), Some(expected)) => assert!(actual.structural_eq(expected)),
                (None, None) => {}
                _ => panic!("parent binding changed"),
            }
        }
        let split = plan.project(fact, lengths).unwrap();
        assert!(split_matches(&split, parent, tests, plan));
    }
    let token = Token {
        fact: None,
        bindings: parent.clone(),
        parent: None,
        owner_node: NodeId(0),
    };
    assert_eq!(
        any_split_matches(fact, &token, tests, plan),
        !expected.is_empty()
    );
    expected.into_iter().map(|(lengths, _)| lengths).collect()
}

#[test]
fn prefix_joins_match_exhaustive_splits_for_small_repeated_sequences() {
    let plan = pair_plan();
    let tests = [
        equality(SlotIndex::Ordered(1), 0),
        equality(SlotIndex::Ordered(3), 1),
    ];
    // Enumerate every binary input through six fields. Repeated values produce
    // multiple valid splits, including empty and anonymous captures.
    for width in 0_usize..=6 {
        for bits in 0_usize..(1 << width) {
            let fact = ordered(
                (0..width).map(|index| Value::Integer(i64::try_from((bits >> index) & 1).unwrap())),
            );
            for first in -1..=2 {
                for second in -1..=2 {
                    let parent = parent([Value::Integer(first), Value::Integer(second)]);
                    assert_equivalent(&fact, &plan, &parent, &tests);
                }
            }
        }
    }
}

fn all_comparisons() -> Vec<JoinTestType> {
    let mut kinds = vec![
        JoinTestType::Equal,
        JoinTestType::NotEqual,
        JoinTestType::GreaterThan,
        JoinTestType::LessThan,
        JoinTestType::GreaterOrEqual,
        JoinTestType::LessOrEqual,
        JoinTestType::LexEqual,
        JoinTestType::LexNotEqual,
        JoinTestType::LexGreaterThan,
        JoinTestType::LexLessThan,
        JoinTestType::LexGreaterOrEqual,
        JoinTestType::LexLessOrEqual,
    ];
    for offset in [-1, 1] {
        kinds.extend([
            JoinTestType::EqualOffset(offset),
            JoinTestType::NotEqualOffset(offset),
            JoinTestType::GreaterThanOffset(offset),
            JoinTestType::LessThanOffset(offset),
            JoinTestType::GreaterOrEqualOffset(offset),
            JoinTestType::LessOrEqualOffset(offset),
        ]);
    }
    kinds
}

#[test]
fn prefix_joins_share_typed_numeric_lexeme_and_offset_semantics() {
    let values = vec![
        Value::Integer(i64::MIN),
        Value::Integer(-1),
        Value::Integer(0),
        Value::Integer(1),
        Value::Integer(i64::MAX),
        Value::Float(-0.0),
        Value::Float(0.0),
        Value::Float(1.0),
        Value::Float(f64::NAN),
        Value::Float(f64::INFINITY),
        Value::String(FerricString::new("a", StringEncoding::Ascii).unwrap()),
        Value::String(FerricString::new("b", StringEncoding::Ascii).unwrap()),
        Value::Void,
    ];
    let fact = ordered(values.clone());
    let plan = ordered_plan(&[
        SequenceField::Multi,
        SequenceField::Single,
        SequenceField::Multi,
    ]);
    for value in values {
        let parent = parent([value]);
        for test_type in all_comparisons() {
            let test = JoinTest {
                alpha_slot: SlotIndex::Ordered(1),
                beta_var: VarId(0),
                test_type,
            };
            assert_equivalent(&fact, &plan, &parent, &[test]);
        }
    }
}

#[test]
fn joins_against_captures_keep_empty_and_nested_multifield_values() {
    let nested = multifield([Value::Integer(1), multifield([Value::Integer(2)])]);
    let source = vec![nested.clone(), Value::Integer(3), Value::Integer(4)];
    let fact = ordered(source.clone());
    let plan = ordered_plan(&[
        SequenceField::Multi,
        SequenceField::Multi,
        SequenceField::Single,
        SequenceField::Multi,
    ]);
    let tests = [
        equality(SlotIndex::Ordered(0), 0),
        equality(SlotIndex::Ordered(2), 1),
    ];
    for prefix in [vec![], vec![nested], source] {
        let parent = parent([multifield(prefix), Value::Integer(4)]);
        let matched = assert_equivalent(&fact, &plan, &parent, &tests);
        if let Value::Multifield(bound) = &**parent.get(VarId(0)).unwrap() {
            assert_eq!(matched.len(), usize::from(bound.len() < 3));
        }
    }
}

#[test]
fn prefix_joins_preserve_independent_template_segments_and_deferred_any_tests() {
    let mut templates = slotmap::SlotMap::with_key();
    let fact = Fact::Template(TemplateFact {
        template_id: templates.insert(()),
        slots: vec![
            multifield([Value::Integer(1), Value::Integer(2)]),
            Value::Integer(9),
            multifield([Value::Integer(2), Value::Integer(3)]),
        ]
        .into_boxed_slice(),
    });
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
                source: SequenceSource::TemplateScalar(1),
                fields: vec![SequenceField::Single],
            },
            SequenceSegment {
                source: SequenceSource::TemplateSlot(2),
                fields: vec![
                    SequenceField::Multi,
                    SequenceField::Single,
                    SequenceField::Multi,
                ],
            },
        ],
        tests: vec![ConstantTest {
            slot: SlotIndex::Template(1),
            test_type: ConstantTestType::Any(vec![
                vec![ConstantTest {
                    slot: SlotIndex::Template(1),
                    test_type: ConstantTestType::EqualSlot(SlotIndex::Template(5)),
                }],
                vec![ConstantTest {
                    slot: SlotIndex::Template(1),
                    test_type: ConstantTestType::EqualSlotOffset(SlotIndex::Template(5), -1),
                }],
            ]),
        }],
    };
    assert!(plan.validate().is_ok());
    assert_eq!(
        assert_equivalent(&fact, &plan, &BindingSet::new(), &[]).len(),
        3
    );
    let tests = [
        equality(SlotIndex::Template(1), 0),
        equality(SlotIndex::Template(5), 1),
    ];
    for first in 0..=3 {
        for second in 0..=4 {
            let parent = parent([Value::Integer(first), Value::Integer(second)]);
            assert_equivalent(&fact, &plan, &parent, &tests);
        }
    }
}

#[test]
fn complete_prefix_rejects_invalid_selectors_and_missing_bindings_without_steps() {
    for fields in [
        vec![],
        vec![SequenceField::Multi],
        vec![SequenceField::Single, SequenceField::Multi],
    ] {
        let fact = if fields.is_empty() {
            ordered([])
        } else {
            ordered([Value::Integer(1), Value::Integer(2)])
        };
        let plan = ordered_plan(&fields);
        for slot in [
            SlotIndex::Ordered(fields.len()),
            SlotIndex::Ordered(usize::MAX),
            SlotIndex::Template(0),
        ] {
            let parent = parent([Value::Integer(1)]);
            assert!(assert_equivalent(&fact, &plan, &parent, &[equality(slot, 0)]).is_empty());
        }
        assert!(assert_equivalent(
            &fact,
            &plan,
            &BindingSet::new(),
            &[equality(SlotIndex::Ordered(0), 0)]
        )
        .is_empty());
    }
}

fn steps_and_matches(
    fact: &Fact,
    plan: &SequencePattern,
    parent: &BindingSet,
    tests: &[JoinTest],
    prune: bool,
) -> (usize, usize) {
    let mut steps = 0;
    let mut matches = 0;
    let _ = plan.search_with_prefix(
        fact,
        &mut |split, checked, complete| {
            !prune || sequence_join_prefix_matches(split, checked, complete, parent, tests)
        },
        &mut |event| {
            match event {
                SplitEvent::Step => steps += 1,
                SplitEvent::Match(split) => {
                    if prune || evaluate_join_fields(|slot| split.get(slot), Some(parent), tests) {
                        matches += 1;
                    }
                }
            }
            ControlFlow::<()>::Continue(())
        },
    );
    (steps, matches)
}

#[test]
fn bound_scalar_fields_prune_before_later_capture_choices() {
    let plan = pair_plan();
    let tests = [
        equality(SlotIndex::Ordered(1), 0),
        equality(SlotIndex::Ordered(3), 1),
    ];
    for width in [32, 128, 512] {
        let fact = ordered((0..width).map(Value::Integer));
        for (first, second, expected) in [
            (width / 4, width * 3 / 4, 1),
            (-1, width - 1, 0),
            (0, -1, 0),
            (width * 3 / 4, width / 4, 0),
            (width / 2, width / 2, 0),
        ] {
            let parent = parent([Value::Integer(first), Value::Integer(second)]);
            let (steps, matched) = steps_and_matches(&fact, &plan, &parent, &tests, true);
            let width = usize::try_from(width).unwrap();
            assert_eq!(matched, expected);
            assert!(steps <= 2 * width, "{steps} choices for {width} fields");
        }
        let parent = parent([Value::Integer(width / 4), Value::Integer(width * 3 / 4)]);
        let (steps, matched) = steps_and_matches(&fact, &plan, &parent, &tests, false);
        let width = usize::try_from(width).unwrap();
        assert_eq!(matched, 1);
        assert!(
            steps > width * width / 3,
            "exhaustive reference must exercise quadratic choices"
        );
    }
}

#[test]
fn bound_splits_keep_longest_first_capture_order() {
    let fact = ordered([0, 1, 0, 1].map(Value::Integer));
    let parent = parent([Value::Integer(0), Value::Integer(1)]);
    let tests = [
        equality(SlotIndex::Ordered(1), 0),
        equality(SlotIndex::Ordered(3), 1),
    ];
    let matches = assert_equivalent(&fact, &pair_plan(), &parent, &tests);
    let lengths: Vec<_> = matches.iter().map(SmallVec::as_slice).collect();
    assert_eq!(
        lengths,
        vec![&[2, 0, 0][..], &[0, 2, 0][..], &[0, 0, 2][..]]
    );
}

#[test]
fn prefix_callback_intervals_restart_for_each_capture_choice() {
    let fact = ordered([Value::Integer(1), Value::Integer(2)]);
    let plan = ordered_plan(&[
        SequenceField::Multi,
        SequenceField::Single,
        SequenceField::Multi,
    ]);
    let mut intervals = Vec::new();
    let mut matches = 0;
    let _ = plan.search_with_prefix(
        &fact,
        &mut |split, checked, complete| {
            intervals.push((checked, split.placed_fields(), complete));
            true
        },
        &mut |event| {
            if matches!(event, SplitEvent::Match(_)) {
                matches += 1;
            }
            ControlFlow::<()>::Continue(())
        },
    );
    assert_eq!(matches, 2);
    assert_eq!(
        intervals,
        [
            (0, 0, false),
            (0, 2, false),
            (2, 3, true),
            (0, 2, false),
            (2, 3, true)
        ]
    );
}

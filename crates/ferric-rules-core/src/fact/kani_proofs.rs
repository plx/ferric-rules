//! Kani proof harnesses for `fact` (see docs/verification.md).
//!
//! What is proved, on the production code:
//!
//! * Toward completeness of duplicate detection (structurally equal facts
//!   must have equal structural fingerprints), two partial checks:
//!   - `value_structural_eq_{symbol, integer, float,
//!     void_and_external_address, fact_address}`: on independent atoms of
//!     each kind (arbitrary payloads), `Value::structural_eq` holds exactly
//!     when the components it is documented to compare are equal: symbols
//!     (both pools) and instance names, integers, floats by bit pattern
//!     (NaNs and signed zeros included), void, external addresses, fact
//!     addresses by epoch, fact id and timestamp but not by the cached
//!     public index (dummy equals dummy, never a real address).
//!     `value_structural_eq_distinguishes_atom_kinds`: atoms of two
//!     different kinds among these seven are never equal, whatever their
//!     payloads (e.g. a symbol and the instance name of the same symbol).
//!   - `fingerprint_ignores_fact_address_public_index`,
//!     `fingerprint_ignores_public_index_in_{ordered, template}_fact`: the
//!     structural hash (value level, and fact level for ordered and template
//!     facts) does not depend on the public index, the only component
//!     equality ignores among these kinds. The compared components are fixed representative values
//!     and only the public index is arbitrary: hashing two independent
//!     symbolic inputs is a multiplier-equivalence problem that did not
//!     finish within ten minutes per harness.
//!   For the scalar kinds the hash writes exactly the compared components
//!   (fact.rs:277 `hash_value_structurally`, by inspection, not by Kani).
//!   Strings and multifields are not covered: every string harness (even
//!   one ASCII byte with fixed representations) and every multifield
//!   harness ran out of memory at 7 GB.
//! * `fact_structural_eq_is_component_equality_{ordered, template}`:
//!   `Fact::structural_eq` on facts with one integer field is exactly "same
//!   key and same field", reflexive and symmetric, and never relates an
//!   ordered fact to a template fact.
//! * `timestamp_next_is_successor_below_max`, `timestamp_next_panics_at_max`:
//!   `Timestamp::next` is the checked successor and panics (as documented)
//!   instead of wrapping.
//! * `assertion_capacity_is_exactly_below_max`: `ensure_assertion_capacity`
//!   succeeds iff `next_timestamp < u64::MAX`, for every counter value.
//! * `exhausted_ordered_assertion_has_no_side_effects`,
//!   `exhausted_template_assertion_has_no_side_effects`: starting from an
//!   empty fact base with the counter at `u64::MAX`, `try_assert_fact`
//!   (either duplicate policy) and `try_assert_distinct_fact` (with the
//!   duplicate index present or dropped) return `FactTimestampExhausted`
//!   without overflow or panic and leave the facts, every index and the
//!   counter unchanged (the duplicate index may only be invalidated, which
//!   is derived state). Exhaustion with live facts, duplicate suppression
//!   while exhausted, and the lazy index rebuild are not checked by Kani.
//! * `retract_missing_fact_is_noop`: retracting an arbitrary id from an
//!   empty fact base returns `None` and changes nothing.
//! * `symbol_map_refines_partial_map`: `SymbolMap` (the relation index
//!   container) refines a partial map over all sequences of two
//!   `get_or_insert_with` / `get_mut` / `remove` operations on symbols with
//!   indices below two in both pools, with symbolic values.
//!
//! Harness technique: symbolic facts whose drop would be explored through
//! the recursive drop glue of `Value` are `mem::forget`-ed at the end; drop
//! glue is not part of any property here. Facts that production code drops
//! itself (the exhausted-insertion paths) use a concrete shape.
//!
//! Known gaps: every successful insertion allocates a hashbrown table
//! (`by_relation` / `by_template`), and operations on allocated tables are
//! beyond Kani's budget here: a harness asserting one ordered fact below
//! `u64::MAX`, checking its timestamp and relation index, and retracting it
//! was still in symbolic execution after eleven minutes and was dropped. So
//! successful insertion, index maintenance across insertions and
//! retractions, duplicate detection against an existing fact, the
//! fingerprint-index rebuild, and the chronology cursors are not checked by
//! Kani. Those algorithms are proved unboundedly on a model in
//! verification/verus/core_stores_fact_base.rs, which assumes fingerprint
//! consistency (partly checked here, see above) and abstracts `SymbolMap`
//! as a map (checked here for small symbol indices).

use super::*;
use crate::symbol::InstanceName;
use crate::value::{ExternalAddress, ExternalTypeId, FactAddress};
use slotmap::{Key, KeyData};

fn any_symbol() -> Symbol {
    let index: u32 = kani::any();
    if kani::any() {
        Symbol(SymbolId::Ascii(index))
    } else {
        Symbol(SymbolId::Utf8(index))
    }
}

fn any_fact_id() -> FactId {
    FactId::from(KeyData::from_ffi(kani::any()))
}

fn any_template_id() -> TemplateId {
    TemplateId::from(KeyData::from_ffi(kani::any()))
}

fn values_fingerprint(values: &[Value]) -> u64 {
    let mut hasher = FxHasher::default();
    for value in values {
        hash_value_structurally(value, &mut hasher);
    }
    hasher.finish()
}

fn value_fingerprint(value: &Value) -> u64 {
    values_fingerprint(std::slice::from_ref(value))
}

/// `structural_eq` of two owned values (forgotten afterwards, see the
/// module documentation).
fn values_equal(a: Value, b: Value) -> bool {
    let equal = a.structural_eq(&b);
    std::mem::forget(a);
    std::mem::forget(b);
    equal
}

/// Fingerprints of two owned values (forgotten afterwards).
fn value_fingerprints(a: Value, b: Value) -> (u64, u64) {
    let fingerprints = (value_fingerprint(&a), value_fingerprint(&b));
    std::mem::forget(a);
    std::mem::forget(b);
    fingerprints
}

// Part 1: on independent atoms of each kind, `structural_eq` holds exactly
// when the components it is documented to compare are equal.

#[kani::proof]
#[kani::unwind(2)]
fn value_structural_eq_symbol() {
    let (s1, s2) = (any_symbol(), any_symbol());
    let equal = values_equal(Value::Symbol(s1), Value::Symbol(s2));
    assert_eq!(equal, s1 == s2);
    let (n1, n2) = (any_symbol(), any_symbol());
    assert_eq!(
        values_equal(
            Value::InstanceName(InstanceName::from_symbol(n1)),
            Value::InstanceName(InstanceName::from_symbol(n2))
        ),
        n1 == n2
    );
    kani::cover!(equal);
}

#[kani::proof]
#[kani::unwind(2)]
fn value_structural_eq_integer() {
    let (i1, i2): (i64, i64) = (kani::any(), kani::any());
    assert_eq!(
        values_equal(Value::Integer(i1), Value::Integer(i2)),
        i1 == i2
    );
}

#[kani::proof]
#[kani::unwind(2)]
fn value_structural_eq_float() {
    let (f1, f2): (f64, f64) = (kani::any(), kani::any());
    let equal = values_equal(Value::Float(f1), Value::Float(f2));
    assert_eq!(equal, f1.to_bits() == f2.to_bits());
    kani::cover!(equal && f1.is_nan());
    kani::cover!(!equal && f1 == f2);
}

#[kani::proof]
#[kani::unwind(2)]
fn value_structural_eq_void_and_external_address() {
    assert!(values_equal(Value::Void, Value::Void));
    let (t1, t2, k1, k2): (u32, u32, u64, u64) =
        (kani::any(), kani::any(), kani::any(), kani::any());
    let e1 = ExternalAddress {
        type_id: ExternalTypeId(t1),
        token: k1,
    };
    let e2 = ExternalAddress {
        type_id: ExternalTypeId(t2),
        token: k2,
    };
    assert_eq!(
        values_equal(Value::ExternalAddress(e1), Value::ExternalAddress(e2)),
        t1 == t2 && k1 == k2
    );
}

#[kani::proof]
#[kani::unwind(2)]
fn value_structural_eq_fact_address() {
    let (id1, id2) = (any_fact_id(), any_fact_id());
    let (ep1, ep2): (u64, u64) = (kani::any(), kani::any());
    let (ts1, ts2): (u64, u64) = (kani::any(), kani::any());
    let (ix1, ix2): (u64, u64) = (kani::any(), kani::any());
    let real1 = FactAddress::new(id1, ep1, Timestamp::new(ts1), ix1);
    let real2 = FactAddress::new(id2, ep2, Timestamp::new(ts2), ix2);
    let equal = values_equal(Value::FactAddress(real1.clone()), Value::FactAddress(real2));
    assert_eq!(equal, id1 == id2 && ep1 == ep2 && ts1 == ts2);
    assert!(values_equal(
        Value::FactAddress(FactAddress::dummy()),
        Value::FactAddress(FactAddress::dummy())
    ));
    assert!(!values_equal(
        Value::FactAddress(FactAddress::dummy()),
        Value::FactAddress(real1)
    ));
    kani::cover!(equal && ix1 != ix2);
}

/// An atom of the given kind (0..7: symbol, instance name, integer, float,
/// void, external address, fact address) with an arbitrary payload.
fn any_atom_of_kind(kind: u8) -> Value {
    match kind {
        0 => Value::Symbol(any_symbol()),
        1 => Value::InstanceName(InstanceName::from_symbol(any_symbol())),
        2 => Value::Integer(kani::any()),
        3 => Value::Float(kani::any()),
        4 => Value::Void,
        5 => Value::ExternalAddress(ExternalAddress {
            type_id: ExternalTypeId(kani::any()),
            token: kani::any(),
        }),
        _ => {
            if kani::any() {
                Value::FactAddress(FactAddress::dummy())
            } else {
                Value::FactAddress(FactAddress::new(
                    any_fact_id(),
                    kani::any(),
                    Timestamp::new(kani::any()),
                    kani::any(),
                ))
            }
        }
    }
}

/// Atoms of different kinds are never structurally equal, whatever their
/// payloads (e.g. a symbol and the instance name with the same symbol, an
/// integer and a float with the same bits, void and the dummy fact
/// address).
#[kani::proof]
#[kani::unwind(2)]
fn value_structural_eq_distinguishes_atom_kinds() {
    let k1: u8 = kani::any_where(|kind| *kind < 7);
    let k2: u8 = kani::any_where(|kind| *kind < 7 && *kind != k1);
    let equal = values_equal(any_atom_of_kind(k1), any_atom_of_kind(k2));
    assert!(!equal);
    kani::cover!(k1 == 0 && k2 == 1);
    kani::cover!(k1 == 2 && k2 == 3);
    kani::cover!(k1 == 4 && k2 == 6);
    kani::cover!(k1 == 6 && k2 == 5);
}

// Part 2: the structural hash ignores exactly what equality ignores. The
// compared components are fixed representative values (hashing two
// independent symbolic inputs is a multiplier-equivalence problem that did
// not finish in ten minutes); the ignored components are arbitrary.

/// Fact addresses differing only in the cached public index hash
/// identically.
#[kani::proof]
#[kani::unwind(2)]
fn fingerprint_ignores_fact_address_public_index() {
    let id = fact_id_from(7);
    let (ix1, ix2): (u64, u64) = (kani::any(), kani::any());
    let (h1, h2) = value_fingerprints(
        Value::FactAddress(FactAddress::new(id, 3, Timestamp::new(5), ix1)),
        Value::FactAddress(FactAddress::new(id, 3, Timestamp::new(5), ix2)),
    );
    assert_eq!(h1, h2);
}

fn fact_id_from(raw: u64) -> FactId {
    FactId::from(KeyData::from_ffi(raw))
}

/// A fact (ordered or template, constant at each call site) with one fact
/// address field whose cached public index is arbitrary.
fn fact_with_arbitrary_public_index(ordered: bool) -> Fact {
    let address = Value::FactAddress(FactAddress::new(
        fact_id_from(7),
        3,
        Timestamp::new(5),
        kani::any(),
    ));
    if ordered {
        let mut fields: SmallVec<[Value; 8]> = SmallVec::new();
        fields.push(address);
        Fact::Ordered(OrderedFact {
            relation: Symbol(SymbolId::Ascii(4)),
            fields,
        })
    } else {
        Fact::Template(TemplateFact {
            template_id: TemplateId::from(KeyData::from_ffi(9)),
            slots: Box::new([address]),
        })
    }
}

fn check_public_index_ignored(ordered: bool) {
    let a = fact_with_arbitrary_public_index(ordered);
    let b = fact_with_arbitrary_public_index(ordered);
    assert!(a.structural_eq(&b));
    assert_eq!(structural_fingerprint(&a), structural_fingerprint(&b));
    std::mem::forget(a);
    std::mem::forget(b);
}

/// Ordered facts that differ only in a field's fact-address public index
/// (which `Fact::structural_eq` ignores) are equal and have equal
/// `structural_fingerprint`s. (Ordered and template facts in one harness ran
/// out of memory at 7 GB.)
#[kani::proof]
#[kani::unwind(2)]
fn fingerprint_ignores_public_index_in_ordered_fact() {
    check_public_index_ignored(true);
}

/// As above for template facts.
#[kani::proof]
#[kani::unwind(2)]
fn fingerprint_ignores_public_index_in_template_fact() {
    check_public_index_ignored(false);
}

/// A fact with no fields, or (`with_field`) one integer field.
fn small_fact(ordered: bool, with_field: bool) -> (Fact, u64, Option<i64>) {
    let mut fields: SmallVec<[Value; 8]> = SmallVec::new();
    let field = if with_field {
        let integer: i64 = kani::any();
        fields.push(Value::Integer(integer));
        Some(integer)
    } else {
        None
    };
    if ordered {
        let index: u32 = kani::any();
        let relation = Symbol(SymbolId::Ascii(index));
        (
            Fact::Ordered(OrderedFact { relation, fields }),
            u64::from(index),
            field,
        )
    } else {
        let raw: u64 = kani::any();
        let template_id = TemplateId::from(KeyData::from_ffi(raw));
        (
            Fact::Template(TemplateFact {
                template_id,
                slots: fields.into_vec().into_boxed_slice(),
            }),
            template_id.data().as_ffi(),
            field,
        )
    }
}

/// `Fact::structural_eq` for one (constant) kind and shape.
fn check_fact_eq_shape(ordered: bool, with_field: bool) -> (bool, bool) {
    let (a, key_a, field_a) = small_fact(ordered, with_field);
    let (b, key_b, field_b) = small_fact(ordered, with_field);
    let (other, _, _) = small_fact(!ordered, with_field);
    let equal = a.structural_eq(&b);
    assert_eq!(equal, key_a == key_b && field_a == field_b);
    assert_eq!(equal, b.structural_eq(&a));
    assert!(a.structural_eq(&a));
    assert!(!a.structural_eq(&other) && !other.structural_eq(&a));
    std::mem::forget(a);
    std::mem::forget(b);
    std::mem::forget(other);
    (equal, !equal && key_a == key_b)
}

/// `Fact::structural_eq` on ordered facts with one integer field is exactly
/// "same relation and same field" (ASCII-pool relations), hence reflexive
/// and symmetric, and an ordered fact never equals a template fact. (All
/// four kind/shape combinations in one harness ran out of memory at 7 GB.)
#[kani::proof]
#[kani::unwind(2)]
fn fact_structural_eq_is_component_equality_ordered() {
    let (equal, unequal_same_key) = check_fact_eq_shape(true, true);
    kani::cover!(equal);
    kani::cover!(unequal_same_key);
}

/// As above for template facts with one integer slot.
#[kani::proof]
#[kani::unwind(2)]
fn fact_structural_eq_is_component_equality_template() {
    let (equal, unequal_same_key) = check_fact_eq_shape(false, true);
    kani::cover!(equal);
    kani::cover!(unequal_same_key);
}

/// Independent oracle for `SymbolMap`: a total function from the four symbols
/// `Ascii(0..2)` / `Utf8(0..2)` to optional values.
#[derive(Clone, Copy)]
struct SymbolMapModel {
    ascii: [Option<u8>; 2],
    utf8: [Option<u8>; 2],
}

impl SymbolMapModel {
    fn slot(&mut self, symbol: Symbol) -> &mut Option<u8> {
        match symbol.0 {
            SymbolId::Ascii(index) => &mut self.ascii[index as usize],
            SymbolId::Utf8(index) => &mut self.utf8[index as usize],
        }
    }
}

fn small_symbol() -> Symbol {
    let index: u32 = kani::any_where(|index| *index < 2);
    if kani::any() {
        Symbol(SymbolId::Ascii(index))
    } else {
        Symbol(SymbolId::Utf8(index))
    }
}

/// `SymbolMap` refines a partial map.
#[kani::proof]
#[kani::unwind(3)]
fn symbol_map_refines_partial_map() {
    let mut map = SymbolMap::<u8>::new();
    let mut model = SymbolMapModel {
        ascii: [None; 2],
        utf8: [None; 2],
    };
    let mut step = 0;
    while step < 2 {
        let key = small_symbol();
        let value: u8 = kani::any();
        match kani::any::<u8>() {
            0 => {
                let expected = model.slot(key).get_or_insert(value);
                assert_eq!(*map.get_or_insert_with(key, || value), *expected);
            }
            1 => {
                let expected = model.slot(key).take();
                assert_eq!(map.remove(key), expected);
            }
            2 => {
                let expected = model.slot(key).as_mut().map(|slot| {
                    *slot = value;
                    *slot
                });
                assert_eq!(
                    map.get_mut(key).map(|slot| {
                        *slot = value;
                        *slot
                    }),
                    expected
                );
            }
            _ => {}
        }
        step += 1;
    }
    let mut index = 0;
    while index < 2 {
        let ascii = Symbol(SymbolId::Ascii(index));
        let utf8 = Symbol(SymbolId::Utf8(index));
        assert_eq!(map.get(ascii).copied(), *model.slot(ascii));
        assert_eq!(map.get(utf8).copied(), *model.slot(utf8));
        index += 1;
    }
    // Symbols beyond the grown pools are absent.
    let beyond: u32 = kani::any_where(|index| *index >= 2);
    assert!(map.get(Symbol(SymbolId::Ascii(beyond))).is_none());
    assert!(map.get(Symbol(SymbolId::Utf8(beyond))).is_none());
    kani::cover!(model.ascii[1].is_some() && model.utf8[0].is_some());
    kani::cover!(model.ascii == [None; 2] && !map.ascii.is_empty());
}

/// `Timestamp::next` is the checked successor.
#[kani::proof]
fn timestamp_next_is_successor_below_max() {
    let raw: u64 = kani::any_where(|raw| *raw < u64::MAX);
    let timestamp = Timestamp::new(raw);
    let next = timestamp.next();
    assert_eq!(next.get(), raw + 1);
    assert!(timestamp < next);
    kani::cover!(raw == u64::MAX - 1);
    kani::cover!(raw == 0);
}

/// `Timestamp::next` panics (documented at fact.rs:39) instead of wrapping.
#[kani::proof]
#[kani::should_panic]
fn timestamp_next_panics_at_max() {
    let _ = Timestamp::new(u64::MAX).next();
}

/// `ensure_assertion_capacity` succeeds exactly when another timestamp can
/// be allocated.
#[kani::proof]
#[kani::unwind(3)]
fn assertion_capacity_is_exactly_below_max() {
    let mut fb = FactBase::new();
    let next: u64 = kani::any();
    fb.next_timestamp = Timestamp::new(next);
    assert_eq!(fb.ensure_assertion_capacity().is_ok(), next < u64::MAX);
    kani::cover!(next == u64::MAX);
    kani::cover!(next == u64::MAX - 1);
    std::mem::forget(fb);
}

/// Shared checks after a rejected insertion into an empty, exhausted base.
fn assert_untouched_after_exhaustion(
    fb: &FactBase,
    result: Result<FactInsertionResult, FactTimestampExhausted>,
) {
    assert_eq!(result, Err(FactTimestampExhausted));
    assert!(fb.is_empty());
    assert_eq!(fb.next_timestamp, Timestamp::new(u64::MAX));
    assert!(fb.by_template.is_empty());
    assert!(fb.by_relation.ascii.is_empty() && fb.by_relation.utf8.is_empty());
    assert!(fb
        .by_structural_fingerprint
        .as_ref()
        .is_none_or(HashMap::is_empty));
    assert!(fb.by_template_chronology.is_empty() && fb.by_relation_chronology.is_empty());
    assert!(fb.ensure_assertion_capacity().is_err());
}

/// Insert through a symbolically chosen checked entry point. Before
/// `try_assert_distinct_fact`, the derived duplicate index is arbitrarily
/// dropped first, as an earlier `try_assert_fact(_, true)` or a snapshot
/// restore leaves it (fact.rs:380 documents both); the second component
/// reports whether it was. (`try_assert_fact(_, false)` with a dropped index
/// rebuilds it, and that path was still in symbolic execution after ten
/// minutes, so it is not covered here.)
fn try_insert_any_way(
    fb: &mut FactBase,
    fact: Fact,
) -> (Result<FactInsertionResult, FactTimestampExhausted>, bool) {
    if kani::any() {
        (fb.try_assert_fact(fact, kani::any()), false)
    } else {
        let dropped: bool = kani::any();
        if dropped {
            fb.by_structural_fingerprint = None;
        }
        let result = fb
            .try_assert_distinct_fact(fact)
            .map(FactInsertionResult::Inserted);
        (result, dropped)
    }
}

/// Exhausted counter: an ordered fact with an arbitrary relation is
/// rejected without side effects by every checked entry point. (Field
/// contents do not influence this path; the fact has no fields so that
/// symbolic execution does not explore every `Value` hashing branch.)
#[kani::proof]
#[kani::unwind(4)]
fn exhausted_ordered_assertion_has_no_side_effects() {
    let mut fb = FactBase::new();
    fb.next_timestamp = Timestamp::new(u64::MAX);
    let fact = Fact::Ordered(OrderedFact {
        relation: any_symbol(),
        fields: SmallVec::new(),
    });
    let (result, index_dropped) = try_insert_any_way(&mut fb, fact);
    assert_untouched_after_exhaustion(&fb, result);
    kani::cover!(index_dropped);
    kani::cover!(!index_dropped);
    std::mem::forget(fb);
}

/// Exhausted counter: a template fact with an arbitrary template id is
/// rejected without side effects by every checked entry point (no slots, as
/// above).
#[kani::proof]
#[kani::unwind(4)]
fn exhausted_template_assertion_has_no_side_effects() {
    let mut fb = FactBase::new();
    fb.next_timestamp = Timestamp::new(u64::MAX);
    let fact = Fact::Template(TemplateFact {
        template_id: any_template_id(),
        slots: Box::new([]),
    });
    let (result, index_dropped) = try_insert_any_way(&mut fb, fact);
    assert_untouched_after_exhaustion(&fb, result);
    kani::cover!(index_dropped);
    kani::cover!(!index_dropped);
    std::mem::forget(fb);
}

/// Retracting an id that was never live returns `None` and changes nothing.
#[kani::proof]
#[kani::unwind(3)]
fn retract_missing_fact_is_noop() {
    let mut fb = FactBase::new();
    let start: u64 = kani::any();
    fb.next_timestamp = Timestamp::new(start);
    let id = any_fact_id();
    let removed = fb.retract(id);
    assert!(removed.is_none());
    assert!(fb.is_empty());
    assert_eq!(fb.next_timestamp, Timestamp::new(start));
    assert!(fb.by_relation.ascii.is_empty() && fb.by_relation.utf8.is_empty());
    assert!(fb.by_template.is_empty());
    std::mem::forget(removed);
    std::mem::forget(fb);
}

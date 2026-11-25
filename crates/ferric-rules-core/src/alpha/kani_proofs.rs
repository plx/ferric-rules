//! Kani proof harnesses for `alpha` (see docs/verification.md).
//!
//! What is proved, on the production code, against reference semantics
//! written from the documented contracts (value.rs `AtomKey` and
//! `structural_eq` docs, string.rs, docs/compatibility.md "Structural
//! equality" and "Predicate and return-value constraints"):
//!
//! * Field selection: `get_slot_value` returns exactly the selected field and
//!   `None` for a wrong fact kind or any out-of-range `usize` index, without
//!   panicking; every field test, with any parameters, fails without its own
//!   field, and every slot-pair test fails without its other field.
//! * Literal tests: `Equal`/`NotEqual`/`EqualAny` are type-sensitive literal
//!   identity (floats by bit pattern, so `-0.0 != +0.0` and a NaN equals its
//!   own bits; integers never equal floats; strings by bytes across both
//!   representations; fact addresses ignore the public index); `Void` and
//!   multifields pass none of them; the empty `EqualAny` never matches.
//! * Ordered key tests (`GreaterThan`, `LessThan`, `GreaterOrEqual`,
//!   `LessOrEqual`): numeric comparison with integer-to-double promotion and
//!   NaN unordered, for every integer key and every float key bit pattern;
//!   a non-numeric field or a symbol key fails.
//! * Slot tests: `EqualSlot` is structural equality for non-string values,
//!   and so is `NotEqualSlot` when the first field is an atom (the
//!   `NotEqualSlot` harness with a `Void`/multifield first field ran out of
//!   memory); each of
//!   the five offset tests other than `NotEqualSlotOffset` compares
//!   `field OP other + offset` with a checked integer sum (overflow fails, as
//!   documented). `NotEqualSlotOffset` is exactly the complement of
//!   `EqualSlotOffset` (`not_equal_slot_offset_is_complement_of_equal`),
//!   which contradicts the documented overflow rule:
//!   `not_equal_slot_offset_overflow_fails_test` FAILS by design and
//!   documents that defect.
//! * `OrderedFieldCount` is exact; entry-type discrimination is exact; `Any`
//!   fails without its own field (its branch semantics are a known gap).
//!
//! Bounds: field values are any atom (symbol, instance name, string of at
//! most two ASCII bytes or one of four fixed UTF-8 texts, any float bit
//! pattern, any integer, external address), `Void`, or a multifield of at
//! most one integer; fact addresses have u8-ranged components; facts have
//! at most three fields.
//!
//! Known gaps: `Any`'s disjunction-of-conjunctions semantics (harnesses for
//! a fixed shape and for the empty cases ran out of memory: CBMC cannot see
//! the nested test vectors' contents, so it explores every nested variant),
//! the whole-fact `Sequence` test (sequence proofs), `AlphaMemory` and its
//! slot indexes (a three-fact insert/index/remove harness did not finish
//! symbolic execution in 10 minutes, even with concrete keys: CBMC does not
//! track the hash tables' heap contents, so every probe loop is unrolled to
//! the bound; the index invariant is proved unboundedly for the model in
//! verification/verus/core_matching.rs), and the whole `AlphaNetwork`:
//! `AlphaNetwork::new` builds a `SparseSecondaryMap` with std `RandomState`,
//! whose seeding calls `getrandom`/`syscall`, which Kani does not support
//! (every harness creating a network fails on those checks), so
//! propagation and the `fact_to_memories` reverse index are proved only in
//! the Verus model; `retain_memories` and `backfill_memory` are not covered.
//! Harness-built values are `mem::forget`-ed instead of dropped (see the cost
//! notes), so drop glue is not checked.

use super::*;
use crate::fact::{OrderedFact, TemplateFact};
use crate::string::FerricString;
use crate::symbol::{InstanceName, SymbolId};
use crate::value::{ExternalAddress, ExternalTypeId, FactAddress, Multifield};

// ---------------------------------------------------------------------------
// Cost notes
//
// * Values and tests built by a harness are `mem::forget`-ed, never dropped:
//   CBMC explores the recursive `Value` drop glue through every enum arm when
//   the discriminant is symbolic, which exhausts memory. Dropping is not a
//   property under test.
// * Where production code itself creates and drops a `Value` (a key turned
//   into a value, an offset sum), the harness case-splits the variant of the
//   input that decides that value's variant, so each copy of the code under
//   test runs with a constant discriminant.
// * Facts are built from fixed-size literals, never by collecting a `Vec`.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Symbolic inputs
// ---------------------------------------------------------------------------

fn any_symbol() -> Symbol {
    if kani::any() {
        Symbol(SymbolId::Ascii(kani::any()))
    } else {
        Symbol(SymbolId::Utf8(kani::any()))
    }
}

fn any_instance_name() -> InstanceName {
    InstanceName::from_symbol(any_symbol())
}

/// A string: either the `Ascii` representation of at most two symbolic ASCII
/// bytes, or the `Utf8` representation of one of a few fixed texts (chosen
/// symbolically; building it from symbolic bytes would unroll the UTF-8
/// validator, which is not under test).
fn any_string() -> FerricString {
    if kani::any() {
        let len: usize = kani::any_where(|len: &usize| *len <= 2);
        let bytes: [u8; 2] = kani::any();
        kani::assume(bytes[0] < 128 && bytes[1] < 128);
        FerricString::Ascii(bytes[..len].into())
    } else {
        const TEXTS: [&str; 4] = ["", "a", "b", "ab"];
        FerricString::Utf8(TEXTS[kani::any_where(|index: &usize| *index < 4)].into())
    }
}

fn any_external() -> ExternalAddress {
    ExternalAddress {
        type_id: ExternalTypeId(kani::any()),
        token: kani::any(),
    }
}

fn any_fact_address() -> FactAddress {
    if kani::any() {
        FactAddress::dummy()
    } else {
        FactAddress::new(
            fact_id(kani::any::<u8>().into()),
            kani::any::<u8>().into(),
            crate::fact::Timestamp::new(kani::any::<u8>().into()),
            kani::any(),
        )
    }
}

/// A symbolic value of any variant except `FactAddress` (its `Arc` makes
/// every harness much larger; fact addresses get a dedicated harness).
/// Multifields hold at most one integer.
fn any_value() -> Value {
    match kani::any::<u8>() {
        0 => Value::Void,
        1 => {
            let mut multifield = Multifield::new();
            if kani::any() {
                multifield.push(Value::Integer(kani::any()));
            }
            Value::Multifield(Box::new(multifield))
        }
        2 => Value::Symbol(any_symbol()),
        3 => Value::InstanceName(any_instance_name()),
        4 => Value::String(any_string()),
        5 => Value::Float(kani::any()),
        6 => Value::ExternalAddress(any_external()),
        _ => Value::Integer(kani::any()),
    }
}

/// A symbolic constant-test key of every kind except `FactAddress`.
fn any_atom_key() -> AtomKey {
    match kani::any::<u8>() {
        0 => AtomKey::Symbol(any_symbol()),
        1 => AtomKey::InstanceName(any_instance_name()),
        2 => AtomKey::String(any_string()),
        3 => AtomKey::FloatBits(kani::any()),
        4 => {
            let external = any_external();
            AtomKey::ExternalAddress {
                type_id: external.type_id,
                token: external.token,
            }
        }
        _ => AtomKey::Integer(kani::any()),
    }
}

/// A symbolic numeric value (`Integer` or `Float`, any bit pattern).
fn any_number() -> Value {
    if kani::any() {
        Value::Integer(kani::any())
    } else {
        Value::Float(kani::any())
    }
}

/// A numeric value or a non-numeric atom, string or `Void`.
fn any_number_or_other() -> Value {
    match kani::any::<u8>() {
        0 => Value::Symbol(any_symbol()),
        1 => Value::Void,
        2 => Value::String(FerricString::Ascii(Box::new([]))),
        _ => any_number(),
    }
}

/// Run `check` on an integer, a float, a symbol and `Void`, each in its own
/// copy so the variant is a constant inside `check`.
fn split_number_or_other(check: &impl Fn(Value)) {
    match kani::any::<u8>() {
        0 => check(Value::Symbol(any_symbol())),
        1 => check(Value::Void),
        2 => check(Value::Float(kani::any())),
        _ => check(Value::Integer(kani::any())),
    }
}

fn fact_id(index: u32) -> FactId {
    FactId::from(slotmap::KeyData::from_ffi(u64::from(index)))
}

fn template_id(index: u32) -> TemplateId {
    TemplateId::from(slotmap::KeyData::from_ffi(u64::from(index)))
}

fn relation(index: u32) -> Symbol {
    Symbol(SymbolId::Ascii(index))
}

fn ordered_fact(fields: SmallVec<[Value; 8]>) -> Fact {
    Fact::Ordered(OrderedFact {
        relation: relation(0),
        fields,
    })
}

fn template_fact(slots: Box<[Value]>) -> Fact {
    Fact::Template(TemplateFact {
        template_id: template_id(0),
        slots,
    })
}

fn ordered1(a: Value) -> Fact {
    ordered_fact(smallvec::smallvec![a])
}

fn ordered2(a: Value, b: Value) -> Fact {
    ordered_fact(smallvec::smallvec![a, b])
}

/// A fact of either kind with `len <= 3` fields, field `i` being `field(i)`.
fn any_kind_fact_len(len: usize, field: impl Fn(usize) -> Value) -> Fact {
    if kani::any() {
        ordered_fact(match len {
            0 => SmallVec::new(),
            1 => smallvec::smallvec![field(0)],
            2 => smallvec::smallvec![field(0), field(1)],
            _ => smallvec::smallvec![field(0), field(1), field(2)],
        })
    } else {
        template_fact(match len {
            0 => Box::new([]),
            1 => Box::new([field(0)]),
            2 => Box::new([field(0), field(1)]),
            _ => Box::new([field(0), field(1), field(2)]),
        })
    }
}

/// A selector of either kind with index 0, 1 or 2.
fn any_slot() -> SlotIndex {
    let index: usize = kani::any_where(|index: &usize| *index <= 2);
    if kani::any() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    }
}

const FIRST: SlotIndex = SlotIndex::Ordered(0);
const SECOND: SlotIndex = SlotIndex::Ordered(1);

fn test_on(slot: SlotIndex, test_type: ConstantTestType) -> ConstantTest {
    ConstantTest { slot, test_type }
}

/// `evaluate_test(fact, test)`, then forget both (see the cost notes).
fn evaluate_and_forget(fact: Fact, test: ConstantTest) -> bool {
    let result = evaluate_test(&fact, &test);
    std::mem::forget(fact);
    std::mem::forget(test);
    result
}

/// Evaluate a test on field 0 of the ordered fact `(r value)`.
fn on_field(value: Value, test_type: ConstantTestType) -> bool {
    evaluate_and_forget(ordered1(value), test_on(FIRST, test_type))
}

/// Evaluate a test on field 0 of `(r first second)`; slot tests read field 1.
fn on_fields(first: Value, second: Value, test_type: ConstantTestType) -> bool {
    evaluate_and_forget(ordered2(first, second), test_on(FIRST, test_type))
}

// ---------------------------------------------------------------------------
// Reference semantics (written from the documented contracts, not the code)
// ---------------------------------------------------------------------------

/// Literal (type-sensitive, CLIPS `eq`) identity between a field and a test
/// key: same type and same content; floats by bit pattern (`AtomKey` doc in
/// value.rs and docs/compatibility.md: "Floats compare by IEEE-754 bit
/// representation, including distinct signed-zero and NaN representations");
/// strings by bytes whatever their representation (string.rs: "Equality is
/// exact byte equality"). `None` when the field is not an atom.
fn oracle_literal_eq(value: &Value, key: &AtomKey) -> Option<bool> {
    Some(match (value, key) {
        (Value::Void | Value::Multifield(_), _) => return None,
        (Value::Integer(a), AtomKey::Integer(b)) => a == b,
        (Value::Float(a), AtomKey::FloatBits(b)) => a.to_bits() == *b,
        (Value::Symbol(a), AtomKey::Symbol(b)) => a == b,
        (Value::InstanceName(a), AtomKey::InstanceName(b)) => a.as_symbol() == b.as_symbol(),
        (Value::String(a), AtomKey::String(b)) => a.as_bytes() == b.as_bytes(),
        (Value::ExternalAddress(a), AtomKey::ExternalAddress { type_id, token }) => {
            a.type_id == *type_id && a.token == *token
        }
        (Value::FactAddress(a), AtomKey::FactAddress(b)) => oracle_same_address(a, b),
        _ => false,
    })
}

/// Fact addresses are the same iff both are the dummy address or both name the
/// same (epoch, fact id, timestamp); the cached public index is ignored
/// (value.rs `FactAddress` doc).
fn oracle_same_address(a: &FactAddress, b: &FactAddress) -> bool {
    a.is_dummy() == b.is_dummy()
        && a.epoch() == b.epoch()
        && a.fact_id() == b.fact_id()
        && a.timestamp() == b.timestamp()
}

/// Structural, type-sensitive equality of whole field values: atoms as
/// literals, multifields element-wise, `Void` equal only to `Void`
/// (value.rs `Value::structural_eq` doc).
fn oracle_structural_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Void, Value::Void) => true,
        (Value::Multifield(a), Value::Multifield(b)) => {
            a.len() == b.len()
                && a.as_slice()
                    .iter()
                    .zip(b.as_slice())
                    .all(|(a, b)| oracle_structural_eq(a, b))
        }
        (Value::Integer(a), Value::Integer(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Symbol(a), Value::Symbol(b)) => a == b,
        (Value::InstanceName(a), Value::InstanceName(b)) => a.as_symbol() == b.as_symbol(),
        (Value::String(a), Value::String(b)) => a.as_bytes() == b.as_bytes(),
        (Value::ExternalAddress(a), Value::ExternalAddress(b)) => {
            a.type_id == b.type_id && a.token == b.token
        }
        (Value::FactAddress(a), Value::FactAddress(b)) => oracle_same_address(a, b),
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmp {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
}

#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

fn oracle_num(value: &Value) -> Option<Num> {
    match value {
        Value::Integer(i) => Some(Num::Int(*i)),
        Value::Float(f) => Some(Num::Float(*f)),
        _ => None,
    }
}

fn oracle_key_num(key: &AtomKey) -> Option<Num> {
    match key {
        AtomKey::Integer(i) => Some(Num::Int(*i)),
        AtomKey::FloatBits(bits) => Some(Num::Float(f64::from_bits(*bits))),
        _ => None,
    }
}

/// Numeric comparison: integers exactly; mixed operands by converting the
/// integer to the nearest double (C/CLIPS promotion); doubles with IEEE
/// operators. NaN is unordered, so every comparison involving NaN fails,
/// including `Ne`, which requires a defined, non-equal ordering.
#[allow(clippy::cast_precision_loss, clippy::float_cmp)]
fn oracle_cmp(op: Cmp, lhs: Num, rhs: Num) -> bool {
    let (a, b) = match (lhs, rhs) {
        (Num::Int(a), Num::Int(b)) => {
            return match op {
                Cmp::Eq => a == b,
                Cmp::Ne => a != b,
                Cmp::Gt => a > b,
                Cmp::Lt => a < b,
                Cmp::Ge => a >= b,
                Cmp::Le => a <= b,
            }
        }
        (Num::Int(a), Num::Float(b)) => (a as f64, b),
        (Num::Float(a), Num::Int(b)) => (a, b as f64),
        (Num::Float(a), Num::Float(b)) => (a, b),
    };
    match op {
        Cmp::Eq => a == b,
        Cmp::Ne => a < b || a > b,
        Cmp::Gt => a > b,
        Cmp::Lt => a < b,
        Cmp::Ge => a >= b,
        Cmp::Le => a <= b,
    }
}

/// `base + offset` per docs/compatibility.md ("a checked integer offset";
/// "An offset that overflows 64-bit integers fails the test instead of
/// wrapping", signalled here by `None`); a float base adds the offset
/// converted to a double. Non-numeric bases have no sum.
#[allow(clippy::cast_precision_loss)]
fn oracle_add_offset(base: &Value, offset: i64) -> Option<Num> {
    match base {
        Value::Integer(i) => {
            let sum = i128::from(*i) + i128::from(offset);
            i64::try_from(sum).ok().map(Num::Int)
        }
        Value::Float(f) => Some(Num::Float(*f + offset as f64)),
        _ => None,
    }
}

/// `lhs OP base + offset`, failing when either side is not a number or the
/// sum is not representable.
fn oracle_offset_cmp(op: Cmp, lhs: &Value, base: &Value, offset: i64) -> bool {
    match (oracle_num(lhs), oracle_add_offset(base, offset)) {
        (Some(lhs), Some(rhs)) => oracle_cmp(op, lhs, rhs),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Constant tests: field selection
// ---------------------------------------------------------------------------

fn any_field_test_type() -> ConstantTestType {
    let key = AtomKey::Integer(kani::any());
    let other = if kani::any() {
        SlotIndex::Ordered(kani::any())
    } else {
        SlotIndex::Template(kani::any())
    };
    let offset: i64 = kani::any();
    match kani::any::<u8>() {
        0 => ConstantTestType::Equal(key),
        1 => ConstantTestType::NotEqual(key),
        2 => ConstantTestType::EqualAny(vec![key]),
        3 => ConstantTestType::GreaterThan(key),
        4 => ConstantTestType::LessThan(key),
        5 => ConstantTestType::GreaterOrEqual(key),
        6 => ConstantTestType::LessOrEqual(key),
        7 => ConstantTestType::EqualSlot(other),
        8 => ConstantTestType::NotEqualSlot(other),
        9 => ConstantTestType::EqualSlotOffset(other, offset),
        10 => ConstantTestType::NotEqualSlotOffset(other, offset),
        11 => ConstantTestType::GreaterThanSlotOffset(other, offset),
        12 => ConstantTestType::LessThanSlotOffset(other, offset),
        13 => ConstantTestType::GreaterOrEqualSlotOffset(other, offset),
        14 => ConstantTestType::LessOrEqualSlotOffset(other, offset),
        _ => ConstantTestType::Any(vec![Vec::new()]),
    }
}

/// Every field test (every variant except the whole-fact `OrderedFieldCount`
/// and `Sequence`), with any parameters, fails when its own selector reads
/// no field. `evaluate_test` reads fields through `get_slot_value`, which
/// `get_slot_value_is_exact` proves returns `None` exactly for a wrong fact
/// kind or an out-of-range index.
#[kani::proof]
#[kani::unwind(2)]
fn field_tests_fail_without_their_field() {
    let test = test_on(any_slot(), any_field_test_type());
    let any = matches!(test.test_type, ConstantTestType::Any(_));
    assert!(!evaluate_field_test(&test, |_| None));
    kani::cover!(any, "Any without its own field");
    kani::cover!(
        matches!(test.test_type, ConstantTestType::NotEqualSlotOffset(..)),
        "NotEqualSlotOffset without its own field"
    );
    std::mem::forget(test);
}

/// Every slot-pair test, with any offset, fails when its own field exists but
/// the other selector reads no field — whatever the own field holds. Each
/// variant is checked in its own copy (see cost notes).
#[kani::proof]
#[kani::unwind(2)]
fn slot_pair_tests_fail_without_the_other_field() {
    let own = Value::Integer(kani::any());
    let reader = |slot: SlotIndex| (slot == FIRST).then_some(&own);
    let check = |test_type: ConstantTestType| {
        let test = test_on(FIRST, test_type);
        assert!(!evaluate_field_test(&test, reader));
        std::mem::forget(test);
    };
    let offset: i64 = kani::any();
    let variant: u8 = kani::any_where(|variant: &u8| *variant < 8);
    match variant {
        0 => check(ConstantTestType::EqualSlot(SECOND)),
        1 => check(ConstantTestType::NotEqualSlot(SECOND)),
        2 => check(ConstantTestType::EqualSlotOffset(SECOND, offset)),
        3 => check(ConstantTestType::NotEqualSlotOffset(SECOND, offset)),
        4 => check(ConstantTestType::GreaterThanSlotOffset(SECOND, offset)),
        5 => check(ConstantTestType::LessThanSlotOffset(SECOND, offset)),
        6 => check(ConstantTestType::GreaterOrEqualSlotOffset(SECOND, offset)),
        _ => check(ConstantTestType::LessOrEqualSlotOffset(SECOND, offset)),
    }
    kani::cover!(variant == 3, "NotEqualSlotOffset still fails");
}

// ---------------------------------------------------------------------------
// Constant tests: value semantics
// ---------------------------------------------------------------------------

/// `Equal(k)` passes iff the field is an atom literally identical to `k`;
/// `NotEqual(k)` iff the field is an atom not identical to `k`. `Void` and
/// multifields pass neither. In particular `-0.0` and `+0.0` differ, a NaN
/// equals its own bit pattern, integer and float twins differ, and symbols,
/// instance names and strings with the same text differ.
#[kani::proof]
#[kani::unwind(3)]
fn literal_key_tests_match_reference() {
    let value = any_value();
    let key = any_atom_key();
    let literal = oracle_literal_eq(&value, &key);
    let is_nan = matches!(value, Value::Float(f) if f.is_nan());
    let is_zero = matches!(value, Value::Float(f) if f == 0.0);
    let key_is_zero = matches!(key, AtomKey::FloatBits(bits) if f64::from_bits(bits) == 0.0);
    let float_vs_integer = matches!(value, Value::Float(_)) && matches!(key, AtomKey::Integer(_));
    let non_atom = matches!(value, Value::Void | Value::Multifield(_));
    let cross_strings = matches!(value, Value::String(FerricString::Ascii(_)))
        && matches!(key, AtomKey::String(FerricString::Utf8(_)));
    let negated: bool = kani::any();
    let actual = if negated {
        on_field(value, ConstantTestType::NotEqual(key))
    } else {
        on_field(value, ConstantTestType::Equal(key))
    };
    assert_eq!(actual, literal == Some(!negated));

    kani::cover!(is_zero && key_is_zero && negated && actual, "-0.0 vs +0.0");
    kani::cover!(is_nan && !negated && actual, "NaN equals its own bits");
    kani::cover!(
        float_vs_integer && negated && actual,
        "float vs integer key"
    );
    kani::cover!(cross_strings && !negated && actual, "Ascii == Utf8 bytes");
    kani::cover!(non_atom && negated && !actual, "non-atom fails NotEqual");
}

/// `EqualAny(ks)` passes iff the field is an atom identical to some key; the
/// empty list never matches.
#[kani::proof]
#[kani::unwind(3)]
fn equal_any_matches_reference() {
    let value = any_value();
    let first = any_atom_key();
    let second = any_atom_key();
    let first_hit = oracle_literal_eq(&value, &first) == Some(true);
    let second_hit = oracle_literal_eq(&value, &second) == Some(true);
    let keys = if kani::any() {
        vec![first, second]
    } else {
        std::mem::forget(second);
        vec![first]
    };
    let two = keys.len() == 2;
    let expected = first_hit || (two && second_hit);
    assert_eq!(on_field(value, ConstantTestType::EqualAny(keys)), expected);
    assert!(!on_field(
        Value::Integer(kani::any()),
        ConstantTestType::EqualAny(Vec::new())
    ));
    kani::cover!(two && !first_hit && second_hit, "matches the second key");
    kani::cover!(first_hit, "matches the first key");
    kani::cover!(!expected, "matches no key");
}

/// Fact-address literals compare by (epoch, fact id, assertion timestamp)
/// and ignore the cached public index; the dummy address equals only itself.
#[kani::proof]
#[kani::unwind(3)]
fn fact_address_literal_tests_match_reference() {
    let field = any_fact_address();
    let key = any_fact_address();
    let expected = oracle_same_address(&field, &key);
    let differs_in_index_only =
        expected && !field.is_dummy() && field.public_index() != key.public_index();
    let negated: bool = kani::any();
    let key = AtomKey::FactAddress(key);
    let value = Value::FactAddress(field);
    let actual = if negated {
        on_field(value, ConstantTestType::NotEqual(key))
    } else {
        on_field(value, ConstantTestType::Equal(key))
    };
    assert_eq!(actual, expected != negated);
    kani::cover!(
        differs_in_index_only && actual && !negated,
        "public index ignored"
    );
    kani::cover!(!expected && !actual && !negated, "different addresses");
}

/// One ordered comparison against a key, checked against the reference:
/// integers compare exactly, mixed operands by promoting the integer to a
/// double, NaN is unordered, and any non-numeric field or key fails.
fn check_numeric_key_test(key: &AtomKey, op: Cmp, test_type: ConstantTestType) {
    let value = any_number_or_other();
    let expected = match (oracle_num(&value), oracle_key_num(key)) {
        (Some(lhs), Some(rhs)) => oracle_cmp(op, lhs, rhs),
        _ => false,
    };
    let mixed = matches!(
        (&value, key),
        (Value::Float(_), AtomKey::Integer(_)) | (Value::Integer(_), AtomKey::FloatBits(_))
    );
    let nan = matches!(value, Value::Float(f) if f.is_nan());
    let negative_zero = matches!(value, Value::Float(f) if f == 0.0 && f.is_sign_negative());
    let actual = on_field(value, test_type);
    assert_eq!(actual, expected);
    kani::cover!(actual, "passes");
    kani::cover!(actual && mixed, "mixed integer/float comparison passes");
    kani::cover!(nan && !actual, "NaN field fails");
    kani::cover!(negative_zero && actual && op == Cmp::Ge, "-0.0 >= key");
}

/// Run `check_numeric_key_test` for each operator, one copy per operator so
/// the test variant (and the key turned into a value) is constant.
fn check_numeric_key_tests(key: AtomKey) {
    match kani::any::<u8>() {
        0 => check_numeric_key_test(&key, Cmp::Gt, ConstantTestType::GreaterThan(key.clone())),
        1 => check_numeric_key_test(&key, Cmp::Lt, ConstantTestType::LessThan(key.clone())),
        2 => check_numeric_key_test(&key, Cmp::Ge, ConstantTestType::GreaterOrEqual(key.clone())),
        _ => check_numeric_key_test(&key, Cmp::Le, ConstantTestType::LessOrEqual(key.clone())),
    }
    std::mem::forget(key);
}

/// `GreaterThan`/`LessThan`/`GreaterOrEqual`/`LessOrEqual` with an integer
/// key compare numerically against integer and float fields (see
/// `check_numeric_key_test`) and fail on non-numeric fields.
#[kani::proof]
#[kani::unwind(1)]
fn numeric_key_tests_match_reference_integer_key() {
    check_numeric_key_tests(AtomKey::Integer(kani::any()));
}

/// As above with a float key (any bit pattern, including NaN and -0.0).
#[kani::proof]
#[kani::unwind(1)]
fn numeric_key_tests_match_reference_float_key() {
    check_numeric_key_tests(AtomKey::FloatBits(kani::any()));
}

/// With a non-numeric key every ordered comparison fails, whatever the field.
#[kani::proof]
#[kani::unwind(1)]
fn numeric_key_tests_fail_for_symbol_keys() {
    let key = AtomKey::Symbol(any_symbol());
    let check = |test_type: ConstantTestType| {
        let value = any_number_or_other();
        let numeric = oracle_num(&value).is_some();
        assert!(!on_field(value, test_type));
        kani::cover!(numeric, "a numeric field is compared");
    };
    match kani::any::<u8>() {
        0 => check(ConstantTestType::GreaterThan(key.clone())),
        1 => check(ConstantTestType::LessThan(key.clone())),
        2 => check(ConstantTestType::GreaterOrEqual(key.clone())),
        _ => check(ConstantTestType::LessOrEqual(key.clone())),
    }
    std::mem::forget(key);
}

/// A scalar atom (no strings: those are covered by the literal harnesses)
/// or, when `atom` is false, `Void` or a multifield of at most one integer.
fn any_slot_value(atom: bool) -> Value {
    if atom {
        match kani::any::<u8>() {
            0 => Value::Symbol(any_symbol()),
            1 => Value::InstanceName(any_instance_name()),
            2 => Value::Float(kani::any()),
            3 => Value::ExternalAddress(any_external()),
            _ => Value::Integer(kani::any()),
        }
    } else if kani::any() {
        Value::Void
    } else {
        let mut multifield = Multifield::new();
        if kani::any() {
            multifield.push(Value::Integer(kani::any()));
        }
        Value::Multifield(Box::new(multifield))
    }
}

/// `EqualSlot`/`NotEqualSlot` compare two fields with structural,
/// type-sensitive equality (floats by bits, multifields element-wise,
/// `Void == Void`). The first field is a scalar atom or a non-atom per
/// harness, the second either (one harness per combination and test: with
/// everything at once CBMC ran out of memory).
fn check_slot_equality(negated: bool, first_atom: bool) {
    let first = any_slot_value(first_atom);
    let second = any_slot_value(kani::any());
    let same = oracle_structural_eq(&first, &second);
    let test_type = if negated {
        ConstantTestType::NotEqualSlot(SECOND)
    } else {
        ConstantTestType::EqualSlot(SECOND)
    };
    let actual = on_fields(first, second, test_type);
    assert_eq!(actual, same != negated);
    kani::cover!(actual, "passes");
    kani::cover!(!actual, "fails");
}

/// `EqualSlot` with a scalar-atom first field.
#[kani::proof]
#[kani::unwind(2)]
fn equal_slot_matches_reference_for_atoms() {
    check_slot_equality(false, true);
}

/// `EqualSlot` with a `Void` or multifield first field.
#[kani::proof]
#[kani::unwind(2)]
fn equal_slot_matches_reference_for_non_atoms() {
    check_slot_equality(false, false);
}

/// `NotEqualSlot` with a scalar-atom first field.
#[kani::proof]
#[kani::unwind(2)]
fn not_equal_slot_matches_reference_for_atoms() {
    check_slot_equality(true, true);
}

/// `field OP other + offset` for one offset-test variant, checked against the
/// reference for an integer, float, symbol or `Void` other field (one copy
/// per kind, see cost notes) and any numeric or non-numeric own field.
fn check_slot_offset_test(op: Cmp, test_type: impl Fn(i64) -> ConstantTestType) {
    split_number_or_other(&|other: Value| {
        let value = any_number_or_other();
        let offset: i64 = kani::any();
        let expected = oracle_offset_cmp(op, &value, &other, offset);
        let overflow = matches!(other, Value::Integer(base) if base.checked_add(offset).is_none());
        let float_base = matches!(other, Value::Float(_));
        let actual = on_fields(value, other, test_type(offset));
        assert_eq!(actual, expected);
        kani::cover!(actual && offset != 0, "passes with a non-zero offset");
        kani::cover!(overflow && !actual, "overflowing sum fails");
        kani::cover!(actual && float_base, "float base passes");
    });
}

/// `EqualSlotOffset` compares `field == other + offset` numerically; an
/// integer sum outside `i64` fails the test, as documented in
/// docs/compatibility.md. (Likewise for the four harnesses below.)
#[kani::proof]
#[kani::unwind(1)]
fn slot_offset_tests_match_reference_equal() {
    check_slot_offset_test(Cmp::Eq, |offset| {
        ConstantTestType::EqualSlotOffset(SECOND, offset)
    });
}

/// `GreaterThanSlotOffset`: `field > other + offset`.
#[kani::proof]
#[kani::unwind(1)]
fn slot_offset_tests_match_reference_greater_than() {
    check_slot_offset_test(Cmp::Gt, |offset| {
        ConstantTestType::GreaterThanSlotOffset(SECOND, offset)
    });
}

/// `LessThanSlotOffset`: `field < other + offset`.
#[kani::proof]
#[kani::unwind(1)]
fn slot_offset_tests_match_reference_less_than() {
    check_slot_offset_test(Cmp::Lt, |offset| {
        ConstantTestType::LessThanSlotOffset(SECOND, offset)
    });
}

/// `GreaterOrEqualSlotOffset`: `field >= other + offset`.
#[kani::proof]
#[kani::unwind(1)]
fn slot_offset_tests_match_reference_greater_or_equal() {
    check_slot_offset_test(Cmp::Ge, |offset| {
        ConstantTestType::GreaterOrEqualSlotOffset(SECOND, offset)
    });
}

/// `LessOrEqualSlotOffset`: `field <= other + offset`.
#[kani::proof]
#[kani::unwind(1)]
fn slot_offset_tests_match_reference_less_or_equal() {
    check_slot_offset_test(Cmp::Le, |offset| {
        ConstantTestType::LessOrEqualSlotOffset(SECOND, offset)
    });
}

/// Documented contract (docs/compatibility.md, "Predicate and return-value
/// constraints"): `neq`/`<>` with an integer offset compare numerically and
/// "An offset that overflows 64-bit integers fails the test instead of
/// wrapping". For integer fields `x` and `y` that means
/// `NotEqualSlotOffset(y, offset)` passes iff `y + offset` is representable
/// and differs from `x`.
///
/// EXPECTED TO FAIL (defect): production evaluates `!EqualSlotOffset`, so an
/// overflowing sum makes the test pass.
#[kani::proof]
#[kani::unwind(1)]
fn not_equal_slot_offset_overflow_fails_test() {
    let x: i64 = kani::any();
    let y: i64 = kani::any();
    let offset: i64 = kani::any();
    let expected = oracle_offset_cmp(Cmp::Ne, &Value::Integer(x), &Value::Integer(y), offset);
    kani::cover!(y.checked_add(offset).is_none(), "overflow reachable");
    kani::cover!(expected, "representable inequality reachable");
    let actual = on_fields(
        Value::Integer(x),
        Value::Integer(y),
        ConstantTestType::NotEqualSlotOffset(SECOND, offset),
    );
    assert_eq!(actual, expected);
}

/// Exact characterization of production `NotEqualSlotOffset`: with both
/// fields present it is the complement of `EqualSlotOffset`, so it passes for
/// non-numeric operands, NaN and overflowing integer sums.
#[kani::proof]
#[kani::unwind(1)]
fn not_equal_slot_offset_is_complement_of_equal() {
    split_number_or_other(&|other: Value| {
        let value = any_number_or_other();
        let offset: i64 = kani::any();
        let equal = oracle_offset_cmp(Cmp::Eq, &value, &other, offset);
        let non_numeric = oracle_num(&value).is_none();
        let actual = on_fields(
            value,
            other,
            ConstantTestType::NotEqualSlotOffset(SECOND, offset),
        );
        assert_eq!(actual, !equal);
        kani::cover!(actual && non_numeric, "passes on a non-numeric field");
        kani::cover!(!actual, "offset-equal fields fail");
    });
}

/// Field-count tests read the whole fact: they pass exactly for ordered facts
/// whose length is within `[min, max]` (no upper bound when `max` is `None`),
/// never for template facts, and never through the field-only evaluator.
#[kani::proof]
#[kani::unwind(3)]
fn ordered_field_count_is_exact() {
    let len: usize = kani::any_where(|len: &usize| *len <= 3);
    let fact = any_kind_fact_len(len, |_| Value::Void);
    let min: usize = kani::any();
    let max: Option<usize> = kani::any();
    let ordered = matches!(fact, Fact::Ordered(_));
    let test = test_on(any_slot(), ConstantTestType::OrderedFieldCount { min, max });
    assert!(!evaluate_field_test(&test, |_| Some(&Value::Void)));
    let expected = ordered && len >= min && max.map_or(true, |max| len <= max);
    assert_eq!(evaluate_and_forget(fact, test), expected);
    kani::cover!(expected && max.is_none() && min > 0, "open-ended count");
    kani::cover!(!expected && ordered, "count rejects an ordered fact");
}

/// `get_slot_value` returns exactly the selected field: the selector kind must
/// match the fact kind and the index must be in range. Any `usize` index is
/// accepted without panicking.
#[kani::proof]
#[kani::unwind(4)]
fn get_slot_value_is_exact() {
    let len: usize = kani::any_where(|len: &usize| *len <= 3);
    let fact = any_kind_fact_len(len, |index| Value::Integer(index as i64));
    let index: usize = kani::any();
    let slot = if kani::any() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    };
    let kinds_match = matches!(
        (&fact, slot),
        (Fact::Ordered(_), SlotIndex::Ordered(_)) | (Fact::Template(_), SlotIndex::Template(_))
    );
    match get_slot_value(&fact, slot) {
        Some(Value::Integer(value)) => {
            assert!(kinds_match && index < len);
            assert_eq!(*value, index as i64);
        }
        Some(_) => panic!("wrong field"),
        None => {
            assert!(!kinds_match || index >= len);
        }
    }
    kani::cover!(kinds_match && len == 3 && index == 2, "last field");
    kani::cover!(kinds_match && index == usize::MAX, "huge index");
    std::mem::forget(fact);
}

/// Entry-type discrimination: a fact matches an entry type iff the entry type
/// names the fact's own relation (ordered) or template (template). Ordered
/// relations and templates never cross-match.
#[kani::proof]
#[kani::unwind(3)]
fn entry_type_discrimination_is_exact() {
    let relation_id: u32 = kani::any_where(|id: &u32| *id <= 1);
    let template: u32 = kani::any_where(|id: &u32| *id <= 1);
    let fact = if kani::any() {
        Fact::Ordered(OrderedFact {
            relation: relation(relation_id),
            fields: SmallVec::new(),
        })
    } else {
        Fact::Template(TemplateFact {
            template_id: template_id(template),
            slots: Box::new([]),
        })
    };
    let probe: u32 = kani::any_where(|id: &u32| *id <= 1);
    let entry = if kani::any() {
        AlphaEntryType::OrderedRelation(relation(probe))
    } else {
        AlphaEntryType::Template(template_id(probe))
    };
    let expected = match (&fact, &entry) {
        (Fact::Ordered(_), AlphaEntryType::OrderedRelation(rel)) => *rel == relation(relation_id),
        (Fact::Template(_), AlphaEntryType::Template(id)) => *id == template_id(template),
        _ => false,
    };
    assert_eq!(fact_matches_entry_type(&fact, &entry), expected);
    std::mem::forget(fact);
    kani::cover!(expected, "match");
    kani::cover!(
        !expected && relation_id == probe && template == probe,
        "same numeric id, different kind"
    );
}

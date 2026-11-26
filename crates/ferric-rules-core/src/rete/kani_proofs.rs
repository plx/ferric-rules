//! Kani proof harnesses for `rete` pure matching functions (see
//! docs/verification.md).
//!
//! What is proved, on the production code, against reference semantics
//! written from the documented contracts (`values_join_eq` doc, string.rs,
//! docs/compatibility.md "Predicate and return-value constraints"):
//!
//! * Index lemma: for any two scalar atoms (integers, float bit patterns,
//!   symbols, instance names, external addresses) and for two fact
//!   addresses, `values_join_eq` is `Some(key_a == key_b)` of their
//!   `AtomKey`s — the `Equal` join test passes exactly for the facts an
//!   `AtomKey` index lookup returns. Its keyless side: a `Void` or a
//!   multifield of at most one integer (which no index holds) is never
//!   join-equal to a scalar atom, in either order.
//! * `evaluate_join`, one test at a time and one copy per test type:
//!   `Equal`/`NotEqual` on scalar atoms (join equality: same type and
//!   content, cross-type values unequal), numeric comparisons
//!   (integer/float promotion, NaN unordered), lexeme
//!   comparisons (strings only, byte order) and the six offset comparisons
//!   (checked integer sum; overflow fails every offset test, including
//!   `NotEqualOffset`, as documented). For the `Equal` test, a missing
//!   field, an unbound or out-of-range variable, or a missing token fails;
//!   an empty list passes (the same guard for the other test types is not
//!   checked: those harnesses ran out of memory).
//! * `indexable_tests`: without a plan, exactly the plain `Equal` tests in
//!   order; with a plan, exactly the `Equal` tests whose logical field has a
//!   physical selector, mapped to it, where the expected selector comes
//!   from an independent reference for `physical_selector` (so the harness
//!   also checks `physical_selector` on these plans).
//! * The capture fast path (`capture_join_matches`) agrees with evaluating
//!   the test on the copied multifield for every numeric, lexeme and offset
//!   join test type (both reject every capture).
//! * Corollary for the two compilations of one predicate (alpha
//!   `*SlotOffset` tests, join `*Offset` tests): the join offset harnesses
//!   here and the alpha slot-offset harnesses match the same reference, so
//!   the forms agree for `=`, `>`, `<`, `>=`, `<=`; for `neq`/`<>` the join
//!   form is "defined and unequal" while the alpha form is the complement of
//!   `EqualSlotOffset` (alpha::kani_proofs), so they disagree exactly when
//!   the comparison is undefined (non-numeric operand, NaN or overflowing
//!   sum) — see the defect report. (Harnesses evaluating both forms at once
//!   ran out of memory.)
//!
//! Bounds: scalar atoms and numbers range over all payloads; strings have
//! at most one byte (or are one of three fixed UTF-8 texts) and appear only
//! in the lexeme harness; multifields and captures hold at most one atom;
//! one join test per `evaluate_join` list (three per unplanned and two per
//! planned `indexable_tests` list); sequence plans of at most two ordered
//! fields or two template segments.
//!
//! Known gaps (each attempted harness ran out of memory or time in CBMC,
//! see the cost notes): `collect_candidate_facts` on a real indexed
//! `AlphaMemory` (16 facts are needed to reach the index path, and even a
//! three-fact indexed memory did not finish symbolic execution in 10
//! minutes; the candidate theorem is proved unboundedly for the model in
//! verification/verus/core_matching.rs, and its ingredients — the index
//! lemma and `indexable_tests` — are proved on production code; the alpha
//! memory's own index is not Kani-checked, see alpha::kani_proofs),
//! `Equal`/`NotEqual` join tests (through `evaluate_join`) and the capture
//! fast path for `Equal`/`NotEqual` when `Void`, multifields or strings are
//! involved, strings in the index lemma (string identity of `AtomKey` is
//! covered by alpha::kani_proofs; harnesses with symbolic strings and with
//! nine concrete string pairs ran out of memory), the index lemma for two
//! multifields (a keyless bound value makes candidate collection scan, so
//! soundness does not depend on it), multi-test lists in `evaluate_join` and
//! `evaluate_pattern` without a plan (the loop is modelled in
//! verification/verus/core_matching.rs), the soundness of sequence
//! physical selectors on real splits (proved unboundedly for the placement
//! model in the Verus file) and `evaluate_pattern`/`split_matches` with a
//! sequence plan against a naive split enumeration (both harnesses ran out
//! of memory in symbolic execution, even with two-field plans and facts).
//! Not attempted: `collect_candidate_parent_tokens` (beta-memory index) and
//! the `sequence_matches` binding extraction. Harness-built values are
//! `mem::forget`-ed instead of dropped, so drop glue is not checked.

use super::*;
use crate::fact::{OrderedFact, TemplateFact, TemplateId};
use crate::sequence::{SequenceField, SequenceSegment, SequenceSource};
use crate::string::FerricString;
use crate::symbol::{InstanceName, Symbol, SymbolId};
use crate::value::{ExternalAddress, ExternalTypeId, Multifield};

// Cost notes (as in alpha::kani_proofs): harness-built values are
// `mem::forget`-ed instead of dropped, and inputs that decide the variant of
// a `Value` that production code creates and drops are case-split.

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

/// `Ascii` with at most one symbolic ASCII byte, or `Utf8` with one of a
/// few fixed texts (see alpha::kani_proofs::any_string); one byte keeps
/// every byte-comparison loop to a single iteration.
fn any_string() -> FerricString {
    if kani::any() {
        let len: usize = kani::any_where(|len: &usize| *len <= 1);
        let bytes: [u8; 1] = [kani::any_where(|byte: &u8| *byte < 128)];
        FerricString::Ascii(bytes[..len].into())
    } else {
        const TEXTS: [&str; 3] = ["", "a", "b"];
        FerricString::Utf8(TEXTS[kani::any_where(|index: &usize| *index < 3)].into())
    }
}

/// A numeric value or a non-numeric atom, string or `Void`.
fn any_number_or_other() -> Value {
    match kani::any::<u8>() {
        0 => Value::Symbol(any_symbol()),
        1 => Value::Void,
        2 => Value::String(FerricString::Ascii(Box::new([]))),
        3 => Value::Float(kani::any()),
        _ => Value::Integer(kani::any()),
    }
}

/// Run `check` once per variant of an integer/float/symbol/`Void` input.
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

fn relation() -> Symbol {
    Symbol(SymbolId::Ascii(0))
}

fn ordered_fact(fields: SmallVec<[Value; 8]>) -> Fact {
    Fact::Ordered(OrderedFact {
        relation: relation(),
        fields,
    })
}

fn template_fact(slots: Box<[Value]>) -> Fact {
    Fact::Template(TemplateFact {
        template_id: TemplateId::from(slotmap::KeyData::from_ffi(0)),
        slots,
    })
}

fn ordered1(a: Value) -> Fact {
    ordered_fact(smallvec::smallvec![a])
}

/// A binding set built directly from its vector (`BindingSet::set` is
/// verified in binding::kani_proofs; going through it here would make CBMC
/// explore the `Value` clone and drop glue in `SmallVec::resize`).
fn bindings_of(values: SmallVec<[Option<ValueRef>; 4]>) -> BindingSet {
    BindingSet { bindings: values }
}

fn token_of(bindings: BindingSet) -> Token {
    Token {
        fact: None,
        bindings,
        parent: None,
        owner_node: NodeId(0),
    }
}

/// A token whose bindings bind `VarId(0)` to `value`.
fn token_binding(value: Value) -> Token {
    token_of(bindings_of(smallvec::smallvec![Some(ValueRef::new(value))]))
}

fn join(alpha_slot: SlotIndex, beta_var: u16, test_type: JoinTestType) -> JoinTest {
    JoinTest {
        alpha_slot,
        beta_var: VarId(beta_var),
        test_type,
    }
}

const FIRST: SlotIndex = SlotIndex::Ordered(0);

/// `evaluate_join` of one test reading field 0 of `(r field)` and `VarId(0)`
/// bound to `bound`. Inputs are forgotten afterwards.
fn join_one(field: Value, bound: Value, test_type: JoinTestType) -> bool {
    let fact = ordered1(field);
    let token = token_binding(bound);
    let result = evaluate_join(&fact, Some(&token), &[join(FIRST, 0, test_type)]);
    std::mem::forget(fact);
    std::mem::forget(token);
    result
}

// ---------------------------------------------------------------------------
// Reference semantics (from documented contracts, not the code)
// ---------------------------------------------------------------------------

/// Join equality (`values_join_eq` doc, docs/compatibility.md): values of the
/// same type compare by content — floats by bit pattern, strings by bytes,
/// multifields element-wise — values of different types are unequal, and
/// `Void` is not comparable (`None`).
fn oracle_join_eq(a: &Value, b: &Value) -> Option<bool> {
    Some(match (a, b) {
        (Value::Void, _) | (_, Value::Void) => return None,
        (Value::Multifield(a), Value::Multifield(b)) => {
            a.len() == b.len()
                && a.as_slice()
                    .iter()
                    .zip(b.as_slice())
                    .all(|(a, b)| oracle_join_eq(a, b) == Some(true))
        }
        (Value::Integer(a), Value::Integer(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Symbol(a), Value::Symbol(b)) => a == b,
        (Value::InstanceName(a), Value::InstanceName(b)) => a.as_symbol() == b.as_symbol(),
        (Value::String(a), Value::String(b)) => a.as_bytes() == b.as_bytes(),
        (Value::ExternalAddress(a), Value::ExternalAddress(b)) => {
            a.type_id == b.type_id && a.token == b.token
        }
        (Value::FactAddress(a), Value::FactAddress(b)) => {
            a.is_dummy() == b.is_dummy()
                && a.epoch() == b.epoch()
                && a.fact_id() == b.fact_id()
                && a.timestamp() == b.timestamp()
        }
        _ => false,
    })
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

impl Cmp {
    fn holds(self, ordering: Ordering) -> bool {
        match self {
            Cmp::Eq => ordering == Ordering::Equal,
            Cmp::Ne => ordering != Ordering::Equal,
            Cmp::Gt => ordering == Ordering::Greater,
            Cmp::Lt => ordering == Ordering::Less,
            Cmp::Ge => ordering != Ordering::Less,
            Cmp::Le => ordering != Ordering::Greater,
        }
    }
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

/// Numeric comparison: integers exactly, mixed operands by promoting the
/// integer to a double, doubles with IEEE operators; NaN is unordered so
/// every comparison with it fails (including `Ne`).
#[allow(clippy::cast_precision_loss, clippy::float_cmp)]
fn oracle_cmp(op: Cmp, lhs: Num, rhs: Num) -> bool {
    let (a, b) = match (lhs, rhs) {
        (Num::Int(a), Num::Int(b)) => return op.holds(a.cmp(&b)),
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

/// Checked integer offset (overflow fails the test, docs/compatibility.md);
/// a float base adds the offset as a double.
#[allow(clippy::cast_precision_loss)]
fn oracle_add_offset(base: &Value, offset: i64) -> Option<Num> {
    match base {
        Value::Integer(i) => i64::try_from(i128::from(*i) + i128::from(offset))
            .ok()
            .map(Num::Int),
        Value::Float(f) => Some(Num::Float(*f + offset as f64)),
        _ => None,
    }
}

fn oracle_offset_cmp(op: Cmp, lhs: &Value, base: &Value, offset: i64) -> bool {
    match (oracle_num(lhs), oracle_add_offset(base, offset)) {
        (Some(lhs), Some(rhs)) => oracle_cmp(op, lhs, rhs),
        _ => false,
    }
}

/// Byte-wise lexicographic order (string.rs: "Ordering is lexicographic by
/// byte value"), written out as a loop.
fn oracle_lex(a: &[u8], b: &[u8]) -> Ordering {
    let mut index = 0;
    while index < a.len() && index < b.len() {
        if a[index] != b[index] {
            return if a[index] < b[index] {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        index += 1;
    }
    if a.len() < b.len() {
        Ordering::Less
    } else if a.len() > b.len() {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// The lowered `str-compare` form matches only when both values are strings
/// (docs/compatibility.md).
fn oracle_lex_cmp(op: Cmp, a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(a), Value::String(b)) => op.holds(oracle_lex(a.as_bytes(), b.as_bytes())),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Join tests
// ---------------------------------------------------------------------------

/// A scalar atom: integer, float (any bits), symbol, instance name or
/// external address (no heap data).
fn any_scalar_atom() -> Value {
    match kani::any::<u8>() {
        0 => Value::Symbol(any_symbol()),
        1 => Value::InstanceName(InstanceName::from_symbol(any_symbol())),
        2 => Value::Float(kani::any()),
        3 => Value::ExternalAddress(ExternalAddress {
            type_id: ExternalTypeId(kani::any()),
            token: kani::any(),
        }),
        _ => Value::Integer(kani::any()),
    }
}

/// Checks the index lemma for one pair: when both values have an
/// `AtomKey`, `values_join_eq` is `Some(key_a == key_b)`; otherwise it is
/// `None` exactly when a `Void` is involved, and a multifield is involved
/// in every other keyless case. It also agrees with the reference.
fn check_index_lemma(a: &Value, b: &Value) -> Option<bool> {
    let result = values_join_eq(a, b);
    let key_a = AtomKey::from_value(a);
    let key_b = AtomKey::from_value(b);
    match (&key_a, &key_b) {
        (Some(key_a), Some(key_b)) => assert_eq!(result, Some(key_a == key_b)),
        _ => {
            let void = matches!(a, Value::Void) || matches!(b, Value::Void);
            assert_eq!(result.is_none(), void);
            assert!(void || matches!(a, Value::Multifield(_)) || matches!(b, Value::Multifield(_)));
        }
    }
    assert_eq!(result, oracle_join_eq(a, b));
    std::mem::forget((key_a, key_b));
    result
}

/// Index soundness and completeness lemma, for scalar atoms: join
/// equality holds exactly when the `AtomKey`s are equal — so an alpha-memory
/// index keyed by `AtomKey` returns a fact iff the `Equal` join test on that
/// slot passes. Integer/float twins, signed zeros, NaN payloads, symbols vs
/// instance names are all distinguished.
#[kani::proof]
#[kani::unwind(2)]
fn join_equality_is_atom_key_identity() {
    let a = any_scalar_atom();
    let b = any_scalar_atom();
    let result = check_index_lemma(&a, &b);
    kani::cover!(result == Some(true), "equal atoms");
    kani::cover!(
        matches!((&a, &b), (Value::Integer(_), Value::Float(_))),
        "integer vs float"
    );
    kani::cover!(
        matches!((&a, &b), (Value::Float(x), Value::Float(y)) if x == y && result == Some(false)),
        "numerically equal floats with different bits"
    );
    std::mem::forget((a, b));
}

/// Run `check` on a scalar atom of each variant (see `any_scalar_atom`), one
/// copy per variant so the discriminant is a constant inside `check`.
fn split_scalar_atom(check: &impl Fn(Value)) {
    match kani::any::<u8>() {
        0 => check(Value::Symbol(any_symbol())),
        1 => check(Value::InstanceName(InstanceName::from_symbol(any_symbol()))),
        2 => check(Value::Float(kani::any())),
        3 => check(Value::ExternalAddress(ExternalAddress {
            type_id: ExternalTypeId(kani::any()),
            token: kani::any(),
        })),
        _ => check(Value::Integer(kani::any())),
    }
}

/// `keyless` has no `AtomKey` (so no index holds a fact with that field
/// value), and join equality with any scalar atom, in either order, is the
/// documented `expected` (`None` for `Void`, `Some(false)` across types).
fn check_keyless_against_atoms(keyless: &Value, expected: Option<bool>) {
    assert!(AtomKey::from_value(keyless).is_none());
    split_scalar_atom(&|atom: Value| {
        assert_eq!(values_join_eq(keyless, &atom), expected);
        assert_eq!(values_join_eq(&atom, keyless), expected);
        std::mem::forget(atom);
    });
}

/// The keyless side of the index lemma: a keyless value (`Void` or a
/// multifield of at most one integer, which an `AtomKey` index never holds)
/// is never join-equal to a scalar atom, in either order, so a fact whose
/// field is keyless is rightly left out of an index lookup for a keyed
/// bound value. `Void` is incomparable (`None`), a multifield against an
/// atom is `Some(false)` (`values_join_eq` doc). One copy per keyless kind
/// and atom variant (a version with symbolic variants ran out of memory).
#[kani::proof]
#[kani::unwind(2)]
fn join_equality_keyless_against_atom_is_never_true() {
    let void: bool = kani::any();
    if void {
        check_keyless_against_atoms(&Value::Void, None);
    } else {
        let mut multifield = Multifield::new();
        let one: bool = kani::any();
        if one {
            multifield.push(Value::Integer(kani::any()));
        }
        let keyless = Value::Multifield(Box::new(multifield));
        check_keyless_against_atoms(&keyless, Some(false));
        kani::cover!(one, "a one-value multifield");
        std::mem::forget(keyless);
    }
    kani::cover!(void, "Void");
}

/// `Equal` passes iff the field and the binding are join-equal scalar atoms
/// (same type and content, floats by bits); `NotEqual` iff they are unequal
/// (including integer/float twins and symbol/instance-name twins). Scalar
/// atoms only: with `Void`, multifields or strings on either side CBMC ran
/// out of memory.
fn check_join_equality(test_type: JoinTestType, negated: bool) {
    let field = any_scalar_atom();
    let bound = any_scalar_atom();
    let expected = oracle_join_eq(&field, &bound);
    let actual = join_one(field, bound, test_type);
    assert_eq!(actual, expected == Some(!negated));
    kani::cover!(actual, "passes");
    kani::cover!(!actual, "fails");
}

/// `Equal` join test (see `check_join_equality`).
#[kani::proof]
#[kani::unwind(2)]
fn join_equal_test_matches_reference() {
    check_join_equality(JoinTestType::Equal, false);
}

/// `NotEqual` join test (see `check_join_equality`).
#[kani::proof]
#[kani::unwind(2)]
fn join_not_equal_test_matches_reference() {
    check_join_equality(JoinTestType::NotEqual, true);
}

/// Ordered numeric join tests compare numerically (integer/float mixing by
/// promotion, NaN unordered) and fail on any non-numeric operand. One copy
/// per test type, so the type is a constant (see the cost notes).
#[kani::proof]
#[kani::unwind(2)]
fn join_numeric_tests_match_reference() {
    let check = |op: Cmp, test_type: JoinTestType| {
        let field = any_number_or_other();
        let bound = any_number_or_other();
        let expected = match (oracle_num(&field), oracle_num(&bound)) {
            (Some(lhs), Some(rhs)) => oracle_cmp(op, lhs, rhs),
            _ => false,
        };
        let mixed = matches!(
            (&field, &bound),
            (Value::Integer(_), Value::Float(_)) | (Value::Float(_), Value::Integer(_))
        );
        let actual = join_one(field, bound, test_type);
        assert_eq!(actual, expected);
        kani::cover!(actual && mixed, "mixed comparison passes");
        kani::cover!(!actual && mixed, "mixed comparison fails");
    };
    match kani::any::<u8>() {
        0 => check(Cmp::Gt, JoinTestType::GreaterThan),
        1 => check(Cmp::Lt, JoinTestType::LessThan),
        2 => check(Cmp::Ge, JoinTestType::GreaterOrEqual),
        _ => check(Cmp::Le, JoinTestType::LessOrEqual),
    }
}

/// Lexeme join tests (`str-compare` lowering) compare strings byte-wise and
/// fail unless both operands are strings. One copy per test type.
#[kani::proof]
#[kani::unwind(2)]
fn join_lexeme_tests_match_reference() {
    let check = |op: Cmp, test_type: JoinTestType| {
        let operand = || {
            if kani::any() {
                Value::String(any_string())
            } else if kani::any() {
                Value::Symbol(any_symbol())
            } else {
                Value::Integer(kani::any())
            }
        };
        let field = operand();
        let bound = operand();
        let expected = oracle_lex_cmp(op, &field, &bound);
        let strings = matches!((&field, &bound), (Value::String(_), Value::String(_)));
        let actual = join_one(field, bound, test_type);
        assert_eq!(actual, expected);
        kani::cover!(actual, "strings compare");
        kani::cover!(!strings && !actual, "non-string fails");
    };
    match kani::any::<u8>() {
        0 => check(Cmp::Eq, JoinTestType::LexEqual),
        1 => check(Cmp::Ne, JoinTestType::LexNotEqual),
        2 => check(Cmp::Gt, JoinTestType::LexGreaterThan),
        3 => check(Cmp::Lt, JoinTestType::LexLessThan),
        4 => check(Cmp::Ge, JoinTestType::LexGreaterOrEqual),
        _ => check(Cmp::Le, JoinTestType::LexLessOrEqual),
    }
}

/// Offset join tests compare `field OP bound + offset` numerically; an
/// integer sum outside `i64` fails every offset test, including
/// `NotEqualOffset` (docs/compatibility.md: "An offset that overflows 64-bit
/// integers fails the test instead of wrapping"). One copy per kind of bound
/// value (production turns it into a `Value` and drops it).
fn check_join_offset(op: Cmp, test_type: impl Fn(i64) -> JoinTestType) {
    split_number_or_other(&|bound: Value| {
        let field = any_number_or_other();
        let offset: i64 = kani::any();
        let expected = oracle_offset_cmp(op, &field, &bound, offset);
        let overflow = matches!(bound, Value::Integer(base) if base.checked_add(offset).is_none());
        let actual = join_one(field, bound, test_type(offset));
        assert_eq!(actual, expected);
        kani::cover!(overflow && !actual, "overflowing sum fails");
        kani::cover!(actual && offset != 0, "passes with a non-zero offset");
    });
}

/// `EqualOffset` (see `check_join_offset`).
#[kani::proof]
#[kani::unwind(2)]
fn join_equal_offset_matches_reference() {
    check_join_offset(Cmp::Eq, JoinTestType::EqualOffset);
}

/// `NotEqualOffset` (see `check_join_offset`): defined and unequal.
#[kani::proof]
#[kani::unwind(2)]
fn join_not_equal_offset_matches_reference() {
    check_join_offset(Cmp::Ne, JoinTestType::NotEqualOffset);
}

/// `GreaterThanOffset` (see `check_join_offset`).
#[kani::proof]
#[kani::unwind(2)]
fn join_greater_than_offset_matches_reference() {
    check_join_offset(Cmp::Gt, JoinTestType::GreaterThanOffset);
}

/// `LessThanOffset` (see `check_join_offset`).
#[kani::proof]
#[kani::unwind(2)]
fn join_less_than_offset_matches_reference() {
    check_join_offset(Cmp::Lt, JoinTestType::LessThanOffset);
}

/// `GreaterOrEqualOffset` (see `check_join_offset`).
#[kani::proof]
#[kani::unwind(2)]
fn join_greater_or_equal_offset_matches_reference() {
    check_join_offset(Cmp::Ge, JoinTestType::GreaterOrEqualOffset);
}

/// `LessOrEqualOffset` (see `check_join_offset`).
#[kani::proof]
#[kani::unwind(2)]
fn join_less_or_equal_offset_matches_reference() {
    check_join_offset(Cmp::Le, JoinTestType::LessOrEqualOffset);
}

/// An `Equal` join test fails when its fact field is missing (wrong kind or
/// out of range) or its variable is unbound (never set, or beyond the
/// binding vector), and every non-empty test list fails without a token. An
/// empty test list always passes. Only `Equal` is checked: one copy per test
/// type, even six types per harness with concrete operands, ran out of
/// memory in the solver.
#[kani::proof]
#[kani::unwind(2)]
fn join_tests_require_field_and_binding() {
    let ordered: bool = kani::any();
    let fact = if ordered {
        ordered1(Value::Integer(1))
    } else {
        template_fact(Box::new([Value::Integer(1)]))
    };
    // Variable 0, variable 1 or neither bound (a vector of width 0, 1 or 2).
    let choice: u8 = kani::any_where(|choice: &u8| *choice < 3);
    let bound_var: Option<u16> = [None, Some(0), Some(1)][usize::from(choice)];
    let token = token_of(bindings_of(match choice {
        0 => SmallVec::new(),
        1 => smallvec::smallvec![Some(ValueRef::new(Value::Integer(1)))],
        _ => smallvec::smallvec![None, Some(ValueRef::new(Value::Integer(1)))],
    }));
    let index: usize = kani::any_where(|index: &usize| *index <= 1);
    let slot = if kani::any() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    };
    let var: u16 = kani::any();
    let test = join(slot, var, JoinTestType::Equal);
    let field_exists = index == 0 && (ordered == matches!(slot, SlotIndex::Ordered(_)));
    let var_bound = bound_var == Some(var);
    let actual = evaluate_join(&fact, Some(&token), std::slice::from_ref(&test));
    assert_eq!(actual, field_exists && var_bound);
    assert!(!evaluate_join(&fact, None, std::slice::from_ref(&test)));
    assert!(evaluate_join(&fact, None, &[]));
    assert!(evaluate_join(&fact, Some(&token), &[]));
    kani::cover!(actual, "passes");
    kani::cover!(
        field_exists && !var_bound && var > 1,
        "variable beyond the vector"
    );
    std::mem::forget(fact);
    std::mem::forget(token);
}

// ---------------------------------------------------------------------------
// Index selection
// ---------------------------------------------------------------------------

fn any_join_test_type() -> JoinTestType {
    let offset: i64 = kani::any();
    match kani::any::<u8>() {
        0 => JoinTestType::Equal,
        1 => JoinTestType::NotEqual,
        2 => JoinTestType::GreaterThan,
        3 => JoinTestType::LessThan,
        4 => JoinTestType::GreaterOrEqual,
        5 => JoinTestType::LessOrEqual,
        6 => JoinTestType::LexEqual,
        7 => JoinTestType::LexNotEqual,
        8 => JoinTestType::LexGreaterThan,
        9 => JoinTestType::LexLessThan,
        10 => JoinTestType::LexGreaterOrEqual,
        11 => JoinTestType::LexLessOrEqual,
        12 => JoinTestType::EqualOffset(offset),
        13 => JoinTestType::NotEqualOffset(offset),
        14 => JoinTestType::GreaterThanOffset(offset),
        15 => JoinTestType::LessThanOffset(offset),
        16 => JoinTestType::GreaterOrEqualOffset(offset),
        _ => JoinTestType::LessOrEqualOffset(offset),
    }
}

fn any_join_test() -> JoinTest {
    let index: usize = kani::any();
    let slot = if kani::any() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    };
    join(slot, kani::any(), any_join_test_type())
}

/// Without a sequence plan, `indexable_tests` yields exactly the plain
/// `Equal` tests, in order, as (field selector, variable) pairs. Every other
/// test kind — including `EqualOffset` and `LexEqual`, whose matches are not
/// determined by `AtomKey` identity — is excluded.
#[kani::proof]
#[kani::unwind(5)]
fn indexable_tests_selects_exactly_plain_equality() {
    let tests = [any_join_test(), any_join_test(), any_join_test()];
    let mut actual = indexable_tests(&tests, None);
    let mut selected = 0;
    for test in &tests {
        if matches!(test.test_type, JoinTestType::Equal) {
            assert_eq!(actual.next(), Some((test.alpha_slot, test.beta_var)));
            selected += 1;
        }
    }
    assert_eq!(actual.next(), None);
    kani::cover!(selected == 2, "two of three selected");
    kani::cover!(selected == 0, "none selected");
}

// ---------------------------------------------------------------------------
// Sequence plans
// ---------------------------------------------------------------------------

fn any_sequence_field() -> SequenceField {
    if kani::any() {
        SequenceField::Single
    } else {
        SequenceField::Multi
    }
}

/// A single-segment ordered plan with at most two logical fields.
fn any_ordered_sequence() -> SequencePattern {
    let width: usize = kani::any_where(|width: &usize| *width <= 2);
    let fields = match width {
        0 => Vec::new(),
        1 => vec![any_sequence_field()],
        _ => vec![any_sequence_field(), any_sequence_field()],
    };
    SequencePattern {
        segments: vec![SequenceSegment {
            source: SequenceSource::Ordered,
            fields,
        }],
        tests: Vec::new(),
    }
}

/// A two-segment template plan over physical slots 0 and 1, each segment a
/// scalar slot (one single field) or a multislot (one or two fields).
fn any_template_sequence() -> SequencePattern {
    let segment = |physical: usize| {
        if kani::any() {
            SequenceSegment {
                source: SequenceSource::TemplateScalar(physical),
                fields: vec![SequenceField::Single],
            }
        } else if kani::any() {
            SequenceSegment {
                source: SequenceSource::TemplateSlot(physical),
                fields: vec![any_sequence_field()],
            }
        } else {
            SequenceSegment {
                source: SequenceSource::TemplateSlot(physical),
                fields: vec![any_sequence_field(), any_sequence_field()],
            }
        }
    };
    let first = segment(0);
    let second = segment(1);
    SequencePattern {
        segments: if kani::any() {
            vec![first, second]
        } else {
            vec![second, first]
        },
        tests: Vec::new(),
    }
}

/// Reference for `SequencePattern::physical_selector`, written from its
/// documented contract (sequence.rs: "an ordered single field before the
/// first capture, or a scalar template slot"): an ordered logical field
/// `i` of the single ordered segment keeps its selector iff fields `0..=i`
/// are all single; a template logical field maps to `Template(p)` iff it
/// is projected by a `TemplateScalar(p)` segment (fields numbered across
/// the segments in written order); anything else, including a selector of
/// the other kind, has no physical selector.
fn oracle_physical_selector(sequence: &SequencePattern, slot: SlotIndex) -> Option<SlotIndex> {
    let ordered_plan =
        sequence.segments.len() == 1 && sequence.segments[0].source == SequenceSource::Ordered;
    match slot {
        SlotIndex::Ordered(index) => {
            if !ordered_plan {
                return None;
            }
            let fields = &sequence.segments[0].fields;
            let mut position = 0;
            while position <= index {
                if position >= fields.len() || fields[position] != SequenceField::Single {
                    return None;
                }
                position += 1;
            }
            Some(slot)
        }
        SlotIndex::Template(index) => {
            let mut first = 0;
            for segment in &sequence.segments {
                let end = first + segment.fields.len();
                if first <= index && index < end {
                    return match segment.source {
                        SequenceSource::TemplateScalar(physical) => {
                            Some(SlotIndex::Template(physical))
                        }
                        _ => None,
                    };
                }
                first = end;
            }
            None
        }
    }
}

/// With a sequence plan, `indexable_tests` yields, in order, exactly the plain
/// `Equal` tests whose logical field has a physical selector (per the
/// independent `oracle_physical_selector`), paired with that physical
/// selector. Selectors of the wrong kind for the plan are never indexed.
#[kani::proof]
#[kani::unwind(5)]
fn indexable_tests_maps_sequence_fields_to_physical_selectors() {
    let ordered: bool = kani::any();
    let sequence = if ordered {
        any_ordered_sequence()
    } else {
        any_template_sequence()
    };
    let test = || {
        let index: usize = kani::any_where(|index: &usize| *index <= 3);
        let slot = if ordered == kani::any::<bool>() {
            SlotIndex::Ordered(index)
        } else {
            SlotIndex::Template(index)
        };
        let test_type = if kani::any() {
            JoinTestType::Equal
        } else {
            JoinTestType::NotEqual
        };
        join(slot, kani::any(), test_type)
    };
    let tests = [test(), test()];
    let mut actual = indexable_tests(&tests, Some(&sequence));
    let mut remapped = false;
    let mut skipped = false;
    let mut kept_ordered = false;
    let mut wrong_kind = false;
    for test in &tests {
        if test.test_type == JoinTestType::Equal {
            match oracle_physical_selector(&sequence, test.alpha_slot) {
                Some(physical) => {
                    assert_eq!(actual.next(), Some((physical, test.beta_var)));
                    remapped |= physical != test.alpha_slot;
                    kept_ordered |= ordered && test.alpha_slot != SlotIndex::Ordered(0);
                }
                None => {
                    skipped = true;
                    wrong_kind |= ordered != matches!(test.alpha_slot, SlotIndex::Ordered(_));
                }
            }
        }
    }
    assert_eq!(actual.next(), None);
    drop(actual);
    kani::cover!(
        !ordered && remapped,
        "template field remapped to its physical slot"
    );
    kani::cover!(kept_ordered, "a later ordered single field is indexed");
    kani::cover!(
        skipped && !wrong_kind,
        "an equality on a capture or shifted field is not indexed"
    );
    kani::cover!(wrong_kind, "a selector of the other kind is not indexed");
    std::mem::forget(sequence);
}

/// Build an atom from a variant tag and a payload (so the same atom can be
/// built twice without cloning a symbolic `Value`).
fn atom_from(tag: u8, payload: i8) -> Value {
    match tag {
        0 => Value::Symbol(Symbol(SymbolId::Ascii(payload as u32))),
        1 => Value::Float(f64::from(payload)),
        _ => Value::Integer(payload.into()),
    }
}

/// A capture of at most one atom (`atom_from` tag and payload) and the
/// same values copied into a multifield, as `SplitView::get` would copy it.
fn any_capture() -> ([Value; 2], usize, Value) {
    let len: usize = kani::any_where(|len: &usize| *len <= 1);
    let tags: [u8; 2] = [kani::any(), kani::any()];
    let payloads: [i8; 2] = [kani::any(), kani::any()];
    let capture = [
        atom_from(tags[0], payloads[0]),
        atom_from(tags[1], payloads[1]),
    ];
    let mut copied = Multifield::new();
    if len >= 1 {
        copied.push(atom_from(tags[0], payloads[0]));
    }
    if len == 2 {
        copied.push(atom_from(tags[1], payloads[1]));
    }
    (capture, len, Value::Multifield(Box::new(copied)))
}

/// `capture_join_matches` (the in-place fast path) against
/// `evaluate_join_fields` on the copied multifield, for one test type and
/// one bound value.
fn check_capture(capture: &[Value], copied: &Value, test_type: JoinTestType, bound: Value) -> bool {
    let fast = capture_join_matches(capture, &bound, test_type);
    let bindings = bindings_of(smallvec::smallvec![Some(ValueRef::new(bound))]);
    let slow = evaluate_join_fields(
        |_| Some(copied),
        Some(&bindings),
        &[join(FIRST, 0, test_type)],
    );
    assert_eq!(fast, slow);
    std::mem::forget(bindings);
    fast
}

/// A non-equality join test never admits a capture, on either path.
fn check_capture_rejected(test_type: JoinTestType) {
    let (capture, len, copied) = any_capture();
    let bound = Value::Integer(kani::any::<i8>().into());
    assert!(!check_capture(&capture[..len], &copied, test_type, bound));
    kani::cover!(len == 1, "one-value capture");
    std::mem::forget((capture, copied));
}

/// Numeric join tests never admit a capture on either path (one copy per
/// test type).
#[kani::proof]
#[kani::unwind(2)]
fn capture_fast_path_rejects_numeric_tests() {
    match kani::any::<u8>() {
        0 => check_capture_rejected(JoinTestType::GreaterThan),
        1 => check_capture_rejected(JoinTestType::LessThan),
        2 => check_capture_rejected(JoinTestType::GreaterOrEqual),
        _ => check_capture_rejected(JoinTestType::LessOrEqual),
    }
}

/// Lexeme join tests never admit a capture on either path.
#[kani::proof]
#[kani::unwind(2)]
fn capture_fast_path_rejects_lexeme_tests() {
    match kani::any::<u8>() {
        0 => check_capture_rejected(JoinTestType::LexEqual),
        1 => check_capture_rejected(JoinTestType::LexNotEqual),
        2 => check_capture_rejected(JoinTestType::LexGreaterThan),
        3 => check_capture_rejected(JoinTestType::LexLessThan),
        4 => check_capture_rejected(JoinTestType::LexGreaterOrEqual),
        _ => check_capture_rejected(JoinTestType::LexLessOrEqual),
    }
}

/// `EqualOffset`, `NotEqualOffset` and `GreaterThanOffset` never admit a
/// capture on either path (three per harness: six at once ran out of
/// memory).
#[kani::proof]
#[kani::unwind(2)]
fn capture_fast_path_rejects_offset_tests() {
    let offset: i64 = kani::any::<i8>().into();
    match kani::any::<u8>() {
        0 => check_capture_rejected(JoinTestType::EqualOffset(offset)),
        1 => check_capture_rejected(JoinTestType::NotEqualOffset(offset)),
        _ => check_capture_rejected(JoinTestType::GreaterThanOffset(offset)),
    }
}

/// `LessThanOffset`, `GreaterOrEqualOffset` and `LessOrEqualOffset` never
/// admit a capture on either path.
#[kani::proof]
#[kani::unwind(2)]
fn capture_fast_path_rejects_more_offset_tests() {
    let offset: i64 = kani::any::<i8>().into();
    match kani::any::<u8>() {
        0 => check_capture_rejected(JoinTestType::LessThanOffset(offset)),
        1 => check_capture_rejected(JoinTestType::GreaterOrEqualOffset(offset)),
        _ => check_capture_rejected(JoinTestType::LessOrEqualOffset(offset)),
    }
}

fn any_fact_address() -> crate::value::FactAddress {
    if kani::any() {
        crate::value::FactAddress::dummy()
    } else {
        crate::value::FactAddress::new(
            fact_id(kani::any::<u8>().into()),
            kani::any::<u8>().into(),
            Timestamp::new(kani::any::<u8>().into()),
            kani::any(),
        )
    }
}

/// The index lemma for fact addresses (kept apart from
/// `join_equality_is_atom_key_identity` because the `Arc` inside makes the
/// general harness much larger): join equality of two fact addresses is
/// identity of their `AtomKey`s, which ignores the cached public index.
#[kani::proof]
#[kani::unwind(2)]
fn join_equality_of_fact_addresses_is_atom_key_identity() {
    let a = Value::FactAddress(any_fact_address());
    let b = Value::FactAddress(any_fact_address());
    let key_a = AtomKey::from_value(&a).unwrap();
    let key_b = AtomKey::from_value(&b).unwrap();
    let result = values_join_eq(&a, &b);
    assert_eq!(result, Some(key_a == key_b));
    assert_eq!(result, oracle_join_eq(&a, &b));
    let index_differs = match (&a, &b) {
        (Value::FactAddress(x), Value::FactAddress(y)) => x.public_index() != y.public_index(),
        _ => false,
    };
    kani::cover!(
        result == Some(true) && index_differs,
        "public index ignored"
    );
    kani::cover!(result == Some(false), "different addresses");
    std::mem::forget((a, b, key_a, key_b));
}

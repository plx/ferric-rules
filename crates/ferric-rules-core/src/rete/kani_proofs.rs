//! Kani proof harnesses for `rete` (see docs/verification.md).
//!
//! Placeholder header; rewritten once the harness set is final.

use super::*;
use crate::alpha::{evaluate_test, ConstantTest, ConstantTestType};
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

/// `Ascii` with at most two symbolic ASCII bytes, or `Utf8` with one of a
/// few fixed texts (see alpha::kani_proofs::any_string).
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

/// Any variant except `FactAddress`; multifields hold at most one integer.
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
        3 => Value::InstanceName(InstanceName::from_symbol(any_symbol())),
        4 => Value::String(any_string()),
        5 => Value::Float(kani::any()),
        6 => Value::ExternalAddress(ExternalAddress {
            type_id: ExternalTypeId(kani::any()),
            token: kani::any(),
        }),
        _ => Value::Integer(kani::any()),
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

/// A token whose bindings bind `VarId(0)` to `value`.
fn token_binding(value: Value) -> Token {
    let mut bindings = BindingSet::new();
    bindings.set(VarId(0), ValueRef::new(value));
    Token {
        fact: None,
        bindings,
        parent: None,
        owner_node: NodeId(0),
    }
}

fn join(alpha_slot: SlotIndex, beta_var: u16, test_type: JoinTestType) -> JoinTest {
    JoinTest {
        alpha_slot,
        beta_var: VarId(beta_var),
        test_type,
    }
}

const FIRST: SlotIndex = SlotIndex::Ordered(0);
const SECOND: SlotIndex = SlotIndex::Ordered(1);

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

/// Index soundness and completeness lemma. For any two values that both
/// convert to an `AtomKey` (every atom), join equality holds exactly when the
/// keys are equal — so an alpha-memory index keyed by `AtomKey` returns a
/// fact iff the `Equal` join test on that slot passes. Non-atoms have no key
/// (candidate collection must then scan): `Void` is incomparable, and a
/// multifield equals only an element-wise equal multifield.
#[kani::proof]
#[kani::unwind(3)]
fn join_equality_is_atom_key_identity() {
    let a = any_value();
    let b = any_value();
    let result = values_join_eq(&a, &b);
    let key_a = AtomKey::from_value(&a);
    let key_b = AtomKey::from_value(&b);
    match (&key_a, &key_b) {
        (Some(key_a), Some(key_b)) => assert_eq!(result, Some(key_a == key_b)),
        _ => {
            let void = matches!(a, Value::Void) || matches!(b, Value::Void);
            assert_eq!(result.is_none(), void);
            assert!(void || matches!(a, Value::Multifield(_)) || matches!(b, Value::Multifield(_)));
        }
    }
    assert_eq!(result, oracle_join_eq(&a, &b));
    kani::cover!(result == Some(true) && key_a.is_some(), "equal atoms");
    kani::cover!(
        result == Some(true) && matches!(a, Value::Multifield(_)),
        "equal multifields"
    );
    kani::cover!(
        matches!((&a, &b), (Value::Integer(_), Value::Float(_))) && result == Some(false),
        "integer and float twins are unequal"
    );
    std::mem::forget((a, b, key_a, key_b));
}

/// `Equal`/`NotEqual` join tests follow join equality: `Equal` passes iff the
/// values are comparable and equal, `NotEqual` iff comparable and unequal;
/// a `Void` field or binding fails both.
#[kani::proof]
#[kani::unwind(3)]
fn join_equality_tests_match_reference() {
    let field = any_value();
    let bound = any_value();
    let expected = oracle_join_eq(&field, &bound);
    let negated: bool = kani::any();
    let void = matches!(field, Value::Void) || matches!(bound, Value::Void);
    let test_type = if negated {
        JoinTestType::NotEqual
    } else {
        JoinTestType::Equal
    };
    let actual = join_one(field, bound, test_type);
    assert_eq!(actual, expected == Some(!negated));
    kani::cover!(actual && !negated, "Equal passes");
    kani::cover!(actual && negated, "NotEqual passes");
    kani::cover!(void && negated && !actual, "Void fails NotEqual");
}

/// Ordered numeric join tests compare numerically (integer/float mixing by
/// promotion, NaN unordered) and fail on any non-numeric operand.
#[kani::proof]
#[kani::unwind(3)]
fn join_numeric_tests_match_reference() {
    let field = any_number_or_other();
    let bound = any_number_or_other();
    let (op, test_type) = match kani::any::<u8>() {
        0 => (Cmp::Gt, JoinTestType::GreaterThan),
        1 => (Cmp::Lt, JoinTestType::LessThan),
        2 => (Cmp::Ge, JoinTestType::GreaterOrEqual),
        _ => (Cmp::Le, JoinTestType::LessOrEqual),
    };
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
}

/// Lexeme join tests (`str-compare` lowering) compare strings byte-wise and
/// fail unless both operands are strings.
#[kani::proof]
#[kani::unwind(4)]
fn join_lexeme_tests_match_reference() {
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
    let (op, test_type) = match kani::any::<u8>() {
        0 => (Cmp::Eq, JoinTestType::LexEqual),
        1 => (Cmp::Ne, JoinTestType::LexNotEqual),
        2 => (Cmp::Gt, JoinTestType::LexGreaterThan),
        3 => (Cmp::Lt, JoinTestType::LexLessThan),
        4 => (Cmp::Ge, JoinTestType::LexGreaterOrEqual),
        _ => (Cmp::Le, JoinTestType::LexLessOrEqual),
    };
    let expected = oracle_lex_cmp(op, &field, &bound);
    let strings = matches!((&field, &bound), (Value::String(_), Value::String(_)));
    let actual = join_one(field, bound, test_type);
    assert_eq!(actual, expected);
    kani::cover!(actual && op == Cmp::Lt, "string less-than passes");
    kani::cover!(actual && op == Cmp::Ne, "string inequality passes");
    kani::cover!(
        !strings && op == Cmp::Ne && !actual,
        "non-string fails LexNotEqual"
    );
}

/// Offset join tests compare `field OP bound + offset` numerically; an
/// integer sum outside `i64` fails every offset test, including
/// `NotEqualOffset` (docs/compatibility.md: "An offset that overflows 64-bit
/// integers fails the test instead of wrapping").
#[kani::proof]
#[kani::unwind(2)]
fn join_offset_tests_match_reference() {
    split_number_or_other(&|bound: Value| {
        let field = any_number_or_other();
        let offset: i64 = kani::any();
        let (op, test_type) = match kani::any::<u8>() {
            0 => (Cmp::Eq, JoinTestType::EqualOffset(offset)),
            1 => (Cmp::Ne, JoinTestType::NotEqualOffset(offset)),
            2 => (Cmp::Gt, JoinTestType::GreaterThanOffset(offset)),
            3 => (Cmp::Lt, JoinTestType::LessThanOffset(offset)),
            4 => (Cmp::Ge, JoinTestType::GreaterOrEqualOffset(offset)),
            _ => (Cmp::Le, JoinTestType::LessOrEqualOffset(offset)),
        };
        let expected = oracle_offset_cmp(op, &field, &bound, offset);
        let overflow = matches!(bound, Value::Integer(base) if base.checked_add(offset).is_none());
        let actual = join_one(field, bound, test_type);
        assert_eq!(actual, expected);
        kani::cover!(
            overflow && op == Cmp::Ne && !actual,
            "overflow fails NotEqualOffset"
        );
        kani::cover!(actual && op == Cmp::Ne, "NotEqualOffset passes");
        kani::cover!(actual && op == Cmp::Eq && offset != 0, "EqualOffset passes");
    });
}

/// A join test fails when its fact field is missing (wrong kind or out of
/// range) or its variable is unbound (never set, or beyond the binding
/// vector), and every non-empty test list fails without a token. An empty
/// test list always passes.
#[kani::proof]
#[kani::unwind(4)]
fn join_tests_require_field_and_binding() {
    let ordered: bool = kani::any();
    let fact = if ordered {
        ordered1(Value::Integer(1))
    } else {
        template_fact(Box::new([Value::Integer(1)]))
    };
    let mut bindings = BindingSet::new();
    let bound_var: u16 = kani::any_where(|var: &u16| *var <= 1);
    if kani::any() {
        bindings.set(VarId(bound_var), ValueRef::new(Value::Integer(1)));
    }
    let token = Token {
        fact: None,
        bindings,
        parent: None,
        owner_node: NodeId(0),
    };
    let index: usize = kani::any_where(|index: &usize| *index <= 1);
    let slot = if kani::any() {
        SlotIndex::Ordered(index)
    } else {
        SlotIndex::Template(index)
    };
    let var: u16 = kani::any();
    let test = join(slot, var, JoinTestType::Equal);
    let field_exists = index == 0 && (ordered == matches!(slot, SlotIndex::Ordered(_)));
    let var_bound = token.bindings.get(VarId(var)).is_some();
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

fn any_cheap_join_test() -> JoinTest {
    let slot = SlotIndex::Ordered(kani::any_where(|index: &usize| *index <= 2));
    let var: u16 = kani::any_where(|var: &u16| *var <= 2);
    let test_type = match kani::any::<u8>() {
        0 => JoinTestType::Equal,
        1 => JoinTestType::NotEqual,
        2 => JoinTestType::GreaterThan,
        3 => JoinTestType::LessOrEqual,
        _ => JoinTestType::LexEqual,
    };
    join(slot, var, test_type)
}

/// A token binding variables 0 and 1 (each optionally) to small integers.
fn any_integer_token() -> Token {
    let mut bindings = BindingSet::new();
    for var in 0..2_u16 {
        if kani::any() {
            bindings.set(
                VarId(var),
                ValueRef::new(Value::Integer(kani::any::<i8>().into())),
            );
        }
    }
    Token {
        fact: None,
        bindings,
        parent: None,
        owner_node: NodeId(0),
    }
}

/// `evaluate_join` is the conjunction of its tests: a list passes iff every
/// single test passes on its own, in either order. Integer fields and
/// bindings.
#[kani::proof]
#[kani::unwind(4)]
fn evaluate_join_is_conjunction() {
    let fact = ordered_fact(smallvec::smallvec![
        Value::Integer(kani::any::<i8>().into()),
        Value::Integer(kani::any::<i8>().into()),
    ]);
    let token = any_integer_token();
    let tests = [any_cheap_join_test(), any_cheap_join_test()];
    let first = evaluate_join(&fact, Some(&token), &tests[..1]);
    let second = evaluate_join(&fact, Some(&token), &tests[1..]);
    assert_eq!(evaluate_join(&fact, Some(&token), &tests), first && second);
    let swapped = [tests[1].clone(), tests[0].clone()];
    assert_eq!(
        evaluate_join(&fact, Some(&token), &swapped),
        first && second
    );
    kani::cover!(first && second, "both pass");
    kani::cover!(first != second, "exactly one fails");
    std::mem::forget(fact);
    std::mem::forget(token);
}

/// Without a sequence plan, `evaluate_pattern` is `evaluate_join` with the
/// token, for every test list.
#[kani::proof]
#[kani::unwind(4)]
fn evaluate_pattern_without_sequence_is_evaluate_join() {
    let fact = ordered_fact(smallvec::smallvec![
        Value::Integer(kani::any::<i8>().into()),
        Value::Integer(kani::any::<i8>().into()),
    ]);
    let token = any_integer_token();
    let count: usize = kani::any_where(|count: &usize| *count <= 2);
    let tests = [any_cheap_join_test(), any_cheap_join_test()];
    let tests = &tests[..count];
    let pattern = evaluate_pattern(&fact, &token, tests, None);
    assert_eq!(pattern, evaluate_join(&fact, Some(&token), tests));
    kani::cover!(pattern && count == 2, "two tests match");
    kani::cover!(!pattern, "no match");
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
    let count: usize = kani::any_where(|count: &usize| *count <= 3);
    let tests = &tests[..count];
    let mut expected: SmallVec<[(SlotIndex, VarId); 3]> = SmallVec::new();
    for test in tests {
        if matches!(test.test_type, JoinTestType::Equal) {
            expected.push((test.alpha_slot, test.beta_var));
        }
    }
    let actual: SmallVec<[(SlotIndex, VarId); 3]> = indexable_tests(tests, None).collect();
    assert_eq!(actual, expected);
    kani::cover!(actual.len() == 2 && count == 3, "two of three selected");
    kani::cover!(actual.is_empty() && count == 3, "none selected");
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

/// A single-segment ordered plan with at most three logical fields.
fn any_ordered_sequence() -> SequencePattern {
    let width: usize = kani::any_where(|width: &usize| *width <= 3);
    let fields = match width {
        0 => Vec::new(),
        1 => vec![any_sequence_field()],
        2 => vec![any_sequence_field(), any_sequence_field()],
        _ => vec![
            any_sequence_field(),
            any_sequence_field(),
            any_sequence_field(),
        ],
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

/// An ordered fact `(r 0 1 .. len-1)` with `len <= 3`.
fn ordered_fact_len(len: usize) -> Fact {
    ordered_fact(match len {
        0 => SmallVec::new(),
        1 => smallvec::smallvec![Value::Integer(0)],
        2 => smallvec::smallvec![Value::Integer(0), Value::Integer(1)],
        _ => smallvec::smallvec![Value::Integer(0), Value::Integer(1), Value::Integer(2)],
    })
}

/// A template fact whose slots 0 and 1 each hold an integer or a multifield
/// of at most two integers.
fn template_fact_any_shape() -> Fact {
    let slot = |base: i64| {
        if kani::any() {
            Value::Integer(base)
        } else {
            let mut values = Multifield::new();
            let len: usize = kani::any_where(|len: &usize| *len <= 2);
            if len >= 1 {
                values.push(Value::Integer(base + 1));
            }
            if len == 2 {
                values.push(Value::Integer(base + 2));
            }
            Value::Multifield(Box::new(values))
        }
    };
    template_fact(Box::new([slot(10), slot(20)]))
}

/// Soundness of sequence index keys: whenever `physical_selector` maps a
/// logical field to a physical selector, the logical field of *every* split
/// the search produces is the very value (same address) the physical
/// selector reads from the fact. Hence an index keyed by the physical field
/// contains every fact having a split whose logical field passes an `Equal`
/// join test (with `join_equality_is_atom_key_identity`). Precondition: the
/// plan passes `validate()`, which the compiler enforces before installing a
/// plan (compiler.rs `validate_entry` call) and which sequence.rs documents
/// as the check "before installing or restoring a pattern".
#[kani::proof]
#[kani::unwind(5)]
fn physical_selectors_read_the_logical_field_of_every_split() {
    let ordered: bool = kani::any();
    let (sequence, fact) = if ordered {
        let len: usize = kani::any_where(|len: &usize| *len <= 3);
        (any_ordered_sequence(), ordered_fact_len(len))
    } else {
        (any_template_sequence(), template_fact_any_shape())
    };
    kani::assume(sequence.validate().is_ok());
    let width = sequence.logical_width();
    let mut splits = 0_usize;
    let mut checked = 0_usize;
    let _ = sequence.search(&fact, &mut |event| {
        if let SplitEvent::Match(split) = event {
            splits += 1;
            for logical in 0..width {
                let slot = if ordered {
                    SlotIndex::Ordered(logical)
                } else {
                    SlotIndex::Template(logical)
                };
                if let Some(physical) = sequence.physical_selector(slot) {
                    let via_split = split.get(slot).unwrap();
                    let via_fact = get_slot_value(&fact, physical).unwrap();
                    assert!(std::ptr::eq(via_split, via_fact));
                    checked += 1;
                }
            }
        }
        ControlFlow::<()>::Continue(())
    });
    kani::cover!(
        ordered && checked >= 1 && splits >= 2,
        "ordered prefix, several splits"
    );
    kani::cover!(
        !ordered && checked >= 1 && splits >= 2,
        "template scalar, several splits"
    );
    std::mem::forget(fact);
    std::mem::forget(sequence);
}

/// With a sequence plan, `indexable_tests` yields, in order, exactly the plain
/// `Equal` tests whose logical field has a physical selector, paired with
/// that physical selector.
#[kani::proof]
#[kani::unwind(5)]
fn indexable_tests_maps_sequence_fields_to_physical_selectors() {
    let ordered: bool = kani::any();
    let sequence = if ordered {
        any_ordered_sequence()
    } else {
        any_template_sequence()
    };
    kani::assume(sequence.validate().is_ok());
    let test = || {
        let index: usize = kani::any_where(|index: &usize| *index <= 3);
        let slot = if ordered {
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
    let mut expected: SmallVec<[(SlotIndex, VarId); 2]> = SmallVec::new();
    for test in &tests {
        if test.test_type == JoinTestType::Equal {
            if let Some(physical) = sequence.physical_selector(test.alpha_slot) {
                expected.push((physical, test.beta_var));
            }
        }
    }
    let actual: SmallVec<[(SlotIndex, VarId); 2]> =
        indexable_tests(&tests, Some(&sequence)).collect();
    assert_eq!(actual, expected);
    kani::cover!(
        !ordered
            && actual
                .iter()
                .any(|(physical, _)| *physical != tests[0].alpha_slot),
        "template field remapped to its physical slot"
    );
    kani::cover!(
        actual.len()
            < tests
                .iter()
                .filter(|t| t.test_type == JoinTestType::Equal)
                .count(),
        "an equality on a capture or shifted field is not indexed"
    );
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

/// The capture fast path of sequence joins (`capture_join_matches`, which
/// compares a borrowed capture in place) agrees with evaluating the same join
/// test on the copied multifield value through `evaluate_join_fields`, for
/// every join test type, every capture of at most two atoms and every kind of
/// bound value (atom, `Void`, multifield of at most two atoms).
#[kani::proof]
#[kani::unwind(4)]
fn capture_fast_path_equals_copied_multifield() {
    let len: usize = kani::any_where(|len: &usize| *len <= 2);
    let tags: [u8; 2] = [kani::any(), kani::any()];
    let payloads: [i8; 2] = [kani::any(), kani::any()];
    let capture: [Value; 2] = [
        atom_from(tags[0], payloads[0]),
        atom_from(tags[1], payloads[1]),
    ];
    let mut copied = Multifield::new();
    for index in 0..len {
        copied.push(atom_from(tags[index], payloads[index]));
    }
    let copied = Value::Multifield(Box::new(copied));
    let test_type = any_join_test_type();
    let check = |bound: Value| {
        let fast = capture_join_matches(&capture[..len], &bound, test_type);
        let mut bindings = BindingSet::new();
        bindings.set(VarId(0), ValueRef::new(bound));
        let slow = evaluate_join_fields(
            |_| Some(&copied),
            Some(&bindings),
            &[join(FIRST, 0, test_type)],
        );
        assert_eq!(fast, slow);
        kani::cover!(fast && test_type == JoinTestType::Equal, "equal capture");
        kani::cover!(
            fast && test_type == JoinTestType::NotEqual,
            "unequal capture"
        );
        std::mem::forget(bindings);
    };
    match kani::any::<u8>() {
        0 => check(Value::Void),
        1 => check(Value::Integer(kani::any::<i8>().into())),
        2 => check(Value::Float(kani::any())),
        3 => check(Value::Symbol(any_symbol())),
        _ => {
            let bound_len: usize = kani::any_where(|len: &usize| *len <= 2);
            let mut bound = Multifield::new();
            for _ in 0..bound_len {
                bound.push(atom_from(kani::any(), kani::any()));
            }
            check(Value::Multifield(Box::new(bound)));
        }
    }
    std::mem::forget(capture);
    std::mem::forget(copied);
}

/// The logical fields of the split of the ordered fact `values` that gives
/// the plan's captures the lengths `lengths` (written independently of the
/// production search: walk the plan, take one value per single field and
/// the next `length` values per capture). `None` when the lengths do not
/// consume the fact exactly.
fn naive_projection(
    fields: &[SequenceField],
    values: &[i64],
    lengths: &[usize],
) -> Option<Vec<Value>> {
    let mut logical = Vec::new();
    let mut position = 0;
    let mut capture = 0;
    for field in fields {
        match field {
            SequenceField::Single => {
                if position >= values.len() {
                    std::mem::forget(logical);
                    return None;
                }
                logical.push(Value::Integer(values[position]));
                position += 1;
            }
            SequenceField::Multi => {
                let length = lengths[capture];
                capture += 1;
                if position + length > values.len() {
                    std::mem::forget(logical);
                    return None;
                }
                let mut multifield = Multifield::new();
                for value in &values[position..position + length] {
                    multifield.push(Value::Integer(*value));
                }
                logical.push(Value::Multifield(Box::new(multifield)));
                position += length;
            }
        }
    }
    if position == values.len() {
        Some(logical)
    } else {
        std::mem::forget(logical);
        None
    }
}

/// Whether the naive split passes the plan's constant tests and the join
/// tests, both read through its logical fields.
fn naive_split_matches(
    logical: &[Value],
    plan_tests: &[ConstantTest],
    bindings: &BindingSet,
    tests: &[JoinTest],
) -> bool {
    let field = |slot: SlotIndex| match slot {
        SlotIndex::Ordered(index) => logical.get(index),
        SlotIndex::Template(_) => None,
    };
    plan_tests
        .iter()
        .all(|test| crate::alpha::evaluate_field_test(test, field))
        && evaluate_join_fields(field, Some(bindings), tests)
}

/// With an ordered sequence plan, `evaluate_pattern` (a depth-first split
/// search that prunes on partially placed prefixes and compares captures in
/// place) agrees with the naive semantics: enumerate every assignment of
/// capture lengths, project each split, and accept iff some split passes
/// all plan constant tests and all join tests. `split_matches` on the
/// production projection of each enumerated split agrees with the naive
/// verdict for that split.
#[kani::proof]
#[kani::unwind(5)]
fn evaluate_pattern_with_sequence_matches_naive_enumeration() {
    let mut sequence = any_ordered_sequence();
    let fields = sequence.segments[0].fields.clone();
    let len: usize = kani::any_where(|len: &usize| *len <= 3);
    let raw: [i64; 3] = [
        kani::any::<bool>().into(),
        kani::any::<bool>().into(),
        kani::any::<bool>().into(),
    ];
    let values = &raw[..len];
    let fact = ordered_fact(match len {
        0 => SmallVec::new(),
        1 => smallvec::smallvec![Value::Integer(raw[0])],
        2 => smallvec::smallvec![Value::Integer(raw[0]), Value::Integer(raw[1])],
        _ => smallvec::smallvec![
            Value::Integer(raw[0]),
            Value::Integer(raw[1]),
            Value::Integer(raw[2])
        ],
    });
    if kani::any() {
        let index: usize = kani::any_where(|index: &usize| *index <= 2);
        let key: i64 = kani::any::<bool>().into();
        sequence.tests.push(ConstantTest {
            slot: SlotIndex::Ordered(index),
            test_type: ConstantTestType::Equal(AtomKey::Integer(key)),
        });
    }
    let join_index: usize = kani::any_where(|index: &usize| *index <= 2);
    let join_type = match kani::any::<u8>() {
        0 => JoinTestType::Equal,
        1 => JoinTestType::NotEqual,
        _ => JoinTestType::GreaterThan,
    };
    let tests = [join(SlotIndex::Ordered(join_index), 0, join_type)];
    let token = if kani::any() {
        token_binding(Value::Integer(kani::any::<bool>().into()))
    } else {
        let mut bound = Multifield::new();
        let bound_len: usize = kani::any_where(|len: &usize| *len <= 2);
        for _ in 0..bound_len {
            bound.push(Value::Integer(kani::any::<bool>().into()));
        }
        token_binding(Value::Multifield(Box::new(bound)))
    };

    let captures = fields
        .iter()
        .filter(|field| **field == SequenceField::Multi)
        .count();
    let mut naive = false;
    let mut naive_splits = 0;
    for first in 0..=len {
        for second in 0..=len {
            for third in 0..=len {
                let lengths = [first, second, third];
                // Enumerate each assignment once: unused capture lengths are 0.
                if lengths[captures..].iter().any(|length| *length != 0) {
                    continue;
                }
                let lengths = &lengths[..captures];
                let Some(logical) = naive_projection(&fields, values, lengths) else {
                    continue;
                };
                naive_splits += 1;
                let passes =
                    naive_split_matches(&logical, &sequence.tests, &token.bindings, &tests);
                naive |= passes;
                let split = sequence.project(&fact, lengths).unwrap();
                assert_eq!(
                    split_matches(&split, &token.bindings, &tests, &sequence),
                    passes
                );
                std::mem::forget(split);
                std::mem::forget(logical);
            }
        }
    }
    let actual = evaluate_pattern(&fact, &token, &tests, Some(&sequence));
    assert_eq!(actual, naive);
    kani::cover!(actual && naive_splits >= 2, "a match among several splits");
    kani::cover!(!actual && naive_splits >= 2, "no split matches");
    kani::cover!(actual && captures == 2, "two captures");
    std::mem::forget(fact);
    std::mem::forget(token);
    std::mem::forget(sequence);
}

// ---------------------------------------------------------------------------
// Alpha slot-offset tests versus join offset tests
// ---------------------------------------------------------------------------

/// The same comparison is compiled to an alpha `*SlotOffset` test when both
/// variables come from one pattern and to a join `*Offset` test otherwise
/// (ferric-rules-runtime loader.rs `to_slot_offset_test` /
/// `to_join_test_with_offset`). For `=`, `>`, `<`, `>=`, `<=` both forms give
/// the same verdict on every pair of values.
#[kani::proof]
#[kani::unwind(2)]
fn alpha_and_join_offset_tests_agree_except_not_equal() {
    split_number_or_other(&|other: Value| {
        let field = any_number_or_other();
        let offset: i64 = kani::any();
        let (alpha, joined) = match kani::any::<u8>() {
            0 => (
                ConstantTestType::EqualSlotOffset(SECOND, offset),
                JoinTestType::EqualOffset(offset),
            ),
            1 => (
                ConstantTestType::GreaterThanSlotOffset(SECOND, offset),
                JoinTestType::GreaterThanOffset(offset),
            ),
            2 => (
                ConstantTestType::LessThanSlotOffset(SECOND, offset),
                JoinTestType::LessThanOffset(offset),
            ),
            3 => (
                ConstantTestType::GreaterOrEqualSlotOffset(SECOND, offset),
                JoinTestType::GreaterOrEqualOffset(offset),
            ),
            _ => (
                ConstantTestType::LessOrEqualSlotOffset(SECOND, offset),
                JoinTestType::LessOrEqualOffset(offset),
            ),
        };
        let fact = ordered_fact(smallvec::smallvec![field, other.clone()]);
        let token = token_binding(other);
        let alpha_verdict = evaluate_test(
            &fact,
            &ConstantTest {
                slot: FIRST,
                test_type: alpha,
            },
        );
        let join_verdict = evaluate_join(&fact, Some(&token), &[join(FIRST, 0, joined)]);
        assert_eq!(alpha_verdict, join_verdict);
        kani::cover!(alpha_verdict, "both pass");
        std::mem::forget(fact);
        std::mem::forget(token);
    });
}

/// Exact characterization of the `neq`/`<>` divergence between the two
/// compiled forms: the alpha `NotEqualSlotOffset` passes iff the join
/// `NotEqualOffset` passes or the numeric comparison is undefined (a
/// non-numeric operand, a NaN, or an overflowing integer sum). See the
/// defect report: the join form follows docs/compatibility.md.
#[kani::proof]
#[kani::unwind(2)]
fn alpha_not_equal_offset_diverges_exactly_when_comparison_undefined() {
    split_number_or_other(&|other: Value| {
        let field = any_number_or_other();
        let offset: i64 = kani::any();
        let undefined = match (oracle_num(&field), oracle_add_offset(&other, offset)) {
            (Some(lhs), Some(rhs)) => {
                !(oracle_cmp(Cmp::Eq, lhs, rhs) || oracle_cmp(Cmp::Ne, lhs, rhs))
            }
            _ => true,
        };
        let fact = ordered_fact(smallvec::smallvec![field, other.clone()]);
        let token = token_binding(other);
        let alpha_verdict = evaluate_test(
            &fact,
            &ConstantTest {
                slot: FIRST,
                test_type: ConstantTestType::NotEqualSlotOffset(SECOND, offset),
            },
        );
        let join_verdict = evaluate_join(
            &fact,
            Some(&token),
            &[join(FIRST, 0, JoinTestType::NotEqualOffset(offset))],
        );
        assert_eq!(alpha_verdict, join_verdict || undefined);
        kani::cover!(alpha_verdict && !join_verdict, "the forms disagree");
        kani::cover!(alpha_verdict && join_verdict, "both pass");
        std::mem::forget(fact);
        std::mem::forget(token);
    });
}

// ---------------------------------------------------------------------------
// Indexed candidate collection
// ---------------------------------------------------------------------------

/// Number of facts in the candidate-collection harness: one more than
/// `INDEX_SCAN_THRESHOLD`, so the index path is enabled.
const MEMORY_FACTS: usize = INDEX_SCAN_THRESHOLD + 1;

/// Slot-0 value of fact `i` in the candidate-collection harness: integer and
/// float keys that collide numerically (`1` and `1.0`), both signed zeros,
/// a symbol, and non-atoms (`Void`, an empty multifield) that are never
/// indexed.
fn memory_field(i: usize) -> Value {
    match i % 8 {
        0 => Value::Integer(0),
        1 => Value::Integer(1),
        2 => Value::Float(1.0),
        3 => Value::Float(0.0),
        4 => Value::Float(-0.0),
        5 => Value::Symbol(Symbol(SymbolId::Ascii(1))),
        6 => Value::Void,
        _ => Value::Multifield(Box::new(Multifield::new())),
    }
}

/// The real `collect_candidate_facts` on a real indexed `AlphaMemory` above
/// the scan threshold, followed by the residual `evaluate_join` filter that
/// every caller applies, keeps exactly the facts (in the same order) that
/// filtering every fact of the memory keeps. The parent binding of the
/// indexed variable is any integer (symbolic: the hash lookup key is
/// symbolic), a float of either sign of zero, `1.0`, a symbol, `Void` or a
/// multifield; the second join test has a symbolic type and bound value, and
/// the two tests come in either order (so the index applies to the first
/// `Equal` test, whichever it is). Bounds: 17 facts with fixed slot-0
/// values (`memory_field`) and slot-1 values `i % 3`.
#[kani::proof]
#[kani::unwind(19)]
fn indexed_candidates_then_join_tests_equal_full_scan() {
    let facts: [Fact; MEMORY_FACTS] = std::array::from_fn(|i| {
        ordered_fact(smallvec::smallvec![
            memory_field(i),
            Value::Integer((i % 3) as i64)
        ])
    });
    let mut memory = AlphaMemory::new(crate::alpha::AlphaMemoryId(0));
    memory.request_index_empty(FIRST);
    for (i, fact) in facts.iter().enumerate() {
        memory.insert(fact_id(i as u32), fact);
    }
    let fact_of = |id: FactId| &facts[(slotmap::Key::data(&id).as_ffi() & 0xffff_ffff) as usize];

    let check = |bound: Value| {
        let mut bindings = BindingSet::new();
        bindings.set(VarId(0), ValueRef::new(bound));
        bindings.set(
            VarId(1),
            ValueRef::new(Value::Integer(kani::any::<i8>().into())),
        );
        let token = Token {
            fact: None,
            bindings,
            parent: None,
            owner_node: NodeId(0),
        };
        let residual = join(
            SECOND,
            1,
            match kani::any::<u8>() {
                0 => JoinTestType::Equal,
                1 => JoinTestType::NotEqual,
                2 => JoinTestType::GreaterThan,
                _ => JoinTestType::LessOrEqual,
            },
        );
        let indexed = join(FIRST, 0, JoinTestType::Equal);
        let tests = if kani::any() {
            [indexed, residual]
        } else {
            [residual, indexed]
        };
        let candidates =
            collect_candidate_facts(&memory, indexable_tests(&tests, None), &token.bindings);
        let narrowed = candidates.len() < memory.len();
        let kept: SmallVec<[FactId; 8]> = candidates
            .into_iter()
            .filter(|id| evaluate_join(fact_of(*id), Some(&token), &tests))
            .collect();
        let scanned: SmallVec<[FactId; 8]> = memory
            .iter()
            .filter(|id| evaluate_join(fact_of(*id), Some(&token), &tests))
            .collect();
        assert_eq!(kept, scanned);
        kani::cover!(narrowed && !kept.is_empty(), "index path keeps matches");
        kani::cover!(!narrowed, "scan path");
        std::mem::forget(token);
    };
    match kani::any::<u8>() {
        0 => check(Value::Float(0.0)),
        1 => check(Value::Float(-0.0)),
        2 => check(Value::Float(1.0)),
        3 => check(Value::Symbol(Symbol(SymbolId::Ascii(1)))),
        4 => check(Value::Void),
        5 => check(Value::Multifield(Box::new(Multifield::new()))),
        _ => check(Value::Integer(kani::any::<i8>().into())),
    }
    std::mem::forget(facts);
    std::mem::forget(memory);
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

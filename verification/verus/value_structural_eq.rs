//! Verus model of `Value::structural_eq` and `Multifield` equality
//! (crates/ferric-rules-core/src/value.rs).
//!
//! # What is proved (unbounded: multifields of any length and any nesting
//! depth, any payloads)
//!
//! * `theorem_structural_eq_is_canonical_equality`: `structural_eq(a, b)`
//!   holds exactly when `canon(a) == canon(b)`, where `canon` erases only
//!   the public index of every fact address (at any depth). So structural
//!   equality is mathematical equality of values except for that
//!   presentation metadata: floats by bit pattern, strings by bytes,
//!   symbols by (pool, index), kinds never mix (INTEGER vs FLOAT, SYMBOL vs
//!   INSTANCE-NAME), multifields elementwise.
//! * Corollaries: `structural_eq` is reflexive, symmetric and transitive
//!   (`lemma_structural_eq_equivalence`), also on every NaN bit pattern.
//! * `theorem_multifield_eq_elementwise`: multifield equality holds iff the
//!   lengths are equal and the elements are pairwise structurally equal.
//!
//! # Model and trusted base
//!
//! * `Value` (value.rs:279) is modeled by the datatype below. `Float(f64)`
//!   is modeled by its bit pattern, which is all `structural_eq` inspects
//!   (`a.to_bits() == b.to_bits()`); `String(FerricString)` by its bytes
//!   (FerricString equality is byte equality, string.rs:82, checked in
//!   string/kani_proofs.rs and ferric_string_order.rs); `FactId` by its
//!   64-bit key (`slotmap::KeyData` equality); `Box<Multifield>` /
//!   `SmallVec<[Value; 8]>` by `Seq<Value>`.
//! * `Multifield::eq` uses `iter().zip(..).all(..)`; the model
//!   `all_pairs_eq` walks both sequences in lock step and stops at the end
//!   of the shorter one, like `zip`, short-circuiting like `all`.
//! * Trusted: the standard library's `Iterator::zip`/`all`, slice length,
//!   and derived `PartialEq` for `SymbolId`/`ExternalAddress` behave as
//!   modeled.
//! * No `assume`, `admit`, `external_body` or trusted glue in this file.
//!
//! # Known gaps
//!
//! * Not mechanically linked to the Rust source. The Kani harnesses in
//!   crates/ferric-rules-core/src/value/kani_proofs.rs check the atom cases
//!   on the production code for all payloads (strings up to 2 bytes) and
//!   one-element multifields; longer and nested multifields were
//!   intractable there and are covered only by this model.

use vstd::prelude::*;

verus! {

/// Models `SymbolId` (crates/ferric-rules-core/src/symbol.rs:44).
pub enum SymbolId {
    Ascii(u32),
    Utf8(u32),
}

/// Models `FactAddressData` (crates/ferric-rules-core/src/value.rs:61).
pub struct FactAddressData {
    pub fact_id: u64,
    pub epoch: u64,
    pub timestamp: u64,
    pub public_index: u64,
}

/// Models `Value` (crates/ferric-rules-core/src/value.rs:279), variant for
/// variant. `FactAddress(None)` is the dummy address.
pub enum Value {
    Symbol(SymbolId),
    String(Seq<u8>),
    Integer(i64),
    Float(u64),
    Multifield(Seq<Value>),
    ExternalAddress(u32, u64),
    Void,
    InstanceName(SymbolId),
    FactAddress(Option<FactAddressData>),
}

/// Models `<FactAddress as PartialEq>::eq` (value.rs:114 fn eq).
pub open spec fn fact_address_eq(a: Option<FactAddressData>, b: Option<FactAddressData>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a.epoch, a.fact_id, a.timestamp) == (b.epoch, b.fact_id, b.timestamp),
        _ => false,
    }
}

/// Models `Value::structural_eq` (value.rs:310 fn structural_eq), arm for
/// arm in the production order.
pub open spec fn structural_eq(a: Value, b: Value) -> bool
    decreases a, 2nat,
{
    match (a, b) {
        (Value::Symbol(x), Value::Symbol(y)) => x == y,
        (Value::InstanceName(x), Value::InstanceName(y)) => x == y,
        (Value::FactAddress(x), Value::FactAddress(y)) => fact_address_eq(x, y),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Integer(x), Value::Integer(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Multifield(x), Value::Multifield(y)) => multifield_eq(x, y),
        (Value::ExternalAddress(t1, k1), Value::ExternalAddress(t2, k2)) => t1 == t2 && k1 == k2,
        (Value::Void, Value::Void) => true,
        _ => false,
    }
}

/// Models `<Multifield as PartialEq>::eq` (value.rs:261 fn eq):
/// `if len != len { return false; } zip(..).all(structural_eq)`.
pub open spec fn multifield_eq(xs: Seq<Value>, ys: Seq<Value>) -> bool
    decreases xs, 1nat,
{
    if xs.len() != ys.len() {
        false
    } else {
        all_pairs_eq(xs, ys)
    }
}

/// Models `self.values.iter().zip(other.values.iter()).all(|(a, b)|
/// a.structural_eq(b))` (value.rs:265-268).
pub open spec fn all_pairs_eq(xs: Seq<Value>, ys: Seq<Value>) -> bool
    decreases xs, 0nat,
{
    if xs.len() == 0 || ys.len() == 0 {
        true
    } else {
        structural_eq(xs[0], ys[0]) && all_pairs_eq(xs.drop_first(), ys.drop_first())
    }
}

// ---------------------------------------------------------------------------
// Specification: canonical form
// ---------------------------------------------------------------------------

/// The value with every fact address's public index (presentation metadata,
/// value.rs:51) set to 0, at every depth.
pub open spec fn canon(v: Value) -> Value
    decreases v,
{
    match v {
        Value::FactAddress(Some(d)) => Value::FactAddress(
            Some(FactAddressData { public_index: 0, ..d }),
        ),
        Value::Multifield(xs) => Value::Multifield(
            Seq::new(
                xs.len(),
                |i: int|
                    if 0 <= i < xs.len() {
                        canon(xs[i])
                    } else {
                        Value::Void
                    },
            ),
        ),
        _ => v,
    }
}

// ---------------------------------------------------------------------------
// Proofs
// ---------------------------------------------------------------------------

/// `all_pairs_eq` on equal-length sequences is pointwise `structural_eq`.
pub proof fn lemma_all_pairs_eq_pointwise(xs: Seq<Value>, ys: Seq<Value>)
    requires
        xs.len() == ys.len(),
    ensures
        all_pairs_eq(xs, ys) <==> forall|i: int|
            0 <= i < xs.len() ==> #[trigger] structural_eq(xs[i], ys[i]),
    decreases xs.len(),
{
    if xs.len() > 0 {
        lemma_all_pairs_eq_pointwise(xs.drop_first(), ys.drop_first());
        if all_pairs_eq(xs, ys) {
            assert forall|i: int| 0 <= i < xs.len() implies #[trigger] structural_eq(
                xs[i],
                ys[i],
            ) by {
                if i > 0 {
                    assert(xs.drop_first()[i - 1] == xs[i]);
                    assert(ys.drop_first()[i - 1] == ys[i]);
                }
            }
        }
        if forall|i: int| 0 <= i < xs.len() ==> #[trigger] structural_eq(xs[i], ys[i]) {
            assert(structural_eq(xs[0], ys[0]));
            assert forall|i: int| 0 <= i < xs.drop_first().len() implies #[trigger] structural_eq(
                xs.drop_first()[i],
                ys.drop_first()[i],
            ) by {
                assert(structural_eq(xs[i + 1], ys[i + 1]));
            }
        }
    }
}

/// Multifield equality is elementwise structural equality.
pub proof fn theorem_multifield_eq_elementwise(xs: Seq<Value>, ys: Seq<Value>)
    ensures
        multifield_eq(xs, ys) <==> xs.len() == ys.len() && forall|i: int|
            0 <= i < xs.len() ==> #[trigger] structural_eq(xs[i], ys[i]),
{
    if xs.len() == ys.len() {
        lemma_all_pairs_eq_pointwise(xs, ys);
    }
}

/// Structural equality is exactly equality of canonical forms.
pub proof fn theorem_structural_eq_is_canonical_equality(a: Value, b: Value)
    ensures
        structural_eq(a, b) <==> canon(a) == canon(b),
    decreases a,
{
    match (a, b) {
        (Value::Multifield(xs), Value::Multifield(ys)) => {
            let (cx, cy) = (canon(a)->Multifield_0, canon(b)->Multifield_0);
            assert forall|i: int| 0 <= i < xs.len() && i < ys.len() implies (#[trigger] structural_eq(
                xs[i],
                ys[i],
            ) <==> canon(xs[i]) == canon(ys[i])) by {
                theorem_structural_eq_is_canonical_equality(xs[i], ys[i]);
            }
            theorem_multifield_eq_elementwise(xs, ys);
            if structural_eq(a, b) {
                assert(multifield_eq(xs, ys));
                assert(cx.len() == cy.len());
                assert forall|i: int| 0 <= i < cx.len() implies cx[i] == cy[i] by {
                    assert(structural_eq(xs[i], ys[i]));
                    assert(cx[i] == canon(xs[i]));
                    assert(cy[i] == canon(ys[i]));
                }
                assert(cx =~= cy);
            }
            if canon(a) == canon(b) {
                assert(cx.len() == cy.len());
                assert forall|i: int| 0 <= i < xs.len() implies #[trigger] structural_eq(
                    xs[i],
                    ys[i],
                ) by {
                    assert(cx[i] == cy[i]);
                    assert(cx[i] == canon(xs[i]));
                    assert(cy[i] == canon(ys[i]));
                }
                assert(multifield_eq(xs, ys));
            }
        },
        (Value::FactAddress(x), Value::FactAddress(y)) => {
            match (x, y) {
                (Some(dx), Some(dy)) => {
                    if canon(a) == canon(b) {
                        assert(canon(a)->FactAddress_0->Some_0.epoch == dx.epoch);
                        assert(canon(b)->FactAddress_0->Some_0.epoch == dy.epoch);
                    }
                },
                _ => {},
            }
        },
        _ => {},
    }
}

/// `structural_eq` is an equivalence relation (for any nesting depth).
pub proof fn lemma_structural_eq_equivalence(a: Value, b: Value, c: Value)
    ensures
        structural_eq(a, a),
        structural_eq(a, b) ==> structural_eq(b, a),
        structural_eq(a, b) && structural_eq(b, c) ==> structural_eq(a, c),
{
    theorem_structural_eq_is_canonical_equality(a, a);
    theorem_structural_eq_is_canonical_equality(a, b);
    theorem_structural_eq_is_canonical_equality(b, a);
    theorem_structural_eq_is_canonical_equality(b, c);
    theorem_structural_eq_is_canonical_equality(a, c);
}

/// Non-vacuity witnesses of the documented semantics.
pub proof fn example_semantics() {
    let one = Value::Integer(1);
    let one_f = Value::Float(0x3FF0_0000_0000_0000);  // 1.0
    let pos_zero = Value::Float(0);
    let neg_zero = Value::Float(0x8000_0000_0000_0000);
    let nan = Value::Float(0x7FF8_0000_0000_0000);
    assert(!structural_eq(one, one_f));
    assert(!structural_eq(pos_zero, neg_zero));
    assert(structural_eq(nan, nan));
    let fa = Value::FactAddress(
        Some(FactAddressData { fact_id: 7, epoch: 1, timestamp: 3, public_index: 3 }),
    );
    let fb = Value::FactAddress(
        Some(FactAddressData { fact_id: 7, epoch: 1, timestamp: 3, public_index: 99 }),
    );
    assert(structural_eq(fa, fb));
    // Nested multifields: elementwise, ignoring public indices at depth.
    let inner = Value::Multifield(seq![nan]);
    lemma_structural_eq_equivalence(inner, inner, inner);
    let (xs, ys) = (seq![fa, inner], seq![fb, inner]);
    assert(structural_eq(xs[0], ys[0]));
    assert(structural_eq(xs[1], ys[1]));
    theorem_multifield_eq_elementwise(xs, ys);
    assert(structural_eq(Value::Multifield(xs), Value::Multifield(ys)));
    // A different length is never equal.
    theorem_multifield_eq_elementwise(xs, seq![fa]);
    assert(!structural_eq(Value::Multifield(xs), Value::Multifield(seq![fa])));
    assert(!structural_eq(Value::Symbol(SymbolId::Ascii(4)), Value::InstanceName(SymbolId::Ascii(4))));
}

} // verus!

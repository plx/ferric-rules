//! Verus model of `FerricString` equality and ordering
//! (crates/ferric-rules-core/src/string.rs).
//!
//! # What is proved (unbounded: byte strings and char sequences of any
//! length)
//!
//! * `ferric_string_eq` (model of `PartialEq::eq`) returns exactly
//!   "the byte sequences are equal", and `ferric_string_cmp` (model of
//!   `Ord::cmp`) returns exactly `lex`, lexicographic byte order written by
//!   definition (first differing byte decides; a proper prefix is smaller).
//! * `lex` is a total order: `Equal` iff the sequences are equal,
//!   antisymmetric (`lex(b, a)` is the reverse of `lex(a, b)`) and
//!   transitive; hence `eq` and `cmp` are consistent
//!   (`theorem_eq_cmp_consistent`).
//! * `theorem_utf8_byte_order_is_scalar_order`: the claim in the
//!   `FerricString` doc comment that byte order "for UTF-8 is equivalent to
//!   ordering by Unicode scalar value": for all `a, b: Seq<char>`,
//!   `lex(encode_utf8(a), encode_utf8(b)) == lex_scalars(a, b)`, where
//!   `encode_utf8` is vstd's independent specification of UTF-8 encoding
//!   (vstd::utf8, written from the Unicode standard) and `lex_scalars`
//!   compares the scalar values (`char as u32`) lexicographically.
//!
//! # Model and trusted base
//!
//! * `FerricString::eq` (string.rs:82) is `self.as_bytes() == other.as_bytes()`
//!   and `FerricString::cmp` (string.rs:97) is
//!   `self.as_bytes().cmp(other.as_bytes())`. Both delegate to core's
//!   `[u8]` implementations: equality compares lengths and then the bytes;
//!   ordering compares the common prefix (memcmp) and then the lengths. The
//!   exec functions below model those algorithms over `&[u8]` (the content
//!   `as_bytes` returns for either variant). Trusted: core's slice
//!   comparison implements this algorithm.
//! * A `&str` / `Box<str>` is modeled by the UTF-8 encoding of its chars
//!   (`encode_utf8`). Trusted: Rust's `str` invariant.
//! * No `assume`, `admit`, `external_body` or trusted glue in this file.
//!
//! # Known gaps
//!
//! * The model is not mechanically linked to the Rust source; the bounded
//!   Kani harnesses in crates/ferric-rules-core/src/string/kani_proofs.rs
//!   check the same properties on the production code for short inputs.
//! * `Hash` is not modeled here (see the Kani harnesses, including the
//!   `Borrow<str>` hash-contract defect).

use vstd::prelude::*;
use vstd::utf8::*;

verus! {

pub enum Ord3 {
    Less,
    Equal,
    Greater,
}

pub open spec fn reverse(o: Ord3) -> Ord3 {
    match o {
        Ord3::Less => Ord3::Greater,
        Ord3::Equal => Ord3::Equal,
        Ord3::Greater => Ord3::Less,
    }
}

/// Lexicographic order on byte strings, by definition.
pub open spec fn lex(a: Seq<u8>, b: Seq<u8>) -> Ord3
    decreases a.len(),
{
    if a.len() == 0 {
        if b.len() == 0 {
            Ord3::Equal
        } else {
            Ord3::Less
        }
    } else if b.len() == 0 {
        Ord3::Greater
    } else if a[0] < b[0] {
        Ord3::Less
    } else if a[0] > b[0] {
        Ord3::Greater
    } else {
        lex(a.drop_first(), b.drop_first())
    }
}

/// Lexicographic order on sequences of Unicode scalar values.
pub open spec fn lex_scalars(a: Seq<char>, b: Seq<char>) -> Ord3
    decreases a.len(),
{
    if a.len() == 0 {
        if b.len() == 0 {
            Ord3::Equal
        } else {
            Ord3::Less
        }
    } else if b.len() == 0 {
        Ord3::Greater
    } else if (a[0] as u32) < (b[0] as u32) {
        Ord3::Less
    } else if (a[0] as u32) > (b[0] as u32) {
        Ord3::Greater
    } else {
        lex_scalars(a.drop_first(), b.drop_first())
    }
}

// ---------------------------------------------------------------------------
// Exec models of the production comparisons
// ---------------------------------------------------------------------------

/// Models `<FerricString as PartialEq>::eq` (crates/ferric-rules-core/src/string.rs:82 fn eq),
/// i.e. `self.as_bytes() == other.as_bytes()`: compare lengths, then bytes.
pub fn ferric_string_eq(a: &[u8], b: &[u8]) -> (r: bool)
    ensures
        r == (a@ == b@),
{
    if a.len() != b.len() {
        return false;
    }
    let mut i: usize = 0;
    while i < a.len()
        invariant
            a.len() == b.len(),
            0 <= i <= a.len(),
            forall|j: int| 0 <= j < i ==> a@[j] == b@[j],
        decreases a.len() - i,
    {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    assert(a@ =~= b@);
    true
}

/// Models `<FerricString as Ord>::cmp` (crates/ferric-rules-core/src/string.rs:97 fn cmp),
/// i.e. `self.as_bytes().cmp(other.as_bytes())`: compare the common prefix
/// byte by byte, then the lengths.
pub fn ferric_string_cmp(a: &[u8], b: &[u8]) -> (r: Ord3)
    ensures
        r == lex(a@, b@),
{
    let n = if a.len() < b.len() {
        a.len()
    } else {
        b.len()
    };
    let mut i: usize = 0;
    assert(a@.subrange(0, a.len() as int) =~= a@);
    assert(b@.subrange(0, b.len() as int) =~= b@);
    while i < n
        invariant
            n <= a.len(),
            n <= b.len(),
            n == a.len() || n == b.len(),
            0 <= i <= n,
            lex(a@, b@) == lex(a@.subrange(i as int, a.len() as int), b@.subrange(i as int, b.len() as int)),
        decreases n - i,
    {
        let ghost sa = a@.subrange(i as int, a.len() as int);
        let ghost sb = b@.subrange(i as int, b.len() as int);
        assert(sa[0] == a@[i as int]);
        assert(sb[0] == b@[i as int]);
        if a[i] < b[i] {
            return Ord3::Less;
        }
        if a[i] > b[i] {
            return Ord3::Greater;
        }
        assert(sa.drop_first() =~= a@.subrange(i + 1, a.len() as int));
        assert(sb.drop_first() =~= b@.subrange(i + 1, b.len() as int));
        i += 1;
    }
    if a.len() < b.len() {
        Ord3::Less
    } else if a.len() > b.len() {
        Ord3::Greater
    } else {
        Ord3::Equal
    }
}

// ---------------------------------------------------------------------------
// `lex` is a total order
// ---------------------------------------------------------------------------

pub proof fn lemma_lex_equal_iff(a: Seq<u8>, b: Seq<u8>)
    ensures
        (lex(a, b) == Ord3::Equal) <==> (a == b),
    decreases a.len(),
{
    if a.len() == 0 && b.len() == 0 {
        assert(a =~= b);
    }
    if a.len() > 0 && b.len() > 0 && a[0] == b[0] {
        lemma_lex_equal_iff(a.drop_first(), b.drop_first());
        if a.drop_first() == b.drop_first() {
            assert(a =~= seq![a[0]] + a.drop_first());
            assert(b =~= seq![b[0]] + b.drop_first());
        }
        if a == b {
            assert(a.drop_first() =~= b.drop_first());
        }
    }
}

pub proof fn lemma_lex_reverse(a: Seq<u8>, b: Seq<u8>)
    ensures
        lex(b, a) == reverse(lex(a, b)),
    decreases a.len(),
{
    if a.len() > 0 && b.len() > 0 && a[0] == b[0] {
        lemma_lex_reverse(a.drop_first(), b.drop_first());
    }
}

pub proof fn lemma_lex_transitive(a: Seq<u8>, b: Seq<u8>, c: Seq<u8>)
    requires
        lex(a, b) != Ord3::Greater,
        lex(b, c) != Ord3::Greater,
    ensures
        lex(a, c) != Ord3::Greater,
        lex(a, b) == Ord3::Less || lex(b, c) == Ord3::Less ==> lex(a, c) == Ord3::Less,
    decreases a.len(),
{
    if a.len() > 0 && b.len() > 0 && c.len() > 0 && a[0] == b[0] && b[0] == c[0] {
        lemma_lex_transitive(a.drop_first(), b.drop_first(), c.drop_first());
    }
}

/// `==` and `cmp` of the model agree (`Eq`/`Ord` consistency).
pub proof fn theorem_eq_cmp_consistent(a: Seq<u8>, b: Seq<u8>)
    ensures
        (a == b) <==> (lex(a, b) == Ord3::Equal),
{
    lemma_lex_equal_iff(a, b);
}

// ---------------------------------------------------------------------------
// Appending to compared prefixes
// ---------------------------------------------------------------------------

/// Two prefixes of the same length: if they differ the prefixes decide,
/// otherwise the suffixes do.
pub proof fn lemma_lex_append_same_len(p: Seq<u8>, q: Seq<u8>, x: Seq<u8>, y: Seq<u8>)
    requires
        p.len() == q.len(),
    ensures
        lex(p, q) != Ord3::Equal ==> lex(p + x, q + y) == lex(p, q),
        lex(p, q) == Ord3::Equal ==> lex(p + x, q + y) == lex(x, y),
    decreases p.len(),
{
    if p.len() == 0 {
        assert(p + x =~= x);
        assert(q + y =~= y);
    } else {
        assert((p + x)[0] == p[0]);
        assert((q + y)[0] == q[0]);
        assert((p + x).drop_first() =~= p.drop_first() + x);
        assert((q + y).drop_first() =~= q.drop_first() + y);
        lemma_lex_append_same_len(p.drop_first(), q.drop_first(), x, y);
    }
}

/// Prefixes whose first bytes differ decide the comparison.
pub proof fn lemma_lex_append_first_byte(p: Seq<u8>, q: Seq<u8>, x: Seq<u8>, y: Seq<u8>)
    requires
        p.len() > 0,
        q.len() > 0,
        p[0] != q[0],
    ensures
        lex(p + x, q + y) == (if p[0] < q[0] {
            Ord3::Less
        } else {
            Ord3::Greater
        }),
{
    assert((p + x)[0] == p[0]);
    assert((q + y)[0] == q[0]);
}

// ---------------------------------------------------------------------------
// Order of single-scalar encodings (bit-vector facts about vstd's encoder)
// ---------------------------------------------------------------------------

pub open spec fn width(c: u32) -> int {
    if has_width_1_encoding(c) {
        1
    } else if has_width_2_encoding(c) {
        2
    } else if has_width_3_encoding(c) {
        3
    } else {
        4
    }
}

/// The first byte of an encoding lies in a range determined by the width,
/// and the ranges increase with the width.
proof fn lemma_lead_ranges(c: u32)
    by (bit_vector)
    requires
        has_width_1_encoding(c) || has_width_2_encoding(c) || has_width_3_encoding(c)
            || has_width_4_encoding(c),
    ensures
        has_width_1_encoding(c) ==> leading_byte_width_1(c) <= 0x7F,
        has_width_2_encoding(c) ==> 0xC0 <= leading_byte_width_2(c) && leading_byte_width_2(c)
            <= 0xDF,
        has_width_3_encoding(c) ==> 0xE0 <= leading_byte_width_3(c) && leading_byte_width_3(c)
            <= 0xEF,
        has_width_4_encoding(c) ==> 0xF0 <= leading_byte_width_4(c),
{
}

proof fn lemma_w1_order(c: u32, d: u32)
    by (bit_vector)
    requires
        has_width_1_encoding(c),
        has_width_1_encoding(d),
        c < d,
    ensures
        leading_byte_width_1(c) < leading_byte_width_1(d),
{
}

proof fn lemma_w2_order(c: u32, d: u32)
    by (bit_vector)
    requires
        has_width_2_encoding(c),
        has_width_2_encoding(d),
        c < d,
    ensures
        leading_byte_width_2(c) < leading_byte_width_2(d) || (leading_byte_width_2(c)
            == leading_byte_width_2(d) && last_continuation_byte(c) < last_continuation_byte(d)),
{
}

proof fn lemma_w3_order(c: u32, d: u32)
    by (bit_vector)
    requires
        has_width_3_encoding(c),
        has_width_3_encoding(d),
        c < d,
    ensures
        leading_byte_width_3(c) < leading_byte_width_3(d) || (leading_byte_width_3(c)
            == leading_byte_width_3(d) && (second_last_continuation_byte(c)
            < second_last_continuation_byte(d) || (second_last_continuation_byte(c)
            == second_last_continuation_byte(d) && last_continuation_byte(c)
            < last_continuation_byte(d)))),
{
}

proof fn lemma_w4_order(c: u32, d: u32)
    by (bit_vector)
    requires
        has_width_4_encoding(c),
        has_width_4_encoding(d),
        c < d,
    ensures
        leading_byte_width_4(c) < leading_byte_width_4(d) || (leading_byte_width_4(c)
            == leading_byte_width_4(d) && (third_last_continuation_byte(c)
            < third_last_continuation_byte(d) || (third_last_continuation_byte(c)
            == third_last_continuation_byte(d) && (second_last_continuation_byte(c)
            < second_last_continuation_byte(d) || (second_last_continuation_byte(c)
            == second_last_continuation_byte(d) && last_continuation_byte(c)
            < last_continuation_byte(d)))))),
{
}

/// Same-width encodings of `c < d` compare `Less` (the encodings have equal
/// length, so the comparison is decided inside them).
proof fn lemma_same_width_order(c: u32, d: u32)
    requires
        is_scalar(c),
        is_scalar(d),
        width(c) == width(d),
        c < d,
    ensures
        encode_scalar(c).len() == encode_scalar(d).len(),
        lex(encode_scalar(c), encode_scalar(d)) == Ord3::Less,
{
    let (ec, ed) = (encode_scalar(c), encode_scalar(d));
    reveal_with_fuel(lex, 5);
    if width(c) == 1 {
        lemma_w1_order(c, d);
    } else if width(c) == 2 {
        lemma_w2_order(c, d);
        assert(ec.drop_first() =~= seq![last_continuation_byte(c)]);
        assert(ed.drop_first() =~= seq![last_continuation_byte(d)]);
    } else if width(c) == 3 {
        lemma_w3_order(c, d);
        assert(ec.drop_first() =~= seq![second_last_continuation_byte(c), last_continuation_byte(c)]);
        assert(ed.drop_first() =~= seq![second_last_continuation_byte(d), last_continuation_byte(d)]);
        assert(ec.drop_first().drop_first() =~= seq![last_continuation_byte(c)]);
        assert(ed.drop_first().drop_first() =~= seq![last_continuation_byte(d)]);
    } else {
        lemma_w4_order(c, d);
        assert(ec.drop_first() =~= seq![
            third_last_continuation_byte(c),
            second_last_continuation_byte(c),
            last_continuation_byte(c),
        ]);
        assert(ed.drop_first() =~= seq![
            third_last_continuation_byte(d),
            second_last_continuation_byte(d),
            last_continuation_byte(d),
        ]);
        assert(ec.drop_first().drop_first() =~= seq![
            second_last_continuation_byte(c),
            last_continuation_byte(c),
        ]);
        assert(ed.drop_first().drop_first() =~= seq![
            second_last_continuation_byte(d),
            last_continuation_byte(d),
        ]);
        assert(ec.drop_first().drop_first().drop_first() =~= seq![last_continuation_byte(c)]);
        assert(ed.drop_first().drop_first().drop_first() =~= seq![last_continuation_byte(d)]);
    }
}

/// For scalars `c < d`, the encoding of `c` followed by anything is smaller
/// than the encoding of `d` followed by anything (UTF-8 is prefix-free and
/// order-preserving on single scalars).
proof fn lemma_scalar_order(c: u32, d: u32, x: Seq<u8>, y: Seq<u8>)
    requires
        is_scalar(c),
        is_scalar(d),
        c < d,
    ensures
        lex(encode_scalar(c) + x, encode_scalar(d) + y) == Ord3::Less,
{
    let (ec, ed) = (encode_scalar(c), encode_scalar(d));
    if width(c) == width(d) {
        lemma_same_width_order(c, d);
        lemma_lex_append_same_len(ec, ed, x, y);
    } else {
        // width(c) < width(d), so the lead bytes lie in increasing ranges.
        lemma_lead_ranges(c);
        lemma_lead_ranges(d);
        assert(ec[0] < ed[0]);
        lemma_lex_append_first_byte(ec, ed, x, y);
    }
}

// ---------------------------------------------------------------------------
// Main theorem
// ---------------------------------------------------------------------------

/// Byte order of UTF-8 encodings equals scalar-value order of the decoded
/// sequences (string.rs, `FerricString` doc comment).
pub proof fn theorem_utf8_byte_order_is_scalar_order(a: Seq<char>, b: Seq<char>)
    ensures
        lex(encode_utf8(a), encode_utf8(b)) == lex_scalars(a, b),
    decreases a.len(),
{
    if a.len() == 0 {
        assert(encode_utf8(a) =~= Seq::<u8>::empty());
        if b.len() > 0 {
            assert(encode_scalar(b[0] as u32).len() > 0);
            assert(encode_utf8(b).len() > 0);
        } else {
            assert(encode_utf8(b) =~= Seq::<u8>::empty());
        }
    } else if b.len() == 0 {
        assert(encode_utf8(b) =~= Seq::<u8>::empty());
        assert(encode_scalar(a[0] as u32).len() > 0);
        assert(encode_utf8(a).len() > 0);
    } else {
        let (c, d) = (a[0] as u32, b[0] as u32);
        let (x, y) = (encode_utf8(a.drop_first()), encode_utf8(b.drop_first()));
        assert(encode_utf8(a) == encode_scalar(c) + x);
        assert(encode_utf8(b) == encode_scalar(d) + y);
        char_is_scalar(a[0]);
        char_is_scalar(b[0]);
        if c < d {
            lemma_scalar_order(c, d, x, y);
        } else if d < c {
            lemma_scalar_order(d, c, y, x);
            lemma_lex_reverse(encode_scalar(d) + y, encode_scalar(c) + x);
        } else {
            lemma_lex_equal_iff(encode_scalar(c), encode_scalar(d));
            lemma_lex_append_same_len(encode_scalar(c), encode_scalar(d), x, y);
            theorem_utf8_byte_order_is_scalar_order(a.drop_first(), b.drop_first());
        }
    }
}

/// Non-vacuity witness on concrete strings: "z" < "é" and "€a" < "😀",
/// both as scalar sequences and as UTF-8 byte sequences.
pub proof fn example_concrete_order() {
    let z = seq!['z'];
    let e = seq!['\u{e9}'];
    theorem_utf8_byte_order_is_scalar_order(z, e);
    assert(lex_scalars(z, e) == Ord3::Less);
    assert(lex(encode_utf8(z), encode_utf8(e)) == Ord3::Less);
    let euro = seq!['\u{20ac}', 'a'];
    let smile = seq!['\u{1f600}'];
    theorem_utf8_byte_order_is_scalar_order(euro, smile);
    assert(lex_scalars(euro, smile) == Ord3::Less);
}

} // verus!

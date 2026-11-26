//! Kani proof harnesses for `string` (`FerricString`).
//!
//! What is proved, on the production code (bounds in brackets; "either
//! representation" means the raw `Ascii` variant over arbitrary bytes, which
//! is public, or the `Utf8` variant when the bytes are valid UTF-8):
//!
//! - `eq_and_cmp_match_byte_spec`: `==`/`!=` is exact byte equality and
//!   `cmp`/`partial_cmp`/`<`/`<=`/`>`/`>=` is lexicographic byte order, both
//!   compared with independent specifications written from the definitions;
//!   `Eq` and `Ord` are consistent; `as_bytes`/`len`/`is_empty` are exact
//!   [all pairs of byte strings of length <= 3, either representation].
//! - `new_accepts_exactly_per_encoding_{ascii,utf8,mixed}` (one harness per
//!   mode): `FerricString::new` accepts iff the
//!   mode is not `Ascii` or every byte is < 0x80, picks the `Ascii` variant
//!   exactly in `Ascii` mode, stores the input byte-exactly (checked through
//!   `as_bytes`, `len`, `is_empty`, and for the `Utf8` variant also
//!   `as_str`, `AsRef<str>`, `Borrow<str>`), and otherwise returns
//!   `NonAsciiString(input)` [every `&str` of <= 3 bytes in `Ascii` mode,
//!   <= 4 bytes in the other two modes]. `as_str` etc. on the `Ascii`
//!   variant: `ascii_variant_as_str_is_exact_on_utf8_content`.
//! - `hash_input_is_exactly_byte_content_{A}_{B}`: the exact sequence of
//!   `Hasher` calls made by `hash` is equal iff the byte contents are equal,
//!   so `a == b => hash(a) == hash(b)` for every deterministic hasher, across
//!   representations [pairs of every length combination <= 2, either
//!   representation; lengths are concrete per harness because `hash`
//!   validates `Ascii` content as UTF-8].
//! - `cmp_is_a_total_order`: reflexive, antisymmetric, transitive [triples,
//!   length <= 2, either representation].
//! - `utf8_byte_order_is_scalar_value_order`: the doc claim that byte order
//!   on UTF-8 content equals Unicode scalar-value order, per scalar [every
//!   pair of `char`s, built in `Utf8` mode].
//! - `borrow_str_eq_agrees_with_owned`: the `Eq` part of the `Borrow<str>`
//!   contract holds [`Utf8`-mode strings of 2 bytes]; see the harness doc
//!   for why the `Ord` part is only implied.
//! - `ascii_variant_as_str_is_exact_on_utf8_content` /
//!   `ascii_variant_as_str_panics_on_invalid_utf8`: `as_str` on the raw
//!   `Ascii` variant returns the content exactly iff it is valid UTF-8 and
//!   panics otherwise (values from `new` are never in the panicking case)
//!   [content of exactly 2 bytes]. The `should_panic` harness ends with a
//!   cover after the call that must be reported UNREACHABLE: Kani's
//!   `should_panic` alone only shows that *some* input panics.
//! - `borrow_str_hash_agrees_with_owned_len{0..3}`: the `Hash` part of the
//!   `Borrow<str>` contract, `hash(x) == hash(x.borrow())`, holds for both
//!   variants [values from `new`, ASCII content of each length 0..=3]. This
//!   harness found a defect: `FerricString` used to hash as `[u8]`
//!   (`write_usize(len)`, then `write(bytes)`) while `str` hashes via
//!   `write_str` (`write(bytes)`, then `write_u8(0xff)`), so a lookup by
//!   `&str` missed present keys.
//!
//! Hashing is observed with `RecordingHasher`, which logs every stable
//! `Hasher` method call with its argument; equal logs imply equal hashes for
//! any deterministic hasher (the unstable `write_length_prefix`/`write_str`
//! reach it through their default delegations).
//!
//! Gaps: `Display` (the formatting machinery) and serde are not checked;
//! lengths are bounded as stated. Wherever `as_str` validates heap content
//! of the `Ascii` variant, the content length is concrete: with a symbolic
//! length those harnesses exceeded the solver's memory (7 GB). Multi-`char`
//! scalar order exceeded it as well. Unbounded proofs of the equality/order
//! algorithm and of the scalar-order theorem, on a model, are in
//! verification/verus/ferric_string_order.rs.

use super::*;
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

/// Maximum byte length of the symbolic strings in the pairwise harnesses.
const MAX: usize = 3;

/// A symbolic byte string: `N` symbolic bytes and a symbolic length `<= N`.
fn any_bytes<const N: usize>() -> ([u8; N], usize) {
    let bytes: [u8; N] = kani::any();
    let len: usize = kani::any_where(|&l: &usize| l <= N);
    (bytes, len)
}

/// Independent specification of lexicographic order, written from the
/// definition: scan for the first position where the sequences differ and
/// compare there; if one sequence runs out first it is the smaller one.
fn spec_lex_cmp<T: Ord>(a: &[T], b: &[T]) -> Ordering {
    let mut i = 0;
    loop {
        match (i < a.len(), i < b.len()) {
            (false, false) => return Ordering::Equal,
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            (true, true) => {
                if a[i] < b[i] {
                    return Ordering::Less;
                }
                if b[i] < a[i] {
                    return Ordering::Greater;
                }
                i += 1;
            }
        }
    }
}

/// Independent specification of exact byte equality.
fn spec_bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Independent ASCII predicate: every byte is below 0x80.
fn spec_all_ascii(bytes: &[u8]) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] >= 0x80 {
            return false;
        }
        i += 1;
    }
    true
}

/// Any `FerricString` whose content is exactly `bytes`, in either
/// representation: the `Ascii` variant over the raw bytes (the variant is
/// public, so this includes values that violate the documented ASCII-only
/// invariant), or the `Utf8` variant when the bytes are valid UTF-8.
fn any_ferric_string(bytes: &[u8]) -> FerricString {
    if kani::any() {
        FerricString::Ascii(bytes.into())
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => FerricString::Utf8(s.into()),
            Err(_) => FerricString::Ascii(bytes.into()),
        }
    }
}

/// A `Hasher` that records every call made to it, so two values with equal
/// recordings produce equal hashes under every deterministic `Hasher` that
/// implements the stable `Hasher` methods. Each call becomes one `u128`
/// entry: the method tag in the top byte and the argument below it. A
/// `write` entry also carries the slice length and its first eight bytes;
/// any further bytes follow, eight per entry. The length fixes how many
/// entries follow, so the encoding is injective, and short logs keep the
/// comparison loop (and so the harnesses' unwinding bounds) small.
#[derive(Default, Debug)]
struct RecordingHasher {
    calls: Vec<u128>,
}

/// Element-wise comparison of the logs (the derived `Vec` equality uses
/// `memcmp`, whose byte loop needs a much larger unwinding bound).
impl PartialEq for RecordingHasher {
    fn eq(&self, other: &Self) -> bool {
        if self.calls.len() != other.calls.len() {
            return false;
        }
        let mut i = 0;
        while i < self.calls.len() {
            if self.calls[i] != other.calls[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl RecordingHasher {
    fn record(&mut self, tag: u8, value: u64) {
        self.calls
            .push((u128::from(tag) << 120) | u128::from(value));
    }
}

#[allow(clippy::cast_sign_loss)]
impl Hasher for RecordingHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut entry = (bytes.len() as u128) << 64;
        let mut shift = 0;
        for &b in bytes {
            if shift == 64 {
                self.calls.push(entry);
                entry = 0xff << 120;
                shift = 0;
            }
            entry |= u128::from(b) << shift;
            shift += 8;
        }
        self.calls.push(entry);
    }
    fn write_u8(&mut self, i: u8) {
        self.record(1, u64::from(i));
    }
    fn write_u16(&mut self, i: u16) {
        self.record(2, u64::from(i));
    }
    fn write_u32(&mut self, i: u32) {
        self.record(3, u64::from(i));
    }
    fn write_u64(&mut self, i: u64) {
        self.record(4, i);
    }
    fn write_usize(&mut self, i: usize) {
        self.record(5, i as u64);
    }
    fn write_i8(&mut self, i: i8) {
        self.record(6, i as u64);
    }
    fn write_i16(&mut self, i: i16) {
        self.record(7, i as u64);
    }
    fn write_i32(&mut self, i: i32) {
        self.record(8, i as u64);
    }
    fn write_i64(&mut self, i: i64) {
        self.record(9, i as u64);
    }
    fn write_isize(&mut self, i: isize) {
        self.record(10, i as u64);
    }
}

fn hash_record<T: Hash + ?Sized>(value: &T) -> RecordingHasher {
    let mut hasher = RecordingHasher::default();
    value.hash(&mut hasher);
    hasher
}

/// `==` on `FerricString` is exact byte equality and `cmp` is lexicographic
/// byte order, for every pair of byte strings of length <= `MAX` and every
/// representation; `partial_cmp`, `<`, `<=`, `>`, `>=` agree with `cmp`.
#[kani::proof]
#[kani::unwind(5)]
fn eq_and_cmp_match_byte_spec() {
    let (abuf, alen) = any_bytes::<MAX>();
    let (bbuf, blen) = any_bytes::<MAX>();
    let (a, b) = (&abuf[..alen], &bbuf[..blen]);
    let fa = any_ferric_string(a);
    let fb = any_ferric_string(b);

    let spec_eq = spec_bytes_eq(a, b);
    let spec_ord = spec_lex_cmp(a, b);

    assert_eq!(fa == fb, spec_eq);
    assert_eq!(fa != fb, !spec_eq);
    assert_eq!(fa.cmp(&fb), spec_ord);
    assert_eq!(fa.partial_cmp(&fb), Some(spec_ord));
    assert_eq!(fa < fb, spec_ord == Ordering::Less);
    assert_eq!(fa <= fb, spec_ord != Ordering::Greater);
    assert_eq!(fa > fb, spec_ord == Ordering::Greater);
    assert_eq!(fa >= fb, spec_ord != Ordering::Less);
    // Eq and Ord are consistent.
    assert_eq!(fa == fb, fa.cmp(&fb) == Ordering::Equal);
    // Accessors expose exactly the stored bytes.
    assert_eq!(fa.as_bytes(), a);
    assert_eq!(fa.len(), alen);
    assert_eq!(fa.is_empty(), alen == 0);

    kani::cover!(matches!(fa, FerricString::Ascii(_)) && matches!(fb, FerricString::Utf8(_)));
    kani::cover!(spec_eq && alen == MAX);
    kani::cover!(spec_ord == Ordering::Less && alen > blen);
    kani::cover!(spec_ord == Ordering::Greater && alen < blen);
    kani::cover!(spec_ord == Ordering::Less && alen < blen && alen > 0);
    kani::cover!(!spec_eq && alen == blen && alen == MAX);
}

/// Checks `FerricString::new(s, encoding)` against the documented rules for
/// every `&str` of byte length <= `N`: `Ascii` accepts iff every byte is
/// < 0x80 and stores the `Ascii` variant; `Utf8` and
/// `AsciiSymbolsUtf8Strings` accept every `&str` and store the `Utf8`
/// variant. Accepted values expose exactly the input through every accessor
/// (and `as_str` does not panic); a rejection carries the input. Returns
/// (input length, input is ASCII) for the harnesses' cover statements.
fn check_new<const N: usize>(encoding: StringEncoding) -> (usize, bool) {
    let (buf, len) = any_bytes::<N>();
    check_new_on(&buf[..len], encoding)
}

/// `check_new` on the given (symbolic) bytes.
fn check_new_on(bytes: &[u8], encoding: StringEncoding) -> (usize, bool) {
    let len = bytes.len();
    let parsed = std::str::from_utf8(bytes);
    kani::assume(parsed.is_ok());
    let s = parsed.unwrap();
    let ascii = spec_all_ascii(bytes);

    match (encoding, FerricString::new(s, encoding)) {
        (StringEncoding::Ascii, Ok(value)) => {
            assert!(ascii);
            assert!(matches!(value, FerricString::Ascii(_)));
            // `as_str`/`AsRef`/`Borrow` on the `Ascii` variant are checked in
            // `ascii_variant_as_str_is_exact_on_utf8_content` (validating
            // heap content here exceeded the solver's memory).
            assert_eq!(value.as_bytes(), s.as_bytes());
            assert_eq!(value.len(), s.len());
            assert_eq!(value.is_empty(), s.is_empty());
        }
        (StringEncoding::Ascii, Err(err)) => {
            assert!(!ascii);
            assert_eq!(err, EncodingError::NonAsciiString(s.to_string()));
        }
        (StringEncoding::Utf8 | StringEncoding::AsciiSymbolsUtf8Strings, Ok(value)) => {
            assert!(matches!(value, FerricString::Utf8(_)));
            check_content(&value, s);
        }
        (StringEncoding::Utf8 | StringEncoding::AsciiSymbolsUtf8Strings, Err(_)) => {
            panic!("UTF-8 string modes must accept every &str");
        }
    }
    (len, ascii)
}

/// `FerricString::new` in `Ascii` mode (see `check_new`), byte length <= 3.
#[kani::proof]
#[kani::unwind(5)]
fn new_accepts_exactly_per_encoding_ascii() {
    let (len, ascii) = check_new::<3>(StringEncoding::Ascii);
    kani::cover!(ascii && len == 3);
    kani::cover!(!ascii && len == 3);
    kani::cover!(!ascii && len == 2);
    kani::cover!(len == 0);
}

/// `FerricString::new` in `Utf8` mode (see `check_new`), byte length <= 4.
#[kani::proof]
#[kani::unwind(6)]
fn new_accepts_exactly_per_encoding_utf8() {
    let (len, ascii) = check_new::<4>(StringEncoding::Utf8);
    kani::cover!(!ascii && len == 3);
    kani::cover!(ascii && len == 4);
}

/// `FerricString::new` in `AsciiSymbolsUtf8Strings` mode (see `check_new`),
/// byte length <= 4.
#[kani::proof]
#[kani::unwind(6)]
fn new_accepts_exactly_per_encoding_mixed() {
    let (len, ascii) = check_new::<4>(StringEncoding::AsciiSymbolsUtf8Strings);
    kani::cover!(!ascii && len == 4);
    kani::cover!(ascii && len == 1);
}

fn check_content(value: &FerricString, s: &str) {
    assert_eq!(value.as_bytes(), s.as_bytes());
    assert_eq!(value.as_str(), s);
    let as_ref: &str = value.as_ref();
    assert_eq!(as_ref, s);
    let borrowed: &str = Borrow::borrow(value);
    assert_eq!(borrowed, s);
    assert_eq!(value.len(), s.len());
    assert_eq!(value.is_empty(), s.is_empty());
}

/// The hash input of a `FerricString` is a function of its bytes alone
/// (independent of representation) and is injective on byte content:
/// equal strings feed identical call sequences to any `Hasher` and unequal
/// strings feed different ones. In particular `a == b => hash(a) == hash(b)`.
///
/// `hash` validates the UTF-8 of `Ascii` content (to hash exactly like the
/// borrowed `str`); with symbolic lengths that validation exceeded the
/// solver's memory, so each harness below fixes the two lengths. Together
/// they cover every pair of lengths <= 2 (the property is symmetric).
fn check_hash_input<const A: usize, const B: usize>() {
    let abuf: [u8; A] = kani::any();
    let bbuf: [u8; B] = kani::any();
    let fa = any_ferric_string(&abuf);
    let fb = any_ferric_string(&bbuf);

    let same_record = hash_record(&fa) == hash_record(&fb);
    assert_eq!(fa == fb, same_record);

    kani::cover!(
        same_record && matches!(fa, FerricString::Ascii(_)) && matches!(fb, FerricString::Utf8(_))
    );
    if A == B && A > 0 {
        kani::cover!(!same_record);
    }
}

macro_rules! hash_input_harnesses {
    ($($name:ident: $a:literal, $b:literal;)*) => {$(
        #[kani::proof]
        #[kani::unwind(4)]
        fn $name() {
            check_hash_input::<$a, $b>();
        }
    )*};
}

hash_input_harnesses! {
    hash_input_is_exactly_byte_content_0_0: 0, 0;
    hash_input_is_exactly_byte_content_0_1: 0, 1;
    hash_input_is_exactly_byte_content_0_2: 0, 2;
    hash_input_is_exactly_byte_content_1_1: 1, 1;
    hash_input_is_exactly_byte_content_1_2: 1, 2;
    hash_input_is_exactly_byte_content_2_2: 2, 2;
}

/// `Ord` on `FerricString` is a total order: reflexive, antisymmetric
/// (`cmp(a, b) == cmp(b, a).reverse()`) and transitive, for all triples of
/// byte strings of length <= 2 in any representation.
#[kani::proof]
#[kani::unwind(4)]
fn cmp_is_a_total_order() {
    let (abuf, alen) = any_bytes::<2>();
    let (bbuf, blen) = any_bytes::<2>();
    let (cbuf, clen) = any_bytes::<2>();
    let fa = any_ferric_string(&abuf[..alen]);
    let fb = any_ferric_string(&bbuf[..blen]);
    let fc = any_ferric_string(&cbuf[..clen]);

    assert_eq!(fa.cmp(&fa), Ordering::Equal);
    assert!(fa == fa);
    assert_eq!(fa.cmp(&fb), fb.cmp(&fa).reverse());
    if fa <= fb && fb <= fc {
        assert!(fa <= fc);
    }
    if fa < fb && fb < fc {
        assert!(fa < fc);
    }
    if fa == fb && fb == fc {
        assert!(fa == fc);
    }

    kani::cover!(fa < fb && fb < fc);
    kani::cover!(fa == fb && fb == fc && alen == 2);
    kani::cover!(fa < fb && fb == fc);
}

/// The documented claim (string.rs, `FerricString` doc) that byte order on
/// UTF-8 content "is equivalent to ordering by Unicode scalar value", for
/// single scalars: for every pair of `char`s, the `FerricString`s built in
/// `Utf8` mode from their encodings compare exactly as the scalar values
/// do. (UTF-8 is prefix-free, so this per-scalar fact extends to sequences;
/// the sequence theorem is proved unboundedly in
/// verification/verus/ferric_string_order.rs. Two-`char` strings exceeded
/// the solver's memory here.)
#[kani::proof]
#[kani::unwind(5)]
fn utf8_byte_order_is_scalar_value_order() {
    let (c, d): (char, char) = (kani::any(), kani::any());
    let (mut cbuf, mut dbuf) = ([0u8; 4], [0u8; 4]);
    let fc = FerricString::new(c.encode_utf8(&mut cbuf), StringEncoding::Utf8).unwrap();
    let fd = FerricString::new(d.encode_utf8(&mut dbuf), StringEncoding::Utf8).unwrap();

    let scalar_order = (c as u32).cmp(&(d as u32));
    assert_eq!(fc.cmp(&fd), scalar_order);
    assert_eq!(fc == fd, c == d);

    kani::cover!(c.len_utf8() == 1 && d.len_utf8() == 4);
    kani::cover!(c.len_utf8() == 3 && d.len_utf8() == 3 && scalar_order == Ordering::Less);
    kani::cover!(c.len_utf8() == 2 && d.len_utf8() == 2 && scalar_order == Ordering::Greater);
}

/// The `Borrow<str>` contract requires `x.borrow() == y.borrow()` iff
/// `x == y`. This holds for all pairs of `Utf8`-mode strings of exactly 2
/// bytes. (The `Ord` half was not checked here: with `str::cmp` on the
/// borrowed forms, and with symbolic lengths, the harness exceeded the
/// solver's memory. It follows from `borrow()` returning exactly the
/// content, `ascii_variant_as_str_is_exact_on_utf8_content` and
/// `new_accepts_exactly_per_encoding_*`, and `cmp` being lexicographic byte
/// order, `eq_and_cmp_match_byte_spec`, given that `str::cmp` is byte order.)
#[kani::proof]
#[kani::unwind(4)]
fn borrow_str_eq_agrees_with_owned() {
    let (abuf, bbuf): ([u8; 2], [u8; 2]) = (kani::any(), kani::any());
    let ra = std::str::from_utf8(&abuf);
    let rb = std::str::from_utf8(&bbuf);
    kani::assume(ra.is_ok() && rb.is_ok());
    let fa = FerricString::new(ra.unwrap(), StringEncoding::Utf8).unwrap();
    let fb = FerricString::new(rb.unwrap(), StringEncoding::Utf8).unwrap();
    let (ba, bb): (&str, &str) = (fa.borrow(), fb.borrow());
    let owned_eq = fa == fb;
    assert_eq!(ba == bb, owned_eq);

    kani::cover!(owned_eq && abuf[0] >= 0xC2);
    kani::cover!(!owned_eq && abuf[0] == bbuf[0]);
}

/// The `Borrow<str>` contract also requires `hash(x) == hash(x.borrow())`,
/// so that a `HashSet<FerricString>`/`HashMap<FerricString, _>` can be
/// queried with a `&str`. Checked for both variants, built by `new` from any
/// ASCII content of a fixed length (one harness per length 0..=3; see
/// `check_hash_input` for why lengths are concrete). The hasher records the
/// full call sequence, so equal records mean equal hashes for every hasher.
fn check_borrow_hash<const N: usize>() {
    let bytes: [u8; N] = kani::any();
    kani::assume(bytes.is_ascii());
    let text = std::str::from_utf8(&bytes).unwrap();
    let encoding = if kani::any() {
        StringEncoding::Ascii
    } else {
        StringEncoding::Utf8
    };
    let owned = FerricString::new(text, encoding).unwrap();
    let borrowed: &str = Borrow::borrow(&owned);
    assert!(hash_record(&owned) == hash_record(borrowed));
    kani::cover!(matches!(owned, FerricString::Ascii(_)));
    kani::cover!(matches!(owned, FerricString::Utf8(_)));
}

macro_rules! borrow_hash_harnesses {
    ($($name:ident: $n:literal;)*) => {$(
        #[kani::proof]
        #[kani::unwind(4)]
        fn $name() {
            check_borrow_hash::<$n>();
        }
    )*};
}

borrow_hash_harnesses! {
    borrow_str_hash_agrees_with_owned_len0: 0;
    borrow_str_hash_agrees_with_owned_len1: 1;
    borrow_str_hash_agrees_with_owned_len2: 2;
    borrow_str_hash_agrees_with_owned_len3: 3;
}

/// `as_str` on the public `Ascii` variant does not depend on the
/// documented ASCII-only invariant except through UTF-8 validity: for any
/// raw content of exactly 2 bytes that is valid UTF-8 (including
/// non-ASCII content, which violates the variant's documented invariant),
/// `as_str`, `AsRef<str>` and `Borrow<str>` return exactly that content.
/// (The length is concrete: UTF-8 validation of heap content of symbolic
/// length exceeded the solver's memory.)
#[kani::proof]
#[kani::unwind(4)]
fn ascii_variant_as_str_is_exact_on_utf8_content() {
    let buf: [u8; 2] = kani::any();
    let parsed = std::str::from_utf8(&buf);
    kani::assume(parsed.is_ok());
    let raw = FerricString::Ascii(buf.as_slice().into());
    check_content(&raw, parsed.unwrap());
    kani::cover!(!spec_all_ascii(&buf));
    kani::cover!(spec_all_ascii(&buf));
}

/// Documented-invariant boundary: a raw `Ascii` variant whose content is
/// not valid UTF-8 makes `as_str` panic (its `expect`), for every such
/// 2-byte content. Such values cannot come from `FerricString::new`;
/// snapshot and host boundaries reject them (snapshot.rs:131-132,
/// ferric-rules-runtime host.rs:134).
///
/// `#[kani::should_panic]` alone only shows that *some* content panics;
/// the last cover statement, after the call, must be reported
/// UNREACHABLE, which shows that `as_str` returns for none of them.
#[kani::proof]
#[kani::should_panic]
#[kani::unwind(4)]
fn ascii_variant_as_str_panics_on_invalid_utf8() {
    let buf: [u8; 2] = kani::any();
    kani::assume(std::str::from_utf8(&buf).is_err());
    kani::cover!(buf[0] == 0xFF);
    kani::cover!(buf[0] == 0xC3 && buf[1] < 0x80);
    kani::cover!(buf[0] < 0x80 && buf[1] >= 0x80);
    let raw = FerricString::Ascii(buf.as_slice().into());
    let _ = raw.as_str();
    kani::cover!(
        true,
        "expected UNREACHABLE (every input panics): as_str returned on invalid UTF-8"
    );
}

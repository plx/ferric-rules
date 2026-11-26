//! Kani proof harnesses for `value` (`Value`, `AtomKey`, `Multifield`,
//! `FactAddress`, `ExternalAddress`).
//!
//! Atoms are generated from an abstract description (`AtomDesc`) of every
//! atom kind with symbolic payloads: any symbol id in either pool, any
//! `i64`, any `f64` bit pattern, any (type id, token), the dummy fact
//! address or any (fact key, epoch, timestamp, public index), and strings
//! of length 0, 1 and 2 (arbitrary bytes in the raw `Ascii` representation,
//! ASCII bytes in the `Utf8` representation). The 15 resulting shapes
//! (`shape`) are enumerated concretely while payloads stay symbolic (see
//! the per-harness lists for which shapes and pairs). The specification of
//! identity, `spec_atom_eq`, is written over descriptions from the
//! documentation in value.rs, independently of the code under test.
//!
//! What is proved, on the production code:
//!
//! - `structural_eq_matches_spec_scalars`, `..._fact_addresses`: for every
//!   pair of shapes of the same scalar or fact-address kind, with all
//!   payloads, `Value::structural_eq` and equality of the `AtomKey`s (used
//!   for alpha-memory indexing) both coincide with `spec_atom_eq`, so
//!   constant tests and index lookups always agree; `structural_eq` is
//!   reflexive (including every NaN), symmetric and transitive. This
//!   includes: floats compare by bit pattern (`-0.0 != +0.0`, equal-bit NaNs
//!   equal), symbols compare by (pool, index), fact addresses ignore the
//!   public index.
//! - `atom_key_eq_matches_spec_strings_across_representations`, `..._raw`,
//!   `..._lengths`: STRING `AtomKey` equality coincides with byte equality
//!   (two-byte raw vs two-byte `Utf8`, two-byte raw vs raw, one byte vs two
//!   bytes).
//! - `structural_eq_distinguishes_kinds`, `atom_key_distinguishes_strings`:
//!   for every ordered pair of distinct non-STRING kinds the atoms are never
//!   equal, structurally or as `AtomKey`s (e.g. INTEGER never equals FLOAT,
//!   even when numerically equal; INSTANCE-NAME never equals SYMBOL); a
//!   STRING key never equals an INTEGER or SYMBOL key.
//! - `atom_key_hash_consistent_with_eq`, `..._strings`: for every
//!   non-STRING shape and for two-byte raw strings, equal `AtomKey`s make
//!   identical `Hasher` call sequences, hence equal hashes under any
//!   deterministic hasher (string hashing across representations:
//!   string/kani_proofs.rs `hash_input_is_exactly_byte_content`).
//! - `atom_key_roundtrip_scalars`, `..._fact_addresses`,
//!   `..._empty_strings`, `..._one_byte_strings`, `..._two_byte_strings`:
//!   `from_value` succeeds on every atom with the matching variant
//!   (`FloatBits` = `to_bits`), `TryFrom` agrees, `to_value` and
//!   `From<AtomKey>` return exactly the original atom (variant and every
//!   payload field, including a fact address's public index), and re-keying
//!   gives an equal key.
//! - `type_names_match_clips_names`: `type_name` gives the CLIPS type name
//!   of every shape, VOID and MULTIFIELD; `is_void` holds exactly for VOID.
//! - `atom_key_rejects_exactly_multifield_and_void`: `from_value`/`TryFrom`
//!   fail on VOID, the empty MULTIFIELD and a one-element MULTIFIELD of each
//!   kind, and succeed on an atom of each kind.
//! - `float_and_integer_identity`: the float/integer rules above for all
//!   `f64`/`i64`/`i32` values directly through `From`, plus exact `FloatBits`
//!   round trip.
//! - `multifield_eq_one_element`: for one-element multifields (built with
//!   `FromIterator` and `push`), `==` holds iff the elements are equal per
//!   the atom specification, for every pair of element kinds
//!   (INTEGER/FLOAT/VOID/SYMBOL, symbolic payloads); `len`/`is_empty` are
//!   exact; an empty multifield equals only an empty one.
//! - `fact_address_identity`: `dummy()`/`default()`, accessors, equality on
//!   (epoch, fact key, timestamp) ignoring the public index, and
//!   hash/equality consistency, for all payloads.
//!
//! Hashing is observed with `RecordingHasher` (see string/kani_proofs.rs).
//! Values built here are leaked instead of dropped (see `for_shapes`).
//!
//! Gaps: strings in atoms are bounded to 2 bytes, with non-ASCII content
//! only in the raw `Ascii` representation (non-ASCII `Utf8` content is
//! covered by string/kani_proofs.rs). `Value::structural_eq` is not checked
//! on STRING values at all (its STRING arm is `a == b` on `FerricString`,
//! checked in string/kani_proofs.rs; modeled in
//! verification/verus/value_structural_eq.rs): a single pair of STRING
//! values exceeded the solver's memory, as did 36 pairs, and 5 pairs did not
//! finish within 18 minutes; even four fully concrete STRING values of 1-2
//! bytes compared pairwise (no symbolic data at all) were still in symbolic
//! execution of the MULTIFIELD arm after 10 minutes. STRING `AtomKey`
//! equality is checked for the listed string shapes only. MULTIFIELD equality is checked only
//! for one-element multifields: two-element multifields and
//! `Value::structural_eq` on MULTIFIELD values (nesting) did not finish
//! within 500 s each (CBMC does not resolve the variants of elements read
//! back from a `SmallVec`, and unrolls the recursion through them). The
//! general recursive case (any length, any nesting) is proved on a model in
//! verification/verus/value_structural_eq.rs. `Drop` of values is not
//! exercised; serde impls are not checked; `Value` deliberately has no
//! `Hash`, so only `AtomKey` hashing is covered.

use super::*;
use crate::fact::FactId;
use crate::symbol::SymbolId;

/// An abstract description of an atomic value. Every atom the harnesses
/// build is described by one of these, and the specification of equality
/// (`spec_atom_eq`) is written over descriptions, independently of
/// `Value::structural_eq` and of the derived `AtomKey` equality.
#[derive(Clone, Copy)]
struct AtomDesc {
    /// 0 Symbol, 1 InstanceName, 2 FactAddress, 3 String, 4 Integer,
    /// 5 Float, 6 ExternalAddress.
    tag: u8,
    /// Symbol pool / FactAddress dummy flag / string representation.
    flag: bool,
    /// Symbol index, fact key bits, integer, float bits, or token.
    x: u64,
    /// Epoch or external type id.
    y: u64,
    /// Fact timestamp.
    z: u64,
    /// Fact public index (presentation metadata only).
    w: u64,
    /// String content.
    bytes: [u8; 2],
    len: usize,
}

/// Atom shape `i` (0..=14) as `(kind tag, string length, flag)`: every
/// kind, both symbol pools, dummy and real fact addresses, and strings of
/// each length 0..=2 in both representations. Harnesses enumerate the
/// shapes concretely (`for_shapes`), so values are built without
/// symbolic branching on their structure; payloads stay symbolic. (With a
/// symbolic kind or flag the harnesses did not finish: CBMC then explores
/// the unreachable MULTIFIELD recursion of `structural_eq` for every call.)
fn shape(i: usize) -> (u8, usize, bool) {
    match i {
        0 => (0, 0, true),
        1 => (0, 0, false),
        2 => (1, 0, true),
        3 => (1, 0, false),
        4 => (2, 0, true),
        5 => (2, 0, false),
        6 => (3, 0, true),
        7 => (3, 0, false),
        8 => (3, 1, true),
        9 => (3, 1, false),
        10 => (3, 2, true),
        11 => (3, 2, false),
        12 => (4, 0, false),
        13 => (5, 0, false),
        _ => (6, 0, false),
    }
}

/// An atom of the given shape with a symbolic payload. Strings in the
/// `Ascii` representation hold arbitrary bytes (the variant is public);
/// strings in the `Utf8` representation hold ASCII content (assumed here),
/// which keeps UTF-8 validation out of these harnesses — non-ASCII `Utf8`
/// content is covered by string/kani_proofs.rs.
fn atom_desc_of(shape: (u8, usize, bool)) -> AtomDesc {
    let d = AtomDesc {
        tag: shape.0,
        flag: shape.2,
        x: kani::any(),
        y: kani::any(),
        z: kani::any(),
        w: kani::any(),
        bytes: kani::any(),
        len: shape.1,
    };
    if d.tag == 3 && !d.flag {
        kani::assume(d.bytes[0] < 0x80 && d.bytes[1] < 0x80);
    }
    d
}

fn desc_fact_id(d: &AtomDesc) -> FactId {
    FactId::from(slotmap::KeyData::from_ffi(d.x))
}

#[allow(clippy::cast_possible_truncation)]
fn desc_symbol(d: &AtomDesc) -> Symbol {
    if d.flag {
        Symbol(SymbolId::Ascii(d.x as u32))
    } else {
        Symbol(SymbolId::Utf8(d.x as u32))
    }
}

fn desc_string(d: &AtomDesc) -> FerricString {
    let bytes = &d.bytes[..d.len];
    if d.flag {
        FerricString::Ascii(bytes.into())
    } else {
        // ASCII content (see `atom_desc_of`), hence valid UTF-8.
        FerricString::Utf8(std::str::from_utf8(bytes).unwrap().into())
    }
}

/// Build the value an `AtomDesc` describes.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn build_atom(d: &AtomDesc) -> Value {
    match d.tag {
        0 => Value::Symbol(desc_symbol(d)),
        1 => Value::InstanceName(InstanceName::from_symbol(desc_symbol(d))),
        2 => {
            if d.flag {
                Value::FactAddress(FactAddress::dummy())
            } else {
                Value::FactAddress(FactAddress::new(
                    desc_fact_id(d),
                    d.y,
                    Timestamp::new(d.z),
                    d.w,
                ))
            }
        }
        3 => Value::String(desc_string(d)),
        4 => Value::Integer(d.x as i64),
        5 => Value::Float(f64::from_bits(d.x)),
        _ => Value::ExternalAddress(ExternalAddress {
            type_id: ExternalTypeId(d.y as u32),
            token: d.x,
        }),
    }
}

/// Specification of atom identity, from the documentation in `value.rs`:
/// same kind, and then
/// - SYMBOL / INSTANCE-NAME: the same interned symbol (pool and index);
///   an instance name never equals a symbol with the same spelling;
/// - FACT-ADDRESS: both the dummy address, or the same (epoch, fact key,
///   timestamp); the public index is presentation metadata and ignored;
/// - STRING: exact byte equality, whatever the representation;
/// - INTEGER: the same integer; INTEGER never equals FLOAT;
/// - FLOAT: the same IEEE-754 bit pattern (`-0.0 != +0.0`, a NaN equals
///   a NaN with the same bits);
/// - EXTERNAL-ADDRESS: the same (type id, token).
#[allow(clippy::cast_possible_truncation)]
fn spec_atom_eq(a: &AtomDesc, b: &AtomDesc) -> bool {
    if a.tag != b.tag {
        return false;
    }
    match a.tag {
        0 | 1 => a.flag == b.flag && (a.x as u32) == (b.x as u32),
        2 => {
            if a.flag || b.flag {
                a.flag == b.flag
            } else {
                a.y == b.y && desc_fact_id(a) == desc_fact_id(b) && a.z == b.z
            }
        }
        3 => a.len == b.len && a.bytes[..a.len] == b.bytes[..b.len],
        4 | 5 => a.x == b.x,
        _ => (a.y as u32) == (b.y as u32) && a.x == b.x,
    }
}

/// CLIPS type name for each atom tag.
fn spec_type_name(tag: u8) -> &'static str {
    match tag {
        0 => "SYMBOL",
        1 => "INSTANCE-NAME",
        2 => "FACT-ADDRESS",
        3 => "STRING",
        4 => "INTEGER",
        5 => "FLOAT",
        _ => "EXTERNAL-ADDRESS",
    }
}

/// A `Hasher` that records every call made to it, tagged by method, so two
/// values with equal recordings produce equal hashes under every
/// deterministic `Hasher` that implements the stable `Hasher` methods.
#[derive(Default, Debug)]
struct RecordingHasher {
    calls: Vec<u64>,
}

/// Element-wise comparison of the logs (the derived `Vec<u64>` equality uses
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
    fn record(&mut self, tag: u64, value: u64) {
        self.calls.push(tag);
        self.calls.push(value);
    }
}

#[allow(clippy::cast_sign_loss)]
impl Hasher for RecordingHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.record(0, bytes.len() as u64);
        for &b in bytes {
            self.calls.push(u64::from(b));
        }
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

/// Calls `$f(shape(i))` for each listed shape index, unrolled.
///
/// The harnesses that compare values use the small unwinding bound 3 and
/// are split into groups of shapes: a loop over shapes would need a larger
/// bound, which makes CBMC unroll the (unreachable) MULTIFIELD recursion of
/// `structural_eq` and of the drop glue much deeper, and one harness over
/// all shapes exceeds the solver's memory. For the same reason the values
/// built here are `leak`ed instead of dropped (their drop glue recurses
/// through `SmallVec<[Value; 8]>`); none of the functions under test drops
/// a `Value`.
macro_rules! for_shapes {
    ($f:expr; $($i:expr),+ $(,)?) => {{
        let f = $f;
        $( f(shape($i)); )+
    }};
}

/// Calls `$f(shape(i), shape(j))` for each listed pair, unrolled.
macro_rules! for_shape_pairs {
    ($f:expr, $(($i:expr, $j:expr)),+ $(,)?) => {{
        let f = $f;
        $( f(shape($i), shape($j)); )+
    }};
}

/// Ends a value's life without running its drop glue (see `for_shapes`).
fn leak(v: Value) {
    std::mem::forget(v);
}

/// Independent check that `v` is exactly the atom `d` describes: the
/// variant and every payload field, including a fact address's public
/// index (written against the accessors, not `structural_eq`).
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn matches_desc(v: &Value, d: &AtomDesc) -> bool {
    match (d.tag, v) {
        (0, Value::Symbol(s)) => *s == desc_symbol(d),
        (1, Value::InstanceName(n)) => n.as_symbol() == desc_symbol(d),
        (2, Value::FactAddress(a)) => {
            if d.flag {
                a.is_dummy()
            } else {
                a.fact_id() == Some(desc_fact_id(d))
                    && a.epoch() == Some(d.y)
                    && a.timestamp() == Some(Timestamp::new(d.z))
                    && a.public_index() == Some(d.w)
            }
        }
        (3, Value::String(s)) => s.as_bytes() == &d.bytes[..d.len],
        (4, Value::Integer(i)) => *i == d.x as i64,
        (5, Value::Float(f)) => f.to_bits() == d.x,
        (6, Value::ExternalAddress(e)) => e.type_id == ExternalTypeId(d.y as u32) && e.token == d.x,
        _ => false,
    }
}

fn check_roundtrip(s: (u8, usize, bool)) -> AtomDesc {
    let d = atom_desc_of(s);
    let v = build_atom(&d);
    assert!(matches_desc(&v, &d));
    assert!(!v.is_void());

    let key = AtomKey::from_value(&v).expect("atoms convert to keys");
    assert_eq!(AtomKey::try_from(&v), Ok(key.clone()));
    let variant_ok = match (&key, d.tag) {
        (AtomKey::Symbol(_), 0)
        | (AtomKey::InstanceName(_), 1)
        | (AtomKey::FactAddress(_), 2)
        | (AtomKey::String(_), 3)
        | (AtomKey::Integer(_), 4)
        | (AtomKey::ExternalAddress { .. }, 6) => true,
        (AtomKey::FloatBits(bits), 5) => *bits == d.x,
        _ => false,
    };
    assert!(variant_ok);

    // The values `to_value`/`From` produce are compared field by field with
    // the description: CBMC does not fold the variant of a value built by a
    // `match` on a key, and `structural_eq` on such a value does not finish
    // (identity semantics are covered on directly built values below).
    let back = key.to_value();
    assert!(matches_desc(&back, &d));
    let from = Value::from(key.clone());
    assert!(matches_desc(&from, &d));
    assert_eq!(AtomKey::from_value(&back), Some(key));
    leak(from);
    leak(back);
    leak(v);
    d
}

/// For every atom shape and payload: `AtomKey::from_value` succeeds with the
/// matching key variant (`TryFrom` agrees), `to_value` and `From<AtomKey>`
/// give back exactly the original atom (same variant and every payload
/// field: float bit pattern, symbol, fact-address metadata including the
/// public index), and converting that value again yields an equal key.
/// Shapes: symbols, instance names, integers, floats, external addresses.
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_roundtrip_scalars() {
    for_shapes!(|s| {
        let d = check_roundtrip(s);
        kani::cover!(d.tag == 5 && f64::from_bits(d.x).is_nan());
        kani::cover!(d.tag == 1 && !d.flag && d.x > u64::from(u32::MAX));
    }; 0, 1, 2, 3, 12, 13, 14);
}

/// As `atom_key_roundtrip_scalars`, for dummy and real fact addresses.
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_roundtrip_fact_addresses() {
    for_shapes!(|s| {
        let d = check_roundtrip(s);
        kani::cover!(d.tag == 2 && !d.flag && d.w != 0);
        kani::cover!(d.tag == 2 && d.flag);
    }; 4, 5);
}

/// As `atom_key_roundtrip_scalars`, for empty strings in both
/// representations.
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_roundtrip_empty_strings() {
    for_shapes!(|s| {
        let d = check_roundtrip(s);
        kani::cover!(d.len == 0 && d.flag);
        kani::cover!(d.len == 0 && !d.flag);
    }; 6, 7);
}

/// As `atom_key_roundtrip_scalars`, for one-byte strings in both
/// representations (the content byte is preserved exactly).
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_roundtrip_one_byte_strings() {
    for_shapes!(|s| {
        let d = check_roundtrip(s);
        kani::cover!(d.flag && d.bytes[0] >= 0x80);
        kani::cover!(!d.flag);
    }; 8, 9);
}

/// As `atom_key_roundtrip_scalars`, for strings of length 2 in both
/// representations.
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_roundtrip_two_byte_strings() {
    for_shapes!(|s| {
        let d = check_roundtrip(s);
        kani::cover!(d.flag && d.bytes[0] >= 0x80);
        kani::cover!(!d.flag && d.bytes[0] != d.bytes[1]);
    }; 10, 11);
}

/// `type_name` gives the CLIPS type name of every atom shape, VOID and
/// MULTIFIELD, and `is_void` holds exactly for VOID.
#[kani::proof]
#[kani::unwind(17)]
fn type_names_match_clips_names() {
    for_shapes!(|s| {
        let d = atom_desc_of(s);
        let v = build_atom(&d);
        assert_eq!(v.type_name(), spec_type_name(d.tag));
        assert!(!v.is_void());
        kani::cover!(d.tag == 3 && !d.flag && d.len == 2);
        leak(v);
    }; 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14);
    assert_eq!(Value::Void.type_name(), "VOID");
    assert!(Value::Void.is_void());
    let multifield = Value::Multifield(Box::default());
    assert_eq!(multifield.type_name(), "MULTIFIELD");
    assert!(!multifield.is_void());
    leak(multifield);
}

/// `AtomKey::from_value` rejects exactly `Multifield` and `Void`: it fails
/// on VOID, on the empty MULTIFIELD and on a one-element MULTIFIELD holding
/// an atom of each kind, and succeeds on an atom of each kind.
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_rejects_exactly_multifield_and_void() {
    assert!(AtomKey::from_value(&Value::Void).is_none());
    assert!(AtomKey::try_from(&Value::Void).is_err());
    let empty = Value::Multifield(Box::default());
    assert!(AtomKey::from_value(&empty).is_none());
    assert!(AtomKey::try_from(&empty).is_err());
    leak(empty);

    for_shapes!(|s| {
        let atom = build_atom(&atom_desc_of(s));
        assert!(AtomKey::from_value(&atom).is_some());
        assert!(AtomKey::try_from(&atom).is_ok());
        let single: Multifield = std::iter::once(atom).collect();
        let wrapped = Value::Multifield(Box::new(single));
        assert!(AtomKey::from_value(&wrapped).is_none());
        assert!(AtomKey::try_from(&wrapped).is_err());
        leak(wrapped);
    }; 0, 3, 5, 8, 12, 13, 14);
}

/// What `check_eq_pair` compared, for the harnesses' cover statements.
struct EqOutcome {
    da: AtomDesc,
    db: AtomDesc,
    spec: bool,
    b_eq_c: bool,
}

/// Checks, for atoms `a` of shape `sa` and `b`, `c` of shape `sb`:
/// `structural_eq` and `AtomKey` equality both coincide with `spec_atom_eq`;
/// `structural_eq` is reflexive, symmetric and transitive on them.
fn check_eq_pair(sa: (u8, usize, bool), sb: (u8, usize, bool)) -> EqOutcome {
    let (da, db, dc) = (atom_desc_of(sa), atom_desc_of(sb), atom_desc_of(sb));
    let (a, b, c) = (build_atom(&da), build_atom(&db), build_atom(&dc));
    let spec = spec_atom_eq(&da, &db);

    assert_eq!(a.structural_eq(&b), spec);
    let (ka, kb) = (
        AtomKey::from_value(&a).unwrap(),
        AtomKey::from_value(&b).unwrap(),
    );
    assert_eq!(ka == kb, spec);
    assert_eq!(ka != kb, !spec);

    assert!(a.structural_eq(&a));
    assert_eq!(b.structural_eq(&a), spec);
    let b_eq_c = b.structural_eq(&c);
    if spec && b_eq_c {
        assert!(a.structural_eq(&c));
    }

    leak(a);
    leak(b);
    leak(c);
    EqOutcome {
        da,
        db,
        spec,
        b_eq_c,
    }
}

/// For string shapes (and a string against another kind): equality of the
/// `AtomKey`s of `a` (shape `sa`) and `b` (shape `sb`) coincides with
/// `spec_atom_eq` and is symmetric. `Value::structural_eq` is not called on
/// STRING values: CBMC does not resolve the (niche-encoded) variant of a
/// STRING value, explores the MULTIFIELD recursion of `structural_eq`, and
/// even a single pair exceeded the solver's memory.
fn check_string_key_pair(
    sa: (u8, usize, bool),
    sb: (u8, usize, bool),
) -> (AtomDesc, AtomDesc, bool) {
    let (da, db) = (atom_desc_of(sa), atom_desc_of(sb));
    let (a, b) = (build_atom(&da), build_atom(&db));
    let spec = spec_atom_eq(&da, &db);
    let (ka, kb) = (
        AtomKey::from_value(&a).unwrap(),
        AtomKey::from_value(&b).unwrap(),
    );
    assert_eq!(ka == kb, spec);
    assert_eq!(kb == ka, spec);
    leak(a);
    leak(b);
    (da, db, spec)
}

/// Refinement of atom identity against the documented specification: for
/// every pair of shapes of the same scalar kind (both symbol pools for
/// SYMBOL and INSTANCE-NAME; INTEGER; FLOAT; EXTERNAL-ADDRESS) and all
/// payloads, `Value::structural_eq` and equality of the derived `AtomKey`s
/// (used for alpha-memory indexing) both coincide with `spec_atom_eq`, and
/// `structural_eq` is reflexive, symmetric and transitive. Different kinds:
/// `structural_eq_distinguishes_kinds`.
#[kani::proof]
#[kani::unwind(3)]
fn structural_eq_matches_spec_scalars() {
    for_shape_pairs!(
        |sa, sb| {
            let EqOutcome {
                da,
                db,
                spec,
                b_eq_c,
            } = check_eq_pair(sa, sb);
            // Witnesses of the documented semantics.
            kani::cover!(da.tag == 5 && spec && f64::from_bits(da.x).is_nan());
            kani::cover!(da.tag == 5 && !spec && f64::from_bits(da.x) == f64::from_bits(db.x));
            kani::cover!(da.tag == 0 && da.flag != db.flag && da.x == db.x);
            kani::cover!(da.tag == 1 && spec && b_eq_c);
            kani::cover!(da.tag == 6 && spec);
        },
        (0, 0),
        (0, 1),
        (1, 0),
        (1, 1),
        (2, 2),
        (2, 3),
        (3, 2),
        (3, 3),
        (12, 12),
        (13, 13),
        (14, 14),
    );
}

/// As `structural_eq_matches_spec_scalars`, for every pair of dummy and
/// real fact addresses (the public index is ignored).
#[kani::proof]
#[kani::unwind(3)]
fn structural_eq_matches_spec_fact_addresses() {
    for_shape_pairs!(
        |sa, sb| {
            let EqOutcome { da, db, spec, .. } = check_eq_pair(sa, sb);
            kani::cover!(!da.flag && !db.flag && spec && da.w != db.w);
            kani::cover!(!da.flag && !db.flag && !spec && da.x == db.x);
            kani::cover!(da.flag && db.flag && spec);
        },
        (4, 4),
        (4, 5),
        (5, 4),
        (5, 5)
    );
}

/// STRING keys: `AtomKey` equality is byte equality across
/// representations, for a two-byte raw `Ascii` string (arbitrary bytes)
/// against a two-byte `Utf8` string (see `check_string_key_pair`).
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_eq_matches_spec_strings_across_representations() {
    let (da, db, spec) = check_string_key_pair(shape(10), shape(11));
    kani::cover!(spec);
    kani::cover!(!spec && da.bytes[0] == db.bytes[0]);
}

/// STRING keys over two-byte raw `Ascii` strings with arbitrary (also
/// non-ASCII) bytes.
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_eq_matches_spec_strings_raw() {
    let (da, db, spec) = check_string_key_pair(shape(10), shape(10));
    kani::cover!(spec && da.bytes[0] >= 0x80);
    kani::cover!(!spec && da.bytes[1] == db.bytes[1]);
}

/// STRING keys of different lengths (one byte vs two bytes) are never
/// equal.
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_eq_matches_spec_strings_lengths() {
    let (da, _, spec) = check_string_key_pair(shape(8), shape(11));
    assert!(!spec);
    kani::cover!(da.bytes[0] >= 0x80);
}

/// Atoms of different kinds are never equal, neither structurally nor as
/// `AtomKey`s, for every ordered pair of distinct non-STRING kinds (one
/// shape per kind, symbolic payloads) — e.g. INTEGER never equals FLOAT and
/// SYMBOL never equals INSTANCE-NAME even with the same payload. STRING:
/// `atom_key_distinguishes_strings`.
#[kani::proof]
#[kani::unwind(3)]
fn structural_eq_distinguishes_kinds() {
    for_shapes!(|sa: (u8, usize, bool)| {
        for_shapes!(|sb: (u8, usize, bool)| {
            if sa.0 != sb.0 {
                let EqOutcome { da, db, .. } = check_eq_pair(sa, sb);
                #[allow(clippy::cast_possible_wrap, clippy::cast_precision_loss)]
                let numerically_equal = (da.x as i64) as f64 == f64::from_bits(db.x);
                kani::cover!(da.tag == 4 && db.tag == 5 && numerically_equal);
                kani::cover!(da.tag == 0 && db.tag == 1 && da.x == db.x);
            }
        }; 0, 3, 5, 12, 13, 14);
    }; 0, 3, 5, 12, 13, 14);
}

/// A STRING key never equals the key of an atom of another kind (one-byte
/// string against INTEGER and SYMBOL, both orders).
#[kani::proof]
#[kani::unwind(3)]
fn atom_key_distinguishes_strings() {
    for_shape_pairs!(
        |sa, sb| {
            let (da, db, spec) = check_string_key_pair(sa, sb);
            assert!(!spec);
            #[allow(clippy::cast_possible_wrap)]
            let same_number = da.tag == 3 && db.x as i64 == i64::from(da.bytes[0]);
            kani::cover!(same_number);
        },
        (8, 12),
        (12, 8),
        (8, 0),
        (0, 8),
    );
}

fn check_hash_pair(s: (u8, usize, bool)) -> (AtomDesc, AtomDesc, bool) {
    let (da, db) = (atom_desc_of(s), atom_desc_of(s));
    let (va, vb) = (build_atom(&da), build_atom(&db));
    let ka = AtomKey::from_value(&va).unwrap();
    let kb = AtomKey::from_value(&vb).unwrap();
    leak(va);
    leak(vb);
    let equal = ka == kb;
    if equal {
        assert!(hash_record(&ka) == hash_record(&kb));
    }
    (da, db, equal)
}

/// `AtomKey` hashing is consistent with its equality: for every shape,
/// equal keys feed identical call sequences to any `Hasher` (so
/// `a == b => hash(a) == hash(b)`), including fact addresses that differ
/// only in their public index. Keys of different kinds are never equal
/// (`structural_eq_distinguishes_kinds`); equal strings in the two
/// representations hash identically by `hash_input_is_exactly_byte_content`
/// in string/kani_proofs.rs.
#[kani::proof]
#[kani::unwind(12)]
fn atom_key_hash_consistent_with_eq() {
    for_shapes!(|s| {
        let (da, db, equal) = check_hash_pair(s);
        kani::cover!(equal && da.tag == 2 && !da.flag && da.w != db.w);
        kani::cover!(equal && da.tag == 5 && f64::from_bits(da.x).is_nan());
        kani::cover!(equal && da.tag == 1 && da.x != db.x);
    }; 0, 1, 2, 3, 4, 5, 12, 13, 14);
}

/// As `atom_key_hash_consistent_with_eq`, for two-byte strings in the raw
/// `Ascii` representation (arbitrary bytes).
#[kani::proof]
#[kani::unwind(12)]
fn atom_key_hash_consistent_with_eq_strings() {
    let (da, _, equal) = check_hash_pair(shape(10));
    kani::cover!(equal && da.bytes[1] >= 0x80);
    kani::cover!(!equal);
}

/// Float and integer handling exactly as documented: float identity is the
/// IEEE-754 bit pattern (so `-0.0` and `+0.0` differ, equal-bit NaNs are
/// equal), an INTEGER is never equal to a FLOAT even when numerically
/// equal, the `From` conversions preserve the exact value, and the key's
/// bits round-trip exactly.
#[kani::proof]
fn float_and_integer_identity() {
    let x: f64 = kani::any();
    let y: f64 = kani::any();
    let i: i64 = kani::any();
    let j: i32 = kani::any();
    let fx = Value::from(x);
    let fy = Value::from(y);

    assert_eq!(fx.structural_eq(&fy), x.to_bits() == y.to_bits());
    assert_eq!(
        AtomKey::from_value(&fx),
        Some(AtomKey::FloatBits(x.to_bits()))
    );
    match AtomKey::FloatBits(x.to_bits()).to_value() {
        Value::Float(back) => assert_eq!(back.to_bits(), x.to_bits()),
        _ => panic!("FloatBits must convert back to a FLOAT"),
    }
    assert!(!Value::from(i).structural_eq(&fx));
    assert!(!fx.structural_eq(&Value::from(i)));
    assert!(Value::from(j).structural_eq(&Value::Integer(i64::from(j))));
    assert!(matches!(Value::from(i), Value::Integer(v) if v == i));

    kani::cover!(x == 0.0 && y == 0.0 && x.to_bits() != y.to_bits());
    kani::cover!(x.is_nan() && x.to_bits() == y.to_bits());
    kani::cover!(x.is_nan() && y.is_nan() && x.to_bits() != y.to_bits());
    #[allow(clippy::cast_precision_loss)]
    let numerically_equal = (i as f64) == x;
    kani::cover!(numerically_equal && i == 1);
    kani::cover!(j < 0);
}

/// Element descriptions for multifield harnesses: INTEGER, FLOAT, VOID or
/// SYMBOL with a symbolic payload.
#[derive(Clone, Copy)]
struct ElemDesc {
    tag: u8,
    x: u64,
}

/// An element of kind `tag` (0 INTEGER, 1 FLOAT, 2 VOID, 3 SYMBOL) with a
/// symbolic payload; kinds are concrete for the same reason as `shape`.
fn elem_of(tag: u8) -> ElemDesc {
    ElemDesc {
        tag,
        x: kani::any(),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn build_elem(d: &ElemDesc) -> Value {
    match d.tag {
        0 => Value::Integer(d.x as i64),
        1 => Value::Float(f64::from_bits(d.x)),
        2 => Value::Void,
        _ => Value::Symbol(Symbol(SymbolId::Ascii(d.x as u32))),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn spec_elem_eq(a: &ElemDesc, b: &ElemDesc) -> bool {
    a.tag == b.tag
        && match a.tag {
            0 | 1 => a.x == b.x,
            2 => true,
            _ => (a.x as u32) == (b.x as u32),
        }
}

/// Calls `$f(i, j)` for every pair of element kinds, unrolled.
macro_rules! for_elem_kind_pairs {
    ($f:expr) => {{
        let f = $f;
        f(0, 0);
        f(0, 1);
        f(0, 2);
        f(0, 3);
        f(1, 0);
        f(1, 1);
        f(1, 2);
        f(1, 3);
        f(2, 0);
        f(2, 1);
        f(2, 2);
        f(2, 3);
        f(3, 0);
        f(3, 1);
        f(3, 2);
        f(3, 3);
    }};
}

/// One-element multifields (built with `FromIterator` and with `push`):
/// `==` holds iff the elements are equal per the atom specification (VOID
/// equals VOID, floats by bit pattern, different kinds never), for every
/// pair of element kinds; `len`/`is_empty` are exact; an empty multifield
/// equals only an empty one.
#[kani::proof]
#[kani::unwind(2)]
fn multifield_eq_one_element() {
    for_elem_kind_pairs!(|ta, tb| {
        let (da, db) = (elem_of(ta), elem_of(tb));
        let ma: Multifield = std::iter::once(build_elem(&da)).collect();
        let mut mb = Multifield::new();
        mb.push(build_elem(&db));
        let spec = spec_elem_eq(&da, &db);
        assert_eq!(ma == mb, spec);
        assert_eq!(mb == ma, spec);
        assert!(ma.len() == 1 && !ma.is_empty());
        kani::cover!(spec && da.tag == 1 && f64::from_bits(da.x).is_nan());
        kani::cover!(!spec && da.tag == 1 && db.tag == 1 && f64::from_bits(da.x) == 0.0);
        kani::cover!(spec && da.tag == 2);
        leak(Value::Multifield(Box::new(ma)));
        leak(Value::Multifield(Box::new(mb)));
    });
    let empty = Multifield::new();
    assert!(empty.is_empty() && empty.len() == 0);
    assert!(empty == Multifield::default());
    let single: Multifield = std::iter::once(build_elem(&elem_of(2))).collect();
    assert!(empty != single);
    assert!(single != empty);
    leak(Value::Multifield(Box::new(single)));
}

/// FACT-ADDRESS identity and accessors: `dummy()`/`default()` is the
/// unique dummy; accessors return the construction metadata; two addresses
/// are equal iff both are dummy or they agree on (epoch, fact key,
/// timestamp) — the public index is ignored — and equal addresses hash
/// identically.
#[kani::proof]
#[kani::unwind(10)]
fn fact_address_identity() {
    let dummy_a: bool = kani::any();
    let dummy_b: bool = kani::any();
    let key_a = FactId::from(slotmap::KeyData::from_ffi(kani::any()));
    let key_b = FactId::from(slotmap::KeyData::from_ffi(kani::any()));
    let (ea, eb): (u64, u64) = (kani::any(), kani::any());
    let (ta, tb) = (Timestamp::new(kani::any()), Timestamp::new(kani::any()));
    let (pa, pb): (u64, u64) = (kani::any(), kani::any());
    let a = if dummy_a {
        FactAddress::dummy()
    } else {
        FactAddress::new(key_a, ea, ta, pa)
    };
    let b = if dummy_b {
        FactAddress::default()
    } else {
        FactAddress::new(key_b, eb, tb, pb)
    };

    assert_eq!(a.is_dummy(), dummy_a);
    if dummy_a {
        assert!(a.fact_id().is_none() && a.epoch().is_none());
        assert!(a.timestamp().is_none() && a.public_index().is_none());
    } else {
        assert_eq!(a.fact_id(), Some(key_a));
        assert_eq!(a.epoch(), Some(ea));
        assert_eq!(a.timestamp(), Some(ta));
        assert_eq!(a.public_index(), Some(pa));
    }

    let spec = if dummy_a || dummy_b {
        dummy_a == dummy_b
    } else {
        ea == eb && key_a == key_b && ta == tb
    };
    assert_eq!(a == b, spec);
    assert!(a.clone() == a);
    if spec {
        assert!(hash_record(&a) == hash_record(&b));
    }

    kani::cover!(spec && !dummy_a && pa != pb);
    kani::cover!(!spec && !dummy_a && !dummy_b && key_a == key_b && ea == eb);
    kani::cover!(spec && dummy_a);
}

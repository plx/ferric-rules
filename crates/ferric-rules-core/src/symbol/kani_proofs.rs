//! Kani proof harnesses for `symbol` (`SymbolTable`, `Symbol`,
//! `InstanceName`).
//!
//! What is proved, on the production code:
//!
//! - `resolve_str_on_ascii_pool_entry`, `resolve_str_on_ascii_pool_out_of_range`,
//!   `resolve_str_is_total_on_utf8_pool`: for every `u32` index,
//!   `resolve_str`/`resolve_symbol_str` return exactly the stored string
//!   when the index is in range (and, in the ASCII pool, the stored bytes
//!   are valid UTF-8 — a corrupt entry of 2 arbitrary bytes yields
//!   `None`) and `None` otherwise, without panicking; `resolve` /
//!   `resolve_symbol` return the stored bytes in range. (Out-of-range ASCII
//!   indices are checked against a fixed entry.)
//! - `resolve_panics_on_out_of_range_id`: `resolve` panics, as documented,
//!   for every out-of-range id in either pool.
//! - `checkpoint_records_pool_lengths`: `checkpoint` records the pool sizes
//!   and `len`/`is_empty` report their sum [pool sizes up to (3, 2)].
//! - `restore_of_current_checkpoint_is_identity` (fixed table) and
//!   `restore_rejects_newer_checkpoint` (`restore` panics, per its
//!   documented `assert!`, for every checkpoint newer than the table in
//!   either pool).
//! - `instance_name_semantics`: `from_symbol`/`as_symbol` are inverse,
//!   instance-name equality is symbol equality, hashing agrees with
//!   equality, and an INSTANCE-NAME value never equals the SYMBOL value of
//!   the same interned spelling, structurally or as an `AtomKey`.
//! - `symbol_identity_is_pool_and_index`: symbols are equal iff pool and
//!   index match (the same index in the two pools differs); hashing agrees.
//!
//! The tables used here are built by pushing directly onto the pool vectors
//! (`table_with`); the functions checked read only the vectors.
//!
//! Gaps: interning (`intern_symbol`, `intern_ascii`, `intern_utf8`),
//! `find_symbol` on non-empty pools and `restore` with removals go through
//! `FxHashMap` probing, which is intractable under Kani 0.68 here: a single
//! `intern_ascii(b"a")` into an empty table did not finish within 9 minutes
//! (unwind 18), and two concrete interns did not finish within 10 minutes
//! (unwind 10). Interning, idempotence, injectivity, `find_symbol`
//! agreement, encoding enforcement in both directions, and exact
//! checkpoint/restore are proved instead, unboundedly, on a model in
//! verification/verus/symbol_table.rs. `find_symbol` on an empty table is
//! checked on production code in encoding/kani_proofs.rs
//! (`mode_policy_*`).

use super::*;
use crate::value::{AtomKey, Value};
use std::hash::{Hash, Hasher};

fn any_symbol_id() -> SymbolId {
    if kani::any() {
        SymbolId::Ascii(kani::any())
    } else {
        SymbolId::Utf8(kani::any())
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

/// A table whose pools hold the given contents. Only the vectors are
/// populated: `resolve`, `resolve_str`, `len`, `checkpoint` and a no-op
/// `restore` read nothing else, so the (empty) maps are irrelevant to the
/// properties checked with it. The ASCII pool may hold arbitrary bytes to
/// cover corrupt (e.g. deserialized) tables.
fn table_with(ascii: &[&[u8]], utf8: &[&str]) -> SymbolTable {
    let mut table = SymbolTable::new();
    for s in ascii {
        table.ascii_strings.push((*s).into());
    }
    for s in utf8 {
        table.utf8_strings.push((*s).into());
    }
    table
}

/// `resolve_str` / `resolve_symbol_str` are exact on an in-range ASCII-pool
/// id: they return `Some(stored)` iff the stored bytes are valid UTF-8 (a
/// corrupt, e.g. deserialized, entry yields `None` instead of panicking),
/// and `resolve` / `resolve_symbol` return the stored bytes. Pool: one
/// arbitrary entry of exactly 2 bytes (a symbolic length exceeded the
/// solver's memory).
#[kani::proof]
#[kani::unwind(4)]
fn resolve_str_on_ascii_pool_entry() {
    let abuf: [u8; 2] = kani::any();
    let entry = &abuf[..];
    let entry_is_utf8 = std::str::from_utf8(entry).is_ok();
    let table = table_with(&[entry], &[]);

    let id = SymbolId::Ascii(0);
    let resolved = table.resolve_str(id);
    if entry_is_utf8 {
        assert_eq!(resolved.map(str::as_bytes), Some(entry));
    } else {
        assert_eq!(resolved, None);
    }
    assert_eq!(table.resolve_symbol_str(Symbol(id)), resolved);
    assert_eq!(table.resolve(id), entry);
    assert_eq!(table.resolve_symbol(Symbol(id)), entry);

    kani::cover!(!entry_is_utf8);
    kani::cover!(entry_is_utf8 && abuf[0] >= 0x80);
}

/// `resolve_str` / `resolve_symbol_str` return `None`, without panicking,
/// for every out-of-range ASCII-pool index (pool of one entry). (With a
/// symbolic entry as well, this exceeded the solver's memory: CBMC builds
/// the UTF-8 validation of the entry read through the symbolic index.)
#[kani::proof]
#[kani::unwind(4)]
fn resolve_str_on_ascii_pool_out_of_range() {
    let table = table_with(&[b"ok"], &[]);
    let index: u32 = kani::any_where(|&i: &u32| i >= 1);
    let id = SymbolId::Ascii(index);
    assert_eq!(table.resolve_str(id), None);
    assert_eq!(table.resolve_symbol_str(Symbol(id)), None);
    kani::cover!(index == 1);
    kani::cover!(index == u32::MAX);
}

/// The same for the UTF-8 pool (one fixed non-ASCII entry): `Some(stored)`
/// iff the index is in range, else `None`; never panics.
#[kani::proof]
#[kani::unwind(7)]
fn resolve_str_is_total_on_utf8_pool() {
    let table = table_with(&[b"ascii"], &["\u{e9}t\u{e9}"]);
    let index: u32 = kani::any();
    let id = SymbolId::Utf8(index);
    let resolved = table.resolve_str(id);
    if index == 0 {
        assert_eq!(resolved, Some("\u{e9}t\u{e9}"));
        assert_eq!(table.resolve(id), "\u{e9}t\u{e9}".as_bytes());
    } else {
        assert_eq!(resolved, None);
    }
    assert_eq!(table.resolve_symbol_str(Symbol(id)), resolved);
    assert_eq!(table.len(), 2);
    kani::cover!(index == 0);
    kani::cover!(index == 1);
}

/// `resolve` panics (as documented) on an id outside its pool.
#[kani::proof]
#[kani::should_panic]
#[kani::unwind(3)]
fn resolve_panics_on_out_of_range_id() {
    let table = table_with(&[b"a"], &["b"]);
    let id = any_symbol_id();
    let in_range = matches!(id, SymbolId::Ascii(0) | SymbolId::Utf8(0));
    kani::assume(!in_range);
    let _ = table.resolve(id);
}

/// `checkpoint` records the pool lengths and `len`/`is_empty` report their
/// sum, for every combination of pool sizes up to (3, 2).
#[kani::proof]
#[kani::unwind(4)]
fn checkpoint_records_pool_lengths() {
    let three = [b"a".as_slice(), b"b", b"c"];
    let n_ascii: usize = kani::any_where(|&n: &usize| n <= 3);
    let n_utf8: usize = kani::any_where(|&n: &usize| n <= 2);
    let table = table_with(&three[..n_ascii], &["x", "y"][..n_utf8]);
    let cp = table.checkpoint();
    assert_eq!(
        cp,
        SymbolTableCheckpoint {
            ascii_len: n_ascii,
            utf8_len: n_utf8
        }
    );
    assert_eq!(table.len(), n_ascii + n_utf8);
    assert_eq!(table.is_empty(), n_ascii + n_utf8 == 0);
    kani::cover!(n_ascii == 3 && n_utf8 == 2);
    kani::cover!(n_ascii == 0 && n_utf8 == 0);
}

/// `restore` of the current checkpoint leaves a fixed table unchanged.
/// (With symbolic pool sizes CBMC unrolls `restore`'s loop bodies, whose
/// hash-map `remove` is intractable, even though they cannot execute; the
/// general checkpoint/restore theorem is in
/// verification/verus/symbol_table.rs.)
#[kani::proof]
#[kani::unwind(4)]
fn restore_of_current_checkpoint_is_identity() {
    let mut table = table_with(&[b"a", b"b"], &["x"]);
    let cp = table.checkpoint();
    table.restore(cp);
    assert_eq!(table.checkpoint(), cp);
    assert_eq!(table.resolve(SymbolId::Ascii(0)), b"a");
    assert_eq!(table.resolve(SymbolId::Ascii(1)), b"b");
    assert_eq!(table.resolve(SymbolId::Utf8(0)), b"x");
}

/// `restore` panics (its documented `assert!`) exactly when the checkpoint
/// is newer than the table in either pool.
#[kani::proof]
#[kani::should_panic]
#[kani::unwind(3)]
fn restore_rejects_newer_checkpoint() {
    let mut table = table_with(&[b"a"], &["b"]);
    let cp = SymbolTableCheckpoint {
        ascii_len: kani::any(),
        utf8_len: kani::any(),
    };
    kani::assume(cp.ascii_len > 1 || cp.utf8_len > 1);
    kani::cover!(cp.ascii_len <= 1);
    kani::cover!(cp.utf8_len <= 1);
    table.restore(cp);
}

/// `InstanceName` semantics (symbol.rs: "distinct from a SYMBOL with the
/// same text"): `from_symbol`/`as_symbol` are inverse, instance names are
/// equal iff their spellings' symbols are, hashing agrees with equality,
/// and an INSTANCE-NAME value never equals a SYMBOL value, even with the
/// same interned spelling, neither structurally nor as an alpha-memory key.
#[kani::proof]
fn instance_name_semantics() {
    let a = Symbol(any_symbol_id());
    let b = Symbol(any_symbol_id());
    let na = InstanceName::from_symbol(a);
    let nb = InstanceName::from_symbol(b);

    assert_eq!(na.as_symbol(), a);
    assert_eq!(na == nb, a == b);
    if na == nb {
        assert!(hash_record(&na) == hash_record(&nb));
    }

    let as_instance = Value::InstanceName(na);
    let as_symbol = Value::Symbol(a);
    assert!(!as_instance.structural_eq(&as_symbol));
    assert!(!as_symbol.structural_eq(&as_instance));
    assert_ne!(
        AtomKey::from_value(&as_instance),
        AtomKey::from_value(&as_symbol)
    );
    assert_eq!(as_instance.type_name(), "INSTANCE-NAME");
    assert_eq!(as_symbol.type_name(), "SYMBOL");
    assert_eq!(as_instance.structural_eq(&Value::InstanceName(nb)), a == b);

    kani::cover!(na == nb);
    kani::cover!(na != nb);
}

/// Symbol identity is (pool, index): equal iff both match (so the same
/// index in the two pools denotes different symbols), and hashing agrees
/// with equality.
#[kani::proof]
fn symbol_identity_is_pool_and_index() {
    let ia = any_symbol_id();
    let ib = any_symbol_id();
    let spec = match (ia, ib) {
        (SymbolId::Ascii(x), SymbolId::Ascii(y)) | (SymbolId::Utf8(x), SymbolId::Utf8(y)) => x == y,
        _ => false,
    };
    assert_eq!(Symbol(ia) == Symbol(ib), spec);
    if spec {
        assert!(hash_record(&Symbol(ia)) == hash_record(&Symbol(ib)));
    }
    kani::cover!(matches!((ia, ib), (SymbolId::Ascii(x), SymbolId::Utf8(y)) if x == y));
    kani::cover!(spec);
}

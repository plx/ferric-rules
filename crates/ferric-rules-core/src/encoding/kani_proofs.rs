//! Kani proof harnesses for `encoding` (`StringEncoding` enforcement).
//!
//! `StringEncoding` itself has no validation code: the modes are enforced by
//! `FerricString::new` (string.rs) and `SymbolTable::intern_symbol` /
//! `find_symbol` (symbol.rs) through `str::is_ascii`, on inputs that are
//! already `&str`, i.e. accepted by `std::str::from_utf8`. `EncodingError`
//! carries the whole offending input, not a byte position.
//!
//! What is proved, on the production code:
//!
//! - `utf8_acceptance_matches_unicode_well_formedness`: for every byte
//!   string of length <= 4, `std::str::from_utf8` accepts exactly the
//!   sequences an independent oracle written from Unicode Table 3-7 accepts,
//!   with the oracle's error position (`valid_up_to`) and error length
//!   (`error_len`, maximal ill-formed subpart or `None` for truncation);
//!   accepted sequences are stored byte-exactly by `FerricString::new`. So
//!   the UTF-8 modes hold exactly the well-formed UTF-8 byte sequences.
//! - `is_ascii_matches_spec`: `str::is_ascii` and `[u8]::is_ascii` are exactly
//!   "every byte < 0x80" [length <= 8].
//! - `mode_policy_ascii`, `mode_policy_utf8`,
//!   `mode_policy_ascii_symbols_utf8_strings` (one harness per mode):
//!   `StringEncoding::default()` is `Utf8`; for every
//!   `&str` of byte length <= 4 and every mode, `FerricString::new` accepts
//!   iff the mode allows non-ASCII strings or the input is ASCII, else
//!   returns `NonAsciiString(input)`; whenever the mode keeps symbols ASCII
//!   and the input is not ASCII, `find_symbol` returns `None` without
//!   panicking.
//!
//! Gaps: `intern_symbol` is not checked on the production code in either
//! direction. Its accepting direction inserts into an `FxHashMap`, which
//! does not finish under Kani even for one concrete string, and CBMC
//! unrolls that direction even for inputs that are non-ASCII (constrained
//! symbolic inputs, and also concrete literals: a harness rejecting three
//! concrete non-ASCII literals did not finish within 10 minutes). Both
//! directions are proved, for all inputs, on the model in
//! verification/verus/symbol_table.rs (`theorem_intern_symbol`). Inputs are
//! bounded as stated.

use super::*;
use crate::string::FerricString;
use crate::symbol::SymbolTable;

/// A symbolic byte string: `N` symbolic bytes and a symbolic length `<= N`.
fn any_bytes<const N: usize>() -> ([u8; N], usize) {
    let bytes: [u8; N] = kani::any();
    let len: usize = kani::any_where(|&l: &usize| l <= N);
    (bytes, len)
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

/// Independent UTF-8 well-formedness oracle, written from Table 3-7
/// ("Well-Formed UTF-8 Byte Sequences") of the Unicode Standard, with the
/// error convention documented for `std::str::Utf8Error`:
/// returns `None` if `bytes` is well formed, otherwise
/// `Some((valid_up_to, error_len))` where `valid_up_to` is the start of the
/// first ill-formed sequence and `error_len` is `None` if the input ends in
/// the middle of a sequence that could still be completed, or
/// `Some(n)` for the length of the maximal ill-formed subpart.
fn spec_utf8_error(bytes: &[u8]) -> Option<(usize, Option<u8>)> {
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        // (sequence length, allowed range of the second byte)
        let (n, lo, hi): (usize, u8, u8) = match b0 {
            0x00..=0x7F => (1, 0, 0),
            0xC2..=0xDF => (2, 0x80, 0xBF),
            0xE0 => (3, 0xA0, 0xBF),
            0xE1..=0xEC | 0xEE..=0xEF => (3, 0x80, 0xBF),
            0xED => (3, 0x80, 0x9F),
            0xF0 => (4, 0x90, 0xBF),
            0xF1..=0xF3 => (4, 0x80, 0xBF),
            0xF4 => (4, 0x80, 0x8F),
            _ => return Some((i, Some(1))),
        };
        let mut k = 1;
        while k < n {
            if i + k >= bytes.len() {
                return Some((i, None));
            }
            let b = bytes[i + k];
            let (l, h) = if k == 1 { (lo, hi) } else { (0x80, 0xBF) };
            if b < l || b > h {
                #[allow(clippy::cast_possible_truncation)]
                return Some((i, Some(k as u8)));
            }
            k += 1;
        }
        i += n;
    }
    None
}

/// The set of byte sequences the UTF-8 modes can hold is exactly the set of
/// well-formed UTF-8 sequences (Unicode Table 3-7), for every byte string of
/// length <= 4: `str::from_utf8` (the gate used by `FerricString::as_str`
/// and `SymbolTable::resolve_str`, and the `&str` invariant every
/// constructor relies on) accepts exactly the oracle's sequences, reports
/// the oracle's error position (`valid_up_to`) and error length
/// (`error_len`), and every accepted sequence is stored byte-exactly by
/// `FerricString::new` in `Utf8` mode (`AsciiSymbolsUtf8Strings` takes the
/// same branch of `new`; see `mode_policy_*`).
#[kani::proof]
#[kani::unwind(6)]
fn utf8_acceptance_matches_unicode_well_formedness() {
    let (buf, len) = any_bytes::<4>();
    let bytes = &buf[..len];
    let spec = spec_utf8_error(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => {
            assert!(spec.is_none());
            let stored = FerricString::new(s, StringEncoding::Utf8).unwrap();
            assert_eq!(stored.as_bytes(), bytes);
        }
        Err(e) => {
            let (pos, err_len) = spec.expect("from_utf8 rejected a well-formed sequence");
            assert_eq!(e.valid_up_to(), pos);
            assert_eq!(e.error_len(), err_len.map(usize::from));
        }
    }

    // Each row of Table 3-7 and each error class is reached.
    kani::cover!(len == 4 && buf[0] == 0xF0 && spec.is_none());
    kani::cover!(len == 4 && buf[0] == 0xF4 && spec.is_none());
    kani::cover!(len == 3 && buf[0] == 0xED && spec.is_none());
    kani::cover!(len == 3 && buf[0] == 0xE0 && spec.is_none());
    kani::cover!(len == 2 && buf[0] == 0xC2 && spec.is_none());
    kani::cover!(len == 3 && buf[0] == 0xED && spec == Some((0, Some(1))));
    kani::cover!(len == 2 && buf[0] == 0xC0 && spec == Some((0, Some(1))));
    kani::cover!(len == 4 && buf[0] == 0xF5);
    kani::cover!(spec == Some((1, None)));
    kani::cover!(spec == Some((0, Some(2))));
    kani::cover!(spec == Some((0, Some(3))));
    kani::cover!(spec == Some((1, Some(1))));
}

/// `str::is_ascii`, which both encoding checks use, is exactly "every byte
/// is below 0x80", for every `&str` of byte length <= 8.
#[kani::proof]
#[kani::unwind(10)]
fn is_ascii_matches_spec() {
    let (buf, len) = any_bytes::<8>();
    let bytes = &buf[..len];
    if let Ok(s) = std::str::from_utf8(bytes) {
        assert_eq!(s.is_ascii(), spec_all_ascii(bytes));
        kani::cover!(len == 8 && s.is_ascii());
        kani::cover!(len == 8 && !s.is_ascii() && buf[7] >= 0x80);
    }
    assert_eq!(bytes.is_ascii(), spec_all_ascii(bytes));
    kani::cover!(len == 8 && !spec_all_ascii(bytes) && buf[0] < 0x80);
}

/// Checks the per-mode policy table for `mode`, for every `&str` of byte
/// length <= 4:
///
/// | mode                      | strings      | symbols      |
/// |---------------------------|--------------|--------------|
/// | `Ascii`                   | ASCII only   | ASCII only   |
/// | `Utf8` (the default)      | any `&str`   | any `&str`   |
/// | `AsciiSymbolsUtf8Strings` | any `&str`   | ASCII only   |
///
/// Strings: `FerricString::new` accepts iff the table allows, and rejects
/// with `NonAsciiString(input)`. Symbols: whenever the table forbids,
/// `find_symbol` returns `None` (on an empty table every lookup is `None`,
/// so this checks only that it does not panic). `intern_symbol` is not
/// called: CBMC also unrolls its accepting path, whose hash-map insertion
/// does not finish, even for concrete non-ASCII inputs (see the module
/// doc). Returns (input length, input is ASCII) for the cover statements.
fn check_mode_policy(mode: StringEncoding) -> (usize, bool) {
    let (buf, len) = any_bytes::<4>();
    let bytes = &buf[..len];
    let parsed = std::str::from_utf8(bytes);
    kani::assume(parsed.is_ok());
    let s = parsed.unwrap();
    let ascii = spec_all_ascii(bytes);

    let strings_ascii_only = mode == StringEncoding::Ascii;
    let symbols_ascii_only = mode != StringEncoding::Utf8;

    match FerricString::new(s, mode) {
        Ok(v) => {
            assert!(ascii || !strings_ascii_only);
            assert_eq!(v.as_bytes(), bytes);
        }
        Err(e) => {
            assert!(strings_ascii_only && !ascii);
            assert_eq!(e, EncodingError::NonAsciiString(s.to_string()));
        }
    }

    if symbols_ascii_only && !ascii {
        let table = SymbolTable::new();
        assert_eq!(table.find_symbol(s, mode), None);
    }
    (len, ascii)
}

/// The policy table row for `Ascii` (see `check_mode_policy`).
#[kani::proof]
#[kani::unwind(6)]
fn mode_policy_ascii() {
    let (len, ascii) = check_mode_policy(StringEncoding::Ascii);
    kani::cover!(!ascii && len == 4);
    kani::cover!(ascii && len == 4);
}

/// The policy table row for `Utf8`, which is also the default mode (see
/// `check_mode_policy`).
#[kani::proof]
#[kani::unwind(6)]
fn mode_policy_utf8() {
    assert_eq!(StringEncoding::default(), StringEncoding::Utf8);
    let (len, ascii) = check_mode_policy(StringEncoding::Utf8);
    kani::cover!(!ascii && len == 4);
    kani::cover!(ascii && len == 2);
}

/// The policy table row for `AsciiSymbolsUtf8Strings` (see
/// `check_mode_policy`).
#[kani::proof]
#[kani::unwind(6)]
fn mode_policy_ascii_symbols_utf8_strings() {
    let (len, ascii) = check_mode_policy(StringEncoding::AsciiSymbolsUtf8Strings);
    kani::cover!(!ascii && len == 4);
    kani::cover!(ascii && len == 3);
}

//! Symbol interning with encoding awareness.
//!
//! Symbols are interned strings that are cheap to copy and compare.
//! A `SymbolId` is only valid within the `SymbolTable` (and thus the engine)
//! that created it.

use std::collections::HashMap;

use crate::encoding::{EncodingError, StringEncoding};

/// An interned symbol — always cheap to copy and compare.
///
/// The `SymbolId` inside is only valid within the engine that created it.
/// Comparing symbols from different engines is undefined behavior at the
/// semantic level (though it won't cause memory unsafety).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Symbol(pub(crate) SymbolId);

/// Internal symbol identifier, distinguishing ASCII and UTF-8 interning pools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SymbolId {
    Ascii(u32),
    Utf8(u32),
}

/// Symbol interning table with encoding awareness.
///
/// Maintains two separate pools (ASCII and UTF-8) to support the
/// `AsciiSymbolsUtf8Strings` encoding mode, where symbols are ASCII-only
/// even though strings may be UTF-8.
pub struct SymbolTable {
    /// ASCII symbols (used in `Ascii` and `AsciiSymbolsUtf8Strings` modes)
    ascii_to_id: HashMap<Box<[u8]>, u32>,
    ascii_strings: Vec<Box<[u8]>>,

    /// UTF-8 symbols (used in `Utf8` mode)
    utf8_to_id: HashMap<Box<str>, u32>,
    utf8_strings: Vec<Box<str>>,
}

impl SymbolTable {
    /// Create a new, empty symbol table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ascii_to_id: HashMap::new(),
            ascii_strings: Vec::new(),
            utf8_to_id: HashMap::new(),
            utf8_strings: Vec::new(),
        }
    }

    /// Intern an ASCII byte string, returning a stable `SymbolId`.
    ///
    /// If the string was already interned, returns the existing ID.
    #[allow(clippy::cast_possible_truncation)] // Symbol count will never reach u32::MAX in practice.
    pub(crate) fn intern_ascii(&mut self, s: &[u8]) -> SymbolId {
        if let Some(&id) = self.ascii_to_id.get(s) {
            return SymbolId::Ascii(id);
        }
        let id = self.ascii_strings.len() as u32;
        let boxed: Box<[u8]> = s.into();
        self.ascii_to_id.insert(boxed.clone(), id);
        self.ascii_strings.push(boxed);
        SymbolId::Ascii(id)
    }

    /// Intern a UTF-8 string, returning a stable `SymbolId`.
    ///
    /// If the string was already interned, returns the existing ID.
    #[allow(clippy::cast_possible_truncation)] // Symbol count will never reach u32::MAX in practice.
    pub(crate) fn intern_utf8(&mut self, s: &str) -> SymbolId {
        if let Some(&id) = self.utf8_to_id.get(s) {
            return SymbolId::Utf8(id);
        }
        let id = self.utf8_strings.len() as u32;
        let boxed: Box<str> = s.into();
        self.utf8_to_id.insert(boxed.clone(), id);
        self.utf8_strings.push(boxed);
        SymbolId::Utf8(id)
    }

    /// Resolve a `SymbolId` back to its byte representation.
    ///
    /// # Panics
    ///
    /// Panics if the `SymbolId` is not valid for this table.
    #[must_use]
    pub(crate) fn resolve(&self, id: SymbolId) -> &[u8] {
        match id {
            SymbolId::Ascii(i) => &self.ascii_strings[i as usize],
            SymbolId::Utf8(i) => self.utf8_strings[i as usize].as_bytes(),
        }
    }

    /// Resolve a `SymbolId` to a `&str`, if possible.
    ///
    /// # Panics
    ///
    /// Panics if the `SymbolId` is not valid for this table.
    #[must_use]
    pub(crate) fn resolve_str(&self, id: SymbolId) -> Option<&str> {
        match id {
            SymbolId::Ascii(i) => std::str::from_utf8(&self.ascii_strings[i as usize]).ok(),
            SymbolId::Utf8(i) => Some(&self.utf8_strings[i as usize]),
        }
    }

    /// Returns the total number of interned symbols (across both pools).
    #[must_use]
    pub fn len(&self) -> usize {
        self.ascii_strings.len() + self.utf8_strings.len()
    }

    /// Returns `true` if no symbols have been interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Resolve a [`Symbol`] back to its byte representation.
    ///
    /// # Panics
    ///
    /// Panics if the `Symbol` is not valid for this table.
    #[must_use]
    pub fn resolve_symbol(&self, sym: Symbol) -> &[u8] {
        self.resolve(sym.0)
    }

    /// Resolve a [`Symbol`] to a `&str`, if possible.
    ///
    /// # Panics
    ///
    /// Panics if the `Symbol` is not valid for this table.
    #[must_use]
    pub fn resolve_symbol_str(&self, sym: Symbol) -> Option<&str> {
        self.resolve_str(sym.0)
    }

    /// Intern a symbol with encoding enforcement.
    ///
    /// This is the encoding-checked constructor for symbols.
    pub fn intern_symbol(
        &mut self,
        s: &str,
        encoding: StringEncoding,
    ) -> Result<Symbol, EncodingError> {
        match encoding {
            StringEncoding::Ascii | StringEncoding::AsciiSymbolsUtf8Strings => {
                if !s.is_ascii() {
                    return Err(EncodingError::NonAsciiSymbol(s.to_string()));
                }
                Ok(Symbol(self.intern_ascii(s.as_bytes())))
            }
            StringEncoding::Utf8 => Ok(Symbol(self.intern_utf8(s))),
        }
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ---- Property-based tests -----------------------------------------------

    proptest! {
        /// Interning an ASCII byte string and resolving it must yield the original bytes.
        ///
        /// Invariant: intern_ascii then resolve is the identity on valid ASCII byte slices.
        #[test]
        fn intern_resolve_ascii_roundtrip(bytes in proptest::collection::vec(0u8..128u8, 0..50)) {
            let mut table = SymbolTable::new();
            let id = table.intern_ascii(&bytes);
            // Postcondition: resolving the returned id gives back the original bytes.
            prop_assert_eq!(
                table.resolve(id),
                bytes.as_slice(),
                "resolved ASCII bytes must match the original input"
            );
        }

        /// Interning a UTF-8 string and resolving it must yield the original str.
        ///
        /// Invariant: intern_utf8 then resolve_str is the identity on arbitrary UTF-8 strings.
        #[test]
        fn intern_resolve_utf8_roundtrip(s in ".*") {
            let mut table = SymbolTable::new();
            let id = table.intern_utf8(&s);
            // Postcondition: resolved str matches the original string.
            prop_assert_eq!(
                table.resolve_str(id),
                Some(s.as_str()),
                "resolved UTF-8 string must match the original input"
            );
            // Postcondition: resolved bytes match the original string's UTF-8 encoding.
            prop_assert_eq!(
                table.resolve(id),
                s.as_bytes(),
                "resolved bytes must match the original UTF-8 encoding"
            );
        }

        /// Interning the same ASCII byte string twice must return the same SymbolId,
        /// and the table size must not grow on the second call.
        ///
        /// Invariant: intern is idempotent — repeated intern of equal inputs yields equal IDs
        /// without allocating new entries.
        #[test]
        fn interning_is_idempotent_ascii_prop(bytes in proptest::collection::vec(0u8..128u8, 0..50)) {
            let mut table = SymbolTable::new();
            let id1 = table.intern_ascii(&bytes);
            let len_after_first = table.len();
            let id2 = table.intern_ascii(&bytes);
            // Postcondition: both calls return the same id.
            prop_assert_eq!(id1, id2, "second intern of the same bytes must return the same SymbolId");
            // Postcondition: len must not grow on the second intern of an already-known string.
            prop_assert_eq!(
                table.len(),
                len_after_first,
                "table length must not increase when re-interning the same bytes"
            );
        }

        /// Interning the same UTF-8 string twice must return the same SymbolId
        /// and not increase the table length.
        ///
        /// Invariant: idempotency holds for the UTF-8 pool independently of the ASCII pool.
        #[test]
        fn interning_is_idempotent_utf8_prop(s in ".*") {
            let mut table = SymbolTable::new();
            let id1 = table.intern_utf8(&s);
            let len_after_first = table.len();
            let id2 = table.intern_utf8(&s);
            // Postcondition: identical SymbolIds for the same string.
            prop_assert_eq!(id1, id2, "second intern of the same UTF-8 string must return the same SymbolId");
            // Postcondition: table length is stable after re-interning.
            prop_assert_eq!(
                table.len(),
                len_after_first,
                "table length must not increase when re-interning the same UTF-8 string"
            );
        }

        /// Two distinct ASCII byte strings must yield distinct SymbolIds.
        ///
        /// Invariant: the intern function is injective — different inputs produce different IDs.
        #[test]
        fn distinct_ascii_strings_get_distinct_ids_prop(
            a in proptest::collection::vec(0u8..128u8, 0..50),
            b in proptest::collection::vec(0u8..128u8, 0..50)
        ) {
            prop_assume!(a != b);
            let mut table = SymbolTable::new();
            let id_a = table.intern_ascii(&a);
            let id_b = table.intern_ascii(&b);
            // Postcondition: different strings map to different IDs in the ASCII pool.
            prop_assert_ne!(id_a, id_b, "distinct ASCII byte strings must produce distinct SymbolIds");
        }

        /// After interning N distinct ASCII byte strings, len() must equal N.
        ///
        /// Invariant: each unique entry occupies exactly one slot in the table.
        #[test]
        fn len_tracking_ascii_prop(
            strings in proptest::collection::vec(
                proptest::collection::vec(0u8..128u8, 0..20),
                1usize..16
            )
        ) {
            let mut table = SymbolTable::new();
            // De-duplicate the input so we can predict the exact count.
            let mut unique: Vec<Vec<u8>> = Vec::new();
            for s in &strings {
                if !unique.contains(s) {
                    unique.push(s.clone());
                }
            }
            for s in &unique {
                table.intern_ascii(s);
            }
            // Postcondition: table length matches the number of distinct strings interned.
            prop_assert_eq!(
                table.len(),
                unique.len(),
                "len() must equal the number of distinct interned strings"
            );
        }

        /// ASCII SymbolId(Ascii(i)) and UTF-8 SymbolId(Utf8(i)) with the same internal
        /// index must never compare equal, even when the underlying text is the same.
        ///
        /// Invariant: the two pools are fully isolated — an ASCII entry and a UTF-8
        /// entry with identical text are different symbols.
        #[test]
        fn cross_pool_isolation_prop(s in "[a-zA-Z0-9]{1,20}") {
            // All chars in the above regex are valid ASCII, so the string is accepted
            // by both intern_ascii and intern_utf8.
            let mut table = SymbolTable::new();
            let ascii_id = table.intern_ascii(s.as_bytes());
            let utf8_id = table.intern_utf8(&s);
            // Postcondition: the two IDs must be different — they live in different pools.
            prop_assert_ne!(
                ascii_id,
                utf8_id,
                "ASCII pool entry and UTF-8 pool entry for the same text must be distinct SymbolIds"
            );
        }

        /// intern_symbol with Ascii mode must reject non-ASCII strings and accept ASCII ones.
        /// intern_symbol with Utf8 mode must accept all strings.
        /// intern_symbol with AsciiSymbolsUtf8Strings mode must reject non-ASCII strings.
        ///
        /// Invariant: encoding enforcement is consistent with the declared StringEncoding mode.
        #[test]
        fn encoding_mode_enforcement_prop(s in ".*") {
            let mut table = SymbolTable::new();
            let is_ascii = s.is_ascii();

            // Ascii mode rejects non-ASCII, accepts ASCII.
            let ascii_result = table.intern_symbol(&s, StringEncoding::Ascii);
            if is_ascii {
                prop_assert!(
                    ascii_result.is_ok(),
                    "Ascii mode must accept ASCII string"
                );
            } else {
                prop_assert!(
                    matches!(ascii_result, Err(EncodingError::NonAsciiSymbol(_))),
                    "Ascii mode must reject non-ASCII string with NonAsciiSymbol error"
                );
            }

            // Utf8 mode accepts everything.
            let utf8_result = table.intern_symbol(&s, StringEncoding::Utf8);
            prop_assert!(
                utf8_result.is_ok(),
                "Utf8 mode must accept any string"
            );

            // AsciiSymbolsUtf8Strings mode rejects non-ASCII symbols.
            let mixed_result = table.intern_symbol(&s, StringEncoding::AsciiSymbolsUtf8Strings);
            if is_ascii {
                prop_assert!(
                    mixed_result.is_ok(),
                    "AsciiSymbolsUtf8Strings mode must accept ASCII string"
                );
            } else {
                prop_assert!(
                    matches!(mixed_result, Err(EncodingError::NonAsciiSymbol(_))),
                    "AsciiSymbolsUtf8Strings mode must reject non-ASCII string with NonAsciiSymbol error"
                );
            }
        }
    }

    // ---- Unit tests (kept from before) -------------------------------------

    #[test]
    fn intern_and_resolve_ascii() {
        let mut table = SymbolTable::new();
        let id = table.intern_ascii(b"hello");
        assert_eq!(table.resolve(id), b"hello");
        assert_eq!(table.resolve_str(id), Some("hello"));
    }

    #[test]
    fn intern_and_resolve_utf8() {
        let mut table = SymbolTable::new();
        let id = table.intern_utf8("héllo");
        assert_eq!(table.resolve(id), "héllo".as_bytes());
        assert_eq!(table.resolve_str(id), Some("héllo"));
    }

    #[test]
    fn interning_is_idempotent() {
        let mut table = SymbolTable::new();
        let id1 = table.intern_ascii(b"foo");
        let id2 = table.intern_ascii(b"foo");
        assert_eq!(id1, id2);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn distinct_strings_get_distinct_ids() {
        let mut table = SymbolTable::new();
        let id1 = table.intern_ascii(b"foo");
        let id2 = table.intern_ascii(b"bar");
        assert_ne!(id1, id2);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn symbol_equality() {
        let mut table = SymbolTable::new();
        let s1 = Symbol(table.intern_ascii(b"test"));
        let s2 = Symbol(table.intern_ascii(b"test"));
        let s3 = Symbol(table.intern_ascii(b"other"));
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn empty_table() {
        let table = SymbolTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn resolve_symbol_convenience() {
        let mut table = SymbolTable::new();
        let sym = Symbol(table.intern_ascii(b"test"));
        assert_eq!(table.resolve_symbol(sym), b"test");
        assert_eq!(table.resolve_symbol_str(sym), Some("test"));
    }

    // --- Encoding-checked constructor tests ---

    #[test]
    fn intern_symbol_ascii_mode_accepts_ascii() {
        let mut table = SymbolTable::new();
        let sym = table.intern_symbol("hello", StringEncoding::Ascii).unwrap();
        assert_eq!(table.resolve_symbol(sym), b"hello");
    }

    #[test]
    fn intern_symbol_ascii_mode_rejects_non_ascii() {
        let mut table = SymbolTable::new();
        let result = table.intern_symbol("héllo", StringEncoding::Ascii);
        assert!(matches!(result, Err(EncodingError::NonAsciiSymbol(_))));
    }

    #[test]
    fn intern_symbol_utf8_mode_accepts_unicode() {
        let mut table = SymbolTable::new();
        let sym = table.intern_symbol("héllo", StringEncoding::Utf8).unwrap();
        assert_eq!(table.resolve_symbol_str(sym), Some("héllo"));
    }

    #[test]
    fn intern_symbol_mixed_mode_rejects_non_ascii_symbol() {
        let mut table = SymbolTable::new();
        let result = table.intern_symbol("héllo", StringEncoding::AsciiSymbolsUtf8Strings);
        assert!(matches!(result, Err(EncodingError::NonAsciiSymbol(_))));
    }

    #[test]
    fn intern_symbol_mixed_mode_accepts_ascii_symbol() {
        let mut table = SymbolTable::new();
        let sym = table
            .intern_symbol("hello", StringEncoding::AsciiSymbolsUtf8Strings)
            .unwrap();
        assert_eq!(table.resolve_symbol(sym), b"hello");
    }
}

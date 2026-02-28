//! Symbol interning with encoding awareness.
//!
//! Symbols are interned strings that are cheap to copy and compare.
//! A `SymbolId` is only valid within the `SymbolTable` (and thus the engine)
//! that created it.

use rustc_hash::FxHashMap as HashMap;

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
            ascii_to_id: HashMap::default(),
            ascii_strings: Vec::new(),
            utf8_to_id: HashMap::default(),
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

//! Variable bindings for pattern matching.
//!
//! This module provides variable ID management and binding storage for
//! the pattern matcher.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::rc::Rc;

use crate::symbol::Symbol;
use crate::value::Value;

/// Variable identifier within a rule or pattern.
///
/// `VarIds` are assigned sequentially starting from 0 when variables are
/// encountered during pattern compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VarId(pub u16);

/// Maps variable names (symbols) to their IDs.
///
/// Used during pattern compilation to assign stable IDs to variables.
/// Uses `FxHashMap` for the name→id mapping so that memory and clone costs
/// scale with the number of variables in the rule rather than the highest
/// interned `SymbolId` in the program.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VarMap {
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_helpers::fx_hash_map"))]
    by_name: FxHashMap<Symbol, VarId>,
    by_id: Vec<Symbol>,
}

impl VarMap {
    /// Create a new, empty variable map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_name: FxHashMap::default(),
            by_id: Vec::new(),
        }
    }

    /// Get or create a `VarId` for the given symbol.
    ///
    /// If the symbol already has an ID, returns it. Otherwise, assigns
    /// the next sequential ID.
    pub fn get_or_create(&mut self, name: Symbol) -> Result<VarId, VarMapError> {
        if let Some(&id) = self.by_name.get(&name) {
            return Ok(id);
        }

        let id_val = self.by_id.len();
        if id_val > u16::MAX as usize {
            return Err(VarMapError::TooManyVariables);
        }

        #[allow(clippy::cast_possible_truncation)]
        let id = VarId(id_val as u16);
        self.by_name.insert(name, id);
        self.by_id.push(name);
        Ok(id)
    }

    /// Lookup the `VarId` for a symbol, if it exists.
    #[must_use]
    pub fn lookup(&self, name: Symbol) -> Option<VarId> {
        self.by_name.get(&name).copied()
    }

    /// Resolve a `VarId` back to its symbol name.
    ///
    /// # Panics
    ///
    /// Panics if the `VarId` is not valid for this map.
    #[must_use]
    pub fn name(&self, id: VarId) -> Symbol {
        self.by_id[id.0 as usize]
    }

    /// Returns the number of variables in this map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Returns `true` if no variables have been registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

impl Default for VarMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors arising from variable map operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VarMapError {
    #[error("too many variables (limit: 65536)")]
    TooManyVariables,
}

/// Smart reference for bound values. Stores small/Copy-like variants inline
/// to avoid Rc heap allocation; wraps heap-owning variants in Rc for cheap cloning.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ValueRef {
    /// Symbol, Integer, Float, `ExternalAddress`, Void — no heap alloc needed.
    Inline(Value),
    /// String, Multifield — heap-allocated data, share via Rc.
    Shared(Rc<Value>),
}

impl ValueRef {
    /// Wrap a `Value`, choosing inline or shared storage automatically.
    pub fn new(value: Value) -> Self {
        match &value {
            Value::String(_) | Value::Multifield(_) => Self::Shared(Rc::new(value)),
            _ => Self::Inline(value),
        }
    }
}

impl Clone for ValueRef {
    fn clone(&self) -> Self {
        match self {
            Self::Inline(v) => Self::Inline(v.clone()),
            Self::Shared(rc) => Self::Shared(rc.clone()),
        }
    }
}

impl std::ops::Deref for ValueRef {
    type Target = Value;

    fn deref(&self) -> &Value {
        match self {
            Self::Inline(v) => v,
            Self::Shared(rc) => rc,
        }
    }
}

/// A set of variable bindings.
///
/// Bindings are stored in a vector indexed by `VarId`. Unbound variables
/// are represented as `None`.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BindingSet {
    bindings: SmallVec<[Option<ValueRef>; 4]>,
}

impl BindingSet {
    /// Create a new, empty binding set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bindings: SmallVec::new(),
        }
    }

    /// Get the value bound to a variable, if any.
    #[must_use]
    pub fn get(&self, var: VarId) -> Option<&ValueRef> {
        self.bindings.get(var.0 as usize)?.as_ref()
    }

    /// Bind a variable to a value.
    ///
    /// Extends the binding vector if necessary to accommodate the `VarId`.
    pub fn set(&mut self, var: VarId, value: ValueRef) {
        let idx = var.0 as usize;
        if idx >= self.bindings.len() {
            self.bindings.resize(idx + 1, None);
        }
        self.bindings[idx] = Some(value);
    }

    /// Extend this binding set with bindings from another set.
    ///
    /// Only copies bindings that are not already set in this set.
    #[allow(clippy::cast_possible_truncation)] // Binding sets with >65536 variables are not expected.
    pub fn extend_from(&mut self, other: &BindingSet) {
        for (idx, maybe_val) in other.bindings.iter().enumerate() {
            if let Some(val) = maybe_val {
                let var = VarId(idx as u16);
                if self.get(var).is_none() {
                    self.set(var, val.clone());
                }
            }
        }
    }

    /// Returns the number of variables that can be addressed (including unbound slots).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.bindings.len()
    }

    /// Returns the number of variables that are actually bound.
    #[must_use]
    pub fn bound_count(&self) -> usize {
        self.bindings.iter().filter(|x| x.is_some()).count()
    }
}

impl Default for BindingSet {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<[Option<ValueRef>]> for BindingSet {
    fn as_ref(&self) -> &[Option<ValueRef>] {
        &self.bindings
    }
}

impl AsMut<[Option<ValueRef>]> for BindingSet {
    fn as_mut(&mut self) -> &mut [Option<ValueRef>] {
        &mut self.bindings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolTable;
    use crate::StringEncoding;

    // --- VarMap tests ---

    #[test]
    fn var_map_new_is_empty() {
        let vm = VarMap::new();
        assert!(vm.is_empty());
        assert_eq!(vm.len(), 0);
    }

    #[test]
    fn var_map_get_or_create_assigns_sequential_ids() {
        let mut table = SymbolTable::new();
        let mut vm = VarMap::new();
        let x = table.intern_symbol("x", StringEncoding::Ascii).unwrap();
        let y = table.intern_symbol("y", StringEncoding::Ascii).unwrap();

        let id_x = vm.get_or_create(x).unwrap();
        let id_y = vm.get_or_create(y).unwrap();

        assert_eq!(id_x, VarId(0));
        assert_eq!(id_y, VarId(1));
        assert_eq!(vm.len(), 2);
    }

    #[test]
    fn var_map_get_or_create_is_idempotent() {
        let mut table = SymbolTable::new();
        let mut vm = VarMap::new();
        let x = table.intern_symbol("x", StringEncoding::Ascii).unwrap();

        let id1 = vm.get_or_create(x).unwrap();
        let id2 = vm.get_or_create(x).unwrap();

        assert_eq!(id1, id2);
        assert_eq!(vm.len(), 1);
    }

    #[test]
    fn var_map_lookup() {
        let mut table = SymbolTable::new();
        let mut vm = VarMap::new();
        let x = table.intern_symbol("x", StringEncoding::Ascii).unwrap();
        let y = table.intern_symbol("y", StringEncoding::Ascii).unwrap();

        vm.get_or_create(x).unwrap();

        assert_eq!(vm.lookup(x), Some(VarId(0)));
        assert_eq!(vm.lookup(y), None);
    }

    #[test]
    fn var_map_name_resolves_id() {
        let mut table = SymbolTable::new();
        let mut vm = VarMap::new();
        let x = table.intern_symbol("x", StringEncoding::Ascii).unwrap();
        let id = vm.get_or_create(x).unwrap();

        assert_eq!(vm.name(id), x);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn var_map_name_panics_on_invalid_id() {
        let vm = VarMap::new();
        let _ = vm.name(VarId(0));
    }

    // --- BindingSet tests ---

    #[test]
    fn binding_set_new_is_empty() {
        let bs = BindingSet::new();
        assert_eq!(bs.capacity(), 0);
        assert_eq!(bs.bound_count(), 0);
    }

    #[test]
    fn binding_set_get_unbound_returns_none() {
        let bs = BindingSet::new();
        assert!(bs.get(VarId(0)).is_none());
    }

    #[test]
    fn binding_set_set_and_get() {
        let mut bs = BindingSet::new();
        let val = ValueRef::new(Value::Integer(42));

        bs.set(VarId(0), val);

        assert_eq!(bs.bound_count(), 1);
        let retrieved = bs.get(VarId(0)).unwrap();
        assert!(matches!(**retrieved, Value::Integer(42)));
    }

    #[test]
    fn binding_set_set_extends_capacity() {
        let mut bs = BindingSet::new();
        let val = ValueRef::new(Value::Integer(42));

        bs.set(VarId(5), val);

        assert_eq!(bs.capacity(), 6);
        assert_eq!(bs.bound_count(), 1);
        assert!(bs.get(VarId(0)).is_none());
        assert!(bs.get(VarId(5)).is_some());
    }

    #[test]
    fn binding_set_extend_from() {
        let mut bs1 = BindingSet::new();
        let mut bs2 = BindingSet::new();

        bs1.set(VarId(0), ValueRef::new(Value::Integer(1)));
        bs1.set(VarId(2), ValueRef::new(Value::Integer(3)));

        bs2.set(VarId(0), ValueRef::new(Value::Integer(999))); // Should not overwrite
        bs2.set(VarId(1), ValueRef::new(Value::Integer(2)));

        bs1.extend_from(&bs2);

        // bs1 should have 1 (original), 2 (from bs2), 3 (original)
        assert!(matches!(**bs1.get(VarId(0)).unwrap(), Value::Integer(1)));
        assert!(matches!(**bs1.get(VarId(1)).unwrap(), Value::Integer(2)));
        assert!(matches!(**bs1.get(VarId(2)).unwrap(), Value::Integer(3)));
    }

    #[test]
    fn binding_set_extend_from_does_not_overwrite() {
        let mut bs1 = BindingSet::new();
        let mut bs2 = BindingSet::new();

        bs1.set(VarId(0), ValueRef::new(Value::Integer(1)));
        bs2.set(VarId(0), ValueRef::new(Value::Integer(2)));

        bs1.extend_from(&bs2);

        // bs1's binding for VarId(0) should remain unchanged
        assert!(matches!(**bs1.get(VarId(0)).unwrap(), Value::Integer(1)));
    }

    #[test]
    fn binding_set_bound_count() {
        let mut bs = BindingSet::new();
        bs.set(VarId(0), ValueRef::new(Value::Integer(1)));
        bs.set(VarId(2), ValueRef::new(Value::Integer(2)));

        assert_eq!(bs.capacity(), 3);
        assert_eq!(bs.bound_count(), 2);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::symbol::SymbolTable;
    use crate::StringEncoding;
    use proptest::prelude::*;
    use std::collections::{HashMap, HashSet};

    // ---------------------------------------------------------------------------
    // VarMap property tests
    // ---------------------------------------------------------------------------

    proptest! {
        /// Calling `get_or_create` N times for the same symbol always returns
        /// the same VarId, and len stays at 1.
        #[test]
        fn idempotent_get_or_create(count in 1..=50_usize) {
            let mut table = SymbolTable::new();
            let mut vm = VarMap::new();
            let sym = table.intern_symbol("x", StringEncoding::Ascii).unwrap();

            let first_id = vm.get_or_create(sym).unwrap();
            for _ in 0..count {
                let id = vm.get_or_create(sym).unwrap();
                prop_assert_eq!(id, first_id);
            }
            prop_assert_eq!(vm.len(), 1);
        }

        /// After creating N distinct symbols, `len() == N`.
        #[test]
        fn len_tracks_distinct_symbols(count in 1..=50_usize) {
            let mut table = SymbolTable::new();
            let mut vm = VarMap::new();

            for i in 0..count {
                let sym = table.intern_symbol(&format!("v{i}"), StringEncoding::Ascii).unwrap();
                vm.get_or_create(sym).unwrap();
            }

            prop_assert_eq!(vm.len(), count);
        }

        /// VarIds are assigned 0, 1, 2, ... for distinct symbols in order.
        #[test]
        fn sequential_id_assignment(count in 1..=50_usize) {
            let mut table = SymbolTable::new();
            let mut vm = VarMap::new();

            for i in 0..count {
                let sym = table.intern_symbol(&format!("v{i}"), StringEncoding::Ascii).unwrap();
                let id = vm.get_or_create(sym).unwrap();
                #[allow(clippy::cast_possible_truncation)]
                let expected_id = VarId(i as u16);
                prop_assert_eq!(id, expected_id);
            }
        }

        /// For every symbol added, `lookup(sym) == Some(id)` and `name(id) == sym`.
        #[test]
        fn bidirectional_consistency(count in 1..=50_usize) {
            let mut table = SymbolTable::new();
            let mut vm = VarMap::new();
            let mut pairs: Vec<(Symbol, VarId)> = Vec::new();

            for i in 0..count {
                let sym = table.intern_symbol(&format!("v{i}"), StringEncoding::Ascii).unwrap();
                let id = vm.get_or_create(sym).unwrap();
                pairs.push((sym, id));
            }

            for (sym, id) in pairs {
                prop_assert_eq!(vm.lookup(sym), Some(id));
                prop_assert_eq!(vm.name(id), sym);
            }
        }

        /// Lookup on symbols not yet registered returns None.
        #[test]
        fn lookup_before_create_returns_none(count in 1..=20_usize) {
            let mut table = SymbolTable::new();
            let vm = VarMap::new();

            for i in 0..count {
                let sym = table.intern_symbol(&format!("v{i}"), StringEncoding::Ascii).unwrap();
                prop_assert_eq!(vm.lookup(sym), None);
            }
        }

        /// Interleaving repeated and new symbol creates still maintains consistency:
        /// each symbol maps to a single stable VarId and `len` equals the number of
        /// distinct symbols.
        #[test]
        fn mixed_idempotent_and_new(
            // A sequence of indices in 0..10; the symbol used is "v{i}".
            ops in prop::collection::vec(0..10_usize, 1..=60)
        ) {
            let mut table = SymbolTable::new();
            let mut vm = VarMap::new();
            // Track first-assigned id per index.
            let mut first_ids: HashMap<usize, VarId> = HashMap::new();

            for i in ops {
                let sym = table.intern_symbol(&format!("v{i}"), StringEncoding::Ascii).unwrap();
                let id = vm.get_or_create(sym).unwrap();
                let entry = first_ids.entry(i).or_insert(id);
                prop_assert_eq!(id, *entry);
            }

            prop_assert_eq!(vm.len(), first_ids.len());
        }

        /// `is_empty() == (len() == 0)` always holds, both before and after insertions.
        #[test]
        fn is_empty_consistent_with_len(count in 0..=30_usize) {
            let mut table = SymbolTable::new();
            let mut vm = VarMap::new();

            prop_assert_eq!(vm.is_empty(), vm.is_empty());

            for i in 0..count {
                let sym = table.intern_symbol(&format!("v{i}"), StringEncoding::Ascii).unwrap();
                vm.get_or_create(sym).unwrap();
                prop_assert_eq!(vm.is_empty(), vm.is_empty());
            }
        }
    }

    // ---------------------------------------------------------------------------
    // BindingSet property tests
    // ---------------------------------------------------------------------------

    proptest! {
        /// For arbitrary (VarId, i64) pairs, the last set value is always
        /// retrievable via get().
        #[test]
        fn set_get_roundtrip(ops in prop::collection::vec((0..20_u16, any::<i64>()), 1..=50)) {
            let mut bs = BindingSet::new();
            let mut last_written: HashMap<u16, i64> = HashMap::new();

            for (var_id, value) in ops {
                bs.set(VarId(var_id), ValueRef::new(Value::Integer(value)));
                last_written.insert(var_id, value);
            }

            for (var_id, expected) in last_written {
                let val = bs.get(VarId(var_id)).expect("binding must exist");
                let Value::Integer(got) = **val else {
                    prop_assert!(false, "expected Integer value");
                    unreachable!();
                };
                prop_assert_eq!(got, expected);
            }
        }

        /// Setting the same VarId multiple times: the final value is the one
        /// returned by get().
        #[test]
        fn last_write_wins(writes in prop::collection::vec(any::<i64>(), 1..=20)) {
            let mut bs = BindingSet::new();
            let var = VarId(3);

            for &v in &writes {
                bs.set(var, ValueRef::new(Value::Integer(v)));
            }

            let last = *writes.last().unwrap();
            let val = bs.get(var).expect("binding must exist");
            let Value::Integer(got) = **val else {
                prop_assert!(false, "expected Integer");
                unreachable!();
            };
            prop_assert_eq!(got, last);
        }

        /// Setting VarId(k) always succeeds and produces `capacity() >= k + 1`.
        #[test]
        fn auto_expansion(k in 0..100_u16) {
            let mut bs = BindingSet::new();
            bs.set(VarId(k), ValueRef::new(Value::Integer(0)));
            prop_assert!(bs.capacity() > (k as usize));
        }

        /// With VarIds drawn from 0..10, bound_count() equals the number of
        /// distinct VarIds that were set.
        #[test]
        fn bound_count_accurate_narrow(
            ops in prop::collection::vec((0..10_u16, any::<i64>()), 0..=50)
        ) {
            let mut bs = BindingSet::new();
            let mut distinct: HashSet<u16> = HashSet::new();

            for (var_id, value) in ops {
                bs.set(VarId(var_id), ValueRef::new(Value::Integer(value)));
                distinct.insert(var_id);
            }

            prop_assert_eq!(bs.bound_count(), distinct.len());
        }

        /// With VarIds drawn from 0..100, bound_count() equals the number of
        /// distinct VarIds that were set.
        #[test]
        fn bound_count_accurate_wide(
            ops in prop::collection::vec((0..100_u16, any::<i64>()), 0..=200)
        ) {
            let mut bs = BindingSet::new();
            let mut distinct: HashSet<u16> = HashSet::new();

            for (var_id, value) in ops {
                bs.set(VarId(var_id), ValueRef::new(Value::Integer(value)));
                distinct.insert(var_id);
            }

            prop_assert_eq!(bs.bound_count(), distinct.len());
        }

        /// `bound_count() <= capacity()` always holds.
        #[test]
        fn bound_count_leq_capacity(
            ops in prop::collection::vec((0..50_u16, any::<i64>()), 0..=100)
        ) {
            let mut bs = BindingSet::new();

            for (var_id, value) in ops {
                bs.set(VarId(var_id), ValueRef::new(Value::Integer(value)));
            }

            prop_assert!(bs.bound_count() <= bs.capacity());
        }

        /// After `a.extend_from(&b)`, any variable already bound in `a` retains
        /// its original value.
        #[test]
        fn extend_from_does_not_overwrite(
            a_ops in prop::collection::vec((0..10_u16, any::<i64>()), 1..=20),
            b_ops in prop::collection::vec((0..10_u16, any::<i64>()), 1..=20),
        ) {
            let mut a = BindingSet::new();
            let mut b = BindingSet::new();

            // Track the last value written to `a` for each var.
            let mut a_last: HashMap<u16, i64> = HashMap::new();
            for (var_id, value) in a_ops {
                a.set(VarId(var_id), ValueRef::new(Value::Integer(value)));
                a_last.insert(var_id, value);
            }
            for (var_id, value) in b_ops {
                b.set(VarId(var_id), ValueRef::new(Value::Integer(value)));
            }

            a.extend_from(&b);

            // Every var that was originally bound in `a` still has the same value.
            for (var_id, expected) in a_last {
                let val = a.get(VarId(var_id)).expect("original binding must survive");
                let Value::Integer(got) = **val else {
                    prop_assert!(false, "expected Integer");
                    unreachable!();
                };
                prop_assert_eq!(got, expected);
            }
        }

        /// After `a.extend_from(&b)`, any variable bound in `b` but not in `a`
        /// is now bound in `a` to `b`'s value.
        #[test]
        fn extend_from_fills_gaps(
            // Use non-overlapping id ranges: a uses 0..5, b uses 5..10.
            a_ids in prop::collection::vec(0..5_u16, 0..=5),
            b_ids in prop::collection::vec(5..10_u16, 1..=5),
        ) {
            let mut a = BindingSet::new();
            let mut b = BindingSet::new();
            let mut b_values: HashMap<u16, i64> = HashMap::new();

            for (idx, &var_id) in a_ids.iter().enumerate() {
                #[allow(clippy::cast_possible_wrap)]
                a.set(VarId(var_id), ValueRef::new(Value::Integer(idx as i64)));
            }
            for (idx, &var_id) in b_ids.iter().enumerate() {
                #[allow(clippy::cast_possible_wrap)]
                let v = (idx as i64) + 100;
                b.set(VarId(var_id), ValueRef::new(Value::Integer(v)));
                b_values.insert(var_id, v);
            }

            a.extend_from(&b);

            // Every var from `b` (which was not in `a`) must now be present in `a`.
            for (var_id, expected) in b_values {
                let val = a.get(VarId(var_id)).expect("gap must be filled from b");
                let Value::Integer(got) = **val else {
                    prop_assert!(false, "expected Integer");
                    unreachable!();
                };
                prop_assert_eq!(got, expected);
            }
        }

        /// Calling `a.extend_from(&b)` twice produces the same result as once.
        #[test]
        fn extend_from_idempotent(
            a_ops in prop::collection::vec((0..10_u16, any::<i64>()), 0..=15),
            b_ops in prop::collection::vec((0..10_u16, any::<i64>()), 1..=15),
        ) {
            // Build `a` and `b`.
            let mut a = BindingSet::new();
            let mut b = BindingSet::new();
            for (var_id, value) in &a_ops {
                a.set(VarId(*var_id), ValueRef::new(Value::Integer(*value)));
            }
            for (var_id, value) in &b_ops {
                b.set(VarId(*var_id), ValueRef::new(Value::Integer(*value)));
            }

            // Apply extend once and snapshot.
            a.extend_from(&b);
            let snapshot: Vec<Option<i64>> = (0..a.capacity())
                .map(|i| {
                    #[allow(clippy::cast_possible_truncation)]
                    a.get(VarId(i as u16)).map(|v| {
                        if let Value::Integer(n) = **v { n } else { 0 }
                    })
                })
                .collect();

            // Apply extend a second time — result must be identical.
            a.extend_from(&b);
            let after_second: Vec<Option<i64>> = (0..a.capacity())
                .map(|i| {
                    #[allow(clippy::cast_possible_truncation)]
                    a.get(VarId(i as u16)).map(|v| {
                        if let Value::Integer(n) = **v { n } else { 0 }
                    })
                })
                .collect();

            prop_assert_eq!(snapshot, after_second);
        }
    }
}

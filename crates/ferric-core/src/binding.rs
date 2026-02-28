//! Variable bindings for pattern matching.
//!
//! This module provides variable ID management and binding storage for
//! the pattern matcher.

use rustc_hash::FxHashMap as HashMap;
use smallvec::SmallVec;
use std::rc::Rc;

use crate::symbol::Symbol;
use crate::value::Value;

/// Variable identifier within a rule or pattern.
///
/// `VarIds` are assigned sequentially starting from 0 when variables are
/// encountered during pattern compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VarId(pub u16);

/// Maps variable names (symbols) to their IDs.
///
/// Used during pattern compilation to assign stable IDs to variables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarMap {
    by_name: HashMap<Symbol, VarId>,
    by_id: Vec<Symbol>,
}

impl VarMap {
    /// Create a new, empty variable map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_name: HashMap::default(),
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

/// Reference-counted value for shared bindings.
pub type ValueRef = Rc<Value>;

/// A set of variable bindings.
///
/// Bindings are stored in a vector indexed by `VarId`. Unbound variables
/// are represented as `None`.
#[derive(Clone, Debug)]
pub struct BindingSet {
    bindings: SmallVec<[Option<ValueRef>; 16]>,
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
        let val = Rc::new(Value::Integer(42));

        bs.set(VarId(0), val.clone());

        assert_eq!(bs.bound_count(), 1);
        let retrieved = bs.get(VarId(0)).unwrap();
        assert!(Rc::ptr_eq(retrieved, &val));
    }

    #[test]
    fn binding_set_set_extends_capacity() {
        let mut bs = BindingSet::new();
        let val = Rc::new(Value::Integer(42));

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

        let val_x = Rc::new(Value::Integer(1));
        let val_y = Rc::new(Value::Integer(2));
        let val_z = Rc::new(Value::Integer(3));

        bs1.set(VarId(0), val_x.clone());
        bs1.set(VarId(2), val_z.clone());

        bs2.set(VarId(0), Rc::new(Value::Integer(999))); // Should not overwrite
        bs2.set(VarId(1), val_y.clone());

        bs1.extend_from(&bs2);

        // bs1 should have x (original), y (from bs2), z (original)
        assert!(Rc::ptr_eq(bs1.get(VarId(0)).unwrap(), &val_x));
        assert!(Rc::ptr_eq(bs1.get(VarId(1)).unwrap(), &val_y));
        assert!(Rc::ptr_eq(bs1.get(VarId(2)).unwrap(), &val_z));
    }

    #[test]
    fn binding_set_extend_from_does_not_overwrite() {
        let mut bs1 = BindingSet::new();
        let mut bs2 = BindingSet::new();

        let val1 = Rc::new(Value::Integer(1));
        let val2 = Rc::new(Value::Integer(2));

        bs1.set(VarId(0), val1.clone());
        bs2.set(VarId(0), val2);

        bs1.extend_from(&bs2);

        // bs1's binding for VarId(0) should remain unchanged
        assert!(Rc::ptr_eq(bs1.get(VarId(0)).unwrap(), &val1));
    }

    #[test]
    fn binding_set_bound_count() {
        let mut bs = BindingSet::new();
        bs.set(VarId(0), Rc::new(Value::Integer(1)));
        bs.set(VarId(2), Rc::new(Value::Integer(2)));

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

    proptest! {
        #[test]
        fn var_map_get_or_create_is_idempotent_prop(count in 1..100_usize) {
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

        #[test]
        fn var_map_distinct_symbols_get_distinct_ids(count in 1..100_usize) {
            let mut table = SymbolTable::new();
            let mut vm = VarMap::new();
            let mut ids = std::collections::HashSet::new();

            for i in 0..count {
                let sym = table.intern_symbol(&format!("var{i}"), StringEncoding::Ascii).unwrap();
                let id = vm.get_or_create(sym).unwrap();
                ids.insert(id);
            }

            prop_assert_eq!(ids.len(), count);
            prop_assert_eq!(vm.len(), count);
        }

        #[test]
        fn binding_set_get_returns_last_set_value(ops in prop::collection::vec((0..10_u16, any::<i64>()), 1..50)) {
            let mut bs = BindingSet::new();
            let mut expected = std::collections::HashMap::new();

            for (var_id, value) in ops {
                let var = VarId(var_id);
                let val = Rc::new(Value::Integer(value));
                bs.set(var, val.clone());
                expected.insert(var_id, value);
            }

            for (var_id, expected_value) in expected {
                let var = VarId(var_id);
                if let Some(val) = bs.get(var) {
                    if let Value::Integer(i) = **val {
                        prop_assert_eq!(i, expected_value);
                    } else {
                        prop_assert!(false, "expected integer value");
                    }
                } else {
                    prop_assert!(false, "expected binding to exist");
                }
            }
        }

        #[test]
        fn binding_set_bound_count_is_accurate(ops in prop::collection::vec((0..20_u16, any::<i64>()), 0..100)) {
            let mut bs = BindingSet::new();
            let mut bound_vars = std::collections::HashSet::new();

            for (var_id, value) in ops {
                let var = VarId(var_id);
                bs.set(var, Rc::new(Value::Integer(value)));
                bound_vars.insert(var_id);
            }

            prop_assert_eq!(bs.bound_count(), bound_vars.len());
        }
    }
}

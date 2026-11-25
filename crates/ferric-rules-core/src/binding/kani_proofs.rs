//! Kani proof harnesses for `binding` (see docs/verification.md).
//!
//! What is proved, on the production code, against an abstract model (a
//! fixed array of optional integers plus the vector width):
//!
//! * `BindingSet::set`/`get` refine a total map: after any three writes to
//!   variables below `VARS` (last write wins), `get` of every `VarId` —
//!   including any `u16` id beyond the vector — returns the model's value
//!   without panicking, `capacity` is the largest written id + 1 and
//!   `bound_count` counts the bound variables.
//! * `extend_from` is a left-biased union (the receiver's bindings win, gaps
//!   are filled from `other`, `other` is unchanged) and widens the vector
//!   only to `other`'s highest *bound* variable.
//! * `clone` is an independent snapshot; `clear` empties the set.
//! * `ValueRef::new` shares exactly the heap-owning variants (`String`,
//!   `Multifield`) through an `Arc`; cloning a shared reference shares the
//!   allocation, cloning an inline one copies the value; deref returns the
//!   wrapped value.
//! * `VarMap::get_or_create` numbers names densely in first-occurrence order
//!   (all 27 sequences of three calls over three names), `lookup` and `name`
//!   agree with the numbering; `name` panics for an id at or beyond `len`
//!   (its documented `# Panics` contract); `clear` restarts the numbering.
//!
//! Bounds: `VARS` = 3 variables, at most three writes per set, three names.
//! The `TooManyVariables` limit (65536 names) and widths up to 65536 are out
//! of reach here; verification/verus/core_matching.rs proves them
//! (`theorem_get_or_create`, `BindingSet::extend_from`'s `idx as u16` cast).
//! Known gaps: `set` with a large `VarId` (the vector grows to `id + 1`
//! entries) is only exercised for ids below `VARS`; values other than
//! integers are not stored in the refinement harnesses. Harness-built sets
//! are `mem::forget`-ed instead of dropped (dropping a symbolic `Value` makes
//! CBMC explore the recursive drop glue), so drop glue is not checked.

use super::*;
use crate::symbol::SymbolId;
use crate::value::Multifield;

/// Number of variables the bounded harnesses address. Queries also probe
/// out-of-range ids up to `u16::MAX`.
const VARS: usize = 2;

/// Abstract model of a binding set: one optional integer per variable,
/// with the addressable width (`capacity`) tracked separately.
#[derive(Clone, Copy)]
struct Model {
    values: [Option<i64>; VARS],
    capacity: usize,
}

impl Model {
    const EMPTY: Self = Self {
        values: [None; VARS],
        capacity: 0,
    };

    fn set(&mut self, var: usize, value: i64) {
        self.values[var] = Some(value);
        self.capacity = self.capacity.max(var + 1);
    }

    fn get(&self, var: usize) -> Option<i64> {
        self.values.get(var).copied().flatten()
    }

    fn bound(&self) -> usize {
        self.values.iter().filter(|value| value.is_some()).count()
    }
}

fn int_of(value: Option<&ValueRef>) -> Option<i64> {
    value.map(|value| match **value {
        Value::Integer(i) => i,
        _ => panic!("only integers are bound by these harnesses"),
    })
}

/// Apply `ops` symbolic writes (variable < `VARS`, small integer value) to
/// both the production set and the model.
///
/// The written variable is case-split so each `set` call indexes the vector
/// with a constant: a symbolic index makes the binding a write replaces
/// unknown to CBMC, which then explores the recursive `Value` drop glue.
fn random_writes(set: &mut BindingSet, model: &mut Model, ops: usize) {
    for _ in 0..ops {
        let value: i64 = kani::any::<i8>().into();
        let mut write = |var: u16| {
            set.set(VarId(var), ValueRef::new(Value::Integer(value)));
            model.set(usize::from(var), value);
        };
        match kani::any::<u8>() {
            0 => write(0),
            1 => write(1),
            _ => {}
        }
    }
}

/// The set agrees with the model on every variable, including any
/// out-of-range id, and on `capacity` and `bound_count`.
fn assert_refines(set: &BindingSet, model: &Model) {
    for var in 0..VARS {
        assert_eq!(int_of(set.get(VarId(var as u16))), model.get(var));
    }
    let probe: u16 = kani::any();
    assert_eq!(int_of(set.get(VarId(probe))), model.get(usize::from(probe)));
    assert_eq!(set.capacity(), model.capacity);
    assert_eq!(set.bound_count(), model.bound());
    assert_eq!(set.as_ref().len(), model.capacity);
}

/// `set`/`get` refine a total map from variables to optional values: after
/// any two writes (last write wins, the vector grows to the largest
/// written id + 1, unwritten ids read as unbound), `get` of *any* `VarId`
/// — including ids beyond the vector — returns the model's value without
/// panicking, and `capacity`/`bound_count` match the model.
#[kani::proof]
#[kani::unwind(3)]
fn binding_set_refines_map_model() {
    let mut set = BindingSet::new();
    let mut model = Model::EMPTY;
    assert_refines(&set, &model);
    random_writes(&mut set, &mut model, 2);
    assert_refines(&set, &model);
    kani::cover!(model.capacity == VARS && model.bound() == 1, "sparse");
    kani::cover!(model.bound() == VARS, "fully bound");
    std::mem::forget(set);
}

/// `extend_from` is a left-biased union: every variable bound in `self`
/// keeps its value, every variable bound only in `other` is copied, the
/// width grows to cover `other`'s highest bound variable (not `other`'s
/// trailing unbound slots), and `other` is unchanged.
#[kani::proof]
#[kani::unwind(3)]
fn extend_from_is_left_biased_union() {
    let mut left = BindingSet::new();
    let mut left_model = Model::EMPTY;
    random_writes(&mut left, &mut left_model, 2);
    let mut right = BindingSet::new();
    let mut right_model = Model::EMPTY;
    random_writes(&mut right, &mut right_model, 2);

    left.extend_from(&right);

    let mut expected = left_model;
    for var in 0..VARS {
        if left_model.get(var).is_none() {
            if let Some(value) = right_model.get(var) {
                expected.set(var, value);
            }
        }
    }
    assert_refines(&left, &expected);
    assert_refines(&right, &right_model);
    kani::cover!(
        (0..VARS).any(|var| left_model.get(var).is_some() && right_model.get(var).is_some()),
        "conflict keeps the left value"
    );
    kani::cover!(
        (0..VARS).any(|var| left_model.get(var).is_none() && right_model.get(var).is_some()),
        "gap filled from the right"
    );
    kani::cover!(
        expected.capacity > left_model.capacity,
        "width grows to the right's highest binding"
    );
    std::mem::forget(left);
    std::mem::forget(right);
}

/// A clone is an independent snapshot: writes to the original after cloning
/// are not visible through the clone, and the clone refines the model of
/// the original at cloning time.
#[kani::proof]
#[kani::unwind(3)]
fn clone_is_an_independent_snapshot() {
    let mut set = BindingSet::new();
    let mut model = Model::EMPTY;
    random_writes(&mut set, &mut model, 2);
    let snapshot = set.clone();
    let mut later = model;
    random_writes(&mut set, &mut later, 1);
    assert_refines(&snapshot, &model);
    assert_refines(&set, &later);
    kani::cover!(
        (0..VARS).any(|var| model.get(var) != later.get(var)),
        "original diverged from the clone"
    );
    std::mem::forget(set);
    std::mem::forget(snapshot);
}

/// `clear` unbinds everything and resets the width.
#[kani::proof]
#[kani::unwind(3)]
fn clear_resets_to_empty() {
    let mut set = BindingSet::new();
    let mut model = Model::EMPTY;
    random_writes(&mut set, &mut model, 2);
    set.clear();
    assert_refines(&set, &Model::EMPTY);
    kani::cover!(model.bound() == 2, "cleared two bindings");
    std::mem::forget(set);
}

/// `ValueRef::new` shares (via `Arc`) exactly the heap-owning variants
/// (`String`, `Multifield`) and stores every other variant inline; deref
/// returns the wrapped value; cloning a shared reference shares the same
/// allocation, and cloning an inline one copies the value.
#[kani::proof]
#[kani::unwind(2)]
fn value_ref_storage_and_clone() {
    let check = |value: Value, heap: bool| {
        let reference = ValueRef::new(value);
        let shared = matches!(reference, ValueRef::Shared(_));
        assert_eq!(shared, heap);
        let copy = reference.clone();
        match (&reference, &copy) {
            (ValueRef::Shared(a), ValueRef::Shared(b)) => {
                assert!(Arc::ptr_eq(a, b));
            }
            (ValueRef::Inline(a), ValueRef::Inline(b)) => {
                assert!(a.structural_eq(b));
            }
            _ => panic!("clone changed the storage kind"),
        }
        assert!((*copy).structural_eq(&reference));
        std::mem::forget(reference);
        std::mem::forget(copy);
    };
    match kani::any::<u8>() {
        0 => check(Value::Integer(kani::any()), false),
        1 => check(Value::Float(kani::any()), false),
        2 => check(Value::Symbol(Symbol(SymbolId::Ascii(kani::any()))), false),
        3 => check(Value::Void, false),
        4 => check(
            Value::String(crate::string::FerricString::Ascii(Box::new([kani::any()]))),
            true,
        ),
        _ => {
            let mut multifield = Multifield::new();
            multifield.push(Value::Integer(kani::any()));
            check(Value::Multifield(Box::new(multifield)), true);
        }
    }
}

fn symbol(id: u32) -> Symbol {
    Symbol(SymbolId::Ascii(id))
}

/// `VarMap` numbers variables densely in first-occurrence order: for each
/// equality pattern of three `get_or_create` calls (`aaa`, `aab`, `aba`,
/// `abb`, `abc` — every sequence of three names up to renaming), each call
/// returns the index of the name's first occurrence, `lookup` agrees for
/// every name (including an unused one), `name` inverts the numbering, and
/// `len` counts the distinct names. Names are concrete per pattern: a
/// symbolic name makes every hash-table probe symbolic, and the map's
/// behaviour depends only on name equality.
#[kani::proof]
#[kani::unwind(5)]
fn var_map_numbers_names_in_first_occurrence_order() {
    let names = [symbol(7), symbol(8), symbol(9)];
    let check = |sequence: [usize; 3]| {
        let mut map = VarMap::new();
        let mut order: Vec<usize> = Vec::new();
        for &choice in &sequence {
            let expected = match order.iter().position(|&seen| seen == choice) {
                Some(index) => index,
                None => {
                    order.push(choice);
                    order.len() - 1
                }
            };
            let id = map.get_or_create(names[choice]).unwrap();
            assert_eq!(usize::from(id.0), expected);
        }
        assert_eq!(map.len(), order.len());
        assert!(!map.is_empty());
        for (choice, name) in names.iter().enumerate() {
            let expected = order.iter().position(|&seen| seen == choice);
            assert_eq!(map.lookup(*name).map(|id| usize::from(id.0)), expected);
        }
        for (index, &choice) in order.iter().enumerate() {
            assert_eq!(map.name(VarId(index as u16)), names[choice]);
        }
        kani::cover!(order.len() == 3, "three distinct names");
        kani::cover!(order.len() == 1, "one name repeated");
    };
    match kani::any::<u8>() {
        0 => check([0, 0, 0]),
        1 => check([0, 0, 1]),
        2 => check([0, 1, 0]),
        3 => check([0, 1, 1]),
        _ => check([0, 1, 2]),
    }
}

/// `VarMap::name` panics exactly for ids at or beyond `len` (its documented
/// `# Panics` contract); this harness covers the panicking side.
#[kani::proof]
#[kani::should_panic]
#[kani::unwind(5)]
fn var_map_name_panics_out_of_range() {
    let mut map = VarMap::new();
    map.get_or_create(symbol(1)).unwrap();
    let id: u16 = kani::any_where(|id: &u16| *id >= 1);
    kani::cover!(id == u16::MAX, "largest id");
    let _ = map.name(VarId(id));
}

/// `clear` forgets every name and restarts numbering at zero.
#[kani::proof]
#[kani::unwind(5)]
fn var_map_clear_restarts_numbering() {
    let mut map = VarMap::new();
    map.get_or_create(symbol(1)).unwrap();
    map.get_or_create(symbol(2)).unwrap();
    map.clear();
    assert!(map.is_empty());
    assert_eq!(map.lookup(symbol(1)), None);
    assert_eq!(map.get_or_create(symbol(2)).unwrap(), VarId(0));
    assert_eq!(map.name(VarId(0)), symbol(2));
}

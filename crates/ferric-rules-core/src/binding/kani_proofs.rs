//! Kani proof harnesses for `binding` (see docs/verification.md).
//!
//! What is proved, on the production code, against an abstract model (one
//! optional integer per variable plus the vector width), one operation at a
//! time from *any* binding set of width at most two (so, by induction, for
//! every sequence of operations on such sets):
//!
//! * `BindingSet::set`/`get` refine a total map: the written variable holds
//!   the new value (last write wins), every other variable is unchanged, the
//!   vector widens exactly to cover the variable, and `get` of every `VarId`
//!   — including any `u16` id beyond the vector — returns the model's value
//!   without panicking; `capacity` and `bound_count` match the model.
//! * `extend_from` is a left-biased union (the receiver's bindings win, gaps
//!   are filled from `other`, `other` is unchanged) and widens the vector
//!   only to `other`'s highest *bound* variable.
//! * `clone` is an independent snapshot; `clear` empties the set.
//! * `ValueRef::new` stores scalars inline (clone copies, deref returns the
//!   value) and shares `String`/`Multifield` through one `Arc` (clone shares
//!   the allocation).
//! * `VarMap::get_or_create` numbers names densely in first-occurrence order
//!   (every equality pattern of three calls), `lookup` and `name` agree with
//!   the numbering; `name` panics for an id at or beyond `len` (its
//!   documented `# Panics` contract); `clear` restarts the numbering.
//!
//! Bounds: binding sets of width at most two holding small integers;
//! `VarMap` with three concrete names (its behaviour depends only on name
//! equality; symbolic names make every hash probe symbolic). The 65536-name
//! `TooManyVariables` limit, widths up to 65536 and the `idx as u16` cast in
//! `extend_from` are out of reach here and are proved in
//! verification/verus/core_matching.rs (`theorem_get_or_create`,
//! `BindingSet::extend_from`). Known gaps: `set` with a `VarId` far beyond
//! the vector (it grows to `id + 1` entries) and non-integer values in the
//! refinement harnesses. Harness-built sets are `mem::forget`-ed instead of
//! dropped (dropping a symbolic `Value` makes CBMC explore the recursive drop
//! glue), so drop glue is not checked.

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

/// One binding-set shape: width and which slots are bound.
#[derive(Clone, Copy)]
struct Shape {
    width: usize,
    bound: [bool; VARS],
}

/// Every shape of width at most two (seven shapes).
const SHAPES: [Shape; 7] = [
    Shape {
        width: 0,
        bound: [false, false],
    },
    Shape {
        width: 1,
        bound: [false, false],
    },
    Shape {
        width: 1,
        bound: [true, false],
    },
    Shape {
        width: 2,
        bound: [false, false],
    },
    Shape {
        width: 2,
        bound: [true, false],
    },
    Shape {
        width: 2,
        bound: [false, true],
    },
    Shape {
        width: 2,
        bound: [true, true],
    },
];

/// A binding set of the given shape, bound slots holding symbolic small
/// integers, with its model. The state is built directly from the binding
/// vector: every such state is reachable through the public API (`set`
/// widens and binds, `AsMut` unbinds). Harnesses case-split the shape so
/// each copy knows which slots are bound: when CBMC cannot tell, every
/// binding that an operation replaces or clones sends it through the
/// recursive `Value` drop and clone glue, which exhausts memory.
fn binding_set_of(shape: Shape) -> (BindingSet, Model) {
    let mut bindings: SmallVec<[Option<ValueRef>; 4]> = SmallVec::new();
    let mut model = Model::EMPTY;
    model.capacity = shape.width;
    for var in 0..shape.width {
        if shape.bound[var] {
            let value: i64 = kani::any::<i8>().into();
            bindings.push(Some(ValueRef::new(Value::Integer(value))));
            model.values[var] = Some(value);
        } else {
            bindings.push(None);
        }
    }
    (BindingSet { bindings }, model)
}

/// Run `check` on a binding set of every shape, one copy per shape.
fn for_each_shape(check: impl Fn(BindingSet, Model)) {
    match kani::any::<u8>() {
        0 => check_shape(&check, SHAPES[0]),
        1 => check_shape(&check, SHAPES[1]),
        2 => check_shape(&check, SHAPES[2]),
        3 => check_shape(&check, SHAPES[3]),
        4 => check_shape(&check, SHAPES[4]),
        5 => check_shape(&check, SHAPES[5]),
        _ => check_shape(&check, SHAPES[6]),
    }
}

fn check_shape(check: &impl Fn(BindingSet, Model), shape: Shape) {
    let (set, model) = binding_set_of(shape);
    check(set, model);
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

/// `set` refines map insertion and `get` is total: from every state of
/// width at most two, binding either variable leaves the set agreeing with
/// the model — the written variable holds the new value (last write wins),
/// every other variable is unchanged, the vector widens exactly to cover the
/// variable — and `get` of *any* `VarId`, including ids beyond the vector,
/// returns the model's value without panicking. One copy per shape and
/// variable.
#[kani::proof]
#[kani::unwind(3)]
fn binding_set_refines_map_model() {
    for_each_shape(|set, model| {
        assert_refines(&set, &model);
        if kani::any() {
            write_and_check(set, model, 0);
        } else {
            write_and_check(set, model, 1);
        }
    });
}

/// `set(var, v)` on the production set and the model, then compare them.
fn write_and_check(mut set: BindingSet, mut model: Model, var: u16) {
    let value: i64 = kani::any::<i8>().into();
    let overwrite = model.get(usize::from(var)).is_some();
    let grows = usize::from(var) >= model.capacity;
    set.set(VarId(var), ValueRef::new(Value::Integer(value)));
    model.set(usize::from(var), value);
    assert_refines(&set, &model);
    kani::cover!(overwrite, "overwrite");
    kani::cover!(grows, "grow");
    std::mem::forget(set);
}

/// `extend_from` is a left-biased union: every variable bound in `self`
/// keeps its value, every variable bound only in `other` is copied, the
/// width grows to cover `other`'s highest *bound* variable (not `other`'s
/// trailing unbound slots), and `other` is unchanged. The receiver ranges
/// over every shape; `other` over the width-two shapes with one or two
/// bindings and the width-one bound shape (one copy per pair).
#[kani::proof]
#[kani::unwind(3)]
fn extend_from_is_left_biased_union() {
    for_each_shape(|left, left_model| match kani::any::<u8>() {
        0 => extend_and_check(left, left_model, SHAPES[2]),
        1 => extend_and_check(left, left_model, SHAPES[4]),
        2 => extend_and_check(left, left_model, SHAPES[5]),
        _ => extend_and_check(left, left_model, SHAPES[6]),
    });
}

/// `left.extend_from(&right)` against the model's left-biased union.
fn extend_and_check(mut left: BindingSet, left_model: Model, right_shape: Shape) {
    let (right, right_model) = binding_set_of(right_shape);
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
        left_model.get(0).is_some() && right_model.get(0).is_some(),
        "conflict keeps the left value"
    );
    kani::cover!(
        expected.capacity > left_model.capacity,
        "width grows to the right's highest binding"
    );
    std::mem::forget(left);
    std::mem::forget(right);
}

/// A clone is an independent snapshot: a write to the original after
/// cloning is not visible through the clone (every shape, either variable).
#[kani::proof]
#[kani::unwind(3)]
fn clone_is_an_independent_snapshot() {
    for_each_shape(|mut set, model| {
        let snapshot = set.clone();
        let value: i64 = kani::any::<i8>().into();
        let mut later = model;
        if kani::any() {
            set.set(VarId(0), ValueRef::new(Value::Integer(value)));
            later.set(0, value);
        } else {
            set.set(VarId(1), ValueRef::new(Value::Integer(value)));
            later.set(1, value);
        }
        assert_refines(&snapshot, &model);
        assert_refines(&set, &later);
        kani::cover!(model.get(0) != later.get(0), "original diverged");
        std::mem::forget(set);
        std::mem::forget(snapshot);
    });
}

/// `clear` unbinds everything and resets the width (every shape).
#[kani::proof]
#[kani::unwind(3)]
fn clear_resets_to_empty() {
    for_each_shape(|mut set, model| {
        set.clear();
        assert_refines(&set, &Model::EMPTY);
        kani::cover!(model.bound() == 2, "cleared two bindings");
        std::mem::forget(set);
    });
}

/// `ValueRef::new` stores the non-heap variants inline; cloning an inline
/// reference copies the value and deref returns it. Checked per variant
/// (integer, float bit pattern, symbol, `Void`) with variant-specific
/// comparisons, avoiding the generic recursive `Value` clone/equality glue.
#[kani::proof]
#[kani::unwind(2)]
fn value_ref_inline_for_scalar_values() {
    match kani::any::<u8>() {
        0 => {
            let x: i64 = kani::any();
            let reference = ValueRef::new(Value::Integer(x));
            let copy = reference.clone();
            assert!(matches!(reference, ValueRef::Inline(Value::Integer(v)) if v == x));
            assert!(matches!(copy, ValueRef::Inline(Value::Integer(v)) if v == x));
            assert!(matches!(*copy, Value::Integer(v) if v == x));
            std::mem::forget((reference, copy));
        }
        1 => {
            let bits: u64 = kani::any();
            let reference = ValueRef::new(Value::Float(f64::from_bits(bits)));
            let copy = reference.clone();
            assert!(matches!(reference, ValueRef::Inline(Value::Float(v)) if v.to_bits() == bits));
            assert!(matches!(*copy, Value::Float(v) if v.to_bits() == bits));
            std::mem::forget((reference, copy));
        }
        2 => {
            let symbol = Symbol(SymbolId::Ascii(kani::any()));
            let reference = ValueRef::new(Value::Symbol(symbol));
            let copy = reference.clone();
            assert!(matches!(reference, ValueRef::Inline(Value::Symbol(s)) if s == symbol));
            assert!(matches!(*copy, Value::Symbol(s) if s == symbol));
            std::mem::forget((reference, copy));
        }
        _ => {
            let reference = ValueRef::new(Value::Void);
            let copy = reference.clone();
            assert!(matches!(reference, ValueRef::Inline(Value::Void)));
            assert!(matches!(*copy, Value::Void));
            std::mem::forget((reference, copy));
        }
    }
}

/// `ValueRef::new` shares the heap-owning variants (`String`, `Multifield`)
/// through an `Arc`; cloning shares the same allocation, and deref of either
/// reference returns that one shared value.
#[kani::proof]
#[kani::unwind(2)]
fn value_ref_shares_heap_values() {
    let reference = if kani::any() {
        ValueRef::new(Value::String(crate::string::FerricString::Ascii(Box::new(
            [kani::any()],
        ))))
    } else {
        let mut multifield = Multifield::new();
        multifield.push(Value::Integer(kani::any()));
        ValueRef::new(Value::Multifield(Box::new(multifield)))
    };
    let copy = reference.clone();
    match (&reference, &copy) {
        (ValueRef::Shared(a), ValueRef::Shared(b)) => {
            assert!(Arc::ptr_eq(a, b));
            assert_eq!(Arc::strong_count(a), 2);
        }
        _ => panic!("heap values must be shared"),
    }
    assert!(std::ptr::eq(&*reference, &*copy));
    let string = matches!(*reference, Value::String(_));
    kani::cover!(string, "string shared");
    kani::cover!(!string, "multifield shared");
    std::mem::forget((reference, copy));
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

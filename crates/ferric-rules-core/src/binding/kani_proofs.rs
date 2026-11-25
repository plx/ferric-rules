//! Kani proof harnesses for `binding` (see docs/verification.md).
//!
//! What is proved, on the production code, against an abstract model (one
//! optional integer per variable plus the vector width):
//!
//! * `BindingSet::get` is total and exact: on every state of width at most
//!   two (bound or unbound slots), every `VarId` — including any `u16` id
//!   beyond the vector — reads the model's value without panicking;
//!   `capacity` is the width and `bound_count` counts the bound slots.
//! * `BindingSet::set` on an empty set binds exactly the written variable
//!   (0, 1 or 2) and widens the vector to cover it, leaving lower variables
//!   unbound.
//! * `ValueRef::new` stores scalar values (integer, float bit pattern,
//!   symbol, `Void`) inline; cloning copies them and deref returns them.
//! * `VarMap::get_or_create` numbers names densely in first-occurrence order
//!   (every equality pattern of two calls), `lookup` and `name` agree with
//!   the numbering; `name` panics for an id at or beyond `len` (its
//!   documented `# Panics` contract).
//!
//! Bounds: binding sets of width at most two (three after `set`) holding
//! integers; `VarMap` with three concrete names (its behaviour depends only
//! on name equality; symbolic names make every hash probe symbolic).
//!
//! Known gaps. Not checked by Kani but proved for the executable Verus
//! model in verification/verus/core_matching.rs: `set` over an existing
//! binding or after another `set`, `extend_from`, the 65536-name
//! `TooManyVariables` limit and the `idx as u16` cast in `extend_from`.
//! Not checked at all: `BindingSet::clone`/`clear`, shared (`Arc`)
//! `ValueRef`s, `VarMap::clear` and `VarMap` sequences of more than two
//! calls (`VarMap` harnesses with three or more hash-table operations ran
//! past 10 minutes). Every Kani attempt at those ran out of memory: CBMC cannot tell which `Value` variant a replaced, cleared or
//! cloned binding holds once it has passed through `SmallVec` storage, so it
//! explores the recursive `Value` drop/clone glue through every variant.
//! Harness-built sets are `mem::forget`-ed instead of dropped for the same
//! reason, so drop glue is not checked.

use super::*;
use crate::symbol::SymbolId;

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

/// `get` is total and exact on every binding-set state of width at most
/// two: for every `VarId` — including any `u16` id beyond the vector — it
/// returns the bound value or `None` exactly as the model does, without
/// panicking; `capacity` is the width and `bound_count` counts the bound
/// slots. One copy per shape.
#[kani::proof]
#[kani::unwind(3)]
fn binding_set_get_is_total_and_exact() {
    for_each_shape(|set, model| {
        assert_refines(&set, &model);
        kani::cover!(model.capacity == 2 && model.bound() == 1, "sparse");
        std::mem::forget(set);
    });
}

/// `set` on an empty binding set binds exactly the written variable and
/// widens the vector to cover it, leaving every lower variable unbound
/// (one copy per variable 0, 1, 2; one write: two writes, or a write over an
/// existing binding, make CBMC explore the recursive `Value` drop glue of
/// the replaced binding and exhaust memory).
#[kani::proof]
#[kani::unwind(4)]
fn binding_set_set_widens_and_binds() {
    let check = |var: u16| {
        let mut set = BindingSet::new();
        let value: i64 = kani::any();
        set.set(VarId(var), ValueRef::new(Value::Integer(value)));
        assert_eq!(set.capacity(), usize::from(var) + 1);
        assert_eq!(set.bound_count(), 1);
        for other in 0..=var {
            let expected = (other == var).then_some(value);
            assert_eq!(int_of(set.get(VarId(other))), expected);
        }
        let probe: u16 = kani::any_where(|probe: &u16| *probe > var);
        assert!(set.get(VarId(probe)).is_none());
        std::mem::forget(set);
    };
    match kani::any::<u8>() {
        0 => check(0),
        1 => check(1),
        _ => check(2),
    }
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

fn symbol(id: u32) -> Symbol {
    Symbol(SymbolId::Ascii(id))
}

/// `VarMap` numbers variables densely in first-occurrence order: for each
/// equality pattern of two `get_or_create` calls (`aa`, `ab` — every
/// sequence of two names up to renaming), each call returns the index of the
/// name's first occurrence, `lookup` agrees for every name (including an
/// unused one), `name` inverts the numbering, and `len` counts the distinct
/// names. Names are concrete per pattern: a symbolic name makes every
/// hash-table probe symbolic, and the map's behaviour depends only on name
/// equality. (Three calls per pattern exceeded 14 minutes.)
#[kani::proof]
#[kani::unwind(5)]
fn var_map_numbers_names_in_first_occurrence_order() {
    let names = [symbol(7), symbol(8), symbol(9)];
    let check = |sequence: [usize; 2]| {
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
        kani::cover!(order.len() == 2, "two distinct names");
        kani::cover!(order.len() == 1, "one name repeated");
    };
    if kani::any() {
        check([0, 0]);
    } else {
        check([0, 1]);
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

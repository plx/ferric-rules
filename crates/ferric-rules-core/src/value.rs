//! Runtime value types for Ferric.
//!
//! The [`Value`] type represents all runtime values. It intentionally does NOT
//! implement `Eq` or `Hash` because it contains `f64` (IEEE 754 equality is
//! not reflexive) and `Multifield` (deep hashing would be expensive).
//!
//! For contexts that need hashing/equality (alpha-memory indexing, constant
//! tests), use [`AtomKey`].

use smallvec::SmallVec;
use std::ffi::c_void;
use std::ops::{Deref, DerefMut};

use crate::string::FerricString;
use crate::symbol::Symbol;

/// Opaque type identifier for external addresses.
///
/// Assigned by the embedding application when registering external types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ExternalTypeId(pub u32);

/// An opaque pointer for embedding, with a type tag.
///
/// The `pointer` field is a raw pointer managed by the embedding application.
/// Ferric does not dereference it; it is only stored and passed back.
#[derive(Clone, Copy, Debug)]
pub struct ExternalAddress {
    pub type_id: ExternalTypeId,
    pub pointer: *mut c_void,
}

/// An ordered collection of [`Value`]s.
///
/// Uses `SmallVec` for common small cases (up to 8 values inline).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Multifield {
    values: SmallVec<[Value; 8]>,
}

impl Multifield {
    /// Create a new, empty multifield.
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: SmallVec::new(),
        }
    }

    /// Returns the values as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[Value] {
        &self.values
    }

    /// Returns the number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` if the multifield is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Push a value onto the end.
    pub fn push(&mut self, value: Value) {
        self.values.push(value);
    }

    /// Iterate over values by shared reference.
    pub fn iter(&self) -> std::slice::Iter<'_, Value> {
        self.values.iter()
    }

    /// Iterate over values by mutable reference.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Value> {
        self.values.iter_mut()
    }
}

impl AsRef<[Value]> for Multifield {
    fn as_ref(&self) -> &[Value] {
        self.as_slice()
    }
}

impl AsMut<[Value]> for Multifield {
    fn as_mut(&mut self) -> &mut [Value] {
        self.values.as_mut_slice()
    }
}

impl Deref for Multifield {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for Multifield {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.values.as_mut_slice()
    }
}

impl Default for Multifield {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<Value> for Multifield {
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Self {
        Self {
            values: iter.into_iter().collect(),
        }
    }
}

impl Extend<Value> for Multifield {
    fn extend<I: IntoIterator<Item = Value>>(&mut self, iter: I) {
        self.values.extend(iter);
    }
}

impl IntoIterator for Multifield {
    type Item = Value;
    type IntoIter = smallvec::IntoIter<[Value; 8]>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

impl<'a> IntoIterator for &'a Multifield {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<'a> IntoIterator for &'a mut Multifield {
    type Item = &'a mut Value;
    type IntoIter = std::slice::IterMut<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter_mut()
    }
}

impl PartialEq for Multifield {
    fn eq(&self, other: &Self) -> bool {
        if self.values.len() != other.values.len() {
            return false;
        }
        self.values
            .iter()
            .zip(other.values.iter())
            .all(|(a, b)| a.structural_eq(b))
    }
}

/// Runtime value in Ferric.
///
/// Value intentionally does NOT implement `Eq` or `Hash` because it contains
/// `Float` (IEEE 754 equality is not reflexive) and `Multifield` (deep hashing
/// would be expensive and leak into hot paths). For contexts that need
/// hashing/equality — alpha-memory indexing, constant tests — use [`AtomKey`].
#[derive(Clone, Debug)]
pub enum Value {
    /// A symbolic atom (interned, always `Copy`).
    Symbol(Symbol),
    /// A string value.
    String(FerricString),
    /// A 64-bit signed integer.
    Integer(i64),
    /// A 64-bit floating point number.
    Float(f64),
    /// An ordered collection of values (heap-allocated to break size recursion).
    Multifield(Box<Multifield>),
    /// An opaque pointer for embedding (with type tag).
    ExternalAddress(ExternalAddress),
    /// The void/nil value.
    Void,
}

impl Value {
    /// Structural equality: compares values by their content.
    ///
    /// For floats, uses bitwise comparison (`to_bits`), matching CLIPS behavior:
    /// `-0.0 != +0.0` and distinct NaN bit patterns are distinct.
    ///
    /// This is NOT `PartialEq` because IEEE 754 makes `NaN != NaN`,
    /// but for pattern matching we need consistent deterministic behavior.
    #[must_use]
    pub fn structural_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Symbol(a), Self::Symbol(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::Multifield(a), Self::Multifield(b)) => a == b,
            (Self::ExternalAddress(a), Self::ExternalAddress(b)) => {
                a.type_id == b.type_id && std::ptr::eq(a.pointer, b.pointer)
            }
            (Self::Void, Self::Void) => true,
            _ => false,
        }
    }

    /// Returns `true` if this value is `Void`.
    #[must_use]
    pub fn is_void(&self) -> bool {
        matches!(self, Self::Void)
    }

    /// Returns the type name of this value (for diagnostics).
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Symbol(_) => "SYMBOL",
            Self::String(_) => "STRING",
            Self::Integer(_) => "INTEGER",
            Self::Float(_) => "FLOAT",
            Self::Multifield(_) => "MULTIFIELD",
            Self::ExternalAddress(_) => "EXTERNAL-ADDRESS",
            Self::Void => "VOID",
        }
    }
}

// ---------------------------------------------------------------------------
// From implementations for convenient Value construction
// ---------------------------------------------------------------------------

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Integer(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Integer(i64::from(v))
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<Symbol> for Value {
    fn from(s: Symbol) -> Self {
        Value::Symbol(s)
    }
}

impl From<FerricString> for Value {
    fn from(s: FerricString) -> Self {
        Value::String(s)
    }
}

// ---------------------------------------------------------------------------
// IntoFieldValues: generic conversion for ordered-fact field arguments
// ---------------------------------------------------------------------------

/// Trait for types that can be converted into a field list for ordered facts.
///
/// This enables `Engine::assert_ordered` to accept single values, vectors,
/// arrays, and primitive types directly — without requiring the caller to
/// wrap everything in `vec![Value::...]`.
pub trait IntoFieldValues {
    /// Convert into the internal field representation.
    fn into_field_values(self) -> SmallVec<[Value; 8]>;
}

impl IntoFieldValues for Vec<Value> {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        self.into_iter().collect()
    }
}

impl IntoFieldValues for SmallVec<[Value; 8]> {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        self
    }
}

impl IntoFieldValues for Value {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        smallvec::smallvec![self]
    }
}

impl<const N: usize> IntoFieldValues for [Value; N] {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        self.into_iter().collect()
    }
}

// Primitive → single-field conversions (combines From<T> for Value).

impl IntoFieldValues for i64 {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        smallvec::smallvec![Value::Integer(self)]
    }
}

impl IntoFieldValues for i32 {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        smallvec::smallvec![Value::Integer(i64::from(self))]
    }
}

impl IntoFieldValues for f64 {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        smallvec::smallvec![Value::Float(self)]
    }
}

impl IntoFieldValues for Symbol {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        smallvec::smallvec![Value::Symbol(self)]
    }
}

impl IntoFieldValues for FerricString {
    fn into_field_values(self) -> SmallVec<[Value; 8]> {
        smallvec::smallvec![Value::String(self)]
    }
}

/// A value that can be used as a hash key in alpha-memory indices
/// and constant-test definitions.
///
/// Covers the "atomic" value types that can appear as constant-test operands
/// or index keys. `Multifield` and `Void` are excluded.
///
/// `FloatBits(u64)` stores the raw IEEE 754 bit pattern via `f64::to_bits()`.
/// This means `-0.0 != +0.0` and each NaN bit pattern is a distinct key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AtomKey {
    Symbol(Symbol),
    String(FerricString),
    Integer(i64),
    /// Float stored as raw bits via `f64::to_bits()`.
    FloatBits(u64),
    /// External address keyed by `(type_id, pointer_as_usize)`.
    ExternalAddress {
        type_id: ExternalTypeId,
        pointer: usize,
    },
}

impl AtomKey {
    /// Convert from a [`Value`], if the value is an atom (not `Multifield` or `Void`).
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        Self::try_from(value).ok()
    }

    /// Convert back to a [`Value`].
    #[must_use]
    pub fn to_value(&self) -> Value {
        self.clone().into()
    }
}

impl TryFrom<&Value> for AtomKey {
    type Error = ();

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Symbol(s) => Ok(Self::Symbol(*s)),
            Value::String(s) => Ok(Self::String(s.clone())),
            Value::Integer(i) => Ok(Self::Integer(*i)),
            Value::Float(f) => Ok(Self::FloatBits(f.to_bits())),
            Value::ExternalAddress(ea) => Ok(Self::ExternalAddress {
                type_id: ea.type_id,
                pointer: ea.pointer as usize,
            }),
            Value::Multifield(_) | Value::Void => Err(()),
        }
    }
}

impl From<AtomKey> for Value {
    #[allow(unsafe_code)]
    fn from(value: AtomKey) -> Self {
        match value {
            AtomKey::Symbol(s) => Value::Symbol(s),
            AtomKey::String(s) => Value::String(s),
            AtomKey::Integer(i) => Value::Integer(i),
            AtomKey::FloatBits(bits) => Value::Float(f64::from_bits(bits)),
            AtomKey::ExternalAddress { type_id, pointer } => {
                Value::ExternalAddress(ExternalAddress {
                    type_id,
                    // Casting usize back to pointer requires allowing unsafe_code lint,
                    // though the cast itself is not an unsafe operation.
                    pointer: pointer as *mut c_void,
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Manual serde impls for Value and AtomKey
// ---------------------------------------------------------------------------
//
// `Value` and `AtomKey` contain `ExternalAddress` / `ExternalAddress`-derived
// variants that hold raw pointers and cannot be serialized. All other variants
// are serialized through a surrogate enum that serde can derive for.

#[cfg(feature = "serde")]
mod serde_impl {
    use super::{AtomKey, FerricString, Multifield, Symbol, Value};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // ---- Value ----

    #[derive(Serialize, Deserialize)]
    enum ValueSurrogate {
        Symbol(Symbol),
        String(FerricString),
        Integer(i64),
        Float(f64),
        Multifield(Box<Multifield>),
        Void,
    }

    impl Serialize for Value {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                Value::Symbol(s) => ValueSurrogate::Symbol(*s).serialize(serializer),
                Value::String(s) => ValueSurrogate::String(s.clone()).serialize(serializer),
                Value::Integer(i) => ValueSurrogate::Integer(*i).serialize(serializer),
                Value::Float(f) => ValueSurrogate::Float(*f).serialize(serializer),
                Value::Multifield(m) => ValueSurrogate::Multifield(m.clone()).serialize(serializer),
                Value::ExternalAddress(_) => Err(serde::ser::Error::custom(
                    "ExternalAddress cannot be serialized",
                )),
                Value::Void => ValueSurrogate::Void.serialize(serializer),
            }
        }
    }

    impl<'de> Deserialize<'de> for Value {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let surrogate = ValueSurrogate::deserialize(deserializer)?;
            Ok(match surrogate {
                ValueSurrogate::Symbol(s) => Value::Symbol(s),
                ValueSurrogate::String(s) => Value::String(s),
                ValueSurrogate::Integer(i) => Value::Integer(i),
                ValueSurrogate::Float(f) => Value::Float(f),
                ValueSurrogate::Multifield(m) => Value::Multifield(m),
                ValueSurrogate::Void => Value::Void,
            })
        }
    }

    // ---- AtomKey ----

    #[derive(Serialize, Deserialize)]
    enum AtomKeySurrogate {
        Symbol(Symbol),
        String(FerricString),
        Integer(i64),
        FloatBits(u64),
    }

    impl Serialize for AtomKey {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                AtomKey::Symbol(s) => AtomKeySurrogate::Symbol(*s).serialize(serializer),
                AtomKey::String(s) => AtomKeySurrogate::String(s.clone()).serialize(serializer),
                AtomKey::Integer(i) => AtomKeySurrogate::Integer(*i).serialize(serializer),
                AtomKey::FloatBits(b) => AtomKeySurrogate::FloatBits(*b).serialize(serializer),
                AtomKey::ExternalAddress { .. } => Err(serde::ser::Error::custom(
                    "ExternalAddress AtomKey cannot be serialized",
                )),
            }
        }
    }

    impl<'de> Deserialize<'de> for AtomKey {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let surrogate = AtomKeySurrogate::deserialize(deserializer)?;
            Ok(match surrogate {
                AtomKeySurrogate::Symbol(s) => AtomKey::Symbol(s),
                AtomKeySurrogate::String(s) => AtomKey::String(s),
                AtomKeySurrogate::Integer(i) => AtomKey::Integer(i),
                AtomKeySurrogate::FloatBits(b) => AtomKey::FloatBits(b),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::StringEncoding;

    // --- AtomKey float bit semantics ---

    #[test]
    fn atom_key_negative_zero_vs_positive_zero() {
        let pos = AtomKey::from_value(&Value::Float(0.0)).unwrap();
        let neg = AtomKey::from_value(&Value::Float(-0.0)).unwrap();
        assert_ne!(pos, neg, "-0.0 and +0.0 must be distinct AtomKeys");
    }

    #[test]
    fn atom_key_nan_bit_patterns() {
        let nan1 = f64::NAN;
        let nan2 = f64::from_bits(f64::NAN.to_bits() | 1); // Different NaN payload
        let key1 = AtomKey::from_value(&Value::Float(nan1)).unwrap();
        let key2 = AtomKey::from_value(&Value::Float(nan2)).unwrap();

        if nan1.to_bits() == nan2.to_bits() {
            assert_eq!(key1, key2);
        } else {
            assert_ne!(
                key1, key2,
                "distinct NaN bit patterns should be distinct keys"
            );
        }
    }

    #[test]
    fn atom_key_float_roundtrip() {
        let values = [
            0.0_f64,
            -0.0,
            1.0,
            -1.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ];
        for &v in &values {
            let key = AtomKey::from_value(&Value::Float(v)).unwrap();
            if let Value::Float(roundtripped) = key.to_value() {
                assert_eq!(
                    v.to_bits(),
                    roundtripped.to_bits(),
                    "float roundtrip should preserve exact bit pattern for {v}"
                );
            } else {
                panic!("AtomKey::to_value should produce Value::Float");
            }
        }
    }

    // --- AtomKey from_value coverage ---

    #[test]
    fn atom_key_from_symbol() {
        let mut table = crate::symbol::SymbolTable::new();
        let sym = table.intern_symbol("test", StringEncoding::Ascii).unwrap();
        let key = AtomKey::from_value(&Value::Symbol(sym)).unwrap();
        assert_eq!(key, AtomKey::Symbol(sym));
    }

    #[test]
    fn atom_key_from_string() {
        let s = FerricString::new("test", StringEncoding::Utf8).unwrap();
        let key = AtomKey::from_value(&Value::String(s.clone())).unwrap();
        assert_eq!(key, AtomKey::String(s));
    }

    #[test]
    fn atom_key_from_integer() {
        let key = AtomKey::from_value(&Value::Integer(42)).unwrap();
        assert_eq!(key, AtomKey::Integer(42));
    }

    #[test]
    fn atom_key_try_from_value() {
        let key = AtomKey::try_from(&Value::Integer(7)).unwrap();
        assert_eq!(key, AtomKey::Integer(7));
    }

    #[test]
    fn atom_key_from_multifield_is_none() {
        let mf = Multifield::new();
        assert!(AtomKey::from_value(&Value::Multifield(Box::new(mf))).is_none());
    }

    #[test]
    fn atom_key_from_void_is_none() {
        assert!(AtomKey::from_value(&Value::Void).is_none());
    }

    // --- Value structural equality ---

    #[test]
    fn structural_eq_integers() {
        assert!(Value::Integer(42).structural_eq(&Value::Integer(42)));
        assert!(!Value::Integer(42).structural_eq(&Value::Integer(43)));
    }

    #[test]
    fn structural_eq_floats_bitwise() {
        assert!(Value::Float(1.0).structural_eq(&Value::Float(1.0)));
        assert!(!Value::Float(0.0).structural_eq(&Value::Float(-0.0)));
        // NaN is structurally equal to itself (same bit pattern)
        assert!(Value::Float(f64::NAN).structural_eq(&Value::Float(f64::NAN)));
    }

    #[test]
    fn structural_eq_void() {
        assert!(Value::Void.structural_eq(&Value::Void));
    }

    #[test]
    fn structural_eq_different_types() {
        assert!(!Value::Integer(1).structural_eq(&Value::Float(1.0)));
        assert!(!Value::Void.structural_eq(&Value::Integer(0)));
    }

    // --- Multifield ---

    #[test]
    fn multifield_push_and_len() {
        let mut mf = Multifield::new();
        assert!(mf.is_empty());
        mf.push(Value::Integer(1));
        mf.push(Value::Integer(2));
        assert_eq!(mf.len(), 2);
        assert!(!mf.is_empty());
    }

    #[test]
    fn multifield_from_iter() {
        let mf: Multifield = vec![Value::Integer(1), Value::Integer(2)]
            .into_iter()
            .collect();
        assert_eq!(mf.len(), 2);
    }

    #[test]
    fn multifield_extend() {
        let mut mf = Multifield::new();
        mf.extend([Value::Integer(1), Value::Integer(2)]);
        assert_eq!(mf.len(), 2);
    }

    #[test]
    fn multifield_into_iter() {
        let mf: Multifield = vec![Value::Integer(1), Value::Integer(2)]
            .into_iter()
            .collect();
        let values: Vec<_> = mf.into_iter().collect();
        assert_eq!(values.len(), 2);
        assert!(matches!(values[0], Value::Integer(1)));
        assert!(matches!(values[1], Value::Integer(2)));
    }

    #[test]
    fn multifield_equality() {
        let a: Multifield = vec![Value::Integer(1), Value::Integer(2)]
            .into_iter()
            .collect();
        let b: Multifield = vec![Value::Integer(1), Value::Integer(2)]
            .into_iter()
            .collect();
        let c: Multifield = vec![Value::Integer(1), Value::Integer(3)]
            .into_iter()
            .collect();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn multifield_equality_different_lengths() {
        let a: Multifield = vec![Value::Integer(1)].into_iter().collect();
        let b: Multifield = vec![Value::Integer(1), Value::Integer(2)]
            .into_iter()
            .collect();
        assert_ne!(a, b);
    }

    // --- Value type_name ---

    #[test]
    fn value_type_names() {
        assert_eq!(Value::Integer(0).type_name(), "INTEGER");
        assert_eq!(Value::Float(0.0).type_name(), "FLOAT");
        assert_eq!(Value::Void.type_name(), "VOID");
    }

    // --- AtomKey roundtrip for all atom types ---

    #[test]
    fn atom_key_integer_roundtrip() {
        let v = Value::Integer(-42);
        let key = AtomKey::from_value(&v).unwrap();
        assert!(key.to_value().structural_eq(&v));
    }

    #[test]
    fn atom_key_into_value() {
        let value: Value = AtomKey::Integer(99).into();
        assert!(value.structural_eq(&Value::Integer(99)));
    }

    #[test]
    fn atom_key_string_roundtrip() {
        let s = FerricString::new("hello world", StringEncoding::Utf8).unwrap();
        let v = Value::String(s);
        let key = AtomKey::from_value(&v).unwrap();
        assert!(key.to_value().structural_eq(&v));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::encoding::StringEncoding;
    use proptest::prelude::*;

    fn arb_ferric_string() -> impl Strategy<Value = FerricString> {
        prop_oneof![
            "[a-zA-Z0-9 ]{0,20}"
                .prop_map(|s| FerricString::new(&s, StringEncoding::Ascii).expect("ASCII string")),
            "\\PC{0,20}"
                .prop_map(|s| FerricString::new(&s, StringEncoding::Utf8).expect("UTF-8 string")),
        ]
    }

    fn arb_atom_value() -> impl Strategy<Value = Value> {
        prop_oneof![
            any::<i64>().prop_map(Value::Integer),
            any::<f64>().prop_map(Value::Float),
            arb_ferric_string().prop_map(Value::String),
        ]
    }

    proptest! {
        #[test]
        fn atom_key_roundtrip_preserves_structural_eq(v in arb_atom_value()) {
            let key = AtomKey::from_value(&v).unwrap();
            let roundtripped = key.to_value();
            prop_assert!(v.structural_eq(&roundtripped),
                "roundtrip should preserve structural equality");
        }

        #[test]
        fn atom_key_float_bits_preserve_sign(f in any::<f64>()) {
            let key = AtomKey::from_value(&Value::Float(f)).unwrap();
            if let AtomKey::FloatBits(bits) = key {
                prop_assert_eq!(bits, f.to_bits());
            } else {
                prop_assert!(false, "should be FloatBits");
            }
        }

        #[test]
        fn ferric_string_byte_equality_is_reflexive(s in arb_ferric_string()) {
            prop_assert_eq!(&s, &s);
        }

        #[test]
        fn ferric_string_ordering_is_consistent_with_eq(
            a in arb_ferric_string(),
            b in arb_ferric_string()
        ) {
            use std::cmp::Ordering;
            let ord = a.cmp(&b);
            if ord == Ordering::Equal {
                prop_assert_eq!(&a, &b);
            } else {
                prop_assert_ne!(&a, &b);
            }
        }

        #[test]
        fn structural_eq_is_reflexive(v in arb_atom_value()) {
            // For non-NaN floats, structural_eq should be reflexive.
            // NaN is also reflexive under structural_eq (bitwise comparison).
            prop_assert!(v.structural_eq(&v));
        }
    }
}

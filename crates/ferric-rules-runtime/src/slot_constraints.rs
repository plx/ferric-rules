//! Compiled slot constraints and constraint-aware default derivation.

use std::cmp::Ordering;

use ferric_rules_core::{
    FactAddress, FerricString, InstanceName, StringEncoding, SymbolTable, Value,
};
use ferric_rules_parser::{
    Cardinality, LiteralKind, NumericBound, NumericRange, SlotType, SlotValueType,
};

/// Bound work caused by a compact cardinality declaration before allocating its default.
pub(crate) const MAX_DERIVED_DEFAULT_FIELDS: u64 = 1_000_000;
/// Repeated strings own their bytes, so a field-count limit alone cannot bound expansion.
pub(crate) const MAX_DERIVED_DEFAULT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct RuntimeAllowedValueSet {
    pub kind: SlotValueType,
    pub values: Vec<Value>,
}

impl PartialEq for RuntimeAllowedValueSet {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .zip(&other.values)
                .all(|(a, b)| a.structural_eq(b))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct RuntimeSlotConstraints {
    /// Missing kinds are unrestricted; a present empty list forbids that kind.
    pub allowed_values: Vec<RuntimeAllowedValueSet>,
    pub range: Option<NumericRange>,
    pub cardinality: Option<Cardinality>,
}

pub(crate) fn value_kind(value: &Value) -> Option<SlotValueType> {
    Some(match value {
        Value::Symbol(_) => SlotValueType::Symbol,
        Value::String(_) => SlotValueType::String,
        Value::Integer(_) => SlotValueType::Integer,
        Value::Float(_) => SlotValueType::Float,
        Value::InstanceName(_) => SlotValueType::InstanceName,
        Value::FactAddress(_) => SlotValueType::FactAddress,
        Value::ExternalAddress(_) => SlotValueType::ExternalAddress,
        Value::Multifield(_) | Value::Void => return None,
    })
}

fn literal_value(
    literal: &LiteralKind,
    symbols: &mut SymbolTable,
    encoding: StringEncoding,
) -> Result<Value, String> {
    Ok(match literal {
        LiteralKind::Integer(value) => Value::Integer(*value),
        LiteralKind::Float(value) => Value::Float(*value),
        LiteralKind::String(value) => {
            Value::String(FerricString::new(value, encoding).map_err(|error| error.to_string())?)
        }
        LiteralKind::Symbol(value) => Value::Symbol(
            symbols
                .intern_symbol(value, encoding)
                .map_err(|error| error.to_string())?,
        ),
        LiteralKind::InstanceName(value) => Value::InstanceName(InstanceName::from_symbol(
            symbols
                .intern_symbol(value, encoding)
                .map_err(|error| error.to_string())?,
        )),
    })
}

pub(crate) fn compile_constraints(
    source: &ferric_rules_parser::SlotConstraints,
    symbols: &mut SymbolTable,
    encoding: StringEncoding,
) -> Result<RuntimeSlotConstraints, String> {
    let allowed_values = source
        .allowed_values
        .iter()
        .map(|set| {
            let values = set
                .values
                .iter()
                .map(|literal| literal_value(&literal.value, symbols, encoding))
                .collect::<Result<_, _>>()?;
            Ok(RuntimeAllowedValueSet {
                kind: set.kind,
                values,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(RuntimeSlotConstraints {
        allowed_values,
        range: source.range,
        cardinality: source.cardinality,
    })
}

/// Compare mixed numeric kinds without rounding an integer through f64 first.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn compare_integer_float(integer: i64, float: f64) -> Option<Ordering> {
    if float.is_nan() {
        return None;
    }
    // i64::MAX rounds up to 2^63, which is outside the integer domain.
    if float >= i64::MAX as f64 {
        return Some(Ordering::Less);
    }
    if float < i64::MIN as f64 {
        return Some(Ordering::Greater);
    }
    let truncated = float as i64;
    match integer.cmp(&truncated) {
        Ordering::Equal => (integer as f64).partial_cmp(&float),
        ordering => Some(ordering),
    }
}

fn compare_numbers(left: NumericBound, right: NumericBound) -> Option<Ordering> {
    match (left, right) {
        (NumericBound::Integer(a), NumericBound::Integer(b)) => Some(a.cmp(&b)),
        (NumericBound::Float(a), NumericBound::Float(b)) => a.partial_cmp(&b),
        (NumericBound::Integer(a), NumericBound::Float(b)) => compare_integer_float(a, b),
        (NumericBound::Float(a), NumericBound::Integer(b)) => {
            compare_integer_float(b, a).map(Ordering::reverse)
        }
    }
}

fn number(value: &Value) -> Option<NumericBound> {
    match value {
        Value::Integer(value) => Some(NumericBound::Integer(*value)),
        Value::Float(value) => Some(NumericBound::Float(*value)),
        _ => None,
    }
}

impl RuntimeSlotConstraints {
    fn allowed(&self, kind: SlotValueType) -> Option<&[Value]> {
        self.allowed_values
            .iter()
            .find(|set| set.kind == kind)
            .map(|set| set.values.as_slice())
    }

    fn validate_number(&self, value: NumericBound) -> Result<(), String> {
        if let Some(range) = self.range {
            if range.min.is_some_and(|min| {
                !matches!(
                    compare_numbers(value, min),
                    Some(Ordering::Equal | Ordering::Greater)
                )
            }) || range.max.is_some_and(|max| {
                !matches!(
                    compare_numbers(value, max),
                    Some(Ordering::Equal | Ordering::Less)
                )
            }) {
                return Err("value does not fall in the allowed numeric range".to_owned());
            }
        }
        Ok(())
    }

    pub(crate) fn validate_field(&self, value: &Value) -> Result<(), String> {
        if let Some(allowed) = value_kind(value).and_then(|kind| self.allowed(kind)) {
            if !allowed
                .iter()
                .any(|candidate| candidate.structural_eq(value))
            {
                return Err("value does not match the allowed values".to_owned());
            }
        }
        if let Some(value) = number(value) {
            self.validate_number(value)?;
        }
        Ok(())
    }

    pub(crate) fn validate_literal(
        &self,
        literal: &LiteralKind,
        symbols: &SymbolTable,
    ) -> Result<(), String> {
        let kind = match literal {
            LiteralKind::Integer(_) => SlotValueType::Integer,
            LiteralKind::Float(_) => SlotValueType::Float,
            LiteralKind::String(_) => SlotValueType::String,
            LiteralKind::Symbol(_) => SlotValueType::Symbol,
            LiteralKind::InstanceName(_) => SlotValueType::InstanceName,
        };
        if let Some(allowed) = self.allowed(kind) {
            let matches = allowed.iter().any(|candidate| match (literal, candidate) {
                (LiteralKind::Integer(a), Value::Integer(b)) => a == b,
                (LiteralKind::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
                (LiteralKind::String(a), Value::String(b)) => a.as_bytes() == b.as_bytes(),
                (LiteralKind::Symbol(a), Value::Symbol(b)) => {
                    a.as_bytes() == symbols.resolve_symbol(*b)
                }
                (LiteralKind::InstanceName(a), Value::InstanceName(b)) => {
                    a.as_bytes() == symbols.resolve_symbol(b.as_symbol())
                }
                _ => false,
            });
            if !matches {
                return Err("value does not match the allowed values".to_owned());
            }
        }
        match literal {
            LiteralKind::Integer(value) => self.validate_number(NumericBound::Integer(*value)),
            LiteralKind::Float(value) => self.validate_number(NumericBound::Float(*value)),
            _ => Ok(()),
        }
    }

    pub(crate) fn validate_cardinality(&self, length: usize) -> Result<(), String> {
        let length = u64::try_from(length).unwrap_or(u64::MAX);
        if self.cardinality.is_some_and(|cardinality| {
            length < cardinality.min || cardinality.max.is_some_and(|max| length > max)
        }) {
            return Err("value does not satisfy the cardinality restrictions".to_owned());
        }
        Ok(())
    }

    /// Validate the normalized representation, including after snapshot decoding.
    pub(crate) fn validate_metadata(
        &self,
        allowed_types: Option<&[SlotValueType]>,
        slot_type: SlotType,
        symbols: &SymbolTable,
    ) -> Result<(), String> {
        #[cfg(not(feature = "serde"))]
        let _ = symbols;
        if allowed_types.is_some_and(|types| {
            types.is_empty() || types.windows(2).any(|pair| pair[0] >= pair[1])
        }) {
            return Err("invalid normalized type constraint".to_owned());
        }
        let mut previous = None;
        for set in &self.allowed_values {
            if previous.is_some_and(|kind| kind >= set.kind)
                || matches!(
                    set.kind,
                    SlotValueType::FactAddress | SlotValueType::ExternalAddress
                )
                || set
                    .values
                    .iter()
                    .any(|value| value_kind(value) != Some(set.kind))
                || (!set.values.is_empty()
                    && allowed_types.is_some_and(|types| !types.contains(&set.kind)))
            {
                return Err("invalid normalized allowed-values constraint".to_owned());
            }
            previous = Some(set.kind);
            #[cfg(feature = "serde")]
            for value in &set.values {
                symbols.validate_snapshot_value(value)?;
            }
        }
        if let Some(range) = self.range {
            if self
                .allowed_values
                .iter()
                .any(|set| matches!(set.kind, SlotValueType::Integer | SlotValueType::Float))
            {
                return Err("numeric range conflicts with allowed numeric values".to_owned());
            }
            if [range.min, range.max].into_iter().flatten().any(|bound| {
                matches!(bound, NumericBound::Float(value) if value.is_nan())
                    || allowed_types.is_some_and(|types| {
                        !types.contains(&match bound {
                            NumericBound::Integer(_) => SlotValueType::Integer,
                            NumericBound::Float(_) => SlotValueType::Float,
                        })
                    })
            }) || matches!((range.min, range.max), (Some(min), Some(max)) if compare_numbers(min, max) == Some(Ordering::Greater))
            {
                return Err("invalid numeric range constraint".to_owned());
            }
        }
        if let Some(cardinality) = self.cardinality {
            if slot_type != SlotType::Multi
                || cardinality.min > u64::MAX / 2
                || cardinality
                    .max
                    .is_some_and(|max| max > u64::MAX / 2 || max < cardinality.min)
            {
                return Err("invalid cardinality constraint".to_owned());
            }
        }
        Ok(())
    }
}

const DEFAULT_KIND_ORDER: [SlotValueType; 7] = [
    SlotValueType::Symbol,
    SlotValueType::String,
    SlotValueType::Integer,
    SlotValueType::Float,
    SlotValueType::InstanceName,
    SlotValueType::FactAddress,
    SlotValueType::ExternalAddress,
];

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn derived_number(kind: SlotValueType, range: Option<NumericRange>) -> Option<Value> {
    let bound = range.and_then(|range| range.min.or(range.max));
    match (kind, bound) {
        (SlotValueType::Integer, None) => Some(Value::Integer(0)),
        (SlotValueType::Integer, Some(NumericBound::Integer(value))) => Some(Value::Integer(value)),
        (SlotValueType::Integer, Some(NumericBound::Float(value))) => {
            // CLIPS truncates a fractional lower bound and can derive an invalid
            // integer. Ferric's always-on checks require a value inside the range;
            // try its first integer, then let derivation fall back to FLOAT.
            let value = if range.is_some_and(|range| range.min.is_some()) {
                value.ceil()
            } else {
                value.floor()
            };
            (value >= i64::MIN as f64 && value < i64::MAX as f64)
                .then(|| Value::Integer(value as i64))
        }
        (SlotValueType::Float, None) => Some(Value::Float(0.0)),
        (SlotValueType::Float, Some(NumericBound::Integer(value))) => {
            Some(Value::Float(value as f64))
        }
        (SlotValueType::Float, Some(NumericBound::Float(value))) => Some(Value::Float(value)),
        _ => None,
    }
}

fn derive_field(
    allowed_types: Option<&[SlotValueType]>,
    constraints: &RuntimeSlotConstraints,
    symbols: &mut SymbolTable,
    encoding: StringEncoding,
) -> Result<Value, String> {
    for kind in DEFAULT_KIND_ORDER {
        if allowed_types.is_some_and(|allowed| !allowed.contains(&kind)) {
            continue;
        }
        if let Some(values) = constraints.allowed(kind) {
            if let Some(value) = values
                .iter()
                .find(|value| constraints.validate_field(value).is_ok())
            {
                return Ok(value.clone());
            }
            continue;
        }
        let candidate = match kind {
            SlotValueType::Symbol => Some(literal_value(&LiteralKind::Symbol("nil".to_owned()), symbols, encoding)?),
            SlotValueType::String => Some(literal_value(&LiteralKind::String(String::new()), symbols, encoding)?),
            SlotValueType::Integer | SlotValueType::Float => derived_number(kind, constraints.range),
            SlotValueType::InstanceName => Some(literal_value(&LiteralKind::InstanceName("nil".to_owned()), symbols, encoding)?),
            SlotValueType::FactAddress => Some(Value::FactAddress(FactAddress::dummy())),
            SlotValueType::ExternalAddress => return Err("an external-address slot requires (default ?NONE); Ferric cannot derive a host-owned token".to_owned()),
        };
        if let Some(candidate) = candidate.filter(|value| constraints.validate_field(value).is_ok())
        {
            return Ok(candidate);
        }
    }
    Err("cannot derive a value satisfying the slot constraints; supply an explicit default or (default ?NONE)".to_owned())
}

pub(crate) fn derive_default(
    slot_type: SlotType,
    allowed_types: Option<&[SlotValueType]>,
    constraints: &RuntimeSlotConstraints,
    symbols: &mut SymbolTable,
    encoding: StringEncoding,
) -> Result<Value, String> {
    constraints.validate_metadata(allowed_types, slot_type, symbols)?;
    if slot_type == SlotType::Single {
        return derive_field(allowed_types, constraints, symbols, encoding);
    }
    let count = constraints
        .cardinality
        .map_or(0, |cardinality| cardinality.min);
    if count > MAX_DERIVED_DEFAULT_FIELDS {
        return Err(format!(
            "derived default exceeds the {MAX_DERIVED_DEFAULT_FIELDS}-field allocation limit"
        ));
    }
    let mut fields = Vec::new();
    if count != 0 {
        let value = derive_field(allowed_types, constraints, symbols, encoding)?;
        let heap_bytes = match &value {
            Value::String(value) => value.len(),
            _ => 0,
        };
        let field_bytes = u64::try_from(std::mem::size_of::<Value>().saturating_add(heap_bytes))
            .unwrap_or(u64::MAX);
        if count.saturating_mul(field_bytes) > MAX_DERIVED_DEFAULT_BYTES {
            return Err(format!(
                "derived default exceeds the {MAX_DERIVED_DEFAULT_BYTES}-byte allocation limit"
            ));
        }
        let count = usize::try_from(count).map_err(|error| error.to_string())?;
        fields
            .try_reserve_exact(count)
            .map_err(|error| error.to_string())?;
        fields.resize(count, value);
    }
    Ok(Value::Multifield(Box::new(fields.into_iter().collect())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_rules_parser::{
        interpret_constructs, parse_sexprs, Construct, FileId, InterpreterConfig, SlotDefinition,
    };

    fn compiled(slot: &str) -> (SlotDefinition, RuntimeSlotConstraints, SymbolTable) {
        let parsed = parse_sexprs(&format!("(deftemplate sample {slot})"), FileId(0));
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        let mut interpreted = interpret_constructs(&parsed.exprs, &InterpreterConfig::default());
        assert!(interpreted.errors.is_empty(), "{:?}", interpreted.errors);
        let Construct::Template(mut template) = interpreted.constructs.remove(0) else {
            panic!("expected a template")
        };
        let slot = template.slots.remove(0);
        let mut symbols = SymbolTable::new();
        let constraints =
            compile_constraints(&slot.constraints, &mut symbols, StringEncoding::Utf8).unwrap();
        constraints
            .validate_metadata(slot.allowed_types.as_deref(), slot.slot_type, &symbols)
            .unwrap();
        (slot, constraints, symbols)
    }

    fn derived(slot: &str) -> (Value, SymbolTable) {
        let (slot, constraints, mut symbols) = compiled(slot);
        let value = derive_default(
            slot.slot_type,
            slot.allowed_types.as_deref(),
            &constraints,
            &mut symbols,
            StringEncoding::Utf8,
        )
        .unwrap();
        (value, symbols)
    }

    #[test]
    fn category_restrictions_leave_other_kinds_unrestricted() {
        let (_, constraints, mut symbols) = compiled("(slot x (allowed-symbols red green))");
        for value in [
            Value::Integer(3),
            Value::String(FerricString::new("blue", StringEncoding::Utf8).unwrap()),
            Value::FactAddress(FactAddress::dummy()),
        ] {
            assert!(constraints.validate_field(&value).is_ok());
        }
        let red = symbols.intern_symbol("red", StringEncoding::Utf8).unwrap();
        let blue = symbols.intern_symbol("blue", StringEncoding::Utf8).unwrap();
        assert!(constraints.validate_field(&Value::Symbol(red)).is_ok());
        assert!(constraints.validate_field(&Value::Symbol(blue)).is_err());
        assert!(constraints
            .validate_literal(&LiteralKind::Symbol("blue".into()), &symbols)
            .is_err());
    }

    #[test]
    fn allowed_values_cover_literal_kinds_but_not_fact_addresses() {
        let (_, constraints, symbols) = compiled("(slot x (allowed-values a 1 [one]))");
        assert!(constraints.validate_field(&Value::Integer(1)).is_ok());
        assert!(constraints.validate_field(&Value::Float(1.0)).is_err());
        assert!(constraints
            .validate_field(&Value::FactAddress(FactAddress::dummy()))
            .is_ok());
        assert!(constraints
            .validate_literal(&LiteralKind::InstanceName("one".into()), &symbols)
            .is_ok());
        assert!(constraints
            .validate_literal(&LiteralKind::InstanceName("two".into()), &symbols)
            .is_err());
    }

    #[test]
    fn numeric_membership_is_type_and_float_bit_sensitive() {
        let (_, constraints, symbols) = compiled("(slot x (allowed-numbers 1 0.0))");
        assert!(constraints.validate_field(&Value::Integer(1)).is_ok());
        assert!(constraints.validate_field(&Value::Float(1.0)).is_err());
        assert!(constraints.validate_field(&Value::Float(0.0)).is_ok());
        assert!(constraints.validate_field(&Value::Float(-0.0)).is_err());
        assert!(constraints
            .validate_literal(&LiteralKind::Float(-0.0), &symbols)
            .is_err());
    }

    #[test]
    fn derived_defaults_choose_kind_before_source_value_order() {
        let (value, symbols) = derived("(slot x (allowed-values 1 a b))");
        let Value::Symbol(value) = value else {
            panic!("expected symbol")
        };
        assert_eq!(symbols.resolve_symbol_str(value), Some("a"));
        assert!(derived("(slot x (type NUMBER) (allowed-numbers 2.5 7 3))")
            .0
            .structural_eq(&Value::Integer(7)));
        assert!(derived("(slot x (type FLOAT) (allowed-floats 7.5 3.5))")
            .0
            .structural_eq(&Value::Float(7.5)));
    }

    #[test]
    fn derived_numeric_defaults_choose_first_bounded_endpoint() {
        for (slot, expected) in [
            (
                "(slot x (type INTEGER) (range -10 10))",
                Value::Integer(-10),
            ),
            (
                "(slot x (type INTEGER) (range ?VARIABLE 10))",
                Value::Integer(10),
            ),
            (
                "(slot x (type FLOAT) (range 2.5 ?VARIABLE))",
                Value::Float(2.5),
            ),
            ("(slot x (type NUMBER) (range 2 4.0))", Value::Integer(2)),
            ("(slot x (type NUMBER) (range 2.5 3.5))", Value::Integer(3)),
            ("(slot x (type NUMBER) (range 2.5 2.8))", Value::Float(2.5)),
            (
                "(slot x (type NUMBER) (range ?VARIABLE 3.5))",
                Value::Integer(3),
            ),
        ] {
            assert!(derived(slot).0.structural_eq(&expected), "{slot}");
        }
    }

    #[test]
    fn derived_instance_name_satisfies_allowed_values() {
        // CLIPS derives an invalid nil symbol without an explicit type here.
        // Ferric's derived default must obey the same constraints as assertions.
        let (value, symbols) = derived("(slot x (allowed-values [one]))");
        let Value::InstanceName(value) = value else {
            panic!("expected instance name")
        };
        assert_eq!(symbols.resolve_symbol_str(value.as_symbol()), Some("one"));
    }

    #[test]
    fn derived_multislot_repeats_valid_field_to_minimum_cardinality() {
        let (value, symbols) = derived("(multislot x (allowed-symbols a b) (cardinality 2 3))");
        let Value::Multifield(values) = value else {
            panic!("expected multifield")
        };
        assert_eq!(values.len(), 2);
        for value in values.iter() {
            let Value::Symbol(value) = value else {
                panic!("expected symbol")
            };
            assert_eq!(symbols.resolve_symbol_str(*value), Some("a"));
        }
        let (value, _) = derived("(multislot x (type EXTERNAL-ADDRESS) (cardinality 0 0))");
        assert!(matches!(value, Value::Multifield(values) if values.is_empty()));
    }

    #[test]
    fn huge_derived_minimum_is_rejected_but_large_maximum_is_supported() {
        let (slot, constraints, mut symbols) =
            compiled("(multislot x (cardinality 9223372036854775807 ?VARIABLE))");
        let error = derive_default(
            slot.slot_type,
            slot.allowed_types.as_deref(),
            &constraints,
            &mut symbols,
            StringEncoding::Utf8,
        )
        .unwrap_err();
        assert!(error.contains("allocation limit"));
        let (value, _) = derived("(multislot x (cardinality 0 9223372036854775807))");
        assert!(matches!(value, Value::Multifield(values) if values.is_empty()));
    }

    #[test]
    fn derived_string_expansion_is_bounded_before_cloning_payloads() {
        let (slot, constraints, mut symbols) = compiled(&format!(
            "(multislot x (type STRING) (allowed-strings \"{}\") (cardinality 1000000 ?VARIABLE))",
            "x".repeat(100)
        ));
        let error = derive_default(
            slot.slot_type,
            slot.allowed_types.as_deref(),
            &constraints,
            &mut symbols,
            StringEncoding::Utf8,
        )
        .unwrap_err();
        assert!(error.contains("byte allocation limit"));
    }

    #[test]
    fn range_comparison_keeps_large_integer_precision() {
        let (_, constraints, _) = compiled("(slot x (range 9007199254740993 9223372036854775807))");
        assert!(constraints
            .validate_field(&Value::Integer(9_007_199_254_740_993))
            .is_ok());
        assert!(constraints
            .validate_field(&Value::Float(9_007_199_254_740_992.0))
            .is_err());
        assert!(constraints
            .validate_field(&Value::Integer(i64::MAX))
            .is_ok());
        assert!(constraints
            .validate_field(&Value::Float(9_223_372_036_854_775_808.0))
            .is_err());
        assert_eq!(
            compare_integer_float(i64::MIN, -9_223_372_036_854_775_808.0),
            Some(Ordering::Equal)
        );
        assert_eq!(compare_integer_float(-3, -3.5), Some(Ordering::Greater));
        assert_eq!(compare_integer_float(3, 3.5), Some(Ordering::Less));
    }

    #[test]
    fn malformed_normalized_constraints_are_rejected() {
        let (_, mut constraints, symbols) = compiled("(slot x (allowed-values a 1))");
        constraints.allowed_values.reverse();
        assert!(constraints
            .validate_metadata(None, SlotType::Single, &symbols)
            .is_err());
        let constraints = RuntimeSlotConstraints {
            cardinality: Some(Cardinality {
                min: u64::MAX,
                max: None,
            }),
            ..RuntimeSlotConstraints::default()
        };
        assert!(constraints
            .validate_metadata(None, SlotType::Multi, &symbols)
            .is_err());
        let constraints = RuntimeSlotConstraints {
            range: Some(NumericRange {
                min: Some(NumericBound::Integer(4)),
                max: Some(NumericBound::Integer(2)),
            }),
            ..RuntimeSlotConstraints::default()
        };
        assert!(constraints
            .validate_metadata(None, SlotType::Single, &symbols)
            .is_err());
    }

    #[test]
    fn numeric_whitelists_conflict_with_ranges_but_symbol_whitelists_do_not() {
        let (_, mut constraints, symbols) = compiled("(slot x (allowed-symbols red) (range 0 2))");
        assert!(constraints.validate_field(&Value::Integer(1)).is_ok());
        assert!(constraints.validate_field(&Value::Integer(3)).is_err());
        for values in [vec![], vec![Value::Integer(1)]] {
            constraints.allowed_values.push(RuntimeAllowedValueSet {
                kind: SlotValueType::Integer,
                values,
            });
            assert!(constraints
                .validate_metadata(None, SlotType::Single, &symbols)
                .unwrap_err()
                .contains("numeric range conflicts"));
            constraints.allowed_values.pop();
        }
    }
}

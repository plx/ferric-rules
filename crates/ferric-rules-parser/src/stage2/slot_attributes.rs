//! Template attribute syntax and declaration-level constraint conflicts.

use super::{
    interpret_action_expr_inner, interpret_action_expr_sequence, interpret_slot_types, ActionExpr,
    AllowedValueSet, Atom, Cardinality, DefaultValue, InterpretError, LiteralKind, LiteralValue,
    NumericBound, NumericRange, SExpr, SlotConstraints, SlotType, SlotValueType, Span,
};

#[derive(Default)]
pub(super) struct Attributes {
    pub allowed_types: Option<Vec<SlotValueType>>,
    pub constraints: SlotConstraints,
    pub default: Option<DefaultValue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AllowedFacet {
    Symbols,
    Strings,
    Lexemes,
    Integers,
    Floats,
    Numbers,
    Values,
}

impl AllowedFacet {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "allowed-symbols" => Self::Symbols,
            "allowed-strings" => Self::Strings,
            "allowed-lexemes" => Self::Lexemes,
            "allowed-integers" => Self::Integers,
            "allowed-floats" => Self::Floats,
            "allowed-numbers" => Self::Numbers,
            "allowed-values" => Self::Values,
            _ => return None,
        })
    }

    fn kinds(self) -> &'static [SlotValueType] {
        use SlotValueType::{Float, InstanceName, Integer, String, Symbol};
        match self {
            Self::Symbols => &[Symbol],
            Self::Strings => &[String],
            Self::Lexemes => &[Symbol, String],
            Self::Integers => &[Integer],
            Self::Floats => &[Float],
            Self::Numbers => &[Integer, Float],
            // Addresses have no literal whitelist representation and remain
            // unrestricted by allowed-values, unless (type ...) excludes them.
            Self::Values => &[Symbol, String, Integer, Float, InstanceName],
        }
    }

    fn conflicts_with_range(self) -> bool {
        matches!(
            self,
            Self::Integers | Self::Floats | Self::Numbers | Self::Values
        )
    }
}

struct AllowedAttribute {
    facet: AllowedFacet,
    /// None means ?VARIABLE: unrestricted, but the attribute remains present.
    values: Option<Vec<LiteralValue>>,
    span: Span,
}

fn variable_placeholder(expression: &SExpr) -> bool {
    matches!(expression.as_atom(), Some(Atom::SingleVar(name)) if name == "VARIABLE")
}

pub(super) fn interpret_attributes(
    options: &[SExpr],
    slot_type: SlotType,
) -> Result<Attributes, InterpretError> {
    let mut attributes = Attributes::default();
    let mut saw_type = false;
    let mut allowed = Vec::new();
    let mut range_span = None;
    for expression in options {
        let option = expression
            .as_list()
            .ok_or_else(|| InterpretError::expected("slot attribute list", expression.span()))?;
        let name = option
            .first()
            .and_then(SExpr::as_symbol)
            .ok_or_else(|| InterpretError::expected("slot attribute name", expression.span()))?;
        let values = &option[1..];
        let span = expression.span();
        match name {
            "default" | "default-dynamic" => {
                if attributes.default.is_some() {
                    return Err(InterpretError::invalid("duplicate default attribute", span));
                }
                attributes.default = Some(interpret_default(
                    values,
                    slot_type,
                    name == "default-dynamic",
                    span,
                )?);
            }
            "type" => {
                if std::mem::replace(&mut saw_type, true) {
                    return Err(InterpretError::invalid("duplicate type attribute", span));
                }
                attributes.allowed_types = interpret_slot_types(values, span)?;
            }
            "range" => {
                if attributes.constraints.range.is_some() {
                    return Err(InterpretError::invalid("duplicate range attribute", span));
                }
                attributes.constraints.range = Some(interpret_range(values, span)?);
                range_span = Some(span);
            }
            "cardinality" => {
                if slot_type != SlotType::Multi {
                    return Err(InterpretError::invalid(
                        "[CSTRNPSR5] cardinality requires a multifield slot",
                        span,
                    ));
                }
                if attributes.constraints.cardinality.is_some() {
                    return Err(InterpretError::invalid(
                        "duplicate cardinality attribute",
                        span,
                    ));
                }
                attributes.constraints.cardinality = Some(interpret_cardinality(values, span)?);
            }
            _ => {
                let facet = AllowedFacet::from_name(name).ok_or_else(|| {
                    InterpretError::invalid(&format!("unsupported slot attribute `{name}`"), span)
                })?;
                allowed.push(interpret_allowed(facet, values, span, &allowed)?);
            }
        }
    }
    validate_declaration(&attributes, &allowed, range_span)?;
    for attribute in allowed {
        if let Some(values) = attribute.values {
            for &kind in attribute.facet.kinds() {
                attributes.constraints.allowed_values.push(AllowedValueSet {
                    kind,
                    values: values
                        .iter()
                        .filter(|value| literal_kind(&value.value) == kind)
                        .cloned()
                        .collect(),
                });
            }
        }
    }
    attributes
        .constraints
        .allowed_values
        .sort_by_key(|set| set.kind);
    Ok(attributes)
}

fn interpret_allowed(
    facet: AllowedFacet,
    values: &[SExpr],
    span: Span,
    previous: &[AllowedAttribute],
) -> Result<AllowedAttribute, InterpretError> {
    for attribute in previous {
        if attribute.facet == facet {
            return Err(InterpretError::invalid(
                "duplicate allowed-values attribute",
                span,
            ));
        }
        if attribute
            .facet
            .kinds()
            .iter()
            .any(|kind| facet.kinds().contains(kind))
        {
            return Err(InterpretError::invalid(
                "[CSTRNPSR3] overlapping allowed-values attributes cannot be combined",
                span,
            ));
        }
    }
    if values.len() == 1 && variable_placeholder(&values[0]) {
        return Ok(AllowedAttribute {
            facet,
            values: None,
            span,
        });
    }
    if values.is_empty() {
        return Err(InterpretError::missing("allowed value or ?VARIABLE", span));
    }
    let values = values
        .iter()
        .map(|value| {
            let ActionExpr::Literal(literal) = interpret_action_expr_inner(value)? else {
                return Err(InterpretError::invalid(
                    "allowed-values attributes require literals or a sole ?VARIABLE",
                    value.span(),
                ));
            };
            if !facet.kinds().contains(&literal_kind(&literal.value)) {
                return Err(InterpretError::invalid(
                    "[CSTRNPSR4] value does not match the allowed-values attribute category",
                    value.span(),
                ));
            }
            Ok(literal)
        })
        .collect::<Result<_, _>>()?;
    Ok(AllowedAttribute {
        facet,
        values: Some(values),
        span,
    })
}

fn literal_kind(value: &LiteralKind) -> SlotValueType {
    match value {
        LiteralKind::Symbol(_) => SlotValueType::Symbol,
        LiteralKind::String(_) => SlotValueType::String,
        LiteralKind::Integer(_) => SlotValueType::Integer,
        LiteralKind::Float(_) => SlotValueType::Float,
        LiteralKind::InstanceName(_) => SlotValueType::InstanceName,
    }
}

fn validate_declaration(
    attributes: &Attributes,
    allowed: &[AllowedAttribute],
    range_span: Option<Span>,
) -> Result<(), InterpretError> {
    for attribute in allowed {
        if attributes.constraints.range.is_some() && attribute.facet.conflicts_with_range() {
            return Err(InterpretError::invalid(
                "[CSTRNPSR3] range cannot be combined with allowed numeric values",
                attribute.span,
            ));
        }
        let (Some(types), Some(values)) = (&attributes.allowed_types, &attribute.values) else {
            continue;
        };
        let compatible = if attribute.facet == AllowedFacet::Values {
            values
                .iter()
                .all(|value| types.contains(&literal_kind(&value.value)))
        } else {
            attribute
                .facet
                .kinds()
                .iter()
                .all(|kind| types.contains(kind))
        };
        if !compatible {
            return Err(InterpretError::invalid(
                "[CSTRNPSR1] type conflicts with allowed-values attribute",
                attribute.span,
            ));
        }
    }
    if let (Some(types), Some(range)) = (&attributes.allowed_types, attributes.constraints.range) {
        for bound in [range.min, range.max].into_iter().flatten() {
            let kind = match bound {
                NumericBound::Integer(_) => SlotValueType::Integer,
                NumericBound::Float(_) => SlotValueType::Float,
            };
            if !types.contains(&kind) {
                return Err(InterpretError::invalid(
                    "[CSTRNPSR1] type conflicts with range attribute",
                    range_span.expect("parsed range retains its source span"),
                ));
            }
        }
    }
    Ok(())
}

fn numeric_bound(value: &SExpr) -> Result<Option<NumericBound>, InterpretError> {
    if variable_placeholder(value) {
        return Ok(None);
    }
    match value.as_atom() {
        Some(Atom::Integer(value)) => Ok(Some(NumericBound::Integer(*value))),
        Some(Atom::Float(value)) => Ok(Some(NumericBound::Float(*value))),
        _ => Err(InterpretError::expected(
            "numeric range endpoint or ?VARIABLE",
            value.span(),
        )),
    }
}

fn interpret_range(values: &[SExpr], span: Span) -> Result<NumericRange, InterpretError> {
    let [min, max] = values else {
        return Err(InterpretError::invalid(
            "range requires two endpoints",
            span,
        ));
    };
    let range = NumericRange {
        min: numeric_bound(min)?,
        max: numeric_bound(max)?,
    };
    if let (Some(minimum), Some(maximum)) = (range.min, range.max) {
        if bound_greater(minimum, maximum) {
            return Err(InterpretError::invalid(
                "[CSTRNPSR2] minimum range must not exceed maximum range",
                span,
            ));
        }
    }
    Ok(range)
}

#[allow(clippy::cast_precision_loss)] // CLIPS promotes mixed numeric endpoint comparisons to double.
fn bound_greater(min: NumericBound, max: NumericBound) -> bool {
    match (min, max) {
        (NumericBound::Integer(a), NumericBound::Integer(b)) => a > b,
        (NumericBound::Float(a), NumericBound::Float(b)) => a > b,
        (NumericBound::Integer(a), NumericBound::Float(b)) => (a as f64) > b,
        (NumericBound::Float(a), NumericBound::Integer(b)) => a > (b as f64),
    }
}

fn cardinality_bound(value: &SExpr) -> Result<Option<u64>, InterpretError> {
    if variable_placeholder(value) {
        return Ok(None);
    }
    let Some(Atom::Integer(number)) = value.as_atom() else {
        return Err(InterpretError::expected(
            "integer cardinality or ?VARIABLE",
            value.span(),
        ));
    };
    u64::try_from(*number).map(Some).map_err(|_| {
        InterpretError::invalid("[CSTRNPSR6] cardinality must be nonnegative", value.span())
    })
}

fn interpret_cardinality(values: &[SExpr], span: Span) -> Result<Cardinality, InterpretError> {
    let [min, max] = values else {
        return Err(InterpretError::invalid(
            "cardinality requires two bounds",
            span,
        ));
    };
    let cardinality = Cardinality {
        min: cardinality_bound(min)?.unwrap_or(0),
        max: cardinality_bound(max)?,
    };
    if cardinality.max.is_some_and(|max| cardinality.min > max) {
        return Err(InterpretError::invalid(
            "[CSTRNPSR2] minimum cardinality must not exceed maximum cardinality",
            span,
        ));
    }
    Ok(cardinality)
}

fn interpret_default(
    values: &[SExpr],
    slot_type: SlotType,
    dynamic: bool,
    span: Span,
) -> Result<DefaultValue, InterpretError> {
    for value in values {
        if let Some(Atom::SingleVar(name)) = value.as_atom() {
            if name.eq_ignore_ascii_case("NONE") || name.eq_ignore_ascii_case("DERIVE") {
                if values.len() != 1 || dynamic {
                    return Err(InterpretError::invalid(
                        "?NONE and ?DERIVE must be the entire static default",
                        span,
                    ));
                }
                return Ok(if name.eq_ignore_ascii_case("NONE") {
                    DefaultValue::None
                } else {
                    DefaultValue::Derive
                });
            }
        }
    }
    let expressions = interpret_action_expr_sequence(values)?;
    if slot_type == SlotType::Single
        && (expressions.len() != 1
            || matches!(&expressions[0], ActionExpr::FunctionCall(call) if call.name == "create$"))
    {
        return Err(InterpretError::invalid(
            "[DEFAULT1] single-field default requires one scalar value",
            span,
        ));
    }
    if dynamic {
        return Ok(DefaultValue::Dynamic(expressions));
    }
    if let [ActionExpr::Literal(literal)] = expressions.as_slice() {
        return Ok(DefaultValue::Value(literal.clone()));
    }
    let mut literals = Vec::new();
    if expressions
        .iter()
        .all(|expression| collect_literal_fields(expression, &mut literals))
    {
        Ok(DefaultValue::Values(literals))
    } else {
        Ok(DefaultValue::Expressions(expressions))
    }
}

fn collect_literal_fields(expression: &ActionExpr, fields: &mut Vec<LiteralValue>) -> bool {
    match expression {
        ActionExpr::Literal(literal) => {
            fields.push(literal.clone());
            true
        }
        ActionExpr::FunctionCall(call) if call.name == "create$" => call
            .args
            .iter()
            .all(|expression| collect_literal_fields(expression, fields)),
        _ => false,
    }
}

#[cfg(test)]
mod tests;

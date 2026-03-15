package ferric

// Ergonomic constructors for wire types.

// IntValue creates an integer WireValue.
func IntValue(v int64) WireValue {
	return WireValue{Kind: WireValueInteger, Integer: v}
}

// FloatValue creates a float WireValue.
func FloatValue(v float64) WireValue {
	return WireValue{Kind: WireValueFloat, Float: v}
}

// SymbolValue creates a symbol WireValue.
func SymbolValue(v string) WireValue {
	return WireValue{Kind: WireValueSymbol, Text: v}
}

// StringValue creates a string WireValue.
func StringValue(v string) WireValue {
	return WireValue{Kind: WireValueString, Text: v}
}

// MultifieldValue creates a multifield WireValue.
func MultifieldValue(v ...WireValue) WireValue {
	return WireValue{Kind: WireValueMultifield, Multifield: v}
}

// OrderedFact creates a WireFactInput for an ordered fact.
func OrderedFact(relation string, fields ...WireValue) WireFactInput {
	return WireFactInput{
		Kind: WireFactKindOrdered,
		Ordered: &WireOrderedFactInput{
			Relation: relation,
			Fields:   fields,
		},
	}
}

// TemplateFact creates a WireFactInput for a template fact.
func TemplateFact(templateName string, slots map[string]WireValue) WireFactInput {
	return WireFactInput{
		Kind: WireFactKindTemplate,
		Template: &WireTemplateFactInput{
			TemplateName: templateName,
			Slots:        slots,
		},
	}
}

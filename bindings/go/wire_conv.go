package ferric

import "fmt"

// --- Go-native → Wire conversion ---

// NativeToWireValue converts a Go-native value to a WireValue.
func NativeToWireValue(v any) (WireValue, error) {
	switch val := v.(type) {
	case nil:
		return WireValue{Kind: WireValueVoid}, nil
	case int:
		return WireValue{Kind: WireValueInteger, Integer: int64(val)}, nil
	case int32:
		return WireValue{Kind: WireValueInteger, Integer: int64(val)}, nil
	case int64:
		return WireValue{Kind: WireValueInteger, Integer: val}, nil
	case float32:
		return WireValue{Kind: WireValueFloat, Float: float64(val)}, nil
	case float64:
		return WireValue{Kind: WireValueFloat, Float: val}, nil
	case Symbol:
		return WireValue{Kind: WireValueSymbol, Text: string(val)}, nil
	case string:
		return WireValue{Kind: WireValueString, Text: val}, nil
	case bool:
		sym := "FALSE"
		if val {
			sym = "TRUE"
		}
		return WireValue{Kind: WireValueSymbol, Text: sym}, nil
	case []any:
		elements := make([]WireValue, len(val))
		for i, elem := range val {
			w, err := NativeToWireValue(elem)
			if err != nil {
				return WireValue{}, err
			}
			elements[i] = w
		}
		return WireValue{Kind: WireValueMultifield, Multifield: elements}, nil
	default:
		return WireValue{}, fmt.Errorf("unsupported type for wire conversion: %T", v)
	}
}

// WireToNativeValue converts a WireValue to a Go-native value.
func WireToNativeValue(w WireValue) (any, error) {
	switch w.Kind {
	case WireValueVoid:
		return nil, nil
	case WireValueInteger:
		return w.Integer, nil
	case WireValueFloat:
		return w.Float, nil
	case WireValueSymbol:
		return Symbol(w.Text), nil
	case WireValueString:
		return w.Text, nil
	case WireValueMultifield:
		result := make([]any, len(w.Multifield))
		for i, elem := range w.Multifield {
			v, err := WireToNativeValue(elem)
			if err != nil {
				return nil, err
			}
			result[i] = v
		}
		return result, nil
	default:
		return nil, fmt.Errorf("unknown wire value kind: %q", w.Kind)
	}
}

// WireSliceToNative converts a slice of WireValues to Go-native values.
func WireSliceToNative(ws []WireValue) ([]any, error) {
	result := make([]any, len(ws))
	for i, w := range ws {
		v, err := WireToNativeValue(w)
		if err != nil {
			return nil, err
		}
		result[i] = v
	}
	return result, nil
}

// WireMapToNative converts a map of WireValues to Go-native values.
func WireMapToNative(ws map[string]WireValue) (map[string]any, error) {
	result := make(map[string]any, len(ws))
	for k, w := range ws {
		v, err := WireToNativeValue(w)
		if err != nil {
			return nil, err
		}
		result[k] = v
	}
	return result, nil
}

// --- Fact conversion ---

// FactToWire converts a native Fact to a WireFact.
func FactToWire(f Fact) (WireFact, error) {
	wf := WireFact{ID: f.ID}
	if f.Type == FactTemplate {
		wf.Kind = WireFactKindTemplate
		slots := make(map[string]WireValue, len(f.Slots))
		for k, v := range f.Slots {
			w, err := NativeToWireValue(v)
			if err != nil {
				return WireFact{}, err
			}
			slots[k] = w
		}
		wf.Template = &WireTemplateFact{
			TemplateName: f.TemplateName,
			Slots:        slots,
		}
	} else {
		wf.Kind = WireFactKindOrdered
		fields := make([]WireValue, len(f.Fields))
		for i, v := range f.Fields {
			w, err := NativeToWireValue(v)
			if err != nil {
				return WireFact{}, err
			}
			fields[i] = w
		}
		wf.Ordered = &WireOrderedFact{
			Relation: f.Relation,
			Fields:   fields,
		}
	}
	return wf, nil
}

// FactsToWire converts a slice of native Facts to WireFacts.
func FactsToWire(facts []Fact) ([]WireFact, error) {
	result := make([]WireFact, len(facts))
	for i, f := range facts {
		wf, err := FactToWire(f)
		if err != nil {
			return nil, err
		}
		result[i] = wf
	}
	return result, nil
}

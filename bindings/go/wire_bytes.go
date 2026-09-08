package ferric

import (
	"encoding/json"
	"fmt"
	"unicode/utf8"
)

// MarshalJSON retains the existing text wire shape and explicitly tags raw bytes.
func (w WireValue) MarshalJSON() ([]byte, error) {
	type wireValueJSON WireValue
	if (w.Kind == WireValueString || w.Kind == WireValueSymbol) && !utf8.ValidString(w.Text) {
		if w.Kind == WireValueString {
			w.Kind = WireValueStringBytes
		} else {
			w.Kind = WireValueSymbolBytes
		}
		w.Bytes, w.Text = []byte(w.Text), ""
	}
	data, err := json.Marshal(wireValueJSON(w))
	if err != nil {
		return nil, fmt.Errorf("encode wire value: %w", err)
	}
	return data, nil
}

// MarshalJSON prevents encoding/json from replacing invalid output UTF-8.
// Valid text stays in output; arbitrary bytes use base64 in output_bytes.
func (r EvaluateResult) MarshalJSON() ([]byte, error) {
	type resultJSON EvaluateResult
	text := make(map[string]string)
	raw := make(map[string][]byte)
	for key, value := range r.Output {
		if utf8.ValidString(value) {
			text[key] = value
		} else {
			raw[key] = []byte(value)
		}
	}
	r.Output = text
	data, err := json.Marshal(struct {
		resultJSON
		OutputBytes map[string][]byte `json:"output_bytes,omitempty"`
	}{resultJSON(r), raw})
	if err != nil {
		return nil, fmt.Errorf("encode evaluation result: %w", err)
	}
	return data, nil
}

// UnmarshalJSON restores exact output bytes into ordinary Go strings.
func (r *EvaluateResult) UnmarshalJSON(data []byte) error {
	type resultJSON EvaluateResult
	var decoded struct {
		resultJSON
		OutputBytes map[string][]byte `json:"output_bytes,omitempty"`
	}
	if err := json.Unmarshal(data, &decoded); err != nil {
		return fmt.Errorf("decode evaluation result: %w", err)
	}
	*r = EvaluateResult(decoded.resultJSON)
	if len(decoded.OutputBytes) > 0 && r.Output == nil {
		r.Output = make(map[string]string)
	}
	for key, value := range decoded.OutputBytes {
		r.Output[key] = string(value)
	}
	return nil
}

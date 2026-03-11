package ferric

// Wire types provide a stable, tagged schema for cross-language
// serialization (e.g., Temporal payloads). These are the types
// that appear in EvaluateRequest/EvaluateResult.

// WireValueKind discriminates value types in the wire format.
type WireValueKind string

const (
	WireValueVoid       WireValueKind = "void"
	WireValueInteger    WireValueKind = "integer"
	WireValueFloat      WireValueKind = "float"
	WireValueSymbol     WireValueKind = "symbol"
	WireValueString     WireValueKind = "string"
	WireValueMultifield WireValueKind = "multifield"
)

// WireValue is a tagged value in the wire format.
type WireValue struct {
	Kind       WireValueKind `json:"kind"`
	Integer    int64         `json:"integer,omitempty"`
	Float      float64       `json:"float,omitempty"`
	Text       string        `json:"text,omitempty"`       // symbol/string payload
	Multifield []WireValue   `json:"multifield,omitempty"` // recursive
}

// WireFactKind discriminates fact types in the wire format.
type WireFactKind string

const (
	WireFactKindOrdered  WireFactKind = "ordered"
	WireFactKindTemplate WireFactKind = "template"
)

// WireOrderedFactInput is the wire representation of an ordered fact to assert.
type WireOrderedFactInput struct {
	Relation string      `json:"relation"`
	Fields   []WireValue `json:"fields,omitempty"`
}

// WireTemplateFactInput is the wire representation of a template fact to assert.
type WireTemplateFactInput struct {
	TemplateName string               `json:"template_name"`
	Slots        map[string]WireValue `json:"slots,omitempty"`
}

// WireFactInput is a tagged fact input for assertion.
type WireFactInput struct {
	Kind     WireFactKind           `json:"kind"`
	Ordered  *WireOrderedFactInput  `json:"ordered,omitempty"`
	Template *WireTemplateFactInput `json:"template,omitempty"`
}

// EvaluateRequest describes facts to assert and evaluation parameters.
type EvaluateRequest struct {
	Facts []WireFactInput `json:"facts"`
	Limit int             `json:"limit,omitempty"` // 0 = unlimited
}

// WireOrderedFact is the wire representation of an ordered fact in results.
type WireOrderedFact struct {
	Relation string      `json:"relation"`
	Fields   []WireValue `json:"fields,omitempty"`
}

// WireTemplateFact is the wire representation of a template fact in results.
type WireTemplateFact struct {
	TemplateName string               `json:"template_name"`
	Slots        map[string]WireValue `json:"slots,omitempty"`
}

// WireFact is a tagged fact in evaluation results.
type WireFact struct {
	ID       uint64           `json:"id"`
	Kind     WireFactKind     `json:"kind"`
	Ordered  *WireOrderedFact `json:"ordered,omitempty"`
	Template *WireTemplateFact `json:"template,omitempty"`
}

// EvaluateResult contains the full outcome of an evaluation.
type EvaluateResult struct {
	RunResult
	Facts  []WireFact        `json:"facts"`
	Output map[string]string `json:"output,omitempty"`
}

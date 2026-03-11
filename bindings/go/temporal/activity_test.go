package temporal

import (
	"context"
	"encoding/json"
	"testing"

	ferric "github.com/prb/ferric-rules/bindings/go"
)

func TestNewRulesActivity(t *testing.T) {
	ra, err := NewRulesActivity(
		[]ferric.EngineSpec{
			{Name: "test", Options: []ferric.EngineOption{
				ferric.WithSource(`(defrule r (x ?v) => (printout t ?v crlf))`),
			}},
		},
		ferric.Threads(1),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer ra.Close()
}

func TestNewRulesActivityMultipleSpecs(t *testing.T) {
	ra, err := NewRulesActivity(
		[]ferric.EngineSpec{
			{Name: "risk", Options: []ferric.EngineOption{ferric.WithSource(`(defrule r => (assert (ok)))`)}},
			{Name: "pricing", Options: []ferric.EngineOption{ferric.WithSource(`(defrule p => (assert (priced)))`)}},
		},
		ferric.Threads(2),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer ra.Close()
}

func TestRulesActivityDirectEvaluate(t *testing.T) {
	// Test the activity evaluation path directly (without Temporal worker).
	ra, err := NewRulesActivity(
		[]ferric.EngineSpec{
			{Name: "greet", Options: []ferric.EngineOption{
				ferric.WithSource(`(defrule greet (person ?n) => (printout t "Hello " ?n crlf))`),
			}},
		},
	)
	if err != nil {
		t.Fatal(err)
	}
	defer ra.Close()

	// Access the manager directly to test evaluate.
	mgr := ra.managers["greet"]
	result, err := mgr.Evaluate(context.Background(), &ferric.EvaluateRequest{
		Facts: []ferric.WireFactInput{
			ferric.OrderedFact("person", ferric.StringValue("World")),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
	if result.Output["stdout"] != "Hello World\n" {
		t.Fatalf("unexpected output: %q", result.Output["stdout"])
	}
}

func TestWirePayloadJSONRoundtrip(t *testing.T) {
	// Verify that wire types serialize/deserialize correctly for Temporal.
	req := &ferric.EvaluateRequest{
		Facts: []ferric.WireFactInput{
			ferric.OrderedFact("age", ferric.IntValue(35)),
			ferric.OrderedFact("score", ferric.FloatValue(720.5)),
			ferric.TemplateFact("person", map[string]ferric.WireValue{
				"name": ferric.StringValue("Alice"),
				"role": ferric.SymbolValue("admin"),
			}),
		},
		Limit: 100,
	}

	data, err := json.Marshal(req)
	if err != nil {
		t.Fatal(err)
	}

	var req2 ferric.EvaluateRequest
	if err := json.Unmarshal(data, &req2); err != nil {
		t.Fatal(err)
	}

	if len(req2.Facts) != 3 {
		t.Fatalf("expected 3 facts, got %d", len(req2.Facts))
	}
	if req2.Limit != 100 {
		t.Fatalf("expected limit 100, got %d", req2.Limit)
	}

	// Check ordered facts.
	f0 := req2.Facts[0]
	if f0.Kind != ferric.WireFactKindOrdered {
		t.Fatalf("expected ordered, got %q", f0.Kind)
	}
	if f0.Ordered.Relation != "age" {
		t.Fatalf("expected 'age', got %q", f0.Ordered.Relation)
	}
	if f0.Ordered.Fields[0].Kind != ferric.WireValueInteger {
		t.Fatalf("expected integer, got %q", f0.Ordered.Fields[0].Kind)
	}
	if f0.Ordered.Fields[0].Integer != 35 {
		t.Fatalf("expected 35, got %d", f0.Ordered.Fields[0].Integer)
	}

	// Check template fact.
	f2 := req2.Facts[2]
	if f2.Kind != ferric.WireFactKindTemplate {
		t.Fatalf("expected template, got %q", f2.Kind)
	}
	if f2.Template.TemplateName != "person" {
		t.Fatalf("expected 'person', got %q", f2.Template.TemplateName)
	}
	nameSlot := f2.Template.Slots["name"]
	if nameSlot.Kind != ferric.WireValueString || nameSlot.Text != "Alice" {
		t.Fatalf("expected string 'Alice', got %v", nameSlot)
	}
	roleSlot := f2.Template.Slots["role"]
	if roleSlot.Kind != ferric.WireValueSymbol || roleSlot.Text != "admin" {
		t.Fatalf("expected symbol 'admin', got %v", roleSlot)
	}
}

func TestWireResultJSONRoundtrip(t *testing.T) {
	result := &ferric.EvaluateResult{
		RunResult: ferric.RunResult{RulesFired: 3, HaltReason: ferric.HaltAgendaEmpty},
		Facts: []ferric.WireFact{
			{
				ID:   1,
				Kind: ferric.WireFactKindOrdered,
				Ordered: &ferric.WireOrderedFact{
					Relation: "color",
					Fields:   []ferric.WireValue{ferric.SymbolValue("red")},
				},
			},
			{
				ID:   2,
				Kind: ferric.WireFactKindTemplate,
				Template: &ferric.WireTemplateFact{
					TemplateName: "person",
					Slots: map[string]ferric.WireValue{
						"name": ferric.StringValue("Alice"),
						"age":  ferric.IntValue(30),
					},
				},
			},
		},
		Output: map[string]string{"stdout": "Hello\n"},
	}

	data, err := json.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}

	var result2 ferric.EvaluateResult
	if err := json.Unmarshal(data, &result2); err != nil {
		t.Fatal(err)
	}

	if result2.RulesFired != 3 {
		t.Fatalf("expected 3, got %d", result2.RulesFired)
	}
	if len(result2.Facts) != 2 {
		t.Fatalf("expected 2 facts, got %d", len(result2.Facts))
	}
	if result2.Output["stdout"] != "Hello\n" {
		t.Fatalf("expected 'Hello\\n', got %q", result2.Output["stdout"])
	}

	// Check template fact roundtrip.
	tf := result2.Facts[1]
	if tf.Kind != ferric.WireFactKindTemplate {
		t.Fatalf("expected template, got %q", tf.Kind)
	}
	if tf.Template.Slots["age"].Integer != 30 {
		t.Fatalf("expected age 30, got %d", tf.Template.Slots["age"].Integer)
	}
}

func TestWireMultifieldRoundtrip(t *testing.T) {
	mf := ferric.MultifieldValue(
		ferric.IntValue(1),
		ferric.StringValue("hello"),
		ferric.FloatValue(3.14),
		ferric.SymbolValue("sym"),
	)

	data, err := json.Marshal(mf)
	if err != nil {
		t.Fatal(err)
	}

	var mf2 ferric.WireValue
	if err := json.Unmarshal(data, &mf2); err != nil {
		t.Fatal(err)
	}

	if mf2.Kind != ferric.WireValueMultifield {
		t.Fatalf("expected multifield, got %q", mf2.Kind)
	}
	if len(mf2.Multifield) != 4 {
		t.Fatalf("expected 4 elements, got %d", len(mf2.Multifield))
	}
	if mf2.Multifield[0].Integer != 1 {
		t.Fatalf("expected 1, got %d", mf2.Multifield[0].Integer)
	}
	if mf2.Multifield[1].Text != "hello" {
		t.Fatalf("expected 'hello', got %q", mf2.Multifield[1].Text)
	}
}

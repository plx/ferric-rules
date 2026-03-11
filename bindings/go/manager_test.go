package ferric

import (
	"context"
	"encoding/json"
	"sync"
	"testing"
)

func TestManagerEvaluate(t *testing.T) {
	mgr, err := NewManager(WithSource(`
		(defrule greet
			(person ?name)
			=>
			(printout t "Hello, " ?name "!" crlf))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
		Facts: []WireFactInput{
			OrderedFact("person", StringValue("Alice")),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
	if result.Output["stdout"] != "Hello, Alice!\n" {
		t.Fatalf("unexpected output: %q", result.Output["stdout"])
	}

	// Verify result facts contain the person fact.
	found := false
	for _, f := range result.Facts {
		if f.Kind == WireFactKindOrdered && f.Ordered != nil && f.Ordered.Relation == "person" {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("expected person fact in results")
	}
}

func TestManagerEvaluateTemplate(t *testing.T) {
	mgr, err := NewManager(WithSource(`
		(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
		(defrule alert
			(sensor (id ?id) (value ?v&:(> ?v 100.0)))
			=>
			(printout t "Alert: " ?id crlf))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
		Facts: []WireFactInput{
			TemplateFact("sensor", map[string]WireValue{
				"id":    IntValue(1),
				"value": FloatValue(150.0),
			}),
			TemplateFact("sensor", map[string]WireValue{
				"id":    IntValue(2),
				"value": FloatValue(50.0),
			}),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
	if result.Output["stdout"] != "Alert: 1\n" {
		t.Fatalf("unexpected output: %q", result.Output["stdout"])
	}
}

func TestManagerEvaluateNative(t *testing.T) {
	mgr, err := NewManager(WithSource(`
		(defrule greet (person ?name) => (printout t "Hi " ?name crlf))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.EvaluateNative(context.Background(), &EvaluateNativeRequest{
		Facts: []NativeFactInput{
			{Relation: "person", Fields: []any{Symbol("Bob")}},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
	if result.Output["stdout"] != "Hi Bob\n" {
		t.Fatalf("unexpected output: %q", result.Output["stdout"])
	}
}

func TestManagerEvaluateStateless(t *testing.T) {
	// Verify that consecutive evaluations are independent.
	mgr, err := NewManager(WithSource(`
		(defrule count (item ?x) => (printout t ?x crlf))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	// First evaluation
	r1, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
		Facts: []WireFactInput{
			OrderedFact("item", SymbolValue("A")),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if r1.RulesFired != 1 {
		t.Fatalf("eval 1: expected 1, got %d", r1.RulesFired)
	}

	// Second evaluation — facts from first should be gone.
	r2, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
		Facts: []WireFactInput{
			OrderedFact("item", SymbolValue("B")),
			OrderedFact("item", SymbolValue("C")),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if r2.RulesFired != 2 {
		t.Fatalf("eval 2: expected 2, got %d", r2.RulesFired)
	}
}

func TestManagerConcurrentEvaluate(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{
			{Name: "test", Options: []EngineOption{WithSource(`
				(defrule echo (msg ?x) => (printout t ?x crlf))
			`)}},
		},
		Threads(4),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer coord.Close()

	mgr, _ := coord.Manager("test")

	var wg sync.WaitGroup
	for i := range 20 {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			result, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
				Facts: []WireFactInput{
					OrderedFact("msg", SymbolValue("hello")),
				},
			})
			if err != nil {
				t.Errorf("goroutine %d: %v", i, err)
				return
			}
			if result.RulesFired != 1 {
				t.Errorf("goroutine %d: expected 1, got %d", i, result.RulesFired)
			}
		}(i)
	}
	wg.Wait()
}

func TestManagerDoEngineReuse(t *testing.T) {
	// Verify that Do reuses the same engine across calls (rules stay compiled).
	mgr, err := NewManager(WithSource(`
		(defglobal ?*counter* = 0)
		(defrule inc (trigger) => (bind ?*counter* (+ ?*counter* 1)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	// Call Do multiple times. Each time reset and assert, but the global
	// should be re-initialized by reset.
	for range 3 {
		err = mgr.Do(context.Background(), func(e *Engine) error {
			e.Reset()
			e.AssertFact("trigger")
			e.Run(context.Background())
			val, err := e.GetGlobal("counter")
			if err != nil {
				return err
			}
			if val != int64(1) {
				t.Errorf("expected 1, got %v", val)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
}

func TestNewManagerConvenience(t *testing.T) {
	mgr, err := NewManager(WithSource(`(defrule r => (assert (ok)))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.Evaluate(context.Background(), &EvaluateRequest{})
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
}

func TestEvaluateResultJSONRoundtrip(t *testing.T) {
	mgr, err := NewManager(WithSource(`(defrule r (x ?v) => (assert (y ?v)))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
		Facts: []WireFactInput{
			OrderedFact("x", IntValue(42)),
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	// Serialize to JSON.
	data, err := json.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}

	// Deserialize back.
	var result2 EvaluateResult
	if err := json.Unmarshal(data, &result2); err != nil {
		t.Fatal(err)
	}

	if result2.RulesFired != result.RulesFired {
		t.Fatalf("roundtrip: rules fired mismatch: %d vs %d", result2.RulesFired, result.RulesFired)
	}
	if len(result2.Facts) != len(result.Facts) {
		t.Fatalf("roundtrip: fact count mismatch: %d vs %d", len(result2.Facts), len(result.Facts))
	}
}

func TestManagerEvaluateWithLimit(t *testing.T) {
	mgr, err := NewManager(WithSource(`
		(defrule r1 => (assert (a)))
		(defrule r2 (a) => (assert (b)))
		(defrule r3 (b) => (assert (c)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mgr.Close()

	result, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
		Limit: 2,
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 2 {
		t.Fatalf("expected 2, got %d", result.RulesFired)
	}
}

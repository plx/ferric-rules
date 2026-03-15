package ferric

import (
	"context"
	"errors"
	"slices"
	"testing"
)

// ---------------------------------------------------------------------------
// Engine.Serialize / WithSnapshot roundtrip
// ---------------------------------------------------------------------------

func TestSerializeRoundtrip(t *testing.T) {
	// Create an engine with rules + template + globals and serialize it.
	src := `
		(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
		(defrule alert
			(sensor (id ?id) (value ?v&:(> ?v 100.0)))
			=>
			(printout t "ALERT: sensor " ?id " value " ?v crlf))
		(defglobal ?*threshold* = 42)
	`
	e, err := NewEngine(WithSource(src))
	if err != nil {
		t.Fatal(err)
	}

	snap, err := e.Serialize()
	if err != nil {
		t.Fatalf("Serialize failed: %v", err)
	}
	mustClose(t, e)

	if len(snap) == 0 {
		t.Fatal("expected non-empty snapshot")
	}

	// Create a new engine from the snapshot.
	e2, err := NewEngine(WithSnapshot(snap))
	if err != nil {
		t.Fatalf("NewEngine(WithSnapshot) failed: %v", err)
	}
	defer mustClose(t, e2)

	// Verify rules survived.
	rules := e2.Rules()
	if len(rules) != 1 {
		t.Fatalf("expected 1 rule, got %d", len(rules))
	}
	if rules[0].Name != "alert" {
		t.Fatalf("expected rule 'alert', got %q", rules[0].Name)
	}

	// Verify templates survived.
	tmpls := e2.Templates()
	if !slices.Contains(tmpls, "sensor") {
		t.Fatalf("expected 'sensor' template, got: %v", tmpls)
	}

	// Verify globals survived.
	val, err := e2.GetGlobal("threshold")
	if err != nil {
		t.Fatalf("GetGlobal failed: %v", err)
	}
	if val != int64(42) {
		t.Fatalf("expected 42, got %v", val)
	}
}

func TestSnapshotEngineCanRun(t *testing.T) {
	// Create, serialize, restore, then run the engine and verify output.
	src := `
		(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
		(defrule alert
			(sensor (id ?id) (value ?v&:(> ?v 100.0)))
			=>
			(printout t "ALERT " ?id crlf))
	`
	e, err := NewEngine(WithSource(src))
	if err != nil {
		t.Fatal(err)
	}
	snap, err := e.Serialize()
	if err != nil {
		t.Fatal(err)
	}
	mustClose(t, e)

	// Restore and exercise.
	e2, err := NewEngine(WithSnapshot(snap))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e2)

	_, err = e2.AssertTemplate("sensor", map[string]any{
		"id":    int64(7),
		"value": 200.0,
	})
	if err != nil {
		t.Fatal(err)
	}

	result, err := e2.Run(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1 rule fired, got %d", result.RulesFired)
	}

	output, ok := e2.GetOutput("t")
	if !ok {
		t.Fatal("expected output")
	}
	if output != "ALERT 7\n" {
		t.Fatalf("unexpected output: %q", output)
	}
}

func TestSnapshotMultipleInstances(t *testing.T) {
	// A single snapshot can produce multiple independent engines.
	src := `
		(defrule count => (assert (counted)))
	`
	e, err := NewEngine(WithSource(src))
	if err != nil {
		t.Fatal(err)
	}
	snap, err := e.Serialize()
	if err != nil {
		t.Fatal(err)
	}
	mustClose(t, e)

	// Create two engines from the same snapshot.
	e1, err := NewEngine(WithSnapshot(snap))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e1)

	e2, err := NewEngine(WithSnapshot(snap))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e2)

	// Run e1 only.
	result1 := mustRun(context.Background(), t, e1)
	if result1.RulesFired != 1 {
		t.Fatalf("e1: expected 1 fired, got %d", result1.RulesFired)
	}

	// e2 should still have a clean agenda (not affected by e1).
	result2 := mustRun(context.Background(), t, e2)
	if result2.RulesFired != 1 {
		t.Fatalf("e2: expected 1 fired, got %d", result2.RulesFired)
	}
}

func TestDeserializeInvalidData(t *testing.T) {
	_, err := NewEngine(WithSnapshot([]byte("not valid snapshot data")))
	if err == nil {
		t.Fatal("expected error for invalid snapshot")
	}
	if !errors.Is(err, ErrSerialization) {
		t.Fatalf("expected SerializationError, got: %T %v", err, err)
	}
}

func TestSerializeEmptyEngine(t *testing.T) {
	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	snap, err := e.Serialize()
	if err != nil {
		t.Fatalf("Serialize failed: %v", err)
	}
	if len(snap) == 0 {
		t.Fatal("expected non-empty snapshot even for empty engine")
	}

	// Roundtrip: create from snapshot and verify it's functional.
	e2, err := NewEngine(WithSnapshot(snap))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e2)

	// Should be able to load source into the restored engine.
	err = e2.Load(`(defrule r => (assert (done)))`)
	if err != nil {
		t.Fatalf("Load into restored engine failed: %v", err)
	}
	result := mustRun(context.Background(), t, e2)
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
}

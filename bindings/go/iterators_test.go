package ferric

import (
	"runtime"
	"testing"
)

func init() {
	runtime.LockOSThread()
}

func TestFactIter(t *testing.T) {
	e, err := NewEngine(WithSource(`
		(deftemplate person (slot name))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	e.AssertFact("color", Symbol("red"))
	e.AssertFact("color", Symbol("blue"))
	e.AssertTemplate("person", map[string]any{"name": "Alice"})

	count := 0
	for f := range e.FactIter() {
		count++
		if f.ID == 0 {
			t.Fatal("expected non-zero ID")
		}
	}
	if count != 3 {
		t.Fatalf("expected 3 facts, got %d", count)
	}
}

func TestFactIterEarlyBreak(t *testing.T) {
	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()
	e.Reset()

	e.AssertFact("a", Symbol("1"))
	e.AssertFact("b", Symbol("2"))
	e.AssertFact("c", Symbol("3"))

	count := 0
	for range e.FactIter() {
		count++
		if count == 2 {
			break
		}
	}
	if count != 2 {
		t.Fatalf("expected 2 iterations, got %d", count)
	}
}

func TestRuleIter(t *testing.T) {
	e, err := NewEngine(WithSource(`
		(defrule r1 (declare (salience 10)) => (assert (a)))
		(defrule r2 => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	names := map[string]bool{}
	for r := range e.RuleIter() {
		names[r.Name] = true
	}
	if !names["r1"] || !names["r2"] {
		t.Fatalf("expected r1 and r2, got %v", names)
	}
}

func TestTemplateIter(t *testing.T) {
	e, err := NewEngine(WithSource(`
		(deftemplate sensor (slot id) (slot value))
		(deftemplate alarm (slot level))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	found := map[string]bool{}
	for name := range e.TemplateIter() {
		found[name] = true
	}
	if !found["sensor"] || !found["alarm"] {
		t.Fatalf("expected sensor and alarm, got %v", found)
	}
}

package ferric

import (
	"context"
	"testing"
)

func TestFactIter(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate person (slot name))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mustAssertFact(t, e, "color", Symbol("red"))
	mustAssertFact(t, e, "color", Symbol("blue"))
	mustAssertTemplate(t, e, "person", map[string]any{"name": "Alice"})

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
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	mustAssertFact(t, e, "a", Symbol("1"))
	mustAssertFact(t, e, "b", Symbol("2"))
	mustAssertFact(t, e, "c", Symbol("3"))

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
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 (declare (salience 10)) => (assert (a)))
		(defrule r2 => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	names := map[string]bool{}
	for r := range e.RuleIter() {
		names[r.Name] = true
	}
	if !names["r1"] || !names["r2"] {
		t.Fatalf("expected r1 and r2, got %v", names)
	}
}

func TestTemplateIter(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate sensor (slot id) (slot value))
		(deftemplate alarm (slot level))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	found := map[string]bool{}
	for name := range e.TemplateIter() {
		found[name] = true
	}
	if !found["sensor"] || !found["alarm"] {
		t.Fatalf("expected sensor and alarm, got %v", found)
	}
}

// ---------------------------------------------------------------------------
// Error-aware iterator variants (iter.Seq2)
// ---------------------------------------------------------------------------

func TestFactIterE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate person (slot name))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mustAssertFact(t, e, "color", Symbol("red"))
	mustAssertFact(t, e, "color", Symbol("blue"))
	mustAssertTemplate(t, e, "person", map[string]any{"name": "Alice"})

	count := 0
	for f, err := range e.FactIterE() {
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		count++
		if f.ID == 0 {
			t.Fatal("expected non-zero ID")
		}
	}
	if count != 3 {
		t.Fatalf("expected 3 facts, got %d", count)
	}
}

func TestFactIterEEarlyBreak(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	mustAssertFact(t, e, "a", Symbol("1"))
	mustAssertFact(t, e, "b", Symbol("2"))
	mustAssertFact(t, e, "c", Symbol("3"))

	count := 0
	for _, err := range e.FactIterE() {
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		count++
		if count == 2 {
			break
		}
	}
	if count != 2 {
		t.Fatalf("expected 2 iterations, got %d", count)
	}
}

func TestRuleIterE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 (declare (salience 10)) => (assert (a)))
		(defrule r2 => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	names := map[string]bool{}
	for r, err := range e.RuleIterE() {
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		names[r.Name] = true
	}
	if !names["r1"] || !names["r2"] {
		t.Fatalf("expected r1 and r2, got %v", names)
	}
}

func TestTemplateIterE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate sensor (slot id) (slot value))
		(deftemplate alarm (slot level))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	found := map[string]bool{}
	for name, err := range e.TemplateIterE() {
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		found[name] = true
	}
	if !found["sensor"] || !found["alarm"] {
		t.Fatalf("expected sensor and alarm, got %v", found)
	}
}

func TestDiagnosticIterE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule boom =>
			(/ 1 0))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mustNoError(t, e.Reset())
	_, _ = e.Run(context.Background())

	// Collect via original API for comparison.
	origDiags := e.Diagnostics()

	// Collect via error-aware iterator.
	iterDiags := make([]string, 0, len(origDiags))
	for msg, err := range e.DiagnosticIterE() {
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		iterDiags = append(iterDiags, msg)
	}

	if len(iterDiags) != len(origDiags) {
		t.Fatalf("DiagnosticIterE returned %d items, Diagnostics returned %d", len(iterDiags), len(origDiags))
	}
}

// ---------------------------------------------------------------------------
// Error-aware introspection variants
// ---------------------------------------------------------------------------

func TestRulesE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 (declare (salience 10)) => (assert (a)))
		(defrule r2 => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	rules, err := e.RulesE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	orig := e.Rules()
	if len(rules) != len(orig) {
		t.Fatalf("RulesE returned %d, Rules returned %d", len(rules), len(orig))
	}
	for i, r := range rules {
		if r.Name != orig[i].Name || r.Salience != orig[i].Salience {
			t.Fatalf("mismatch at %d: %+v vs %+v", i, r, orig[i])
		}
	}
}

func TestTemplatesE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate sensor (slot id) (slot value))
		(deftemplate alarm (slot level))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	names, err := e.TemplatesE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	orig := e.Templates()
	if len(names) != len(orig) {
		t.Fatalf("TemplatesE returned %d, Templates returned %d", len(names), len(orig))
	}
}

func TestCurrentModuleE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mod, err := e.CurrentModuleE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	orig := e.CurrentModule()
	if mod != orig {
		t.Fatalf("CurrentModuleE=%q, CurrentModule=%q", mod, orig)
	}
}

func TestFocusE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	name, ok, err := e.FocusE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	origName, origOK := e.Focus()
	if name != origName || ok != origOK {
		t.Fatalf("FocusE=(%q,%v), Focus=(%q,%v)", name, ok, origName, origOK)
	}
}

func TestFocusStackE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	stack, err := e.FocusStackE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	orig := e.FocusStack()
	if len(stack) != len(orig) {
		t.Fatalf("FocusStackE returned %d, FocusStack returned %d", len(stack), len(orig))
	}
	for i, s := range stack {
		if s != orig[i] {
			t.Fatalf("mismatch at %d: %q vs %q", i, s, orig[i])
		}
	}
}

func TestAgendaSizeE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 => (assert (a)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	size, err := e.AgendaSizeE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	orig := e.AgendaSize()
	if size != orig {
		t.Fatalf("AgendaSizeE=%d, AgendaSize=%d", size, orig)
	}
}

func TestIsHaltedE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	halted, err := e.IsHaltedE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	orig := e.IsHalted()
	if halted != orig {
		t.Fatalf("IsHaltedE=%v, IsHalted=%v", halted, orig)
	}
}

func TestDiagnosticsE(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule boom =>
			(/ 1 0))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mustNoError(t, e.Reset())
	_, _ = e.Run(context.Background())

	diags, err := e.DiagnosticsE()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	orig := e.Diagnostics()
	if len(diags) != len(orig) {
		t.Fatalf("DiagnosticsE returned %d, Diagnostics returned %d", len(diags), len(orig))
	}
	for i, d := range diags {
		if d != orig[i] {
			t.Fatalf("mismatch at %d: %q vs %q", i, d, orig[i])
		}
	}
}

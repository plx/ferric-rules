package ferric

import (
	"context"
	"errors"
	"runtime"
	"testing"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

// ---------------------------------------------------------------------------
// Engine lifecycle
// ---------------------------------------------------------------------------

func TestNewEngineDefault(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
}

func TestNewEngineWithSource(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`(defrule r => (printout t "ok" crlf))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
}

func TestNewEngineWithConfig(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(
		WithStrategy(StrategyBreadth),
		WithEncoding(EncodingUTF8),
		WithMaxCallDepth(512),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
}

func TestNewEngineWithSourceAndConfig(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(
		WithSource(`(defrule r => (assert (done)))`),
		WithStrategy(StrategyDepth),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
}

func TestNewEngineInvalidSource(t *testing.T) {
	lockThread(t)

	_, err := NewEngine(WithSource(`(defrule bad`))
	if err == nil {
		t.Fatal("expected error for invalid source")
	}
	if !errors.Is(err, ErrParse) {
		t.Fatalf("expected ParseError, got: %v", err)
	}
}

func TestEngineDoubleClose(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
	// Second close should be a no-op.
	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

func TestLoad(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	err = e.Load(`
		(deftemplate person (slot name) (slot age))
		(defrule greet (person (name ?n)) => (printout t "Hello " ?n crlf))
	`)
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	rules := e.Rules()
	if len(rules) != 1 {
		t.Fatalf("expected 1 rule, got %d", len(rules))
	}
	if rules[0].Name != "greet" {
		t.Fatalf("expected rule 'greet', got %q", rules[0].Name)
	}
}

func TestLoadInvalid(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	err = e.Load(`(defrule bad`)
	if err == nil {
		t.Fatal("expected error")
	}
}

// ---------------------------------------------------------------------------
// Ordered facts
// ---------------------------------------------------------------------------

func TestAssertStringAndFacts(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	id, err := e.AssertString("(assert (color red))")
	if err != nil {
		t.Fatal(err)
	}
	if id == 0 {
		t.Fatal("expected non-zero fact ID")
	}

	facts, err := e.Facts()
	if err != nil {
		t.Fatal(err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if facts[0].Relation != "color" {
		t.Fatalf("expected relation 'color', got %q", facts[0].Relation)
	}
	if facts[0].Type != FactOrdered {
		t.Fatal("expected ordered fact")
	}
}

func TestAssertFact(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	id, err := e.AssertFact("temperature", Symbol("high"))
	if err != nil {
		t.Fatal(err)
	}
	if id == 0 {
		t.Fatal("expected non-zero fact ID")
	}

	count, err := e.FactCount()
	if err != nil {
		t.Fatal(err)
	}
	if count != 1 {
		t.Fatalf("expected 1 fact, got %d", count)
	}
}

func TestRetract(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	id, err := e.AssertFact("color", Symbol("red"))
	if err != nil {
		t.Fatal(err)
	}

	err = e.Retract(id)
	if err != nil {
		t.Fatal(err)
	}

	count, err := e.FactCount()
	if err != nil {
		t.Fatal(err)
	}
	if count != 0 {
		t.Fatalf("expected 0 facts after retract, got %d", count)
	}
}

func TestFindFacts(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	mustAssertFact(t, e, "color", Symbol("red"))
	mustAssertFact(t, e, "color", Symbol("blue"))
	mustAssertFact(t, e, "shape", Symbol("circle"))

	colors, err := e.FindFacts("color")
	if err != nil {
		t.Fatal(err)
	}
	if len(colors) != 2 {
		t.Fatalf("expected 2 color facts, got %d", len(colors))
	}

	shapes, err := e.FindFacts("shape")
	if err != nil {
		t.Fatal(err)
	}
	if len(shapes) != 1 {
		t.Fatalf("expected 1 shape fact, got %d", len(shapes))
	}
}

func TestGetFact(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	id, err := e.AssertFact("color", Symbol("red"))
	if err != nil {
		t.Fatal(err)
	}

	f, err := e.GetFact(id)
	if err != nil {
		t.Fatal(err)
	}
	if f.Relation != "color" {
		t.Fatalf("expected 'color', got %q", f.Relation)
	}
	if len(f.Fields) != 1 {
		t.Fatalf("expected 1 field, got %d", len(f.Fields))
	}
	sym, ok := f.Fields[0].(Symbol)
	if !ok {
		t.Fatalf("expected Symbol, got %T", f.Fields[0])
	}
	if sym != "red" {
		t.Fatalf("expected 'red', got %q", sym)
	}
}

// ---------------------------------------------------------------------------
// Template facts
// ---------------------------------------------------------------------------

func TestAssertTemplate(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate person
			(slot name (type STRING))
			(slot age (type INTEGER) (default 0)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	id, err := e.AssertTemplate("person", map[string]any{
		"name": "Alice",
		"age":  int64(30),
	})
	if err != nil {
		t.Fatal(err)
	}

	f, err := e.GetFact(id)
	if err != nil {
		t.Fatal(err)
	}
	if f.Type != FactTemplate {
		t.Fatal("expected template fact")
	}
	if f.TemplateName != "person" {
		t.Fatalf("expected 'person', got %q", f.TemplateName)
	}
	if f.Slots["name"] != "Alice" {
		t.Fatalf("expected name 'Alice', got %v", f.Slots["name"])
	}
	if f.Slots["age"] != int64(30) {
		t.Fatalf("expected age 30, got %v", f.Slots["age"])
	}
}

func TestAssertTemplateDefaults(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate person
			(slot name (type STRING))
			(slot age (type INTEGER) (default 0))
			(slot active (type SYMBOL) (default TRUE)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	id, err := e.AssertTemplate("person", map[string]any{
		"name": "Bob",
	})
	if err != nil {
		t.Fatal(err)
	}

	f, err := e.GetFact(id)
	if err != nil {
		t.Fatal(err)
	}
	if f.Slots["age"] != int64(0) {
		t.Fatalf("expected default age 0, got %v", f.Slots["age"])
	}
	if f.Slots["active"] != Symbol("TRUE") {
		t.Fatalf("expected default active TRUE, got %v (%T)", f.Slots["active"], f.Slots["active"])
	}
}

func TestAssertTemplateNotFound(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	_, err = e.AssertTemplate("nonexistent", map[string]any{})
	if err == nil {
		t.Fatal("expected error")
	}
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("expected NotFound, got: %v", err)
	}
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

func TestRunAndOutput(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule hello => (printout t "Hello!" crlf))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	result, err := e.Run(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
	if result.HaltReason != HaltAgendaEmpty {
		t.Fatalf("expected AgendaEmpty, got %d", result.HaltReason)
	}

	output, ok := e.GetOutput("t")
	if !ok {
		t.Fatal("expected output")
	}
	if output != "Hello!\n" {
		t.Fatalf("unexpected output: %q", output)
	}
}

func TestRunWithLimit(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 => (assert (a)))
		(defrule r2 (a) => (assert (b)))
		(defrule r3 (b) => (assert (c)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	result, err := e.RunWithLimit(context.Background(), 2)
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 2 {
		t.Fatalf("expected 2, got %d", result.RulesFired)
	}
	if result.HaltReason != HaltLimitReached {
		t.Fatalf("expected LimitReached, got %d", result.HaltReason)
	}
}

func TestRunWithLimitSmall(t *testing.T) {
	lockThread(t)

	// Small limit uses the direct FFI call path.
	e, err := NewEngine(WithSource(`
		(defrule r1 => (assert (a)))
		(defrule r2 (a) => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	result, err := e.RunWithLimit(context.Background(), 1)
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
}

func TestRunContextCancel(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 => (assert (a)))
		(defrule r2 (a) => (assert (b)))
		(defrule r3 (b) => (assert (c)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // cancel immediately

	// With a cancelled context, RunWithLimit should return early.
	_, err = e.RunWithLimit(ctx, 1000)
	if err == nil {
		t.Fatal("expected context error")
	}
}

func TestStep(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 => (assert (done)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	fired, err := e.Step()
	if err != nil {
		t.Fatal(err)
	}
	if fired == nil {
		t.Fatal("expected rule to fire")
	}

	// Second step: agenda empty
	fired, err = e.Step()
	if err != nil {
		t.Fatal(err)
	}
	if fired != nil {
		t.Fatal("expected nil (empty agenda)")
	}
}

func TestHalt(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`(defrule r => (assert (x)))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	e.Halt()
	if !e.IsHalted() {
		t.Fatal("expected halted")
	}
}

func TestReset(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r => (assert (done)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Run to completion.
	mustRun(context.Background(), t, e)

	count, _ := e.FactCount()
	if count == 0 {
		t.Fatal("expected facts after run")
	}

	// Reset clears facts.
	err = e.Reset()
	if err != nil {
		t.Fatal(err)
	}

	count, _ = e.FactCount()
	if count != 0 {
		t.Fatalf("expected 0 facts after reset, got %d", count)
	}

	// Can run again after reset.
	result, err := e.Run(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 {
		t.Fatalf("expected 1, got %d", result.RulesFired)
	}
}

func TestClear(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`(defrule r => (assert (done)))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	rules := e.Rules()
	if len(rules) != 1 {
		t.Fatalf("expected 1 rule, got %d", len(rules))
	}

	e.Clear()

	rules = e.Rules()
	if len(rules) != 0 {
		t.Fatalf("expected 0 rules after clear, got %d", len(rules))
	}
}

// ---------------------------------------------------------------------------
// Introspection
// ---------------------------------------------------------------------------

func TestRules(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 (declare (salience 10)) => (assert (a)))
		(defrule r2 => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	rules := e.Rules()
	if len(rules) != 2 {
		t.Fatalf("expected 2 rules, got %d", len(rules))
	}

	// Find r1 and check salience.
	for _, r := range rules {
		if r.Name == "r1" {
			if r.Salience != 10 {
				t.Fatalf("expected salience 10, got %d", r.Salience)
			}
			return
		}
	}
	t.Fatal("rule 'r1' not found")
}

func TestTemplates(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate sensor (slot id) (slot value))
		(deftemplate alarm (slot level))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	tmpls := e.Templates()
	if len(tmpls) < 2 {
		t.Fatalf("expected at least 2 templates, got %d", len(tmpls))
	}

	found := map[string]bool{}
	for _, name := range tmpls {
		found[name] = true
	}
	if !found["sensor"] || !found["alarm"] {
		t.Fatalf("expected sensor and alarm templates, got: %v", tmpls)
	}
}

func TestGetGlobal(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`(defglobal ?*threshold* = 42)`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	val, err := e.GetGlobal("threshold")
	if err != nil {
		t.Fatal(err)
	}
	if val != int64(42) {
		t.Fatalf("expected 42, got %v (%T)", val, val)
	}
}

func TestCurrentModule(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mod := e.CurrentModule()
	if mod != "MAIN" {
		t.Fatalf("expected MAIN, got %q", mod)
	}
}

func TestAgendaSize(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r1 => (assert (a)))
		(defrule r2 => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	size := e.AgendaSize()
	if size != 2 {
		t.Fatalf("expected 2, got %d", size)
	}
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

func TestOutput(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r => (printout t "line1" crlf "line2" crlf))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mustRun(context.Background(), t, e)

	out, ok := e.GetOutput("t")
	if !ok {
		t.Fatal("expected output")
	}
	if out != "line1\nline2\n" {
		t.Fatalf("unexpected: %q", out)
	}

	e.ClearOutput("t")
	_, ok = e.GetOutput("t")
	if ok {
		t.Fatal("expected no output after clear")
	}
}

func TestPushInput(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Just test it doesn't panic.
	e.PushInput("hello")
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

func TestDiagnostics(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Initially no diagnostics.
	diags := e.Diagnostics()
	if len(diags) != 0 {
		t.Fatalf("expected 0 diagnostics, got %d", len(diags))
	}

	e.ClearDiagnostics() // should not panic even with nothing to clear
}

// ---------------------------------------------------------------------------
// Error type hierarchy
// ---------------------------------------------------------------------------

func TestErrorIs(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	// Retract a nonexistent fact.
	err = e.Retract(999999)
	if err == nil {
		t.Fatal("expected error")
	}
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("expected ErrNotFound, got: %v", err)
	}
}

func TestErrorAs(t *testing.T) {
	lockThread(t)

	_, err := NewEngine(WithSource(`(defrule bad`))
	if err == nil {
		t.Fatal("expected error")
	}

	var pe *ParseError
	if !errors.As(err, &pe) {
		t.Fatalf("expected ParseError, got %T: %v", err, err)
	}
}

func TestErrorFromFFIThreadViolation(t *testing.T) {
	lockThread(t)

	err := errorFromFFI(ffi.ErrThreadViolation, nil)
	if err == nil {
		t.Fatal("expected error")
	}
	if !errors.Is(err, ErrThreadViolation) {
		t.Fatalf("expected ErrThreadViolation, got: %v", err)
	}
	var tve *ThreadViolationError
	if !errors.As(err, &tve) {
		t.Fatalf("expected ThreadViolationError, got %T: %v", err, err)
	}
}

func TestErrorFromFFIInvalidArgument(t *testing.T) {
	lockThread(t)

	err := errorFromFFI(ffi.ErrInvalidArgument, nil)
	if err == nil {
		t.Fatal("expected error")
	}
	if !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("expected ErrInvalidArgument, got: %v", err)
	}
	var iae *InvalidArgumentError
	if !errors.As(err, &iae) {
		t.Fatalf("expected InvalidArgumentError, got %T: %v", err, err)
	}
}

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

func TestValueConversionRoundtrip(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defglobal ?*int-val* = 42)
		(defglobal ?*float-val* = 3.14)
		(defglobal ?*sym-val* = foo)
		(defglobal ?*str-val* = "hello")
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Integer
	v, err := e.GetGlobal("int-val")
	if err != nil {
		t.Fatal(err)
	}
	if v != int64(42) {
		t.Fatalf("expected int64(42), got %v (%T)", v, v)
	}

	// Float
	v, err = e.GetGlobal("float-val")
	if err != nil {
		t.Fatal(err)
	}
	f, ok := v.(float64)
	if !ok {
		t.Fatalf("expected float64, got %T", v)
	}
	if f < 3.13 || f > 3.15 {
		t.Fatalf("expected ~3.14, got %f", f)
	}

	// Symbol
	v, err = e.GetGlobal("sym-val")
	if err != nil {
		t.Fatal(err)
	}
	sym, ok := v.(Symbol)
	if !ok {
		t.Fatalf("expected Symbol, got %T", v)
	}
	if sym != "foo" {
		t.Fatalf("expected 'foo', got %q", sym)
	}

	// String
	v, err = e.GetGlobal("str-val")
	if err != nil {
		t.Fatal(err)
	}
	s, ok := v.(string)
	if !ok {
		t.Fatalf("expected string, got %T", v)
	}
	if s != "hello" {
		t.Fatalf("expected 'hello', got %q", s)
	}
}

// ---------------------------------------------------------------------------
// Value cleanup stress tests (#47 / GOB-002)
// ---------------------------------------------------------------------------

func TestAssertFactFreesValues(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	const iterations = 1000
	for i := range iterations {
		id, err := e.AssertFact("data",
			Symbol("sym-value"),
			"string-value",
			int64(i),
			3.14,
			true,
		)
		if err != nil {
			t.Fatalf("iteration %d: %v", i, err)
		}
		if id == 0 {
			t.Fatalf("iteration %d: expected non-zero fact ID", i)
		}
	}

	count, err := e.FactCount()
	if err != nil {
		t.Fatal(err)
	}
	if count != iterations {
		t.Fatalf("expected %d facts, got %d", iterations, count)
	}
}

func TestAssertTemplateFreesValues(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate sensor
			(slot id (type INTEGER))
			(slot name (type STRING))
			(slot status (type SYMBOL))
			(slot value (type FLOAT)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	const iterations = 1000
	for i := range iterations {
		id, err := e.AssertTemplate("sensor", map[string]any{
			"id":     int64(i),
			"name":   "sensor-name",
			"status": Symbol("active"),
			"value":  float64(i) * 0.1,
		})
		if err != nil {
			t.Fatalf("iteration %d: %v", i, err)
		}
		if id == 0 {
			t.Fatalf("iteration %d: expected non-zero fact ID", i)
		}
	}

	count, err := e.FactCount()
	if err != nil {
		t.Fatal(err)
	}
	if count != iterations {
		t.Fatalf("expected %d facts, got %d", iterations, count)
	}
}

func TestAssertFactFreesValuesOnError(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	// Pass a value that goToFFIValue cannot convert (struct{}).
	// Earlier symbol/string values should still be freed.
	_, err = e.AssertFact("data", Symbol("leaky"), "also-leaky", struct{}{})
	if err == nil {
		t.Fatal("expected error for unsupported type")
	}
}

func TestAssertTemplateFreesValuesOnError(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate item (slot name (type STRING)) (slot tags))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Pass a value that goToFFIValue cannot convert.
	_, err = e.AssertTemplate("item", map[string]any{
		"name": "leaky-string",
		"tags": struct{}{},
	})
	if err == nil {
		t.Fatal("expected error for unsupported type")
	}
}

func TestAssertOrderedFactWithMultifield(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	// Assert an ordered fact with a multifield field.
	id, err := e.AssertFact("data", Symbol("tag"), []any{int64(1), int64(2), int64(3)})
	if err != nil {
		t.Fatalf("expected multifield assertion to succeed: %v", err)
	}
	if id == 0 {
		t.Fatal("expected non-zero fact ID")
	}

	// Read back and verify.
	f, err := e.GetFact(id)
	if err != nil {
		t.Fatal(err)
	}
	if f.Type != FactOrdered {
		t.Fatalf("expected ordered fact, got %v", f.Type)
	}
	if f.Relation != "data" {
		t.Fatalf("expected relation 'data', got %q", f.Relation)
	}
	if len(f.Fields) != 2 {
		t.Fatalf("expected 2 fields, got %d", len(f.Fields))
	}
	// Field 0: symbol "tag"
	if sym, ok := f.Fields[0].(Symbol); !ok || sym != "tag" {
		t.Fatalf("expected Symbol(tag), got %v (%T)", f.Fields[0], f.Fields[0])
	}
	// Field 1: multifield [1, 2, 3]
	mf, ok := f.Fields[1].([]any)
	if !ok {
		t.Fatalf("expected []any for multifield, got %T", f.Fields[1])
	}
	if len(mf) != 3 {
		t.Fatalf("expected 3 multifield elements, got %d", len(mf))
	}
	for i, want := range []int64{1, 2, 3} {
		got, ok := mf[i].(int64)
		if !ok || got != want {
			t.Fatalf("multifield[%d]: expected %d, got %v (%T)", i, want, mf[i], mf[i])
		}
	}
}

func TestAssertTemplateFactWithMultifield(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate item
			(slot name (type STRING))
			(multislot tags))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	id, err := e.AssertTemplate("item", map[string]any{
		"name": "widget",
		"tags": []any{"red", "large"},
	})
	if err != nil {
		t.Fatalf("expected multifield template assertion to succeed: %v", err)
	}
	if id == 0 {
		t.Fatal("expected non-zero fact ID")
	}

	// Read back and verify.
	f, err := e.GetFact(id)
	if err != nil {
		t.Fatal(err)
	}
	if f.Type != FactTemplate {
		t.Fatalf("expected template fact, got %v", f.Type)
	}
	if f.TemplateName != "item" {
		t.Fatalf("expected template 'item', got %q", f.TemplateName)
	}

	name, ok := f.Slots["name"]
	if !ok {
		t.Fatal("expected 'name' slot")
	}
	if name != "widget" {
		t.Fatalf("expected name='widget', got %v", name)
	}

	tags, ok := f.Slots["tags"]
	if !ok {
		t.Fatal("expected 'tags' slot")
	}
	mf, ok := tags.([]any)
	if !ok {
		t.Fatalf("expected []any for tags, got %T", tags)
	}
	if len(mf) != 2 {
		t.Fatalf("expected 2 tags, got %d", len(mf))
	}
	if mf[0] != "red" {
		t.Fatalf("expected tag[0]='red', got %v", mf[0])
	}
	if mf[1] != "large" {
		t.Fatalf("expected tag[1]='large', got %v", mf[1])
	}
}

func TestAssertMultifieldEmptySlice(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate item
			(slot name (type STRING))
			(multislot tags))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Assert with empty multifield.
	id, err := e.AssertTemplate("item", map[string]any{
		"name": "empty",
		"tags": []any{},
	})
	if err != nil {
		t.Fatalf("expected empty multifield assertion to succeed: %v", err)
	}

	f, err := e.GetFact(id)
	if err != nil {
		t.Fatal(err)
	}
	tags, ok := f.Slots["tags"]
	if !ok {
		t.Fatal("expected 'tags' slot")
	}
	mf, ok := tags.([]any)
	if !ok {
		t.Fatalf("expected []any for tags, got %T", tags)
	}
	if len(mf) != 0 {
		t.Fatalf("expected 0 tags, got %d", len(mf))
	}
}

func TestAssertMultifieldConversionErrorFreesPartial(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	// A multifield with an unsupported element should fail and free
	// the already-converted elements without leaking.
	_, err = e.AssertFact("data", []any{int64(1), "ok", struct{}{}})
	if err == nil {
		t.Fatal("expected error for unsupported type inside multifield")
	}
}

// ---------------------------------------------------------------------------
// Rule-fires-on-template-fact integration test
// ---------------------------------------------------------------------------

func TestRuleFiresOnTemplateFact(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate sensor
			(slot id (type INTEGER))
			(slot value (type FLOAT)))
		(defrule alert
			(sensor (id ?id) (value ?v&:(> ?v 100.0)))
			=>
			(printout t "ALERT: sensor " ?id " at " ?v crlf))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Assert a fact that should trigger the rule.
	_, err = e.AssertTemplate("sensor", map[string]any{
		"id":    int64(1),
		"value": 150.5,
	})
	if err != nil {
		t.Fatal(err)
	}

	// Assert a fact that should NOT trigger the rule.
	_, err = e.AssertTemplate("sensor", map[string]any{
		"id":    int64(2),
		"value": 50.0,
	})
	if err != nil {
		t.Fatal(err)
	}

	result, err := e.Run(context.Background())
	if err != nil {
		t.Fatal(err)
	}

	output, ok := e.GetOutput("t")
	if !ok {
		t.Fatal("expected output")
	}

	// Only the sensor with value > 100 should trigger the alert.
	if result.RulesFired != 1 {
		t.Fatalf("expected 1 rule fired, got %d; output: %q", result.RulesFired, output)
	}
	if output != "ALERT: sensor 1 at 150.5\n" {
		t.Fatalf("unexpected output: %q", output)
	}
}

// ---------------------------------------------------------------------------
// Strict fact-building coverage (GOB-006)
// ---------------------------------------------------------------------------

func TestBuildFactOrderedAlwaysHasRelation(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	mustAssertFact(t, e, "color", Symbol("red"))
	mustAssertFact(t, e, "shape", Symbol("circle"))

	facts, err := e.Facts()
	if err != nil {
		t.Fatal(err)
	}
	for _, f := range facts {
		if f.Type != FactOrdered {
			t.Fatalf("expected ordered fact, got type %d", f.Type)
		}
		if f.Relation == "" {
			t.Fatalf("fact %d has empty Relation", f.ID)
		}
	}
}

func TestBuildFactTemplateAlwaysHasSlots(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate sensor
			(slot id (type INTEGER))
			(slot name (type STRING))
			(slot value (type FLOAT)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	mustAssertTemplate(t, e, "sensor", map[string]any{
		"id":    int64(1),
		"name":  "temp-1",
		"value": 98.6,
	})
	mustAssertTemplate(t, e, "sensor", map[string]any{
		"id":    int64(2),
		"name":  "temp-2",
		"value": 72.0,
	})

	facts, err := e.Facts()
	if err != nil {
		t.Fatal(err)
	}
	for _, f := range facts {
		if f.Type != FactTemplate {
			t.Fatalf("expected template fact, got type %d", f.Type)
		}
		if f.TemplateName != "sensor" {
			t.Fatalf("expected template name 'sensor', got %q", f.TemplateName)
		}
		if f.Slots == nil {
			t.Fatalf("fact %d has nil Slots", f.ID)
		}
		if len(f.Slots) != 3 {
			t.Fatalf("fact %d: expected 3 slots, got %d", f.ID, len(f.Slots))
		}
		for _, key := range []string{"id", "name", "value"} {
			if _, ok := f.Slots[key]; !ok {
				t.Fatalf("fact %d: missing slot %q", f.ID, key)
			}
		}
	}
}

func TestBuildFactFindFactsCompleteness(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	mustAssertFact(t, e, "color", Symbol("red"))
	mustAssertFact(t, e, "color", Symbol("blue"))

	facts, err := e.FindFacts("color")
	if err != nil {
		t.Fatal(err)
	}
	if len(facts) != 2 {
		t.Fatalf("expected 2 facts, got %d", len(facts))
	}
	for _, f := range facts {
		if f.Relation == "" {
			t.Fatalf("FindFacts returned fact %d with empty Relation", f.ID)
		}
		if f.Type != FactOrdered {
			t.Fatalf("expected ordered fact, got type %d", f.Type)
		}
	}
}

func TestBuildFactGetFactCompleteness(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(deftemplate item (slot name (type STRING)) (slot count (type INTEGER)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Verify ordered fact via GetFact.
	orderedID := mustAssertFact(t, e, "tag", Symbol("important"))
	f, err := e.GetFact(orderedID)
	if err != nil {
		t.Fatal(err)
	}
	if f.Relation == "" {
		t.Fatal("GetFact returned ordered fact with empty Relation")
	}

	// Verify template fact via GetFact.
	templateID := mustAssertTemplate(t, e, "item", map[string]any{
		"name":  "widget",
		"count": int64(5),
	})
	f, err = e.GetFact(templateID)
	if err != nil {
		t.Fatal(err)
	}
	if f.TemplateName == "" {
		t.Fatal("GetFact returned template fact with empty TemplateName")
	}
	if f.Slots == nil {
		t.Fatal("GetFact returned template fact with nil Slots")
	}
	if len(f.Slots) != 2 {
		t.Fatalf("expected 2 slots, got %d", len(f.Slots))
	}
	if f.Slots["name"] != "widget" {
		t.Fatalf("expected slot name 'widget', got %v", f.Slots["name"])
	}
	if f.Slots["count"] != int64(5) {
		t.Fatalf("expected slot count 5, got %v", f.Slots["count"])
	}
}

// ---------------------------------------------------------------------------
// Wrong-thread coverage (GOB-003)
// ---------------------------------------------------------------------------

func TestEngineWrongThread(t *testing.T) {
	lockThread(t)

	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustNoError(t, e.Reset())

	mustAssertFact(t, e, "color", Symbol("red"))

	// Call FactCount from a different OS thread.
	errc := make(chan error, 1)
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		_, err := e.FactCount()
		errc <- err
	}()

	err = <-errc
	if err == nil {
		t.Fatal("expected thread violation error from wrong thread")
	}
	if !errors.Is(err, ErrThreadViolation) {
		t.Fatalf("expected ErrThreadViolation, got: %v", err)
	}
	var tve *ThreadViolationError
	if !errors.As(err, &tve) {
		t.Fatalf("expected ThreadViolationError, got %T: %v", err, err)
	}
}

func TestEngineWrongThreadMultipleOps(t *testing.T) {
	lockThread(t)

	e, err := NewEngine(WithSource(`
		(defrule r => (assert (done)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	// Verify several operations all return ErrThreadViolation from a wrong thread.
	type opResult struct {
		name string
		err  error
	}
	results := make(chan opResult, 10)

	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()

		_, err := e.Run(context.Background())
		results <- opResult{"Run", err}

		err = e.Reset()
		results <- opResult{"Reset", err}

		err = e.Load(`(defrule r2 => (assert (x)))`)
		results <- opResult{"Load", err}

		_, err = e.AssertFact("x", Symbol("y"))
		results <- opResult{"AssertFact", err}

		_, err = e.AssertString("(assert (z))")
		results <- opResult{"AssertString", err}

		close(results)
	}()

	for r := range results {
		if r.err == nil {
			t.Fatalf("%s: expected thread violation error", r.name)
		}
		if !errors.Is(r.err, ErrThreadViolation) {
			t.Fatalf("%s: expected ErrThreadViolation, got: %v", r.name, r.err)
		}
	}
}

package ffi

import (
	"runtime"
	"testing"
)

func init() {
	// Lock the main test goroutine to an OS thread since
	// all engine operations must happen on the creating thread.
	runtime.LockOSThread()
}

func TestEngineLifecycle(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}

	rc := EngineFree(h)
	if rc != ErrOK {
		t.Fatalf("EngineFree returned %d", rc)
	}
}

func TestEngineNewWithSource(t *testing.T) {
	h := EngineNewWithSource(`(defrule hello => (printout t "Hello from Go!" crlf))`)
	if h == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}

	fired, reason, rc := EngineRunEx(h, -1)
	if rc != ErrOK {
		t.Fatalf("EngineRunEx returned %d", rc)
	}
	if fired != 1 {
		t.Fatalf("expected 1 rule fired, got %d", fired)
	}
	if reason != HaltReasonAgendaEmpty {
		t.Fatalf("expected AgendaEmpty, got %d", reason)
	}

	output, ok := EngineGetOutput(h, "t")
	if !ok {
		t.Fatal("expected stdout output")
	}
	if output != "Hello from Go!\n" {
		t.Fatalf("unexpected output: %q", output)
	}

	rc = EngineFree(h)
	if rc != ErrOK {
		t.Fatalf("EngineFree returned %d", rc)
	}
}

func TestLoadAndRun(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	rc := EngineLoadString(h, `
		(defrule greet
			(person (name ?n))
			=>
			(printout t "Hello, " ?n "!" crlf))

		(deftemplate person
			(slot name (type STRING)))
	`)
	if rc != ErrOK {
		t.Fatalf("EngineLoadString returned %d, error: %s", rc, EngineLastError(h))
	}

	rc = EngineReset(h)
	if rc != ErrOK {
		t.Fatalf("EngineReset returned %d", rc)
	}

	// Assert a template fact
	factID, rc := EngineAssertTemplate(h, "person",
		[]string{"name"},
		[]Value{ValueString("Alice")},
	)
	if rc != ErrOK {
		t.Fatalf("EngineAssertTemplate returned %d, error: %s", rc, EngineLastError(h))
	}
	if factID == 0 {
		t.Fatal("expected non-zero fact ID")
	}

	// Run
	fired, rc := EngineRun(h, -1)
	if rc != ErrOK {
		t.Fatalf("EngineRun returned %d", rc)
	}
	if fired != 1 {
		t.Fatalf("expected 1 rule fired, got %d", fired)
	}

	output, ok := EngineGetOutput(h, "t")
	if !ok {
		t.Fatal("expected stdout output")
	}
	if output != "Hello, Alice!\n" {
		t.Fatalf("unexpected output: %q", output)
	}
}

//nolint:funlen // integration test intentionally exercises end-to-end fact APIs in one flow.
func TestFactOperations(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	rc := EngineReset(h)
	if rc != ErrOK {
		t.Fatalf("EngineReset returned %d", rc)
	}

	// Assert ordered facts
	id1, rc := EngineAssertString(h, "(assert (color red))")
	if rc != ErrOK {
		t.Fatalf("EngineAssertString returned %d", rc)
	}
	if id1 == 0 {
		t.Fatal("expected non-zero fact ID")
	}

	id2, rc := EngineAssertString(h, "(assert (color blue))")
	if rc != ErrOK {
		t.Fatalf("EngineAssertString returned %d", rc)
	}

	// Check fact count (2 user facts)
	count, rc := EngineFactCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineFactCount returned %d", rc)
	}
	if count != 2 {
		t.Fatalf("expected 2 facts, got %d", count)
	}

	// Get all fact IDs
	ids, rc := EngineFactIDs(h)
	if rc != ErrOK {
		t.Fatalf("EngineFactIDs returned %d", rc)
	}
	if len(ids) != 2 {
		t.Fatalf("expected 2 fact IDs, got %d", len(ids))
	}

	// Find facts by relation
	colorIDs, rc := EngineFindFactIDs(h, "color")
	if rc != ErrOK {
		t.Fatalf("EngineFindFactIDs returned %d", rc)
	}
	if len(colorIDs) != 2 {
		t.Fatalf("expected 2 color facts, got %d", len(colorIDs))
	}

	// Check fact type
	ft, rc := EngineGetFactType(h, id1)
	if rc != ErrOK {
		t.Fatalf("EngineGetFactType returned %d", rc)
	}
	if ft != FactTypeOrdered {
		t.Fatalf("expected ordered fact type, got %d", ft)
	}

	// Get relation name
	rel, rc := EngineGetFactRelation(h, id1)
	if rc != ErrOK {
		t.Fatalf("EngineGetFactRelation returned %d", rc)
	}
	if rel != "color" {
		t.Fatalf("expected relation 'color', got %q", rel)
	}

	// Get field count and field value
	fieldCount, rc := EngineGetFactFieldCount(h, id1)
	if rc != ErrOK {
		t.Fatalf("EngineGetFactFieldCount returned %d", rc)
	}
	if fieldCount != 1 {
		t.Fatalf("expected 1 field, got %d", fieldCount)
	}

	val, rc := EngineGetFactField(h, id1, 0)
	if rc != ErrOK {
		t.Fatalf("EngineGetFactField returned %d", rc)
	}
	if val.value_type != ValueTypeSymbol {
		t.Fatalf("expected symbol value type, got %d", val.value_type)
	}
	ValueFree(&val)

	// Retract
	rc = EngineRetract(h, id2)
	if rc != ErrOK {
		t.Fatalf("EngineRetract returned %d", rc)
	}

	count, rc = EngineFactCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineFactCount returned %d", rc)
	}
	if count != 1 {
		t.Fatalf("expected 1 fact after retract, got %d", count)
	}
}

//nolint:funlen // integration test intentionally covers full template-slot lifecycle in one flow.
func TestTemplateFactSlotByName(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	rc := EngineLoadString(h, `
		(deftemplate person
			(slot name (type STRING))
			(slot age (type INTEGER) (default 0)))
	`)
	if rc != ErrOK {
		t.Fatalf("EngineLoadString returned %d", rc)
	}

	rc = EngineReset(h)
	if rc != ErrOK {
		t.Fatalf("EngineReset returned %d", rc)
	}

	factID, rc := EngineAssertTemplate(h, "person",
		[]string{"name", "age"},
		[]Value{ValueString("Bob"), ValueInteger(42)},
	)
	if rc != ErrOK {
		t.Fatalf("EngineAssertTemplate returned %d", rc)
	}

	// Check it's a template fact
	ft, rc := EngineGetFactType(h, factID)
	if rc != ErrOK {
		t.Fatalf("EngineGetFactType returned %d", rc)
	}
	if ft != FactTypeTemplate {
		t.Fatalf("expected template fact, got %d", ft)
	}

	// Get template name
	tmplName, rc := EngineGetFactTemplateName(h, factID)
	if rc != ErrOK {
		t.Fatalf("EngineGetFactTemplateName returned %d", rc)
	}
	if tmplName != "person" {
		t.Fatalf("expected template name 'person', got %q", tmplName)
	}

	// Get slot by name
	nameVal, rc := EngineGetFactSlotByName(h, factID, "name")
	if rc != ErrOK {
		t.Fatalf("EngineGetFactSlotByName(name) returned %d", rc)
	}
	if nameVal.value_type != ValueTypeString {
		t.Fatalf("expected string value type for name, got %d", nameVal.value_type)
	}
	ValueFree(&nameVal)

	ageVal, rc := EngineGetFactSlotByName(h, factID, "age")
	if rc != ErrOK {
		t.Fatalf("EngineGetFactSlotByName(age) returned %d", rc)
	}
	if ageVal.value_type != ValueTypeInteger {
		t.Fatalf("expected integer value type for age, got %d", ageVal.value_type)
	}
	if ageVal.integer != 42 {
		t.Fatalf("expected age 42, got %d", ageVal.integer)
	}

	// Nonexistent slot
	_, rc = EngineGetFactSlotByName(h, factID, "nonexistent")
	if rc != ErrNotFound {
		t.Fatalf("expected NotFound for nonexistent slot, got %d", rc)
	}
}

func TestEngineStep(t *testing.T) {
	h := EngineNewWithSource(`
		(defrule r1 => (assert (step1 done)))
		(defrule r2 (step1 done) => (assert (step2 done)))
	`)
	if h == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}
	defer EngineFree(h)

	// First step: r1 fires
	status, rc := EngineStep(h)
	if rc != ErrOK {
		t.Fatalf("EngineStep returned %d", rc)
	}
	if status != 1 {
		t.Fatalf("expected status 1 (fired), got %d", status)
	}

	// Second step: r2 fires
	status, rc = EngineStep(h)
	if rc != ErrOK {
		t.Fatalf("EngineStep returned %d", rc)
	}
	if status != 1 {
		t.Fatalf("expected status 1 (fired), got %d", status)
	}

	// Third step: agenda empty
	status, rc = EngineStep(h)
	if rc != ErrOK {
		t.Fatalf("EngineStep returned %d", rc)
	}
	if status != 0 {
		t.Fatalf("expected status 0 (empty), got %d", status)
	}
}

//nolint:funlen // integration test intentionally validates multiple introspection surfaces together.
func TestIntrospection(t *testing.T) {
	h := EngineNewWithSource(`
		(deftemplate sensor
			(slot id (type INTEGER))
			(slot value (type FLOAT)))
		(defrule check-sensor
			(sensor (id ?id) (value ?v&:(> ?v 100)))
			=>
			(printout t "Sensor " ?id " alert!" crlf))
	`)
	if h == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}
	defer EngineFree(h)

	// Template count
	tmplCount, rc := EngineTemplateCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineTemplateCount returned %d", rc)
	}
	if tmplCount < 1 {
		t.Fatalf("expected at least 1 template, got %d", tmplCount)
	}

	// Rule count
	ruleCount, rc := EngineRuleCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineRuleCount returned %d", rc)
	}
	if ruleCount != 1 {
		t.Fatalf("expected 1 rule, got %d", ruleCount)
	}

	// Rule info
	ruleName, salience, rc := EngineRuleInfo(h, 0)
	if rc != ErrOK {
		t.Fatalf("EngineRuleInfo returned %d", rc)
	}
	if ruleName != "check-sensor" {
		t.Fatalf("expected rule name 'check-sensor', got %q", ruleName)
	}
	if salience != 0 {
		t.Fatalf("expected default salience 0, got %d", salience)
	}

	// Module count and name
	modCount, rc := EngineModuleCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineModuleCount returned %d", rc)
	}
	if modCount < 1 {
		t.Fatalf("expected at least 1 module, got %d", modCount)
	}

	modName, rc := EngineModuleName(h, 0)
	if rc != ErrOK {
		t.Fatalf("EngineModuleName returned %d", rc)
	}
	if modName != "MAIN" {
		t.Fatalf("expected module name 'MAIN', got %q", modName)
	}

	// Current module
	curMod, rc := EngineCurrentModule(h)
	if rc != ErrOK {
		t.Fatalf("EngineCurrentModule returned %d", rc)
	}
	if curMod != "MAIN" {
		t.Fatalf("expected current module 'MAIN', got %q", curMod)
	}

	// Template slot introspection
	slotCount, rc := EngineTemplateSlotCount(h, "sensor")
	if rc != ErrOK {
		t.Fatalf("EngineTemplateSlotCount returned %d", rc)
	}
	if slotCount != 2 {
		t.Fatalf("expected 2 slots for sensor, got %d", slotCount)
	}

	slotName, rc := EngineTemplateSlotName(h, "sensor", 0)
	if rc != ErrOK {
		t.Fatalf("EngineTemplateSlotName returned %d", rc)
	}
	if slotName != "id" {
		t.Fatalf("expected slot name 'id', got %q", slotName)
	}
}

func TestGlobalVariable(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	rc := EngineLoadString(h, `(defglobal ?*x* = 42)`)
	if rc != ErrOK {
		t.Fatalf("EngineLoadString returned %d", rc)
	}

	rc = EngineReset(h)
	if rc != ErrOK {
		t.Fatalf("EngineReset returned %d", rc)
	}

	val, rc := EngineGetGlobal(h, "x")
	if rc != ErrOK {
		t.Fatalf("EngineGetGlobal returned %d", rc)
	}
	if val.value_type != ValueTypeInteger {
		t.Fatalf("expected integer, got %d", val.value_type)
	}
	if val.integer != 42 {
		t.Fatalf("expected 42, got %d", val.integer)
	}
}

func TestHalt(t *testing.T) {
	h := EngineNewWithSource(`
		(defrule loop
			=>
			(assert (keep-going)))
	`)
	if h == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}
	defer EngineFree(h)

	EngineHalt(h)

	halted, rc := EngineIsHalted(h)
	if rc != ErrOK {
		t.Fatalf("EngineIsHalted returned %d", rc)
	}
	if !halted {
		t.Fatal("expected engine to be halted")
	}
}

func TestAgendaCount(t *testing.T) {
	h := EngineNewWithSource(`
		(defrule r1 => (assert (done)))
		(defrule r2 => (assert (also-done)))
	`)
	if h == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}
	defer EngineFree(h)

	count, rc := EngineAgendaCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineAgendaCount returned %d", rc)
	}
	if count != 2 {
		t.Fatalf("expected 2 activations, got %d", count)
	}
}

func TestFreeUncheckedFromDifferentThread(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}

	done := make(chan ErrorCode, 1)
	go func() {
		done <- EngineFreeUnchecked(h)
	}()

	rc := <-done
	if rc != ErrOK {
		t.Fatalf("EngineFreeUnchecked from different thread returned %d", rc)
	}
}

func TestErrorRetrieval(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	// Trigger an error by loading invalid CLIPS
	rc := EngineLoadString(h, "(defrule bad ())") // malformed
	if rc == ErrOK {
		// The parser may accept weird forms, so let's try something definitely bad
		_ = EngineLoadString(h, "(this is not valid CLIPS at all !!!")
	}
	// We mostly just want to confirm the error retrieval functions don't crash
	_ = EngineLastError(h)

	// Global error
	_ = LastErrorGlobal()
	ClearErrorGlobal()
}

func TestAssertOrdered(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	rc := EngineReset(h)
	if rc != ErrOK {
		t.Fatalf("EngineReset returned %d", rc)
	}

	factID, rc := EngineAssertOrdered(h, "color", []Value{ValueSymbol("red")})
	if rc != ErrOK {
		t.Fatalf("EngineAssertOrdered returned %d, error: %s", rc, EngineLastError(h))
	}
	if factID == 0 {
		t.Fatal("expected non-zero fact ID")
	}

	count, rc := EngineFactCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineFactCount returned %d", rc)
	}
	if count != 1 {
		t.Fatalf("expected 1 fact, got %d", count)
	}
}

func TestEngineClear(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	rc := EngineLoadString(h, `(defrule r => (assert (x)))`)
	if rc != ErrOK {
		t.Fatalf("EngineLoadString returned %d", rc)
	}

	rc = EngineClear(h)
	if rc != ErrOK {
		t.Fatalf("EngineClear returned %d", rc)
	}

	// After clear, should have 0 rules
	ruleCount, rc := EngineRuleCount(h)
	if rc != ErrOK {
		t.Fatalf("EngineRuleCount returned %d", rc)
	}
	if ruleCount != 0 {
		t.Fatalf("expected 0 rules after clear, got %d", ruleCount)
	}
}

func TestPushInput(t *testing.T) {
	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)

	// Just test that push_input doesn't crash
	rc := EnginePushInput(h, "hello")
	if rc != ErrOK {
		t.Fatalf("EnginePushInput returned %d", rc)
	}
}

func TestEngineNewWithConfig(t *testing.T) {
	h := EngineNewWithConfigHelper(StringEncodingUTF8, ConflictStrategyDepth, 256)
	if h == nil {
		t.Fatal("EngineNewWithConfig returned nil")
	}
	defer EngineFree(h)
}

func TestFocusStack(t *testing.T) {
	h := EngineNewWithSource(`
		(defmodule A)
		(defmodule B)
	`)
	if h == nil {
		t.Fatal("EngineNewWithSource returned nil")
	}
	defer EngineFree(h)

	depth, rc := EngineFocusStackDepth(h)
	if rc != ErrOK {
		t.Fatalf("EngineFocusStackDepth returned %d", rc)
	}
	// After reset, focus stack may have at least MAIN
	if depth > 0 {
		entry, rc := EngineFocusStackEntry(h, 0)
		if rc != ErrOK {
			t.Fatalf("EngineFocusStackEntry returned %d", rc)
		}
		_ = entry // just making sure it doesn't crash
	}

	_, rc = EngineGetFocus(h)
	// May return NotFound if focus stack is empty; that's OK
	if rc != ErrOK && rc != ErrNotFound {
		t.Fatalf("EngineGetFocus returned unexpected %d", rc)
	}
}

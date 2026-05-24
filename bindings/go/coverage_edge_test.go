//nolint:funlen,gocyclo,maintidx // Coverage edge tests intentionally enumerate related API branches together.
package ferric

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"math"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"
	"unsafe"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
	"pgregory.net/rapid"
)

func resetFFIHooks() {
	ffiEngineNew = ffi.EngineNew
	ffiEngineNewWithConfig = ffi.EngineNewWithConfig
	ffiEngineNewWithSource = ffi.EngineNewWithSource
	ffiEngineNewWithSourceConfig = ffi.EngineNewWithSourceConfig
	ffiEngineDeserializeAs = ffi.EngineDeserializeAs
	ffiLastErrorGlobal = ffi.LastErrorGlobal
	ffiEngineFree = ffi.EngineFree
	ffiEngineFreeUnchecked = ffi.EngineFreeUnchecked
	ffiEngineLoadString = ffi.EngineLoadString
	ffiEngineAssertString = ffi.EngineAssertString
	ffiEngineAssertOrdered = ffi.EngineAssertOrdered
	ffiEngineAssertTemplate = ffi.EngineAssertTemplate
	ffiEngineRetract = ffi.EngineRetract
	ffiEngineFactIDs = ffi.EngineFactIDs
	ffiEngineFindFactIDs = ffi.EngineFindFactIDs
	ffiEngineFactCount = ffi.EngineFactCount
	ffiEngineRunEx = ffi.EngineRunEx
	ffiEngineStep = ffi.EngineStep
	ffiEngineHalt = ffi.EngineHalt
	ffiEngineReset = ffi.EngineReset
	ffiEngineClear = ffi.EngineClear
	ffiEngineSerializeAs = ffi.EngineSerializeAs
	ffiEngineRuleCount = ffi.EngineRuleCount
	ffiEngineRuleInfo = ffi.EngineRuleInfo
	ffiEngineTemplateCount = ffi.EngineTemplateCount
	ffiEngineTemplateName = ffi.EngineTemplateName
	ffiEngineGetGlobal = ffi.EngineGetGlobal
	ffiEngineCurrentModule = ffi.EngineCurrentModule
	ffiEngineGetFocus = ffi.EngineGetFocus
	ffiEngineFocusStackDepth = ffi.EngineFocusStackDepth
	ffiEngineFocusStackEntry = ffi.EngineFocusStackEntry
	ffiEngineAgendaCount = ffi.EngineAgendaCount
	ffiEngineIsHalted = ffi.EngineIsHalted
	ffiEngineGetOutput = ffi.EngineGetOutput
	ffiEngineClearOutput = ffi.EngineClearOutput
	ffiEnginePushInput = ffi.EnginePushInput
	ffiEngineActionDiagnosticCount = ffi.EngineActionDiagnosticCount
	ffiEngineActionDiagnosticCopy = ffi.EngineActionDiagnosticCopy
	ffiEngineClearActionDiagnostics = ffi.EngineClearActionDiagnostics
	ffiEngineGetFactType = ffi.EngineGetFactType
	ffiEngineGetFactFieldCount = ffi.EngineGetFactFieldCount
	ffiEngineGetFactField = ffi.EngineGetFactField
	ffiEngineGetFactTemplateName = ffi.EngineGetFactTemplateName
	ffiEngineTemplateSlotCount = ffi.EngineTemplateSlotCount
	ffiEngineTemplateSlotName = ffi.EngineTemplateSlotName
	ffiEngineGetFactRelation = ffi.EngineGetFactRelation
	ffiValueFree = ffi.ValueFree
	factsToWire = FactsToWire
}

func withFFIHooks(t *testing.T) {
	t.Helper()
	resetFFIHooks()
	t.Cleanup(resetFFIHooks)
}

type doneOnlyContext struct {
	done chan struct{}
}

func (c doneOnlyContext) Deadline() (time.Time, bool) { return time.Time{}, false }
func (c doneOnlyContext) Done() <-chan struct{}       { return c.done }
func (c doneOnlyContext) Err() error                  { return nil }
func (c doneOnlyContext) Value(any) any               { return nil }

func TestManualConfigurationValidationBranches(t *testing.T) {
	// These checks exercise the enum and integer validation that runs before
	// FFI engine construction. Keeping them as direct unit tests makes invalid
	// public options fail deterministically without depending on native errors.
	validEncodings := []Encoding{
		EncodingASCII,
		EncodingUTF8,
		EncodingASCIISymbolsUTF8Strings,
	}
	for _, enc := range validEncodings {
		if _, err := toFFIStringEncoding(enc); err != nil {
			t.Fatalf("encoding %d should be valid: %v", enc, err)
		}
	}

	validStrategies := []Strategy{
		StrategyDepth,
		StrategyBreadth,
		StrategyLEX,
		StrategyMEA,
	}
	for _, strategy := range validStrategies {
		if _, err := toFFIConflictStrategy(strategy); err != nil {
			t.Fatalf("strategy %d should be valid: %v", strategy, err)
		}
	}

	if _, err := NewEngine(WithEncoding(Encoding(99))); !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("invalid encoding should report ErrInvalidArgument, got %v", err)
	}
	if _, err := NewEngine(WithStrategy(Strategy(99))); !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("invalid strategy should report ErrInvalidArgument, got %v", err)
	}
	if _, err := NewEngine(WithMaxCallDepth(-1)); !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("negative max call depth should report ErrInvalidArgument, got %v", err)
	}
	if _, err := formatToFFI(Format(99)); !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("invalid snapshot format should report ErrInvalidArgument, got %v", err)
	}
	if _, err := NewEngine(WithSnapshot([]byte("data"), Format(99))); !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("invalid snapshot option should report ErrInvalidArgument, got %v", err)
	}
}

func TestManualIntegerConversionBoundaries(t *testing.T) {
	// Boundary checks pin the public binding to Go's host integer width. These
	// conversions guard native uintptr/uint64 values before exposing them as int.
	maxIntUint := uint64(^uint(0) >> 1)
	if got, err := uint64ToInt(maxIntUint); err != nil || got != int(maxIntUint) {
		t.Fatalf("uint64 max int conversion = (%d, %v)", got, err)
	}
	if _, err := uint64ToInt(maxIntUint + 1); !errors.Is(err, errIntOverflow) {
		t.Fatalf("uint64 overflow should report errIntOverflow, got %v", err)
	}

	maxIntPtr := uintptr(^uint(0) >> 1)
	if got, err := uintptrToInt(maxIntPtr); err != nil || got != int(maxIntPtr) {
		t.Fatalf("uintptr max int conversion = (%d, %v)", got, err)
	}
	if _, err := uintptrToInt(maxIntPtr + 1); !errors.Is(err, errIntOverflow) {
		t.Fatalf("uintptr overflow should report errIntOverflow, got %v", err)
	}
	if got := clampUintptrToInt(maxIntPtr + 1); got != int(maxIntPtr) {
		t.Fatalf("clamp overflow = %d, want %d", got, int(maxIntPtr))
	}
}

func TestManualErrorTypesAndTranslations(t *testing.T) {
	// The Go API promises stable errors.Is sentinels while still preserving
	// concrete error types. This verifies both explicit errors and FFI mappings.
	base := &FerricError{Code: 123, Message: "boom"}
	if got := base.Error(); got != "ferric: boom" {
		t.Fatalf("FerricError.Error() = %q", got)
	}

	concrete := []struct {
		err    error
		target error
	}{
		{&ParseError{}, ErrParse},
		{&CompileError{}, ErrCompile},
		{&RuntimeError{}, ErrRuntime},
		{&NotFoundError{}, ErrNotFound},
		{&IOError{}, ErrIO},
		{&SerializationError{}, ErrSerialization},
		{&ThreadViolationError{}, ErrThreadViolation},
		{&InvalidArgumentError{}, ErrInvalidArgument},
	}
	for _, tc := range concrete {
		if !errors.Is(tc.err, tc.target) {
			t.Fatalf("%T should match %v", tc.err, tc.target)
		}
	}

	ffi.ClearErrorGlobal()
	mapped := []struct {
		code   ffi.ErrorCode
		target error
	}{
		{ffi.ErrParseError, ErrParse},
		{ffi.ErrCompileError, ErrCompile},
		{ffi.ErrRuntimeError, ErrRuntime},
		{ffi.ErrNotFound, ErrNotFound},
		{ffi.ErrIOError, ErrIO},
		{ffi.ErrSerializationError, ErrSerialization},
		{ffi.ErrThreadViolation, ErrThreadViolation},
		{ffi.ErrInvalidArgument, ErrInvalidArgument},
	}
	for _, tc := range mapped {
		if err := errorFromFFI(tc.code, nil); !errors.Is(err, tc.target) {
			t.Fatalf("errorFromFFI(%d) = %T %v, want %v", tc.code, err, err, tc.target)
		}
	}
	if err := errorFromFFI(ffi.ErrOK, nil); err != nil {
		t.Fatalf("ErrOK should translate to nil, got %v", err)
	}
	if err := errorFromFFI(ffi.ErrNullPointer, nil); err == nil || !strings.Contains(err.Error(), "error code") {
		t.Fatalf("unknown FFI error should use generic fallback, got %v", err)
	}
}

func TestManualWireConversionEdges(t *testing.T) {
	// These examples cover every supported scalar shape plus the recursive
	// error paths. They document what callers may pass to wire conversion.
	cases := []struct {
		in   any
		want WireValue
	}{
		{int(7), WireValue{Kind: WireValueInteger, Integer: 7}},
		{int32(8), WireValue{Kind: WireValueInteger, Integer: 8}},
		{float32(1.25), WireValue{Kind: WireValueFloat, Float: float64(float32(1.25))}},
		{true, WireValue{Kind: WireValueSymbol, Text: "TRUE"}},
		{false, WireValue{Kind: WireValueSymbol, Text: "FALSE"}},
		{[]any{int32(1), false}, MultifieldValue(IntValue(1), SymbolValue("FALSE"))},
	}
	for _, tc := range cases {
		got, err := NativeToWireValue(tc.in)
		if err != nil {
			t.Fatalf("NativeToWireValue(%T) unexpected error: %v", tc.in, err)
		}
		if !reflect.DeepEqual(got, tc.want) {
			t.Fatalf("NativeToWireValue(%#v) = %#v, want %#v", tc.in, got, tc.want)
		}
	}

	if _, err := NativeToWireValue(struct{}{}); !errors.Is(err, errUnsupportedWireConversionType) {
		t.Fatalf("unsupported native type should fail, got %v", err)
	}
	if _, err := NativeToWireValue([]any{int64(1), struct{}{}}); !errors.Is(err, errUnsupportedWireConversionType) {
		t.Fatalf("unsupported nested native type should fail, got %v", err)
	}
	unknown := WireValue{Kind: WireValueKind("bogus")}
	if _, err := WireToNativeValue(unknown); !errors.Is(err, errUnknownWireValueKind) {
		t.Fatalf("unknown wire kind should fail, got %v", err)
	}
	if _, err := WireToNativeValue(MultifieldValue(unknown)); !errors.Is(err, errUnknownWireValueKind) {
		t.Fatalf("unknown nested wire kind should fail, got %v", err)
	}
	if _, err := WireSliceToNative([]WireValue{unknown}); !errors.Is(err, errUnknownWireValueKind) {
		t.Fatalf("bad wire slice should fail, got %v", err)
	}
	if _, err := WireMapToNative(map[string]WireValue{"bad": unknown}); !errors.Is(err, errUnknownWireValueKind) {
		t.Fatalf("bad wire map should fail, got %v", err)
	}
	if _, err := FactToWire(Fact{Type: FactTemplate, Slots: map[string]any{"bad": struct{}{}}}); !errors.Is(err, errUnsupportedWireConversionType) {
		t.Fatalf("bad template fact should fail, got %v", err)
	}
	if _, err := FactToWire(Fact{Type: FactOrdered, Fields: []any{struct{}{}}}); !errors.Is(err, errUnsupportedWireConversionType) {
		t.Fatalf("bad ordered fact should fail, got %v", err)
	}
	if _, err := FactsToWire([]Fact{{Type: FactOrdered, Fields: []any{struct{}{}}}}); !errors.Is(err, errUnsupportedWireConversionType) {
		t.Fatalf("bad fact slice should fail, got %v", err)
	}
}

func TestManualFFIValueConversionEdges(t *testing.T) {
	// The FFI conversion layer normalizes Go convenience types into CLIPS values.
	// These examples include bools, nil, multifields, and unsupported cleanup.
	cases := []struct {
		in   any
		want any
	}{
		{int(7), int64(7)},
		{int32(8), int64(8)},
		{float32(1.25), float64(float32(1.25))},
		{true, Symbol("TRUE")},
		{false, Symbol("FALSE")},
		{nil, nil},
		{[]any{int32(1), Symbol("x"), "s", nil}, []any{int64(1), Symbol("x"), "s", nil}},
	}
	for _, tc := range cases {
		v, err := goToFFIValue(tc.in)
		if err != nil {
			t.Fatalf("goToFFIValue(%T) unexpected error: %v", tc.in, err)
		}
		got := ffiValueToGoAndFree(&v)
		if !reflect.DeepEqual(got, tc.want) {
			t.Fatalf("ffi roundtrip %#v = %#v, want %#v", tc.in, got, tc.want)
		}
	}
	if _, err := goToFFIValue([]any{int64(1), struct{}{}}); !errors.Is(err, errUnsupportedGoTypeForFFI) {
		t.Fatalf("bad multifield element should fail and clean up, got %v", err)
	}
	if _, err := goToFFIValue(struct{}{}); !errors.Is(err, errUnsupportedGoTypeForFFI) {
		t.Fatalf("unsupported Go value should fail, got %v", err)
	}

	var external ffi.Value
	*(*ffi.ValueType)(unsafe.Pointer(&external)) = ffi.ValueTypeExternalAddress
	if got, ok := ffiValueToGo(&external).(unsafe.Pointer); !ok || got != nil {
		t.Fatalf("zero external pointer = (%T, %v), want nil unsafe.Pointer", got, got)
	}

	var unknown ffi.Value
	*(*ffi.ValueType)(unsafe.Pointer(&unknown)) = ffi.ValueType(9999)
	if got := ffiValueToGo(&unknown); got != nil {
		t.Fatalf("unknown value type = %v, want nil", got)
	}
}

func TestManualNilEngineErrorBranches(t *testing.T) {
	// A zero Engine has a nil native handle. The FFI should reject every native
	// operation without panicking, which exercises the Go defensive error paths.
	e := &Engine{}
	assertErr := func(name string, err error) {
		t.Helper()
		if err == nil {
			t.Fatalf("%s: expected error", name)
		}
	}

	if err := e.Close(); err != nil {
		t.Fatalf("Close on a nil handle should remain idempotent, got %v", err)
	}
	assertErr("Load", e.Load(`(defrule r =>)`))
	if _, err := e.AssertString("(assert (x))"); err == nil {
		t.Fatal("AssertString: expected error")
	}
	if _, err := e.AssertFact("x", int64(1)); err == nil {
		t.Fatal("AssertFact: expected error")
	}
	if _, err := e.AssertTemplate("x", map[string]any{}); err == nil {
		t.Fatal("AssertTemplate: expected error")
	}
	assertErr("Retract", e.Retract(1))
	if _, err := e.GetFact(1); err == nil {
		t.Fatal("GetFact: expected error")
	}
	if _, err := e.Facts(); err == nil {
		t.Fatal("Facts: expected error")
	}
	if _, err := e.FindFacts("x"); err == nil {
		t.Fatal("FindFacts: expected error")
	}
	if _, err := e.FactCount(); err == nil {
		t.Fatal("FactCount: expected error")
	}
	var nilCtx context.Context
	if _, err := e.RunWithLimit(nilCtx, 0); !errors.Is(err, errNilContext) {
		t.Fatalf("nil context should fail with errNilContext, got %v", err)
	}
	if _, err := e.Run(context.Background()); err == nil {
		t.Fatal("Run: expected error")
	}
	if _, err := e.RunWithLimit(t.Context(), 1); err == nil {
		t.Fatal("RunWithLimit cancelable context: expected error")
	}
	if _, err := e.Step(); err == nil {
		t.Fatal("Step: expected error")
	}
	assertErr("Reset", e.Reset())
	if _, err := e.Serialize(FormatBincode); err == nil {
		t.Fatal("Serialize: expected error")
	}
	if err := e.SerializeToFile("unused", Format(99)); !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("SerializeToFile invalid format should fail before writing, got %v", err)
	}
	if _, err := e.GetGlobal("missing"); err == nil {
		t.Fatal("GetGlobal: expected error")
	}
	if _, err := e.RulesE(); err == nil {
		t.Fatal("RulesE: expected error")
	}
	if _, err := e.TemplatesE(); err == nil {
		t.Fatal("TemplatesE: expected error")
	}
	if _, err := e.DiagnosticsE(); err == nil {
		t.Fatal("DiagnosticsE: expected error")
	}
	if _, err := e.CurrentModuleE(); err == nil {
		t.Fatal("CurrentModuleE: expected error")
	}
	if _, _, err := e.FocusE(); err == nil {
		t.Fatal("FocusE: expected error")
	}
	if _, err := e.FocusStackE(); err == nil {
		t.Fatal("FocusStackE: expected error")
	}
	if _, err := e.AgendaSizeE(); err == nil {
		t.Fatal("AgendaSizeE: expected error")
	}
	if _, err := e.IsHaltedE(); err == nil {
		t.Fatal("IsHaltedE: expected error")
	}

	if got := e.Rules(); got != nil {
		t.Fatalf("Rules nil handle = %#v, want nil", got)
	}
	if got := e.Templates(); got != nil {
		t.Fatalf("Templates nil handle = %#v, want nil", got)
	}
	if got := e.CurrentModule(); got != "" {
		t.Fatalf("CurrentModule nil handle = %q, want empty", got)
	}
	if name, ok := e.Focus(); name != "" || ok {
		t.Fatalf("Focus nil handle = (%q, %v), want empty false", name, ok)
	}
	if got := e.FocusStack(); got != nil {
		t.Fatalf("FocusStack nil handle = %#v, want nil", got)
	}
	if got := e.AgendaSize(); got != 0 {
		t.Fatalf("AgendaSize nil handle = %d, want 0", got)
	}
	if got := e.IsHalted(); got {
		t.Fatal("IsHalted nil handle = true, want false")
	}
	if got := e.Diagnostics(); got != nil {
		t.Fatalf("Diagnostics nil handle = %#v, want nil", got)
	}
}

func TestManualIteratorErrorAndEarlyBreakBranches(t *testing.T) {
	// Iterators intentionally hide errors in the simple forms and expose them in
	// the E forms. This test covers nil-handle errors and yield-stop branches.
	nilEngine := &Engine{}
	for range nilEngine.FactIter() {
		t.Fatal("FactIter should not yield for nil handle")
	}
	for range nilEngine.RuleIter() {
		t.Fatal("RuleIter should not yield for nil handle")
	}
	for range nilEngine.TemplateIter() {
		t.Fatal("TemplateIter should not yield for nil handle")
	}
	for range nilEngine.DiagnosticIter() {
		t.Fatal("DiagnosticIter should not yield for nil handle")
	}
	for _, err := range nilEngine.FactIterE() {
		if err == nil {
			t.Fatal("FactIterE nil handle should yield an error")
		}
	}
	for _, err := range nilEngine.RuleIterE() {
		if err == nil {
			t.Fatal("RuleIterE nil handle should yield an error")
		}
	}
	for _, err := range nilEngine.TemplateIterE() {
		if err == nil {
			t.Fatal("TemplateIterE nil handle should yield an error")
		}
	}
	for _, err := range nilEngine.DiagnosticIterE() {
		if err == nil {
			t.Fatal("DiagnosticIterE nil handle should yield an error")
		}
	}

	lockThread(t)
	e, err := NewEngine(WithSource(`
		(deftemplate sensor (slot id))
		(deftemplate alarm (slot level))
		(defrule r1 => (assert (a)))
		(defrule r2 => (assert (b)))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	for range e.RuleIter() {
		break
	}
	for range e.TemplateIter() {
		break
	}
	for _, err := range e.RuleIterE() {
		if err != nil {
			t.Fatalf("RuleIterE unexpected error: %v", err)
		}
		break
	}
	for _, err := range e.TemplateIterE() {
		if err != nil {
			t.Fatalf("TemplateIterE unexpected error: %v", err)
		}
		break
	}
}

func TestManualCancelableRunBatchesAndHaltReason(t *testing.T) {
	// A cancelable context uses the batched path. A chain longer than one batch
	// proves that LimitReached can continue across batches and still stop on the
	// caller's total limit.
	lockThread(t)
	e, err := NewEngine(WithSource(`
		(defrule chain
			(level ?n&:(< ?n 200))
			=>
			(assert (level (+ ?n 1))))
	`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	mustAssertFact(t, e, "level", int64(0))

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	result, err := e.RunWithLimit(ctx, 150)
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 150 || result.HaltReason != HaltLimitReached {
		t.Fatalf("batched limit result = %+v, want 150/LimitReached", result)
	}

	e2, err := NewEngine(WithSource(`(defrule r => (halt))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e2)
	halted, err := e2.Run(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if halted.HaltReason != HaltRequested {
		t.Fatalf("halted run reason = %v, want HaltRequested", halted.HaltReason)
	}

	e3, err := NewEngine(WithSource(`(defrule r => (halt))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e3)
	cancelable, cancel := context.WithCancel(context.Background())
	defer cancel()
	cancelableHalt, err := e3.RunWithLimit(cancelable, 10)
	if err != nil {
		t.Fatal(err)
	}
	if cancelableHalt.HaltReason != HaltRequested {
		t.Fatalf("cancelable halted run reason = %v, want HaltRequested", cancelableHalt.HaltReason)
	}
}

func TestManualCoordinatorWorkerAndManagerErrorBranches(t *testing.T) {
	// These checks cover coordinator/manager failures that occur before or
	// during worker cold-start, where user input names an unknown or invalid spec.
	if got := (roundRobinPolicy{}).PickWorker(RouteHint{}, 0, 10); got != 0 {
		t.Fatalf("round robin with no workers = %d, want 0", got)
	}

	var buf bytes.Buffer
	logger := slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{}))
	o := newObs(&coordConfig{logger: logger})
	w := &worker{
		specs:    map[string][]EngineOption{"bad": {WithSource("(defrule bad")}},
		engines:  make(map[string]*Engine),
		requests: make(chan workerRequest, 1),
		stop:     make(chan struct{}),
		done:     make(chan struct{}),
		obs:      o,
	}
	if _, err := w.getOrCreateEngine("missing"); !errors.Is(err, errUnknownEngineSpec) {
		t.Fatalf("missing worker spec should report errUnknownEngineSpec, got %v", err)
	}
	if _, err := w.getOrCreateEngine("bad"); err == nil || !strings.Contains(err.Error(), "creating engine") {
		t.Fatalf("bad worker spec should fail cold start, got %v", err)
	}
	if !strings.Contains(buf.String(), "engine cold start failed") {
		t.Fatalf("expected cold-start failure log, got %q", buf.String())
	}

	resp := make(chan error, 1)
	w.handle(workerRequest{specName: "missing", resp: resp})
	if err := <-resp; !errors.Is(err, errUnknownEngineSpec) {
		t.Fatalf("worker handle missing spec = %v", err)
	}

	mgr, err := NewManager(WithSource(`(defrule r => (assert (ok)))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, mgr)
	var nilMgrCtx context.Context
	if err := mgr.Do(nilMgrCtx, func(*Engine) error { return nil }); !errors.Is(err, errNilContext) {
		t.Fatalf("Do nil context = %v", err)
	}
	if _, err := mgr.Evaluate(context.Background(), nil); !errors.Is(err, errNilEvaluateRequest) {
		t.Fatalf("Evaluate nil request = %v", err)
	}
	if _, err := mgr.EvaluateNative(context.Background(), nil); !errors.Is(err, errNilEvaluateRequest) {
		t.Fatalf("EvaluateNative nil request = %v", err)
	}

	badMgr, err := NewManager(WithSource("(defrule bad"))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, badMgr)
	if _, err := badMgr.Evaluate(context.Background(), &EvaluateRequest{}); err == nil {
		t.Fatal("Evaluate with invalid source should fail during worker cold-start")
	}
	if _, err := badMgr.EvaluateNative(context.Background(), &EvaluateNativeRequest{}); err == nil {
		t.Fatal("EvaluateNative with invalid source should fail during worker cold-start")
	}
}

func TestManualAssertWireFactsAndEvaluateNativeErrors(t *testing.T) {
	// The request conversion layer rejects malformed wire facts before running
	// rules. These examples cover each validation branch and native conversion.
	lockThread(t)
	e, err := NewEngine(WithSource(`(deftemplate sensor (slot id))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	if err := assertWireFacts(e, []WireFactInput{{Kind: WireFactKindOrdered}}); !errors.Is(err, errOrderedFactPayloadMissing) {
		t.Fatalf("missing ordered payload = %v", err)
	}
	if err := assertWireFacts(e, []WireFactInput{OrderedFact("x", WireValue{Kind: "bad"})}); !errors.Is(err, errUnknownWireValueKind) {
		t.Fatalf("bad ordered field = %v", err)
	}
	if err := assertWireFacts(&Engine{}, []WireFactInput{OrderedFact("x", IntValue(1))}); err == nil {
		t.Fatal("ordered assertion through nil engine should fail")
	}
	if err := assertWireFacts(e, []WireFactInput{{Kind: WireFactKindTemplate}}); !errors.Is(err, errTemplateFactPayloadMissing) {
		t.Fatalf("missing template payload = %v", err)
	}
	if err := assertWireFacts(e, []WireFactInput{TemplateFact("sensor", map[string]WireValue{"id": {Kind: "bad"}})}); !errors.Is(err, errUnknownWireValueKind) {
		t.Fatalf("bad template slot = %v", err)
	}
	if err := assertWireFacts(&Engine{}, []WireFactInput{TemplateFact("sensor", map[string]WireValue{"id": IntValue(1)})}); err == nil {
		t.Fatal("template assertion through nil engine should fail")
	}
	if err := assertWireFacts(e, []WireFactInput{{Kind: WireFactKind("bogus")}}); !errors.Is(err, errUnsupportedFactKind) {
		t.Fatalf("unsupported fact kind = %v", err)
	}

	mgr, err := NewManager(WithSource(`(deftemplate sensor (slot id))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, mgr)
	if _, err := mgr.EvaluateNative(context.Background(), &EvaluateNativeRequest{
		Facts: []NativeFactInput{{TemplateName: "sensor", Slots: map[string]any{"id": struct{}{}}}},
	}); !errors.Is(err, errUnsupportedWireConversionType) {
		t.Fatalf("bad native slot = %v", err)
	}
	if _, err := mgr.EvaluateNative(context.Background(), &EvaluateNativeRequest{
		Facts: []NativeFactInput{{Relation: "x", Fields: []any{struct{}{}}}},
	}); !errors.Is(err, errUnsupportedWireConversionType) {
		t.Fatalf("bad native field = %v", err)
	}

	if _, err := mgr.Evaluate(context.Background(), &EvaluateRequest{
		Facts: []WireFactInput{{Kind: WireFactKindOrdered}},
	}); !errors.Is(err, errOrderedFactPayloadMissing) {
		t.Fatalf("Evaluate bad wire fact = %v", err)
	}

	stderrMgr, err := NewManager(WithSource(`(defrule warn => (printout stderr "warn" crlf))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, stderrMgr)
	result, err := stderrMgr.Evaluate(context.Background(), &EvaluateRequest{})
	if err != nil {
		t.Fatal(err)
	}
	if result.Output["stderr"] != "warn\n" {
		t.Fatalf("stderr output = %q, want warn", result.Output["stderr"])
	}

	if _, err := buildEvaluateResult(&Engine{}, &RunResult{}); err == nil {
		t.Fatal("buildEvaluateResult should fail when Facts fails")
	}
}

func TestManualPinnedEngineCancellationAndSerializationFile(t *testing.T) {
	// PinnedEngine should forward file serialization and respect cancellation
	// while waiting for an already-dispatched worker operation to finish.
	p, err := NewPinnedEngine(WithSource(`(defrule r => (assert (ok)))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, p)

	path := t.TempDir() + "/snapshot.bin"
	if err := p.SerializeToFile(path, FormatBincode); err != nil {
		t.Fatalf("SerializeToFile failed: %v", err)
	}
	var nilPinnedCtx context.Context
	if err := p.Do(nilPinnedCtx, func(*Engine) error { return nil }); !errors.Is(err, errNilContext) {
		t.Fatalf("PinnedEngine.Do nil context = %v", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	started := make(chan struct{})
	release := make(chan struct{})
	errCh := make(chan error, 1)
	go func() {
		errCh <- p.Do(ctx, func(*Engine) error {
			close(started)
			<-release
			return nil
		})
	}()
	<-started
	cancel()
	err = <-errCh
	close(release)
	if err == nil || !errors.Is(err, context.Canceled) {
		t.Fatalf("PinnedEngine.Do canceled while waiting = %v", err)
	}

	if err := p.Close(); err != nil {
		t.Fatal(err)
	}
	if err := p.Do(context.Background(), func(*Engine) error { return nil }); !errors.Is(err, errPinnedEngineClosed) {
		t.Fatalf("PinnedEngine.Do after close = %v", err)
	}

	blocked := &PinnedEngine{
		requests: make(chan pinnedRequest),
		done:     make(chan struct{}),
	}
	blockedCtx, blockedCancel := context.WithCancel(context.Background())
	blockedErr := make(chan error, 1)
	go func() {
		blockedErr <- blocked.tryEnqueue(blockedCtx, pinnedRequest{resp: make(chan error, 1)})
	}()
	blockedCancel()
	if err := <-blockedErr; err == nil || !errors.Is(err, context.Canceled) {
		t.Fatalf("blocked tryEnqueue canceled before dispatch = %v", err)
	}
}

func TestManualEngineSourceConfigWrongThreadCloseAndDiagnostics(t *testing.T) {
	// These examples cover source+config construction, Engine.Close's
	// thread-affinity error, and the simple diagnostic iterator success path.
	lockThread(t)

	e, err := NewEngine(
		WithSource(`(defrule r => (assert (ok)))`),
		WithStrategy(StrategyBreadth),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	wrongThread := make(chan error, 1)
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		wrongThread <- e.Close()
	}()
	if err := <-wrongThread; !errors.Is(err, ErrThreadViolation) {
		t.Fatalf("wrong-thread Close = %v", err)
	}

	diag, err := NewEngine(WithSource(`(defrule boom => (/ 1 0))`))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, diag)
	_, _ = diag.Run(context.Background())
	count := 0
	for range diag.DiagnosticIter() {
		count++
		break
	}
	if count == 0 {
		t.Fatal("DiagnosticIter yielded no diagnostics from a divide-by-zero rule")
	}
	for _, err := range diag.DiagnosticIterE() {
		if err != nil {
			t.Fatalf("DiagnosticIterE unexpected error: %v", err)
		}
		break
	}
}

func TestManualFinalizerAndBuildFactsHelpers(t *testing.T) {
	// The named finalizer keeps GC cleanup testable without waiting for the
	// runtime, and buildFacts centralizes ID-to-fact error handling.
	finalizeEngine(&Engine{closed: true})
	finalizeEngine(&Engine{})

	lockThread(t)
	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)
	if _, err := e.buildFacts([]uint64{999}); err == nil {
		t.Fatal("buildFacts should fail when an ID cannot be resolved")
	}
}

func TestManualHookedNewEngineNativeFallbacks(t *testing.T) {
	// These branches defend against native constructors reporting success while
	// returning a nil handle. Hooks make those impossible native states explicit.
	t.Run("snapshot nil handle", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineDeserializeAs = func([]byte, ffi.SerializationFormat) (ffi.EngineHandle, ffi.ErrorCode) {
			return nil, ffi.ErrOK
		}
		_, err := NewEngine(WithSnapshot([]byte("snapshot"), FormatBincode))
		if err == nil || !strings.Contains(err.Error(), "snapshot") {
			t.Fatalf("snapshot nil handle error = %v", err)
		}
	})

	t.Run("source nil handle empty native error", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineNewWithSource = func(string) ffi.EngineHandle { return nil }
		ffiLastErrorGlobal = func() string { return "" }
		_, err := NewEngine(WithSource("(defrule r =>)"))
		if err == nil || !strings.Contains(err.Error(), "failed to create engine from source") {
			t.Fatalf("source nil handle error = %v", err)
		}
	})

	t.Run("configured nil handle", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineNewWithConfig = func(*ffi.Config) ffi.EngineHandle { return nil }
		_, err := NewEngine(WithStrategy(StrategyBreadth))
		if err == nil || !strings.Contains(err.Error(), "failed to create engine") {
			t.Fatalf("configured nil handle error = %v", err)
		}
	})
}

func TestManualHookedRunEdgeBranches(t *testing.T) {
	// The real FFI should respect batch limits and host integer bounds. These
	// hooks prove the Go wrapper still handles violations predictably.
	t.Run("cancelable loop stops when previous batch over-fired", func(t *testing.T) {
		withFFIHooks(t)
		calls := 0
		ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
			calls++
			return 2, ffi.HaltReason(999), ffi.ErrOK
		}

		result, err := (&Engine{}).RunWithLimit(t.Context(), 1)
		if err != nil {
			t.Fatal(err)
		}
		if result.RulesFired != 2 || result.HaltReason != HaltLimitReached || calls != 1 {
			t.Fatalf("over-fired result = %+v after %d calls", result, calls)
		}
	})

	t.Run("cancelable fired count overflow", func(t *testing.T) {
		withFFIHooks(t)
		maxInt := uint64(^uint(0) >> 1)
		ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
			return maxInt + 1, ffi.HaltReasonAgendaEmpty, ffi.ErrOK
		}

		_, err := (&Engine{}).RunWithLimit(t.Context(), 0)
		if !errors.Is(err, errIntOverflow) {
			t.Fatalf("cancelable overflow error = %v", err)
		}
	})

	t.Run("direct fired count overflow", func(t *testing.T) {
		withFFIHooks(t)
		maxInt := uint64(^uint(0) >> 1)
		ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
			return maxInt + 1, ffi.HaltReasonAgendaEmpty, ffi.ErrOK
		}

		_, err := (&Engine{}).Run(context.Background())
		if !errors.Is(err, errIntOverflow) {
			t.Fatalf("direct overflow error = %v", err)
		}
	})
}

func TestManualHookedIntrospectionAndIteratorErrors(t *testing.T) {
	// The simple introspection APIs stop on mid-stream native errors, while E
	// variants and E iterators expose the error. Hooks simulate that mid-stream.
	withFFIHooks(t)
	e := &Engine{}

	ffiEngineFactIDs = func(ffi.EngineHandle) ([]uint64, ffi.ErrorCode) {
		return []uint64{1}, ffi.ErrOK
	}
	ffiEngineGetFactType = func(ffi.EngineHandle, uint64) (ffi.FactType, ffi.ErrorCode) {
		return 0, ffi.ErrNotFound
	}
	for range e.FactIter() {
		t.Fatal("FactIter should stop when buildFact fails")
	}
	for _, err := range e.FactIterE() {
		if err == nil {
			t.Fatal("FactIterE should yield buildFact error")
		}
	}

	ffiEngineRuleCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
	ffiEngineRuleInfo = func(ffi.EngineHandle, uintptr) (string, int32, ffi.ErrorCode) {
		return "", 0, ffi.ErrRuntimeError
	}
	if got := e.Rules(); len(got) != 0 {
		t.Fatalf("Rules after item error = %v, want empty", got)
	}
	if _, err := e.RulesE(); err == nil {
		t.Fatal("RulesE should return item error")
	}
	for range e.RuleIter() {
		t.Fatal("RuleIter should stop on item error")
	}
	for _, err := range e.RuleIterE() {
		if err == nil {
			t.Fatal("RuleIterE should yield item error")
		}
	}

	ffiEngineTemplateCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
	ffiEngineTemplateName = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
		return "", ffi.ErrRuntimeError
	}
	if got := e.Templates(); len(got) != 0 {
		t.Fatalf("Templates after item error = %v, want empty", got)
	}
	if _, err := e.TemplatesE(); err == nil {
		t.Fatal("TemplatesE should return item error")
	}
	for range e.TemplateIter() {
		t.Fatal("TemplateIter should stop on item error")
	}
	for _, err := range e.TemplateIterE() {
		if err == nil {
			t.Fatal("TemplateIterE should yield item error")
		}
	}

	ffiEngineFocusStackDepth = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
	ffiEngineFocusStackEntry = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
		return "", ffi.ErrRuntimeError
	}
	if got := e.FocusStack(); len(got) != 0 {
		t.Fatalf("FocusStack after item error = %v, want empty", got)
	}
	if _, err := e.FocusStackE(); err == nil {
		t.Fatal("FocusStackE should return item error")
	}

	ffiEngineActionDiagnosticCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
	ffiEngineActionDiagnosticCopy = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
		return "", ffi.ErrRuntimeError
	}
	if got := e.Diagnostics(); len(got) != 0 {
		t.Fatalf("Diagnostics after item error = %v, want empty", got)
	}
	if _, err := e.DiagnosticsE(); err == nil {
		t.Fatal("DiagnosticsE should return item error")
	}
	for range e.DiagnosticIter() {
		t.Fatal("DiagnosticIter should stop on item error")
	}
	for _, err := range e.DiagnosticIterE() {
		if err == nil {
			t.Fatal("DiagnosticIterE should yield item error")
		}
	}
}

func installOrderedBuildFactHooks() {
	ffiEngineGetFactType = func(ffi.EngineHandle, uint64) (ffi.FactType, ffi.ErrorCode) {
		return ffi.FactTypeOrdered, ffi.ErrOK
	}
	ffiEngineGetFactFieldCount = func(ffi.EngineHandle, uint64) (uintptr, ffi.ErrorCode) {
		return 1, ffi.ErrOK
	}
	ffiEngineGetFactField = func(ffi.EngineHandle, uint64, uintptr) (ffi.Value, ffi.ErrorCode) {
		return ffi.ValueInteger(1), ffi.ErrOK
	}
	ffiEngineGetFactRelation = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
		return "rel", ffi.ErrOK
	}
}

func installTemplateBuildFactHooks() {
	ffiEngineGetFactType = func(ffi.EngineHandle, uint64) (ffi.FactType, ffi.ErrorCode) {
		return ffi.FactTypeTemplate, ffi.ErrOK
	}
	ffiEngineGetFactFieldCount = func(ffi.EngineHandle, uint64) (uintptr, ffi.ErrorCode) {
		return 1, ffi.ErrOK
	}
	ffiEngineGetFactField = func(ffi.EngineHandle, uint64, uintptr) (ffi.Value, ffi.ErrorCode) {
		return ffi.ValueInteger(1), ffi.ErrOK
	}
	ffiEngineGetFactTemplateName = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
		return "tmpl", ffi.ErrOK
	}
	ffiEngineTemplateSlotCount = func(ffi.EngineHandle, string) (uintptr, ffi.ErrorCode) {
		return 1, ffi.ErrOK
	}
	ffiEngineTemplateSlotName = func(ffi.EngineHandle, string, uintptr) (string, ffi.ErrorCode) {
		return "slot", ffi.ErrOK
	}
}

func TestManualHookedBuildFactErrors(t *testing.T) {
	// buildFact composes multiple native lookups. Each subtest proves a later
	// lookup failure is wrapped instead of returning a partially corrupted Fact.
	cases := []struct {
		name  string
		setup func()
		want  string
	}{
		{
			name: "field count error",
			setup: func() {
				installOrderedBuildFactHooks()
				ffiEngineGetFactFieldCount = func(ffi.EngineHandle, uint64) (uintptr, ffi.ErrorCode) {
					return 0, ffi.ErrRuntimeError
				}
			},
		},
		{
			name: "field error",
			setup: func() {
				installOrderedBuildFactHooks()
				ffiEngineGetFactField = func(ffi.EngineHandle, uint64, uintptr) (ffi.Value, ffi.ErrorCode) {
					return ffi.Value{}, ffi.ErrRuntimeError
				}
			},
		},
		{
			name: "template name error",
			setup: func() {
				installTemplateBuildFactHooks()
				ffiEngineGetFactTemplateName = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
					return "", ffi.ErrRuntimeError
				}
			},
		},
		{
			name: "slot count error",
			setup: func() {
				installTemplateBuildFactHooks()
				ffiEngineTemplateSlotCount = func(ffi.EngineHandle, string) (uintptr, ffi.ErrorCode) {
					return 0, ffi.ErrRuntimeError
				}
			},
			want: "slot count",
		},
		{
			name: "slot name error",
			setup: func() {
				installTemplateBuildFactHooks()
				ffiEngineTemplateSlotName = func(ffi.EngineHandle, string, uintptr) (string, ffi.ErrorCode) {
					return "", ffi.ErrRuntimeError
				}
			},
			want: "slot name",
		},
		{
			name: "relation error",
			setup: func() {
				installOrderedBuildFactHooks()
				ffiEngineGetFactRelation = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
					return "", ffi.ErrRuntimeError
				}
			},
			want: "relation",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			withFFIHooks(t)
			tc.setup()
			_, err := (&Engine{}).buildFact(1)
			if err == nil {
				t.Fatal("expected buildFact error")
			}
			if tc.want != "" && !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("buildFact error = %v, want substring %q", err, tc.want)
			}
		})
	}
}

func TestManualHookedEvaluateAndPinnedEdges(t *testing.T) {
	// Evaluate's closure is normally backed by a healthy worker engine. Hooks
	// let us cover reset, run, and result-conversion failures deterministically.
	t.Run("reset failure", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineReset = func(ffi.EngineHandle) ffi.ErrorCode { return ffi.ErrRuntimeError }
		mgr, err := NewManager()
		if err != nil {
			t.Fatal(err)
		}
		defer mustClose(t, mgr)
		if _, err := mgr.Evaluate(context.Background(), &EvaluateRequest{}); err == nil {
			t.Fatal("Evaluate should fail when Reset fails")
		}
	})

	t.Run("run failure", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
			return 0, 0, ffi.ErrRuntimeError
		}
		mgr, err := NewManager()
		if err != nil {
			t.Fatal(err)
		}
		defer mustClose(t, mgr)
		if _, err := mgr.Evaluate(context.Background(), &EvaluateRequest{}); err == nil {
			t.Fatal("Evaluate should fail when Run fails")
		}
	})

	t.Run("wire conversion failure", func(t *testing.T) {
		withFFIHooks(t)
		factsToWire = func([]Fact) ([]WireFact, error) { return nil, errUnsupportedWireConversionType }
		e, err := NewEngine()
		if err != nil {
			t.Fatal(err)
		}
		defer mustClose(t, e)
		if _, err := buildEvaluateResult(e, &RunResult{}); err == nil {
			t.Fatal("buildEvaluateResult should fail when factsToWire fails")
		}
	})

	t.Run("pinned select cancellation", func(t *testing.T) {
		withFFIHooks(t)
		done := make(chan struct{})
		close(done)
		p := &PinnedEngine{requests: make(chan pinnedRequest), done: make(chan struct{})}
		err := p.tryEnqueue(doneOnlyContext{done: done}, pinnedRequest{resp: make(chan error, 1)})
		if err == nil {
			t.Fatal("tryEnqueue should return an error from ctx.Done")
		}
	})
}

func TestPropertyConfigurationHelpers(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		n := rapid.IntRange(0, 1<<31-1).Draw(t, "max_call_depth")
		got, err := intToUintptr(n)
		if err != nil {
			t.Fatalf("non-negative depth rejected: %v", err)
		}
		if got != uintptr(n) {
			t.Fatalf("intToUintptr(%d) = %d", n, got)
		}

		enc := rapid.SampledFrom([]Encoding{
			EncodingASCII,
			EncodingUTF8,
			EncodingASCIISymbolsUTF8Strings,
		}).Draw(t, "encoding")
		if _, err := toFFIStringEncoding(enc); err != nil {
			t.Fatalf("valid encoding rejected: %v", err)
		}

		strategy := rapid.SampledFrom([]Strategy{
			StrategyDepth,
			StrategyBreadth,
			StrategyLEX,
			StrategyMEA,
		}).Draw(t, "strategy")
		if _, err := toFFIConflictStrategy(strategy); err != nil {
			t.Fatalf("valid strategy rejected: %v", err)
		}

		if _, err := intToUintptr(-1); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("negative max call depth error = %v", err)
		}
		if _, err := toFFIStringEncoding(Encoding(rapid.IntRange(100, 200).Draw(t, "bad_encoding"))); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("invalid encoding error = %v", err)
		}
		if _, err := toFFIConflictStrategy(Strategy(rapid.IntRange(100, 200).Draw(t, "bad_strategy"))); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("invalid strategy error = %v", err)
		}
		if _, err := formatToFFI(Format(rapid.IntRange(100, 200).Draw(t, "bad_format"))); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("invalid format error = %v", err)
		}
		if _, err := NewEngine(WithEncoding(Encoding(rapid.IntRange(100, 200).Draw(t, "engine_bad_encoding")))); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("NewEngine invalid encoding error = %v", err)
		}
		if _, err := NewEngine(WithStrategy(Strategy(rapid.IntRange(100, 200).Draw(t, "engine_bad_strategy")))); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("NewEngine invalid strategy error = %v", err)
		}
		if _, err := NewEngine(WithMaxCallDepth(-1)); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("NewEngine invalid call depth error = %v", err)
		}
		if _, err := NewCoordinator(nil, Threads(0)); !errors.Is(err, errInvalidThreadCount) {
			t.Fatalf("invalid coordinator thread count error = %v", err)
		}
		if got := (roundRobinPolicy{}).PickWorker(RouteHint{}, 0, rapid.Uint64().Draw(t, "counter")); got != 0 {
			t.Fatalf("roundRobinPolicy empty pool = %d, want 0", got)
		}
	})
}

func TestPropertyWireConversionErrorSurfaces(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		kind := WireValueKind(rapid.String().Filter(func(s string) bool {
			switch WireValueKind(s) {
			case WireValueVoid, WireValueInteger, WireValueFloat, WireValueSymbol, WireValueString, WireValueMultifield:
				return false
			default:
				return true
			}
		}).Draw(t, "kind"))
		_, err := WireToNativeValue(WireValue{Kind: kind})
		if !errors.Is(err, errUnknownWireValueKind) {
			t.Fatalf("unknown kind %q error = %v", kind, err)
		}
	})
}

func TestPropertyHookedNewEngineFallbacks(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		resetFFIHooks()
		defer resetFFIHooks()

		scenario := rapid.SampledFrom([]string{"snapshot", "source", "config"}).Draw(t, "scenario")
		switch scenario {
		case "snapshot":
			ffiEngineDeserializeAs = func([]byte, ffi.SerializationFormat) (ffi.EngineHandle, ffi.ErrorCode) {
				return nil, ffi.ErrOK
			}
			_, err := NewEngine(WithSnapshot([]byte("snapshot"), FormatBincode))
			if err == nil {
				t.Fatal("snapshot nil handle should fail")
			}
		case "source":
			ffiEngineNewWithSource = func(string) ffi.EngineHandle { return nil }
			ffiLastErrorGlobal = func() string { return "" }
			_, err := NewEngine(WithSource("(defrule r =>)"))
			if err == nil {
				t.Fatal("source nil handle should fail")
			}
		case "config":
			ffiEngineNewWithConfig = func(*ffi.Config) ffi.EngineHandle { return nil }
			_, err := NewEngine(WithStrategy(StrategyBreadth))
			if err == nil {
				t.Fatal("configured nil handle should fail")
			}
		}
	})
}

func TestPropertyHookedRunEdges(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		resetFFIHooks()
		defer resetFFIHooks()

		scenario := rapid.SampledFrom([]string{"overfire", "cancelable_overflow", "direct_overflow"}).Draw(t, "scenario")
		maxInt := uint64(^uint(0) >> 1)
		switch scenario {
		case "overfire":
			fired := rapid.Uint64Range(2, 20).Draw(t, "fired")
			ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
				return fired, ffi.HaltReason(999), ffi.ErrOK
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			result, err := (&Engine{}).RunWithLimit(ctx, 1)
			if err != nil {
				t.Fatal(err)
			}
			//nolint:gosec // fired is drawn from rapid.Uint64Range(2, 20), safely fits in int
			if result.RulesFired != int(fired) || result.HaltReason != HaltLimitReached {
				t.Fatalf("overfire result = %+v, fired %d", result, fired)
			}
		case "cancelable_overflow":
			ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
				return maxInt + 1, ffi.HaltReasonAgendaEmpty, ffi.ErrOK
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			_, err := (&Engine{}).RunWithLimit(ctx, 0)
			if !errors.Is(err, errIntOverflow) {
				t.Fatalf("cancelable overflow error = %v", err)
			}
		case "direct_overflow":
			ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
				return maxInt + 1, ffi.HaltReasonAgendaEmpty, ffi.ErrOK
			}
			_, err := (&Engine{}).Run(context.Background())
			if !errors.Is(err, errIntOverflow) {
				t.Fatalf("direct overflow error = %v", err)
			}
		}
	})
}

func TestPropertyHookedIntrospectionErrors(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		resetFFIHooks()
		defer resetFFIHooks()

		e := &Engine{}
		scenario := rapid.SampledFrom([]string{"fact", "rule", "template", "focus", "diagnostic"}).Draw(t, "scenario")
		switch scenario {
		case "fact":
			ffiEngineFactIDs = func(ffi.EngineHandle) ([]uint64, ffi.ErrorCode) {
				return []uint64{rapid.Uint64().Draw(t, "fact_id")}, ffi.ErrOK
			}
			ffiEngineGetFactType = func(ffi.EngineHandle, uint64) (ffi.FactType, ffi.ErrorCode) {
				return 0, ffi.ErrNotFound
			}
			for range e.FactIter() {
				t.Fatal("FactIter should stop on buildFact error")
			}
			for _, err := range e.FactIterE() {
				if err == nil {
					t.Fatal("FactIterE should yield buildFact error")
				}
			}
		case "rule":
			ffiEngineRuleCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
			ffiEngineRuleInfo = func(ffi.EngineHandle, uintptr) (string, int32, ffi.ErrorCode) {
				return "", 0, ffi.ErrRuntimeError
			}
			if len(e.Rules()) != 0 {
				t.Fatal("Rules should return partial empty slice on item error")
			}
			if _, err := e.RulesE(); err == nil {
				t.Fatal("RulesE should return item error")
			}
		case "template":
			ffiEngineTemplateCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
			ffiEngineTemplateName = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
				return "", ffi.ErrRuntimeError
			}
			if len(e.Templates()) != 0 {
				t.Fatal("Templates should return partial empty slice on item error")
			}
			if _, err := e.TemplatesE(); err == nil {
				t.Fatal("TemplatesE should return item error")
			}
		case "focus":
			ffiEngineFocusStackDepth = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
			ffiEngineFocusStackEntry = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
				return "", ffi.ErrRuntimeError
			}
			if len(e.FocusStack()) != 0 {
				t.Fatal("FocusStack should return partial empty slice on item error")
			}
			if _, err := e.FocusStackE(); err == nil {
				t.Fatal("FocusStackE should return item error")
			}
		case "diagnostic":
			ffiEngineActionDiagnosticCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 1, ffi.ErrOK }
			ffiEngineActionDiagnosticCopy = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
				return "", ffi.ErrRuntimeError
			}
			if len(e.Diagnostics()) != 0 {
				t.Fatal("Diagnostics should return partial empty slice on item error")
			}
			if _, err := e.DiagnosticsE(); err == nil {
				t.Fatal("DiagnosticsE should return item error")
			}
		}
	})
}

func TestPropertyHookedBuildFactErrors(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		resetFFIHooks()
		defer resetFFIHooks()

		scenario := rapid.SampledFrom([]string{
			"field_count", "field", "template_name", "slot_count", "slot_name", "relation",
		}).Draw(t, "scenario")
		switch scenario {
		case "field_count":
			installOrderedBuildFactHooks()
			ffiEngineGetFactFieldCount = func(ffi.EngineHandle, uint64) (uintptr, ffi.ErrorCode) {
				return 0, ffi.ErrRuntimeError
			}
		case "field":
			installOrderedBuildFactHooks()
			ffiEngineGetFactField = func(ffi.EngineHandle, uint64, uintptr) (ffi.Value, ffi.ErrorCode) {
				return ffi.Value{}, ffi.ErrRuntimeError
			}
		case "template_name":
			installTemplateBuildFactHooks()
			ffiEngineGetFactTemplateName = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
				return "", ffi.ErrRuntimeError
			}
		case "slot_count":
			installTemplateBuildFactHooks()
			ffiEngineTemplateSlotCount = func(ffi.EngineHandle, string) (uintptr, ffi.ErrorCode) {
				return 0, ffi.ErrRuntimeError
			}
		case "slot_name":
			installTemplateBuildFactHooks()
			ffiEngineTemplateSlotName = func(ffi.EngineHandle, string, uintptr) (string, ffi.ErrorCode) {
				return "", ffi.ErrRuntimeError
			}
		case "relation":
			installOrderedBuildFactHooks()
			ffiEngineGetFactRelation = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
				return "", ffi.ErrRuntimeError
			}
		}
		if _, err := (&Engine{}).buildFact(rapid.Uint64().Draw(t, "fact_id")); err == nil {
			t.Fatal("buildFact should fail for injected native lookup error")
		}
	})
}

func TestPropertyHookedEvaluateAndPinnedEdges(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		resetFFIHooks()
		defer resetFFIHooks()

		scenario := rapid.SampledFrom([]string{"reset", "run", "wire", "pinned"}).Draw(t, "scenario")
		switch scenario {
		case "reset":
			ffiEngineReset = func(ffi.EngineHandle) ffi.ErrorCode { return ffi.ErrRuntimeError }
			mgr, err := NewManager()
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = mgr.Close() }()
			if _, err := mgr.Evaluate(context.Background(), &EvaluateRequest{}); err == nil {
				t.Fatal("Evaluate should fail when Reset fails")
			}
		case "run":
			ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
				return 0, 0, ffi.ErrRuntimeError
			}
			mgr, err := NewManager()
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = mgr.Close() }()
			if _, err := mgr.Evaluate(context.Background(), &EvaluateRequest{}); err == nil {
				t.Fatal("Evaluate should fail when Run fails")
			}
		case "wire":
			factsToWire = func([]Fact) ([]WireFact, error) { return nil, errUnsupportedWireConversionType }
			e, err := NewEngine()
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = e.Close() }()
			if _, err := buildEvaluateResult(e, &RunResult{}); err == nil {
				t.Fatal("buildEvaluateResult should fail when factsToWire fails")
			}
		case "pinned":
			done := make(chan struct{})
			close(done)
			p := &PinnedEngine{requests: make(chan pinnedRequest), done: make(chan struct{})}
			if err := p.tryEnqueue(doneOnlyContext{done: done}, pinnedRequest{resp: make(chan error, 1)}); err == nil {
				t.Fatal("tryEnqueue should fail when Done is selected")
			}
		}
	})
}

func TestPropertyEngineManagerAndPinnedSurfaceSweep(t *testing.T) {
	lockThread(t)

	tmpDir := t.TempDir()
	source := `
		(defglobal ?*threshold* = 10)
		(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
		(defrule bootstrap => (assert (booted)))
		(defrule color-seen (color ?c) => (assert (matched ?c)) (printout t ?c crlf))
		(defrule sensor-seen (sensor (id ?id) (value ?v)) => (assert (observed ?id)))
	`

	rapid.Check(t, func(rt *rapid.T) {
		id := rapid.Int64Range(1, 1000).Draw(rt, "id")
		value := rapid.Float64Range(0.1, 1000.0).Draw(rt, "value")
		policyIndex := rapid.IntRange(-8, 8).Draw(rt, "policy_index")
		format := rapid.SampledFrom([]Format{
			FormatBincode,
			FormatJSON,
			FormatCBOR,
			FormatMessagePack,
			FormatPostcard,
		}).Draw(rt, "format")

		e, err := NewEngine(
			WithSource(source),
			WithEncoding(EncodingUTF8),
			WithStrategy(StrategyDepth),
			WithMaxCallDepth(512),
		)
		if err != nil {
			rt.Fatal(err)
		}
		defer func() { _ = e.Close() }()

		if err := e.Load(`(defrule loaded => (assert (loaded)))`); err != nil {
			rt.Fatal(err)
		}
		if err := e.Reset(); err != nil {
			rt.Fatal(err)
		}
		if fired, err := e.Step(); err != nil {
			rt.Fatal(err)
		} else if fired == nil {
			rt.Fatal("expected a bootstrap rule to fire")
		}

		colorID, err := e.AssertString("(assert (color red))")
		if err != nil {
			rt.Fatal(err)
		}
		dataID, err := e.AssertFact("data", id, Symbol("ok"))
		if err != nil {
			rt.Fatal(err)
		}
		sensorID, err := e.AssertTemplate("sensor", map[string]any{"id": id, "value": value})
		if err != nil {
			rt.Fatal(err)
		}
		if _, err := e.GetFact(sensorID); err != nil {
			rt.Fatal(err)
		}
		if facts, err := e.Facts(); err != nil || len(facts) == 0 {
			rt.Fatalf("Facts = (%v, %v), want facts", facts, err)
		}
		if facts, err := e.FindFacts("data"); err != nil || len(facts) == 0 {
			rt.Fatalf("FindFacts(data) = (%v, %v), want facts", facts, err)
		}
		if count, err := e.FactCount(); err != nil || count == 0 {
			rt.Fatalf("FactCount = (%d, %v), want positive", count, err)
		}

		_ = e.Rules()
		_, _ = e.RulesE()
		for range e.RuleIter() { //nolint:revive // exhaust iterator without inspection to exercise the surface
		}
		for _, err := range e.RuleIterE() {
			if err != nil {
				rt.Fatal(err)
			}
		}
		_ = e.Templates()
		_, _ = e.TemplatesE()
		for range e.TemplateIter() { //nolint:revive // exhaust iterator without inspection to exercise the surface
		}
		for _, err := range e.TemplateIterE() {
			if err != nil {
				rt.Fatal(err)
			}
		}
		_, _ = e.GetGlobal("threshold")
		_ = e.CurrentModule()
		_, _ = e.CurrentModuleE()
		_, _ = e.Focus()
		_, _, _ = e.FocusE()
		_ = e.FocusStack()
		_, _ = e.FocusStackE()
		_ = e.AgendaSize()
		_, _ = e.AgendaSizeE()
		_ = e.IsHalted()
		_, _ = e.IsHaltedE()
		_ = e.Diagnostics()
		_, _ = e.DiagnosticsE()
		for range e.DiagnosticIter() { //nolint:revive // exhaust iterator without inspection to exercise the surface
		}
		for _, err := range e.DiagnosticIterE() {
			if err != nil {
				rt.Fatal(err)
			}
		}
		e.ClearDiagnostics()
		e.PushInput("unused")
		for range e.FactIter() {
		}
		for _, err := range e.FactIterE() {
			if err != nil {
				rt.Fatal(err)
			}
		}

		runResult, err := e.RunWithLimit(context.Background(), 1)
		if err != nil {
			rt.Fatal(err)
		}
		if runResult.RulesFired < 0 {
			rt.Fatal("negative rules fired")
		}
		if _, err := e.Run(context.Background()); err != nil {
			rt.Fatal(err)
		}
		_, _ = e.GetOutput("t")
		e.ClearOutput("t")

		data, err := e.Serialize(format)
		if err != nil {
			rt.Fatal(err)
		}
		restored, err := NewEngine(WithSnapshot(data, format))
		if err != nil {
			rt.Fatal(err)
		}
		_ = restored.Close()
		path := tmpDir + "/engine-surface.bin"
		if err := e.SerializeToFile(path, format); err != nil {
			rt.Fatal(err)
		}
		fromFile, err := NewEngineFromFile(path, format)
		if err != nil {
			rt.Fatal(err)
		}
		_ = fromFile.Close()

		if err := e.Retract(colorID); err != nil {
			rt.Fatal(err)
		}
		if err := e.Retract(dataID); err != nil {
			rt.Fatal(err)
		}
		e.Halt()
		e.Clear()
		_ = HaltAgendaEmpty.String()
		_ = HaltLimitReached.String()
		_ = HaltRequested.String()
		_ = HaltReason(99).String()

		mgr, err := NewManager(WithSource(source))
		if err != nil {
			rt.Fatal(err)
		}
		defer func() { _ = mgr.Close() }()
		req := &EvaluateRequest{Facts: []WireFactInput{
			OrderedFact("color", SymbolValue("blue"), StringValue("label"), MultifieldValue(IntValue(id))),
			TemplateFact("sensor", map[string]WireValue{"id": IntValue(id), "value": FloatValue(value)}),
		}}
		if _, err := mgr.Evaluate(context.Background(), req); err != nil {
			rt.Fatal(err)
		}
		req.Limit = 1
		if _, err := mgr.Evaluate(context.Background(), req); err != nil {
			rt.Fatal(err)
		}
		if _, err := mgr.EvaluateNative(context.Background(), &EvaluateNativeRequest{Facts: []NativeFactInput{
			{Relation: "color", Fields: []any{Symbol("green")}},
			{TemplateName: "sensor", Slots: map[string]any{"id": id, "value": value}},
		}}); err != nil {
			rt.Fatal(err)
		}

		coord, err := NewCoordinator(
			[]EngineSpec{{Name: "sweep", Options: []EngineOption{WithSource(source)}}},
			Threads(rapid.IntRange(1, 3).Draw(rt, "threads")),
			WithDispatchPolicy(fixedIndexPolicy{index: policyIndex}),
			WithLogger(slog.Default()),
			WithTracerProvider(nil),
			WithMeterProvider(nil),
		)
		if err != nil {
			rt.Fatal(err)
		}
		defer func() { _ = coord.Close() }()
		cm, err := coord.Manager("sweep")
		if err != nil {
			rt.Fatal(err)
		}
		if err := cm.Do(context.Background(), func(engine *Engine) error {
			_, err := engine.Run(context.Background())
			return err
		}); err != nil {
			rt.Fatal(err)
		}

		p, err := NewPinnedEngine(WithSource(source))
		if err != nil {
			rt.Fatal(err)
		}
		defer func() { _ = p.Close() }()
		if err := p.Load(`(defrule pinned-loaded => (assert (pinned-loaded)))`); err != nil {
			rt.Fatal(err)
		}
		if err := p.Reset(); err != nil {
			rt.Fatal(err)
		}
		_, _ = p.Step()
		pColorID, err := p.AssertString("(assert (color purple))")
		if err != nil {
			rt.Fatal(err)
		}
		if _, err := p.AssertFact("data", id); err != nil {
			rt.Fatal(err)
		}
		pSensorID, err := p.AssertTemplate("sensor", map[string]any{"id": id, "value": value})
		if err != nil {
			rt.Fatal(err)
		}
		if _, err := p.GetFact(pSensorID); err != nil {
			rt.Fatal(err)
		}
		_, _ = p.Facts()
		_, _ = p.FindFacts("data")
		_, _ = p.FactCount()
		_, _ = p.RunWithLimit(context.Background(), 1)
		_, _ = p.Run(context.Background())
		_, _ = p.Serialize(format)
		if err := p.SerializeToFile(tmpDir+"/pinned-surface.bin", format); err != nil {
			rt.Fatal(err)
		}
		_ = p.Rules()
		_ = p.Templates()
		_, _ = p.GetGlobal("threshold")
		_ = p.CurrentModule()
		_, _ = p.Focus()
		_ = p.FocusStack()
		_ = p.AgendaSize()
		_ = p.IsHalted()
		_, _ = p.GetOutput("t")
		p.ClearOutput("t")
		p.PushInput("unused")
		_ = p.Diagnostics()
		p.ClearDiagnostics()
		if err := p.Retract(pColorID); err != nil {
			rt.Fatal(err)
		}
		p.Halt()
		p.Clear()
	})
}

func TestPropertyErrorSentinelsAndFFIValueConversions(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		text := rapid.String().Draw(t, "text")
		i := rapid.Int64().Draw(t, "integer")
		i32 := rapid.Int32().Draw(t, "integer32")
		f := rapid.Float64().Filter(func(v float64) bool { return !math.IsNaN(v) }).Draw(t, "float")

		for _, tc := range []struct {
			code   ffi.ErrorCode
			target error
		}{
			{ffi.ErrParseError, ErrParse},
			{ffi.ErrCompileError, ErrCompile},
			{ffi.ErrRuntimeError, ErrRuntime},
			{ffi.ErrNotFound, ErrNotFound},
			{ffi.ErrIOError, ErrIO},
			{ffi.ErrSerializationError, ErrSerialization},
			{ffi.ErrThreadViolation, ErrThreadViolation},
			{ffi.ErrInvalidArgument, ErrInvalidArgument},
		} {
			if err := errorFromFFI(tc.code, nil); !errors.Is(err, tc.target) {
				t.Fatalf("errorFromFFI(%d) = %v, want %v", tc.code, err, tc.target)
			}
		}
		if err := errorFromFFI(ffi.ErrOK, nil); err != nil {
			t.Fatalf("ErrOK translated to %v", err)
		}
		if err := errorFromFFI(ffi.ErrorCode(999), nil); err == nil || err.Error() == "" {
			t.Fatalf("unknown FFI error translated to %v", err)
		}

		values := []any{
			int(i),
			i,
			i32,
			f,
			float32(f),
			Symbol(text),
			text,
			true,
			false,
			nil,
			[]any{i, Symbol(text), text},
		}
		for _, value := range values {
			fv, err := goToFFIValue(value)
			if err != nil {
				t.Fatalf("goToFFIValue(%T) failed: %v", value, err)
			}
			_ = ffiValueToGo(&fv)
			ffi.ValueFree(&fv)
		}
		if _, err := goToFFIValue(struct{}{}); !errors.Is(err, errUnsupportedGoTypeForFFI) {
			t.Fatalf("unsupported FFI value error = %v", err)
		}
		if _, err := goToFFIValue([]any{struct{}{}}); !errors.Is(err, errUnsupportedGoTypeForFFI) {
			t.Fatalf("unsupported nested FFI value error = %v", err)
		}

		var external ffi.Value
		*(*ffi.ValueType)(unsafe.Pointer(&external)) = ffi.ValueTypeExternalAddress
		if _, ok := ffiValueToGo(&external).(unsafe.Pointer); !ok {
			t.Fatal("external FFI value did not convert to unsafe.Pointer")
		}
		var unknown ffi.Value
		*(*ffi.ValueType)(unsafe.Pointer(&unknown)) = ffi.ValueType(999)
		if got := ffiValueToGo(&unknown); got != nil {
			t.Fatalf("unknown FFI value = %v, want nil", got)
		}

		nativeCases := []any{
			int(i),
			i32,
			i,
			float32(f),
			f,
			Symbol(text),
			text,
			true,
			false,
			nil,
			[]any{i, Symbol(text), text},
		}
		for _, value := range nativeCases {
			wire, err := NativeToWireValue(value)
			if err != nil {
				t.Fatalf("NativeToWireValue(%T) failed: %v", value, err)
			}
			if _, err := WireToNativeValue(wire); err != nil {
				t.Fatalf("WireToNativeValue(%v) failed: %v", wire, err)
			}
		}
		if _, err := NativeToWireValue(struct{}{}); !errors.Is(err, errUnsupportedWireConversionType) {
			t.Fatalf("unsupported native wire conversion error = %v", err)
		}
		if _, err := NativeToWireValue([]any{struct{}{}}); !errors.Is(err, errUnsupportedWireConversionType) {
			t.Fatalf("unsupported nested native wire conversion error = %v", err)
		}
		if _, err := WireSliceToNative([]WireValue{{Kind: WireValueKind("bad")}}); !errors.Is(err, errUnknownWireValueKind) {
			t.Fatalf("bad wire slice conversion error = %v", err)
		}
		if _, err := WireMapToNative(map[string]WireValue{"x": {Kind: WireValueKind("bad")}}); !errors.Is(err, errUnknownWireValueKind) {
			t.Fatalf("bad wire map conversion error = %v", err)
		}
		if _, err := FactToWire(Fact{Type: FactOrdered, Fields: []any{struct{}{}}}); !errors.Is(err, errUnsupportedWireConversionType) {
			t.Fatalf("bad ordered fact conversion error = %v", err)
		}
		if _, err := FactToWire(Fact{Type: FactTemplate, Slots: map[string]any{"x": struct{}{}}}); !errors.Is(err, errUnsupportedWireConversionType) {
			t.Fatalf("bad template fact conversion error = %v", err)
		}
		if _, err := FactsToWire([]Fact{{Type: FactOrdered, Fields: []any{struct{}{}}}}); !errors.Is(err, errUnsupportedWireConversionType) {
			t.Fatalf("bad facts conversion error = %v", err)
		}
	})
}

func TestPropertyHookedMutationAndAccessorErrors(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		resetFFIHooks()
		defer resetFFIHooks()

		e := &Engine{}
		scenario := rapid.SampledFrom([]string{
			"finalizer",
			"close",
			"load",
			"assert_string",
			"assert_ordered",
			"assert_template",
			"retract",
			"fact_ids",
			"find_fact_ids",
			"fact_count",
			"step",
			"serialize",
			"get_global",
			"current_module",
			"focus",
			"agenda",
			"is_halted",
			"diagnostics",
		}).Draw(t, "scenario")
		switch scenario {
		case "finalizer":
			calls := 0
			ffiEngineFreeUnchecked = func(ffi.EngineHandle) ffi.ErrorCode {
				calls++
				return ffi.ErrOK
			}
			finalizeEngine(&Engine{})
			finalizeEngine(&Engine{closed: true})
			if calls != 1 {
				t.Fatalf("finalizer free calls = %d, want 1", calls)
			}
		case "close":
			ffiEngineFree = func(ffi.EngineHandle) ffi.ErrorCode { return ffi.ErrRuntimeError }
			if err := e.Close(); err == nil {
				t.Fatal("Close should surface native free errors")
			}
		case "load":
			ffiEngineLoadString = func(ffi.EngineHandle, string) ffi.ErrorCode { return ffi.ErrRuntimeError }
			if err := e.Load("bad"); err == nil {
				t.Fatal("Load should surface native errors")
			}
		case "assert_string":
			ffiEngineAssertString = func(ffi.EngineHandle, string) (uint64, ffi.ErrorCode) {
				return 0, ffi.ErrRuntimeError
			}
			if _, err := e.AssertString("(x)"); err == nil {
				t.Fatal("AssertString should surface native errors")
			}
		case "assert_ordered":
			ffiEngineAssertOrdered = func(ffi.EngineHandle, string, []ffi.Value) (uint64, ffi.ErrorCode) {
				return 0, ffi.ErrRuntimeError
			}
			if _, err := e.AssertFact("x", int64(1)); err == nil {
				t.Fatal("AssertFact should surface native errors")
			}
		case "assert_template":
			ffiEngineAssertTemplate = func(ffi.EngineHandle, string, []string, []ffi.Value) (uint64, ffi.ErrorCode) {
				return 0, ffi.ErrRuntimeError
			}
			if _, err := e.AssertTemplate("x", map[string]any{"slot": int64(1)}); err == nil {
				t.Fatal("AssertTemplate should surface native errors")
			}
		case "retract":
			ffiEngineRetract = func(ffi.EngineHandle, uint64) ffi.ErrorCode { return ffi.ErrRuntimeError }
			if err := e.Retract(rapid.Uint64().Draw(t, "fact_id")); err == nil {
				t.Fatal("Retract should surface native errors")
			}
		case "fact_ids":
			ffiEngineFactIDs = func(ffi.EngineHandle) ([]uint64, ffi.ErrorCode) { return nil, ffi.ErrRuntimeError }
			if _, err := e.Facts(); err == nil {
				t.Fatal("Facts should surface native errors")
			}
		case "find_fact_ids":
			ffiEngineFindFactIDs = func(ffi.EngineHandle, string) ([]uint64, ffi.ErrorCode) {
				return nil, ffi.ErrRuntimeError
			}
			if _, err := e.FindFacts("x"); err == nil {
				t.Fatal("FindFacts should surface native errors")
			}
		case "fact_count":
			ffiEngineFactCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			if _, err := e.FactCount(); err == nil {
				t.Fatal("FactCount should surface native errors")
			}
		case "step":
			ffiEngineStep = func(ffi.EngineHandle) (int32, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			if _, err := e.Step(); err == nil {
				t.Fatal("Step should surface native errors")
			}
		case "serialize":
			ffiEngineSerializeAs = func(ffi.EngineHandle, ffi.SerializationFormat) ([]byte, ffi.ErrorCode) {
				return nil, ffi.ErrRuntimeError
			}
			if _, err := e.Serialize(FormatBincode); err == nil {
				t.Fatal("Serialize should surface native errors")
			}
		case "get_global":
			ffiEngineGetGlobal = func(ffi.EngineHandle, string) (ffi.Value, ffi.ErrorCode) {
				return ffi.Value{}, ffi.ErrRuntimeError
			}
			if _, err := e.GetGlobal("x"); err == nil {
				t.Fatal("GetGlobal should surface native errors")
			}
		case "current_module":
			ffiEngineCurrentModule = func(ffi.EngineHandle) (string, ffi.ErrorCode) { return "", ffi.ErrRuntimeError }
			if got := e.CurrentModule(); got != "" {
				t.Fatalf("CurrentModule error result = %q, want empty", got)
			}
			if _, err := e.CurrentModuleE(); err == nil {
				t.Fatal("CurrentModuleE should surface native errors")
			}
		case "focus":
			ffiEngineGetFocus = func(ffi.EngineHandle) (string, ffi.ErrorCode) { return "", ffi.ErrRuntimeError }
			if name, ok := e.Focus(); name != "" || ok {
				t.Fatalf("Focus error result = (%q, %v), want empty false", name, ok)
			}
			if _, _, err := e.FocusE(); err == nil {
				t.Fatal("FocusE should surface native errors")
			}
		case "agenda":
			ffiEngineAgendaCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			if got := e.AgendaSize(); got != 0 {
				t.Fatalf("AgendaSize error result = %d, want 0", got)
			}
			if _, err := e.AgendaSizeE(); err == nil {
				t.Fatal("AgendaSizeE should surface native errors")
			}
		case "is_halted":
			ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) { return false, ffi.ErrRuntimeError }
			if got := e.IsHalted(); got {
				t.Fatal("IsHalted error result = true, want false")
			}
			if _, err := e.IsHaltedE(); err == nil {
				t.Fatal("IsHaltedE should surface native errors")
			}
		case "diagnostics":
			ffiEngineActionDiagnosticCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
				return 0, ffi.ErrRuntimeError
			}
			if got := e.Diagnostics(); got != nil {
				t.Fatalf("Diagnostics error result = %v, want nil", got)
			}
			if _, err := e.DiagnosticsE(); err == nil {
				t.Fatal("DiagnosticsE should surface native errors")
			}
		}
	})
}

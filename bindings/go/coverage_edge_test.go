//nolint:funlen,gocyclo,maintidx // Coverage edge tests intentionally enumerate related API branches together.
package ferric

import (
	"context"
	"errors"
	"math"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"unsafe"

	"github.com/plx/ferric-rules/bindings/go/internal/ffi"
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
	ffiEngineContinueRunEx = ffi.EngineContinueRunEx
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
	ffiEngineGetOutputCopy = ffi.EngineGetOutputCopy
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
	ffiValueSymbolBytes = ffi.ValueSymbolBytes
	ffiValueStringBytes = ffi.ValueStringBytes
	ffiValueMultifieldCopy = ffi.ValueMultifieldCopy
	ffiValueFree = ffi.ValueFree
}

func withFFIHooks(t *testing.T) {
	t.Helper()
	resetFFIHooks()
	t.Cleanup(resetFFIHooks)
}

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

	// The errors.Is sentinel match must be backed by the documented concrete
	// type carrying the originating FFI code, not just any error.
	var re *RuntimeError
	if err := errorFromFFI(ffi.ErrRuntimeError, nil); !errors.As(err, &re) || re.Code != int(ffi.ErrRuntimeError) {
		t.Fatalf("errorFromFFI(runtime) = %#v, want *RuntimeError with code %d", re, int(ffi.ErrRuntimeError))
	}
	// The generic fallback is a plain *FerricError that still propagates the code.
	var fe *FerricError
	if err := errorFromFFI(ffi.ErrNullPointer, nil); !errors.As(err, &fe) || fe.Code != int(ffi.ErrNullPointer) {
		t.Fatalf("errorFromFFI(null) = %#v, want *FerricError with code %d", fe, int(ffi.ErrNullPointer))
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

	// Verify the documented cleanup actually runs: the elements converted
	// before an unsupported one must each be freed exactly once.
	t.Run("multifield error frees converted elements", func(t *testing.T) {
		withFFIHooks(t)
		freed := 0
		ffiValueFree = func(value *ffi.Value) {
			freed++
			ffi.ValueFree(value)
		}
		if _, err := goToFFIValue([]any{"a", []any{"b"}, struct{}{}}); !errors.Is(err, errUnsupportedGoTypeForFFI) {
			t.Fatalf("expected unsupported error, got %v", err)
		}
		if freed != 3 {
			t.Fatalf("freed %d converted values, want 3", freed)
		}
	})

	t.Run("multifield copy failure frees every borrowed element", func(t *testing.T) {
		withFFIHooks(t)
		copied := 0
		freed := 0
		ffiValueMultifieldCopy = func(elements []ffi.Value) (ffi.Value, ffi.ErrorCode) {
			copied++
			if len(elements) != 3 {
				t.Fatalf("copy received %d elements, want 3", len(elements))
			}
			return ffi.ValueVoid(), ffi.ErrInvalidArgument
		}
		ffiValueFree = func(value *ffi.Value) {
			freed++
			ffi.ValueFree(value)
		}

		if _, err := goToFFIValue([]any{"a", Symbol("b"), int64(3)}); !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("copy failure = %v, want ErrInvalidArgument", err)
		}
		if copied != 1 {
			t.Fatalf("copy called %d times, want 1", copied)
		}
		if freed != 3 {
			t.Fatalf("freed %d borrowed elements, want 3", freed)
		}
	})

	var external ffi.Value
	*(*ffi.ValueType)(unsafe.Pointer(&external)) = ffi.ValueTypeExternalAddress //nolint:gosec // intentional unsafe write of opaque ffi.Value type tag to exercise the discriminator branch
	if got, ok := ffiValueToGo(&external).(unsafe.Pointer); !ok || got != nil {
		t.Fatalf("zero external pointer = (%T, %v), want nil unsafe.Pointer", got, got)
	}

	var unknown ffi.Value
	*(*ffi.ValueType)(unsafe.Pointer(&unknown)) = ffi.ValueType(9999) //nolint:gosec // intentional unsafe write of opaque ffi.Value type tag to exercise the unknown-type branch
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

	// Representative check that the nil-handle path surfaces the native
	// null-pointer code as a typed *FerricError, not just "some error".
	var fe *FerricError
	if err := e.Load(`(defrule r =>)`); !errors.As(err, &fe) || fe.Code != int(ffi.ErrNullPointer) {
		t.Fatalf("Load on nil handle = %#v, want *FerricError code %d", fe, int(ffi.ErrNullPointer))
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
	if err := e.Close(); err != nil {
		t.Fatalf("Close on a nil handle should remain idempotent, got %v", err)
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

func TestManualEngineSourceConfigTransferredCloseAndDiagnostics(t *testing.T) {
	// These examples cover source+config construction, Engine.Close's
	// transferred close, and the simple diagnostic iterator success path.
	lockThread(t)

	e, err := NewEngine(
		WithSource(`(defrule r => (assert (ok)))`),
		WithStrategy(StrategyBreadth),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	transferredClose := make(chan error, 1)
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		transferredClose <- e.Close()
	}()
	if err := <-transferredClose; err != nil {
		t.Fatalf("transferred Close = %v", err)
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
	if _, err := e.buildFacts(e.handle, []uint64{999}); err == nil {
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
		var fe *FerricError
		if !errors.As(err, &fe) || !strings.Contains(err.Error(), "snapshot") {
			t.Fatalf("snapshot nil handle error = %v, want *FerricError mentioning snapshot", err)
		}
	})

	t.Run("source nil handle empty native error", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineNewWithSource = func(string) ffi.EngineHandle { return nil }
		ffiLastErrorGlobal = func() string { return "" }
		_, err := NewEngine(WithSource("(defrule r =>)"))
		// A nil handle from a source build is reported as a parse failure.
		var pe *ParseError
		if !errors.As(err, &pe) || !errors.Is(err, ErrParse) ||
			!strings.Contains(err.Error(), "failed to create engine from source") {
			t.Fatalf("source nil handle error = %v, want *ParseError fallback message", err)
		}
	})

	t.Run("source nil handle prefers native message", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineNewWithSource = func(string) ffi.EngineHandle { return nil }
		ffiLastErrorGlobal = func() string { return "native parse boom" }
		_, err := NewEngine(WithSource("(defrule r =>)"))
		// When the native error channel has a message, it is preferred over the
		// generic fallback.
		if !errors.Is(err, ErrParse) || !strings.Contains(err.Error(), "native parse boom") {
			t.Fatalf("source nil handle error = %v, want native message preferred", err)
		}
	})

	t.Run("configured nil handle", func(t *testing.T) {
		withFFIHooks(t)
		ffiEngineNewWithConfig = func(*ffi.Config) ffi.EngineHandle { return nil }
		_, err := NewEngine(WithStrategy(StrategyBreadth))
		var fe *FerricError
		if !errors.As(err, &fe) || !strings.Contains(err.Error(), "failed to create engine") {
			t.Fatalf("configured nil handle error = %v, want *FerricError", err)
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

// TestManualHookedIntrospectionKeepsItemsBeforeError pins the partial-result
// contract that the item-0-fails cases above cannot: when a mid-stream item
// (here index 1 of 2) fails, the simple accessors break and return the items
// collected *before* the failure rather than discarding them or returning nil.
func TestManualHookedIntrospectionKeepsItemsBeforeError(t *testing.T) {
	withFFIHooks(t)
	e := &Engine{}

	ffiEngineRuleCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 2, ffi.ErrOK }
	ffiEngineRuleInfo = func(_ ffi.EngineHandle, i uintptr) (string, int32, ffi.ErrorCode) {
		if i == 0 {
			return "r0", 7, ffi.ErrOK
		}
		return "", 0, ffi.ErrRuntimeError
	}
	if got := e.Rules(); len(got) != 1 || got[0].Name != "r0" {
		t.Fatalf("Rules after item-1 error = %v, want [r0]", got)
	}

	ffiEngineTemplateCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 2, ffi.ErrOK }
	ffiEngineTemplateName = func(_ ffi.EngineHandle, i uintptr) (string, ffi.ErrorCode) {
		if i == 0 {
			return "t0", ffi.ErrOK
		}
		return "", ffi.ErrRuntimeError
	}
	if got := e.Templates(); len(got) != 1 || got[0] != "t0" {
		t.Fatalf("Templates after item-1 error = %v, want [t0]", got)
	}

	ffiEngineFocusStackDepth = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 2, ffi.ErrOK }
	ffiEngineFocusStackEntry = func(_ ffi.EngineHandle, i uintptr) (string, ffi.ErrorCode) {
		if i == 0 {
			return "MAIN", ffi.ErrOK
		}
		return "", ffi.ErrRuntimeError
	}
	if got := e.FocusStack(); len(got) != 1 || got[0] != "MAIN" {
		t.Fatalf("FocusStack after item-1 error = %v, want [MAIN]", got)
	}

	ffiEngineActionDiagnosticCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 2, ffi.ErrOK }
	ffiEngineActionDiagnosticCopy = func(_ ffi.EngineHandle, i uintptr) (string, ffi.ErrorCode) {
		if i == 0 {
			return "d0", ffi.ErrOK
		}
		return "", ffi.ErrRuntimeError
	}
	if got := e.Diagnostics(); len(got) != 1 || got[0] != "d0" {
		t.Fatalf("Diagnostics after item-1 error = %v, want [d0]", got)
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
			_, err := (&Engine{}).buildFact(nil, 1)
			// Every lookup failure must preserve the underlying native code
			// (ErrRuntime) through any fmt.Errorf wrapping buildFact adds.
			if !errors.Is(err, ErrRuntime) {
				t.Fatalf("buildFact error = %v, want ErrRuntime", err)
			}
			if tc.want != "" && !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("buildFact error = %v, want substring %q", err, tc.want)
			}
		})
	}
}

func TestPropertyConfigurationHelpers(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		n := rapid.IntRange(0, math.MaxInt).Draw(t, "max_call_depth")
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
	})
}

// TestPropertyEngineSurfaceSweep fuzzes the raw Engine API surface and the
// serialize/restore round-trip across every format.
func TestPropertyEngineSurfaceSweep(t *testing.T) {
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
		} else if !fired {
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
		sensorFact, err := e.GetFact(sensorID)
		if err != nil {
			rt.Fatal(err)
		}
		// The drawn slot values must round-trip back through GetFact.
		if got, ok := sensorFact.Slots["id"].(int64); !ok || got != id {
			rt.Fatalf("sensor.id slot = %v, want %d", sensorFact.Slots["id"], id)
		}
		if got, ok := sensorFact.Slots["value"].(float64); !ok || got != value {
			rt.Fatalf("sensor.value slot = %v, want %v", sensorFact.Slots["value"], value)
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
		for range e.FactIter() { //nolint:revive // exhaust iterator without inspection to exercise the surface
		}
		for _, err := range e.FactIterE() {
			if err != nil {
				rt.Fatal(err)
			}
		}

		if _, err := e.RunWithLimit(context.Background(), 1); err != nil {
			rt.Fatal(err)
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
		wantCount, err := e.FactCount()
		if err != nil {
			rt.Fatal(err)
		}
		restored, err := NewEngine(WithSnapshot(data, format))
		if err != nil {
			rt.Fatal(err)
		}
		// A restored snapshot must reproduce the original engine's fact set.
		if gotCount, err := restored.FactCount(); err != nil || gotCount != wantCount {
			rt.Fatalf("restored FactCount = (%d, %v), want %d", gotCount, err, wantCount)
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
		if err := e.Clear(); err != nil {
			rt.Fatal(err)
		}
	})
}

func TestPropertyErrorSentinelsAndFFIValueConversions(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		ffiText := rapid.String().Filter(func(value string) bool {
			return !strings.ContainsRune(value, '\x00')
		}).Draw(t, "ffi_text")
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
			Symbol(ffiText),
			ffiText,
			true,
			false,
			nil,
			[]any{i, Symbol(ffiText), ffiText},
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
		*(*ffi.ValueType)(unsafe.Pointer(&external)) = ffi.ValueTypeExternalAddress //nolint:gosec // intentional unsafe write of opaque ffi.Value type tag to exercise the discriminator branch
		if _, ok := ffiValueToGo(&external).(unsafe.Pointer); !ok {
			t.Fatal("external FFI value did not convert to unsafe.Pointer")
		}
		var unknown ffi.Value
		*(*ffi.ValueType)(unsafe.Pointer(&unknown)) = ffi.ValueType(999) //nolint:gosec // intentional unsafe write of opaque ffi.Value type tag to exercise the unknown-type branch
		if got := ffiValueToGo(&unknown); got != nil {
			t.Fatalf("unknown FFI value = %v, want nil", got)
		}
	})
}

// TestManualFinalizerFreesLiveSkipsClosed pins the non-obvious finalizer
// invariant directly (rather than as 1 of N random branches): a live engine is
// freed exactly once, and an already-closed engine is skipped.
func TestManualFinalizerFreesLiveSkipsClosed(t *testing.T) {
	withFFIHooks(t)
	calls := 0
	ffiEngineFreeUnchecked = func(ffi.EngineHandle) ffi.ErrorCode {
		calls++
		return ffi.ErrOK
	}
	finalizeEngine(&Engine{})             // live engine: frees once
	finalizeEngine(&Engine{closed: true}) // already closed: skipped
	if calls != 1 {
		t.Fatalf("finalizer free calls = %d, want 1", calls)
	}
}

// TestManualHookedMutationAndAccessorErrors verifies that each Engine method
// translates an injected native ErrRuntimeError into the ErrRuntime sentinel
// (mutators and E-accessors) and returns its documented zero value for the
// non-erroring accessors. Asserting errors.Is(ErrRuntime) — not merely err !=
// nil — proves the FFI error code is actually carried through, not swallowed
// or replaced by an unrelated error.
func TestManualHookedMutationAndAccessorErrors(t *testing.T) {
	cases := []struct {
		name  string
		setup func()
		check func(t *testing.T, e *Engine)
	}{
		{
			name:  "close",
			setup: func() { ffiEngineFree = func(ffi.EngineHandle) ffi.ErrorCode { return ffi.ErrRuntimeError } },
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if err := e.Close(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("Close err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "load",
			setup: func() {
				ffiEngineLoadString = func(ffi.EngineHandle, string) ffi.ErrorCode { return ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if err := e.Load("bad"); !errors.Is(err, ErrRuntime) {
					t.Fatalf("Load err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "assert_string",
			setup: func() {
				ffiEngineAssertString = func(ffi.EngineHandle, string) (uint64, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.AssertString("(x)"); !errors.Is(err, ErrRuntime) {
					t.Fatalf("AssertString err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "assert_ordered",
			setup: func() {
				ffiEngineAssertOrdered = func(ffi.EngineHandle, string, []ffi.Value) (uint64, ffi.ErrorCode) {
					return 0, ffi.ErrRuntimeError
				}
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.AssertFact("x", int64(1)); !errors.Is(err, ErrRuntime) {
					t.Fatalf("AssertFact err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "assert_template",
			setup: func() {
				ffiEngineAssertTemplate = func(ffi.EngineHandle, string, []string, []ffi.Value) (uint64, ffi.ErrorCode) {
					return 0, ffi.ErrRuntimeError
				}
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.AssertTemplate("x", map[string]any{"slot": int64(1)}); !errors.Is(err, ErrRuntime) {
					t.Fatalf("AssertTemplate err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name:  "retract",
			setup: func() { ffiEngineRetract = func(ffi.EngineHandle, uint64) ffi.ErrorCode { return ffi.ErrRuntimeError } },
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if err := e.Retract(1); !errors.Is(err, ErrRuntime) {
					t.Fatalf("Retract err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "fact_ids",
			setup: func() {
				ffiEngineFactIDs = func(ffi.EngineHandle) ([]uint64, ffi.ErrorCode) { return nil, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.Facts(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("Facts err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "find_fact_ids",
			setup: func() {
				ffiEngineFindFactIDs = func(ffi.EngineHandle, string) ([]uint64, ffi.ErrorCode) { return nil, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.FindFacts("x"); !errors.Is(err, ErrRuntime) {
					t.Fatalf("FindFacts err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "fact_count",
			setup: func() {
				ffiEngineFactCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.FactCount(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("FactCount err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "step",
			setup: func() {
				ffiEngineStep = func(ffi.EngineHandle) (int32, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.Step(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("Step err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name:  "clear",
			setup: func() { ffiEngineClear = func(ffi.EngineHandle) ffi.ErrorCode { return ffi.ErrRuntimeError } },
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if err := e.Clear(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("Clear err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "serialize",
			setup: func() {
				ffiEngineSerializeAs = func(ffi.EngineHandle, ffi.SerializationFormat) ([]byte, ffi.ErrorCode) {
					return nil, ffi.ErrRuntimeError
				}
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.Serialize(FormatBincode); !errors.Is(err, ErrRuntime) {
					t.Fatalf("Serialize err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "get_global",
			setup: func() {
				ffiEngineGetGlobal = func(ffi.EngineHandle, string) (ffi.Value, ffi.ErrorCode) {
					return ffi.Value{}, ffi.ErrRuntimeError
				}
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if _, err := e.GetGlobal("x"); !errors.Is(err, ErrRuntime) {
					t.Fatalf("GetGlobal err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "current_module",
			setup: func() {
				ffiEngineCurrentModule = func(ffi.EngineHandle) (string, ffi.ErrorCode) { return "", ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if got := e.CurrentModule(); got != "" {
					t.Fatalf("CurrentModule = %q, want empty", got)
				}
				if _, err := e.CurrentModuleE(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("CurrentModuleE err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "focus",
			setup: func() {
				ffiEngineGetFocus = func(ffi.EngineHandle) (string, ffi.ErrorCode) { return "", ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if name, ok := e.Focus(); name != "" || ok {
					t.Fatalf("Focus = (%q, %v), want empty false", name, ok)
				}
				if _, _, err := e.FocusE(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("FocusE err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "agenda",
			setup: func() {
				ffiEngineAgendaCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if got := e.AgendaSize(); got != 0 {
					t.Fatalf("AgendaSize = %d, want 0", got)
				}
				if _, err := e.AgendaSizeE(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("AgendaSizeE err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "is_halted",
			setup: func() {
				ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) { return false, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if got := e.IsHalted(); got {
					t.Fatalf("IsHalted = true, want false")
				}
				if _, err := e.IsHaltedE(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("IsHaltedE err = %v, want ErrRuntime", err)
				}
			},
		},
		{
			name: "diagnostics",
			setup: func() {
				ffiEngineActionDiagnosticCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) { return 0, ffi.ErrRuntimeError }
			},
			check: func(t *testing.T, e *Engine) {
				t.Helper()
				if got := e.Diagnostics(); got != nil {
					t.Fatalf("Diagnostics = %v, want nil", got)
				}
				if _, err := e.DiagnosticsE(); !errors.Is(err, ErrRuntime) {
					t.Fatalf("DiagnosticsE err = %v, want ErrRuntime", err)
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			withFFIHooks(t)
			tc.setup()
			tc.check(t, &Engine{})
		})
	}
}

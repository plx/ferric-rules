package ferric

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"runtime"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

func TestEngineRealFFIPostClose(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	e, err := NewEngine()
	if err != nil {
		t.Fatalf("NewEngine: %v", err)
	}
	if err := e.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

	if _, err := e.AssertString("(probe)"); !errors.Is(err, ErrEngineClosed) {
		t.Fatalf("AssertString after Close = %v, want ErrEngineClosed", err)
	}
	if _, err := e.Run(context.Background()); !errors.Is(err, ErrEngineClosed) {
		t.Fatalf("Run after Close = %v, want ErrEngineClosed", err)
	}
	if _, err := e.GetFact(1); !errors.Is(err, ErrEngineClosed) {
		t.Fatalf("GetFact after Close = %v, want ErrEngineClosed", err)
	}
}

//nolint:gocyclo,err113,funlen,maintidx // The exhaustive table keeps each method's zero-value contract beside its call.
func TestEngineMethodsAfterClose(t *testing.T) {
	withFFIHooks(t)

	freeCalls := 0
	ffiEngineFree = func(ffi.EngineHandle) ffi.ErrorCode {
		freeCalls++
		return ffi.ErrOK
	}

	e := &Engine{}
	if err := e.Close(); err != nil {
		t.Fatalf("initial Close: %v", err)
	}
	if freeCalls != 1 {
		t.Fatalf("initial Close called free %d times, want 1", freeCalls)
	}

	forbidEngineOperationFFI(t)
	ffiEngineFree = func(ffi.EngineHandle) ffi.ErrorCode {
		t.Error("repeated Close reached native free")
		return ffi.ErrOK
	}

	wantClosed := func(err error) error {
		if !errors.Is(err, ErrEngineClosed) {
			return fmt.Errorf("want ErrEngineClosed: %w", err)
		}
		return nil
	}
	wantNil := func(name string, value any, err error) error {
		if value != nil && !reflect.ValueOf(value).IsNil() {
			return fmt.Errorf("%s value = %#v, want nil", name, value)
		}
		return wantClosed(err)
	}

	tests := []struct {
		name string
		call func() error
	}{
		{"Close", e.Close},
		{"Load", func() error { return wantClosed(e.Load("(deffacts x)")) }},
		{"AssertString", func() error {
			id, err := e.AssertString("(x)")
			if id != 0 {
				return fmt.Errorf("id = %d, want 0", id)
			}
			return wantClosed(err)
		}},
		{"AssertFact", func() error {
			id, err := e.AssertFact("x", int64(1))
			if id != 0 {
				return fmt.Errorf("id = %d, want 0", id)
			}
			return wantClosed(err)
		}},
		{"AssertTemplate", func() error {
			id, err := e.AssertTemplate("x", map[string]any{"slot": int64(1)})
			if id != 0 {
				return fmt.Errorf("id = %d, want 0", id)
			}
			return wantClosed(err)
		}},
		{"Retract", func() error { return wantClosed(e.Retract(1)) }},
		{"GetFact", func() error {
			value, err := e.GetFact(1)
			return wantNil("fact", value, err)
		}},
		{"Facts", func() error {
			value, err := e.Facts()
			return wantNil("facts", value, err)
		}},
		{"FindFacts", func() error {
			value, err := e.FindFacts("x")
			return wantNil("facts", value, err)
		}},
		{"FactCount", func() error {
			value, err := e.FactCount()
			if value != 0 {
				return fmt.Errorf("count = %d, want 0", value)
			}
			return wantClosed(err)
		}},
		{"Run", func() error {
			value, err := e.Run(context.Background())
			return wantNil("result", value, err)
		}},
		{"RunWithLimit", func() error {
			value, err := e.RunWithLimit(nil, 1) //nolint:staticcheck // Closed state intentionally takes precedence over nil-context validation.
			return wantNil("result", value, err)
		}},
		{"Step", func() error {
			value, err := e.Step()
			return wantNil("fired rule", value, err)
		}},
		{"Halt", func() error {
			e.Halt()
			return nil
		}},
		{"Reset", func() error { return wantClosed(e.Reset()) }},
		{"Clear", func() error {
			e.Clear()
			return nil
		}},
		{"Serialize", func() error {
			value, err := e.Serialize(FormatBincode)
			return wantNil("snapshot", value, err)
		}},
		{"SerializeToFile", func() error {
			return wantClosed(e.SerializeToFile(t.TempDir()+"/snapshot.bin", FormatBincode))
		}},
		{"Rules", func() error {
			if value := e.Rules(); value != nil {
				return fmt.Errorf("rules = %#v, want nil", value)
			}
			return nil
		}},
		{"Templates", func() error {
			if value := e.Templates(); value != nil {
				return fmt.Errorf("templates = %#v, want nil", value)
			}
			return nil
		}},
		{"GetGlobal", func() error {
			value, err := e.GetGlobal("x")
			return wantNil("global", value, err)
		}},
		{"CurrentModule", func() error {
			if value := e.CurrentModule(); value != "" {
				return fmt.Errorf("module = %q, want empty", value)
			}
			return nil
		}},
		{"Focus", func() error {
			name, ok := e.Focus()
			if name != "" || ok {
				return fmt.Errorf("focus = (%q, %v), want empty false", name, ok)
			}
			return nil
		}},
		{"FocusStack", func() error {
			if value := e.FocusStack(); value != nil {
				return fmt.Errorf("focus stack = %#v, want nil", value)
			}
			return nil
		}},
		{"AgendaSize", func() error {
			if value := e.AgendaSize(); value != 0 {
				return fmt.Errorf("agenda size = %d, want 0", value)
			}
			return nil
		}},
		{"IsHalted", func() error {
			if e.IsHalted() {
				return errors.New("IsHalted = true, want false")
			}
			return nil
		}},
		{"GetOutput", func() error {
			output, ok := e.GetOutput("stdout")
			if output != "" || ok {
				return fmt.Errorf("output = (%q, %v), want empty false", output, ok)
			}
			return nil
		}},
		{"GetOutputE", func() error {
			output, ok, err := e.GetOutputE("stdout")
			if output != "" || ok {
				return fmt.Errorf("output = (%q, %v), want empty false", output, ok)
			}
			return wantClosed(err)
		}},
		{"ClearOutput", func() error {
			e.ClearOutput("stdout")
			return nil
		}},
		{"ClearOutputE", func() error { return wantClosed(e.ClearOutputE("stdout")) }},
		{"PushInput", func() error {
			e.PushInput("line")
			return nil
		}},
		{"PushInputE", func() error { return wantClosed(e.PushInputE("line")) }},
		{"Diagnostics", func() error {
			if value := e.Diagnostics(); value != nil {
				return fmt.Errorf("diagnostics = %#v, want nil", value)
			}
			return nil
		}},
		{"ClearDiagnostics", func() error {
			e.ClearDiagnostics()
			return nil
		}},
		{"RulesE", func() error {
			value, err := e.RulesE()
			return wantNil("rules", value, err)
		}},
		{"TemplatesE", func() error {
			value, err := e.TemplatesE()
			return wantNil("templates", value, err)
		}},
		{"DiagnosticsE", func() error {
			value, err := e.DiagnosticsE()
			return wantNil("diagnostics", value, err)
		}},
		{"CurrentModuleE", func() error {
			value, err := e.CurrentModuleE()
			if value != "" {
				return fmt.Errorf("module = %q, want empty", value)
			}
			return wantClosed(err)
		}},
		{"FocusE", func() error {
			name, ok, err := e.FocusE()
			if name != "" || ok {
				return fmt.Errorf("focus = (%q, %v), want empty false", name, ok)
			}
			return wantClosed(err)
		}},
		{"FocusStackE", func() error {
			value, err := e.FocusStackE()
			return wantNil("focus stack", value, err)
		}},
		{"AgendaSizeE", func() error {
			value, err := e.AgendaSizeE()
			if value != 0 {
				return fmt.Errorf("agenda size = %d, want 0", value)
			}
			return wantClosed(err)
		}},
		{"IsHaltedE", func() error {
			value, err := e.IsHaltedE()
			if value {
				return errors.New("IsHaltedE = true, want false")
			}
			return wantClosed(err)
		}},
		{"FactIter", func() error {
			for value := range e.FactIter() {
				return fmt.Errorf("unexpected fact: %#v", value)
			}
			return nil
		}},
		{"RuleIter", func() error {
			for value := range e.RuleIter() {
				return fmt.Errorf("unexpected rule: %#v", value)
			}
			return nil
		}},
		{"TemplateIter", func() error {
			for value := range e.TemplateIter() {
				return fmt.Errorf("unexpected template: %q", value)
			}
			return nil
		}},
		{"DiagnosticIter", func() error {
			for value := range e.DiagnosticIter() {
				return fmt.Errorf("unexpected diagnostic: %q", value)
			}
			return nil
		}},
		{"FactIterE", func() error {
			count := 0
			for value, err := range e.FactIterE() {
				count++
				if !reflect.DeepEqual(value, Fact{}) {
					return fmt.Errorf("fact = %#v, want zero", value)
				}
				if err := wantClosed(err); err != nil {
					return err
				}
			}
			if count != 1 {
				return fmt.Errorf("yield count = %d, want 1", count)
			}
			return nil
		}},
		{"RuleIterE", func() error {
			count := 0
			for value, err := range e.RuleIterE() {
				count++
				if value != (RuleInfo{}) {
					return fmt.Errorf("rule = %#v, want zero", value)
				}
				if err := wantClosed(err); err != nil {
					return err
				}
			}
			if count != 1 {
				return fmt.Errorf("yield count = %d, want 1", count)
			}
			return nil
		}},
		{"TemplateIterE", func() error {
			count := 0
			for value, err := range e.TemplateIterE() {
				count++
				if value != "" {
					return fmt.Errorf("template = %q, want empty", value)
				}
				if err := wantClosed(err); err != nil {
					return err
				}
			}
			if count != 1 {
				return fmt.Errorf("yield count = %d, want 1", count)
			}
			return nil
		}},
		{"DiagnosticIterE", func() error {
			count := 0
			for value, err := range e.DiagnosticIterE() {
				count++
				if value != "" {
					return fmt.Errorf("diagnostic = %q, want empty", value)
				}
				if err := wantClosed(err); err != nil {
					return err
				}
			}
			if count != 1 {
				return fmt.Errorf("yield count = %d, want 1", count)
			}
			return nil
		}},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if err := test.call(); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestEngineCloseWaitsForInFlightFFI(t *testing.T) {
	withFFIHooks(t)

	entered := make(chan struct{})
	release := make(chan struct{})
	var inFlight atomic.Bool
	var freedDuringCall atomic.Bool

	ffiEngineFactCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		inFlight.Store(true)
		close(entered)
		<-release
		inFlight.Store(false)
		return 0, ffi.ErrOK
	}
	ffiEngineFree = func(ffi.EngineHandle) ffi.ErrorCode {
		if inFlight.Load() {
			freedDuringCall.Store(true)
		}
		return ffi.ErrOK
	}

	e := &Engine{}
	callDone := make(chan error, 1)
	go func() {
		_, err := e.FactCount()
		callDone <- err
	}()
	<-entered

	closeDone := make(chan error, 1)
	go func() {
		closeDone <- e.Close()
	}()
	close(release)

	if err := <-callDone; err != nil {
		t.Fatalf("FactCount: %v", err)
	}
	if err := <-closeDone; err != nil {
		t.Fatalf("Close: %v", err)
	}
	if freedDuringCall.Load() {
		t.Fatal("Close freed the native engine while FactCount was in flight")
	}
}

func TestEngineConcurrentCleanupFreesExactlyOnce(t *testing.T) {
	withFFIHooks(t)

	var freeCalls atomic.Int64
	free := func(ffi.EngineHandle) ffi.ErrorCode {
		freeCalls.Add(1)
		return ffi.ErrOK
	}
	ffiEngineFree = free
	ffiEngineFreeUnchecked = free

	e := &Engine{}
	const closers = 32
	errs := make(chan error, closers+1)
	var wg sync.WaitGroup
	wg.Add(closers + 1)
	for range closers {
		go func() {
			defer wg.Done()
			errs <- e.Close()
		}()
	}
	go func() {
		defer wg.Done()
		finalizeEngine(e)
		errs <- nil
	}()
	wg.Wait()
	close(errs)

	for err := range errs {
		if err != nil {
			t.Fatalf("cleanup: %v", err)
		}
	}
	if got := freeCalls.Load(); got != 1 {
		t.Fatalf("native free calls = %d, want 1", got)
	}
}

func TestEngineFailedCloseRemainsRetryable(t *testing.T) {
	withFFIHooks(t)

	freeCalls := 0
	ffiEngineFree = func(ffi.EngineHandle) ffi.ErrorCode {
		freeCalls++
		if freeCalls == 1 {
			return ffi.ErrThreadViolation
		}
		return ffi.ErrOK
	}
	factCalls := 0
	ffiEngineFactCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		factCalls++
		return 7, ffi.ErrOK
	}

	e := &Engine{}
	if err := e.Close(); !errors.Is(err, ErrThreadViolation) {
		t.Fatalf("first Close error = %v, want ErrThreadViolation", err)
	}
	if count, err := e.FactCount(); err != nil || count != 7 {
		t.Fatalf("FactCount after failed Close = (%d, %v), want (7, nil)", count, err)
	}
	if err := e.Close(); err != nil {
		t.Fatalf("retry Close: %v", err)
	}
	if freeCalls != 2 {
		t.Fatalf("native free calls = %d, want 2", freeCalls)
	}
	if factCalls != 1 {
		t.Fatalf("native fact-count calls = %d, want 1", factCalls)
	}
}

//nolint:funlen // Every engine FFI hook must fail independently if a closed method reaches it.
func forbidEngineOperationFFI(t *testing.T) {
	t.Helper()
	called := func(name string) {
		t.Helper()
		t.Errorf("post-close %s reached FFI", name)
	}

	ffiEngineLoadString = func(ffi.EngineHandle, string) ffi.ErrorCode {
		called("Load")
		return ffi.ErrOK
	}
	ffiEngineAssertString = func(ffi.EngineHandle, string) (uint64, ffi.ErrorCode) {
		called("AssertString")
		return 0, ffi.ErrOK
	}
	ffiEngineAssertOrdered = func(ffi.EngineHandle, string, []ffi.Value) (uint64, ffi.ErrorCode) {
		called("AssertFact")
		return 0, ffi.ErrOK
	}
	ffiEngineAssertTemplate = func(ffi.EngineHandle, string, []string, []ffi.Value) (uint64, ffi.ErrorCode) {
		called("AssertTemplate")
		return 0, ffi.ErrOK
	}
	ffiEngineRetract = func(ffi.EngineHandle, uint64) ffi.ErrorCode {
		called("Retract")
		return ffi.ErrOK
	}
	ffiEngineFactIDs = func(ffi.EngineHandle) ([]uint64, ffi.ErrorCode) {
		called("FactIDs")
		return nil, ffi.ErrOK
	}
	ffiEngineFindFactIDs = func(ffi.EngineHandle, string) ([]uint64, ffi.ErrorCode) {
		called("FindFactIDs")
		return nil, ffi.ErrOK
	}
	ffiEngineFactCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		called("FactCount")
		return 0, ffi.ErrOK
	}
	ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		called("Run")
		return 0, ffi.HaltReasonAgendaEmpty, ffi.ErrOK
	}
	ffiEngineStep = func(ffi.EngineHandle) (int32, ffi.ErrorCode) {
		called("Step")
		return 0, ffi.ErrOK
	}
	ffiEngineHalt = func(ffi.EngineHandle) ffi.ErrorCode {
		called("Halt")
		return ffi.ErrOK
	}
	ffiEngineReset = func(ffi.EngineHandle) ffi.ErrorCode {
		called("Reset")
		return ffi.ErrOK
	}
	ffiEngineClear = func(ffi.EngineHandle) ffi.ErrorCode {
		called("Clear")
		return ffi.ErrOK
	}
	ffiEngineSerializeAs = func(ffi.EngineHandle, ffi.SerializationFormat) ([]byte, ffi.ErrorCode) {
		called("Serialize")
		return nil, ffi.ErrOK
	}
	ffiEngineRuleCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		called("RuleCount")
		return 0, ffi.ErrOK
	}
	ffiEngineRuleInfo = func(ffi.EngineHandle, uintptr) (string, int32, ffi.ErrorCode) {
		called("RuleInfo")
		return "", 0, ffi.ErrOK
	}
	ffiEngineTemplateCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		called("TemplateCount")
		return 0, ffi.ErrOK
	}
	ffiEngineTemplateName = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
		called("TemplateName")
		return "", ffi.ErrOK
	}
	ffiEngineGetGlobal = func(ffi.EngineHandle, string) (ffi.Value, ffi.ErrorCode) {
		called("GetGlobal")
		return ffi.Value{}, ffi.ErrOK
	}
	ffiEngineCurrentModule = func(ffi.EngineHandle) (string, ffi.ErrorCode) {
		called("CurrentModule")
		return "", ffi.ErrOK
	}
	ffiEngineGetFocus = func(ffi.EngineHandle) (string, ffi.ErrorCode) {
		called("GetFocus")
		return "", ffi.ErrOK
	}
	ffiEngineFocusStackDepth = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		called("FocusStackDepth")
		return 0, ffi.ErrOK
	}
	ffiEngineFocusStackEntry = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
		called("FocusStackEntry")
		return "", ffi.ErrOK
	}
	ffiEngineAgendaCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		called("AgendaCount")
		return 0, ffi.ErrOK
	}
	ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) {
		called("IsHalted")
		return false, ffi.ErrOK
	}
	ffiEngineGetOutputCopy = func(ffi.EngineHandle, string) (string, bool, ffi.ErrorCode) {
		called("GetOutputCopy")
		return "", false, ffi.ErrOK
	}
	ffiEngineClearOutput = func(ffi.EngineHandle, string) ffi.ErrorCode {
		called("ClearOutput")
		return ffi.ErrOK
	}
	ffiEnginePushInput = func(ffi.EngineHandle, string) ffi.ErrorCode {
		called("PushInput")
		return ffi.ErrOK
	}
	ffiEngineActionDiagnosticCount = func(ffi.EngineHandle) (uintptr, ffi.ErrorCode) {
		called("DiagnosticCount")
		return 0, ffi.ErrOK
	}
	ffiEngineActionDiagnosticCopy = func(ffi.EngineHandle, uintptr) (string, ffi.ErrorCode) {
		called("DiagnosticCopy")
		return "", ffi.ErrOK
	}
	ffiEngineClearActionDiagnostics = func(ffi.EngineHandle) ffi.ErrorCode {
		called("ClearDiagnostics")
		return ffi.ErrOK
	}
	ffiEngineGetFactType = func(ffi.EngineHandle, uint64) (ffi.FactType, ffi.ErrorCode) {
		called("GetFactType")
		return ffi.FactTypeOrdered, ffi.ErrOK
	}
	ffiEngineGetFactFieldCount = func(ffi.EngineHandle, uint64) (uintptr, ffi.ErrorCode) {
		called("GetFactFieldCount")
		return 0, ffi.ErrOK
	}
	ffiEngineGetFactField = func(ffi.EngineHandle, uint64, uintptr) (ffi.Value, ffi.ErrorCode) {
		called("GetFactField")
		return ffi.Value{}, ffi.ErrOK
	}
	ffiEngineGetFactTemplateName = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
		called("GetFactTemplateName")
		return "", ffi.ErrOK
	}
	ffiEngineTemplateSlotCount = func(ffi.EngineHandle, string) (uintptr, ffi.ErrorCode) {
		called("TemplateSlotCount")
		return 0, ffi.ErrOK
	}
	ffiEngineTemplateSlotName = func(ffi.EngineHandle, string, uintptr) (string, ffi.ErrorCode) {
		called("TemplateSlotName")
		return "", ffi.ErrOK
	}
	ffiEngineGetFactRelation = func(ffi.EngineHandle, uint64) (string, ffi.ErrorCode) {
		called("GetFactRelation")
		return "", ffi.ErrOK
	}
}

package ferric

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"testing"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

type goLogicalRunObservation struct {
	result      RunResult
	diagnostics []string
	halted      bool
	agendaSize  int
}

func observeGoLogicalRun(t *testing.T, source string, cancelable bool) goLogicalRunObservation {
	t.Helper()
	return observeGoLogicalRunWithLimit(t, source, cancelable, 0)
}

func observeGoLogicalRunWithLimit(
	t *testing.T,
	source string,
	cancelable bool,
	limit int,
) goLogicalRunObservation {
	t.Helper()

	engine, err := NewEngine(WithSource(source))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, engine)

	ctx := context.Background()
	cancel := func() {}
	if cancelable {
		ctx, cancel = context.WithCancel(t.Context())
	}
	defer cancel()

	result, err := engine.RunWithLimit(ctx, limit)
	if err != nil {
		t.Fatal(err)
	}
	diagnostics, err := engine.DiagnosticsE()
	if err != nil {
		t.Fatal(err)
	}
	halted, err := engine.IsHaltedE()
	if err != nil {
		t.Fatal(err)
	}
	agendaSize, err := engine.AgendaSizeE()
	if err != nil {
		t.Fatal(err)
	}

	return goLogicalRunObservation{
		result:      *result,
		diagnostics: diagnostics,
		halted:      halted,
		agendaSize:  agendaSize,
	}
}

func haltAtActivationSource(boundary int) string {
	target := boundary - 1
	return fmt.Sprintf(`
		(deffacts start (position 0))
		(defrule halt-at-boundary
			(declare (salience 100))
			(position %d)
			=>
			(halt))
		(defrule advance
			?current <- (position ?n&:(< ?n %d))
			=>
			(retract ?current)
			(assert (position (+ ?n 1))))
		(defrule after-halt
			(declare (salience -100))
			?current <- (position %d)
			=>
			(retract ?current)
			(assert (past-boundary)))
	`, target, target, target)
}

func assertLogicalRunObservationsEqual(
	t *testing.T,
	got goLogicalRunObservation,
	want goLogicalRunObservation,
) {
	t.Helper()

	if got.result != want.result {
		t.Fatalf("run result = %+v, want %+v", got.result, want.result)
	}
	if !slices.Equal(got.diagnostics, want.diagnostics) {
		t.Fatalf("diagnostics = %q, want %q", got.diagnostics, want.diagnostics)
	}
	if got.halted != want.halted {
		t.Fatalf("halted = %v, want %v", got.halted, want.halted)
	}
	if got.agendaSize != want.agendaSize {
		t.Fatalf("agenda size = %d, want %d", got.agendaSize, want.agendaSize)
	}
}

func TestCancelableRunMatchesDirectAtHaltBoundaries(t *testing.T) {
	for _, boundary := range []int{1, 100, 101, 200} {
		t.Run(fmt.Sprintf("activation-%d", boundary), func(t *testing.T) {
			lockThread(t)
			source := haltAtActivationSource(boundary)
			direct := observeGoLogicalRun(t, source, false)
			cancelable := observeGoLogicalRun(t, source, true)

			want := RunResult{RulesFired: boundary, HaltReason: HaltRequested}
			if direct.result != want {
				t.Fatalf("direct run result = %+v, want %+v", direct.result, want)
			}
			if !direct.halted || direct.agendaSize != 1 {
				t.Fatalf(
					"direct terminal state = halted %v, agenda %d; want true/1",
					direct.halted,
					direct.agendaSize,
				)
			}
			assertLogicalRunObservationsEqual(t, cancelable, direct)
		})
	}
}

func TestRunLimitWinsAtExactHaltBoundary(t *testing.T) {
	lockThread(t)

	const boundary = 100
	source := haltAtActivationSource(boundary)
	direct := observeGoLogicalRunWithLimit(t, source, false, boundary)
	cancelable := observeGoLogicalRunWithLimit(t, source, true, boundary)

	want := RunResult{RulesFired: boundary, HaltReason: HaltLimitReached}
	if direct.result != want {
		t.Fatalf("direct run result = %+v, want %+v", direct.result, want)
	}
	if !direct.halted || direct.agendaSize != 1 {
		t.Fatalf(
			"direct terminal state = halted %v, agenda %d; want true/1",
			direct.halted,
			direct.agendaSize,
		)
	}
	assertLogicalRunObservationsEqual(t, cancelable, direct)
}

func TestCancelableRunPreservesDiagnosticsAcrossChunks(t *testing.T) {
	lockThread(t)

	const source = `
		(defrule seed
			(initial-fact)
			=>
			(assert (candidate 1))
			(assert (position 0)))
		(defrule bad-match
			(candidate ?value&:(/ 1 0))
			=>
			(assert (must-not-fire)))
		(defrule advance
			?current <- (position ?n&:(< ?n 100))
			=>
			(retract ?current)
			(assert (position (+ ?n 1))))
	`

	direct := observeGoLogicalRun(t, source, false)
	if direct.result != (RunResult{RulesFired: 101, HaltReason: HaltAgendaEmpty}) {
		t.Fatalf("direct run result = %+v, want 101/AgendaEmpty", direct.result)
	}
	if len(direct.diagnostics) == 0 {
		t.Fatal("diagnostic fixture did not emit an action diagnostic")
	}

	cancelable := observeGoLogicalRun(t, source, true)
	assertLogicalRunObservationsEqual(t, cancelable, direct)
}

func TestCancelableRunStartsOnceThenContinues(t *testing.T) {
	withFFIHooks(t)

	var calls []string
	ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		calls = append(calls, "run")
		return 100, ffi.HaltReasonLimitReached, ffi.ErrOK
	}
	ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) {
		calls = append(calls, "is-halted")
		return false, ffi.ErrOK
	}
	ffiEngineContinueRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		calls = append(calls, "continue")
		return 1, ffi.HaltReasonAgendaEmpty, ffi.ErrOK
	}

	result, err := (&Engine{}).Run(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if *result != (RunResult{RulesFired: 101, HaltReason: HaltAgendaEmpty}) {
		t.Fatalf("run result = %+v, want 101/AgendaEmpty", *result)
	}
	if !slices.Equal(calls, []string{"run", "is-halted", "continue"}) {
		t.Fatalf("FFI calls = %v, want one fresh chunk followed by one continuation", calls)
	}
}

func TestCancelableRunReportsCancellationBetweenChunks(t *testing.T) {
	withFFIHooks(t)

	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	haltChecks := 0
	ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		cancel()
		return 37, ffi.HaltReasonLimitReached, ffi.ErrOK
	}
	ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) {
		haltChecks++
		return false, ffi.ErrOK
	}
	ffiEngineContinueRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		t.Fatal("continuation called after context cancellation")
		return 0, ffi.HaltReasonAgendaEmpty, ffi.ErrInternalError
	}

	result, err := (&Engine{}).Run(ctx)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("run error = %v, want wrapped context.Canceled", err)
	}
	if result == nil || *result != (RunResult{RulesFired: 37, HaltReason: HaltRequested}) {
		t.Fatalf("partial run result = %+v, want 37/HaltRequested", result)
	}
	if haltChecks != 0 {
		t.Fatalf("halt state inspected %d times after cancellation, want 0", haltChecks)
	}
}

func TestCancelableRunExplicitLimitPrecedesCancellationAndInspection(t *testing.T) {
	withFFIHooks(t)

	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		cancel()
		return 23, ffi.HaltReasonLimitReached, ffi.ErrOK
	}
	ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) {
		t.Fatal("halt state inspected after the explicit caller limit was reached")
		return false, ffi.ErrInternalError
	}
	ffiEngineContinueRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		t.Fatal("continuation called after the explicit caller limit was reached")
		return 0, ffi.HaltReasonAgendaEmpty, ffi.ErrInternalError
	}

	result, err := (&Engine{}).RunWithLimit(ctx, 23)
	if err != nil {
		t.Fatalf("run error = %v, want nil", err)
	}
	if result == nil || *result != (RunResult{RulesFired: 23, HaltReason: HaltLimitReached}) {
		t.Fatalf("run result = %+v, want 23/LimitReached", result)
	}
}

func TestCancelableRunReturnsHaltInspectionError(t *testing.T) {
	withFFIHooks(t)

	ffiEngineRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		return 23, ffi.HaltReasonLimitReached, ffi.ErrOK
	}
	ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) {
		return false, ffi.ErrRuntimeError
	}
	ffiEngineContinueRunEx = func(ffi.EngineHandle, int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		t.Fatal("continuation called after halt-state inspection failed")
		return 0, ffi.HaltReasonAgendaEmpty, ffi.ErrInternalError
	}

	result, err := (&Engine{}).Run(t.Context())
	if !errors.Is(err, ErrRuntime) {
		t.Fatalf("run error = %v, want ErrRuntime", err)
	}
	if result == nil || result.RulesFired != 23 {
		t.Fatalf("partial run result = %+v, want 23 completed rules", result)
	}
}

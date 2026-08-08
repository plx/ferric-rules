package ferric

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

const pinnedInterruptTestTimeout = 2 * time.Second

const pinnedCyclingRules = `
	(defrule cycle
		?fact <- (counter ?value)
		=>
		(retract ?fact)
		(assert (counter (+ ?value 1))))
	(deffacts initial (counter 0))
`

type pinnedRunTestOutcome struct {
	result *RunResult
	err    error
}

func startPinnedTestRun(ctx context.Context, engine *PinnedEngine, limit int) <-chan pinnedRunTestOutcome {
	outcomes := make(chan pinnedRunTestOutcome, 1)
	go func() {
		result, err := engine.RunWithLimit(ctx, limit)
		outcomes <- pinnedRunTestOutcome{result: result, err: err}
		close(outcomes)
	}()
	return outcomes
}

func waitForPinnedActiveRun(t *testing.T, engine *PinnedEngine) {
	t.Helper()

	deadline := time.NewTimer(pinnedInterruptTestTimeout)
	defer deadline.Stop()
	ticker := time.NewTicker(time.Millisecond)
	defer ticker.Stop()
	for {
		if engine.activeRun.Load() != nil {
			return
		}
		select {
		case <-deadline.C:
			t.Fatal("pinned run did not become active")
		case <-ticker.C:
		}
	}
}

func waitForPinnedQueueDepth(t *testing.T, engine *PinnedEngine, depth int) {
	t.Helper()

	deadline := time.NewTimer(pinnedInterruptTestTimeout)
	defer deadline.Stop()
	ticker := time.NewTicker(time.Millisecond)
	defer ticker.Stop()
	for {
		if len(engine.requests) >= depth {
			return
		}
		select {
		case <-deadline.C:
			t.Fatalf("pinned request queue depth did not reach %d", depth)
		case <-ticker.C:
		}
	}
}

func receivePinnedRunOutcome(t *testing.T, outcomes <-chan pinnedRunTestOutcome) pinnedRunTestOutcome {
	t.Helper()
	select {
	case outcome := <-outcomes:
		return outcome
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("pinned run did not settle")
		return pinnedRunTestOutcome{}
	}
}

func newPinnedCyclingEngine(t *testing.T) *PinnedEngine {
	t.Helper()
	engine, err := NewPinnedEngine(WithSource(pinnedCyclingRules))
	if err != nil {
		t.Fatal(err)
	}
	if err := engine.Reset(); err != nil {
		_ = engine.Close()
		t.Fatal(err)
	}
	return engine
}

func TestPinnedEngineRunUsesBoundedLogicalContinuation(t *testing.T) {
	withFFIHooks(t)

	var calls []string
	ffiEngineRunEx = func(_ ffi.EngineHandle, limit int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		calls = append(calls, fmt.Sprintf("run:%d", limit))
		return uint64(runBatchSize), ffi.HaltReasonLimitReached, ffi.ErrOK
	}
	ffiEngineIsHalted = func(ffi.EngineHandle) (bool, ffi.ErrorCode) {
		calls = append(calls, "is-halted")
		return false, ffi.ErrOK
	}
	ffiEngineContinueRunEx = func(_ ffi.EngineHandle, limit int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		calls = append(calls, fmt.Sprintf("continue:%d", limit))
		return 1, ffi.HaltReasonAgendaEmpty, ffi.ErrOK
	}

	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, engine)

	result, err := engine.Run(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	want := RunResult{RulesFired: runBatchSize + 1, HaltReason: HaltAgendaEmpty}
	if result == nil || *result != want {
		t.Fatalf("run result = %+v, want %+v", result, want)
	}
	wantCalls := []string{
		fmt.Sprintf("run:%d", runBatchSize),
		"is-halted",
		fmt.Sprintf("continue:%d", runBatchSize),
	}
	if !slices.Equal(calls, wantCalls) {
		t.Fatalf("FFI calls = %v, want %v", calls, wantCalls)
	}
}

//nolint:funlen // Keep the interruption handshake and its timeout-safe cleanup together.
func TestPinnedEngineHaltInterruptsActiveUnlimitedRun(t *testing.T) {
	withFFIHooks(t)

	originalRunEx := ffiEngineRunEx
	enteredChunk := make(chan struct{})
	releaseChunk := make(chan struct{})
	var enterOnce sync.Once
	var releaseOnce sync.Once
	ffiEngineRunEx = func(handle ffi.EngineHandle, limit int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		enterOnce.Do(func() { close(enteredChunk) })
		<-releaseChunk
		return originalRunEx(handle, limit)
	}

	engine := newPinnedCyclingEngine(t)
	ctx, cancel := context.WithCancel(t.Context())
	t.Cleanup(func() {
		cancel()
		releaseOnce.Do(func() { close(releaseChunk) })
		_ = engine.Close()
	})
	outcomes := startPinnedTestRun(ctx, engine, 0)

	select {
	case <-enteredChunk:
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("unlimited run did not enter its first native chunk")
	}

	haltDone := make(chan struct{})
	go func() {
		engine.Halt()
		close(haltDone)
	}()
	select {
	case <-haltDone:
	case <-time.After(pinnedInterruptTestTimeout):
		cancel()
		releaseOnce.Do(func() { close(releaseChunk) })
		t.Fatal("Halt blocked behind the active unlimited run")
	}
	releaseOnce.Do(func() { close(releaseChunk) })

	outcome := receivePinnedRunOutcome(t, outcomes)
	if outcome.err != nil {
		t.Fatalf("halted run error = %v, want nil", outcome.err)
	}
	if outcome.result == nil || outcome.result.HaltReason != HaltRequested {
		t.Fatalf("halted run result = %+v, want partial HaltRequested", outcome.result)
	}
	if outcome.result.RulesFired == 0 || outcome.result.RulesFired > runBatchSize {
		t.Fatalf("halted run fired %d rules, want 1..%d", outcome.result.RulesFired, runBatchSize)
	}

	result, err := engine.RunWithLimit(context.Background(), 5)
	if err != nil {
		t.Fatalf("worker reuse run failed: %v", err)
	}
	want := RunResult{RulesFired: 5, HaltReason: HaltLimitReached}
	if result == nil || *result != want {
		t.Fatalf("worker reuse run result = %+v, want %+v", result, want)
	}
}

func TestPinnedEngineHaltWhileIdleDoesNotLatch(t *testing.T) {
	engine := newPinnedCyclingEngine(t)
	defer mustClose(t, engine)

	engine.Halt()
	result, err := engine.RunWithLimit(context.Background(), 5)
	if err != nil {
		t.Fatal(err)
	}
	want := RunResult{RulesFired: 5, HaltReason: HaltLimitReached}
	if result == nil || *result != want {
		t.Fatalf("run after idle Halt = %+v, want %+v", result, want)
	}
}

func TestPinnedEngineHaltDoesNotLatchOntoQueuedRun(t *testing.T) {
	engine := newPinnedCyclingEngine(t)
	releaseBlocker := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(releaseBlocker) })
		_ = engine.Close()
	})

	blockerEntered := make(chan struct{})
	blockerDone := make(chan error, 1)
	go func() {
		blockerDone <- engine.Do(context.Background(), func(*Engine) error {
			close(blockerEntered)
			<-releaseBlocker
			return nil
		})
	}()
	select {
	case <-blockerEntered:
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("blocking request did not start")
	}

	runCtx, cancelRun := context.WithCancel(t.Context())
	defer cancelRun()
	outcomes := startPinnedTestRun(runCtx, engine, 5)
	waitForPinnedQueueDepth(t, engine, 1)

	haltDone := make(chan struct{})
	go func() {
		engine.Halt()
		close(haltDone)
	}()
	select {
	case <-haltDone:
	case <-time.After(pinnedInterruptTestTimeout):
		cancelRun()
		releaseOnce.Do(func() { close(releaseBlocker) })
		t.Fatal("Halt blocked behind a non-Run request")
	}

	releaseOnce.Do(func() { close(releaseBlocker) })
	select {
	case err := <-blockerDone:
		if err != nil {
			t.Fatalf("blocking request failed: %v", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("blocking request did not finish")
	}
	outcome := receivePinnedRunOutcome(t, outcomes)
	if outcome.err != nil {
		t.Fatal(outcome.err)
	}
	want := RunResult{RulesFired: 5, HaltReason: HaltLimitReached}
	if outcome.result == nil || *outcome.result != want {
		t.Fatalf("queued run result = %+v, want %+v", outcome.result, want)
	}
}

//nolint:funlen // Keep the shutdown handshake and all terminal assertions together.
func TestPinnedEngineCloseInterruptsActiveUnlimitedRun(t *testing.T) {
	withFFIHooks(t)

	originalRunEx := ffiEngineRunEx
	enteredChunk := make(chan struct{})
	releaseChunk := make(chan struct{})
	var enterOnce sync.Once
	var releaseOnce sync.Once
	ffiEngineRunEx = func(handle ffi.EngineHandle, limit int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		enterOnce.Do(func() { close(enteredChunk) })
		<-releaseChunk
		return originalRunEx(handle, limit)
	}

	engine := newPinnedCyclingEngine(t)
	ctx, cancel := context.WithCancel(t.Context())
	t.Cleanup(func() {
		cancel()
		releaseOnce.Do(func() { close(releaseChunk) })
		_ = engine.Close()
	})
	outcomes := startPinnedTestRun(ctx, engine, 0)
	select {
	case <-enteredChunk:
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("unlimited run did not enter its first native chunk")
	}

	closeDone := make(chan error, 1)
	go func() { closeDone <- engine.Close() }()
	deadline := time.NewTimer(pinnedInterruptTestTimeout)
	defer deadline.Stop()
	for !engine.closed.Load() {
		select {
		case <-deadline.C:
			t.Fatal("Close did not publish closed state before waiting")
		default:
			time.Sleep(time.Millisecond)
		}
	}
	releaseOnce.Do(func() { close(releaseChunk) })

	outcome := receivePinnedRunOutcome(t, outcomes)
	if outcome.err != nil {
		t.Fatalf("run interrupted by Close error = %v, want nil", outcome.err)
	}
	if outcome.result == nil || outcome.result.HaltReason != HaltRequested {
		t.Fatalf("run interrupted by Close = %+v, want partial HaltRequested", outcome.result)
	}
	if outcome.result.RulesFired == 0 || outcome.result.RulesFired > runBatchSize {
		t.Fatalf("closed run fired %d rules, want 1..%d", outcome.result.RulesFired, runBatchSize)
	}
	select {
	case err := <-closeDone:
		if err != nil {
			t.Fatalf("Close failed: %v", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("Close did not finish after the active chunk returned")
	}

	result, err := engine.Run(context.Background())
	if result != nil || !errors.Is(err, errPinnedEngineClosed) {
		t.Fatalf("post-close run = (%+v, %v), want nil/errPinnedEngineClosed", result, err)
	}
}

//nolint:funlen // The staged queue verifies run cancellation and non-run draining together.
func TestPinnedEngineCloseInterruptsQueuedRunAndDrainsNonRunWork(t *testing.T) {
	engine := newPinnedCyclingEngine(t)
	releaseBlocker := make(chan struct{})
	var releaseOnce sync.Once
	runCtx, cancelRun := context.WithCancel(t.Context())
	t.Cleanup(func() {
		cancelRun()
		releaseOnce.Do(func() { close(releaseBlocker) })
		_ = engine.Close()
	})

	blockerEntered := make(chan struct{})
	blockerDone := make(chan error, 1)
	go func() {
		blockerDone <- engine.Do(context.Background(), func(*Engine) error {
			close(blockerEntered)
			<-releaseBlocker
			return nil
		})
	}()
	select {
	case <-blockerEntered:
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("blocking request did not start")
	}

	runOutcomes := startPinnedTestRun(runCtx, engine, 0)
	waitForPinnedQueueDepth(t, engine, 1)
	drainDone := make(chan error, 1)
	go func() {
		drainDone <- engine.Do(context.Background(), func(*Engine) error {
			return errTestSentinel
		})
	}()
	waitForPinnedQueueDepth(t, engine, 2)

	closeDone := make(chan error, 1)
	go func() { closeDone <- engine.Close() }()
	deadline := time.NewTimer(pinnedInterruptTestTimeout)
	defer deadline.Stop()
	for !engine.closed.Load() {
		select {
		case <-deadline.C:
			t.Fatal("Close did not publish closed state")
		default:
			time.Sleep(time.Millisecond)
		}
	}
	releaseOnce.Do(func() { close(releaseBlocker) })

	select {
	case err := <-blockerDone:
		if err != nil {
			t.Fatalf("blocking request failed: %v", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("blocking request did not finish")
	}
	runOutcome := receivePinnedRunOutcome(t, runOutcomes)
	wantRun := RunResult{RulesFired: 0, HaltReason: HaltRequested}
	if runOutcome.err != nil || runOutcome.result == nil || *runOutcome.result != wantRun {
		t.Fatalf("queued run outcome = (%+v, %v), want %+v/nil", runOutcome.result, runOutcome.err, wantRun)
	}
	select {
	case err := <-drainDone:
		if !errors.Is(err, errTestSentinel) {
			t.Fatalf("accepted non-Run result = %v, want sentinel", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("accepted non-Run work was not drained")
	}
	select {
	case err := <-closeDone:
		if err != nil {
			t.Fatalf("Close failed: %v", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("Close did not finish after draining accepted work")
	}
}

func TestPinnedEngineExplicitLimitWinsAtInterruptBoundary(t *testing.T) {
	withFFIHooks(t)

	enteredChunk := make(chan struct{})
	releaseChunk := make(chan struct{})
	var releaseOnce sync.Once
	ffiEngineRunEx = func(_ ffi.EngineHandle, _ int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		close(enteredChunk)
		<-releaseChunk
		return uint64(runBatchSize), ffi.HaltReasonLimitReached, ffi.ErrOK
	}

	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	t.Cleanup(func() {
		cancel()
		releaseOnce.Do(func() { close(releaseChunk) })
		_ = engine.Close()
	})
	outcomes := startPinnedTestRun(ctx, engine, runBatchSize)
	select {
	case <-enteredChunk:
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("limited run did not enter its native chunk")
	}
	engine.Halt()
	closeDone := make(chan error, 1)
	go func() { closeDone <- engine.Close() }()
	deadline := time.NewTimer(pinnedInterruptTestTimeout)
	defer deadline.Stop()
	for !engine.closed.Load() {
		select {
		case <-deadline.C:
			t.Fatal("Close did not publish closed state")
		default:
			time.Sleep(time.Millisecond)
		}
	}
	cancel()
	releaseOnce.Do(func() { close(releaseChunk) })

	outcome := receivePinnedRunOutcome(t, outcomes)
	want := RunResult{RulesFired: runBatchSize, HaltReason: HaltLimitReached}
	if outcome.err != nil || outcome.result == nil || *outcome.result != want {
		t.Fatalf("exact-boundary outcome = (%+v, %v), want %+v/nil", outcome.result, outcome.err, want)
	}
	select {
	case err := <-closeDone:
		if err != nil {
			t.Fatalf("Close failed: %v", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("Close did not settle after the explicit-limit run")
	}
}

func TestPinnedEngineContextWinsAtInterruptBoundary(t *testing.T) {
	withFFIHooks(t)

	enteredChunk := make(chan struct{})
	releaseChunk := make(chan struct{})
	var releaseOnce sync.Once
	ffiEngineRunEx = func(_ ffi.EngineHandle, _ int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
		close(enteredChunk)
		<-releaseChunk
		return 1, ffi.HaltReasonLimitReached, ffi.ErrOK
	}

	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	t.Cleanup(func() {
		cancel()
		releaseOnce.Do(func() { close(releaseChunk) })
		_ = engine.Close()
	})
	outcomes := startPinnedTestRun(ctx, engine, 0)
	select {
	case <-enteredChunk:
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("run did not enter its native chunk")
	}

	engine.Halt()
	closeDone := make(chan error, 1)
	go func() { closeDone <- engine.Close() }()
	deadline := time.NewTimer(pinnedInterruptTestTimeout)
	defer deadline.Stop()
	for !engine.closed.Load() {
		select {
		case <-deadline.C:
			t.Fatal("Close did not publish closed state")
		default:
			time.Sleep(time.Millisecond)
		}
	}
	cancel()
	releaseOnce.Do(func() { close(releaseChunk) })

	outcome := receivePinnedRunOutcome(t, outcomes)
	want := RunResult{RulesFired: 1, HaltReason: HaltRequested}
	if outcome.result == nil || *outcome.result != want || !errors.Is(outcome.err, context.Canceled) {
		t.Fatalf("context-priority outcome = (%+v, %v), want %+v/context.Canceled", outcome.result, outcome.err, want)
	}
	select {
	case err := <-closeDone:
		if err != nil {
			t.Fatalf("Close failed: %v", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("Close did not settle after context cancellation")
	}
}

func TestPinnedEngineAcceptedQueuedRunWaitsForCooperativeCancellation(t *testing.T) {
	engine := newPinnedCyclingEngine(t)
	releaseBlocker := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(releaseBlocker) })
		_ = engine.Close()
	})

	blockerEntered := make(chan struct{})
	blockerDone := make(chan error, 1)
	go func() {
		blockerDone <- engine.Do(context.Background(), func(*Engine) error {
			close(blockerEntered)
			<-releaseBlocker
			return nil
		})
	}()
	select {
	case <-blockerEntered:
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("blocking request did not start")
	}

	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	outcomes := startPinnedTestRun(ctx, engine, 0)
	waitForPinnedQueueDepth(t, engine, 1)
	cancel()

	select {
	case outcome := <-outcomes:
		t.Fatalf("accepted queued run returned before its worker response: (%+v, %v)", outcome.result, outcome.err)
	case <-time.After(25 * time.Millisecond):
	}

	releaseOnce.Do(func() { close(releaseBlocker) })
	select {
	case err := <-blockerDone:
		if err != nil {
			t.Fatalf("blocking request failed: %v", err)
		}
	case <-time.After(pinnedInterruptTestTimeout):
		t.Fatal("blocking request did not finish")
	}
	outcome := receivePinnedRunOutcome(t, outcomes)
	want := RunResult{RulesFired: 0, HaltReason: HaltRequested}
	if outcome.result == nil || *outcome.result != want || !errors.Is(outcome.err, context.Canceled) {
		t.Fatalf("queued cancellation outcome = (%+v, %v), want %+v/context.Canceled", outcome.result, outcome.err, want)
	}
}

//nolint:funlen // Each iteration owns a complete three-way race and cleanup barrier.
func TestPinnedEngineCompetingInterruptsSettleRunOnce(t *testing.T) {
	const iterations = 25
	for iteration := range iterations {
		t.Run(fmt.Sprintf("iteration-%d", iteration), func(t *testing.T) {
			withFFIHooks(t)

			enteredChunk := make(chan struct{})
			releaseChunk := make(chan struct{})
			ffiEngineRunEx = func(_ ffi.EngineHandle, _ int64) (uint64, ffi.HaltReason, ffi.ErrorCode) {
				close(enteredChunk)
				<-releaseChunk
				return 1, ffi.HaltReasonLimitReached, ffi.ErrOK
			}

			engine, err := NewPinnedEngine()
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithCancel(t.Context())
			t.Cleanup(func() {
				cancel()
				_ = engine.Close()
			})
			outcomes := startPinnedTestRun(ctx, engine, 0)
			select {
			case <-enteredChunk:
			case <-time.After(pinnedInterruptTestTimeout):
				t.Fatal("run did not enter native chunk")
			}

			start := make(chan struct{})
			haltDone := make(chan struct{})
			closeDone := make(chan error, 1)
			cancelDone := make(chan struct{})
			go func() {
				<-start
				engine.Halt()
				close(haltDone)
			}()
			go func() {
				<-start
				closeDone <- engine.Close()
			}()
			go func() {
				<-start
				cancel()
				close(cancelDone)
			}()
			close(start)
			<-haltDone
			<-cancelDone
			deadline := time.NewTimer(pinnedInterruptTestTimeout)
			for !engine.closed.Load() {
				select {
				case <-deadline.C:
					t.Fatal("competing Close did not publish closed state")
				default:
					time.Sleep(time.Millisecond)
				}
			}
			deadline.Stop()
			close(releaseChunk)

			outcome := receivePinnedRunOutcome(t, outcomes)
			if outcome.result == nil || outcome.result.HaltReason != HaltRequested {
				t.Fatalf("competing interrupt result = %+v, want HaltRequested", outcome.result)
			}
			if !errors.Is(outcome.err, context.Canceled) {
				t.Fatalf("competing interrupt error = %v, want context.Canceled", outcome.err)
			}
			if _, ok := <-outcomes; ok {
				t.Fatal("run produced more than one terminal outcome")
			}
			select {
			case err := <-closeDone:
				if err != nil {
					t.Fatalf("Close failed: %v", err)
				}
			case <-time.After(pinnedInterruptTestTimeout):
				t.Fatal("competing Close did not settle")
			}
		})
	}
}

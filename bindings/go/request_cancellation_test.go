package ferric

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

const requestCancellationTestTimeout = 2 * time.Second

var (
	errStartedCallbackResult = errors.New("started callback result")
	errClaimedCallbackResult = errors.New("claimed callback result")
)

type requestCancellationOutcome[T any] struct {
	value T
	err   error
}

func waitForRequestQueueLength[T any](t *testing.T, queue *requestQueue[T], want int) {
	t.Helper()

	deadline := time.NewTimer(requestCancellationTestTimeout)
	defer deadline.Stop()
	ticker := time.NewTicker(time.Millisecond)
	defer ticker.Stop()
	for {
		if queue.len() == want {
			return
		}
		select {
		case <-deadline.C:
			t.Fatalf("request queue length = %d, want %d", queue.len(), want)
		case <-ticker.C:
		}
	}
}

func receiveRequestCancellationOutcome[T any](
	t *testing.T,
	outcomes <-chan requestCancellationOutcome[T],
	label string,
) requestCancellationOutcome[T] {
	t.Helper()
	select {
	case outcome, ok := <-outcomes:
		if !ok {
			t.Fatalf("%s closed without an outcome", label)
		}
		return outcome
	case <-time.After(requestCancellationTestTimeout):
		t.Fatalf("timed out waiting for %s", label)
		var zero requestCancellationOutcome[T]
		return zero
	}
}

func assertRequestCancellationPending[T any](
	t *testing.T,
	outcomes <-chan requestCancellationOutcome[T],
	label string,
) {
	t.Helper()
	select {
	case outcome, ok := <-outcomes:
		if !ok {
			t.Fatalf("%s closed before the worker callback settled", label)
		}
		t.Fatalf("%s returned before the worker callback settled: (%v, %v)", label, outcome.value, outcome.err)
	case <-time.After(25 * time.Millisecond):
	}
}

func assertRequestCancellationOutcomeClosed[T any](
	t *testing.T,
	outcomes <-chan requestCancellationOutcome[T],
	label string,
) {
	t.Helper()
	select {
	case _, ok := <-outcomes:
		if ok {
			t.Fatalf("%s produced more than one outcome", label)
		}
	case <-time.After(requestCancellationTestTimeout):
		t.Fatalf("timed out waiting for %s channel to close", label)
	}
}

func TestRequestQueueCancellationReclaimsCapacity(t *testing.T) {
	queue := newRequestQueue[int](1)
	t.Cleanup(queue.close)
	first, err := queue.enqueue(context.Background(), 1)
	if err != nil {
		t.Fatal(err)
	}

	secondResult := make(chan requestCancellationOutcome[*queuedRequest[int]], 1)
	go func() {
		queued, enqueueErr := queue.enqueue(context.Background(), 2)
		secondResult <- requestCancellationOutcome[*queuedRequest[int]]{value: queued, err: enqueueErr}
		close(secondResult)
	}()
	assertRequestCancellationPending(t, secondResult, "enqueue into full request queue")

	if !first.cancel() {
		t.Fatal("canceling queued request reported worker ownership")
	}
	outcome := receiveRequestCancellationOutcome(t, secondResult, "replacement enqueue")
	if outcome.err != nil || outcome.value == nil {
		t.Fatalf("replacement enqueue = (%v, %v), want admitted request", outcome.value, outcome.err)
	}
	assertRequestCancellationOutcomeClosed(t, secondResult, "replacement enqueue")
	if got := queue.len(); got != 1 {
		t.Fatalf("queue length after capacity reclamation = %d, want 1", got)
	}
	dequeued := queue.dequeue()
	if !dequeued.ok || dequeued.value != 2 {
		t.Fatalf("dequeued replacement = (%d, %t), want (2, true)", dequeued.value, dequeued.ok)
	}
	if outcome.value.cancel() {
		t.Fatal("dequeued request remained cancelable")
	}
}

func TestCanceledBeforeAdmissionDoesNotRunCallbacks(t *testing.T) {
	t.Run("PinnedEngine", func(t *testing.T) {
		engine, err := NewPinnedEngine()
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = engine.Close() })

		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		var calls atomic.Int32
		err = engine.Do(ctx, func(*Engine) error {
			calls.Add(1)
			return nil
		})
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("pre-admission cancellation error = %v, want context.Canceled", err)
		}
		if calls.Load() != 0 || engine.requests.len() != 0 {
			t.Fatalf("pre-admission cancellation ran callback %d times with queue length %d", calls.Load(), engine.requests.len())
		}
	})

	t.Run("Manager", func(t *testing.T) {
		coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = coordinator.Close() })
		manager, err := coordinator.Manager("test")
		if err != nil {
			t.Fatal(err)
		}

		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		var calls atomic.Int32
		err = manager.Do(ctx, func(*Engine) error {
			calls.Add(1)
			return nil
		})
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("pre-admission cancellation error = %v, want context.Canceled", err)
		}
		if calls.Load() != 0 || coordinator.workers[0].requests.len() != 0 {
			t.Fatalf("pre-admission cancellation ran callback %d times with queue length %d", calls.Load(), coordinator.workers[0].requests.len())
		}
	})
}

//nolint:funlen // The staged blocker, cancellation, and successor form one lifecycle assertion.
func TestPinnedEngineQueuedCancellationRemovesCallback(t *testing.T) {
	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	releaseBlocker := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(releaseBlocker) })
		_ = engine.Close()
	})

	blockerStarted := make(chan struct{})
	blockerResult := make(chan error, 1)
	go func() {
		blockerResult <- engine.Do(context.Background(), func(*Engine) error {
			close(blockerStarted)
			<-releaseBlocker
			return nil
		})
	}()
	select {
	case <-blockerStarted:
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("pinned blocker did not start")
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	var canceledCalls atomic.Int32
	canceledResult := make(chan requestCancellationOutcome[struct{}], 1)
	go func() {
		err := engine.Do(ctx, func(callbackEngine *Engine) error {
			canceledCalls.Add(1)
			_, callbackErr := callbackEngine.AssertFact("must-not-run")
			return callbackErr
		})
		canceledResult <- requestCancellationOutcome[struct{}]{err: err}
		close(canceledResult)
	}()
	waitForRequestQueueLength(t, engine.requests, 1)
	cancel()

	outcome := receiveRequestCancellationOutcome(t, canceledResult, "queued pinned cancellation")
	if !errors.Is(outcome.err, context.Canceled) {
		t.Fatalf("queued pinned cancellation error = %v, want context.Canceled", outcome.err)
	}
	assertRequestCancellationOutcomeClosed(t, canceledResult, "queued pinned cancellation")
	waitForRequestQueueLength(t, engine.requests, 0)

	var successorFactCount atomic.Int64
	successorResult := make(chan error, 1)
	go func() {
		successorResult <- engine.Do(context.Background(), func(callbackEngine *Engine) error {
			count, countErr := callbackEngine.FactCount()
			if countErr != nil {
				return countErr
			}
			successorFactCount.Store(int64(count))
			return nil
		})
	}()
	waitForRequestQueueLength(t, engine.requests, 1)
	releaseOnce.Do(func() { close(releaseBlocker) })
	select {
	case err := <-blockerResult:
		if err != nil {
			t.Fatalf("pinned blocker failed: %v", err)
		}
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("pinned blocker did not settle")
	}
	select {
	case err := <-successorResult:
		if err != nil {
			t.Fatalf("pinned successor failed: %v", err)
		}
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("pinned successor did not settle")
	}
	if count := successorFactCount.Load(); count != 0 {
		t.Fatalf("fact count after canceled callback = %d, want 0", count)
	}
	if calls := canceledCalls.Load(); calls != 0 {
		t.Fatalf("queued canceled pinned callback ran %d times, want 0", calls)
	}
}

//nolint:funlen // The test keeps cancellation, no-mutation, and reuse barriers in one scenario.
func TestManagerQueuedEvaluateCancellationReturnsTypedZero(t *testing.T) {
	withFFIHooks(t)
	coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
	if err != nil {
		t.Fatal(err)
	}
	releaseBlocker := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(releaseBlocker) })
		_ = coordinator.Close()
	})
	manager, err := coordinator.Manager("test")
	if err != nil {
		t.Fatal(err)
	}
	worker := coordinator.workers[0]

	blockerStarted := make(chan struct{})
	blockerResult := make(chan error, 1)
	go func() {
		blockerResult <- manager.Do(context.Background(), func(*Engine) error {
			close(blockerStarted)
			<-releaseBlocker
			return nil
		})
	}()
	select {
	case <-blockerStarted:
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("manager blocker did not start")
	}

	originalReset := ffiEngineReset
	var resetCalls atomic.Int32
	ffiEngineReset = func(handle ffi.EngineHandle) ffi.ErrorCode {
		resetCalls.Add(1)
		return originalReset(handle)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	evaluateResult := make(chan requestCancellationOutcome[*EvaluateResult], 1)
	go func() {
		result, evaluateErr := manager.Evaluate(ctx, &EvaluateRequest{
			Facts: []WireFactInput{OrderedFact("must-not-run")},
		})
		evaluateResult <- requestCancellationOutcome[*EvaluateResult]{value: result, err: evaluateErr}
		close(evaluateResult)
	}()
	waitForRequestQueueLength(t, worker.requests, 1)
	cancel()

	outcome := receiveRequestCancellationOutcome(t, evaluateResult, "queued Evaluate cancellation")
	if outcome.value != nil || !errors.Is(outcome.err, context.Canceled) {
		t.Fatalf("queued Evaluate cancellation = (%+v, %v), want nil/context.Canceled", outcome.value, outcome.err)
	}
	assertRequestCancellationOutcomeClosed(t, evaluateResult, "queued Evaluate cancellation")
	waitForRequestQueueLength(t, worker.requests, 0)

	var successorFactCount atomic.Int64
	successorResult := make(chan error, 1)
	go func() {
		successorResult <- manager.Do(context.Background(), func(callbackEngine *Engine) error {
			count, countErr := callbackEngine.FactCount()
			if countErr != nil {
				return countErr
			}
			successorFactCount.Store(int64(count))
			return nil
		})
	}()
	waitForRequestQueueLength(t, worker.requests, 1)
	releaseOnce.Do(func() { close(releaseBlocker) })
	select {
	case err := <-blockerResult:
		if err != nil {
			t.Fatalf("manager blocker failed: %v", err)
		}
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("manager blocker did not settle")
	}
	select {
	case err := <-successorResult:
		if err != nil {
			t.Fatalf("manager successor failed: %v", err)
		}
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("manager successor did not settle")
	}
	if count := successorFactCount.Load(); count != 0 {
		t.Fatalf("fact count after canceled Evaluate = %d, want 0", count)
	}
	if calls := resetCalls.Load(); calls != 0 {
		t.Fatalf("queued canceled Evaluate entered engine Reset %d times, want 0", calls)
	}
}

func TestManagerStartedCancellationWaitsForCallbackResult(t *testing.T) {
	coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
	if err != nil {
		t.Fatal(err)
	}
	release := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(release) })
		_ = coordinator.Close()
	})
	manager, err := coordinator.Manager("test")
	if err != nil {
		t.Fatal(err)
	}
	worker := coordinator.workers[0]

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	started := make(chan struct{})
	var mutationComplete atomic.Bool
	outcomes := make(chan requestCancellationOutcome[struct{}], 1)
	go func() {
		err := manager.Do(ctx, func(*Engine) error {
			close(started)
			<-release
			mutationComplete.Store(true)
			return errStartedCallbackResult
		})
		outcomes <- requestCancellationOutcome[struct{}]{err: err}
		close(outcomes)
	}()
	select {
	case <-started:
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("manager callback did not start")
	}
	cancel()
	assertRequestCancellationPending(t, outcomes, "started manager cancellation")
	releaseOnce.Do(func() { close(release) })

	outcome := receiveRequestCancellationOutcome(t, outcomes, "started manager result")
	if !errors.Is(outcome.err, errStartedCallbackResult) {
		t.Fatalf("started manager cancellation error = %v, want callback error %v", outcome.err, errStartedCallbackResult)
	}
	assertRequestCancellationOutcomeClosed(t, outcomes, "started manager result")
	if !mutationComplete.Load() {
		t.Fatal("started Manager.Do returned before callback mutation completed")
	}
	if coordinator.workers[0] != worker {
		t.Fatal("started cancellation replaced the coordinator worker")
	}
	if err := manager.Do(context.Background(), func(callbackEngine *Engine) error {
		return callbackEngine.Reset()
	}); err != nil {
		t.Fatalf("manager worker was not reusable after started cancellation: %v", err)
	}
}

//nolint:funlen // The FFI barrier proves cancellation occurs during typed result assembly.
func TestManagerStartedEvaluateCancellationReturnsTypedResult(t *testing.T) {
	withFFIHooks(t)
	coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = coordinator.Close() })
	manager, err := coordinator.Manager("test")
	if err != nil {
		t.Fatal(err)
	}

	originalFactIDs := ffiEngineFactIDs
	resultAssemblyStarted := make(chan struct{})
	releaseResultAssembly := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() { releaseOnce.Do(func() { close(releaseResultAssembly) }) })
	var blockOnce sync.Once
	ffiEngineFactIDs = func(handle ffi.EngineHandle) ([]uint64, ffi.ErrorCode) {
		blockOnce.Do(func() {
			close(resultAssemblyStarted)
			<-releaseResultAssembly
		})
		return originalFactIDs(handle)
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	outcomes := make(chan requestCancellationOutcome[*EvaluateResult], 1)
	go func() {
		result, evaluateErr := manager.Evaluate(ctx, &EvaluateRequest{
			Facts: []WireFactInput{OrderedFact("typed-result", SymbolValue("kept"))},
		})
		outcomes <- requestCancellationOutcome[*EvaluateResult]{value: result, err: evaluateErr}
		close(outcomes)
	}()
	select {
	case <-resultAssemblyStarted:
	case <-time.After(requestCancellationTestTimeout):
		t.Fatal("Evaluate did not reach typed result assembly")
	}
	cancel()
	assertRequestCancellationPending(t, outcomes, "started typed Evaluate cancellation")
	releaseOnce.Do(func() { close(releaseResultAssembly) })

	outcome := receiveRequestCancellationOutcome(t, outcomes, "started typed Evaluate result")
	if outcome.err != nil || outcome.value == nil {
		t.Fatalf("started typed Evaluate result = (%+v, %v), want full result", outcome.value, outcome.err)
	}
	assertRequestCancellationOutcomeClosed(t, outcomes, "started typed Evaluate result")
	found := false
	for _, fact := range outcome.value.Facts {
		if fact.Kind == WireFactKindOrdered && fact.Ordered != nil && fact.Ordered.Relation == "typed-result" {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("started typed Evaluate facts = %+v, want typed-result fact", outcome.value.Facts)
	}
	if err := manager.Do(context.Background(), func(callbackEngine *Engine) error {
		return callbackEngine.Reset()
	}); err != nil {
		t.Fatalf("manager worker was not reusable after typed Evaluate cancellation: %v", err)
	}
}

//nolint:funlen // Each iteration owns the full cancel, claim, Close, and cleanup race.
func TestCoordinatorQueuedCancelClaimCloseRace(t *testing.T) {
	const iterations = 12

	for iteration := range iterations {
		t.Run(fmt.Sprintf("iteration_%d", iteration), func(t *testing.T) {
			coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
			if err != nil {
				t.Fatal(err)
			}
			manager, err := coordinator.Manager("test")
			if err != nil {
				t.Fatal(err)
			}

			releaseBlocker := make(chan struct{})
			var releaseOnce sync.Once
			t.Cleanup(func() {
				releaseOnce.Do(func() { close(releaseBlocker) })
				_ = coordinator.Close()
			})
			blockerStarted := make(chan struct{})
			blockerResult := make(chan error, 1)
			go func() {
				blockerResult <- manager.Do(context.Background(), func(*Engine) error {
					close(blockerStarted)
					<-releaseBlocker
					return nil
				})
			}()
			select {
			case <-blockerStarted:
			case <-time.After(requestCancellationTestTimeout):
				t.Fatal("race blocker did not start")
			}

			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			var callbackCalls atomic.Int32
			outcomes := make(chan requestCancellationOutcome[struct{}], 1)
			go func() {
				err := manager.Do(ctx, func(*Engine) error {
					callbackCalls.Add(1)
					return errClaimedCallbackResult
				})
				outcomes <- requestCancellationOutcome[struct{}]{err: err}
				close(outcomes)
			}()
			waitForRequestQueueLength(t, coordinator.workers[0].requests, 1)

			startRace := make(chan struct{})
			cancelDone := make(chan struct{})
			go func() {
				<-startRace
				cancel()
				close(cancelDone)
			}()
			releaseDone := make(chan struct{})
			go func() {
				<-startRace
				releaseOnce.Do(func() { close(releaseBlocker) })
				close(releaseDone)
			}()
			closeResult := make(chan error, 1)
			go func() {
				<-startRace
				closeResult <- coordinator.Close()
			}()
			close(startRace)
			<-cancelDone
			<-releaseDone

			outcome := receiveRequestCancellationOutcome(t, outcomes, "cancel/claim/Close race")
			assertRequestCancellationOutcomeClosed(t, outcomes, "cancel/claim/Close race")
			calls := callbackCalls.Load()
			switch {
			case errors.Is(outcome.err, context.Canceled):
				if calls != 0 {
					t.Fatalf("cancellation won but callback ran %d times", calls)
				}
			case errors.Is(outcome.err, errClaimedCallbackResult):
				if calls != 1 {
					t.Fatalf("worker claim won but callback ran %d times", calls)
				}
			default:
				t.Fatalf("cancel/claim/Close race error = %v, want context.Canceled or callback result", outcome.err)
			}
			select {
			case err := <-blockerResult:
				if err != nil {
					t.Fatalf("race blocker failed: %v", err)
				}
			case <-time.After(requestCancellationTestTimeout):
				t.Fatal("race blocker did not settle")
			}
			select {
			case err := <-closeResult:
				if err != nil {
					t.Fatalf("race Close failed: %v", err)
				}
			case <-time.After(requestCancellationTestTimeout):
				t.Fatal("race Close did not settle")
			}
		})
	}
}

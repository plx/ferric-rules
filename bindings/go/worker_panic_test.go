package ferric

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

const (
	workerPanicHelperEnv    = "FERRIC_GO_WORKER_PANIC_HELPER"
	workerPanicTestTimeout  = 2 * time.Second
	workerPanicChildTimeout = 8 * time.Second
)

type workerPanicPayload struct {
	message string
}

func (p *workerPanicPayload) String() string {
	return p.message
}

type hostileWorkerPanicError struct {
	formatCalls atomic.Int32
}

func (p *hostileWorkerPanicError) Error() string {
	p.panicDuringFormatting()
	return ""
}

func (p *hostileWorkerPanicError) String() string {
	p.panicDuringFormatting()
	return ""
}

func (p *hostileWorkerPanicError) Format(fmt.State, rune) {
	p.panicDuringFormatting()
}

func (p *hostileWorkerPanicError) panicDuringFormatting() {
	p.formatCalls.Add(1)
	panic("hostile panic payload formatting")
}

func TestPinnedEngineCallbackPanicRecovery(t *testing.T) {
	runWorkerPanicSubprocess(t, "pinned-recovery")
}

func TestCoordinatorCallbackPanicRecovery(t *testing.T) {
	runWorkerPanicSubprocess(t, "coordinator-recovery")
}

func TestPinnedEngineCallbackPanicRacingClose(t *testing.T) {
	runWorkerPanicSubprocess(t, "pinned-close")
}

func TestCoordinatorCallbackPanicRacingClose(t *testing.T) {
	runWorkerPanicSubprocess(t, "coordinator-close")
}

func TestPinnedEngineCanceledCallbackPanicDoesNotWedgeWorker(t *testing.T) {
	runWorkerPanicSubprocess(t, "pinned-cancel")
}

func TestCoordinatorCanceledCallbackPanicDoesNotWedgeWorker(t *testing.T) {
	runWorkerPanicSubprocess(t, "coordinator-cancel")
}

func TestCoordinatorHostileCallbackPanicPayload(t *testing.T) {
	runWorkerPanicSubprocess(t, "coordinator-hostile-payload")
}

func TestPinnedEngineNilCallbackPanic(t *testing.T) {
	goDebug := "panicnil=1"
	if inherited := os.Getenv("GODEBUG"); inherited != "" {
		goDebug = inherited + ",panicnil=1"
	}
	runWorkerPanicSubprocess(t, "pinned-nil-panic", "GODEBUG="+goDebug)
}

func TestPinnedEngineDrainCallbackPanicRecovery(t *testing.T) {
	runWorkerPanicSubprocess(t, "pinned-drain")
}

func TestWorkerPanicHelperProcess(t *testing.T) {
	mode := os.Getenv(workerPanicHelperEnv)
	if mode == "" {
		t.Skip("helper process")
	}

	switch mode {
	case "pinned-recovery":
		runPinnedPanicRecoveryScenario(t)
	case "coordinator-recovery":
		runCoordinatorPanicRecoveryScenario(t)
	case "pinned-close":
		runPinnedPanicCloseScenario(t)
	case "coordinator-close":
		runCoordinatorPanicCloseScenario(t)
	case "pinned-cancel":
		runPinnedPanicCancellationScenario(t)
	case "coordinator-cancel":
		runCoordinatorPanicCancellationScenario(t)
	case "coordinator-hostile-payload":
		runCoordinatorHostilePanicPayloadScenario(t)
	case "pinned-nil-panic":
		runPinnedNilPanicScenario(t)
	case "pinned-drain":
		runPinnedPanicDrainScenario(t)
	default:
		t.Fatalf("unknown worker panic helper mode %q", mode)
	}
}

func runWorkerPanicSubprocess(t *testing.T, mode string, extraEnv ...string) {
	t.Helper()

	ctx, cancel := context.WithTimeout(t.Context(), workerPanicChildTimeout)
	defer cancel()
	// The current test binary is a fixed executable, not caller-controlled input.
	cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestWorkerPanicHelperProcess$", "-test.v") //nolint:gosec
	cmd.Env = append(os.Environ(), workerPanicHelperEnv+"="+mode)
	cmd.Env = append(cmd.Env, extraEnv...)
	output, err := cmd.CombinedOutput()
	if ctx.Err() != nil {
		t.Fatalf("worker panic helper %q timed out: %v\n%s", mode, ctx.Err(), output)
	}
	if err != nil {
		t.Fatalf("worker panic helper %q failed: %v\n%s", mode, err, output)
	}
}

func runPinnedPanicRecoveryScenario(t *testing.T) {
	t.Helper()

	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = engine.Close() })

	payload := &workerPanicPayload{message: "pinned callback boom"}
	entered := make(chan struct{})
	release := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() { releaseOnce.Do(func() { close(release) }) })
	panicResult := make(chan error, 1)
	go func() {
		panicResult <- engine.Do(context.Background(), func(*Engine) error {
			close(entered)
			<-release
			panic(payload)
		})
	}()
	waitWorkerPanicSignal(t, entered, "pinned callback entry")

	successorResult := make(chan error, 1)
	go func() {
		successorResult <- engine.Do(context.Background(), func(callbackEngine *Engine) error {
			return callbackEngine.Reset()
		})
	}()
	waitWorkerPanicCondition(t, "queued pinned successor", func() bool {
		return engine.requests.len() == 1
	})
	releaseOnce.Do(func() { close(release) })

	panicErr := waitWorkerPanicError(t, panicResult, "pinned panic response")
	assertWorkerPanicError(t, panicErr, payload, "runPinnedPanicRecoveryScenario")
	if err := waitWorkerPanicError(t, successorResult, "queued pinned successor"); err != nil {
		t.Fatalf("queued successor failed after panic: %v", err)
	}
	select {
	case <-engine.done:
		t.Fatal("pinned worker exited after recovered callback panic")
	default:
	}

	assertRecoverableRuntimePanic(t, func(fn func(*Engine) error) error {
		return engine.Do(context.Background(), fn)
	})
	if err := engine.Do(context.Background(), func(callbackEngine *Engine) error {
		return callbackEngine.Reset()
	}); err != nil {
		t.Fatalf("pinned worker was not reusable after runtime panic: %v", err)
	}
}

func runCoordinatorPanicRecoveryScenario(t *testing.T) {
	t.Helper()

	coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = coordinator.Close() })
	manager, err := coordinator.Manager("test")
	if err != nil {
		t.Fatal(err)
	}
	workerBefore := coordinator.workers[0]

	payload := &workerPanicPayload{message: "coordinator callback boom"}
	entered := make(chan struct{})
	release := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() { releaseOnce.Do(func() { close(release) }) })
	panicResult := make(chan error, 1)
	go func() {
		panicResult <- manager.Do(context.Background(), func(*Engine) error {
			close(entered)
			<-release
			panic(payload)
		})
	}()
	waitWorkerPanicSignal(t, entered, "coordinator callback entry")

	successorResult := make(chan error, 1)
	go func() {
		successorResult <- manager.Do(context.Background(), func(callbackEngine *Engine) error {
			return callbackEngine.Reset()
		})
	}()
	waitWorkerPanicCondition(t, "queued coordinator successor", func() bool {
		return workerBefore.requests.len() == 1
	})
	releaseOnce.Do(func() { close(release) })

	panicErr := waitWorkerPanicError(t, panicResult, "coordinator panic response")
	assertWorkerPanicError(t, panicErr, payload, "runCoordinatorPanicRecoveryScenario")
	if err := waitWorkerPanicError(t, successorResult, "queued coordinator successor"); err != nil {
		t.Fatalf("queued successor failed after panic: %v", err)
	}
	assertCoordinatorWorkerAlive(t, coordinator, workerBefore)

	assertRecoverableRuntimePanic(t, func(fn func(*Engine) error) error {
		return manager.Do(context.Background(), fn)
	})
	if err := manager.Do(context.Background(), func(callbackEngine *Engine) error {
		return callbackEngine.Reset()
	}); err != nil {
		t.Fatalf("coordinator worker was not reusable after runtime panic: %v", err)
	}
}

func runPinnedPanicCloseScenario(t *testing.T) {
	t.Helper()

	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	payload := &workerPanicPayload{message: "pinned close boom"}
	entered := make(chan struct{})
	release := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(release) })
		_ = engine.Close()
	})
	panicResult := make(chan error, 1)
	go func() {
		panicResult <- engine.Do(context.Background(), func(*Engine) error {
			close(entered)
			<-release
			panic(payload)
		})
	}()
	waitWorkerPanicSignal(t, entered, "pinned close callback entry")

	closeResult := make(chan error, 1)
	go func() { closeResult <- engine.Close() }()
	waitWorkerPanicCondition(t, "pinned closed state", engine.closed.Load)
	select {
	case err := <-closeResult:
		t.Fatalf("Close returned before accepted callback settled: %v", err)
	default:
	}
	releaseOnce.Do(func() { close(release) })

	panicErr := waitWorkerPanicError(t, panicResult, "pinned close panic response")
	assertWorkerPanicError(t, panicErr, payload, "runPinnedPanicCloseScenario")
	if err := waitWorkerPanicError(t, closeResult, "pinned Close"); err != nil {
		t.Fatalf("Close failed after recovered callback panic: %v", err)
	}
}

func runCoordinatorPanicCloseScenario(t *testing.T) {
	t.Helper()

	coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
	if err != nil {
		t.Fatal(err)
	}
	manager, err := coordinator.Manager("test")
	if err != nil {
		t.Fatal(err)
	}
	payload := &workerPanicPayload{message: "coordinator close boom"}
	entered := make(chan struct{})
	release := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(release) })
		_ = coordinator.Close()
	})
	panicResult := make(chan error, 1)
	go func() {
		panicResult <- manager.Do(context.Background(), func(*Engine) error {
			close(entered)
			<-release
			panic(payload)
		})
	}()
	waitWorkerPanicSignal(t, entered, "coordinator close callback entry")

	closeResult := make(chan error, 1)
	go func() { closeResult <- coordinator.Close() }()
	waitWorkerPanicCondition(t, "coordinator closed state", coordinator.closed.Load)
	select {
	case err := <-closeResult:
		t.Fatalf("Close returned before accepted callback settled: %v", err)
	default:
	}
	releaseOnce.Do(func() { close(release) })

	panicErr := waitWorkerPanicError(t, panicResult, "coordinator close panic response")
	assertWorkerPanicError(t, panicErr, payload, "runCoordinatorPanicCloseScenario")
	if err := waitWorkerPanicError(t, closeResult, "coordinator Close"); err != nil {
		t.Fatalf("Close failed after recovered callback panic: %v", err)
	}
}

func runPinnedPanicCancellationScenario(t *testing.T) {
	t.Helper()

	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	release := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() {
		releaseOnce.Do(func() { close(release) })
		_ = engine.Close()
	})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	entered := make(chan struct{})
	payload := &workerPanicPayload{message: "pinned canceled callback boom"}
	panicResult := make(chan error, 1)
	go func() {
		defer close(panicResult)
		panicResult <- engine.Do(ctx, func(*Engine) error {
			close(entered)
			<-release
			panic(payload)
		})
	}()
	waitWorkerPanicSignal(t, entered, "pinned canceled callback entry")
	cancel()
	assertWorkerPanicPending(t, panicResult, "started pinned callback after cancellation")

	successorResult := make(chan error, 1)
	go func() {
		successorResult <- engine.Do(context.Background(), func(callbackEngine *Engine) error {
			return callbackEngine.Reset()
		})
	}()
	waitWorkerPanicCondition(t, "queued pinned successor after cancellation", func() bool {
		return engine.requests.len() == 1
	})
	releaseOnce.Do(func() { close(release) })
	panicErr := waitWorkerPanicError(t, panicResult, "pinned panic response after cancellation")
	assertWorkerPanicError(t, panicErr, payload, "runPinnedPanicCancellationScenario")
	assertWorkerPanicResultClosed(t, panicResult, "pinned panic response")
	if err := waitWorkerPanicError(t, successorResult, "pinned successor after cancellation"); err != nil {
		t.Fatalf("pinned worker was not reusable after canceled callback panic: %v", err)
	}
}

func runCoordinatorPanicCancellationScenario(t *testing.T) {
	t.Helper()

	coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = coordinator.Close() })
	manager, err := coordinator.Manager("test")
	if err != nil {
		t.Fatal(err)
	}
	workerBefore := coordinator.workers[0]
	release := make(chan struct{})
	var releaseOnce sync.Once
	t.Cleanup(func() { releaseOnce.Do(func() { close(release) }) })
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	entered := make(chan struct{})
	payload := &workerPanicPayload{message: "coordinator canceled callback boom"}
	panicResult := make(chan error, 1)
	go func() {
		defer close(panicResult)
		panicResult <- manager.Do(ctx, func(*Engine) error {
			close(entered)
			<-release
			panic(payload)
		})
	}()
	waitWorkerPanicSignal(t, entered, "coordinator canceled callback entry")
	cancel()
	assertWorkerPanicPending(t, panicResult, "started coordinator callback after cancellation")

	successorResult := make(chan error, 1)
	go func() {
		successorResult <- manager.Do(context.Background(), func(callbackEngine *Engine) error {
			return callbackEngine.Reset()
		})
	}()
	waitWorkerPanicCondition(t, "queued coordinator successor after cancellation", func() bool {
		return workerBefore.requests.len() == 1
	})
	releaseOnce.Do(func() { close(release) })
	panicErr := waitWorkerPanicError(t, panicResult, "coordinator panic response after cancellation")
	assertWorkerPanicError(t, panicErr, payload, "runCoordinatorPanicCancellationScenario")
	assertWorkerPanicResultClosed(t, panicResult, "coordinator panic response")
	if err := waitWorkerPanicError(t, successorResult, "coordinator successor after cancellation"); err != nil {
		t.Fatalf("coordinator worker was not reusable after canceled callback panic: %v", err)
	}
	assertCoordinatorWorkerAlive(t, coordinator, workerBefore)
}

func runCoordinatorHostilePanicPayloadScenario(t *testing.T) {
	t.Helper()

	coordinator, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(1))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = coordinator.Close() })
	manager, err := coordinator.Manager("test")
	if err != nil {
		t.Fatal(err)
	}
	workerBefore := coordinator.workers[0]
	payload := &hostileWorkerPanicError{}

	err = manager.Do(context.Background(), func(*Engine) error {
		panic(payload)
	})
	assertWorkerPanicError(t, err, payload, "runCoordinatorHostilePanicPayloadScenario")
	if calls := payload.formatCalls.Load(); calls != 0 {
		t.Fatalf("panic payload formatting methods called %d times, want 0", calls)
	}
	assertCoordinatorWorkerAlive(t, coordinator, workerBefore)
	if err := manager.Do(context.Background(), func(callbackEngine *Engine) error {
		return callbackEngine.Reset()
	}); err != nil {
		t.Fatalf("coordinator worker was not reusable after hostile panic payload: %v", err)
	}
}

func runPinnedNilPanicScenario(t *testing.T) {
	t.Helper()

	if !strings.Contains(os.Getenv("GODEBUG"), "panicnil=1") {
		t.Fatalf("nil-panic helper GODEBUG = %q, want panicnil=1", os.Getenv("GODEBUG"))
	}
	engine, err := NewPinnedEngine()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = engine.Close() })

	err = engine.Do(context.Background(), func(*Engine) error {
		panic(nil)
	})
	assertWorkerPanicError(t, err, nil, "runPinnedNilPanicScenario")
	select {
	case <-engine.done:
		t.Fatal("pinned worker exited after recovered nil callback panic")
	default:
	}
	if err := engine.Do(context.Background(), func(callbackEngine *Engine) error {
		return callbackEngine.Reset()
	}); err != nil {
		t.Fatalf("pinned worker was not reusable after nil callback panic: %v", err)
	}
}

//nolint:funlen // The closed-queue drain must cover the blocker, panic, successor, and Close barriers.
func runPinnedPanicDrainScenario(t *testing.T) {
	t.Helper()

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
	waitWorkerPanicSignal(t, blockerStarted, "pinned drain blocker entry")

	payload := &workerPanicPayload{message: "pinned drain callback boom"}
	panicResult := make(chan error, 1)
	successorResult := make(chan error, 1)
	go func() {
		panicResult <- engine.Do(context.Background(), func(*Engine) error {
			panic(payload)
		})
	}()
	waitWorkerPanicCondition(t, "queued pinned drain panic", func() bool {
		return engine.requests.len() == 1
	})
	go func() {
		successorResult <- engine.Do(context.Background(), func(callbackEngine *Engine) error {
			return callbackEngine.Reset()
		})
	}()
	waitWorkerPanicCondition(t, "queued pinned drain successor", func() bool {
		return engine.requests.len() == 2
	})

	closeResult := make(chan error, 1)
	go func() { closeResult <- engine.Close() }()
	waitWorkerPanicCondition(t, "pinned drain closed state", engine.closed.Load)
	releaseOnce.Do(func() { close(releaseBlocker) })
	if err := waitWorkerPanicError(t, blockerResult, "pinned drain blocker response"); err != nil {
		t.Fatalf("pinned drain blocker failed: %v", err)
	}
	assertWorkerPanicError(
		t,
		waitWorkerPanicError(t, panicResult, "pinned drain panic response"),
		payload,
		"runPinnedPanicDrainScenario",
	)
	if err := waitWorkerPanicError(t, successorResult, "pinned drain successor"); err != nil {
		t.Fatalf("pinned drain stopped after recovered callback panic: %v", err)
	}
	if err := waitWorkerPanicError(t, closeResult, "pinned drain Close"); err != nil {
		t.Fatalf("pinned drain Close failed: %v", err)
	}
}

func assertCoordinatorWorkerAlive(t *testing.T, coordinator *Coordinator, want *worker) {
	t.Helper()
	if len(coordinator.workers) != 1 || coordinator.workers[0] != want {
		t.Fatal("coordinator callback panic changed the worker pool")
	}
	select {
	case <-want.done:
		t.Fatal("coordinator worker exited after recovered callback panic")
	default:
	}
}

func assertRecoverableRuntimePanic(t *testing.T, invoke func(func(*Engine) error) error) {
	t.Helper()
	err := invoke(func(*Engine) error {
		values := []int{}
		_ = values[1] //nolint:gosec // Deliberately exercise recovery of a runtime bounds panic.
		return nil
	})
	var panicErr *PanicError
	if !errors.As(err, &panicErr) {
		t.Fatalf("recoverable runtime panic error = %T %v, want *PanicError", err, err)
	}
	if _, ok := panicErr.Value.(runtime.Error); !ok {
		t.Fatalf("recoverable runtime panic value = %T, want runtime.Error", panicErr.Value)
	}
	if !bytes.Contains(panicErr.Stack, []byte("assertRecoverableRuntimePanic")) {
		t.Fatalf("runtime panic stack does not contain callback frame:\n%s", panicErr.Stack)
	}
}

func assertWorkerPanicError(
	t *testing.T,
	err error,
	payload any,
	callbackFrame string,
) {
	t.Helper()
	var panicErr *PanicError
	if !errors.As(err, &panicErr) {
		t.Fatalf("callback error type = %T, want *PanicError", err)
	}
	if panicErr.Value != payload {
		t.Fatalf("panic payload type = %T, want identical value of type %T", panicErr.Value, payload)
	}
	if len(panicErr.Stack) == 0 {
		t.Fatal("panic stack is empty")
	}
	if !bytes.Contains(panicErr.Stack, []byte(callbackFrame)) {
		t.Fatalf("panic stack does not contain callback frame %q:\n%s", callbackFrame, panicErr.Stack)
	}
	wantText := fmt.Sprintf("ferric: worker callback panicked with %T", payload)
	if got := panicErr.Error(); got != wantText {
		t.Fatalf("PanicError text = %q, want %q", got, wantText)
	}
}

func waitWorkerPanicSignal(t *testing.T, signal <-chan struct{}, label string) {
	t.Helper()
	select {
	case <-signal:
	case <-time.After(workerPanicTestTimeout):
		t.Fatalf("timed out waiting for %s", label)
	}
}

func assertWorkerPanicPending(t *testing.T, result <-chan error, label string) {
	t.Helper()
	select {
	case err := <-result:
		t.Fatalf("%s settled before its callback completed: %v", label, err)
	case <-time.After(25 * time.Millisecond):
	}
}

func assertWorkerPanicResultClosed(t *testing.T, result <-chan error, label string) {
	t.Helper()
	select {
	case _, ok := <-result:
		if ok {
			t.Fatalf("%s produced more than one result", label)
		}
	case <-time.After(workerPanicTestTimeout):
		t.Fatalf("timed out waiting for %s channel to close", label)
	}
}

func waitWorkerPanicError(t *testing.T, result <-chan error, label string) error {
	t.Helper()
	select {
	case err := <-result:
		return err
	case <-time.After(workerPanicTestTimeout):
		t.Fatalf("timed out waiting for %s", label)
		return nil
	}
}

func waitWorkerPanicCondition(t *testing.T, label string, condition func() bool) {
	t.Helper()
	deadline := time.NewTimer(workerPanicTestTimeout)
	defer deadline.Stop()
	ticker := time.NewTicker(time.Millisecond)
	defer ticker.Stop()
	for {
		if condition() {
			return
		}
		select {
		case <-deadline.C:
			t.Fatalf("timed out waiting for %s", label)
		case <-ticker.C:
		}
	}
}

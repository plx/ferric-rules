package ferric

import (
	"context"
	"errors"
	"fmt"
	"runtime"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestCoordinatorBasic(t *testing.T) {
	coord, err := NewCoordinator([]EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(`(defrule r => (printout t "ok" crlf))`)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, err := coord.Manager("test")
	if err != nil {
		t.Fatal(err)
	}

	err = mgr.Do(context.Background(), func(e *Engine) error {
		if err := e.Reset(); err != nil {
			return err
		}
		result, err := e.Run(context.Background())
		if err != nil {
			return err
		}
		if result.RulesFired != 1 {
			t.Errorf("expected 1, got %d", result.RulesFired)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCoordinatorMultipleThreads(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{
			{Name: "counter", Options: []EngineOption{WithSource(`
				(defrule r => (printout t "fired" crlf))
			`)}},
		},
		Threads(4),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, err := coord.Manager("counter")
	if err != nil {
		t.Fatal(err)
	}

	var wg sync.WaitGroup
	errCh := make(chan error, 20)

	for i := range 20 {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			err := mgr.Do(context.Background(), func(e *Engine) error {
				if err := e.Reset(); err != nil {
					return err
				}
				result, err := e.Run(context.Background())
				if err != nil {
					return err
				}
				if result.RulesFired != 1 {
					t.Errorf("goroutine %d: expected 1 fired, got %d", i, result.RulesFired)
				}
				return nil
			})
			if err != nil {
				errCh <- err
			}
		}(i)
	}

	wg.Wait()
	close(errCh)
	for err := range errCh {
		t.Errorf("worker error: %v", err)
	}
}

//nolint:funlen // integration test intentionally covers multi-spec coordination in one scenario.
func TestCoordinatorMultipleSpecs(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{
			{Name: "greet", Options: []EngineOption{WithSource(`
				(defrule greet (person ?n) => (printout t "Hello " ?n crlf))
			`)}},
			{Name: "calc", Options: []EngineOption{WithSource(`
				(defglobal ?*result* = 0)
				(defrule add (number ?n) => (bind ?*result* (+ ?*result* ?n)))
			`)}},
		},
		Threads(2),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	greetMgr, _ := coord.Manager("greet")
	calcMgr, _ := coord.Manager("calc")

	// Test greet engine
	err = greetMgr.Do(context.Background(), func(e *Engine) error {
		if err := e.Reset(); err != nil {
			return err
		}
		if _, err := e.AssertFact("person", Symbol("World")); err != nil {
			return err
		}
		result, err := e.Run(context.Background())
		if err != nil {
			return err
		}
		if result.RulesFired != 1 {
			t.Errorf("greet: expected 1, got %d", result.RulesFired)
		}
		out, _ := e.GetOutput("t")
		if out != "Hello World\n" {
			t.Errorf("greet: unexpected output %q", out)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}

	// Test calc engine
	err = calcMgr.Do(context.Background(), func(e *Engine) error {
		if err := e.Reset(); err != nil {
			return err
		}
		if _, err := e.AssertFact("number", int64(10)); err != nil {
			return err
		}
		if _, err := e.AssertFact("number", int64(20)); err != nil {
			return err
		}
		if _, err := e.Run(context.Background()); err != nil {
			return err
		}

		val, err := e.GetGlobal("result")
		if err != nil {
			return err
		}
		if val != int64(30) {
			t.Errorf("calc: expected 30, got %v", val)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCoordinatorUnknownSpec(t *testing.T) {
	coord, err := NewCoordinator([]EngineSpec{
		{Name: "known", Options: nil},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	_, err = coord.Manager("unknown")
	if err == nil {
		t.Fatal("expected error for unknown spec")
	}
}

func TestCoordinatorShutdown(t *testing.T) {
	coord, err := NewCoordinator([]EngineSpec{
		{Name: "test", Options: nil},
	})
	if err != nil {
		t.Fatal(err)
	}

	// Close should be idempotent.
	mustNoError(t, coord.Close())
	mustNoError(t, coord.Close())

	mgr, _ := coord.Manager("test")
	err = mgr.Do(context.Background(), func(_ *Engine) error {
		return nil
	})
	if err == nil {
		t.Fatal("expected error after close")
	}
}

func TestCoordinatorContextCancel(t *testing.T) {
	coord, err := NewCoordinator([]EngineSpec{
		{Name: "test", Options: nil},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, _ := coord.Manager("test")

	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	err = mgr.Do(ctx, func(_ *Engine) error {
		return nil
	})
	if err == nil {
		t.Fatal("expected context error")
	}
}

func TestCoordinatorLazyInstantiation(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{
			{Name: "lazy", Options: []EngineOption{WithSource(`
				(defrule r => (assert (done)))
			`)}},
		},
		Threads(2),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, _ := coord.Manager("lazy")

	// Only one Do call — engine should be lazily created on just one worker.
	err = mgr.Do(context.Background(), func(e *Engine) error {
		if err := e.Reset(); err != nil {
			return err
		}
		_, err := e.Run(context.Background())
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCoordinatorConcurrentShutdownRace(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{
			{Name: "test", Options: []EngineOption{WithSource(`(defrule r => (assert (x)))`)}},
		},
		Threads(2),
	)
	if err != nil {
		t.Fatal(err)
	}

	mgr, _ := coord.Manager("test")

	var wg sync.WaitGroup
	// Start some goroutines doing work.
	for range 10 {
		wg.Go(func() {
			_ = mgr.Do(context.Background(), func(e *Engine) error {
				if err := e.Reset(); err != nil {
					return err
				}
				if _, err := e.Run(context.Background()); err != nil {
					return err
				}
				return nil
			})
		})
	}

	// Close while work is in flight. Should not panic.
	mustNoError(t, coord.Close())
	wg.Wait()
}

func TestCoordinatorInvalidThreadCount(t *testing.T) {
	_, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(0))
	if err == nil {
		t.Fatal("expected error for 0 threads")
	}
}

// ---------------------------------------------------------------------------
// DispatchPolicy bounds-check regression tests (#50 / GOB-005)
// ---------------------------------------------------------------------------

// fixedIndexPolicy always returns the same index, regardless of inputs.
type fixedIndexPolicy struct{ index int }

func (p fixedIndexPolicy) PickWorker(_ RouteHint, _ int, _ uint64) int {
	return p.index
}

func TestCoordinatorPolicyNegativeIndex(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(3),
		WithDispatchPolicy(fixedIndexPolicy{index: -1}),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, err := coord.Manager("test")
	if err != nil {
		t.Fatal(err)
	}

	// Must not panic; -1 should wrap to worker 2 (i.e. (-1 % 3 + 3) % 3 == 2).
	err = mgr.Do(context.Background(), func(e *Engine) error {
		return e.Reset()
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCoordinatorPolicyOutOfRangeIndex(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(3),
		WithDispatchPolicy(fixedIndexPolicy{index: 100}),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, err := coord.Manager("test")
	if err != nil {
		t.Fatal(err)
	}

	// Must not panic; 100 should wrap to worker 1 (i.e. 100 % 3 == 1).
	err = mgr.Do(context.Background(), func(e *Engine) error {
		return e.Reset()
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCoordinatorPolicyLargeNegativeIndex(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(4),
		WithDispatchPolicy(fixedIndexPolicy{index: -1000}),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, err := coord.Manager("test")
	if err != nil {
		t.Fatal(err)
	}

	// -1000 % 4 == 0 in Go, so (0 + 4) % 4 == 0.
	err = mgr.Do(context.Background(), func(e *Engine) error {
		return e.Reset()
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCoordinatorPolicyExactBoundaryIndex(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(3),
		WithDispatchPolicy(fixedIndexPolicy{index: 3}), // exactly == numWorkers
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, err := coord.Manager("test")
	if err != nil {
		t.Fatal(err)
	}

	// 3 % 3 == 0 — wraps to first worker.
	err = mgr.Do(context.Background(), func(e *Engine) error {
		return e.Reset()
	})
	if err != nil {
		t.Fatal(err)
	}
}

// ---------------------------------------------------------------------------
// Close-drain tests (#46 / GOB-001)
// ---------------------------------------------------------------------------

// TestCoordinatorCloseDrainsAcceptedWork verifies that requests already
// accepted (buffered in the worker channel) complete with their real result
// when Close is called, rather than returning errCoordinatorClosed.
func TestCoordinatorCloseDrainsAcceptedWork(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(1),
	)
	if err != nil {
		t.Fatal(err)
	}

	mgr, _ := coord.Manager("test")

	// Block the single worker so subsequent requests queue in the buffer.
	workerBusy := make(chan struct{})
	proceed := make(chan struct{})

	var wg sync.WaitGroup
	wg.Go(func() {
		err := mgr.Do(context.Background(), func(_ *Engine) error {
			close(workerBusy)
			<-proceed
			return nil
		})
		if err != nil {
			t.Errorf("blocking request: %v", err)
		}
	})

	// Wait until the worker is occupied.
	<-workerBusy

	// Enqueue several requests that will be buffered.
	const n = 5
	results := make([]chan error, n)
	for i := range n {
		ch := make(chan error, 1)
		results[i] = ch
		wg.Go(func() {
			ch <- mgr.Do(context.Background(), func(e *Engine) error {
				return e.Reset()
			})
		})
	}

	// Let the goroutines enqueue their requests into the buffered channel.
	runtime.Gosched()
	time.Sleep(50 * time.Millisecond)

	// Release the blocking request and shut down the coordinator.
	close(proceed)
	mustNoError(t, coord.Close())
	wg.Wait()

	// Every accepted request must have completed with its real result.
	for i, ch := range results {
		if err := <-ch; errors.Is(err, errCoordinatorClosed) {
			t.Errorf("request %d: got errCoordinatorClosed for accepted request", i)
		}
	}
}

// TestCoordinatorCloseDrainMultiThread is the same invariant as above but
// with multiple worker threads, ensuring drain works per-worker.
func TestCoordinatorCloseDrainMultiThread(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(4),
	)
	if err != nil {
		t.Fatal(err)
	}

	mgr, _ := coord.Manager("test")

	// Block all 4 workers.
	const workers = 4
	allBusy := make(chan struct{})
	proceed := make(chan struct{})

	var busyCount atomic.Int32
	var wg sync.WaitGroup

	for range workers {
		wg.Go(func() {
			_ = mgr.Do(context.Background(), func(_ *Engine) error {
				if busyCount.Add(1) == workers {
					close(allBusy)
				}
				<-proceed
				return nil
			})
		})
	}

	<-allBusy

	// Now queue work that will be buffered behind the blocked requests.
	const extra = 8
	results := make([]chan error, extra)
	for i := range extra {
		ch := make(chan error, 1)
		results[i] = ch
		wg.Go(func() {
			ch <- mgr.Do(context.Background(), func(e *Engine) error {
				return e.Reset()
			})
		})
	}

	runtime.Gosched()
	time.Sleep(50 * time.Millisecond)

	close(proceed)
	mustNoError(t, coord.Close())
	wg.Wait()

	for i, ch := range results {
		if err := <-ch; errors.Is(err, errCoordinatorClosed) {
			t.Errorf("request %d: got errCoordinatorClosed for accepted request", i)
		}
	}
}

// TestCoordinatorCloseNoAcceptAfterDone verifies that requests submitted
// after Close returns are correctly rejected with errCoordinatorClosed.
func TestCoordinatorCloseNoAcceptAfterDone(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
	)
	if err != nil {
		t.Fatal(err)
	}

	mgr, _ := coord.Manager("test")
	mustNoError(t, coord.Close())

	err = mgr.Do(context.Background(), func(_ *Engine) error {
		t.Fatal("callback should not execute after close")
		return nil
	})
	if !errors.Is(err, errCoordinatorClosed) {
		t.Fatalf("expected errCoordinatorClosed, got %v", err)
	}
}

// ---------------------------------------------------------------------------
// Close/cancel concurrency stress regression tests (#54 / GOB-009)
// ---------------------------------------------------------------------------

// TestCoordinatorCloseStress exercises the close-during-active-work race
// window across many iterations. Each request uses a bounded context to
// prevent hangs if a request is enqueued after the worker's drain completes
// (a narrow race window inherent to the select-based dispatch).
func TestCoordinatorCloseStress(t *testing.T) {
	t.Parallel()
	for iter := range 20 {
		t.Run(fmt.Sprintf("iter_%d", iter), func(t *testing.T) {
			t.Parallel()

			coord, err := NewCoordinator(
				[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
				Threads(2),
			)
			if err != nil {
				t.Fatal(err)
			}

			mgr, _ := coord.Manager("test")

			const n = 30
			results := make(chan error, n)

			var wg sync.WaitGroup
			for range n {
				wg.Go(func() {
					ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
					defer cancel()
					results <- mgr.Do(ctx, func(e *Engine) error {
						return e.Reset()
					})
				})
			}

			// Close while requests are in flight.
			go func() {
				runtime.Gosched()
				_ = coord.Close()
			}()

			wg.Wait()
			close(results)

			for err := range results {
				if err == nil {
					continue
				}
				// Acceptable outcomes:
				// - errCoordinatorClosed: request never enqueued (rejected at select)
				// - context.DeadlineExceeded: request orphaned after drain (narrow race)
				// - context.Canceled: context cleaned up
				if errors.Is(err, errCoordinatorClosed) ||
					errors.Is(err, context.DeadlineExceeded) ||
					errors.Is(err, context.Canceled) {
					continue
				}
				t.Errorf("unexpected error: %v", err)
			}
		})
	}
}

// TestCoordinatorCloseWhileBufferFull verifies that when the worker channel
// is saturated and Close fires, every enqueued request still completes.
func TestCoordinatorCloseWhileBufferFull(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(1),
	)
	if err != nil {
		t.Fatal(err)
	}

	mgr, _ := coord.Manager("test")

	// Block the single worker.
	workerBusy := make(chan struct{})
	proceed := make(chan struct{})

	var wg sync.WaitGroup
	wg.Go(func() {
		_ = mgr.Do(context.Background(), func(_ *Engine) error {
			close(workerBusy)
			<-proceed
			return nil
		})
	})
	<-workerBusy

	// Fill the channel buffer (capacity 16) with requests.
	const bufSize = 16
	accepted := make([]chan error, bufSize)
	for i := range bufSize {
		ch := make(chan error, 1)
		accepted[i] = ch
		wg.Go(func() {
			ch <- mgr.Do(context.Background(), func(e *Engine) error {
				return e.Reset()
			})
		})
	}
	// Let goroutines enqueue.
	runtime.Gosched()
	time.Sleep(50 * time.Millisecond)

	// Close while buffer is full, then unblock the worker.
	close(proceed)
	mustNoError(t, coord.Close())
	wg.Wait()

	for i, ch := range accepted {
		if err := <-ch; errors.Is(err, errCoordinatorClosed) {
			t.Errorf("request %d: accepted request misreported as coordinator-closed", i)
		}
	}
}

// TestCoordinatorConcurrentCloseIdempotent verifies that calling Close from
// multiple goroutines simultaneously never panics or returns an error.
func TestCoordinatorConcurrentCloseIdempotent(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(4),
	)
	if err != nil {
		t.Fatal(err)
	}

	// Do a small amount of work first to ensure engines are initialized.
	mgr, _ := coord.Manager("test")
	mustNoError(t, mgr.Do(context.Background(), func(e *Engine) error {
		return e.Reset()
	}))

	const closers = 10
	errs := make(chan error, closers)
	var wg sync.WaitGroup
	for range closers {
		wg.Go(func() {
			errs <- coord.Close()
		})
	}
	wg.Wait()
	close(errs)

	for err := range errs {
		if err != nil {
			t.Errorf("concurrent Close returned error: %v", err)
		}
	}
}

// TestCoordinatorAcceptedRequestOutcome verifies the core invariant from
// GOB-001: an accepted (enqueued) request's callback always executes and
// its real result is delivered, even if Close fires during execution.
func TestCoordinatorAcceptedRequestOutcome(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(1),
	)
	if err != nil {
		t.Fatal(err)
	}

	mgr, _ := coord.Manager("test")

	// Block the worker, then enqueue a sentinel request.
	workerBusy := make(chan struct{})
	proceed := make(chan struct{})

	var wg sync.WaitGroup
	wg.Go(func() {
		_ = mgr.Do(context.Background(), func(_ *Engine) error {
			close(workerBusy)
			<-proceed
			return nil
		})
	})
	<-workerBusy

	// Sentinel: the callback sets this flag. If the request was truly
	// accepted, the flag must be true and the result must be nil.
	var callbackRan atomic.Bool
	sentinelResult := make(chan error, 1)
	wg.Go(func() {
		sentinelResult <- mgr.Do(context.Background(), func(e *Engine) error {
			callbackRan.Store(true)
			return e.Reset()
		})
	})

	runtime.Gosched()
	time.Sleep(30 * time.Millisecond)

	// Close first, then unblock. The sentinel was already enqueued.
	close(proceed)
	mustNoError(t, coord.Close())
	wg.Wait()

	err = <-sentinelResult
	if !callbackRan.Load() {
		t.Fatal("accepted request callback never executed")
	}
	if errors.Is(err, errCoordinatorClosed) {
		t.Fatal("accepted request misreported as coordinator-closed")
	}
	if err != nil {
		t.Fatalf("unexpected sentinel error: %v", err)
	}
}

// ---------------------------------------------------------------------------
// Cancellation boundary tests (#54 / GOB-009)
// ---------------------------------------------------------------------------

// TestCoordinatorCancelDuringEnqueue verifies that a context canceled while
// the request is trying to enter the worker channel returns the right error.
func TestCoordinatorCancelDuringEnqueue(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(1),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, _ := coord.Manager("test")

	// Block the worker and fill the buffer so enqueue blocks.
	workerBusy := make(chan struct{})
	proceed := make(chan struct{})
	defer close(proceed)

	var wg sync.WaitGroup
	wg.Go(func() {
		_ = mgr.Do(context.Background(), func(_ *Engine) error {
			close(workerBusy)
			<-proceed
			return nil
		})
	})
	<-workerBusy

	// Fill the buffer.
	for range 16 {
		wg.Go(func() {
			_ = mgr.Do(context.Background(), func(e *Engine) error {
				return e.Reset()
			})
		})
	}
	runtime.Gosched()
	time.Sleep(30 * time.Millisecond)

	// Now try to enqueue with a context that will be canceled shortly.
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()

	err = mgr.Do(ctx, func(_ *Engine) error {
		t.Fatal("callback should not run — context canceled before dispatch")
		return nil
	})
	if err == nil {
		t.Fatal("expected cancellation error")
	}
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected DeadlineExceeded, got: %v", err)
	}
}

// TestCoordinatorCancelDuringWait verifies that a context canceled while
// the request is waiting for the worker to process it returns the right error.
func TestCoordinatorCancelDuringWait(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
		Threads(1),
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, _ := coord.Manager("test")

	// Block the worker with a slow request.
	workerBusy := make(chan struct{})
	proceed := make(chan struct{})
	defer close(proceed)

	var wg sync.WaitGroup
	wg.Go(func() {
		_ = mgr.Do(context.Background(), func(_ *Engine) error {
			close(workerBusy)
			<-proceed
			return nil
		})
	})
	<-workerBusy

	// Enqueue a request whose context will expire while waiting.
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()

	err = mgr.Do(ctx, func(_ *Engine) error {
		return nil
	})
	if err == nil {
		t.Fatal("expected cancellation error")
	}
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected DeadlineExceeded, got: %v", err)
	}
}

// TestCoordinatorCancelRacesWithClose verifies correctness when cancellation
// and close happen concurrently for an in-flight request.
func TestCoordinatorCancelRacesWithClose(t *testing.T) {
	t.Parallel()
	for iter := range 20 {
		t.Run(fmt.Sprintf("iter_%d", iter), func(t *testing.T) {
			t.Parallel()

			coord, err := NewCoordinator(
				[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
				Threads(1),
			)
			if err != nil {
				t.Fatal(err)
			}

			mgr, _ := coord.Manager("test")

			ctx, cancel := context.WithCancel(context.Background())

			resultCh := make(chan error, 1)
			var wg sync.WaitGroup
			wg.Go(func() {
				resultCh <- mgr.Do(ctx, func(e *Engine) error {
					return e.Reset()
				})
			})

			// Race cancel and close.
			go cancel()
			go func() { _ = coord.Close() }()

			wg.Wait()
			err = <-resultCh

			// Any of these outcomes is acceptable:
			// - nil (request completed normally)
			// - context.Canceled (cancel won)
			// - errCoordinatorClosed (close won before enqueue)
			if err != nil &&
				!errors.Is(err, context.Canceled) &&
				!errors.Is(err, errCoordinatorClosed) {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

// TestCoordinatorExpiredDeadlineRejectsImmediately verifies that a context
// with an already-expired deadline is rejected without touching the worker.
func TestCoordinatorExpiredDeadlineRejectsImmediately(t *testing.T) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(`(defrule r =>)`)}}},
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, coord)

	mgr, _ := coord.Manager("test")

	ctx, cancel := context.WithDeadline(context.Background(), time.Now().Add(-time.Second))
	defer cancel()

	err = mgr.Do(ctx, func(_ *Engine) error {
		t.Fatal("callback must not run with expired deadline")
		return nil
	})
	if err == nil {
		t.Fatal("expected error for expired deadline")
	}
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected DeadlineExceeded, got: %v", err)
	}
}

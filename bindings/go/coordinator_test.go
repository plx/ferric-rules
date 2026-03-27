package ferric

import (
	"context"
	"errors"
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

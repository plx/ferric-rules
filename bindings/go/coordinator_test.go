package ferric

import (
	"context"
	"sync"
	"testing"
)

func TestCoordinatorBasic(t *testing.T) {
	coord, err := NewCoordinator([]EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(`(defrule r => (printout t "ok" crlf))`)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer coord.Close()

	mgr, err := coord.Manager("test")
	if err != nil {
		t.Fatal(err)
	}

	err = mgr.Do(context.Background(), func(e *Engine) error {
		e.Reset()
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
	defer coord.Close()

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
				e.Reset()
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
	defer coord.Close()

	greetMgr, _ := coord.Manager("greet")
	calcMgr, _ := coord.Manager("calc")

	// Test greet engine
	err = greetMgr.Do(context.Background(), func(e *Engine) error {
		e.Reset()
		e.AssertFact("person", Symbol("World"))
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
		e.Reset()
		e.AssertFact("number", int64(10))
		e.AssertFact("number", int64(20))
		e.Run(context.Background())

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
	defer coord.Close()

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
	coord.Close()
	coord.Close()

	mgr, _ := coord.Manager("test")
	err = mgr.Do(context.Background(), func(e *Engine) error {
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
	defer coord.Close()

	mgr, _ := coord.Manager("test")

	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	err = mgr.Do(ctx, func(e *Engine) error {
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
	defer coord.Close()

	mgr, _ := coord.Manager("lazy")

	// Only one Do call — engine should be lazily created on just one worker.
	err = mgr.Do(context.Background(), func(e *Engine) error {
		e.Reset()
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
		wg.Add(1)
		go func() {
			defer wg.Done()
			_ = mgr.Do(context.Background(), func(e *Engine) error {
				e.Reset()
				e.Run(context.Background())
				return nil
			})
		}()
	}

	// Close while work is in flight. Should not panic.
	coord.Close()
	wg.Wait()
}

func TestCoordinatorInvalidThreadCount(t *testing.T) {
	_, err := NewCoordinator([]EngineSpec{{Name: "test"}}, Threads(0))
	if err == nil {
		t.Fatal("expected error for 0 threads")
	}
}

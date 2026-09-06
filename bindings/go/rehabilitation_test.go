package ferric

import (
	"context"
	"errors"
	"math"
	"testing"
	"time"
)

func TestSnapshotOptionsRejectAmbiguousConstruction(t *testing.T) {
	cases := []struct {
		name string
		opts []EngineOption
	}{
		{"nil", []EngineOption{WithSnapshot(nil, FormatCBOR)}},
		{"empty", []EngineOption{WithSnapshot([]byte{}, FormatCBOR)}},
		{"source", []EngineOption{WithSource("(ready)"), WithSnapshot([]byte{1}, FormatCBOR)}},
		{"empty-source", []EngineOption{WithSnapshot([]byte{1}, FormatCBOR), WithSource("")}},
		{"strategy", []EngineOption{WithSnapshot([]byte{1}, FormatCBOR), WithStrategy(StrategyDepth)}},
		{"encoding", []EngineOption{WithEncoding(EncodingUTF8), WithSnapshot([]byte{1}, FormatCBOR)}},
		{"depth", []EngineOption{WithSnapshot([]byte{1}, FormatCBOR), WithMaxCallDepth(64)}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			calls := recordEngineConstructors(t)
			_, err := NewEngine(tc.opts...)
			var invalid *InvalidArgumentError
			if !errors.Is(err, ErrInvalidArgument) || !errors.As(err, &invalid) {
				t.Fatalf("error = %v, want typed invalid argument", err)
			}
			if len(*calls) != 0 {
				t.Fatalf("native constructors called before validation: %v", *calls)
			}
		})
	}
}

func TestSnapshotWithoutOverridesRetainsSavedStrategy(t *testing.T) {
	e, err := NewEngine(WithStrategy(StrategyBreadth), WithSource(`
		(deffacts seeds (item 1) (item 2))
		(defrule emit (item ?n) => (printout t ?n))
	`))
	mustNoError(t, err)
	defer mustClose(t, e)
	data, err := e.Serialize(FormatCBOR)
	mustNoError(t, err)
	restored, err := NewEngine(WithSnapshot(data, FormatCBOR))
	mustNoError(t, err)
	defer mustClose(t, restored)
	result, err := restored.Run(context.Background())
	mustNoError(t, err)
	output, ok := restored.GetOutput("t")
	if result.RulesFired != 2 || !ok || output != "12" {
		t.Fatalf("restored breadth run = %+v, output %q (present %t)", result, output, ok)
	}
}

const boundedRunSource = `(deffacts seeds (item 1) (item 2) (item 3))
(defrule consume (item ?n) => (printout t ?n))`

type boundedRunner interface {
	RunWithLimit(ctx context.Context, limit int) (*RunResult, error)
	Reset() error
	Close() error
}

func TestRunLimitsRejectNegativeAcrossEngineFacades(t *testing.T) {
	constructors := []struct {
		name string
		new  func() (boundedRunner, error)
	}{
		{"raw", func() (boundedRunner, error) { return NewEngine(WithSource(boundedRunSource)) }},
		{"pinned", func() (boundedRunner, error) { return NewPinnedEngine(WithSource(boundedRunSource)) }},
	}
	for _, tc := range constructors {
		t.Run(tc.name, func(t *testing.T) {
			e, err := tc.new()
			mustNoError(t, err)
			defer mustClose(t, e)
			for _, limit := range []int{-1, math.MinInt} {
				result, err := e.RunWithLimit(context.Background(), limit)
				var invalid *InvalidArgumentError
				if result != nil || !errors.As(err, &invalid) {
					t.Fatalf("limit %d = (%+v, %v), want nil invalid argument", limit, result, err)
				}
			}
			// Rejected calls must leave the pending work intact.
			result, err := e.RunWithLimit(context.Background(), 0)
			mustNoError(t, err)
			if result.RulesFired != 3 {
				t.Fatalf("rejected run changed agenda: %+v", result)
			}
			for _, limit := range []int{1, math.MaxInt} {
				mustNoError(t, e.Reset())
				result, err := e.RunWithLimit(context.Background(), limit)
				mustNoError(t, err)
				if want := min(limit, 3); result.RulesFired != want {
					t.Fatalf("limit %d fired %d, want %d", limit, result.RulesFired, want)
				}
			}
		})
	}
}

func TestManagerRejectsNegativeLimitBeforeDispatch(t *testing.T) {
	coord, err := NewCoordinator([]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(boundedRunSource)}}})
	mustNoError(t, err)
	defer mustClose(t, coord)
	mgr, err := coord.Manager("test")
	mustNoError(t, err)
	started, release, done := make(chan struct{}), make(chan struct{}), make(chan error, 1)
	go func() {
		done <- mgr.Do(context.Background(), func(_ *Engine) error {
			close(started)
			<-release
			return nil
		})
	}()
	<-started
	defer func() { close(release); mustNoError(t, <-done) }()
	for _, limit := range []int{-1, math.MinInt} {
		result := make(chan error, 1)
		go func() {
			_, err := mgr.Evaluate(context.Background(), &EvaluateRequest{Limit: limit})
			result <- err
		}()
		select {
		case err := <-result:
			if !errors.Is(err, ErrInvalidArgument) {
				t.Fatalf("limit %d error = %v, want invalid argument", limit, err)
			}
		case <-time.After(time.Second):
			t.Fatal("invalid limit was queued behind active work")
		}
	}
}

func TestCoordinatorEveryCloseWaitsForActiveWork(t *testing.T) {
	coord, err := NewCoordinator([]EngineSpec{{Name: "test"}})
	mustNoError(t, err)
	mgr, err := coord.Manager("test")
	mustNoError(t, err)
	started, release, workDone := make(chan struct{}), make(chan struct{}), make(chan error, 1)
	go func() {
		workDone <- mgr.Do(context.Background(), func(e *Engine) error {
			close(started)
			<-release
			// The admitted operation retains a live engine until it finishes.
			_, err := e.AssertString("(assert (completed))")
			return err
		})
	}()
	<-started
	const closers = 8
	returned := make(chan error, closers)
	go func() { returned <- coord.Close() }()
	<-coord.done // first Close has committed to shutdown while work is held
	ready := make(chan struct{}, closers-1)
	for range closers - 1 {
		go func() { ready <- struct{}{}; returned <- coord.Close() }()
	}
	for range closers - 1 {
		<-ready
	}
	early := false
	select {
	case err := <-returned:
		early = true
		t.Errorf("Close returned before admitted work completed: %v", err)
	case <-time.After(25 * time.Millisecond):
	}
	close(release)
	mustNoError(t, <-workDone)
	remaining := closers
	if early {
		remaining--
	}
	for range remaining {
		select {
		case err := <-returned:
			mustNoError(t, err)
		case <-time.After(time.Second):
			t.Fatal("Close did not finish after active work completed")
		}
	}
}

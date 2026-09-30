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

func TestRunLimitRejectsNegative(t *testing.T) {
	e, err := NewEngine(WithSource(boundedRunSource))
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
}

func TestRawNegativeRunLimitDoesNotWaitForEngineAdmission(t *testing.T) {
	e, err := NewEngine()
	mustNoError(t, err)
	defer mustClose(t, e)
	e.lifecycle.Lock()
	defer e.lifecycle.Unlock()
	result := make(chan error, 1)
	go func() { _, err := e.RunWithLimit(context.Background(), -1); result <- err }()
	select {
	case err := <-result:
		if !errors.Is(err, ErrInvalidArgument) {
			t.Fatalf("error = %v, want invalid argument", err)
		}
	case <-time.After(time.Second):
		t.Fatal("invalid raw run waited for engine admission")
	}
}

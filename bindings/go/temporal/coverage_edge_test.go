package temporal

import (
	"context"
	"errors"
	"fmt"
	"testing"

	ferric "github.com/prb/ferric-rules/bindings/go"
	"pgregory.net/rapid"
)

func TestManualRulesActivityErrorOptionsAndRegisteredClosure(t *testing.T) {
	// Invalid coordinator options must surface through the Temporal constructor
	// so a worker fails during setup instead of registering broken activities.
	if _, err := NewRulesActivity(nil, ferric.Threads(0)); err == nil {
		t.Fatal("expected invalid thread count error")
	}

	// ActivityOption currently stores pass-through coordinator options. Testing
	// it directly documents the option contract even though construction accepts
	// coordinator options for compatibility.
	var cfg activityConfig
	WithCoordinatorOptions(ferric.Threads(2), ferric.Threads(3))(&cfg)
	if len(cfg.coordinatorOpts) != 2 {
		t.Fatalf("coordinator option count = %d, want 2", len(cfg.coordinatorOpts))
	}

	ra, err := NewRulesActivity(
		[]ferric.EngineSpec{
			{Name: "risk", Options: []ferric.EngineOption{
				ferric.WithSource(`(defrule r => (printout t "ok" crlf))`),
			}},
		},
	)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, ra)

	spy := &spyWorker{}
	ra.Register(spy)
	if len(spy.activities) != 1 {
		t.Fatalf("registered activity count = %d, want 1", len(spy.activities))
	}
	fn, ok := spy.activities[0].fn.(func(context.Context, *ferric.EvaluateRequest) (*ferric.EvaluateResult, error))
	if !ok {
		t.Fatalf("registered function has unexpected type %T", spy.activities[0].fn)
	}
	result, err := fn(context.Background(), &ferric.EvaluateRequest{})
	if err != nil {
		t.Fatal(err)
	}
	if result.RulesFired != 1 || result.Output["stdout"] != "ok\n" {
		t.Fatalf("activity result = %+v", result)
	}
}

func TestPropertyRulesActivityRegistersGeneratedSpecs(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		count := rapid.IntRange(1, 4).Draw(t, "count")
		threads := rapid.IntRange(1, 3).Draw(t, "threads")
		var cfg activityConfig
		WithCoordinatorOptions(ferric.Threads(threads))(&cfg)
		if len(cfg.coordinatorOpts) != 1 {
			t.Fatalf("coordinator option count = %d, want 1", len(cfg.coordinatorOpts))
		}
		if _, err := NewRulesActivity(nil, ferric.Threads(0)); err == nil {
			t.Fatal("invalid coordinator option should fail construction")
		}

		specs := make([]ferric.EngineSpec, count)
		for i := range specs {
			specs[i] = ferric.EngineSpec{
				Name: fmt.Sprintf("spec_%d", i),
				Options: []ferric.EngineOption{
					ferric.WithSource(`(defrule r => (assert (ok)))`),
				},
			}
		}

		ra, err := NewRulesActivity(specs)
		if err != nil {
			t.Fatal(err)
		}
		defer func() {
			if err := ra.Close(); err != nil && !errors.Is(err, context.Canceled) {
				t.Fatalf("close failed: %v", err)
			}
		}()

		spy := &spyWorker{}
		ra.Register(spy)
		if len(spy.activities) != count {
			t.Fatalf("registered %d activities, want %d", len(spy.activities), count)
		}
		fn, ok := spy.activities[0].fn.(func(context.Context, *ferric.EvaluateRequest) (*ferric.EvaluateResult, error))
		if !ok {
			t.Fatalf("registered function has unexpected type %T", spy.activities[0].fn)
		}
		if _, err := fn(context.Background(), &ferric.EvaluateRequest{}); err != nil {
			t.Fatalf("registered function failed: %v", err)
		}
	})
}

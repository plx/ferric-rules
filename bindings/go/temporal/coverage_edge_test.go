package temporal

import (
	"context"
	"testing"

	ferric "github.com/prb/ferric-rules/bindings/go"
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

// TestManualRulesActivityRoutesEachSpecToOwnManager verifies that Register
// builds a distinct closure per spec, each routed to that spec's own engine.
// Existing tests (TestRegisterMultipleSpecs) only check activity names/count; a
// regression that captured the same manager for every closure — or routed to
// the wrong one — would pass those but fail here. Each spec prints a unique
// token so the invoked closure's output identifies which engine actually ran.
func TestManualRulesActivityRoutesEachSpecToOwnManager(t *testing.T) {
	specs := []ferric.EngineSpec{
		{Name: "alpha", Options: []ferric.EngineOption{ferric.WithSource(`(defrule a => (printout t "alpha" crlf))`)}},
		{Name: "beta", Options: []ferric.EngineOption{ferric.WithSource(`(defrule b => (printout t "beta" crlf))`)}},
	}
	ra, err := NewRulesActivity(specs)
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, ra)

	spy := &spyWorker{}
	ra.Register(spy)
	if len(spy.activities) != len(specs) {
		t.Fatalf("registered %d activities, want %d", len(spy.activities), len(specs))
	}

	want := map[string]string{
		"ferric.Evaluate.alpha": "alpha\n",
		"ferric.Evaluate.beta":  "beta\n",
	}
	for _, a := range spy.activities {
		fn, ok := a.fn.(func(context.Context, *ferric.EvaluateRequest) (*ferric.EvaluateResult, error))
		if !ok {
			t.Fatalf("activity %q has unexpected fn type %T", a.opts.Name, a.fn)
		}
		result, err := fn(context.Background(), &ferric.EvaluateRequest{})
		if err != nil {
			t.Fatalf("activity %q failed: %v", a.opts.Name, err)
		}
		if got := result.Output["stdout"]; got != want[a.opts.Name] {
			t.Fatalf("activity %q stdout = %q, want %q", a.opts.Name, got, want[a.opts.Name])
		}
	}
}

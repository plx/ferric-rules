// Package temporal provides Temporal-idiomatic wrappers for ferric rules engine
// evaluation. Activities use the wire-level EvaluateRequest/EvaluateResult
// contract for cross-language compatibility.
package temporal

import (
	"context"
	"fmt"

	ferric "github.com/prb/ferric-rules/bindings/go"

	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/worker"
)

// RulesActivity is a Temporal activity struct backed by a ferric Coordinator.
// Each engine spec becomes a registered activity: "ferric.Evaluate.<specName>".
type RulesActivity struct {
	coord    *ferric.Coordinator
	managers map[string]*ferric.Manager
}

// NewRulesActivity creates activity handlers for the given engine specs.
func NewRulesActivity(specs []ferric.EngineSpec, opts ...ferric.CoordinatorOption) (*RulesActivity, error) {
	coord, err := ferric.NewCoordinator(specs, opts...)
	if err != nil {
		return nil, err
	}
	managers := make(map[string]*ferric.Manager, len(specs))
	for _, s := range specs {
		mgr, err := coord.Manager(s.Name)
		if err != nil {
			coord.Close()
			return nil, fmt.Errorf("temporal: getting manager for %q: %w", s.Name, err)
		}
		managers[s.Name] = mgr
	}
	return &RulesActivity{coord: coord, managers: managers}, nil
}

// Register registers per-spec Evaluate activities on a Temporal worker.
// Each spec produces an activity named "ferric.Evaluate.<specName>".
func (a *RulesActivity) Register(w worker.Worker) {
	for name, mgr := range a.managers {
		mgr := mgr
		activityName := "ferric.Evaluate." + name
		w.RegisterActivityWithOptions(
			func(ctx context.Context, req *ferric.EvaluateRequest) (*ferric.EvaluateResult, error) {
				return mgr.Evaluate(ctx, req)
			},
			activity.RegisterOptions{Name: activityName},
		)
	}
}

// Close shuts down the Coordinator. Call from worker shutdown hook.
func (a *RulesActivity) Close() error {
	return a.coord.Close()
}

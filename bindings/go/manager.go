package ferric

import (
	"context"
	"errors"
	"fmt"
)

// Manager is a handle for interacting with a specific engine type.
// It dispatches operations to the Coordinator's thread pool.
// Safe for concurrent use from multiple goroutines.
type Manager struct {
	coord    *Coordinator
	specName string
}

// Manager returns a Manager handle for the named engine spec.
// The spec must have been provided when creating the Coordinator.
func (c *Coordinator) Manager(specName string) (*Manager, error) {
	if _, ok := c.specs[specName]; !ok {
		return nil, fmt.Errorf("ferric: unknown engine spec %q", specName)
	}
	return &Manager{coord: c, specName: specName}, nil
}

// Do dispatches a function to an engine of this Manager's type.
// The function runs on a thread-locked worker goroutine. The Engine
// must not be retained beyond the closure's return.
func (m *Manager) Do(ctx context.Context, fn func(*Engine) error) error {
	if ctx == nil {
		ctx = context.Background()
	}
	if m.coord.closed.Load() {
		return errors.New("ferric: coordinator is closed")
	}
	w := m.coord.pickWorker(RouteHint{SpecName: m.specName})
	resp := make(chan error, 1)
	req := workerRequest{
		specName: m.specName,
		fn:       fn,
		resp:     resp,
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-m.coord.done:
		return errors.New("ferric: coordinator is closed")
	case w.requests <- req:
	}

	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-m.coord.done:
		return errors.New("ferric: coordinator is closed")
	case err := <-resp:
		return err
	}
}

// Evaluate resets the engine, asserts the given facts, runs to completion,
// and returns the resulting facts and output. This is the primary entry
// point for stateless one-shot evaluation.
func (m *Manager) Evaluate(ctx context.Context, req *EvaluateRequest) (*EvaluateResult, error) {
	var result *EvaluateResult
	err := m.Do(ctx, func(e *Engine) error {
		if err := e.Reset(); err != nil {
			return err
		}

		// Assert request facts from wire schema.
		for _, wf := range req.Facts {
			switch wf.Kind {
			case WireFactKindOrdered:
				if wf.Ordered == nil {
					return errors.New("ferric: ordered fact payload missing")
				}
				fields, err := WireSliceToNative(wf.Ordered.Fields)
				if err != nil {
					return err
				}
				if _, err := e.AssertFact(wf.Ordered.Relation, fields...); err != nil {
					return err
				}
			case WireFactKindTemplate:
				if wf.Template == nil {
					return errors.New("ferric: template fact payload missing")
				}
				slots, err := WireMapToNative(wf.Template.Slots)
				if err != nil {
					return err
				}
				if _, err := e.AssertTemplate(wf.Template.TemplateName, slots); err != nil {
					return err
				}
			default:
				return fmt.Errorf("ferric: unsupported fact kind %q", wf.Kind)
			}
		}

		// Run.
		var runResult *RunResult
		var err error
		if req.Limit > 0 {
			runResult, err = e.RunWithLimit(ctx, req.Limit)
		} else {
			runResult, err = e.Run(ctx)
		}
		if err != nil {
			return err
		}

		// Collect results.
		nativeFacts, err := e.Facts()
		if err != nil {
			return err
		}
		wireFacts, err := FactsToWire(nativeFacts)
		if err != nil {
			return err
		}

		output := make(map[string]string)
		if s, ok := e.GetOutput("t"); ok {
			output["stdout"] = s
		}
		if s, ok := e.GetOutput("stderr"); ok {
			output["stderr"] = s
		}

		result = &EvaluateResult{
			RunResult: *runResult,
			Facts:     wireFacts,
			Output:    output,
		}
		return nil
	})
	return result, err
}

// EvaluateNative is a Go-convenience wrapper that works with native
// Go types instead of wire types.
func (m *Manager) EvaluateNative(ctx context.Context, req *EvaluateNativeRequest) (*EvaluateNativeResult, error) {
	// Convert native request to wire request.
	wireReq := &EvaluateRequest{
		Limit: req.Limit,
		Facts: make([]WireFactInput, len(req.Facts)),
	}
	for i, nf := range req.Facts {
		if nf.TemplateName != "" {
			slots := make(map[string]WireValue, len(nf.Slots))
			for k, v := range nf.Slots {
				wv, err := NativeToWireValue(v)
				if err != nil {
					return nil, err
				}
				slots[k] = wv
			}
			wireReq.Facts[i] = TemplateFact(nf.TemplateName, slots)
		} else {
			fields := make([]WireValue, len(nf.Fields))
			for j, v := range nf.Fields {
				wv, err := NativeToWireValue(v)
				if err != nil {
					return nil, err
				}
				fields[j] = wv
			}
			wireReq.Facts[i] = OrderedFact(nf.Relation, fields...)
		}
	}

	wireResult, err := m.Evaluate(ctx, wireReq)
	if err != nil {
		return nil, err
	}

	// Convert wire results to native.
	nativeFacts := make([]Fact, len(wireResult.Facts))
	for i, wf := range wireResult.Facts {
		nativeFacts[i] = wireFactToNative(wf)
	}

	return &EvaluateNativeResult{
		RunResult: wireResult.RunResult,
		Facts:     nativeFacts,
		Output:    wireResult.Output,
	}, nil
}

func wireFactToNative(wf WireFact) Fact {
	f := Fact{ID: wf.ID}
	if wf.Kind == WireFactKindTemplate && wf.Template != nil {
		f.Type = FactTemplate
		f.TemplateName = wf.Template.TemplateName
		f.Slots = make(map[string]any, len(wf.Template.Slots))
		for k, wv := range wf.Template.Slots {
			v, _ := WireToNativeValue(wv)
			f.Slots[k] = v
		}
	} else if wf.Ordered != nil {
		f.Type = FactOrdered
		f.Relation = wf.Ordered.Relation
		f.Fields = make([]any, len(wf.Ordered.Fields))
		for i, wv := range wf.Ordered.Fields {
			v, _ := WireToNativeValue(wv)
			f.Fields[i] = v
		}
	}
	return f
}

// NativeFactInput describes a fact to assert using Go-native types.
type NativeFactInput struct {
	Relation     string
	Fields       []any
	TemplateName string
	Slots        map[string]any
}

// EvaluateNativeRequest is the Go-convenience request type.
type EvaluateNativeRequest struct {
	Facts []NativeFactInput
	Limit int
}

// EvaluateNativeResult is the Go-convenience result type.
type EvaluateNativeResult struct {
	RunResult
	Facts  []Fact
	Output map[string]string
}

// ---------------------------------------------------------------------------
// Standalone Manager (convenience for single-engine-type use cases)
// ---------------------------------------------------------------------------

// StandaloneManager wraps a Manager and owns its Coordinator.
type StandaloneManager struct {
	*Manager
	coord *Coordinator
}

// NewManager creates a standalone Manager backed by a single dedicated thread.
// For multiple engine types or higher concurrency, use NewCoordinator.
func NewManager(opts ...EngineOption) (*StandaloneManager, error) {
	coord, err := NewCoordinator(
		[]EngineSpec{{Name: "_default", Options: opts}},
	)
	if err != nil {
		return nil, err
	}
	mgr, _ := coord.Manager("_default")
	return &StandaloneManager{coord: coord, Manager: mgr}, nil
}

// Close shuts down the underlying Coordinator.
func (sm *StandaloneManager) Close() error {
	return sm.coord.Close()
}

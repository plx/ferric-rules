package ferric

import (
	"context"
	"errors"
	"fmt"
	"time"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/trace"
)

var (
	errNilContext                 = errors.New("ferric: nil context")
	errNilEvaluateRequest         = errors.New("ferric: nil evaluate request")
	errCoordinatorClosed          = errors.New("ferric: coordinator is closed")
	errOrderedFactPayloadMissing  = errors.New("ferric: ordered fact payload missing")
	errTemplateFactPayloadMissing = errors.New("ferric: template fact payload missing")
	errUnsupportedFactKind        = errors.New("ferric: unsupported fact kind")
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
		return nil, fmt.Errorf("%w %q", errUnknownEngineSpec, specName)
	}
	return &Manager{coord: c, specName: specName}, nil
}

// Do dispatches a function to an engine of this Manager's type.
// The function runs on a thread-locked worker goroutine. The Engine
// must not be retained beyond the closure's return.
//nolint:nonamedreturns // named return needed for deferred observability recording.
func (m *Manager) Do(ctx context.Context, fn func(*Engine) error) (retErr error) {
	if ctx == nil {
		return errNilContext
	}
	if m.coord.closed.Load() {
		return errCoordinatorClosed
	}

	o := m.coord.obs
	specAttr := attribute.String("ferric.spec", m.specName)
	ctx, span := o.tracer.Start(ctx, "ferric.dispatch", trace.WithAttributes(specAttr))
	start := time.Now()

	defer func() {
		dur := sinceSeconds(start)
		o.dispatchDuration.Record(ctx, dur, metric.WithAttributes(specAttr))
		o.dispatchTotal.Add(ctx, 1, metric.WithAttributes(specAttr))
		if retErr != nil {
			o.dispatchErrors.Add(ctx, 1, metric.WithAttributes(specAttr))
			o.recordError(span, retErr)
			if o.logger != nil {
				o.logger.WarnContext(ctx, "dispatch failed",
					"spec", m.specName, "duration_s", dur, "error", retErr)
			}
		}
		span.End()
	}()

	w := m.coord.pickWorker(RouteHint{SpecName: m.specName})
	resp := make(chan error, 1)
	req := workerRequest{
		specName:   m.specName,
		fn:         fn,
		resp:       resp,
		enqueuedAt: time.Now(),
	}

	// Check for cancellation before enqueueing. Without this explicit
	// check, a canceled ctx and a ready channel are both selectable and
	// Go picks pseudo-randomly, allowing a request to be dispatched
	// after the caller believes it was aborted.
	if err := ctx.Err(); err != nil {
		return fmt.Errorf("ferric: request canceled before dispatch: %w", err)
	}

	select {
	case <-ctx.Done():
		return fmt.Errorf("ferric: request canceled before dispatch: %w", ctx.Err())
	case <-m.coord.done:
		return errCoordinatorClosed
	case w.requests <- req:
	}

	// The worker guarantees it will drain all buffered requests before
	// exiting, so an accepted request always gets a real response.
	// We intentionally do NOT select on m.coord.done here: doing so
	// would race with resp and could discard the real result.
	select {
	case <-ctx.Done():
		return fmt.Errorf("ferric: request canceled while waiting for worker: %w", ctx.Err())
	case err := <-resp:
		return err
	}
}

// Evaluate resets the engine, asserts the given facts, runs to completion,
// and returns the resulting facts and output. This is the primary entry
// point for stateless one-shot evaluation.
func (m *Manager) Evaluate(ctx context.Context, req *EvaluateRequest) (*EvaluateResult, error) {
	if req == nil {
		return nil, errNilEvaluateRequest
	}

	o := m.coord.obs
	specAttr := attribute.String("ferric.spec", m.specName)
	ctx, span := o.tracer.Start(ctx, "ferric.evaluate", trace.WithAttributes(specAttr))
	defer span.End()

	var result *EvaluateResult
	err := m.Do(ctx, func(e *Engine) error {
		if err := e.Reset(); err != nil {
			return err
		}

		if err := assertWireFacts(e, req.Facts); err != nil {
			return err
		}

		runStart := time.Now()
		runResult, err := runEvaluate(ctx, e, req.Limit)
		if err != nil {
			return err
		}
		o.runDuration.Record(ctx, sinceSeconds(runStart), metric.WithAttributes(specAttr))

		span.SetAttributes(
			attribute.Int("ferric.rules_fired", runResult.RulesFired),
			attribute.String("ferric.halt_reason", runResult.HaltReason.String()),
		)

		if o.logger != nil {
			o.logger.DebugContext(ctx, "evaluate completed",
				"spec", m.specName,
				"rules_fired", runResult.RulesFired,
				"halt_reason", runResult.HaltReason.String())
		}

		result, err = buildEvaluateResult(e, runResult)
		return err
	})

	if err != nil {
		o.recordError(span, err)
	}
	return result, err
}

func assertWireFacts(e *Engine, facts []WireFactInput) error {
	for _, wf := range facts {
		switch wf.Kind {
		case WireFactKindOrdered:
			if wf.Ordered == nil {
				return errOrderedFactPayloadMissing
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
				return errTemplateFactPayloadMissing
			}
			slots, err := WireMapToNative(wf.Template.Slots)
			if err != nil {
				return err
			}
			if _, err := e.AssertTemplate(wf.Template.TemplateName, slots); err != nil {
				return err
			}
		default:
			return fmt.Errorf("%w %q", errUnsupportedFactKind, wf.Kind)
		}
	}

	return nil
}

func runEvaluate(ctx context.Context, e *Engine, limit int) (*RunResult, error) {
	if limit > 0 {
		return e.RunWithLimit(ctx, limit)
	}
	return e.Run(ctx)
}

func buildEvaluateResult(e *Engine, runResult *RunResult) (*EvaluateResult, error) {
	nativeFacts, err := e.Facts()
	if err != nil {
		return nil, err
	}
	wireFacts, err := FactsToWire(nativeFacts)
	if err != nil {
		return nil, err
	}

	output := make(map[string]string)
	if s, ok := e.GetOutput("t"); ok {
		output["stdout"] = s
	}
	if s, ok := e.GetOutput("stderr"); ok {
		output["stderr"] = s
	}

	return &EvaluateResult{
		RunResult: *runResult,
		Facts:     wireFacts,
		Output:    output,
	}, nil
}

// EvaluateNative is a Go-convenience wrapper that works with native
// Go types instead of wire types.
func (m *Manager) EvaluateNative(ctx context.Context, req *EvaluateNativeRequest) (*EvaluateNativeResult, error) {
	if req == nil {
		return nil, errNilEvaluateRequest
	}
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

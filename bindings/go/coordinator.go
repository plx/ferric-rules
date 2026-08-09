package ferric

import (
	"context"
	"errors"
	"fmt"
	"runtime"
	"sync/atomic"
	"time"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

var (
	errInvalidThreadCount = errors.New("ferric: thread count must be >= 1")
	errUnknownEngineSpec  = errors.New("ferric: unknown engine spec")
)

// RouteHint carries request metadata for dispatch policy selection.
type RouteHint struct {
	SpecName string
}

// DispatchPolicy picks a worker index for a request.
// The returned index is normalized to [0, numWorkers) via modular arithmetic,
// so out-of-range or negative values are safe (they wrap deterministically).
type DispatchPolicy interface {
	PickWorker(hint RouteHint, numWorkers int, counter uint64) int
}

type roundRobinPolicy struct{}

func (roundRobinPolicy) PickWorker(_ RouteHint, numWorkers int, counter uint64) int {
	if numWorkers <= 0 {
		return 0
	}
	//nolint:gosec // numWorkers is derived from len(c.workers) and is non-negative.
	return int(counter % uint64(numWorkers))
}

// Coordinator manages a pool of OS threads and a fixed set of engine types.
// Engines are lazily instantiated per-thread on first use.
type Coordinator struct {
	specs   map[string][]EngineOption
	workers []*worker
	next    atomic.Uint64
	policy  DispatchPolicy
	done    chan struct{}
	closed  atomic.Bool
	obs     *obs
}

// NewCoordinator creates a Coordinator with the given engine specs and
// thread pool configuration. All engine specs must be provided upfront.
func NewCoordinator(specs []EngineSpec, opts ...CoordinatorOption) (*Coordinator, error) {
	cfg := coordConfig{
		threads: 1,
		policy:  roundRobinPolicy{},
	}
	for _, opt := range opts {
		opt(&cfg)
	}
	if cfg.threads < 1 {
		return nil, errInvalidThreadCount
	}

	c := &Coordinator{
		specs:  make(map[string][]EngineOption, len(specs)),
		policy: cfg.policy,
		done:   make(chan struct{}),
		obs:    newObs(&cfg),
	}
	for _, s := range specs {
		c.specs[s.Name] = s.Options
	}

	c.workers = make([]*worker, cfg.threads)
	for i := range c.workers {
		w := newWorker(c.specs, c.obs)
		c.workers[i] = w
	}
	return c, nil
}

func (c *Coordinator) pickWorker(hint RouteHint) *worker {
	rr := c.next.Add(1) - 1
	n := len(c.workers)
	idx := c.policy.PickWorker(hint, n, rr)
	// Normalize: map any int (including negative) into [0, n).
	idx = ((idx % n) + n) % n
	return c.workers[idx]
}

// Close shuts down the coordinator. It stops accepting new requests, completes
// all non-canceled work already queued or started, and then frees all engines.
// It blocks until the worker goroutines have exited.
func (c *Coordinator) Close() error {
	if !c.closed.CompareAndSwap(false, true) {
		return nil
	}
	// Phase 1: Signal callers that no new work will be accepted.
	close(c.done)
	// Phase 2: Closing each request queue wakes blocked submitters and lets its
	// worker drain all remaining, non-canceled requests before exiting.
	for _, w := range c.workers {
		if w != nil {
			w.requests.close()
		}
	}
	for _, w := range c.workers {
		if w != nil {
			<-w.done
		}
	}
	return nil
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

type workerRequest struct {
	specName   string
	call       *workerCall
	enqueuedAt time.Time
}

type worker struct {
	specs    map[string][]EngineOption
	engines  map[string]*Engine
	requests *requestQueue[workerRequest]
	done     chan struct{}
	obs      *obs
}

func newWorker(specs map[string][]EngineOption, o *obs) *worker {
	w := &worker{
		specs:    specs,
		engines:  make(map[string]*Engine),
		requests: newRequestQueue[workerRequest](workerRequestQueueCapacity),
		done:     make(chan struct{}),
		obs:      o,
	}

	ready := make(chan struct{})

	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		defer close(w.done)
		defer w.closeAllEngines()

		close(ready)

		for {
			request := w.requests.dequeue()
			if !request.ok {
				return
			}
			w.handle(request.value)
		}
	}()

	<-ready
	return w
}

func (w *worker) handle(req workerRequest) {
	if !req.enqueuedAt.IsZero() {
		specAttr := attribute.String("ferric.spec", req.specName)
		w.obs.waitDuration.Record(context.Background(), sinceSeconds(req.enqueuedAt),
			metric.WithAttributes(specAttr))
	}
	engine, err := w.getOrCreateEngine(req.specName)
	req.call.complete(engine, err)
}

func (w *worker) getOrCreateEngine(specName string) (*Engine, error) {
	if eng, ok := w.engines[specName]; ok {
		return eng, nil
	}
	opts, ok := w.specs[specName]
	if !ok {
		return nil, fmt.Errorf("%w %q", errUnknownEngineSpec, specName)
	}

	specAttr := attribute.String("ferric.spec", specName)
	start := time.Now()
	eng, err := NewEngine(opts...)
	dur := sinceSeconds(start)

	if err != nil {
		if w.obs.logger != nil {
			w.obs.logger.Error("engine cold start failed",
				"spec", specName, "duration_s", dur, "error", err)
		}
		return nil, fmt.Errorf("ferric: creating engine %q: %w", specName, err)
	}

	w.obs.coldStartDuration.Record(context.Background(), dur,
		metric.WithAttributes(specAttr))
	if w.obs.logger != nil {
		w.obs.logger.Info("engine cold start",
			"spec", specName, "duration_s", dur)
	}

	w.engines[specName] = eng
	return eng, nil
}

func (w *worker) closeAllEngines() {
	for _, eng := range w.engines {
		_ = eng.Close()
	}
}

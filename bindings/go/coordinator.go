package ferric

import (
	"errors"
	"fmt"
	"runtime"
	"sync/atomic"
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
	}
	for _, s := range specs {
		c.specs[s.Name] = s.Options
	}

	c.workers = make([]*worker, cfg.threads)
	for i := range c.workers {
		w := newWorker(c.specs)
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

// Close shuts down the coordinator. It stops accepting new requests, drains
// all previously-accepted work so that every in-flight request completes with
// its real result, and then frees all engines. Blocks until every accepted
// request has been processed and all worker goroutines have exited.
func (c *Coordinator) Close() error {
	if !c.closed.CompareAndSwap(false, true) {
		return nil
	}
	// Signal callers that no new work will be accepted.
	close(c.done)
	// Tell workers to drain remaining requests and exit.
	for _, w := range c.workers {
		if w != nil {
			close(w.stop)
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
	specName string
	fn       func(*Engine) error
	resp     chan error
}

type worker struct {
	specs    map[string][]EngineOption
	engines  map[string]*Engine
	requests chan workerRequest
	stop     chan struct{}
	done     chan struct{}
}

func newWorker(specs map[string][]EngineOption) *worker {
	w := &worker{
		specs:    specs,
		engines:  make(map[string]*Engine),
		requests: make(chan workerRequest, 16),
		stop:     make(chan struct{}),
		done:     make(chan struct{}),
	}

	ready := make(chan struct{})

	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		defer close(w.done)
		defer w.closeAllEngines()

		close(ready)

		for {
			select {
			case <-w.stop:
				w.drain()
				return
			case req := <-w.requests:
				w.handle(req)
			}
		}
	}()

	<-ready
	return w
}

func (w *worker) handle(req workerRequest) {
	engine, err := w.getOrCreateEngine(req.specName)
	if err != nil {
		req.resp <- err
		return
	}
	req.resp <- req.fn(engine)
}

// drain processes all buffered requests remaining in the channel so that
// accepted work always completes with its real result during shutdown.
func (w *worker) drain() {
	for {
		select {
		case req := <-w.requests:
			w.handle(req)
		default:
			return
		}
	}
}

func (w *worker) getOrCreateEngine(specName string) (*Engine, error) {
	if eng, ok := w.engines[specName]; ok {
		return eng, nil
	}
	opts, ok := w.specs[specName]
	if !ok {
		return nil, fmt.Errorf("%w %q", errUnknownEngineSpec, specName)
	}
	eng, err := NewEngine(opts...)
	if err != nil {
		return nil, fmt.Errorf("ferric: creating engine %q: %w", specName, err)
	}
	w.engines[specName] = eng
	return eng, nil
}

func (w *worker) closeAllEngines() {
	for _, eng := range w.engines {
		_ = eng.Close()
	}
}

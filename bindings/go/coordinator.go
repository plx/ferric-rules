package ferric

import (
	"errors"
	"fmt"
	"runtime"
	"sync/atomic"
)

// RouteHint carries request metadata for dispatch policy selection.
type RouteHint struct {
	SpecName string
}

// DispatchPolicy picks a worker index for a request.
type DispatchPolicy interface {
	PickWorker(hint RouteHint, numWorkers int, counter uint64) int
}

type roundRobinPolicy struct{}

func (roundRobinPolicy) PickWorker(_ RouteHint, numWorkers int, counter uint64) int {
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
		return nil, errors.New("ferric: thread count must be >= 1")
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
		w, err := newWorker(c.specs)
		if err != nil {
			c.Close()
			return nil, fmt.Errorf("ferric: starting worker %d: %w", i, err)
		}
		c.workers[i] = w
	}
	return c, nil
}

func (c *Coordinator) pickWorker(hint RouteHint) *worker {
	rr := c.next.Add(1) - 1
	idx := c.policy.PickWorker(hint, len(c.workers), rr)
	return c.workers[idx]
}

// Close shuts down all worker goroutines and frees all engines.
// Blocks until all in-flight requests complete.
func (c *Coordinator) Close() error {
	if !c.closed.CompareAndSwap(false, true) {
		return nil
	}
	close(c.done)
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

func newWorker(specs map[string][]EngineOption) (*worker, error) {
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
				return
			case req := <-w.requests:
				engine, err := w.getOrCreateEngine(req.specName)
				if err != nil {
					req.resp <- err
					continue
				}
				req.resp <- req.fn(engine)
			}
		}
	}()

	<-ready
	return w, nil
}

func (w *worker) getOrCreateEngine(specName string) (*Engine, error) {
	if eng, ok := w.engines[specName]; ok {
		return eng, nil
	}
	opts, ok := w.specs[specName]
	if !ok {
		return nil, fmt.Errorf("ferric: unknown engine spec %q", specName)
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
		eng.Close()
	}
}

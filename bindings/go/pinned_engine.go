package ferric

import (
	"context"
	"errors"
	"fmt"
	"runtime"
	"sync"
	"sync/atomic"
)

var errPinnedEngineClosed = errors.New("ferric: pinned engine is closed")

// PinnedEngine is a stateful single-engine wrapper that hides thread-affinity
// mechanics from callers. It manages a dedicated OS-locked goroutine and
// serializes all engine operations through it.
//
// All methods are safe for concurrent use from multiple goroutines.
// Operations are serialized on the internal worker goroutine in FIFO order.
//
// PinnedEngine implements io.Closer. Always defer Close() after creation.
type PinnedEngine struct {
	requests   chan pinnedRequest
	stop       chan struct{}
	done       chan struct{}
	closeOnce  sync.Once
	closed     atomic.Bool
	closeGuard sync.RWMutex // protects enqueue windows during shutdown
}

type pinnedRequest struct {
	fn   func(*Engine) error
	resp chan error
}

// NewPinnedEngine creates a PinnedEngine backed by a dedicated OS-locked
// goroutine. The engine is created on the worker thread using the given options.
// Returns an error if engine creation fails.
func NewPinnedEngine(opts ...EngineOption) (*PinnedEngine, error) {
	p := &PinnedEngine{
		requests: make(chan pinnedRequest, 16),
		stop:     make(chan struct{}),
		done:     make(chan struct{}),
	}

	ready := make(chan error, 1)

	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		defer close(p.done)

		eng, err := NewEngine(opts...)
		if err != nil {
			ready <- err
			return
		}
		defer eng.Close() //nolint:errcheck // best-effort cleanup on worker exit.

		close(ready) // signal success

		for {
			select {
			case <-p.stop:
				p.drain(eng)
				return
			case req := <-p.requests:
				req.resp <- req.fn(eng)
			}
		}
	}()

	if err := <-ready; err != nil {
		return nil, err
	}
	return p, nil
}

// drain processes all buffered requests so that accepted work completes
// with its real result during shutdown.
func (p *PinnedEngine) drain(eng *Engine) {
	for {
		select {
		case req := <-p.requests:
			req.resp <- req.fn(eng)
		default:
			return
		}
	}
}

// Close shuts down the PinnedEngine. It stops accepting new requests,
// waits for all in-progress enqueue attempts to resolve, drains all
// previously-accepted work, closes the underlying engine, and blocks
// until the worker goroutine exits.
//
// Close is idempotent and safe to call from any goroutine.
func (p *PinnedEngine) Close() error {
	p.closeOnce.Do(func() {
		p.closed.Store(true)
		// Wait for all in-progress enqueue attempts to resolve,
		// then signal worker to stop while holding the write lock.
		// The write lock blocks until every RLock holder (tryEnqueue) exits.
		// After this acquires, no goroutine can write to p.requests.
		p.closeGuard.Lock()
		close(p.stop)
		p.closeGuard.Unlock()
	})
	<-p.done
	return nil
}

// tryEnqueue attempts to place req into the worker's request channel.
// It holds a read lock on closeGuard so that Close (which takes a write
// lock) blocks until all in-progress enqueue attempts resolve. This
// eliminates the race where a request is enqueued after the worker's
// drain has completed.
func (p *PinnedEngine) tryEnqueue(ctx context.Context, req pinnedRequest) error {
	p.closeGuard.RLock()
	defer p.closeGuard.RUnlock()

	if p.closed.Load() {
		return errPinnedEngineClosed
	}

	if err := ctx.Err(); err != nil {
		return fmt.Errorf("ferric: request canceled before dispatch: %w", err)
	}

	select {
	case <-ctx.Done():
		return fmt.Errorf("ferric: request canceled before dispatch: %w", ctx.Err())
	case <-p.done:
		return errPinnedEngineClosed
	case p.requests <- req:
		return nil
	}
}

// Do dispatches an arbitrary function to the pinned engine's worker thread.
// The function runs with exclusive access to the underlying Engine.
// The Engine must not be retained beyond the closure's return.
//
// Returns errPinnedEngineClosed if the PinnedEngine has been closed.
// Respects context cancellation for both dispatch and waiting.
func (p *PinnedEngine) Do(ctx context.Context, fn func(*Engine) error) error {
	if ctx == nil {
		return errNilContext
	}

	resp := make(chan error, 1)
	req := pinnedRequest{fn: fn, resp: resp}

	if err := p.tryEnqueue(ctx, req); err != nil {
		return err
	}

	select {
	case <-ctx.Done():
		return fmt.Errorf("ferric: request canceled while waiting for worker: %w", ctx.Err())
	case err := <-resp:
		return err
	}
}

// do is the internal dispatch helper used by all typed methods.
// It uses context.Background() since typed methods that need context
// accept it explicitly and pass it to the engine within the closure.
func (p *PinnedEngine) do(fn func(*Engine) error) error {
	return p.Do(context.Background(), fn)
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

// Load loads CLIPS source into the engine.
func (p *PinnedEngine) Load(source string) error {
	return p.do(func(e *Engine) error {
		return e.Load(source)
	})
}

// ---------------------------------------------------------------------------
// Fact Operations
// ---------------------------------------------------------------------------

// AssertString asserts a fact from a CLIPS source string.
func (p *PinnedEngine) AssertString(source string) (uint64, error) {
	var id uint64
	err := p.do(func(e *Engine) error {
		var err error
		id, err = e.AssertString(source)
		return err
	})
	return id, err
}

// AssertFact asserts an ordered fact with the given relation and fields.
func (p *PinnedEngine) AssertFact(relation string, fields ...any) (uint64, error) {
	var id uint64
	err := p.do(func(e *Engine) error {
		var err error
		id, err = e.AssertFact(relation, fields...)
		return err
	})
	return id, err
}

// AssertTemplate asserts a template fact with named slot values.
func (p *PinnedEngine) AssertTemplate(templateName string, slots map[string]any) (uint64, error) {
	var id uint64
	err := p.do(func(e *Engine) error {
		var err error
		id, err = e.AssertTemplate(templateName, slots)
		return err
	})
	return id, err
}

// Retract removes a fact by its ID.
func (p *PinnedEngine) Retract(factID uint64) error {
	return p.do(func(e *Engine) error {
		return e.Retract(factID)
	})
}

// GetFact returns a snapshot of a single fact.
func (p *PinnedEngine) GetFact(factID uint64) (*Fact, error) {
	var f *Fact
	err := p.do(func(e *Engine) error {
		var err error
		f, err = e.GetFact(factID)
		return err
	})
	return f, err
}

// Facts returns snapshots of all user-visible facts.
func (p *PinnedEngine) Facts() ([]Fact, error) {
	var facts []Fact
	err := p.do(func(e *Engine) error {
		var err error
		facts, err = e.Facts()
		return err
	})
	return facts, err
}

// FindFacts returns snapshots of facts matching the given relation name.
func (p *PinnedEngine) FindFacts(relation string) ([]Fact, error) {
	var facts []Fact
	err := p.do(func(e *Engine) error {
		var err error
		facts, err = e.FindFacts(relation)
		return err
	})
	return facts, err
}

// FactCount returns the number of user-visible facts.
func (p *PinnedEngine) FactCount() (int, error) {
	var count int
	err := p.do(func(e *Engine) error {
		var err error
		count, err = e.FactCount()
		return err
	})
	return count, err
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

// Run runs the engine to completion, checking context for cancellation.
func (p *PinnedEngine) Run(ctx context.Context) (*RunResult, error) {
	return p.RunWithLimit(ctx, 0)
}

// RunWithLimit runs the engine with a maximum number of rule firings.
// A limit of 0 means unlimited. Checks context for cancellation.
func (p *PinnedEngine) RunWithLimit(ctx context.Context, limit int) (*RunResult, error) {
	var result *RunResult
	err := p.Do(ctx, func(e *Engine) error {
		var err error
		result, err = e.RunWithLimit(ctx, limit)
		return err
	})
	return result, err
}

// Step executes a single rule firing.
// Returns nil if the agenda is empty.
func (p *PinnedEngine) Step() (*FiredRule, error) {
	var fr *FiredRule
	err := p.do(func(e *Engine) error {
		var err error
		fr, err = e.Step()
		return err
	})
	return fr, err
}

// Halt requests the engine to halt.
func (p *PinnedEngine) Halt() {
	_ = p.do(func(e *Engine) error {
		e.Halt()
		return nil
	})
}

// Reset resets the engine to its initial state (facts cleared, rules kept).
func (p *PinnedEngine) Reset() error {
	return p.do(func(e *Engine) error {
		return e.Reset()
	})
}

// Clear removes all rules, facts, templates, etc. from the engine.
func (p *PinnedEngine) Clear() {
	_ = p.do(func(e *Engine) error {
		e.Clear()
		return nil
	})
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

// Serialize produces a snapshot of the engine's current state.
func (p *PinnedEngine) Serialize(format Format) ([]byte, error) {
	var data []byte
	err := p.do(func(e *Engine) error {
		var err error
		data, err = e.Serialize(format)
		return err
	})
	return data, err
}

// SerializeToFile writes a serialized snapshot to the given file path.
func (p *PinnedEngine) SerializeToFile(path string, format Format) error {
	return p.do(func(e *Engine) error {
		return e.SerializeToFile(path, format)
	})
}

// ---------------------------------------------------------------------------
// Introspection
// ---------------------------------------------------------------------------

// Rules returns information about all registered rules.
func (p *PinnedEngine) Rules() []RuleInfo {
	var rules []RuleInfo
	_ = p.do(func(e *Engine) error {
		rules = e.Rules()
		return nil
	})
	return rules
}

// Templates returns the names of all registered templates.
func (p *PinnedEngine) Templates() []string {
	var names []string
	_ = p.do(func(e *Engine) error {
		names = e.Templates()
		return nil
	})
	return names
}

// GetGlobal retrieves a global variable's value by name.
func (p *PinnedEngine) GetGlobal(name string) (any, error) {
	var val any
	err := p.do(func(e *Engine) error {
		var err error
		val, err = e.GetGlobal(name)
		return err
	})
	return val, err
}

// CurrentModule returns the name of the current module.
func (p *PinnedEngine) CurrentModule() string {
	var name string
	_ = p.do(func(e *Engine) error {
		name = e.CurrentModule()
		return nil
	})
	return name
}

// Focus returns the module at the top of the focus stack.
func (p *PinnedEngine) Focus() (string, bool) {
	var name string
	var ok bool
	_ = p.do(func(e *Engine) error {
		name, ok = e.Focus()
		return nil
	})
	return name, ok
}

// FocusStack returns the focus stack entries from bottom to top.
func (p *PinnedEngine) FocusStack() []string {
	var stack []string
	_ = p.do(func(e *Engine) error {
		stack = e.FocusStack()
		return nil
	})
	return stack
}

// AgendaSize returns the number of activations on the agenda.
func (p *PinnedEngine) AgendaSize() int {
	var count int
	_ = p.do(func(e *Engine) error {
		count = e.AgendaSize()
		return nil
	})
	return count
}

// IsHalted returns true if the engine is halted.
func (p *PinnedEngine) IsHalted() bool {
	var halted bool
	_ = p.do(func(e *Engine) error {
		halted = e.IsHalted()
		return nil
	})
	return halted
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

// GetOutput retrieves captured output for a named channel.
func (p *PinnedEngine) GetOutput(channel string) (string, bool) {
	var s string
	var ok bool
	_ = p.do(func(e *Engine) error {
		s, ok = e.GetOutput(channel)
		return nil
	})
	return s, ok
}

// ClearOutput clears a specific output channel.
func (p *PinnedEngine) ClearOutput(channel string) {
	_ = p.do(func(e *Engine) error {
		e.ClearOutput(channel)
		return nil
	})
}

// PushInput pushes an input line for read/readline.
func (p *PinnedEngine) PushInput(line string) {
	_ = p.do(func(e *Engine) error {
		e.PushInput(line)
		return nil
	})
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

// Diagnostics returns all action diagnostic messages from recent execution.
func (p *PinnedEngine) Diagnostics() []string {
	var diags []string
	_ = p.do(func(e *Engine) error {
		diags = e.Diagnostics()
		return nil
	})
	return diags
}

// ClearDiagnostics clears all stored action diagnostics.
func (p *PinnedEngine) ClearDiagnostics() {
	_ = p.do(func(e *Engine) error {
		e.ClearDiagnostics()
		return nil
	})
}

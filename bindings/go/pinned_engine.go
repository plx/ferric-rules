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
// Engine operations are serialized on the internal worker goroutine in FIFO
// order. Halt and the initial Close signal are out-of-band controls so they can
// interrupt an active Run without violating engine thread affinity.
//
// PinnedEngine implements io.Closer. Always defer Close() after creation.
type PinnedEngine struct {
	requests  *requestQueue[pinnedRequest]
	done      chan struct{}
	closeOnce sync.Once
	closed    atomic.Bool
	activeRun atomic.Pointer[pinnedRunControl]
}

type pinnedRunControl struct {
	halted atomic.Bool
}

type pinnedRequest struct {
	call *workerCall
}

type valueOK[T any] struct {
	value T
	ok    bool
}

// NewPinnedEngine creates a PinnedEngine backed by a dedicated OS-locked
// goroutine. The engine is created on the worker thread using the given options.
// Returns an error if engine creation fails.
func NewPinnedEngine(opts ...EngineOption) (*PinnedEngine, error) {
	p := &PinnedEngine{
		requests: newRequestQueue[pinnedRequest](workerRequestQueueCapacity),
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
			request := p.requests.dequeue()
			if !request.ok {
				return
			}
			request.value.call.complete(eng, nil)
		}
	}()

	if err := <-ready; err != nil {
		return nil, err
	}
	return p, nil
}

// Close shuts down the PinnedEngine. It stops accepting new requests and
// interrupts active and already-queued Run requests. Close-induced
// interruption uses the same cooperative-cancellation outcome as Halt: a
// partial (or zero) RunResult with HaltRequested and a nil error, rather than
// errPinnedEngineClosed. Requests rejected after Close begins return
// errPinnedEngineClosed. Other non-canceled work already queued or started is
// completed before the underlying engine is closed. Close then blocks until the
// worker goroutine exits.
//
// Run interruption is cooperative and is observed between batches of at most
// 100 rule firings. Close cannot preempt an arbitrary function submitted with
// Do while that function is executing.
//
// Close is idempotent and safe to call from any goroutine.
func (p *PinnedEngine) Close() error {
	p.closeOnce.Do(func() {
		// The run loop observes this sticky flag without going through the
		// request queue. Publish it before closing the queue so an active run
		// can make progress toward worker shutdown.
		p.closed.Store(true)
		p.requests.close()
	})
	<-p.done
	return nil
}

// tryEnqueue places req into the removable worker queue. The returned handle
// owns cancellation until the worker dequeues the request.
func (p *PinnedEngine) tryEnqueue(
	ctx context.Context,
	req pinnedRequest,
) (*queuedRequest[pinnedRequest], error) {
	if p.closed.Load() {
		return nil, errPinnedEngineClosed
	}

	queued, err := p.requests.enqueue(ctx, req)
	if errors.Is(err, errRequestQueueClosed) {
		return nil, errPinnedEngineClosed
	}
	if err != nil {
		return nil, fmt.Errorf("ferric: request canceled before dispatch: %w", err)
	}
	return queued, nil
}

// Do dispatches an arbitrary function to the pinned engine's worker thread.
// The function runs with exclusive access to the underlying Engine.
// The Engine must not be retained beyond the closure's return.
// A panic in fn is recovered by the worker as *PanicError; engine changes
// completed before the panic are not rolled back. A request is queued after it
// enters the FIFO and becomes started when the worker dequeues it. Cancellation
// removes a queued callback and returns an error wrapping ctx.Err(). Once
// started, Do waits for fn's real result; cancellation does not preempt an
// arbitrary callback.
//
// Returns errPinnedEngineClosed if the PinnedEngine has been closed.
// Respects context cancellation for both dispatch and waiting.
func (p *PinnedEngine) Do(ctx context.Context, fn func(*Engine) error) error {
	response := pinnedCall(ctx, p, func(engine *Engine) (struct{}, error) {
		return struct{}{}, fn(engine)
	})
	return response.err
}

func pinnedCall[T any](
	ctx context.Context,
	p *PinnedEngine,
	fn func(*Engine) (T, error),
) workerResponse[T] {
	if ctx == nil {
		return workerResponse[T]{err: errNilContext}
	}

	call, responses := newWorkerCall(fn)
	queued, err := p.tryEnqueue(ctx, pinnedRequest{call: call})
	if err != nil {
		return workerResponse[T]{err: err}
	}
	return waitWorkerResponse(ctx, queued.cancel, responses)
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
	response := pinnedCall(context.Background(), p, func(e *Engine) (uint64, error) {
		return e.AssertString(source)
	})
	return response.value, response.err
}

// AssertFact asserts an ordered fact with the given relation and fields.
func (p *PinnedEngine) AssertFact(relation string, fields ...any) (uint64, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (uint64, error) {
		return e.AssertFact(relation, fields...)
	})
	return response.value, response.err
}

// AssertTemplate asserts a template fact with named slot values.
func (p *PinnedEngine) AssertTemplate(templateName string, slots map[string]any) (uint64, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (uint64, error) {
		return e.AssertTemplate(templateName, slots)
	})
	return response.value, response.err
}

// Retract removes a fact by its ID.
func (p *PinnedEngine) Retract(factID uint64) error {
	return p.do(func(e *Engine) error {
		return e.Retract(factID)
	})
}

// GetFact returns a snapshot of a single fact.
func (p *PinnedEngine) GetFact(factID uint64) (*Fact, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (*Fact, error) {
		return e.GetFact(factID)
	})
	return response.value, response.err
}

// Facts returns snapshots of all user-visible facts.
func (p *PinnedEngine) Facts() ([]Fact, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) ([]Fact, error) {
		return e.Facts()
	})
	return response.value, response.err
}

// FindFacts returns snapshots of facts matching the given relation name.
func (p *PinnedEngine) FindFacts(relation string) ([]Fact, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) ([]Fact, error) {
		return e.FindFacts(relation)
	})
	return response.value, response.err
}

// FactCount returns the number of user-visible facts.
func (p *PinnedEngine) FactCount() (int, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (int, error) {
		return e.FactCount()
	})
	return response.value, response.err
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

// Run runs the engine to completion, checking context for cancellation and
// PinnedEngine interruption between batches of at most 100 rule firings.
func (p *PinnedEngine) Run(ctx context.Context) (*RunResult, error) {
	return p.RunWithLimit(ctx, 0)
}

// RunWithLimit runs the engine with a maximum number of rule firings.
// A limit of 0 means unlimited. Context cancellation before the worker starts
// removes a queued run and returns a nil result with an error wrapping
// ctx.Err(). Once started, RunWithLimit waits for the cooperative worker
// response; context cancellation returns a partial RunResult with HaltRequested
// and an error wrapping ctx.Err(). Halt or Close interruption returns a partial
// (or zero) RunResult with HaltRequested and a nil error. A native terminal
// result or completed explicit limit wins first; at an internal batch boundary,
// caller context cancellation wins over simultaneous Halt or Close interruption.
func (p *PinnedEngine) RunWithLimit(ctx context.Context, limit int) (*RunResult, error) {
	response := pinnedCall(ctx, p, func(e *Engine) (*RunResult, error) {
		control := &pinnedRunControl{}
		p.activeRun.Store(control)
		defer p.activeRun.CompareAndSwap(control, nil)

		return e.runWithLimit(ctx, limit, func() bool {
			return control.halted.Load() || p.closed.Load()
		})
	})
	return response.value, response.err
}

// Step executes a single rule firing.
// Returns nil if the agenda is empty.
func (p *PinnedEngine) Step() (*FiredRule, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (*FiredRule, error) {
		return e.Step()
	})
	return response.value, response.err
}

// Halt requests that the currently active Run stop at the next batch boundary.
// It returns immediately, has no effect while the worker is idle or handling a
// non-Run operation, and does not latch onto queued or future runs.
func (p *PinnedEngine) Halt() {
	if control := p.activeRun.Load(); control != nil {
		control.halted.Store(true)
	}
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
	response := pinnedCall(context.Background(), p, func(e *Engine) ([]byte, error) {
		return e.Serialize(format)
	})
	return response.value, response.err
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
	response := pinnedCall(context.Background(), p, func(e *Engine) ([]RuleInfo, error) {
		return e.Rules(), nil
	})
	return response.value
}

// Templates returns the names of all registered templates.
func (p *PinnedEngine) Templates() []string {
	response := pinnedCall(context.Background(), p, func(e *Engine) ([]string, error) {
		return e.Templates(), nil
	})
	return response.value
}

// GetGlobal retrieves a global variable's value by name.
func (p *PinnedEngine) GetGlobal(name string) (any, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (any, error) {
		return e.GetGlobal(name)
	})
	return response.value, response.err
}

// CurrentModule returns the name of the current module.
func (p *PinnedEngine) CurrentModule() string {
	response := pinnedCall(context.Background(), p, func(e *Engine) (string, error) {
		return e.CurrentModule(), nil
	})
	return response.value
}

// Focus returns the module at the top of the focus stack.
func (p *PinnedEngine) Focus() (string, bool) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (valueOK[string], error) {
		name, ok := e.Focus()
		return valueOK[string]{value: name, ok: ok}, nil
	})
	return response.value.value, response.value.ok
}

// FocusStack returns the focus stack entries from bottom to top.
func (p *PinnedEngine) FocusStack() []string {
	response := pinnedCall(context.Background(), p, func(e *Engine) ([]string, error) {
		return e.FocusStack(), nil
	})
	return response.value
}

// AgendaSize returns the number of activations on the agenda.
func (p *PinnedEngine) AgendaSize() int {
	response := pinnedCall(context.Background(), p, func(e *Engine) (int, error) {
		return e.AgendaSize(), nil
	})
	return response.value
}

// IsHalted returns true if the engine is halted.
func (p *PinnedEngine) IsHalted() bool {
	response := pinnedCall(context.Background(), p, func(e *Engine) (bool, error) {
		return e.IsHalted(), nil
	})
	return response.value
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

// GetOutput retrieves captured output for a named channel. For backward
// compatibility it discards validation, dispatch, closed-state, and native
// errors; use GetOutputE when the error must be observed.
func (p *PinnedEngine) GetOutput(channel string) (string, bool) {
	value, ok, _ := p.GetOutputE(channel)
	return value, ok
}

// GetOutputE retrieves captured output for a named channel and reports
// validation, dispatch, closed-state, or native errors.
func (p *PinnedEngine) GetOutputE(channel string) (string, bool, error) {
	response := pinnedCall(context.Background(), p, func(e *Engine) (valueOK[string], error) {
		value, ok, err := e.GetOutputE(channel)
		return valueOK[string]{value: value, ok: ok}, err
	})
	return response.value.value, response.value.ok, response.err
}

// ClearOutput clears a specific output channel. For backward compatibility it
// discards validation, dispatch, closed-state, and native errors; use
// ClearOutputE when the error must be observed.
func (p *PinnedEngine) ClearOutput(channel string) {
	_ = p.ClearOutputE(channel)
}

// ClearOutputE clears a specific output channel and reports validation,
// dispatch, closed-state, or native errors.
func (p *PinnedEngine) ClearOutputE(channel string) error {
	return p.do(func(e *Engine) error {
		return e.ClearOutputE(channel)
	})
}

// PushInput pushes an input line for read/readline. For backward compatibility
// it discards validation, dispatch, closed-state, and native errors; use
// PushInputE when the error must be observed.
func (p *PinnedEngine) PushInput(line string) {
	_ = p.PushInputE(line)
}

// PushInputE pushes an input line for read/readline and reports validation,
// dispatch, closed-state, or native errors.
func (p *PinnedEngine) PushInputE(line string) error {
	return p.do(func(e *Engine) error {
		return e.PushInputE(line)
	})
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

// Diagnostics returns all action diagnostic messages from recent execution.
func (p *PinnedEngine) Diagnostics() []string {
	response := pinnedCall(context.Background(), p, func(e *Engine) ([]string, error) {
		return e.Diagnostics(), nil
	})
	return response.value
}

// ClearDiagnostics clears all stored action diagnostics.
func (p *PinnedEngine) ClearDiagnostics() {
	_ = p.do(func(e *Engine) error {
		e.ClearDiagnostics()
		return nil
	})
}

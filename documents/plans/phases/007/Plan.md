# Phase 007 Plan: Go Bindings

## Intent

Deliver production-quality Go bindings for ferric-rules, suitable for use in Temporal workflow activities. The bindings should be idiomatic Go, efficient (minimize copies), and correctly handle ferric's thread-affinity constraint.

## Background & Constraints

### Thread Affinity

Every `FerricEngine` instance is bound to the OS thread that created it. All subsequent calls must happen on that same thread, enforced at runtime (returns `FERRIC_ERROR_THREAD_VIOLATION`). This isn't just "serialized access"—it's same-thread-only.

### Existing C FFI Surface

The `ferric-ffi` crate exposes 57 `extern "C"` functions with:
- Opaque `FerricEngine*` handle (create → use → free lifecycle)
- Per-engine error state + thread-local global error state
- `FerricValue` union-type struct for all value types
- Buffer-copy pattern for strings (size-query → allocate → copy)
- `cbindgen`-generated `ferric.h` header

The C FFI is **frozen** (existing signatures don't change) but **extensible** (new functions can be added).

### Temporal Go SDK

- **Workflow code** is deterministic and cannot use goroutines, `runtime.LockOSThread()`, CGo, or I/O.
- **Activity code** is unrestricted—normal Go code. Goroutines, channels, CGo, thread locking are all fine.
- A Temporal worker processes multiple activities concurrently (default `MaxConcurrentActivityExecutionSize` is high). Each activity runs in its own goroutine.
- The rules engine will be used exclusively from activities, never from workflow code.

### Python Bindings as Reference

The `ferric-python` crate provides a high-level Pythonic API via PyO3. It wraps the Rust `Engine` directly (not through the C FFI). Key design choices worth mirroring:
- `Engine.from_source(source)` — one-shot create+load+reset
- Structured fact assertion: `engine.assert_fact("color", "red")` alongside string-based: `engine.assert_string("(color red)")`
- Rich `Fact` snapshot type with relation, fields, template info
- `RunResult` with rules_fired count and halt reason
- Error type hierarchy (parse, compile, runtime, not-found)

---

## Resolved Decisions

1. **Static linking only.** No dynamic linking support planned.

2. **Go 1.26** (current latest). Greenfield project targeting our own use case, no need for broad version compatibility. We can use all modern Go features (`iter.Seq`, etc.).

3. **Monorepo** during development (`bindings/go/` in the ferric-rules repo). Module path: `github.com/<org>/ferric-rules/bindings/go`. May migrate to standalone module later.

4. **Template fact API first.** Before starting Go bindings, add structured template assertion to the C FFI and Python bindings. Then the Go bindings can use it from day one.

5. **Coordinator + Manager architecture** (see Layer 3 below).

---

## Architecture Decision: CGo + Existing C FFI

**Chosen approach:** CGo calling the existing C FFI (`ferric.h`).

**Rationale:**
- The C FFI already exists, is well-tested, and handles thread-affinity enforcement
- CGo is the most mature Go-to-C interop mechanism (~30-40ns per-call overhead, negligible for engine operations)
- No additional Rust-side crate needed—just link against `libferric_ffi`
- `cbindgen` already generates the header; Go just needs `#include` and `#cgo LDFLAGS`
- Alternative (UniFFI/uniffi-bindgen-go) is less mature and would require rewriting the FFI surface

**What we do NOT need:** A new Rust crate. The Go bindings are pure Go code wrapping the C FFI.

---

## Package Structure

```
bindings/go/                          # Go module root (within ferric-rules monorepo)
├── go.mod                            # module github.com/<org>/ferric-rules/bindings/go
├── ferric.go                         # Package doc
├── engine.go                         # Engine type (low-level, single-thread)
├── engine_options.go                 # Functional options for engine creation
├── coordinator.go                    # Coordinator (owns thread pool, vends Managers)
├── coordinator_options.go            # Functional options for NewCoordinator
├── manager.go                        # Manager (per-engine-type handle, thin shim)
├── fact.go                           # Fact, FactType, value types
├── result.go                         # RunResult, HaltReason, FiredRule
├── errors.go                         # Error types (FerricError, ParseError, etc.)
├── values.go                         # Go ↔ FerricValue conversion
├── internal/
│   └── ffi/
│       ├── ffi.go                    # CGo declarations, raw C function wrappers
│       ├── types.go                  # Go mirrors of C types (FerricValue, etc.)
│       └── lib/                      # Pre-built library + header
│           ├── ferric.h              # Copied from ferric-ffi
│           └── libferric_ffi.a       # Static library (platform-specific)
├── temporal/                         # Optional sub-package for Temporal helpers
│   ├── activity.go                   # RulesActivity struct
│   └── activity_options.go           # Configuration
├── engine_test.go                    # Tests (+ per-file _test.go)
├── coordinator_test.go
├── manager_test.go
└── testdata/                         # CLIPS fixture files
```

This is a single Go module. The `internal/ffi` package is private; users only see the top-level `ferric` package and optionally `ferric/temporal`.

---

## Layer 1: Low-Level CGo Wrapper (`internal/ffi`)

### Purpose

Thin 1:1 wrappers around every C FFI function. Handles CGo type marshaling but no Go-idiomatic abstractions. This layer is private (`internal/`).

### Design

```go
package ffi

/*
#cgo LDFLAGS: -L${SRCDIR}/lib -lferric_ffi -lm -ldl -lpthread
#cgo darwin LDFLAGS: -framework Security -framework CoreFoundation
#include "lib/ferric.h"
#include <stdlib.h>
*/
import "C"
import "unsafe"

// EngineHandle wraps the opaque C pointer.
type EngineHandle = *C.FerricEngine

// Error codes mirrored from C.
type ErrorCode = C.enum_FerricError

const (
    ErrOK              ErrorCode = C.FERRIC_ERROR_OK
    ErrNullPointer     ErrorCode = C.FERRIC_ERROR_NULL_POINTER
    ErrThreadViolation ErrorCode = C.FERRIC_ERROR_THREAD_VIOLATION
    ErrNotFound        ErrorCode = C.FERRIC_ERROR_NOT_FOUND
    ErrParseError      ErrorCode = C.FERRIC_ERROR_PARSE_ERROR
    ErrCompileError    ErrorCode = C.FERRIC_ERROR_COMPILE_ERROR
    ErrRuntimeError    ErrorCode = C.FERRIC_ERROR_RUNTIME_ERROR
    ErrIOError         ErrorCode = C.FERRIC_ERROR_IO_ERROR
    ErrBufferTooSmall  ErrorCode = C.FERRIC_ERROR_BUFFER_TOO_SMALL
    ErrInvalidArgument ErrorCode = C.FERRIC_ERROR_INVALID_ARGUMENT
    ErrInternalError   ErrorCode = C.FERRIC_ERROR_INTERNAL_ERROR
)

func EngineNew() EngineHandle {
    return C.ferric_engine_new()
}

func EngineFree(h EngineHandle) ErrorCode {
    return C.ferric_engine_free(h)
}

func EngineLoadString(h EngineHandle, source string) ErrorCode {
    cs := C.CString(source)
    defer C.free(unsafe.Pointer(cs))
    return C.ferric_engine_load_string(h, cs)
}

// ... one wrapper per C function (57+ total)
```

### Key Patterns

- **String passing (Go → C):** `C.CString()` + `defer C.free()` for every input string.
- **String retrieval (C → Go):** Two-call buffer pattern: first call with `nil` buf to get size, then allocate and copy. Wrap this in a helper: `func getString(fn func(buf *C.char, bufLen C.uintptr_t, outLen *C.uintptr_t) ErrorCode) (string, ErrorCode)`.
- **Value conversion:** `C.FerricValue` ↔ Go types, with explicit free calls for owned strings/multifields.
- **No error wrapping at this level** — just return `ErrorCode`.

---

## Layer 2: Idiomatic Go API (`ferric` package)

This is the public API surface for direct engine interaction. The `Engine` type is the low-level building block used internally by the Coordinator's worker threads.

### Core Types

```go
package ferric

// Strategy is the conflict resolution strategy.
type Strategy int

const (
    StrategyDepth   Strategy = iota
    StrategyBreadth
    StrategyLEX
    StrategyMEA
)

// Encoding is the string encoding mode.
type Encoding int

const (
    EncodingASCII                   Encoding = iota
    EncodingUTF8
    EncodingASCIISymbolsUTF8Strings
)

// HaltReason describes why engine execution stopped.
type HaltReason int

const (
    HaltAgendaEmpty    HaltReason = iota
    HaltLimitReached
    HaltRequested
)

// FactType distinguishes ordered from template facts.
type FactType int

const (
    FactOrdered  FactType = iota
    FactTemplate
)
```

### Value Type

```go
// Symbol is a distinct type to differentiate CLIPS symbols from strings.
type Symbol string

// Value conversion from C FFI values produces:
//   Integer       → int64
//   Float         → float64
//   Symbol        → ferric.Symbol
//   String        → string
//   Multifield    → []any (recursive)
//   Void          → nil
//   ExternalAddr  → unsafe.Pointer (rare)
//
// Value conversion to C FFI values accepts:
//   int, int64    → Integer
//   float64       → Float
//   Symbol        → Symbol
//   string        → String (or Symbol based on context)
//   []any         → Multifield
//   bool          → Symbol (TRUE/FALSE)
//   nil           → Void
```

### Fact Type

```go
// Fact is an immutable snapshot of a fact in working memory.
type Fact struct {
    ID           uint64
    Type         FactType
    Relation     string            // non-empty for ordered facts
    TemplateName string            // non-empty for template facts
    Fields       []any             // ordered field values
    Slots        map[string]any    // template slot values (nil for ordered)
}
```

### RunResult Type

```go
// RunResult contains the outcome of an engine run.
type RunResult struct {
    RulesFired int
    HaltReason HaltReason
}

// FiredRule identifies a single rule that fired during a step.
type FiredRule struct {
    RuleName string
}

// RuleInfo describes a registered rule.
type RuleInfo struct {
    Name     string
    Salience int
}
```

### Error Types

```go
// FerricError is the base error type for all ferric operations.
type FerricError struct {
    Code    ErrorCode  // underlying C error code
    Message string     // human-readable description
}

func (e *FerricError) Error() string { return e.Message }

// Sentinel errors for errors.Is() support:
var (
    ErrParse    = errors.New("ferric: parse error")
    ErrCompile  = errors.New("ferric: compile error")
    ErrRuntime  = errors.New("ferric: runtime error")
    ErrNotFound = errors.New("ferric: not found")
    ErrEncoding = errors.New("ferric: encoding error")
)

// Concrete error types for errors.As() support:
type ParseError struct{ FerricError }
func (e *ParseError) Is(target error) bool { return target == ErrParse }

type CompileError struct{ FerricError }
func (e *CompileError) Is(target error) bool { return target == ErrCompile }

type RuntimeError struct{ FerricError }
func (e *RuntimeError) Is(target error) bool { return target == ErrRuntime }

// etc.
```

### Engine Type

The `Engine` type is the **low-level, thread-bound** wrapper. It is public so that advanced users can use it with manual `runtime.LockOSThread()` if they wish, but the normal path is through `Coordinator`/`Manager`.

```go
// Engine wraps a single ferric rules engine instance.
//
// An Engine is bound to the OS thread that created it. All methods
// must be called from that same thread. For concurrent or multi-engine
// use, use Coordinator and Manager instead.
//
// Engine implements io.Closer. Always defer Close() after creation.
type Engine struct {
    handle ffi.EngineHandle
    closed bool
}
```

#### Constructor (Functional Options)

```go
// EngineOption configures an Engine.
type EngineOption func(*engineConfig)

type engineConfig struct {
    strategy     Strategy
    encoding     Encoding
    maxCallDepth int
    source       string    // if non-empty, load+reset at creation
}

func WithStrategy(s Strategy) EngineOption   { ... }
func WithEncoding(e Encoding) EngineOption   { ... }
func WithMaxCallDepth(n int) EngineOption    { ... }
func WithSource(clips string) EngineOption   { ... }

// NewEngine creates a new engine on the current OS thread.
// The caller is responsible for thread affinity.
func NewEngine(opts ...EngineOption) (*Engine, error) { ... }
```

#### Methods

```go
// --- Loading ---
func (e *Engine) Load(source string) error
func (e *Engine) LoadFile(path string) error

// --- Fact Operations ---
func (e *Engine) AssertString(source string) (uint64, error)
func (e *Engine) AssertFact(relation string, fields ...any) (uint64, error)
func (e *Engine) AssertTemplate(templateName string, slots map[string]any) (uint64, error)
func (e *Engine) Retract(factID uint64) error
func (e *Engine) GetFact(factID uint64) (*Fact, error)
func (e *Engine) Facts() ([]Fact, error)
func (e *Engine) FindFacts(relation string) ([]Fact, error)
func (e *Engine) FactCount() (int, error)

// --- Execution ---
func (e *Engine) Run(ctx context.Context) (*RunResult, error)
func (e *Engine) RunWithLimit(ctx context.Context, limit int) (*RunResult, error)
func (e *Engine) Step() (*FiredRule, error)
func (e *Engine) Halt()
func (e *Engine) Reset() error
func (e *Engine) Clear()

// --- Introspection ---
func (e *Engine) Rules() []RuleInfo
func (e *Engine) Templates() []string
func (e *Engine) GetGlobal(name string) (any, error)
func (e *Engine) CurrentModule() string
func (e *Engine) Focus() (string, bool)
func (e *Engine) FocusStack() []string
func (e *Engine) AgendaSize() int
func (e *Engine) IsHalted() bool

// --- I/O ---
func (e *Engine) GetOutput(channel string) (string, bool)
func (e *Engine) ClearOutput(channel string)
func (e *Engine) PushInput(line string)

// --- Diagnostics ---
func (e *Engine) Diagnostics() []string
func (e *Engine) ClearDiagnostics()

// --- Lifecycle ---
func (e *Engine) Close() error    // implements io.Closer
```

#### Context Support for Run

The C FFI's `ferric_engine_run` blocks until completion. To support cancellation:
- For small limits, a single FFI call is fine.
- For unbounded or large runs, use `ferric_engine_step()` in a loop, checking `ctx.Err()` between steps. This gives cancellation granularity at the per-rule-firing level.

```go
func (e *Engine) RunWithLimit(ctx context.Context, limit int) (*RunResult, error) {
    if limit > 0 && limit <= smallRunThreshold {
        return e.runDirect(limit)
    }
    fired := 0
    for limit == 0 || fired < limit {
        if err := ctx.Err(); err != nil {
            return &RunResult{RulesFired: fired, HaltReason: HaltRequested}, err
        }
        result, err := e.stepInternal()
        if err != nil {
            return &RunResult{RulesFired: fired}, err
        }
        if result == nil {
            return &RunResult{RulesFired: fired, HaltReason: HaltAgendaEmpty}, nil
        }
        fired++
    }
    return &RunResult{RulesFired: fired, HaltReason: HaltLimitReached}, nil
}
```

#### Finalizer Safety Net

Finalizers in Go run on arbitrary threads. Since `ferric_engine_free` enforces thread affinity, a finalizer cannot use it. We add `ferric_engine_free_unchecked()` to the C FFI that skips the thread check (safe because in a finalizer, no other code holds a reference). This is a single new FFI function.

---

## Layer 3: Coordinator + Manager Architecture

### Overview

```
┌─────────────────────────────────────────────────────────┐
│                   Coordinator                            │
│                                                         │
│  Initialized with:                                      │
│    - Thread count (T)                                   │
│    - Engine specs: {"risk": opts, "pricing": opts, ...} │
│                                                         │
│  Owns: T worker goroutines (each LockOSThread)          │
│  Vends: Manager handles (cheap shims)                   │
│                                                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │  Thread 0    │  │  Thread 1    │  │  Thread 2    │    │
│  │  [Locked]    │  │  [Locked]    │  │  [Locked]    │    │
│  │             │  │             │  │             │     │
│  │ "risk" → E  │  │ "risk" → E  │  │ (lazy)      │     │
│  │ "pricing"→E │  │ (lazy)      │  │             │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
└──────────┬──────────────┬──────────────┬────────────────┘
           │              │              │
   ┌───────┴──────┐ ┌────┴─────┐ ┌──────┴──────┐
   │ Manager      │ │ Manager  │ │ Manager     │
   │ "risk"       │ │"pricing" │ │ "fraud"     │
   │ (shim)       │ │ (shim)   │ │ (shim)      │
   └──────────────┘ └──────────┘ └─────────────┘
```

- **Coordinator** is initialized once with the full inventory of engine specs (types + options). It owns the thread pool. Engine types are fixed at construction time — no dynamic registration.
- Each **worker thread** (locked OS thread) lazily instantiates engines by spec name on first use. A thread may hold 0..N engines (one per spec that has been dispatched to it).
- **Manager** is a cheap, lightweight shim: just a pointer back to the coordinator plus a spec name. Obtained from `coord.Manager("risk")`. Multiple goroutines can share a Manager safely — it's just a dispatch handle.

### EngineSpec

```go
// EngineSpec describes a named engine configuration.
// Used to tell the Coordinator what engine types exist.
type EngineSpec struct {
    Name    string          // unique identifier for this engine type
    Options []EngineOption  // options for engine construction
}
```

### Coordinator

```go
// Coordinator manages a pool of OS threads and a fixed set of
// engine types. Engines are lazily instantiated per-thread on
// first use. All engine operations are dispatched to the correct
// thread automatically.
//
// Create with NewCoordinator, obtain Managers with Manager(),
// shut down with Close().
type Coordinator struct {
    specs   map[string][]EngineOption  // engine spec registry (immutable after init)
    workers []*worker
    next    atomic.Uint64              // round-robin dispatch
    closed  atomic.Bool
}
```

#### Constructor

```go
// CoordinatorOption configures a Coordinator.
type CoordinatorOption func(*coordConfig)

type coordConfig struct {
    threads int  // number of OS threads (default: 1)
}

func Threads(n int) CoordinatorOption {
    return func(c *coordConfig) { c.threads = n }
}

// NewCoordinator creates a Coordinator with the given engine specs
// and thread pool configuration.
//
// All engine specs must be provided upfront. The set of engine types
// is fixed for the lifetime of the Coordinator.
func NewCoordinator(specs []EngineSpec, opts ...CoordinatorOption) (*Coordinator, error) {
    cfg := coordConfig{threads: 1}
    for _, opt := range opts {
        opt(&cfg)
    }

    c := &Coordinator{
        specs: make(map[string][]EngineOption, len(specs)),
    }
    for _, s := range specs {
        c.specs[s.Name] = s.Options
    }

    // Start worker goroutines
    c.workers = make([]*worker, cfg.threads)
    for i := range c.workers {
        w, err := newWorker(c.specs)
        if err != nil {
            c.Close() // clean up already-started workers
            return nil, fmt.Errorf("starting worker %d: %w", i, err)
        }
        c.workers[i] = w
    }
    return c, nil
}
```

#### Worker (internal)

Each worker runs on a locked OS thread and maintains a map of lazily-instantiated engines:

```go
type worker struct {
    specs    map[string][]EngineOption  // shared reference to spec registry
    engines  map[string]*Engine         // lazily instantiated per-spec
    requests chan workerRequest
    done     chan struct{}
}

type workerRequest struct {
    specName string
    fn       func(*Engine) error
    resp     chan error
}

func newWorker(specs map[string][]EngineOption) (*worker, error) {
    w := &worker{
        specs:    specs,
        engines:  make(map[string]*Engine),
        requests: make(chan workerRequest, 16),
        done:     make(chan struct{}),
    }

    initDone := make(chan error, 1)

    go func() {
        runtime.LockOSThread()
        defer runtime.UnlockOSThread()
        defer close(w.done)
        defer w.closeAllEngines()

        close(initDone) // thread is locked and ready

        for req := range w.requests {
            engine, err := w.getOrCreateEngine(req.specName)
            if err != nil {
                req.resp <- err
                continue
            }
            req.resp <- req.fn(engine)
        }
    }()

    if err := <-initDone; err != nil {
        return nil, err
    }
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
```

#### Manager

```go
// Manager is a handle for interacting with a specific engine type.
// It is a lightweight shim that dispatches operations to the
// Coordinator's thread pool. Safe for concurrent use.
//
// Obtain a Manager from Coordinator.Manager().
type Manager struct {
    coord    *Coordinator
    specName string
}

// Manager returns a Manager handle for the named engine spec.
// The spec must have been provided when creating the Coordinator.
// Managers are cheap and safe to share across goroutines.
func (c *Coordinator) Manager(specName string) (*Manager, error) {
    if _, ok := c.specs[specName]; !ok {
        return nil, fmt.Errorf("ferric: unknown engine spec %q", specName)
    }
    return &Manager{coord: c, specName: specName}, nil
}
```

#### Manager Operations

```go
// Do dispatches a function to an engine of this Manager's type.
// The function runs on a thread-locked worker goroutine. The Engine
// must not be retained beyond the closure's return.
func (m *Manager) Do(fn func(*Engine) error) error {
    if m.coord.closed.Load() {
        return errors.New("ferric: coordinator is closed")
    }
    w := m.coord.nextWorker()
    resp := make(chan error, 1)
    w.requests <- workerRequest{
        specName: m.specName,
        fn:       fn,
        resp:     resp,
    }
    return <-resp
}

// Evaluate resets the engine, asserts the given facts, runs to
// completion, and returns the resulting facts and output.
// This is the primary entry point for most use cases.
func (m *Manager) Evaluate(ctx context.Context, req *EvaluateRequest) (*EvaluateResult, error) {
    var result *EvaluateResult
    err := m.Do(func(e *Engine) error {
        // Reset (not Clear+Load) — rules stay compiled from the spec's WithSource
        if err := e.Reset(); err != nil {
            return err
        }
        // Assert request facts
        for _, f := range req.Facts {
            switch {
            case f.TemplateName != "" && f.Slots != nil:
                if _, err := e.AssertTemplate(f.TemplateName, f.Slots); err != nil {
                    return err
                }
            default:
                if _, err := e.AssertFact(f.Relation, f.Fields...); err != nil {
                    return err
                }
            }
        }
        // Run
        limit := req.Limit
        var runResult *RunResult
        var err error
        if limit > 0 {
            runResult, err = e.RunWithLimit(ctx, limit)
        } else {
            runResult, err = e.Run(ctx)
        }
        if err != nil {
            return err
        }
        // Collect results
        facts, err := e.Facts()
        if err != nil {
            return err
        }
        output := make(map[string]string)
        if s, ok := e.GetOutput("stdout"); ok {
            output["stdout"] = s
        }
        if s, ok := e.GetOutput("stderr"); ok {
            output["stderr"] = s
        }
        result = &EvaluateResult{
            RunResult: *runResult,
            Facts:     facts,
            Output:    output,
        }
        return nil
    })
    return result, err
}
```

#### Request/Result Types

```go
// EvaluateRequest describes facts to assert and evaluation parameters.
type EvaluateRequest struct {
    Facts []FactInput `json:"facts"`
    Limit int         `json:"limit,omitempty"` // 0 = unlimited
}

// FactInput describes a fact to assert.
type FactInput struct {
    // For ordered facts:
    Relation string `json:"relation,omitempty"`
    Fields   []any  `json:"fields,omitempty"`

    // For template facts:
    TemplateName string         `json:"template_name,omitempty"`
    Slots        map[string]any `json:"slots,omitempty"`
}

// EvaluateResult contains the full outcome of an evaluation.
type EvaluateResult struct {
    RunResult
    Facts  []Fact            `json:"facts"`
    Output map[string]string `json:"output,omitempty"`
}
```

All request/result types use JSON struct tags for Temporal serialization compatibility.

#### Coordinator Lifecycle

```go
// Close shuts down all worker goroutines and frees all engines.
// Blocks until all in-flight requests complete.
func (c *Coordinator) Close() error {
    if !c.closed.CompareAndSwap(false, true) {
        return nil
    }
    for _, w := range c.workers {
        if w != nil {
            close(w.requests)
        }
    }
    for _, w := range c.workers {
        if w != nil {
            <-w.done
        }
    }
    return nil
}
```

#### Convenience: NewManager (Simple Case)

For the common single-engine-type use case, provide a shorthand that creates a 1-thread coordinator internally:

```go
// NewManager creates a standalone Manager backed by a single
// dedicated thread. This is a convenience for simple use cases.
// For multiple engine types or higher concurrency, use NewCoordinator.
//
// The returned Manager must be closed when done (which closes the
// underlying coordinator).
func NewManager(opts ...EngineOption) (*StandaloneManager, error) {
    coord, err := NewCoordinator(
        []EngineSpec{{Name: "_default", Options: opts}},
    )
    if err != nil {
        return nil, err
    }
    mgr, _ := coord.Manager("_default") // can't fail, we just registered it
    return &StandaloneManager{coord: coord, Manager: mgr}, nil
}

// StandaloneManager wraps a Manager and owns its Coordinator.
type StandaloneManager struct {
    *Manager
    coord *Coordinator
}

func (sm *StandaloneManager) Close() error {
    return sm.coord.Close()
}
```

Usage:

```go
// Simple: single engine type, single thread
mgr, _ := ferric.NewManager(ferric.WithSource(rules))
defer mgr.Close()
result, _ := mgr.Evaluate(ctx, &ferric.EvaluateRequest{...})

// Full: multiple engine types sharing a thread pool
coord, _ := ferric.NewCoordinator(
    []ferric.EngineSpec{
        {Name: "risk", Options: []ferric.EngineOption{ferric.WithSource(riskRules)}},
        {Name: "pricing", Options: []ferric.EngineOption{ferric.WithSource(pricingRules)}},
    },
    ferric.Threads(4),
)
defer coord.Close()

riskMgr, _ := coord.Manager("risk")
pricingMgr, _ := coord.Manager("pricing")

// Both dispatch to the same 4 worker threads
result1, _ := riskMgr.Evaluate(ctx, riskReq)
result2, _ := pricingMgr.Evaluate(ctx, pricingReq)
```

---

## Layer 4: Temporal Integration (`ferric/temporal`)

Optional sub-package providing Temporal-idiomatic wrappers.

```go
package temporal

import (
    "context"
    "go.temporal.io/sdk/activity"
    "go.temporal.io/sdk/worker"
    "github.com/<org>/ferric-rules/bindings/go"
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
        mgr, _ := coord.Manager(s.Name)
        managers[s.Name] = mgr
    }
    return &RulesActivity{coord: coord, managers: managers}, nil
}

// Register registers per-spec Evaluate activities on a Temporal worker.
func (a *RulesActivity) Register(w worker.Worker) {
    for name, mgr := range a.managers {
        mgr := mgr // capture
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
```

### Usage in Temporal Worker

```go
func main() {
    c, _ := client.Dial(client.Options{})
    defer c.Close()

    w := worker.New(c, "rules-task-queue", worker.Options{})

    rulesActivity, err := temporal.NewRulesActivity(
        []ferric.EngineSpec{
            {Name: "risk", Options: []ferric.EngineOption{ferric.WithSource(riskRules)}},
            {Name: "pricing", Options: []ferric.EngineOption{ferric.WithSource(pricingRules)}},
        },
        ferric.Threads(4),
    )
    if err != nil { log.Fatal(err) }
    defer rulesActivity.Close()

    rulesActivity.Register(w)

    if err := w.Run(worker.InterruptCh()); err != nil {
        log.Fatal(err)
    }
}
```

### Usage from Workflow

```go
func MyWorkflow(ctx workflow.Context, input Input) (Output, error) {
    var result ferric.EvaluateResult
    err := workflow.ExecuteActivity(ctx, "ferric.Evaluate.risk", &ferric.EvaluateRequest{
        Facts: []ferric.FactInput{
            {Relation: "applicant-age", Fields: []any{35}},
            {Relation: "credit-score", Fields: []any{720}},
        },
    }).Get(ctx, &result)
    if err != nil {
        return Output{}, err
    }
    return Output{Decision: extractDecision(result)}, nil
}
```

### Temporal-Specific Considerations

1. **Serialization:** `EvaluateRequest` and `EvaluateResult` have JSON struct tags. All field types are JSON-primitive (strings, ints, maps, slices).

2. **Idempotency:** Activity retries re-execute safely. Each invocation resets the engine (clearing previous facts, preserving compiled rules).

3. **Heartbeating:** For long-running evaluations, the context-aware `Run()` respects Temporal's activity context cancellation. For very long runs, the step loop could add periodic `activity.RecordHeartbeat()`.

4. **No goroutine creation in workflows:** All engine work happens inside activities. Workflow code only issues `ExecuteActivity` calls.

---

## C FFI Additions Required

New functions to add to `ferric-ffi` (additive only, no changes to existing functions):

| Function | Purpose | Priority |
|----------|---------|----------|
| `ferric_engine_free_unchecked(engine)` | Free engine from any thread (for GC finalizers) | Required |
| `ferric_engine_assert_template(engine, template, names, values, count, out_id)` | Assert template fact with named slots | Required (also benefits Python) |
| `ferric_engine_get_fact_slot_by_name(engine, fact_id, slot_name, out_value)` | Get template fact slot by name | Required (also benefits Python) |
| `ferric_engine_eval_string(engine, expr, out_value)` | Evaluate a CLIPS expression, return value | Nice-to-have |
| `ferric_engine_facts_snapshot(engine, out_facts, out_count)` | Bulk-fetch all fact metadata in one call | Nice-to-have |

The "required" functions should be added early (Pass 1 or as a pre-pass). The "nice-to-have" functions can wait for later optimization.

---

## Implementation Passes

### Pre-Pass: Template Fact FFI + Python Bindings

**Goal:** Add structured template assertion to C FFI and Python bindings before starting Go work.

- Add `ferric_engine_assert_template` to `ferric-ffi`
- Add `ferric_engine_get_fact_slot_by_name` to `ferric-ffi`
- Add `ferric_engine_free_unchecked` to `ferric-ffi`
- Update Python bindings with `engine.assert_template("person", name="Alice", age=30)`
- Tests for all new FFI functions and Python methods

### Pass 1: Scaffold + Low-Level FFI (`internal/ffi`)

**Goal:** Working CGo build that can create/free an engine.

- Set up Go module structure under `bindings/go/`
- Copy `ferric.h` into `internal/ffi/lib/`
- Build `libferric_ffi.a` for the host platform
- Write `internal/ffi/ffi.go` with CGo preamble and all function wrappers
- Write `internal/ffi/types.go` with Go mirrors of C enums and structs
- Write string-buffer helper for the buffer-copy pattern
- Write `FerricValue` ↔ Go conversion helpers
- Test: create engine, load a rule, run, get output, free engine — all via raw FFI
- Add `just build-go-ffi` and `just test-go` targets

### Pass 2: Idiomatic Engine API

**Goal:** Public `ferric.Engine` type with full API.

- Implement `Engine` struct wrapping `ffi.EngineHandle`
- Implement functional options
- Implement all Engine methods
- Value conversion layer (Go `any` ↔ `FerricValue`)
- `Fact` snapshot construction from FFI queries
- Error type hierarchy with `errors.Is`/`errors.As` support
- `context.Context`-aware `Run`/`RunWithLimit` via step loop
- `Close()` + finalizer with `free_unchecked`
- Comprehensive tests mirroring the Python test suite structure

### Pass 3: Coordinator + Manager

**Goal:** Thread-safe Coordinator with lazy engine instantiation.

- Implement worker goroutine with `LockOSThread` and per-spec engine map
- Implement `Coordinator` with round-robin dispatch
- Implement `Manager` as thin dispatch shim
- Implement `Do()` for raw access, `Evaluate()` for one-shot use
- Engine reuse via `Reset()` (rules stay compiled)
- `NewManager()` convenience for single-engine case
- `Close()` with graceful shutdown
- Concurrent tests: multiple goroutines, multiple managers, shared threads
- Benchmark: throughput, latency distribution, lazy instantiation overhead

### Pass 4: Temporal Integration

**Goal:** `ferric/temporal` sub-package.

- Implement `RulesActivity` struct with per-spec activity registration
- Ensure request/result types serialize cleanly via Temporal's JSON codec
- Integration test with `testsuite.WorkflowTestSuite`
- Example: Temporal worker with multiple engine types

### Pass 5: Build System + Distribution

**Goal:** Easy consumption and CI.

- `justfile` targets for full build pipeline (Rust lib → Go tests)
- CI workflow: build Rust lib → run Go tests with `-race`
- Cross-compilation for darwin-arm64, darwin-amd64, linux-amd64
- Example programs: standalone evaluation, multi-engine coordinator, Temporal worker

### Pass 6: Polish + Optimization

**Goal:** Performance and ergonomics improvements.

- Add `ferric_engine_eval_string` and `ferric_engine_facts_snapshot` to FFI
- Bulk fact snapshot for reduced FFI round-trips
- `iter.Seq`-based iterators for facts, rules, etc.
- Performance benchmarks and optimization
- Go doc comments, package-level examples, usage guide

---

## Testing Strategy

| Level | What | How |
|-------|------|-----|
| FFI unit | Each C function wrapper works | `internal/ffi/ffi_test.go` |
| Engine unit | Each Engine method works | `engine_test.go`, `fact_test.go`, etc. (mirror Python tests) |
| Coordinator | Concurrent access, lazy instantiation, lifecycle | `coordinator_test.go` with `-race` |
| Manager | Dispatch correctness, multiple types | `manager_test.go` with `-race` |
| Temporal | Activity registration, execution, retry | `temporal/activity_test.go` via `testsuite` |
| End-to-end | Load real CLIPS programs, verify output | `testdata/*.clp` fixture files |

All tests run with `-race` in CI.

---

## Build & Linking Strategy

**Static linking** only:

```
#cgo LDFLAGS: -L${SRCDIR}/lib -lferric_ffi -lm -ldl -lpthread
#cgo darwin LDFLAGS: -framework Security -framework CoreFoundation
```

**Build workflow:**
1. `cargo build -p ferric-ffi --profile ffi-release` → `libferric_ffi.a`
2. Copy `libferric_ffi.a` + `ferric.h` into `bindings/go/internal/ffi/lib/`
3. `go test ./...` picks them up via CGo LDFLAGS

A `just build-go` target orchestrates this.

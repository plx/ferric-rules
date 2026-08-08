package ferric

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

var errIntOverflow = fmt.Errorf("ferric: integer overflow")

// Engine wraps a single ferric rules engine instance.
//
// An Engine is bound to the OS thread that created it. All methods
// must be called from that same thread. For concurrent or multi-engine
// use, use Coordinator and Manager instead.
//
// Engine implements io.Closer. Always defer Close() after creation.
// An Engine must not be copied after first use.
type Engine struct {
	lifecycle sync.RWMutex
	handle    ffi.EngineHandle
	closed    bool
}

// NewEngine creates a new engine on the current OS thread.
// The caller is responsible for ensuring thread affinity
// (e.g., via runtime.LockOSThread).
func NewEngine(opts ...EngineOption) (*Engine, error) {
	cfg := defaultEngineConfig()
	for _, opt := range opts {
		opt(&cfg)
	}

	// Build and validate the complete effective configuration before invoking
	// any native constructor. Snapshot restoration does not currently apply
	// these overrides, but explicitly supplied values must still be validated.
	config, err := makeConfig(&cfg)
	if err != nil {
		return nil, err
	}

	var h ffi.EngineHandle

	if cfg.hasSnapshot() {
		// Deserialize from snapshot — skips parse/compile.
		ffiFormat, err := formatToFFI(cfg.snapshotFormat)
		if err != nil {
			return nil, err
		}
		if len(cfg.snapshot) == 0 {
			return nil, &InvalidArgumentError{FerricError{
				Code:    int(ffi.ErrInvalidArgument),
				Message: "snapshot data is empty",
			}}
		}
		var rc ffi.ErrorCode
		h, rc = ffiEngineDeserializeAs(cfg.snapshot, ffiFormat)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, nil)
		}
		if h == nil {
			return nil, &FerricError{Message: "failed to create engine from snapshot"}
		}
	} else {
		if cfg.hasSource() {
			if cfg.hasEngineConfig() {
				h = ffiEngineNewWithSourceConfig(cfg.source, config)
			} else {
				h = ffiEngineNewWithSource(cfg.source)
			}
			if h == nil {
				msg := ffiLastErrorGlobal()
				if msg == "" {
					msg = "failed to create engine from source"
				}
				return nil, &ParseError{FerricError{Message: msg}}
			}
		} else {
			if cfg.hasEngineConfig() {
				h = ffiEngineNewWithConfig(config)
			} else {
				h = ffiEngineNew()
			}
			if h == nil {
				return nil, &FerricError{Message: "failed to create engine"}
			}
		}
	}

	e := &Engine{handle: h}
	runtime.SetFinalizer(e, finalizeEngine)
	return e, nil
}

func finalizeEngine(e *Engine) {
	_, _ = e.closeWith(ffiEngineFreeUnchecked)
}

// NewEngineFromFile creates a new engine by deserializing a snapshot from the
// given file path. The format must match the one used during serialization.
// Additional options (e.g., WithMaxCallDepth) are applied after restoration.
func NewEngineFromFile(path string, format Format, opts ...EngineOption) (*Engine, error) {
	data, err := os.ReadFile(filepath.Clean(path)) // #nosec G304 -- caller-controlled path
	if err != nil {
		return nil, fmt.Errorf("ferric: reading snapshot file: %w", err)
	}
	combined := append([]EngineOption{WithSnapshot(data, format)}, opts...)
	return NewEngine(combined...)
}

func makeConfig(cfg *engineConfig) (*ffi.Config, error) {
	encoding, err := toFFIStringEncoding(cfg.encoding)
	if err != nil {
		return nil, err
	}
	strategy, err := toFFIConflictStrategy(cfg.strategy)
	if err != nil {
		return nil, err
	}
	maxCallDepth, err := intToUintptr(cfg.maxCallDepth)
	if err != nil {
		return nil, err
	}

	return ffi.MakeConfig(encoding, strategy, maxCallDepth), nil
}

func toFFIStringEncoding(e Encoding) (ffi.StringEncoding, error) {
	switch e {
	case EncodingASCII:
		return ffi.StringEncodingASCII, nil
	case EncodingUTF8:
		return ffi.StringEncodingUTF8, nil
	case EncodingASCIISymbolsUTF8Strings:
		return ffi.StringEncodingASCIISymbolsUTF8Strings, nil
	default:
		return 0, fmt.Errorf("%w: unsupported encoding %d", ErrInvalidArgument, e)
	}
}

func toFFIConflictStrategy(s Strategy) (ffi.ConflictStrategy, error) {
	switch s {
	case StrategyDepth:
		return ffi.ConflictStrategyDepth, nil
	case StrategyBreadth:
		return ffi.ConflictStrategyBreadth, nil
	case StrategyLEX:
		return ffi.ConflictStrategyLEX, nil
	case StrategyMEA:
		return ffi.ConflictStrategyMEA, nil
	default:
		return 0, fmt.Errorf("%w: unsupported conflict strategy %d", ErrInvalidArgument, s)
	}
}

func intToUintptr(n int) (uintptr, error) {
	if n < 0 {
		return 0, fmt.Errorf("%w: negative max call depth %d", ErrInvalidArgument, n)
	}
	return uintptr(n), nil
}

func uint64ToInt(n uint64) (int, error) {
	maxInt := uint64(^uint(0) >> 1)
	if n > maxInt {
		return 0, fmt.Errorf("%w: uint64 value %d exceeds int", errIntOverflow, n)
	}
	return int(n), nil
}

func uintptrToInt(n uintptr) (int, error) {
	maxInt := uintptr(^uint(0) >> 1)
	if n > maxInt {
		return 0, fmt.Errorf("%w: uintptr value %d exceeds int", errIntOverflow, n)
	}
	return int(n), nil
}

func clampUintptrToInt(n uintptr) int {
	maxInt := uintptr(^uint(0) >> 1)
	if n > maxInt {
		return int(maxInt)
	}
	return int(n)
}

// Close frees the engine. Implements io.Closer.
func (e *Engine) Close() error {
	closed, err := e.closeWith(ffiEngineFree)
	if err != nil {
		return err
	}
	if closed {
		runtime.SetFinalizer(e, nil)
	}
	return nil
}

// closeWith serializes explicit and finalizer cleanup. The handle becomes nil
// only after the native free succeeds, so a failed thread-affine Close remains
// retryable while successful cleanup is published exactly once.
func (e *Engine) closeWith(free func(ffi.EngineHandle) ffi.ErrorCode) (bool, error) {
	e.lifecycle.Lock()
	defer e.lifecycle.Unlock()

	if e.closed {
		return false, nil
	}
	rc := free(e.handle)
	if rc != ffi.ErrOK {
		return false, errorFromFFI(rc, e.handle)
	}
	e.handle = nil
	e.closed = true
	return true, nil
}

// leaseHandle prevents Close from freeing the native engine until release is
// called. Every native operation must hold this lease for its complete FFI
// interaction, including error-message retrieval.
func (e *Engine) leaseHandle() (ffi.EngineHandle, func(), error) {
	e.lifecycle.RLock()
	if e.closed {
		e.lifecycle.RUnlock()
		return nil, nil, ErrEngineClosed
	}
	return e.handle, e.lifecycle.RUnlock, nil
}

// --- Loading ---

// Load loads CLIPS source into the engine.
func (e *Engine) Load(source string) error {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return err
	}
	defer release()

	rc := ffiEngineLoadString(handle, source)
	if rc != ffi.ErrOK {
		return errorFromFFI(rc, handle)
	}
	return nil
}

// --- Fact Operations ---

// AssertString asserts a fact from a CLIPS source string
// (e.g., "(assert (color red))").
func (e *Engine) AssertString(source string) (uint64, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return 0, err
	}
	defer release()

	id, rc := ffiEngineAssertString(handle, source)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, handle)
	}
	return id, nil
}

// AssertFact asserts an ordered fact with the given relation and fields.
// Slice values are recursively copied into temporary Ferric-owned multifields;
// the Go inputs remain caller-owned and are not retained by Ferric. Nested
// slices deeper than 128 levels return ErrInvalidArgument.
func (e *Engine) AssertFact(relation string, fields ...any) (uint64, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return 0, err
	}
	defer release()

	vals := make([]ffi.Value, len(fields))
	defer func() {
		for i := range vals {
			ffiValueFree(&vals[i])
		}
	}()
	for i, f := range fields {
		v, conversionErr := goToFFIValue(f)
		if conversionErr != nil {
			return 0, conversionErr
		}
		vals[i] = v
	}

	id, rc := ffiEngineAssertOrdered(handle, relation, vals)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, handle)
	}
	return id, nil
}

// AssertTemplate asserts a template fact with named slot values.
// Slice values are recursively copied into temporary Ferric-owned multifields;
// the Go inputs remain caller-owned and are not retained by Ferric. Nested
// slices deeper than 128 levels return ErrInvalidArgument.
func (e *Engine) AssertTemplate(templateName string, slots map[string]any) (uint64, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return 0, err
	}
	defer release()

	names := make([]string, 0, len(slots))
	vals := make([]ffi.Value, 0, len(slots))
	defer func() {
		for i := range vals {
			ffiValueFree(&vals[i])
		}
	}()
	for k, v := range slots {
		fv, conversionErr := goToFFIValue(v)
		if conversionErr != nil {
			return 0, conversionErr
		}
		names = append(names, k)
		vals = append(vals, fv)
	}

	id, rc := ffiEngineAssertTemplate(handle, templateName, names, vals)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, handle)
	}
	return id, nil
}

// Retract removes a fact by its ID.
func (e *Engine) Retract(factID uint64) error {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return err
	}
	defer release()

	rc := ffiEngineRetract(handle, factID)
	if rc != ffi.ErrOK {
		return errorFromFFI(rc, handle)
	}
	return nil
}

// GetFact returns a snapshot of a single fact.
func (e *Engine) GetFact(factID uint64) (*Fact, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	return e.buildFact(handle, factID)
}

// Facts returns snapshots of all user-visible facts.
func (e *Engine) Facts() ([]Fact, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	ids, rc := ffiEngineFactIDs(handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	return e.buildFacts(handle, ids)
}

func (e *Engine) buildFacts(handle ffi.EngineHandle, ids []uint64) ([]Fact, error) {
	facts := make([]Fact, 0, len(ids))
	for _, id := range ids {
		f, err := e.buildFact(handle, id)
		if err != nil {
			return nil, err
		}
		facts = append(facts, *f)
	}
	return facts, nil
}

// FindFacts returns snapshots of facts matching the given relation name.
func (e *Engine) FindFacts(relation string) ([]Fact, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	ids, rc := ffiEngineFindFactIDs(handle, relation)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	return e.buildFacts(handle, ids)
}

// FactCount returns the number of user-visible facts.
func (e *Engine) FactCount() (int, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return 0, err
	}
	defer release()

	count, rc := ffiEngineFactCount(handle)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, handle)
	}
	return uintptrToInt(count)
}

// --- Execution ---

// Run runs the engine to completion, checking context for cancellation.
func (e *Engine) Run(ctx context.Context) (*RunResult, error) {
	return e.RunWithLimit(ctx, 0)
}

// RunWithLimit runs the engine with a maximum number of rule firings.
// A limit of 0 means unlimited. Checks context for cancellation between
// batches of rule firings. Cancellation returns the partial RunResult with
// HaltRequested and an error wrapping ctx.Err(); an engine-requested halt
// returns HaltRequested with a nil error.
func (e *Engine) RunWithLimit(ctx context.Context, limit int) (*RunResult, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	if ctx == nil {
		return nil, errNilContext
	}

	// If context has no deadline/cancel, use a single direct FFI call.
	if ctx.Done() == nil {
		ffiLimit := int64(-1)
		if limit > 0 {
			ffiLimit = int64(limit)
		}
		return e.runDirect(handle, ffiLimit)
	}
	return e.runCancelable(ctx, handle, limit)
}

func (e *Engine) runCancelable(ctx context.Context, handle ffi.EngineHandle, limit int) (*RunResult, error) {
	// For cancelable contexts, run in small batches and check context.
	const batchSize = 100
	totalFired := 0
	runChunk := ffiEngineRunEx
	for {
		if err := ctx.Err(); err != nil {
			return &RunResult{RulesFired: totalFired, HaltReason: HaltRequested}, fmt.Errorf("ferric: run canceled: %w", err)
		}

		// Compute batch limit.
		batch := int64(batchSize)
		if limit > 0 {
			remaining := int64(limit - totalFired)
			if remaining <= 0 {
				return &RunResult{RulesFired: totalFired, HaltReason: HaltLimitReached}, nil
			}
			if remaining < batch {
				batch = remaining
			}
		}

		fired, reason, rc := runChunk(handle, batch)
		if rc != ffi.ErrOK {
			return &RunResult{RulesFired: totalFired}, errorFromFFI(rc, handle)
		}
		firedCount, err := uint64ToInt(fired)
		if err != nil {
			return &RunResult{RulesFired: totalFired}, err
		}
		totalFired += firedCount

		switch reason {
		case ffi.HaltReasonAgendaEmpty:
			return &RunResult{RulesFired: totalFired, HaltReason: HaltAgendaEmpty}, nil
		case ffi.HaltReasonHaltRequested:
			return &RunResult{RulesFired: totalFired, HaltReason: HaltRequested}, nil
		case ffi.HaltReasonActionError:
			return &RunResult{RulesFired: totalFired, HaltReason: HaltActionError}, nil
		case ffi.HaltReasonLimitReached:
			// Batch limit reached — continue if we haven't hit total limit.
			if limit > 0 && totalFired >= limit {
				return &RunResult{RulesFired: totalFired, HaltReason: HaltLimitReached}, nil
			}
			if err := ctx.Err(); err != nil {
				return &RunResult{RulesFired: totalFired, HaltReason: HaltRequested}, fmt.Errorf("ferric: run canceled: %w", err)
			}
			halted, rc := ffiEngineIsHalted(handle)
			if rc != ffi.ErrOK {
				return &RunResult{RulesFired: totalFired}, errorFromFFI(rc, handle)
			}
			if halted {
				return &RunResult{RulesFired: totalFired, HaltReason: HaltRequested}, nil
			}
			// Otherwise preserve this logical run and check context before the
			// next continuation chunk.
			runChunk = ffiEngineContinueRunEx
		}
	}
}

func (e *Engine) runDirect(handle ffi.EngineHandle, limit int64) (*RunResult, error) {
	fired, reason, rc := ffiEngineRunEx(handle, limit)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	var hr HaltReason
	switch reason {
	case ffi.HaltReasonAgendaEmpty:
		hr = HaltAgendaEmpty
	case ffi.HaltReasonLimitReached:
		hr = HaltLimitReached
	case ffi.HaltReasonHaltRequested:
		hr = HaltRequested
	case ffi.HaltReasonActionError:
		hr = HaltActionError
	}
	firedCount, err := uint64ToInt(fired)
	if err != nil {
		return nil, err
	}
	return &RunResult{RulesFired: firedCount, HaltReason: hr}, nil
}

// Step executes a single rule firing.
// Returns nil if the agenda is empty.
func (e *Engine) Step() (*FiredRule, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	status, rc := ffiEngineStep(handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	if status != 1 {
		return nil, nil //nolint:nilnil // nil indicates agenda empty and is part of Step's public contract.
	}
	// The C FFI doesn't currently return the rule name from step.
	return &FiredRule{}, nil
}

// Halt requests the engine to halt. It is a no-op after Close.
func (e *Engine) Halt() {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return
	}
	defer release()

	ffiEngineHalt(handle)
}

// Reset resets the engine to its initial state (facts cleared, rules kept).
func (e *Engine) Reset() error {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return err
	}
	defer release()

	rc := ffiEngineReset(handle)
	if rc != ffi.ErrOK {
		return errorFromFFI(rc, handle)
	}
	return nil
}

// Clear removes all rules, facts, templates, etc. from the engine. It is a
// no-op after Close.
func (e *Engine) Clear() {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return
	}
	defer release()

	ffiEngineClear(handle)
}

// Serialize produces a snapshot of the engine's current state using the
// specified format. The snapshot can be used with WithSnapshot to create
// new engines that skip the parse/compile pipeline.
func (e *Engine) Serialize(format Format) ([]byte, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	ffiFormat, err := formatToFFI(format)
	if err != nil {
		return nil, err
	}
	data, rc := ffiEngineSerializeAs(handle, ffiFormat)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	return data, nil
}

// SerializeToFile writes a serialized snapshot of the engine to the given
// file path using the specified format.
func (e *Engine) SerializeToFile(path string, format Format) error {
	data, err := e.Serialize(format)
	if err != nil {
		return err
	}
	if err = os.WriteFile(filepath.Clean(path), data, 0600); err != nil { // #nosec G306
		return fmt.Errorf("ferric: writing snapshot file: %w", err)
	}
	return nil
}

// --- Introspection ---

// Rules returns information about all registered rules. It returns nil after
// Close; use RulesE to distinguish a closed engine from an empty rule set.
func (e *Engine) Rules() []RuleInfo {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil
	}
	defer release()

	count, rc := ffiEngineRuleCount(handle)
	if rc != ffi.ErrOK {
		return nil
	}
	rules := make([]RuleInfo, 0, count)
	for i := range count {
		name, salience, rc := ffiEngineRuleInfo(handle, i)
		if rc != ffi.ErrOK {
			break
		}
		rules = append(rules, RuleInfo{Name: name, Salience: int(salience)})
	}
	return rules
}

// Templates returns the names of all registered templates. It returns nil
// after Close; use TemplatesE to distinguish a closed engine from no templates.
func (e *Engine) Templates() []string {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil
	}
	defer release()

	count, rc := ffiEngineTemplateCount(handle)
	if rc != ffi.ErrOK {
		return nil
	}
	names := make([]string, 0, count)
	for i := range count {
		name, rc := ffiEngineTemplateName(handle, i)
		if rc != ffi.ErrOK {
			break
		}
		names = append(names, name)
	}
	return names
}

// GetGlobal retrieves a global variable's value by name.
// The name should not include the ?* prefix/suffix.
func (e *Engine) GetGlobal(name string) (any, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	val, rc := ffiEngineGetGlobal(handle, name)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	result := ffiValueToGoAndFree(&val)
	return result, nil
}

// CurrentModule returns the name of the current module, or an empty string
// after Close. Use CurrentModuleE to distinguish that state from an empty name.
func (e *Engine) CurrentModule() string {
	name, _ := e.CurrentModuleE()
	return name
}

// Focus returns the module at the top of the focus stack.
// Returns empty string and false if the focus stack is empty or the engine is
// closed. Use FocusE to distinguish an error.
func (e *Engine) Focus() (string, bool) {
	name, ok, _ := e.FocusE()
	return name, ok
}

// FocusStack returns the focus stack entries from bottom to top, or nil after
// Close. Use FocusStackE to distinguish an error.
func (e *Engine) FocusStack() []string {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil
	}
	defer release()

	depth, rc := ffiEngineFocusStackDepth(handle)
	if rc != ffi.ErrOK {
		return nil
	}
	stack := make([]string, 0, depth)
	for i := range depth {
		name, rc := ffiEngineFocusStackEntry(handle, i)
		if rc != ffi.ErrOK {
			break
		}
		stack = append(stack, name)
	}
	return stack
}

// AgendaSize returns the number of activations on the agenda, or zero after
// Close. Use AgendaSizeE to distinguish an error.
func (e *Engine) AgendaSize() int {
	count, _ := e.AgendaSizeE()
	return count
}

// IsHalted returns true if the engine is halted and false after Close. Use
// IsHaltedE to distinguish an error.
func (e *Engine) IsHalted() bool {
	halted, _ := e.IsHaltedE()
	return halted
}

// --- I/O ---

// GetOutput retrieves captured output for a named channel.
// Returns the output string and true, or empty string and false if no output.
func (e *Engine) GetOutput(channel string) (string, bool) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return "", false
	}
	defer release()

	return ffiEngineGetOutput(handle, channel)
}

// ClearOutput clears a specific output channel. It is a no-op after Close.
func (e *Engine) ClearOutput(channel string) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return
	}
	defer release()

	ffiEngineClearOutput(handle, channel)
}

// PushInput pushes an input line for read/readline. It is a no-op after Close.
func (e *Engine) PushInput(line string) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return
	}
	defer release()

	ffiEnginePushInput(handle, line)
}

// --- Diagnostics ---

// Diagnostics returns all action diagnostic messages from recent execution. It
// returns nil after Close; use DiagnosticsE to distinguish an error.
func (e *Engine) Diagnostics() []string {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil
	}
	defer release()

	count, rc := ffiEngineActionDiagnosticCount(handle)
	if rc != ffi.ErrOK {
		return nil
	}
	diags := make([]string, 0, count)
	for i := range count {
		msg, rc := ffiEngineActionDiagnosticCopy(handle, i)
		if rc != ffi.ErrOK {
			break
		}
		diags = append(diags, msg)
	}
	return diags
}

// ClearDiagnostics clears all stored action diagnostics. It is a no-op after
// Close.
func (e *Engine) ClearDiagnostics() {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return
	}
	defer release()

	ffiEngineClearActionDiagnostics(handle)
}

// ---------------------------------------------------------------------------
// Error-aware introspection variants
// ---------------------------------------------------------------------------

// RulesE returns information about all registered rules, or an error.
func (e *Engine) RulesE() ([]RuleInfo, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	count, rc := ffiEngineRuleCount(handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	rules := make([]RuleInfo, 0, count)
	for i := range count {
		name, salience, rc := ffiEngineRuleInfo(handle, i)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, handle)
		}
		rules = append(rules, RuleInfo{Name: name, Salience: int(salience)})
	}
	return rules, nil
}

// TemplatesE returns the names of all registered templates, or an error.
func (e *Engine) TemplatesE() ([]string, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	count, rc := ffiEngineTemplateCount(handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	names := make([]string, 0, count)
	for i := range count {
		name, rc := ffiEngineTemplateName(handle, i)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, handle)
		}
		names = append(names, name)
	}
	return names, nil
}

// DiagnosticsE returns all action diagnostic messages, or an error.
func (e *Engine) DiagnosticsE() ([]string, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	count, rc := ffiEngineActionDiagnosticCount(handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	diags := make([]string, 0, count)
	for i := range count {
		msg, rc := ffiEngineActionDiagnosticCopy(handle, i)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, handle)
		}
		diags = append(diags, msg)
	}
	return diags, nil
}

// CurrentModuleE returns the name of the current module, or an error.
func (e *Engine) CurrentModuleE() (string, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return "", err
	}
	defer release()

	name, rc := ffiEngineCurrentModule(handle)
	if rc != ffi.ErrOK {
		return "", errorFromFFI(rc, handle)
	}
	return name, nil
}

// FocusE returns the module at the top of the focus stack, or an error.
// Returns ("", false, nil) when the result cannot be distinguished from
// an empty stack without an error; callers should check the bool first.
func (e *Engine) FocusE() (string, bool, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return "", false, err
	}
	defer release()

	name, rc := ffiEngineGetFocus(handle)
	if rc != ffi.ErrOK {
		return "", false, errorFromFFI(rc, handle)
	}
	return name, true, nil
}

// FocusStackE returns the focus stack entries from bottom to top, or an error.
func (e *Engine) FocusStackE() ([]string, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return nil, err
	}
	defer release()

	depth, rc := ffiEngineFocusStackDepth(handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}
	stack := make([]string, 0, depth)
	for i := range depth {
		name, rc := ffiEngineFocusStackEntry(handle, i)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, handle)
		}
		stack = append(stack, name)
	}
	return stack, nil
}

// AgendaSizeE returns the number of activations on the agenda, or an error.
func (e *Engine) AgendaSizeE() (int, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return 0, err
	}
	defer release()

	count, rc := ffiEngineAgendaCount(handle)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, handle)
	}
	return clampUintptrToInt(count), nil
}

// IsHaltedE returns whether the engine is halted, or an error.
func (e *Engine) IsHaltedE() (bool, error) {
	handle, release, err := e.leaseHandle()
	if err != nil {
		return false, err
	}
	defer release()

	halted, rc := ffiEngineIsHalted(handle)
	if rc != ffi.ErrOK {
		return false, errorFromFFI(rc, handle)
	}
	return halted, nil
}

// --- Internal: fact building ---

func (e *Engine) buildFact(handle ffi.EngineHandle, factID uint64) (*Fact, error) {
	ft, rc := ffiEngineGetFactType(handle, factID)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}

	fieldCount, rc := ffiEngineGetFactFieldCount(handle, factID)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, handle)
	}

	fields := make([]any, fieldCount)
	for i := range fieldCount {
		val, rc := ffiEngineGetFactField(handle, factID, i)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, handle)
		}
		fields[i] = ffiValueToGoAndFree(&val)
	}

	fact := &Fact{
		ID:     factID,
		Fields: fields,
	}

	if ft == ffi.FactTypeTemplate {
		fact.Type = FactTemplate
		name, rc := ffiEngineGetFactTemplateName(handle, factID)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, handle)
		}
		fact.TemplateName = name

		// Build slot map by querying template slot names.
		slotCount, rc := ffiEngineTemplateSlotCount(handle, name)
		if rc != ffi.ErrOK {
			return nil, fmt.Errorf("ferric: failed to get slot count for template %q: %w", name, errorFromFFI(rc, handle))
		}
		if slotCount > 0 {
			fact.Slots = make(map[string]any, slotCount)
			for i := range slotCount {
				slotName, rc := ffiEngineTemplateSlotName(handle, name, i)
				if rc != ffi.ErrOK {
					return nil, fmt.Errorf("ferric: failed to get slot name %d for template %q: %w", i, name, errorFromFFI(rc, handle))
				}
				if i < fieldCount {
					fact.Slots[slotName] = fields[i]
				}
			}
		}
	} else {
		fact.Type = FactOrdered
		rel, rc := ffiEngineGetFactRelation(handle, factID)
		if rc != ffi.ErrOK {
			return nil, fmt.Errorf("ferric: failed to get relation for fact %d: %w", factID, errorFromFFI(rc, handle))
		}
		fact.Relation = rel
	}

	return fact, nil
}

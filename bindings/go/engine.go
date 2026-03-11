package ferric

import (
	"context"
	"runtime"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

// smallRunThreshold is the maximum rule limit for a direct (non-stepping)
// run call. Below this, we skip per-step context checking.
const smallRunThreshold = 100

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

// NewEngine creates a new engine on the current OS thread.
// The caller is responsible for ensuring thread affinity
// (e.g., via runtime.LockOSThread).
func NewEngine(opts ...EngineOption) (*Engine, error) {
	cfg := engineConfig{
		maxCallDepth: 256,
	}
	for _, opt := range opts {
		opt(&cfg)
	}

	var h ffi.EngineHandle
	if cfg.source != "" {
		if cfg.strategy != 0 || cfg.encoding != 0 || cfg.maxCallDepth != 256 {
			h = ffi.EngineNewWithSourceConfig(cfg.source, makeConfig(&cfg))
		} else {
			h = ffi.EngineNewWithSource(cfg.source)
		}
		if h == nil {
			msg := ffi.LastErrorGlobal()
			if msg == "" {
				msg = "failed to create engine from source"
			}
			return nil, &ParseError{FerricError{Message: msg}}
		}
	} else {
		if cfg.strategy != 0 || cfg.encoding != 0 || cfg.maxCallDepth != 256 {
			h = ffi.EngineNewWithConfig(makeConfig(&cfg))
		} else {
			h = ffi.EngineNew()
		}
		if h == nil {
			return nil, &FerricError{Message: "failed to create engine"}
		}
	}

	e := &Engine{handle: h}
	runtime.SetFinalizer(e, func(e *Engine) {
		if !e.closed {
			ffi.EngineFreeUnchecked(e.handle)
		}
	})
	return e, nil
}

func makeConfig(cfg *engineConfig) *ffi.Config {
	return ffi.MakeConfig(
		ffi.StringEncoding(cfg.encoding),
		ffi.ConflictStrategy(cfg.strategy),
		uintptr(cfg.maxCallDepth),
	)
}

// Close frees the engine. Implements io.Closer.
func (e *Engine) Close() error {
	if e.closed {
		return nil
	}
	e.closed = true
	runtime.SetFinalizer(e, nil)
	rc := ffi.EngineFree(e.handle)
	if rc != ffi.ErrOK {
		return errorFromFFI(rc, e.handle)
	}
	return nil
}

// --- Loading ---

// Load loads CLIPS source into the engine.
func (e *Engine) Load(source string) error {
	rc := ffi.EngineLoadString(e.handle, source)
	if rc != ffi.ErrOK {
		return errorFromFFI(rc, e.handle)
	}
	return nil
}

// --- Fact Operations ---

// AssertString asserts a fact from a CLIPS source string
// (e.g., "(assert (color red))").
func (e *Engine) AssertString(source string) (uint64, error) {
	id, rc := ffi.EngineAssertString(e.handle, source)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, e.handle)
	}
	return id, nil
}

// AssertFact asserts an ordered fact with the given relation and fields.
func (e *Engine) AssertFact(relation string, fields ...any) (uint64, error) {
	vals := make([]ffi.Value, len(fields))
	for i, f := range fields {
		v, err := goToFFIValue(f)
		if err != nil {
			return 0, err
		}
		vals[i] = v
	}

	id, rc := ffi.EngineAssertOrdered(e.handle, relation, vals)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, e.handle)
	}
	return id, nil
}

// AssertTemplate asserts a template fact with named slot values.
func (e *Engine) AssertTemplate(templateName string, slots map[string]any) (uint64, error) {
	names := make([]string, 0, len(slots))
	vals := make([]ffi.Value, 0, len(slots))
	for k, v := range slots {
		fv, err := goToFFIValue(v)
		if err != nil {
			return 0, err
		}
		names = append(names, k)
		vals = append(vals, fv)
	}

	id, rc := ffi.EngineAssertTemplate(e.handle, templateName, names, vals)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, e.handle)
	}
	return id, nil
}

// Retract removes a fact by its ID.
func (e *Engine) Retract(factID uint64) error {
	rc := ffi.EngineRetract(e.handle, factID)
	if rc != ffi.ErrOK {
		return errorFromFFI(rc, e.handle)
	}
	return nil
}

// GetFact returns a snapshot of a single fact.
func (e *Engine) GetFact(factID uint64) (*Fact, error) {
	return e.buildFact(factID)
}

// Facts returns snapshots of all user-visible facts.
func (e *Engine) Facts() ([]Fact, error) {
	ids, rc := ffi.EngineFactIDs(e.handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, e.handle)
	}
	facts := make([]Fact, 0, len(ids))
	for _, id := range ids {
		f, err := e.buildFact(id)
		if err != nil {
			return nil, err
		}
		facts = append(facts, *f)
	}
	return facts, nil
}

// FindFacts returns snapshots of facts matching the given relation name.
func (e *Engine) FindFacts(relation string) ([]Fact, error) {
	ids, rc := ffi.EngineFindFactIDs(e.handle, relation)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, e.handle)
	}
	facts := make([]Fact, 0, len(ids))
	for _, id := range ids {
		f, err := e.buildFact(id)
		if err != nil {
			return nil, err
		}
		facts = append(facts, *f)
	}
	return facts, nil
}

// FactCount returns the number of user-visible facts.
func (e *Engine) FactCount() (int, error) {
	count, rc := ffi.EngineFactCount(e.handle)
	if rc != ffi.ErrOK {
		return 0, errorFromFFI(rc, e.handle)
	}
	return int(count), nil
}

// --- Execution ---

// Run runs the engine to completion, checking context for cancellation.
func (e *Engine) Run(ctx context.Context) (*RunResult, error) {
	return e.RunWithLimit(ctx, 0)
}

// RunWithLimit runs the engine with a maximum number of rule firings.
// A limit of 0 means unlimited. Checks context for cancellation between
// batches of rule firings.
func (e *Engine) RunWithLimit(ctx context.Context, limit int) (*RunResult, error) {
	if ctx == nil {
		ctx = context.Background()
	}

	// If context has no deadline/cancel, use a single direct FFI call.
	if ctx.Done() == nil {
		ffiLimit := int64(-1)
		if limit > 0 {
			ffiLimit = int64(limit)
		}
		return e.runDirect(ffiLimit)
	}

	// For cancelable contexts, run in small batches and check context.
	const batchSize = 100
	totalFired := 0
	for {
		if err := ctx.Err(); err != nil {
			return &RunResult{RulesFired: totalFired, HaltReason: HaltRequested}, err
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

		fired, reason, rc := ffi.EngineRunEx(e.handle, batch)
		if rc != ffi.ErrOK {
			return &RunResult{RulesFired: totalFired}, errorFromFFI(rc, e.handle)
		}
		totalFired += int(fired)

		switch reason {
		case ffi.HaltReasonAgendaEmpty:
			return &RunResult{RulesFired: totalFired, HaltReason: HaltAgendaEmpty}, nil
		case ffi.HaltReasonHaltRequested:
			return &RunResult{RulesFired: totalFired, HaltReason: HaltRequested}, nil
		case ffi.HaltReasonLimitReached:
			// Batch limit reached — continue if we haven't hit total limit.
			if limit > 0 && totalFired >= limit {
				return &RunResult{RulesFired: totalFired, HaltReason: HaltLimitReached}, nil
			}
			// Otherwise loop and check context before next batch.
		}
	}
}

func (e *Engine) runDirect(limit int64) (*RunResult, error) {
	fired, reason, rc := ffi.EngineRunEx(e.handle, limit)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, e.handle)
	}
	var hr HaltReason
	switch reason {
	case ffi.HaltReasonAgendaEmpty:
		hr = HaltAgendaEmpty
	case ffi.HaltReasonLimitReached:
		hr = HaltLimitReached
	case ffi.HaltReasonHaltRequested:
		hr = HaltRequested
	}
	return &RunResult{RulesFired: int(fired), HaltReason: hr}, nil
}

// Step executes a single rule firing.
// Returns nil if the agenda is empty.
func (e *Engine) Step() (*FiredRule, error) {
	status, rc := ffi.EngineStep(e.handle)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, e.handle)
	}
	if status != 1 {
		return nil, nil
	}
	// The C FFI doesn't currently return the rule name from step.
	return &FiredRule{}, nil
}

// Halt requests the engine to halt.
func (e *Engine) Halt() {
	ffi.EngineHalt(e.handle)
}

// Reset resets the engine to its initial state (facts cleared, rules kept).
func (e *Engine) Reset() error {
	rc := ffi.EngineReset(e.handle)
	if rc != ffi.ErrOK {
		return errorFromFFI(rc, e.handle)
	}
	return nil
}

// Clear removes all rules, facts, templates, etc. from the engine.
func (e *Engine) Clear() {
	ffi.EngineClear(e.handle)
}

// --- Introspection ---

// Rules returns information about all registered rules.
func (e *Engine) Rules() []RuleInfo {
	count, rc := ffi.EngineRuleCount(e.handle)
	if rc != ffi.ErrOK {
		return nil
	}
	rules := make([]RuleInfo, 0, count)
	for i := range count {
		name, salience, rc := ffi.EngineRuleInfo(e.handle, i)
		if rc != ffi.ErrOK {
			break
		}
		rules = append(rules, RuleInfo{Name: name, Salience: int(salience)})
	}
	return rules
}

// Templates returns the names of all registered templates.
func (e *Engine) Templates() []string {
	count, rc := ffi.EngineTemplateCount(e.handle)
	if rc != ffi.ErrOK {
		return nil
	}
	names := make([]string, 0, count)
	for i := range count {
		name, rc := ffi.EngineTemplateName(e.handle, i)
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
	val, rc := ffi.EngineGetGlobal(e.handle, name)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, e.handle)
	}
	result := ffiValueToGoAndFree(&val)
	return result, nil
}

// CurrentModule returns the name of the current module.
func (e *Engine) CurrentModule() string {
	name, rc := ffi.EngineCurrentModule(e.handle)
	if rc != ffi.ErrOK {
		return ""
	}
	return name
}

// Focus returns the module at the top of the focus stack.
// Returns empty string and false if the focus stack is empty.
func (e *Engine) Focus() (string, bool) {
	name, rc := ffi.EngineGetFocus(e.handle)
	if rc != ffi.ErrOK {
		return "", false
	}
	return name, true
}

// FocusStack returns the focus stack entries from bottom to top.
func (e *Engine) FocusStack() []string {
	depth, rc := ffi.EngineFocusStackDepth(e.handle)
	if rc != ffi.ErrOK {
		return nil
	}
	stack := make([]string, 0, depth)
	for i := range depth {
		name, rc := ffi.EngineFocusStackEntry(e.handle, i)
		if rc != ffi.ErrOK {
			break
		}
		stack = append(stack, name)
	}
	return stack
}

// AgendaSize returns the number of activations on the agenda.
func (e *Engine) AgendaSize() int {
	count, rc := ffi.EngineAgendaCount(e.handle)
	if rc != ffi.ErrOK {
		return 0
	}
	return int(count)
}

// IsHalted returns true if the engine is halted.
func (e *Engine) IsHalted() bool {
	halted, rc := ffi.EngineIsHalted(e.handle)
	if rc != ffi.ErrOK {
		return false
	}
	return halted
}

// --- I/O ---

// GetOutput retrieves captured output for a named channel.
// Returns the output string and true, or empty string and false if no output.
func (e *Engine) GetOutput(channel string) (string, bool) {
	return ffi.EngineGetOutput(e.handle, channel)
}

// ClearOutput clears a specific output channel.
func (e *Engine) ClearOutput(channel string) {
	ffi.EngineClearOutput(e.handle, channel)
}

// PushInput pushes an input line for read/readline.
func (e *Engine) PushInput(line string) {
	ffi.EnginePushInput(e.handle, line)
}

// --- Diagnostics ---

// Diagnostics returns all action diagnostic messages from recent execution.
func (e *Engine) Diagnostics() []string {
	count, rc := ffi.EngineActionDiagnosticCount(e.handle)
	if rc != ffi.ErrOK {
		return nil
	}
	diags := make([]string, 0, count)
	for i := range count {
		msg, rc := ffi.EngineActionDiagnosticCopy(e.handle, i)
		if rc != ffi.ErrOK {
			break
		}
		diags = append(diags, msg)
	}
	return diags
}

// ClearDiagnostics clears all stored action diagnostics.
func (e *Engine) ClearDiagnostics() {
	ffi.EngineClearActionDiagnostics(e.handle)
}

// --- Internal: fact building ---

func (e *Engine) buildFact(factID uint64) (*Fact, error) {
	ft, rc := ffi.EngineGetFactType(e.handle, factID)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, e.handle)
	}

	fieldCount, rc := ffi.EngineGetFactFieldCount(e.handle, factID)
	if rc != ffi.ErrOK {
		return nil, errorFromFFI(rc, e.handle)
	}

	fields := make([]any, fieldCount)
	for i := range fieldCount {
		val, rc := ffi.EngineGetFactField(e.handle, factID, i)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, e.handle)
		}
		fields[i] = ffiValueToGoAndFree(&val)
	}

	fact := &Fact{
		ID:     factID,
		Fields: fields,
	}

	if ft == ffi.FactTypeTemplate {
		fact.Type = FactTemplate
		name, rc := ffi.EngineGetFactTemplateName(e.handle, factID)
		if rc != ffi.ErrOK {
			return nil, errorFromFFI(rc, e.handle)
		}
		fact.TemplateName = name

		// Build slot map by querying template slot names.
		slotCount, rc := ffi.EngineTemplateSlotCount(e.handle, name)
		if rc == ffi.ErrOK && slotCount > 0 {
			fact.Slots = make(map[string]any, slotCount)
			for i := range slotCount {
				slotName, rc := ffi.EngineTemplateSlotName(e.handle, name, i)
				if rc != ffi.ErrOK {
					break
				}
				if i < fieldCount {
					fact.Slots[slotName] = fields[i]
				}
			}
		}
	} else {
		fact.Type = FactOrdered
		rel, rc := ffi.EngineGetFactRelation(e.handle, factID)
		if rc == ffi.ErrOK {
			fact.Relation = rel
		}
	}

	return fact, nil
}

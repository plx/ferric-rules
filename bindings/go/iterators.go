package ferric

import (
	"iter"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

// FactIter returns an iterator over all user-visible facts.
// Each iteration yields a Fact snapshot. Stops early on error.
func (e *Engine) FactIter() iter.Seq[Fact] {
	return func(yield func(Fact) bool) {
		ids, rc := ffi.EngineFactIDs(e.handle)
		if rc != ffi.ErrOK {
			return
		}
		for _, id := range ids {
			f, err := e.buildFact(id)
			if err != nil {
				return
			}
			if !yield(*f) {
				return
			}
		}
	}
}

// RuleIter returns an iterator over all registered rules.
func (e *Engine) RuleIter() iter.Seq[RuleInfo] {
	return func(yield func(RuleInfo) bool) {
		count, rc := ffi.EngineRuleCount(e.handle)
		if rc != ffi.ErrOK {
			return
		}
		for i := range count {
			name, salience, rc := ffi.EngineRuleInfo(e.handle, i)
			if rc != ffi.ErrOK {
				return
			}
			if !yield(RuleInfo{Name: name, Salience: int(salience)}) {
				return
			}
		}
	}
}

// TemplateIter returns an iterator over all registered template names.
func (e *Engine) TemplateIter() iter.Seq[string] {
	return func(yield func(string) bool) {
		count, rc := ffi.EngineTemplateCount(e.handle)
		if rc != ffi.ErrOK {
			return
		}
		for i := range count {
			name, rc := ffi.EngineTemplateName(e.handle, i)
			if rc != ffi.ErrOK {
				return
			}
			if !yield(name) {
				return
			}
		}
	}
}

// DiagnosticIter returns an iterator over action diagnostic messages.
func (e *Engine) DiagnosticIter() iter.Seq[string] {
	return func(yield func(string) bool) {
		count, rc := ffi.EngineActionDiagnosticCount(e.handle)
		if rc != ffi.ErrOK {
			return
		}
		for i := range count {
			msg, rc := ffi.EngineActionDiagnosticCopy(e.handle, i)
			if rc != ffi.ErrOK {
				return
			}
			if !yield(msg) {
				return
			}
		}
	}
}

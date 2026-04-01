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

// ---------------------------------------------------------------------------
// Error-aware iterator variants
// ---------------------------------------------------------------------------

// FactIterE returns an error-aware iterator over all user-visible facts.
// Each iteration yields (Fact, nil) on success. If an error occurs,
// a final (Fact{}, err) is yielded and iteration stops.
func (e *Engine) FactIterE() iter.Seq2[Fact, error] {
	return func(yield func(Fact, error) bool) {
		ids, rc := ffi.EngineFactIDs(e.handle)
		if rc != ffi.ErrOK {
			yield(Fact{}, errorFromFFI(rc, e.handle))
			return
		}
		for _, id := range ids {
			f, err := e.buildFact(id)
			if err != nil {
				yield(Fact{}, err)
				return
			}
			if !yield(*f, nil) {
				return
			}
		}
	}
}

// RuleIterE returns an error-aware iterator over all registered rules.
// Each iteration yields (RuleInfo, nil) on success. If an error occurs,
// a final (RuleInfo{}, err) is yielded and iteration stops.
func (e *Engine) RuleIterE() iter.Seq2[RuleInfo, error] {
	return func(yield func(RuleInfo, error) bool) {
		count, rc := ffi.EngineRuleCount(e.handle)
		if rc != ffi.ErrOK {
			yield(RuleInfo{}, errorFromFFI(rc, e.handle))
			return
		}
		for i := range count {
			name, salience, rc := ffi.EngineRuleInfo(e.handle, i)
			if rc != ffi.ErrOK {
				yield(RuleInfo{}, errorFromFFI(rc, e.handle))
				return
			}
			if !yield(RuleInfo{Name: name, Salience: int(salience)}, nil) {
				return
			}
		}
	}
}

// TemplateIterE returns an error-aware iterator over all registered
// template names. Each iteration yields (name, nil) on success. If an
// error occurs, a final ("", err) is yielded and iteration stops.
func (e *Engine) TemplateIterE() iter.Seq2[string, error] {
	return func(yield func(string, error) bool) {
		count, rc := ffi.EngineTemplateCount(e.handle)
		if rc != ffi.ErrOK {
			yield("", errorFromFFI(rc, e.handle))
			return
		}
		for i := range count {
			name, rc := ffi.EngineTemplateName(e.handle, i)
			if rc != ffi.ErrOK {
				yield("", errorFromFFI(rc, e.handle))
				return
			}
			if !yield(name, nil) {
				return
			}
		}
	}
}

// DiagnosticIterE returns an error-aware iterator over action diagnostic
// messages. Each iteration yields (msg, nil) on success. If an error
// occurs, a final ("", err) is yielded and iteration stops.
func (e *Engine) DiagnosticIterE() iter.Seq2[string, error] {
	return func(yield func(string, error) bool) {
		count, rc := ffi.EngineActionDiagnosticCount(e.handle)
		if rc != ffi.ErrOK {
			yield("", errorFromFFI(rc, e.handle))
			return
		}
		for i := range count {
			msg, rc := ffi.EngineActionDiagnosticCopy(e.handle, i)
			if rc != ffi.ErrOK {
				yield("", errorFromFFI(rc, e.handle))
				return
			}
			if !yield(msg, nil) {
				return
			}
		}
	}
}

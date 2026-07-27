package ferric

import (
	"iter"
)

// FactIter returns an iterator over all user-visible facts.
// Each iteration yields a Fact snapshot. Stops early on error.
func (e *Engine) FactIter() iter.Seq[Fact] {
	return func(yield func(Fact) bool) {
		facts, err := e.Facts()
		if err != nil {
			return
		}
		for _, fact := range facts {
			if !yield(fact) {
				return
			}
		}
	}
}

// RuleIter returns an iterator over all registered rules.
func (e *Engine) RuleIter() iter.Seq[RuleInfo] {
	return func(yield func(RuleInfo) bool) {
		rules := e.Rules()
		for _, rule := range rules {
			if !yield(rule) {
				return
			}
		}
	}
}

// TemplateIter returns an iterator over all registered template names.
func (e *Engine) TemplateIter() iter.Seq[string] {
	return func(yield func(string) bool) {
		names := e.Templates()
		for _, name := range names {
			if !yield(name) {
				return
			}
		}
	}
}

// DiagnosticIter returns an iterator over action diagnostic messages.
func (e *Engine) DiagnosticIter() iter.Seq[string] {
	return func(yield func(string) bool) {
		diagnostics := e.Diagnostics()
		for _, msg := range diagnostics {
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
		facts, err := e.Facts()
		if err != nil {
			yield(Fact{}, err)
			return
		}
		for _, fact := range facts {
			if !yield(fact, nil) {
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
		rules, err := e.RulesE()
		if err != nil {
			yield(RuleInfo{}, err)
			return
		}
		for _, rule := range rules {
			if !yield(rule, nil) {
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
		names, err := e.TemplatesE()
		if err != nil {
			yield("", err)
			return
		}
		for _, name := range names {
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
		diagnostics, err := e.DiagnosticsE()
		if err != nil {
			yield("", err)
			return
		}
		for _, msg := range diagnostics {
			if !yield(msg, nil) {
				return
			}
		}
	}
}

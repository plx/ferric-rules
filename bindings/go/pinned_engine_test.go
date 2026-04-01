package ferric

import (
	"context"
	"errors"
	"fmt"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

const ruleColorMatched = `(defrule r (color red) => (assert (matched yes)))`

var errTestSentinel = errors.New("test error")

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

func TestPinnedEngine_NewAndClose(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	require.NoError(t, p.Close())
}

func TestPinnedEngine_NewWithSource(t *testing.T) {
	p, err := NewPinnedEngine(WithSource(`(defrule r => (printout t "ok" crlf))`))
	require.NoError(t, err)
	defer mustClose(t, p)
}

func TestPinnedEngine_NewWithConfig(t *testing.T) {
	p, err := NewPinnedEngine(
		WithStrategy(StrategyBreadth),
		WithEncoding(EncodingUTF8),
		WithMaxCallDepth(512),
	)
	require.NoError(t, err)
	defer mustClose(t, p)
}

func TestPinnedEngine_NewInvalidSource(t *testing.T) {
	_, err := NewPinnedEngine(WithSource(`(defrule bad`))
	require.Error(t, err)
	assert.ErrorIs(t, err, ErrParse)
}

func TestPinnedEngine_DoubleClose(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	require.NoError(t, p.Close())
	// Second close is idempotent.
	require.NoError(t, p.Close())
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

func TestPinnedEngine_Load(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	err = p.Load(`(defrule greet => (printout t "hello" crlf))`)
	require.NoError(t, err)
}

func TestPinnedEngine_LoadInvalid(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	err = p.Load(`(defrule bad`)
	require.Error(t, err)
	assert.ErrorIs(t, err, ErrParse)
}

// ---------------------------------------------------------------------------
// Fact Operations
// ---------------------------------------------------------------------------

func TestPinnedEngine_AssertString(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	id, err := p.AssertString(`(assert (color red))`)
	require.NoError(t, err)
	assert.NotZero(t, id)
}

func TestPinnedEngine_AssertFact(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	id, err := p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)
	assert.NotZero(t, id)
}

func TestPinnedEngine_AssertTemplate(t *testing.T) {
	src := `(deftemplate person (slot name) (slot age))`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	id, err := p.AssertTemplate("person", map[string]any{
		"name": "Alice",
		"age":  int64(30),
	})
	require.NoError(t, err)
	assert.NotZero(t, id)
}

func TestPinnedEngine_Retract(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	id, err := p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)

	err = p.Retract(id)
	require.NoError(t, err)
}

func TestPinnedEngine_GetFact(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	id, err := p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)

	f, err := p.GetFact(id)
	require.NoError(t, err)
	assert.Equal(t, "color", f.Relation)
}

func TestPinnedEngine_Facts(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)
	_, err = p.AssertFact("color", Symbol("blue"))
	require.NoError(t, err)

	facts, err := p.Facts()
	require.NoError(t, err)
	// initial-fact + 2 asserted facts
	assert.GreaterOrEqual(t, len(facts), 2)
}

func TestPinnedEngine_FindFacts(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)
	_, err = p.AssertFact("shape", Symbol("circle"))
	require.NoError(t, err)

	colors, err := p.FindFacts("color")
	require.NoError(t, err)
	assert.Len(t, colors, 1)
}

func TestPinnedEngine_FactCount(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)

	count, err := p.FactCount()
	require.NoError(t, err)
	assert.GreaterOrEqual(t, count, 1)
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

func TestPinnedEngine_Run(t *testing.T) {
	p, err := NewPinnedEngine(WithSource(ruleColorMatched))
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)

	result, err := p.Run(context.Background())
	require.NoError(t, err)
	assert.Equal(t, 1, result.RulesFired)
}

func TestPinnedEngine_RunWithLimit(t *testing.T) {
	src := `
		(defrule r1 (color red) => (assert (shape circle)))
		(defrule r2 (shape circle) => (assert (done yes)))
	`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)

	result, err := p.RunWithLimit(context.Background(), 1)
	require.NoError(t, err)
	assert.Equal(t, 1, result.RulesFired)
	assert.Equal(t, HaltLimitReached, result.HaltReason)
}

func TestPinnedEngine_RunContextCancellation(t *testing.T) {
	// Use a rule that produces unbounded activations so we can test cancel.
	src := `(defrule loop (initial-fact) => (assert (tick)))`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()

	_, err = p.Run(ctx)
	// Should either complete or be canceled — both are acceptable.
	if err != nil {
		assert.ErrorIs(t, err, context.DeadlineExceeded)
	}
}

func TestPinnedEngine_Step(t *testing.T) {
	p, err := NewPinnedEngine(WithSource(ruleColorMatched))
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)

	fr, err := p.Step()
	require.NoError(t, err)
	assert.NotNil(t, fr)

	// Agenda should now be empty.
	fr2, err := p.Step()
	require.NoError(t, err)
	assert.Nil(t, fr2)
}

func TestPinnedEngine_Halt(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	// Halt is fire-and-forget; just verify it doesn't panic.
	p.Halt()
}

func TestPinnedEngine_Reset(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.AssertFact("color", Symbol("red"))
	require.NoError(t, err)

	err = p.Reset()
	require.NoError(t, err)

	// After reset, the asserted fact should be gone (only initial-fact remains).
	colors, err := p.FindFacts("color")
	require.NoError(t, err)
	assert.Empty(t, colors)
}

func TestPinnedEngine_Clear(t *testing.T) {
	src := `(defrule r => (printout t "ok" crlf))`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	p.Clear()

	rules := p.Rules()
	assert.Empty(t, rules)
}

// ---------------------------------------------------------------------------
// Introspection
// ---------------------------------------------------------------------------

func TestPinnedEngine_Rules(t *testing.T) {
	src := `(defrule greet => (printout t "hi" crlf))`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	rules := p.Rules()
	assert.Len(t, rules, 1)
	assert.Equal(t, "greet", rules[0].Name)
}

func TestPinnedEngine_Templates(t *testing.T) {
	src := `(deftemplate person (slot name))`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	templates := p.Templates()
	assert.Contains(t, templates, "person")
}

func TestPinnedEngine_GetGlobal(t *testing.T) {
	src := `(defglobal ?*x* = 42)`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	val, err := p.GetGlobal("x")
	require.NoError(t, err)
	assert.Equal(t, int64(42), val)
}

func TestPinnedEngine_CurrentModule(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	mod := p.CurrentModule()
	assert.Equal(t, "MAIN", mod)
}

func TestPinnedEngine_Focus(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	name, ok := p.Focus()
	// Fresh engine may or may not have a focus entry; just ensure no panic.
	_ = name
	_ = ok
}

func TestPinnedEngine_FocusStack(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	stack := p.FocusStack()
	_ = stack // no panic
}

func TestPinnedEngine_AgendaSize(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	size := p.AgendaSize()
	assert.GreaterOrEqual(t, size, 0)
}

func TestPinnedEngine_IsHalted(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	halted := p.IsHalted()
	assert.False(t, halted)
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

func TestPinnedEngine_Output(t *testing.T) {
	src := `(defrule r => (printout t "hello" crlf))`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	_, err = p.Run(context.Background())
	require.NoError(t, err)

	out, ok := p.GetOutput("t")
	assert.True(t, ok)
	assert.Contains(t, out, "hello")

	p.ClearOutput("t")
	out2, ok2 := p.GetOutput("t")
	assert.False(t, ok2)
	assert.Empty(t, out2)
}

func TestPinnedEngine_PushInput(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	// Just verify no panic.
	p.PushInput("test-line")
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

func TestPinnedEngine_Diagnostics(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	diags := p.Diagnostics()
	assert.Empty(t, diags)

	p.ClearDiagnostics() // no panic
}

// ---------------------------------------------------------------------------
// Do escape hatch
// ---------------------------------------------------------------------------

func TestPinnedEngine_Do(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	var factCount int
	err = p.Do(context.Background(), func(e *Engine) error {
		_, err := e.AssertFact("color", Symbol("red"))
		if err != nil {
			return err
		}
		count, err := e.FactCount()
		if err != nil {
			return err
		}
		factCount = count
		return nil
	})
	require.NoError(t, err)
	assert.GreaterOrEqual(t, factCount, 1)
}

func TestPinnedEngine_DoErrorPropagation(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	err = p.Do(context.Background(), func(_ *Engine) error {
		return errTestSentinel
	})
	assert.ErrorIs(t, err, errTestSentinel)
}

func TestPinnedEngine_DoNilContext(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	err = p.Do(nil, func(_ *Engine) error { return nil }) //nolint:staticcheck // testing nil context.
	require.Error(t, err)
}

func TestPinnedEngine_DoAfterClose(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	require.NoError(t, p.Close())

	err = p.Do(context.Background(), func(_ *Engine) error { return nil })
	assert.ErrorIs(t, err, errPinnedEngineClosed)
}

func TestPinnedEngine_DoContextCanceled(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // pre-cancel

	err = p.Do(ctx, func(_ *Engine) error { return nil })
	require.Error(t, err)
	assert.ErrorIs(t, err, context.Canceled)
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

func TestPinnedEngine_Serialize(t *testing.T) {
	src := `(defrule r (color red) => (assert (matched)))`
	p, err := NewPinnedEngine(WithSource(src))
	require.NoError(t, err)
	defer mustClose(t, p)

	data, err := p.Serialize(FormatBincode)
	require.NoError(t, err)
	assert.NotEmpty(t, data)

	// Deserialize into a new PinnedEngine.
	p2, err := NewPinnedEngine(WithSnapshot(data, FormatBincode))
	require.NoError(t, err)
	defer mustClose(t, p2)

	rules := p2.Rules()
	assert.Len(t, rules, 1)
}

// ---------------------------------------------------------------------------
// Thread-affinity safety: concurrent goroutine access
// ---------------------------------------------------------------------------

func TestPinnedEngine_ConcurrentAccess(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	defer mustClose(t, p)

	const goroutines = 10
	var wg sync.WaitGroup
	wg.Add(goroutines)

	errs := make(chan error, goroutines)

	for range goroutines {
		go func() {
			defer wg.Done()
			_, err := p.AssertFact("color", Symbol("red"))
			if err != nil {
				errs <- err
			}
		}()
	}

	wg.Wait()
	close(errs)

	for err := range errs {
		t.Errorf("concurrent AssertFact failed: %v", err)
	}

	count, err := p.FactCount()
	require.NoError(t, err)
	// initial-fact + goroutines asserted facts
	assert.GreaterOrEqual(t, count, goroutines)
}

func TestPinnedEngine_ConcurrentRunAndAssert(t *testing.T) {
	p, err := NewPinnedEngine(WithSource(ruleColorMatched))
	require.NoError(t, err)
	defer mustClose(t, p)

	const goroutines = 5
	var wg sync.WaitGroup
	wg.Add(goroutines * 2)

	for range goroutines {
		go func() {
			defer wg.Done()
			_, _ = p.AssertFact("color", Symbol("red"))
		}()
		go func() {
			defer wg.Done()
			_, _ = p.Run(context.Background())
		}()
	}

	wg.Wait()
	// No thread violation panics = success.
}

// ---------------------------------------------------------------------------
// Close-race stress test
// ---------------------------------------------------------------------------

// TestPinnedEngine_CloseStress exercises the close-during-active-work race
// window across many iterations. The two-phase shutdown guarantees that
// every accepted request completes with its real result.
func TestPinnedEngine_CloseStress(t *testing.T) {
	t.Parallel()
	for iter := range 50 {
		t.Run(fmt.Sprintf("iter_%d", iter), func(t *testing.T) {
			t.Parallel()

			p, err := NewPinnedEngine()
			require.NoError(t, err)

			const n = 30
			results := make(chan error, n)

			var wg sync.WaitGroup
			for range n {
				wg.Go(func() {
					results <- p.Do(context.Background(), func(e *Engine) error {
						return e.Reset()
					})
				})
			}

			// Close while requests are in flight.
			go func() {
				runtime.Gosched()
				_ = p.Close()
			}()

			wg.Wait()
			close(results)

			for err := range results {
				if err == nil || errors.Is(err, errPinnedEngineClosed) {
					continue
				}
				t.Errorf("unexpected error: %v", err)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Operations after close return appropriate error
// ---------------------------------------------------------------------------

func TestPinnedEngine_LoadAfterClose(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	require.NoError(t, p.Close())

	err = p.Load(`(defrule r => )`)
	assert.ErrorIs(t, err, errPinnedEngineClosed)
}

func TestPinnedEngine_AssertStringAfterClose(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	require.NoError(t, p.Close())

	_, err = p.AssertString(`(color red)`)
	assert.ErrorIs(t, err, errPinnedEngineClosed)
}

func TestPinnedEngine_RunAfterClose(t *testing.T) {
	p, err := NewPinnedEngine()
	require.NoError(t, err)
	require.NoError(t, p.Close())

	_, err = p.Run(context.Background())
	assert.ErrorIs(t, err, errPinnedEngineClosed)
}

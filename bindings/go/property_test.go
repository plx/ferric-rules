package ferric

import (
	"context"
	"math"
	"sort"
	"strings"
	"sync"
	"testing"

	"github.com/stretchr/testify/require"
	"pgregory.net/rapid"
)

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

// genLeafWireValue returns a generator for non-multifield WireValues.
func genLeafWireValue() *rapid.Generator[WireValue] {
	return rapid.OneOf(
		rapid.Just(WireValue{Kind: WireValueVoid}),
		rapid.Map(rapid.Int64(), func(v int64) WireValue {
			return WireValue{Kind: WireValueInteger, Integer: v}
		}),
		rapid.Map(
			rapid.Float64().Filter(func(f float64) bool { return !math.IsNaN(f) }),
			func(v float64) WireValue {
				return WireValue{Kind: WireValueFloat, Float: v}
			},
		),
		rapid.Map(rapid.String(), func(v string) WireValue {
			return WireValue{Kind: WireValueSymbol, Text: v}
		}),
		rapid.Map(rapid.String(), func(v string) WireValue {
			return WireValue{Kind: WireValueString, Text: v}
		}),
	)
}

// genWireValue returns a generator for WireValues including multifields.
func genWireValue() *rapid.Generator[WireValue] {
	return rapid.OneOf(
		genLeafWireValue(),
		rapid.Map(
			rapid.SliceOfN(genLeafWireValue(), 0, 8),
			func(elems []WireValue) WireValue {
				if elems == nil {
					elems = []WireValue{}
				}
				return WireValue{Kind: WireValueMultifield, Multifield: elems}
			},
		),
	)
}

// genLeafNative returns a generator for scalar Go values that roundtrip
// cleanly through the wire conversion layer (already-normalized types).
func genLeafNative() *rapid.Generator[any] {
	return rapid.OneOf[any](
		rapid.Just[any](nil),
		rapid.Map(rapid.Int64(), func(v int64) any { return v }),
		rapid.Map(
			rapid.Float64().Filter(func(f float64) bool { return !math.IsNaN(f) }),
			func(v float64) any { return v },
		),
		rapid.Map(rapid.String(), func(v string) any { return Symbol(v) }),
		rapid.Map(rapid.String(), func(v string) any { return v }),
	)
}

// genNativeValue returns a generator for Go native values (scalars + slices).
func genNativeValue() *rapid.Generator[any] {
	return rapid.OneOf[any](
		genLeafNative(),
		rapid.Map(
			rapid.SliceOfN(genLeafNative(), 0, 5),
			func(elems []any) any {
				if elems == nil {
					elems = []any{}
				}
				return elems
			},
		),
	)
}

// ---------------------------------------------------------------------------
// Property 1: Wire → Native → Wire roundtrip
// ---------------------------------------------------------------------------

func TestPropertyWireValueRoundtrip(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		w := genWireValue().Draw(t, "wire_value")

		native, err := WireToNativeValue(w)
		require.NoError(t, err)

		back, err := NativeToWireValue(native)
		require.NoError(t, err)

		require.Equal(t, w, back, "wire → native → wire mismatch")
	})
}

// ---------------------------------------------------------------------------
// Property 2: Native → Wire → Native roundtrip
// ---------------------------------------------------------------------------

func TestPropertyNativeToWireRoundtrip(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		v := genNativeValue().Draw(t, "native_value")

		w, err := NativeToWireValue(v)
		require.NoError(t, err)

		back, err := WireToNativeValue(w)
		require.NoError(t, err)

		require.Equal(t, v, back, "native → wire → native mismatch")
	})
}

// ---------------------------------------------------------------------------
// Property 3: Execution determinism — same inputs yield identical results
// ---------------------------------------------------------------------------

func TestPropertyExecutionDeterminism(t *testing.T) {
	lockThread(t)

	src := `
		(defrule process
			(data ?x)
			=>
			(printout t "processed " ?x crlf))
	`

	rapid.Check(t, func(t *rapid.T) {
		values := rapid.SliceOfN(rapid.Int64Range(-1000, 1000), 1, 10).Draw(t, "values")

		e, err := NewEngine(WithSource(src))
		require.NoError(t, err)
		defer func() { _ = e.Close() }()

		assertAll := func() {
			for _, v := range values {
				_, err := e.AssertFact("data", v)
				require.NoError(t, err)
			}
		}

		// First run.
		assertAll()
		r1, err := e.Run(context.Background())
		require.NoError(t, err)
		out1, _ := e.GetOutput("t")

		// Reset, clear output, re-run with identical inputs.
		require.NoError(t, e.Reset())
		e.ClearOutput("t")
		assertAll()
		r2, err := e.Run(context.Background())
		require.NoError(t, err)
		out2, _ := e.GetOutput("t")

		require.Equal(t, r1.RulesFired, r2.RulesFired, "fired count")
		require.Equal(t, sortedLines(out1), sortedLines(out2), "output")
	})
}

// ---------------------------------------------------------------------------
// Property 4: Snapshot equivalence — snapshot engine matches fresh engine
// ---------------------------------------------------------------------------

func TestPropertySnapshotEquivalence(t *testing.T) {
	lockThread(t)

	src := `
		(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
		(defrule alert
			(sensor (id ?id) (value ?v&:(> ?v 0.0)))
			=>
			(printout t "alert " ?id crlf))
	`

	rapid.Check(t, func(t *rapid.T) {
		format := rapid.SampledFrom([]Format{
			FormatBincode, FormatJSON, FormatCBOR, FormatMessagePack, FormatPostcard,
		}).Draw(t, "format")

		id := rapid.Int64Range(1, 100).Draw(t, "sensor_id")
		value := rapid.Float64Range(0.1, 1000.0).Draw(t, "sensor_value")

		// Create and snapshot the engine.
		orig, err := NewEngine(WithSource(src))
		require.NoError(t, err)
		snap, err := orig.Serialize(format)
		require.NoError(t, err)
		_ = orig.Close()

		// Fresh engine from source.
		fresh, err := NewEngine(WithSource(src))
		require.NoError(t, err)
		defer func() { _ = fresh.Close() }()

		// Restored engine from snapshot.
		restored, err := NewEngine(WithSnapshot(snap, format))
		require.NoError(t, err)
		defer func() { _ = restored.Close() }()

		// Assert same facts to both.
		slots := map[string]any{"id": id, "value": value}
		_, err = fresh.AssertTemplate("sensor", slots)
		require.NoError(t, err)
		_, err = restored.AssertTemplate("sensor", slots)
		require.NoError(t, err)

		r1, err := fresh.Run(context.Background())
		require.NoError(t, err)
		r2, err := restored.Run(context.Background())
		require.NoError(t, err)

		out1, _ := fresh.GetOutput("t")
		out2, _ := restored.GetOutput("t")

		require.Equal(t, r1.RulesFired, r2.RulesFired, "fired count")
		require.Equal(t, out1, out2, "output")
	})
}

// ---------------------------------------------------------------------------
// Property 5: Coordinator concurrent safety — no corruption under load
// ---------------------------------------------------------------------------

func TestPropertyCoordinatorConcurrentSafety(t *testing.T) {
	src := `(defrule echo (input ?x) => (printout t ?x crlf))`

	rapid.Check(t, func(t *rapid.T) {
		numRequests := rapid.IntRange(5, 20).Draw(t, "num_requests")
		numThreads := rapid.IntRange(1, 4).Draw(t, "num_threads")

		coord, err := NewCoordinator(
			[]EngineSpec{{Name: "test", Options: []EngineOption{WithSource(src)}}},
			Threads(numThreads),
		)
		require.NoError(t, err)
		defer func() { _ = coord.Close() }()

		mgr, err := coord.Manager("test")
		require.NoError(t, err)

		type result struct {
			fired int
			err   error
		}
		results := make([]result, numRequests)

		var wg sync.WaitGroup
		for i := range numRequests {
			wg.Go(func() {
				doErr := mgr.Do(context.Background(), func(e *Engine) error {
					if err := e.Reset(); err != nil {
						return err
					}
					if _, err := e.AssertFact("input", int64(i)); err != nil {
						return err
					}
					r, err := e.Run(context.Background())
					if err != nil {
						return err
					}
					results[i] = result{fired: r.RulesFired}
					return nil
				})
				if doErr != nil {
					results[i] = result{err: doErr}
				}
			})
		}
		wg.Wait()

		for i, r := range results {
			require.NoError(t, r.err, "request %d", i)
			require.Equal(t, 1, r.fired, "request %d fired count", i)
		}
	})
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func sortedLines(s string) []string {
	lines := strings.Split(strings.TrimRight(s, "\n"), "\n")
	sort.Strings(lines)
	return lines
}

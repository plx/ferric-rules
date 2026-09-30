package ferric

import (
	"context"
	"sort"
	"strings"
	"testing"

	"github.com/stretchr/testify/require"
	"pgregory.net/rapid"
)

// ---------------------------------------------------------------------------
// Property: Execution determinism — same inputs yield identical results
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
// Property: Snapshot equivalence — snapshot engine matches fresh engine
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
			FormatJSON, FormatCBOR,
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
// Helpers
// ---------------------------------------------------------------------------

func sortedLines(s string) []string {
	lines := strings.Split(strings.TrimRight(s, "\n"), "\n")
	sort.Strings(lines)
	return lines
}

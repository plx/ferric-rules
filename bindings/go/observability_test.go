package ferric

import (
	"bytes"
	"context"
	"log/slog"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
)

// testRule fires a single rule and prints "ok".
const testRule = `(defrule r => (printout t "ok" crlf))`

// newTestCoordinator creates a Coordinator wired with test OTel providers and
// a slog logger writing to buf. Returns the coordinator and functions to
// retrieve collected spans, metrics, and log output.
func newTestCoordinator(t *testing.T, buf *bytes.Buffer, specs []EngineSpec) (
	*Coordinator,
	func() []tracetest.SpanStub,
	func() metricdata.ResourceMetrics,
) {
	t.Helper()

	spanExp := tracetest.NewInMemoryExporter()
	tp := sdktrace.NewTracerProvider(sdktrace.WithSyncer(spanExp))
	t.Cleanup(func() { _ = tp.Shutdown(context.Background()) })

	reader := sdkmetric.NewManualReader()
	mp := sdkmetric.NewMeterProvider(sdkmetric.WithReader(reader))
	t.Cleanup(func() { _ = mp.Shutdown(context.Background()) })

	logger := slog.New(slog.NewTextHandler(buf, &slog.HandlerOptions{Level: slog.LevelDebug}))

	coord, err := NewCoordinator(specs,
		WithTracerProvider(tp),
		WithMeterProvider(mp),
		WithLogger(logger),
	)
	require.NoError(t, err)
	t.Cleanup(func() { _ = coord.Close() })

	getSpans := func() []tracetest.SpanStub {
		return spanExp.GetSpans()
	}
	getMetrics := func() metricdata.ResourceMetrics {
		var rm metricdata.ResourceMetrics
		err := reader.Collect(context.Background(), &rm)
		require.NoError(t, err)
		return rm
	}
	return coord, getSpans, getMetrics
}

// findMetric finds a metric by name in the collected ResourceMetrics.
func findMetric(rm metricdata.ResourceMetrics, name string) *metricdata.Metrics {
	for _, sm := range rm.ScopeMetrics {
		for i := range sm.Metrics {
			if sm.Metrics[i].Name == name {
				return &sm.Metrics[i]
			}
		}
	}
	return nil
}

// findSpans returns all spans with the given name.
func findSpans(stubs []tracetest.SpanStub, name string) []tracetest.SpanStub {
	var out []tracetest.SpanStub
	for _, s := range stubs {
		if s.Name == name {
			out = append(out, s)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

func TestObservabilityDefaultNoopDoesNotPanic(t *testing.T) {
	// Creating a Coordinator without any observability options must not panic.
	coord, err := NewCoordinator([]EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	})
	require.NoError(t, err)
	defer mustClose(t, coord)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	err = mgr.Do(context.Background(), func(e *Engine) error {
		_ = e.Reset()
		_, err := e.Run(context.Background())
		return err
	})
	require.NoError(t, err)
}

func TestObservabilityDispatchSpans(t *testing.T) {
	var buf bytes.Buffer
	specs := []EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	}
	coord, getSpans, _ := newTestCoordinator(t, &buf, specs)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	err = mgr.Do(context.Background(), func(e *Engine) error {
		_ = e.Reset()
		_, err := e.Run(context.Background())
		return err
	})
	require.NoError(t, err)

	spans := getSpans()
	dispatchSpans := findSpans(spans, "ferric.dispatch")
	assert.NotEmpty(t, dispatchSpans, "expected at least one ferric.dispatch span")

	// Check that the span has the spec attribute.
	if len(dispatchSpans) > 0 {
		found := false
		for _, attr := range dispatchSpans[0].Attributes {
			if string(attr.Key) == "ferric.spec" && attr.Value.AsString() == "test" {
				found = true
				break
			}
		}
		assert.True(t, found, "ferric.dispatch span should have ferric.spec=test attribute")
	}
}

func TestObservabilityEvaluateSpans(t *testing.T) {
	var buf bytes.Buffer
	specs := []EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	}
	coord, getSpans, _ := newTestCoordinator(t, &buf, specs)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	result, err := mgr.Evaluate(context.Background(), &EvaluateRequest{})
	require.NoError(t, err)
	assert.Equal(t, 1, result.RulesFired)

	spans := getSpans()
	evalSpans := findSpans(spans, "ferric.evaluate")
	assert.NotEmpty(t, evalSpans, "expected at least one ferric.evaluate span")

	// Evaluate also creates a child ferric.dispatch span.
	dispatchSpans := findSpans(spans, "ferric.dispatch")
	assert.NotEmpty(t, dispatchSpans, "expected a ferric.dispatch span from Evaluate")
}

func TestObservabilityMetrics(t *testing.T) {
	var buf bytes.Buffer
	specs := []EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	}
	coord, _, getMetrics := newTestCoordinator(t, &buf, specs)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	// First call triggers cold start + dispatch.
	_, err = mgr.Evaluate(context.Background(), &EvaluateRequest{})
	require.NoError(t, err)

	// Second call should not trigger cold start.
	_, err = mgr.Evaluate(context.Background(), &EvaluateRequest{})
	require.NoError(t, err)

	rm := getMetrics()

	// dispatch.total should be 2 (one per Evaluate).
	m := findMetric(rm, "ferric.dispatch.total")
	require.NotNil(t, m, "ferric.dispatch.total metric should exist")
	if s, ok := m.Data.(metricdata.Sum[int64]); ok {
		var total int64
		for _, dp := range s.DataPoints {
			total += dp.Value
		}
		assert.Equal(t, int64(2), total, "dispatch.total should be 2")
	}

	// dispatch.duration should have data points.
	m = findMetric(rm, "ferric.dispatch.duration")
	require.NotNil(t, m, "ferric.dispatch.duration metric should exist")
	if h, ok := m.Data.(metricdata.Histogram[float64]); ok {
		assert.NotEmpty(t, h.DataPoints, "dispatch.duration should have data points")
	}

	// run.duration should have data points.
	m = findMetric(rm, "ferric.run.duration")
	require.NotNil(t, m, "ferric.run.duration metric should exist")
	if h, ok := m.Data.(metricdata.Histogram[float64]); ok {
		assert.NotEmpty(t, h.DataPoints, "run.duration should have data points")
	}

	// cold_start.duration should have exactly 1 data point (only first call triggers cold start).
	m = findMetric(rm, "ferric.engine.cold_start.duration")
	require.NotNil(t, m, "ferric.engine.cold_start.duration metric should exist")
	if h, ok := m.Data.(metricdata.Histogram[float64]); ok {
		assert.NotEmpty(t, h.DataPoints, "cold_start.duration should have data points")
		if len(h.DataPoints) > 0 {
			assert.Equal(t, uint64(1), h.DataPoints[0].Count, "cold start should fire only once")
		}
	}

	// dispatch.errors should be 0 (no errors occurred).
	m = findMetric(rm, "ferric.dispatch.errors")
	if m != nil {
		if s, ok := m.Data.(metricdata.Sum[int64]); ok {
			var total int64
			for _, dp := range s.DataPoints {
				total += dp.Value
			}
			assert.Equal(t, int64(0), total, "dispatch.errors should be 0")
		}
	}
}

func TestObservabilityLogging(t *testing.T) {
	var buf bytes.Buffer
	specs := []EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	}
	coord, _, _ := newTestCoordinator(t, &buf, specs)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	_, err = mgr.Evaluate(context.Background(), &EvaluateRequest{})
	require.NoError(t, err)

	logOutput := buf.String()

	// Should contain a cold start log.
	assert.Contains(t, logOutput, "engine cold start", "should log engine cold start")
	assert.Contains(t, logOutput, `spec=test`, "cold start log should include spec name")

	// Should contain evaluate completed log.
	assert.Contains(t, logOutput, "evaluate completed", "should log evaluate completed")
}

func TestObservabilityErrorMetrics(t *testing.T) {
	var buf bytes.Buffer
	specs := []EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	}
	coord, getSpans, getMetrics := newTestCoordinator(t, &buf, specs)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	// Cause an error via Do.
	doErr := mgr.Do(context.Background(), func(_ *Engine) error {
		return assert.AnError
	})
	require.Error(t, doErr)

	rm := getMetrics()

	// dispatch.errors should be 1.
	m := findMetric(rm, "ferric.dispatch.errors")
	require.NotNil(t, m, "ferric.dispatch.errors metric should exist")
	if s, ok := m.Data.(metricdata.Sum[int64]); ok {
		var total int64
		for _, dp := range s.DataPoints {
			total += dp.Value
		}
		assert.Equal(t, int64(1), total, "dispatch.errors should be 1")
	}

	// The dispatch span should have an error status.
	spans := getSpans()
	dispatchSpans := findSpans(spans, "ferric.dispatch")
	require.NotEmpty(t, dispatchSpans)
	assert.NotEmpty(t, dispatchSpans[0].Events, "dispatch span should have error event")

	// Log should contain the dispatch failure.
	logOutput := buf.String()
	assert.Contains(t, logOutput, "dispatch failed", "should log dispatch failure")
}

func TestObservabilityColdStartLogsOnlyOnce(t *testing.T) {
	var buf bytes.Buffer
	specs := []EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	}
	coord, _, _ := newTestCoordinator(t, &buf, specs)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	for range 3 {
		_, err = mgr.Evaluate(context.Background(), &EvaluateRequest{})
		require.NoError(t, err)
	}

	logOutput := buf.String()
	// "engine cold start" should appear exactly once (first call only).
	count := strings.Count(logOutput, "engine cold start")
	assert.Equal(t, 1, count, "cold start should be logged exactly once")
}

func TestObservabilityContextCancellation(t *testing.T) {
	var buf bytes.Buffer
	specs := []EngineSpec{
		{Name: "test", Options: []EngineOption{WithSource(testRule)}},
	}
	coord, _, getMetrics := newTestCoordinator(t, &buf, specs)

	mgr, err := coord.Manager("test")
	require.NoError(t, err)

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // Cancel immediately.

	err = mgr.Do(ctx, func(_ *Engine) error { return nil })
	require.Error(t, err)

	rm := getMetrics()

	// Should record a dispatch error for the canceled request.
	m := findMetric(rm, "ferric.dispatch.errors")
	require.NotNil(t, m, "ferric.dispatch.errors metric should exist after cancellation")
}

func TestHaltReasonString(t *testing.T) {
	tests := []struct {
		hr   HaltReason
		want string
	}{
		{HaltAgendaEmpty, "agenda_empty"},
		{HaltLimitReached, "limit_reached"},
		{HaltRequested, "requested"},
		{HaltReason(99), "unknown"},
	}
	for _, tt := range tests {
		assert.Equal(t, tt.want, tt.hr.String())
	}
}

package ferric

import (
	"log/slog"
	"time"

	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/metric"
	metricnoop "go.opentelemetry.io/otel/metric/noop"
	"go.opentelemetry.io/otel/trace"
	tracenoop "go.opentelemetry.io/otel/trace/noop"
)

const instrumentationName = "github.com/prb/ferric-rules/bindings/go"

// obs holds the observability primitives for a Coordinator.
// All fields are safe for concurrent use. When no providers are
// configured, noop implementations are used and add negligible overhead.
type obs struct {
	logger *slog.Logger
	tracer trace.Tracer

	dispatchDuration  metric.Float64Histogram
	waitDuration      metric.Float64Histogram
	runDuration       metric.Float64Histogram
	coldStartDuration metric.Float64Histogram
	dispatchTotal     metric.Int64Counter
	dispatchErrors    metric.Int64Counter
}

func newObs(cfg *coordConfig) *obs {
	o := &obs{logger: cfg.logger}

	tp := cfg.tracerProvider
	if tp == nil {
		tp = tracenoop.NewTracerProvider()
	}
	o.tracer = tp.Tracer(instrumentationName)

	mp := cfg.meterProvider
	if mp == nil {
		mp = metricnoop.NewMeterProvider()
	}
	meter := mp.Meter(instrumentationName)

	o.dispatchDuration, _ = meter.Float64Histogram("ferric.dispatch.duration",
		metric.WithDescription("Total time from dispatch entry to completion"),
		metric.WithUnit("s"))
	o.waitDuration, _ = meter.Float64Histogram("ferric.dispatch.wait_duration",
		metric.WithDescription("Time a request waited in the worker queue"),
		metric.WithUnit("s"))
	o.runDuration, _ = meter.Float64Histogram("ferric.run.duration",
		metric.WithDescription("Engine run execution time"),
		metric.WithUnit("s"))
	o.coldStartDuration, _ = meter.Float64Histogram("ferric.engine.cold_start.duration",
		metric.WithDescription("Time to create a new engine instance"),
		metric.WithUnit("s"))
	o.dispatchTotal, _ = meter.Int64Counter("ferric.dispatch.total",
		metric.WithDescription("Total number of dispatched requests"))
	o.dispatchErrors, _ = meter.Int64Counter("ferric.dispatch.errors",
		metric.WithDescription("Number of failed dispatch requests"))

	return o
}

// recordError marks a span as failed and records the error.
func (*obs) recordError(span trace.Span, err error) {
	span.RecordError(err)
	span.SetStatus(codes.Error, err.Error())
}

// sinceSeconds returns the elapsed time since t in fractional seconds.
func sinceSeconds(t time.Time) float64 {
	return time.Since(t).Seconds()
}

// String returns a human-readable label for a HaltReason.
func (h HaltReason) String() string {
	switch h {
	case HaltAgendaEmpty:
		return "agenda_empty"
	case HaltLimitReached:
		return "limit_reached"
	case HaltRequested:
		return "requested"
	default:
		return "unknown"
	}
}

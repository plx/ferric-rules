package ferric

import (
	"log/slog"

	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/trace"
)

// CoordinatorOption configures a Coordinator.
type CoordinatorOption func(*coordConfig)

type coordConfig struct {
	threads        int
	policy         DispatchPolicy
	logger         *slog.Logger
	tracerProvider trace.TracerProvider
	meterProvider  metric.MeterProvider
}

// Threads sets the number of worker goroutines (locked OS threads).
func Threads(n int) CoordinatorOption {
	return func(c *coordConfig) { c.threads = n }
}

// WithDispatchPolicy sets a custom dispatch policy.
// If nil, the default round-robin policy is used.
func WithDispatchPolicy(p DispatchPolicy) CoordinatorOption {
	return func(c *coordConfig) {
		if p != nil {
			c.policy = p
		}
	}
}

// WithLogger sets a structured logger for operational events.
// When nil (the default), no log events are emitted.
func WithLogger(l *slog.Logger) CoordinatorOption {
	return func(c *coordConfig) { c.logger = l }
}

// WithTracerProvider sets an OpenTelemetry TracerProvider for distributed tracing.
// When nil (the default), a noop tracer is used.
func WithTracerProvider(tp trace.TracerProvider) CoordinatorOption {
	return func(c *coordConfig) { c.tracerProvider = tp }
}

// WithMeterProvider sets an OpenTelemetry MeterProvider for metrics collection.
// When nil (the default), a noop meter is used.
func WithMeterProvider(mp metric.MeterProvider) CoordinatorOption {
	return func(c *coordConfig) { c.meterProvider = mp }
}

// EngineSpec describes a named engine configuration.
type EngineSpec struct {
	Name    string
	Options []EngineOption
}

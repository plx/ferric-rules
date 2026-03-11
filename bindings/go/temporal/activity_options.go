package temporal

import (
	ferric "github.com/prb/ferric-rules/bindings/go"
)

// ActivityOption configures a RulesActivity.
type ActivityOption func(*activityConfig)

type activityConfig struct {
	coordinatorOpts []ferric.CoordinatorOption
}

// WithCoordinatorOptions passes options through to the Coordinator.
func WithCoordinatorOptions(opts ...ferric.CoordinatorOption) ActivityOption {
	return func(c *activityConfig) {
		c.coordinatorOpts = append(c.coordinatorOpts, opts...)
	}
}

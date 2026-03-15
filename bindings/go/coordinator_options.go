package ferric

// CoordinatorOption configures a Coordinator.
type CoordinatorOption func(*coordConfig)

type coordConfig struct {
	threads int
	policy  DispatchPolicy
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

// EngineSpec describes a named engine configuration.
type EngineSpec struct {
	Name    string
	Options []EngineOption
}

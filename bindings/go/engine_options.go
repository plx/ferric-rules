package ferric

// EngineOption configures an Engine.
type EngineOption func(*engineConfig)

type engineConfig struct {
	strategy     Strategy
	encoding     Encoding
	maxCallDepth int
	source       string // if non-empty, load+reset at creation
}

// WithStrategy sets the conflict resolution strategy.
func WithStrategy(s Strategy) EngineOption {
	return func(c *engineConfig) { c.strategy = s }
}

// WithEncoding sets the string encoding mode.
func WithEncoding(e Encoding) EngineOption {
	return func(c *engineConfig) { c.encoding = e }
}

// WithMaxCallDepth sets the maximum call depth.
func WithMaxCallDepth(n int) EngineOption {
	return func(c *engineConfig) { c.maxCallDepth = n }
}

// WithSource loads CLIPS source and resets the engine at creation time.
func WithSource(clips string) EngineOption {
	return func(c *engineConfig) { c.source = clips }
}

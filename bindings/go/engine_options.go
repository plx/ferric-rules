package ferric

import (
	"fmt"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

// EngineOption configures an Engine.
type EngineOption func(*engineConfig)

type engineConfig struct {
	strategy        Strategy
	strategySet     bool
	encoding        Encoding
	encodingSet     bool
	maxCallDepth    int
	maxCallDepthSet bool
	source          string // if non-empty, load+reset at creation
	sourceSet       bool
	snapshot        []byte // if non-nil, deserialize instead of creating fresh
	snapshotSet     bool
	snapshotFormat  Format // format of snapshot data
}

func defaultEngineConfig() engineConfig {
	return engineConfig{
		strategy:     StrategyDepth,
		encoding:     EncodingUTF8,
		maxCallDepth: 64,
	}
}

func (c *engineConfig) hasEngineConfig() bool {
	return c.strategySet || c.encodingSet || c.maxCallDepthSet
}

func (c *engineConfig) hasSource() bool {
	return c.sourceSet && c.source != ""
}

func (c *engineConfig) hasSnapshot() bool {
	return c.snapshotSet && c.snapshot != nil
}

// WithStrategy sets the conflict resolution strategy.
func WithStrategy(s Strategy) EngineOption {
	return func(c *engineConfig) {
		c.strategy = s
		c.strategySet = true
	}
}

// WithEncoding sets the string encoding mode.
func WithEncoding(e Encoding) EngineOption {
	return func(c *engineConfig) {
		c.encoding = e
		c.encodingSet = true
	}
}

// WithMaxCallDepth sets the maximum call depth.
func WithMaxCallDepth(n int) EngineOption {
	return func(c *engineConfig) {
		c.maxCallDepth = n
		c.maxCallDepthSet = true
	}
}

// WithSource loads CLIPS source and resets the engine at creation time.
func WithSource(clips string) EngineOption {
	return func(c *engineConfig) {
		c.source = clips
		c.sourceSet = true
	}
}

// WithSnapshot creates the engine by deserializing a snapshot previously
// produced by Engine.Serialize. The format must match the one used during
// serialization. This skips parsing and compilation, providing fast engine
// instantiation. Mutually exclusive with WithSource.
func WithSnapshot(data []byte, format Format) EngineOption {
	return func(c *engineConfig) {
		c.snapshot = data
		c.snapshotSet = true
		c.snapshotFormat = format
	}
}

// formatToFFI converts a public Format to the FFI-level format enum.
func formatToFFI(f Format) (ffi.SerializationFormat, error) {
	switch f {
	case FormatBincode:
		return ffi.FormatBincode, nil
	case FormatJSON:
		return ffi.FormatJSON, nil
	case FormatCBOR:
		return ffi.FormatCBOR, nil
	case FormatMessagePack:
		return ffi.FormatMessagePack, nil
	case FormatPostcard:
		return ffi.FormatPostcard, nil
	default:
		return 0, fmt.Errorf("%w: unsupported serialization format %d", ErrInvalidArgument, f)
	}
}

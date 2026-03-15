package ferric

import "github.com/prb/ferric-rules/bindings/go/internal/ffi"

// EngineOption configures an Engine.
type EngineOption func(*engineConfig)

type engineConfig struct {
	strategy       Strategy
	encoding       Encoding
	maxCallDepth   int
	source         string // if non-empty, load+reset at creation
	snapshot       []byte // if non-nil, deserialize instead of creating fresh
	snapshotFormat Format // format of snapshot data
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

// WithSnapshot creates the engine by deserializing a snapshot previously
// produced by Engine.Serialize. The format must match the one used during
// serialization. This skips parsing and compilation, providing fast engine
// instantiation. Mutually exclusive with WithSource.
func WithSnapshot(data []byte, format Format) EngineOption {
	return func(c *engineConfig) {
		c.snapshot = data
		c.snapshotFormat = format
	}
}

// formatToFFI converts a public Format to the FFI-level format enum.
func formatToFFI(f Format) ffi.SerializationFormat {
	switch f {
	case FormatBincode:
		return ffi.FormatBincode
	case FormatJSON:
		return ffi.FormatJSON
	case FormatCBOR:
		return ffi.FormatCBOR
	case FormatMessagePack:
		return ffi.FormatMessagePack
	case FormatPostcard:
		return ffi.FormatPostcard
	default:
		return ffi.FormatBincode
	}
}

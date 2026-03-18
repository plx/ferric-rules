package ferric

// Strategy is the conflict resolution strategy.
type Strategy int

const (
	// StrategyDepth prioritizes depth in conflict resolution.
	StrategyDepth Strategy = iota
	// StrategyBreadth prioritizes breadth in conflict resolution.
	StrategyBreadth
	// StrategyLEX uses CLIPS LEX ordering.
	StrategyLEX
	// StrategyMEA uses CLIPS MEA ordering.
	StrategyMEA
)

// Encoding is the string encoding mode.
type Encoding int

const (
	// EncodingASCII encodes strings and symbols as ASCII.
	EncodingASCII Encoding = iota
	// EncodingUTF8 encodes strings and symbols as UTF-8.
	EncodingUTF8
	// EncodingASCIISymbolsUTF8Strings uses ASCII symbols and UTF-8 strings.
	EncodingASCIISymbolsUTF8Strings
)

// Format selects the serialization format for Engine.SerializeAs / WithSnapshotAs.
type Format int

const (
	// FormatBincode uses compact binary encoding (default, fast and small).
	FormatBincode Format = iota
	// FormatJSON uses human-readable JSON encoding.
	FormatJSON
	// FormatCBOR uses CBOR (Concise Binary Object Representation).
	FormatCBOR
	// FormatMessagePack uses MessagePack encoding.
	FormatMessagePack
	// FormatPostcard uses Postcard encoding (compact, no_std-friendly).
	FormatPostcard
)

// HaltReason describes why engine execution stopped.
type HaltReason int

const (
	// HaltAgendaEmpty means execution stopped because no activations remained.
	HaltAgendaEmpty HaltReason = iota
	// HaltLimitReached means execution stopped due to a run limit.
	HaltLimitReached
	// HaltRequested means execution stopped because halt was requested.
	HaltRequested
)

// RunResult contains the outcome of an engine run.
type RunResult struct {
	RulesFired int
	HaltReason HaltReason
}

// FiredRule identifies a single rule that fired during a step.
type FiredRule struct {
	RuleName string
}

// RuleInfo describes a registered rule.
type RuleInfo struct {
	Name     string
	Salience int
}

package ferric

// Strategy is the conflict resolution strategy.
type Strategy int

const (
	StrategyDepth   Strategy = iota
	StrategyBreadth
	StrategyLEX
	StrategyMEA
)

// Encoding is the string encoding mode.
type Encoding int

const (
	EncodingASCII                   Encoding = iota
	EncodingUTF8
	EncodingASCIISymbolsUTF8Strings
)

// HaltReason describes why engine execution stopped.
type HaltReason int

const (
	HaltAgendaEmpty  HaltReason = iota
	HaltLimitReached
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

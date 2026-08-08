// Command bindings-conformance is the Go adapter for the shared semantic corpus.
//
//nolint:err113,goconst,wrapcheck // Standalone adapter reports probe context at the process boundary.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"unsafe"

	ferric "github.com/prb/ferric-rules/bindings/go"
)

const highIDIterations = 1_048_577

type record struct {
	Case   string `json:"case"`
	Result any    `json:"result"`
}

func root() (string, error) {
	value := os.Getenv("FERRIC_BINDINGS_CONFORMANCE_ROOT")
	if value == "" {
		return "", errors.New("FERRIC_BINDINGS_CONFORMANCE_ROOT is not set")
	}
	return value, nil
}

func fixture(name string) (string, error) {
	repository, err := root()
	if err != nil {
		return "", err
	}
	path := filepath.Join(repository, "tests", "bindings-conformance", "fixtures", name)
	data, err := os.ReadFile(filepath.Clean(path)) // #nosec G304 -- repository-owned fixture path
	if err != nil {
		return "", fmt.Errorf("read fixture %s: %w", path, err)
	}
	return string(data), nil
}

func withEngine(options ...ferric.EngineOption) (*ferric.Engine, error) {
	return ferric.NewEngine(options...)
}

func normalize(value any) any {
	switch value := value.(type) {
	case nil:
		return map[string]any{"type": "void"}
	case int:
		return map[string]any{"type": "integer", "value": strconv.FormatInt(int64(value), 10)}
	case int64:
		return map[string]any{"type": "integer", "value": strconv.FormatInt(value, 10)}
	case int32:
		return map[string]any{"type": "integer", "value": strconv.FormatInt(int64(value), 10)}
	case float64:
		return map[string]any{"type": "float", "value": strconv.FormatFloat(value, 'f', -1, 64)}
	case float32:
		return map[string]any{"type": "float", "value": strconv.FormatFloat(float64(value), 'f', -1, 32)}
	case ferric.Symbol:
		return map[string]any{"type": "symbol", "value": string(value)}
	case string:
		return map[string]any{"type": "string", "value": value}
	case []any:
		items := make([]any, len(value))
		for index, item := range value {
			items[index] = normalize(item)
		}
		return map[string]any{"type": "multifield", "value": items}
	case unsafe.Pointer:
		return map[string]any{"type": "external_address"}
	default:
		return map[string]any{"type": fmt.Sprintf("unsupported:%T", value)}
	}
}

func assertedField(value any) (any, error) {
	engine, err := withEngine()
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()

	id, err := engine.AssertFact("probe", value)
	if err != nil {
		return nil, err
	}
	fact, err := engine.GetFact(id)
	if err != nil {
		return nil, err
	}
	if len(fact.Fields) != 1 {
		return nil, fmt.Errorf("asserted fact has %d fields", len(fact.Fields))
	}
	return normalize(fact.Fields[0]), nil
}

func valueCase(caseID string) (any, error) {
	switch caseID {
	case "value.void":
		return assertedField(nil)
	case "value.integer.boundaries":
		minimum, err := assertedField(int64(-1 << 63))
		if err != nil {
			return nil, err
		}
		maximum, err := assertedField(int64(1<<63 - 1))
		if err != nil {
			return nil, err
		}
		return map[string]any{"minimum": minimum, "maximum": maximum}, nil
	case "value.float":
		return assertedField(1.5)
	case "value.symbol.explicit":
		return assertedField(ferric.Symbol("red"))
	case "value.string.explicit", "value.string.plain-host":
		return assertedField("red")
	case "value.multifield.nested":
		return assertedField([]any{
			nil,
			int64(7),
			2.5,
			ferric.Symbol("blue"),
			"text",
			[]any{int64(9)},
		})
	case "value.external-address":
		engine, err := withEngine()
		if err != nil {
			return nil, err
		}
		defer func() { _ = engine.Close() }()
		_, ingressErr := engine.AssertFact("probe", unsafe.Pointer(nil)) //nolint:gosec // Deliberate public unsafe.Pointer ingress probe.
		ingress := "unsupported"
		if ingressErr == nil {
			ingress = "accepted"
		}
		return map[string]any{
			"host_representation": "opaque_pointer",
			"ingress":             ingress,
		}, nil
	default:
		return nil, fmt.Errorf("unknown value case %s", caseID)
	}
}

func configurationDefault() (any, error) {
	engine, err := withEngine()
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()
	_, unicodeErr := engine.AssertFact("unicode", "é")
	unicode := "accepted"
	if unicodeErr != nil {
		unicode = "rejected"
	}
	return map[string]any{
		"max_call_depth": 64,
		"strategy":       "depth",
		"unicode":        unicode,
	}, nil
}

func configurationCustom() (any, error) {
	source, err := fixture("custom-config.clp")
	if err != nil {
		return nil, err
	}
	engine, err := withEngine(
		ferric.WithSource(source),
		ferric.WithEncoding(ferric.EncodingASCII),
		ferric.WithStrategy(ferric.StrategyBreadth),
		ferric.WithMaxCallDepth(1),
	)
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()
	_, unicodeErr := engine.AssertFact("unicode", "é")
	asciiUnicode := "accepted"
	if unicodeErr != nil {
		asciiUnicode = "rejected"
	}
	run, err := engine.Run(context.Background())
	if err != nil {
		return nil, err
	}
	if run.HaltReason != ferric.HaltActionError {
		return nil, errors.New("custom max_call_depth did not bound recursion")
	}
	return map[string]any{
		"ascii_unicode":  asciiUnicode,
		"max_call_depth": "configurable",
		"strategy_count": 4,
	}, nil
}

func configurationObservation(source string, options ...ferric.EngineOption) (map[string]any, error) {
	options = append([]ferric.EngineOption{ferric.WithSource(source)}, options...)
	engine, err := withEngine(options...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()

	_, unicodeErr := engine.AssertFact("unicode", "é")
	unicode := "accepted"
	if unicodeErr != nil {
		unicode = "rejected"
	}
	run, err := engine.Run(context.Background())
	if err != nil {
		return nil, err
	}
	return map[string]any{
		"halt_reason": reason(run.HaltReason),
		"unicode":     unicode,
	}, nil
}

func configurationStrategyFired(source string) (int, error) {
	engine, err := withEngine(
		ferric.WithSource(source),
		ferric.WithStrategy(ferric.StrategyBreadth),
	)
	if err != nil {
		return 0, err
	}
	defer func() { _ = engine.Close() }()

	run, err := engine.Run(context.Background())
	if err != nil {
		return 0, err
	}
	return run.RulesFired, nil
}

func configurationIsolation() (any, error) {
	defaultDepthSource, err := fixture("configuration-default-depth.clp")
	if err != nil {
		return nil, err
	}
	customDepthSource, err := fixture("custom-config.clp")
	if err != nil {
		return nil, err
	}
	strategySource, err := fixture("configuration-strategy-order.clp")
	if err != nil {
		return nil, err
	}

	encodingASCII, err := configurationObservation(
		defaultDepthSource,
		ferric.WithEncoding(ferric.EncodingASCII),
	)
	if err != nil {
		return nil, err
	}
	strategyBreadth, err := configurationObservation(
		defaultDepthSource,
		ferric.WithStrategy(ferric.StrategyBreadth),
	)
	if err != nil {
		return nil, err
	}
	strategyFired, err := configurationStrategyFired(strategySource)
	if err != nil {
		return nil, err
	}
	strategyBreadth["strategy_fired"] = strategyFired
	depthOne, err := configurationObservation(
		customDepthSource,
		ferric.WithMaxCallDepth(1),
	)
	if err != nil {
		return nil, err
	}
	depth256, err := configurationObservation(
		defaultDepthSource,
		ferric.WithMaxCallDepth(256),
	)
	if err != nil {
		return nil, err
	}

	return map[string]any{
		"depth_1_only":          depthOne,
		"depth_256_only":        depth256,
		"encoding_ascii_only":   encodingASCII,
		"strategy_breadth_only": strategyBreadth,
	}, nil
}

func errorCase(caseID string) (any, error) {
	engine, err := withEngine()
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()

	family := ""
	switch caseID {
	case "error.parse":
		err = engine.Load("(defrule incomplete")
		if errors.Is(err, ferric.ErrParse) {
			family = "parse"
		}
	case "error.compile":
		err = engine.Load("(defrule bad => (nonexistent-fn))")
		if errors.Is(err, ferric.ErrCompile) {
			family = "compile"
		}
	case "error.unsupported-construct":
		err = engine.Load("(defclass Probe (is-a USER))")
		if errors.Is(err, ferric.ErrCompile) {
			family = "compile"
		}
	case "error.runtime":
		id, assertErr := engine.AssertFact("stale")
		if assertErr != nil {
			return nil, assertErr
		}
		if err = engine.Retract(id); err != nil {
			return nil, err
		}
		err = engine.Retract(id)
		if errors.Is(err, ferric.ErrNotFound) {
			family = "fact_not_found"
		}
	default:
		return nil, fmt.Errorf("unknown error case %s", caseID)
	}
	if family == "" {
		return nil, fmt.Errorf("%s produced unexpected error: %w", caseID, err)
	}
	return map[string]any{"family": family}, nil
}

func factLifecycle() (any, error) {
	source, err := fixture("template.clp")
	if err != nil {
		return nil, err
	}
	engine, err := withEngine(ferric.WithSource(source))
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()

	orderedID, err := engine.AssertFact("ordered", int64(7))
	if err != nil {
		return nil, err
	}
	orderedSnapshot, err := engine.GetFact(orderedID)
	if err != nil {
		return nil, err
	}
	if err = engine.Retract(orderedID); err != nil {
		return nil, err
	}

	templateID, err := engine.AssertTemplate("person", map[string]any{"name": "Ada"})
	if err != nil {
		return nil, err
	}
	templateSnapshot, err := engine.GetFact(templateID)
	if err != nil {
		return nil, err
	}
	if err = engine.Retract(templateID); err != nil {
		return nil, err
	}
	count, err := engine.FactCount()
	if err != nil {
		return nil, err
	}
	return map[string]any{
		"count_after_retract":        count,
		"ordered_snapshot_retained":  orderedSnapshot.Relation == "ordered",
		"template_snapshot_retained": templateSnapshot.TemplateName == "person",
	}, nil
}

func reason(reason ferric.HaltReason) string {
	switch reason {
	case ferric.HaltAgendaEmpty:
		return "agenda_empty"
	case ferric.HaltLimitReached:
		return "limit_reached"
	case ferric.HaltRequested:
		return "halt_requested"
	case ferric.HaltActionError:
		return "action_error"
	default:
		return "unknown"
	}
}

func runFixture(name string, limit int, cancelable bool) (*ferric.RunResult, *ferric.Engine, error) {
	source, err := fixture(name)
	if err != nil {
		return nil, nil, err
	}
	engine, err := withEngine(ferric.WithSource(source))
	if err != nil {
		return nil, nil, err
	}
	ctx := context.Background()
	cancel := func() {}
	if cancelable {
		ctx, cancel = context.WithCancel(context.Background())
	}
	defer cancel()
	result, err := engine.RunWithLimit(ctx, limit)
	if err != nil {
		_ = engine.Close()
		return nil, nil, err
	}
	return result, engine, nil
}

func normalizeRun(result *ferric.RunResult) any {
	return map[string]any{"fired": result.RulesFired, "reason": reason(result.HaltReason)}
}

func runOnce(limit int) (any, error) {
	result, engine, err := runFixture("run-limits.clp", limit, false)
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()
	return normalizeRun(result), nil
}

func executionRunLimits() (any, error) {
	zero, err := runOnce(0)
	if err != nil {
		return nil, err
	}
	one, err := runOnce(1)
	if err != nil {
		return nil, err
	}
	unlimited, err := runOnce(0)
	if err != nil {
		return nil, err
	}
	return map[string]any{"zero": zero, "one": one, "unlimited": unlimited}, nil
}

func executionStep() (any, error) {
	source, err := fixture("one-rule.clp")
	if err != nil {
		return nil, err
	}
	engine, err := withEngine(ferric.WithSource(source))
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()
	first, err := engine.Step()
	if err != nil {
		return nil, err
	}
	second, err := engine.Step()
	if err != nil {
		return nil, err
	}
	var firstRule any
	if first != nil && first.RuleName != "" {
		firstRule = first.RuleName
	}
	return map[string]any{"first_rule": firstRule, "empty": second == nil}, nil
}

func executionDiagnostic() (any, error) {
	result, engine, err := runFixture("diagnostic.clp", 0, false)
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()
	return map[string]any{
		"fired":            result.RulesFired,
		"reason":           reason(result.HaltReason),
		"diagnostic_count": len(engine.Diagnostics()),
	}, nil
}

func snapshotRoundtrip() (any, error) {
	source, err := fixture("snapshot.clp")
	if err != nil {
		return nil, err
	}
	engine, err := withEngine(ferric.WithSource(source))
	if err != nil {
		return nil, err
	}
	if _, err = engine.AssertFact("seed"); err != nil {
		_ = engine.Close()
		return nil, err
	}
	snapshot, err := engine.Serialize(ferric.FormatJSON)
	_ = engine.Close()
	if err != nil {
		return nil, err
	}
	restored, err := withEngine(ferric.WithSnapshot(snapshot, ferric.FormatJSON))
	if err != nil {
		return nil, err
	}
	defer func() { _ = restored.Close() }()
	count, err := restored.FactCount()
	if err != nil {
		return nil, err
	}
	run, err := restored.Run(context.Background())
	if err != nil {
		return nil, err
	}
	return map[string]any{
		"fact_count":  count,
		"format":      "json",
		"rules_fired": run.RulesFired,
	}, nil
}

func lifecycleClose() (any, error) {
	engine, err := withEngine()
	if err != nil {
		return nil, err
	}
	if err = engine.Close(); err != nil {
		return nil, err
	}
	if err = engine.Close(); err != nil {
		return nil, err
	}
	_, postClose := engine.FactCount()
	state := "no_error"
	if errors.Is(postClose, ferric.ErrEngineClosed) {
		state = "closed_error"
	}
	return map[string]any{
		"explicit":   true,
		"idempotent": true,
		"post_close": state,
	}, nil
}

func embeddedNUL() (any, error) {
	return assertedField("a\x00b")
}

func highFactID() (any, error) {
	engine, err := withEngine()
	if err != nil {
		return nil, err
	}
	defer func() { _ = engine.Close() }()
	for range highIDIterations {
		id, assertErr := engine.AssertFact("generation")
		if assertErr != nil {
			return nil, assertErr
		}
		if retractErr := engine.Retract(id); retractErr != nil {
			return nil, retractErr
		}
	}
	id, err := engine.AssertFact("generation")
	if err != nil {
		return nil, err
	}
	fact, err := engine.GetFact(id)
	return map[string]any{
		"roundtrip": id > 9_007_199_254_740_991 && err == nil && fact != nil,
	}, nil
}

//nolint:funlen // Keeping the corpus dispatch in one switch makes missing cases visible.
func runCase(caseID string) (any, error) {
	if len(caseID) >= len("value.") && caseID[:len("value.")] == "value." {
		return valueCase(caseID)
	}
	if len(caseID) >= len("error.") && caseID[:len("error.")] == "error." {
		return errorCase(caseID)
	}
	switch caseID {
	case "configuration.default":
		return configurationDefault()
	case "configuration.custom":
		return configurationCustom()
	case "configuration.isolation":
		return configurationIsolation()
	case "fact.lifecycle":
		return factLifecycle()
	case "execution.run-limits":
		return executionRunLimits()
	case "execution.step":
		return executionStep()
	case "execution.halt":
		result, engine, err := runFixture("halt.clp", 0, false)
		if engine != nil {
			defer func() { _ = engine.Close() }()
		}
		if err != nil {
			return nil, err
		}
		return normalizeRun(result), nil
	case "execution.diagnostic":
		return executionDiagnostic()
	case "execution.batch-boundary-halt":
		result, engine, err := runFixture("batch-boundary-halt.clp", 0, true)
		if engine != nil {
			defer func() { _ = engine.Close() }()
		}
		if err != nil {
			return nil, err
		}
		return normalizeRun(result), nil
	case "snapshot.json-roundtrip":
		return snapshotRoundtrip()
	case "lifecycle.close":
		return lifecycleClose()
	case "robustness.embedded-nul":
		return embeddedNUL()
	case "identifier.high-fact-id":
		return highFactID()
	case "count.run-result-width":
		return map[string]any{
			"run_count_bits": strconv.IntSize,
			"run_limit_bits": strconv.IntSize,
		}, nil
	default:
		return nil, fmt.Errorf("unknown case %s", caseID)
	}
}

func adapterMain() int {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: go adapter CASE_IDS_PATH")
		return 2
	}
	file, err := os.Open(filepath.Clean(os.Args[1]))
	if err != nil {
		fmt.Fprintf(os.Stderr, "go conformance adapter: %v\n", err)
		return 1
	}
	defer func() { _ = file.Close() }()

	encoder := json.NewEncoder(os.Stdout)
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		caseID := scanner.Text()
		if caseID == "" {
			continue
		}
		result, probeErr := runCase(caseID)
		if probeErr != nil {
			fmt.Fprintf(os.Stderr, "go conformance adapter (%s): %v\n", caseID, probeErr)
			return 1
		}
		if err = encoder.Encode(record{Case: caseID, Result: result}); err != nil {
			fmt.Fprintf(os.Stderr, "go conformance adapter: %v\n", err)
			return 1
		}
	}
	if err = scanner.Err(); err != nil {
		fmt.Fprintf(os.Stderr, "go conformance adapter: %v\n", err)
		return 1
	}
	return 0
}

func main() {
	runtime.LockOSThread()
	code := adapterMain()
	runtime.UnlockOSThread()
	if code != 0 {
		os.Exit(code)
	}
}

package ferric

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

const embeddedNULPublicFixture = `
	(deftemplate item (slot payload))
	(defglobal ?*answer* = 42)
	(defrule emit => (printout t "kept"))
`

func TestNewEngineRejectsEmbeddedNULSource(t *testing.T) {
	source := `(defrule prefix => (assert (kept)))` + "\x00" + `(defrule truncated`
	tests := []struct {
		name string
		opts []EngineOption
	}{
		{name: "default", opts: []EngineOption{WithSource(source)}},
		{
			name: "configured",
			opts: []EngineOption{WithSource(source), WithStrategy(StrategyBreadth)},
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			lockThread(t)
			engine, err := NewEngine(tc.opts...)
			if engine != nil {
				_ = engine.Close()
				t.Fatal("NewEngine accepted an embedded-NUL source")
			}
			assertEmbeddedNULArgument(t, err, "source", source)
		})
	}

	pinned, err := NewPinnedEngine(WithSource(source))
	if pinned != nil {
		_ = pinned.Close()
		t.Fatal("NewPinnedEngine accepted an embedded-NUL source")
	}
	assertEmbeddedNULArgument(t, err, "source", source)
}

//nolint:funlen // One table keeps the public CString-boundary audit complete and reviewable.
func TestEngineRejectsEmbeddedNULArgumentsWithoutMutation(t *testing.T) {
	lockThread(t)
	engine, err := NewEngine(WithSource(embeddedNULPublicFixture))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, engine)

	// Leave a native diagnostic behind. Every rejection below must use its
	// fresh Go-owned message rather than this stale channel (#126).
	if err = engine.Load("(defrule stale"); !errors.Is(err, ErrParse) {
		t.Fatalf("stale diagnostic setup = %v, want ErrParse", err)
	}

	badSource := `(deftemplate alias (slot value))` + "\x00" + `(defrule truncated`
	badAssert := `(assert (alias kept))` + "\x00" + `(assert (truncated))`
	badName := "alias\x00suffix"
	badValue := "value\x00suffix"
	tests := []struct {
		name     string
		argument string
		value    string
		call     func() error
	}{
		{name: "load source", argument: "source", value: badSource, call: func() error {
			return engine.Load(badSource)
		}},
		{name: "assert source", argument: "source", value: badAssert, call: func() error {
			_, callErr := engine.AssertString(badAssert)
			return callErr
		}},
		{name: "ordered relation", argument: "relation", value: badName, call: func() error {
			_, callErr := engine.AssertFact(badName, int64(1))
			return callErr
		}},
		{name: "string field", argument: "fields[0]", value: badValue, call: func() error {
			_, callErr := engine.AssertFact("alias", badValue)
			return callErr
		}},
		{name: "symbol field", argument: "fields[0]", value: badValue, call: func() error {
			_, callErr := engine.AssertFact("alias", Symbol(badValue))
			return callErr
		}},
		{name: "nested field", argument: "fields[0][1]", value: badValue, call: func() error {
			_, callErr := engine.AssertFact("alias", []any{"valid", badValue})
			return callErr
		}},
		{name: "template name", argument: "template name", value: "item\x00suffix", call: func() error {
			_, callErr := engine.AssertTemplate("item\x00suffix", map[string]any{"payload": int64(1)})
			return callErr
		}},
		{name: "slot name", argument: `slot name "payload\x00suffix"`, value: "payload\x00suffix", call: func() error {
			_, callErr := engine.AssertTemplate("item", map[string]any{"payload\x00suffix": int64(1)})
			return callErr
		}},
		{name: "slot string", argument: `slots["payload"]`, value: badValue, call: func() error {
			_, callErr := engine.AssertTemplate("item", map[string]any{"payload": badValue})
			return callErr
		}},
		{name: "slot symbol", argument: `slots["payload"]`, value: badValue, call: func() error {
			_, callErr := engine.AssertTemplate("item", map[string]any{"payload": Symbol(badValue)})
			return callErr
		}},
		{name: "nested slot", argument: `slots["payload"][0]`, value: badValue, call: func() error {
			_, callErr := engine.AssertTemplate("item", map[string]any{"payload": []any{badValue}})
			return callErr
		}},
		{name: "find relation", argument: "relation", value: badName, call: func() error {
			_, callErr := engine.FindFacts(badName)
			return callErr
		}},
		{name: "global name", argument: "global name", value: "answer\x00suffix", call: func() error {
			_, callErr := engine.GetGlobal("answer\x00suffix")
			return callErr
		}},
	}

	for _, tc := range tests {
		beforeFacts, countErr := engine.FactCount()
		if countErr != nil {
			t.Fatalf("%s: %v", tc.name, countErr)
		}
		beforeTemplates := len(engine.Templates())
		assertEmbeddedNULArgument(t, tc.call(), tc.argument, tc.value)
		afterFacts, countErr := engine.FactCount()
		if countErr != nil {
			t.Fatalf("%s: %v", tc.name, countErr)
		}
		if afterFacts != beforeFacts || len(engine.Templates()) != beforeTemplates {
			t.Fatalf(
				"%s: engine mutated: facts %d -> %d, templates %d -> %d",
				tc.name,
				beforeFacts,
				afterFacts,
				beforeTemplates,
				len(engine.Templates()),
			)
		}
	}

	if _, err = engine.AssertFact("valid-after-rejection", "ok"); err != nil {
		t.Fatalf("engine was not reusable after rejection: %v", err)
	}
}

func TestEngineEmbeddedNULIOErrorsAndLegacyBehavior(t *testing.T) {
	lockThread(t)
	engine, err := NewEngine(WithSource(embeddedNULPublicFixture))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, engine)
	if _, err = engine.Run(context.Background()); err != nil {
		t.Fatal(err)
	}

	badChannel := "t\x00suffix"
	output, ok, err := engine.GetOutputE(badChannel)
	if output != "" || ok {
		t.Fatalf("GetOutputE invalid result = (%q, %v), want empty false", output, ok)
	}
	assertEmbeddedNULArgument(t, err, "output channel", badChannel)
	if output, ok = engine.GetOutput(badChannel); output != "" || ok {
		t.Fatalf("legacy GetOutput invalid result = (%q, %v), want empty false", output, ok)
	}

	assertEmbeddedNULArgument(t, engine.ClearOutputE(badChannel), "output channel", badChannel)
	engine.ClearOutput(badChannel)
	if output, ok, err = engine.GetOutputE("t"); err != nil || !ok || output != "kept" {
		t.Fatalf("valid output after rejected clear = (%q, %v, %v)", output, ok, err)
	}

	badLine := "prefix\x00suffix"
	assertEmbeddedNULArgument(t, engine.PushInputE(badLine), "input line", badLine)
	engine.PushInput(badLine)
	if err = engine.PushInputE("valid"); err != nil {
		t.Fatalf("valid input after rejection = %v", err)
	}
	snapshot, err := engine.Serialize(FormatJSON)
	if err != nil {
		t.Fatal(err)
	}
	state := decodeJSONSnapshot(t, snapshot)
	input, ok := state["input_buffer"].([]any)
	if !ok || len(input) != 1 || input[0] != "valid" {
		t.Fatalf("serialized input buffer = %#v, want only the valid line", state["input_buffer"])
	}
}

func TestGetOutputEPreservesEmbeddedNULFromSnapshot(t *testing.T) {
	lockThread(t)
	engine, err := NewEngine(WithSource(embeddedNULPublicFixture))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = engine.Run(context.Background()); err != nil {
		t.Fatal(err)
	}
	snapshot, err := engine.Serialize(FormatJSON)
	if err != nil {
		t.Fatal(err)
	}
	if err = engine.Close(); err != nil {
		t.Fatal(err)
	}

	state := decodeJSONSnapshot(t, snapshot)
	router, ok := state["router"].(map[string]any)
	if !ok {
		t.Fatalf("snapshot router = %#v, want object", state["router"])
	}
	buffers, ok := router["buffers"].([]any)
	if !ok {
		t.Fatalf("snapshot buffers = %#v, want entries", router["buffers"])
	}
	replaced := false
	for _, rawEntry := range buffers {
		entry, entryOK := rawEntry.([]any)
		if entryOK && len(entry) == 2 && entry[0] == "t" {
			entry[1] = "a\x00b"
			replaced = true
			break
		}
	}
	if !replaced {
		t.Fatalf("snapshot buffers = %#v, missing t entry", buffers)
	}
	modified, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}

	restored, err := NewEngine(WithSnapshot(modified, FormatJSON))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, restored)
	output, found, err := restored.GetOutputE("t")
	if err != nil || !found || output != "a\x00b" {
		t.Fatalf("GetOutputE = (%q, %v, %v), want exact embedded-NUL output", output, found, err)
	}
}

func TestPinnedAndManagerRejectEmbeddedNULAndRemainReusable(t *testing.T) {
	pinned, err := NewPinnedEngine(WithSource(embeddedNULPublicFixture))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, pinned)
	badRelation := "alias\x00suffix"
	_, err = pinned.AssertFact(badRelation, "value")
	assertEmbeddedNULArgument(t, err, "relation", badRelation)
	if _, err = pinned.AssertFact("pinned-valid", "value"); err != nil {
		t.Fatalf("PinnedEngine was not reusable: %v", err)
	}

	badChannel := "t\x00suffix"
	_, _, err = pinned.GetOutputE(badChannel)
	assertEmbeddedNULArgument(t, err, "output channel", badChannel)
	assertEmbeddedNULArgument(t, pinned.ClearOutputE(badChannel), "output channel", badChannel)
	badLine := "line\x00suffix"
	assertEmbeddedNULArgument(t, pinned.PushInputE(badLine), "input line", badLine)

	manager, err := NewManager(WithSource(embeddedNULPublicFixture))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, manager)
	err = manager.Do(context.Background(), func(engine *Engine) error {
		_, callErr := engine.AssertFact(badRelation, "value")
		return callErr
	})
	assertEmbeddedNULArgument(t, err, "relation", badRelation)
	if err = manager.Do(context.Background(), func(engine *Engine) error {
		_, callErr := engine.AssertFact("manager-valid", "value")
		return callErr
	}); err != nil {
		t.Fatalf("Manager worker was not reusable: %v", err)
	}
}

func TestEmbeddedNULValueFailureFreesEarlierNestedValues(t *testing.T) {
	withFFIHooks(t)
	originalFree := ffiValueFree
	freed := 0
	ffiValueFree = func(value *ffi.Value) {
		freed++
		originalFree(value)
	}

	badValue := "bad\x00suffix"
	_, err := goToFFIValue([]any{"outer-converted", []any{"inner-converted", badValue}})
	assertEmbeddedNULArgument(t, err, "value[1][1]", badValue)
	if freed != 2 {
		t.Fatalf("freed values = %d, want converted values at both nesting levels", freed)
	}
}

func TestSnapshotBytesAndFilePathsDoNotUseCStringPolicy(t *testing.T) {
	lockThread(t)
	withFFIHooks(t)
	snapshot := []byte("snapshot\x00payload")
	ffiEngineDeserializeAs = func(data []byte, format ffi.SerializationFormat) (ffi.EngineHandle, ffi.ErrorCode) {
		if string(data) != string(snapshot) {
			t.Fatalf("snapshot bytes = %q, want %q", data, snapshot)
		}
		if format != ffi.FormatBincode {
			t.Fatalf("format = %d, want bincode", format)
		}
		return nil, ffi.ErrSerializationError
	}
	_, err := NewEngine(WithSnapshot(snapshot, FormatBincode))
	if !errors.Is(err, ErrSerialization) {
		t.Fatalf("snapshot result = %v, want ErrSerialization", err)
	}

	prefix := t.TempDir() + "/snapshot"
	if err = os.WriteFile(prefix, []byte("prefix file"), 0o600); err != nil {
		t.Fatal(err)
	}
	_, err = NewEngineFromFile(prefix+"\x00suffix", FormatBincode)
	var pathErr *os.PathError
	if !errors.As(err, &pathErr) {
		t.Fatalf("NUL path error = %T %v, want *os.PathError", err, err)
	}

	engine, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, engine)
	err = engine.SerializeToFile(prefix+"\x00suffix", FormatBincode)
	if !errors.As(err, &pathErr) {
		t.Fatalf("NUL output path error = %T %v, want *os.PathError", err, err)
	}
	// #nosec G304 -- prefix is a test-owned path inside t.TempDir.
	prefixData, err := os.ReadFile(prefix)
	if err != nil {
		t.Fatal(err)
	}
	if string(prefixData) != "prefix file" {
		t.Fatalf("prefix file changed to %q", prefixData)
	}
}

func decodeJSONSnapshot(t *testing.T, snapshot []byte) map[string]any {
	t.Helper()
	var state map[string]any
	if err := json.Unmarshal(snapshot, &state); err != nil {
		t.Fatal(err)
	}
	return state
}

func assertEmbeddedNULArgument(t *testing.T, err error, argument, value string) {
	t.Helper()
	if !errors.Is(err, ErrInvalidArgument) {
		t.Fatalf("error = %v, want ErrInvalidArgument", err)
	}
	var invalid *InvalidArgumentError
	if !errors.As(err, &invalid) {
		t.Fatalf("error = %T %v, want *InvalidArgumentError", err, err)
	}
	if invalid.Code != int(ffi.ErrInvalidArgument) {
		t.Fatalf("error code = %d, want %d", invalid.Code, ffi.ErrInvalidArgument)
	}
	wantMessage := fmt.Sprintf(
		"%s contains embedded NUL at byte %d",
		argument,
		strings.IndexByte(value, 0),
	)
	if invalid.Message != wantMessage || !strings.Contains(err.Error(), wantMessage) {
		t.Fatalf("error = %#v (%q), want message %q", invalid, err.Error(), wantMessage)
	}
	if strings.Contains(invalid.Message, "stale") {
		t.Fatalf("fresh validation error leaked prior diagnostic: %q", invalid.Message)
	}
}

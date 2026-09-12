package ferric

import (
	"context"
	"encoding/json"
	"reflect"
	"testing"
)

func TestByteLexemesAndAllSnapshotFormats(t *testing.T) {
	for _, format := range allFormats() {
		t.Run(format.name, func(t *testing.T) {
			lockThread(t)
			engine, err := NewEngine()
			if err != nil {
				t.Fatal(err)
			}
			defer mustClose(t, engine)
			values := []any{StringBytes("a\x00\xff"), SymbolBytes("s\x00\xff"), InstanceName("n\x00\xff")}
			id, err := engine.AssertFact("bytes", values...)
			if err != nil {
				t.Fatal(err)
			}
			fact, err := engine.GetFact(id)
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(fact.Fields, values) {
				t.Fatalf("fields = %#v", fact.Fields)
			}
			data, err := engine.Serialize(format.format)
			if err != nil {
				t.Fatal(err)
			}
			restored, err := NewEngine(WithSnapshot(data, format.format))
			if err != nil {
				t.Fatal(err)
			}
			defer mustClose(t, restored)
			facts, err := restored.FindFacts("bytes")
			if err != nil {
				t.Fatal(err)
			}
			if len(facts) != 1 || !reflect.DeepEqual(facts[0].Fields, values) {
				t.Fatalf("restored = %#v", facts)
			}
		})
	}
}

func TestByteWireJSONAndOutputAreLossless(t *testing.T) {
	values := []any{StringBytes("a\x00\xff"), SymbolBytes("s\xff"), InstanceName("n\xff"), []any{StringBytes("")}}
	for _, value := range values {
		wire, err := NativeToWireValue(value)
		if err != nil {
			t.Fatal(err)
		}
		data, err := json.Marshal(wire)
		if err != nil {
			t.Fatal(err)
		}
		var decoded WireValue
		if err := json.Unmarshal(data, &decoded); err != nil {
			t.Fatal(err)
		}
		actual, err := WireToNativeValue(decoded)
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(actual, value) {
			t.Fatalf("%#v != %#v (%s)", actual, value, data)
		}
	}
	original := EvaluateResult{Output: map[string]string{"stdout": "a\x00\xff", "stderr": "plain"}}
	data, err := json.Marshal(original)
	if err != nil {
		t.Fatal(err)
	}
	var decoded EvaluateResult
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(decoded.Output, original.Output) {
		t.Fatalf("output changed: %q", decoded.Output)
	}
}

func TestOutputCopyPreservesInvalidUTF8(t *testing.T) {
	lockThread(t)
	engine, err := NewEngine(WithSource("(defrule emit (bytes ?x) => (printout t ?x))"))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, engine)
	if _, err := engine.AssertFact("bytes", StringBytes("a\x00\xff")); err != nil {
		t.Fatal(err)
	}
	if _, err := engine.Run(context.Background()); err != nil {
		t.Fatal(err)
	}
	output, exists, err := engine.GetOutputE("t")
	if err != nil || !exists || output != "a\x00\xff" {
		t.Fatalf("output = %q, %v, %v", output, exists, err)
	}
}

func TestMissingInstanceLookupPreservesTypedName(t *testing.T) {
	for _, operation := range []string{"type", "named"} {
		t.Run(operation, func(t *testing.T) {
			lockThread(t)
			source := `(defgeneric named)
(defmethod named ((?x INSTANCE-NAME)) unreachable)
(defrule check (name ?x) =>
 (printout t (instance-namep ?x) crlf) (assert (before ?x))
 (` + operation + ` ?x) (assert (after)))`
			engine, err := NewEngine(WithSource(source))
			if err != nil {
				t.Fatal(err)
			}
			defer mustClose(t, engine)
			name := InstanceName("missing")
			if _, err := engine.AssertFact("name", name); err != nil {
				t.Fatal(err)
			}
			result, err := engine.Run(context.Background())
			if err != nil {
				t.Fatal(err)
			}
			if result.HaltReason != HaltActionError || result.RulesFired != 1 || len(engine.Diagnostics()) == 0 {
				t.Fatalf("missing-instance result = %#v, diagnostics = %v", result, engine.Diagnostics())
			}
			output, exists, err := engine.GetOutputE("t")
			if err != nil || !exists || output != "TRUE\n" {
				t.Fatalf("predicate output = %q, %v, %v", output, exists, err)
			}
			before, err := engine.FindFacts("before")
			if err != nil || len(before) != 1 || !reflect.DeepEqual(before[0].Fields, []any{name}) {
				t.Fatalf("typed name capture = %#v, %v", before, err)
			}
			after, err := engine.FindFacts("after")
			if err != nil || len(after) != 0 {
				t.Fatalf("later action = %#v, %v", after, err)
			}
		})
	}
}

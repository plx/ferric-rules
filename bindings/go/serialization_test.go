package ferric

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

// ---------------------------------------------------------------------------
// Engine.Serialize / WithSnapshot roundtrip — all formats
// ---------------------------------------------------------------------------

func allFormats() []struct {
	name   string
	format Format
} {
	return []struct {
		name   string
		format Format
	}{
		{"Bincode", FormatBincode},
		{"JSON", FormatJSON},
		{"CBOR", FormatCBOR},
		{"MessagePack", FormatMessagePack},
		{"Postcard", FormatPostcard},
	}
}

func TestSerializeRoundtrip(t *testing.T) {
	for _, tc := range allFormats() {
		t.Run(tc.name, func(t *testing.T) {
			lockThread(t)
			src := `
				(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
				(defrule alert
					(sensor (id ?id) (value ?v&:(> ?v 100.0)))
					=>
					(printout t "ALERT: sensor " ?id " value " ?v crlf))
				(defglobal ?*threshold* = 42)
			`
			e, err := NewEngine(WithSource(src))
			if err != nil {
				t.Fatal(err)
			}

			snap, err := e.Serialize(tc.format)
			if err != nil {
				t.Fatalf("Serialize failed: %v", err)
			}
			mustClose(t, e)

			if len(snap) == 0 {
				t.Fatal("expected non-empty snapshot")
			}

			e2, err := NewEngine(WithSnapshot(snap, tc.format))
			if err != nil {
				t.Fatalf("NewEngine(WithSnapshot) failed: %v", err)
			}
			defer mustClose(t, e2)

			rules := e2.Rules()
			if len(rules) != 1 {
				t.Fatalf("expected 1 rule, got %d", len(rules))
			}
			if rules[0].Name != "alert" {
				t.Fatalf("expected rule 'alert', got %q", rules[0].Name)
			}

			tmpls := e2.Templates()
			if !slices.Contains(tmpls, "sensor") {
				t.Fatalf("expected 'sensor' template, got: %v", tmpls)
			}

			val, err := e2.GetGlobal("threshold")
			if err != nil {
				t.Fatalf("GetGlobal failed: %v", err)
			}
			if val != int64(42) {
				t.Fatalf("expected 42, got %v", val)
			}
		})
	}
}

func TestSnapshotEngineCanRun(t *testing.T) {
	for _, tc := range allFormats() {
		t.Run(tc.name, func(t *testing.T) {
			lockThread(t)
			src := `
				(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
				(defrule alert
					(sensor (id ?id) (value ?v&:(> ?v 100.0)))
					=>
					(printout t "ALERT " ?id crlf))
			`
			e, err := NewEngine(WithSource(src))
			if err != nil {
				t.Fatal(err)
			}
			snap, err := e.Serialize(tc.format)
			if err != nil {
				t.Fatal(err)
			}
			mustClose(t, e)

			e2, err := NewEngine(WithSnapshot(snap, tc.format))
			if err != nil {
				t.Fatal(err)
			}
			defer mustClose(t, e2)

			_, err = e2.AssertTemplate("sensor", map[string]any{
				"id":    int64(7),
				"value": 200.0,
			})
			if err != nil {
				t.Fatal(err)
			}

			result, err := e2.Run(context.Background())
			if err != nil {
				t.Fatal(err)
			}
			if result.RulesFired != 1 {
				t.Fatalf("expected 1 rule fired, got %d", result.RulesFired)
			}

			output, ok := e2.GetOutput("t")
			if !ok {
				t.Fatal("expected output")
			}
			if output != "ALERT 7\n" {
				t.Fatalf("unexpected output: %q", output)
			}
		})
	}
}

func TestSnapshotMultipleInstances(t *testing.T) {
	lockThread(t)
	src := `
		(defrule count => (assert (counted)))
	`
	e, err := NewEngine(WithSource(src))
	if err != nil {
		t.Fatal(err)
	}
	snap, err := e.Serialize(FormatBincode)
	if err != nil {
		t.Fatal(err)
	}
	mustClose(t, e)

	e1, err := NewEngine(WithSnapshot(snap, FormatBincode))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e1)

	e2, err := NewEngine(WithSnapshot(snap, FormatBincode))
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e2)

	result1 := mustRun(context.Background(), t, e1)
	if result1.RulesFired != 1 {
		t.Fatalf("e1: expected 1 fired, got %d", result1.RulesFired)
	}

	result2 := mustRun(context.Background(), t, e2)
	if result2.RulesFired != 1 {
		t.Fatalf("e2: expected 1 fired, got %d", result2.RulesFired)
	}
}

func TestDeserializeInvalidData(t *testing.T) {
	for _, tc := range allFormats() {
		t.Run(tc.name, func(t *testing.T) {
			lockThread(t)
			_, err := NewEngine(WithSnapshot([]byte("not valid snapshot data"), tc.format))
			if err == nil {
				t.Fatal("expected error for invalid snapshot")
			}
			if !errors.Is(err, ErrSerialization) {
				t.Fatalf("expected SerializationError, got: %T %v", err, err)
			}
		})
	}
}

func TestSerializeEmptyEngine(t *testing.T) {
	for _, tc := range allFormats() {
		t.Run(tc.name, func(t *testing.T) {
			lockThread(t)
			e, err := NewEngine()
			if err != nil {
				t.Fatal(err)
			}
			defer mustClose(t, e)

			snap, err := e.Serialize(tc.format)
			if err != nil {
				t.Fatalf("Serialize failed: %v", err)
			}
			if len(snap) == 0 {
				t.Fatal("expected non-empty snapshot even for empty engine")
			}

			e2, err := NewEngine(WithSnapshot(snap, tc.format))
			if err != nil {
				t.Fatal(err)
			}
			defer mustClose(t, e2)

			err = e2.Load(`(defrule r => (assert (done)))`)
			if err != nil {
				t.Fatalf("Load into restored engine failed: %v", err)
			}
			result := mustRun(context.Background(), t, e2)
			if result.RulesFired != 1 {
				t.Fatalf("expected 1, got %d", result.RulesFired)
			}
		})
	}
}

func TestCrossFormatRejection(t *testing.T) {
	lockThread(t)
	// Serialize as bincode, try to deserialize as JSON — should fail.
	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	snap, err := e.Serialize(FormatBincode)
	if err != nil {
		t.Fatal(err)
	}

	_, err = NewEngine(WithSnapshot(snap, FormatJSON))
	if err == nil {
		t.Fatal("expected error when deserializing bincode as JSON")
	}
}

func TestSerializeToFileRoundtrip(t *testing.T) {
	for _, tc := range allFormats() {
		t.Run(tc.name, func(t *testing.T) {
			lockThread(t)
			src := `
				(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
				(defrule alert
					(sensor (id ?id) (value ?v&:(> ?v 100.0)))
					=>
					(printout t "ALERT " ?id crlf))
				(defglobal ?*threshold* = 42)
			`
			e, err := NewEngine(WithSource(src))
			if err != nil {
				t.Fatal(err)
			}
			defer mustClose(t, e)

			path := filepath.Join(t.TempDir(), "snapshot.bin")
			if err := e.SerializeToFile(path, tc.format); err != nil {
				t.Fatalf("SerializeToFile failed: %v", err)
			}

			info, err := os.Stat(path)
			if err != nil {
				t.Fatalf("snapshot file not found: %v", err)
			}
			if info.Size() == 0 {
				t.Fatal("snapshot file is empty")
			}

			e2, err := NewEngineFromFile(path, tc.format)
			if err != nil {
				t.Fatalf("NewEngineFromFile failed: %v", err)
			}
			defer mustClose(t, e2)

			rules := e2.Rules()
			if len(rules) != 1 {
				t.Fatalf("expected 1 rule, got %d", len(rules))
			}
			if rules[0].Name != "alert" {
				t.Fatalf("expected rule 'alert', got %q", rules[0].Name)
			}

			val, err := e2.GetGlobal("threshold")
			if err != nil {
				t.Fatalf("GetGlobal failed: %v", err)
			}
			if val != int64(42) {
				t.Fatalf("expected 42, got %v", val)
			}
		})
	}
}

func TestNewEngineFromFileNonexistent(t *testing.T) {
	_, err := NewEngineFromFile("/nonexistent/path/snap.bin", FormatBincode)
	if err == nil {
		t.Fatal("expected error for nonexistent file")
	}
}

func TestSerializeToFileUnwritable(t *testing.T) {
	lockThread(t)
	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	err = e.SerializeToFile("/nonexistent/dir/snap.bin", FormatBincode)
	if err == nil {
		t.Fatal("expected error for unwritable path")
	}
}

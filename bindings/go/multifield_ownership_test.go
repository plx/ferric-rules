package ferric

import (
	"errors"
	"reflect"
	"testing"
)

//nolint:funlen // The table keeps the required empty, nested, mixed, and large ownership cases together.
func TestMultifieldAllocatorProvenanceRoundTrip(t *testing.T) {
	lockThread(t)

	engine, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, engine)
	mustNoError(t, engine.Reset())

	large := make([]any, 4096)
	for i := range large {
		large[i] = int64(i)
	}

	cases := []struct {
		name  string
		input []any
		want  []any
	}{
		{
			name:  "empty",
			input: []any{},
			want:  []any{},
		},
		{
			name: "nested",
			input: []any{
				Symbol("outer"),
				[]any{
					int64(7),
					[]any{"deep", Symbol("leaf")},
				},
			},
			want: []any{
				Symbol("outer"),
				[]any{
					int64(7),
					[]any{"deep", Symbol("leaf")},
				},
			},
		},
		{
			name:  "mixed",
			input: []any{nil, int32(3), float32(1.25), Symbol("sym"), "text", true, false},
			want:  []any{nil, int64(3), float64(float32(1.25)), Symbol("sym"), "text", Symbol("TRUE"), Symbol("FALSE")},
		},
		{
			name:  "large",
			input: large,
			want:  large,
		},
	}

	for _, tc := range cases {
		roundTrips := 1
		if tc.name == "nested" {
			roundTrips = 100
		}
		for range roundTrips {
			factID, assertErr := engine.AssertFact("multifield-ownership", tc.input)
			if assertErr != nil {
				t.Fatalf("%s: AssertFact failed: %v", tc.name, assertErr)
			}
			fact, getErr := engine.GetFact(factID)
			if getErr != nil {
				t.Fatalf("%s: GetFact failed: %v", tc.name, getErr)
			}
			if len(fact.Fields) != 1 {
				t.Fatalf("%s: field count = %d, want 1", tc.name, len(fact.Fields))
			}
			if !reflect.DeepEqual(fact.Fields[0], tc.want) {
				t.Fatalf("%s: round trip = %#v, want %#v", tc.name, fact.Fields[0], tc.want)
			}
			if retractErr := engine.Retract(factID); retractErr != nil {
				t.Fatalf("%s: Retract failed: %v", tc.name, retractErr)
			}
		}
	}
}

func TestMultifieldCopyDepthBoundaryCleansUp(t *testing.T) {
	var nested any = int64(1)
	for range maxMultifieldNestingDepth {
		nested = []any{nested}
	}

	value, err := goToFFIValue(nested)
	if err != nil {
		t.Fatalf("%d-level multifield failed: %v", maxMultifieldNestingDepth, err)
	}
	ffiValueFree(&value)

	nested = []any{nested}
	value, err = goToFFIValue(nested)
	if !errors.Is(err, ErrInvalidArgument) {
		if err == nil {
			ffiValueFree(&value)
		}
		t.Fatalf("%d-level multifield error = %v, want ErrInvalidArgument", maxMultifieldNestingDepth+1, err)
	}
}

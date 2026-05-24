//nolint:funlen,gocyclo,maintidx // Coverage edge tests intentionally enumerate related FFI branches together.
package ffi

import (
	"bytes"
	"math"
	"testing"
	"unsafe"

	"pgregory.net/rapid"
)

func ffiFormats() []SerializationFormat {
	return []SerializationFormat{
		FormatBincode,
		FormatJSON,
		FormatCBOR,
		FormatMessagePack,
		FormatPostcard,
	}
}

func TestManualValueAccessorsAndConstructors(t *testing.T) {
	// The accessor helpers are the only safe way for the outer Go package to
	// inspect cgo-owned FerricValue fields. This covers each public value shape.
	integer := ValueInteger(-42)
	if ValueGetType(&integer) != ValueTypeInteger || ValueGetInteger(&integer) != -42 {
		t.Fatalf("integer accessor mismatch")
	}

	float := ValueFloat(1.25)
	if ValueGetType(&float) != ValueTypeFloat || ValueGetFloat(&float) != 1.25 {
		t.Fatalf("float accessor mismatch")
	}

	void := ValueVoid()
	if ValueGetType(&void) != ValueTypeVoid {
		t.Fatalf("void accessor mismatch")
	}
	if got := ValueGetStringPtr(&void); got != "" {
		t.Fatalf("nil string pointer = %q, want empty", got)
	}

	symbol := ValueSymbol("sym")
	if ValueGetType(&symbol) != ValueTypeSymbol || ValueGetStringPtr(&symbol) != "sym" {
		t.Fatalf("symbol accessor mismatch")
	}
	ValueFree(&symbol)

	str := ValueString("hello")
	if ValueGetType(&str) != ValueTypeString || ValueGetStringPtr(&str) != "hello" {
		t.Fatalf("string accessor mismatch")
	}
	ValueFree(&str)

	multifield := ValueMultifield([]Value{
		ValueInteger(1),
		ValueString("nested"),
	})
	if ValueGetType(&multifield) != ValueTypeMultifield {
		t.Fatalf("multifield type mismatch")
	}
	if got := ValueGetMultifieldLen(&multifield); got != 2 {
		t.Fatalf("multifield length = %d, want 2", got)
	}
	if elem := ValueGetMultifieldElement(&multifield, 1); ValueGetStringPtr(&elem) != "nested" {
		t.Fatalf("multifield element mismatch")
	}
	ValueFree(&multifield)

	emptyMultifield := ValueMultifield(nil)
	if ValueGetType(&emptyMultifield) != ValueTypeMultifield || ValueGetMultifieldLen(&emptyMultifield) != 0 {
		t.Fatalf("empty multifield accessor mismatch")
	}
	ValueFree(&emptyMultifield)

	marker := 7
	var external Value
	external.value_type = ValueTypeExternalAddress
	external.external_pointer = unsafe.Pointer(&marker)
	if got := ValueGetExternalPointer(&external); got != unsafe.Pointer(&marker) {
		t.Fatalf("external pointer = %v, want %v", got, unsafe.Pointer(&marker))
	}

	cfg := MakeConfig(StringEncodingUTF8, ConflictStrategyLEX, 512)
	if cfg == nil {
		t.Fatal("MakeConfig returned nil")
	}

	StringFree(nil)
	ValueArrayFree(nil, 0)
}

func TestManualConfigConstructorsOutputDiagnosticsAndErrors(t *testing.T) {
	// These examples exercise FFI constructors and mutable side channels that
	// the high-level package usually reaches indirectly.
	lockThread(t)

	cfg := MakeConfig(StringEncodingUTF8, ConflictStrategyDepth, 256)
	h := EngineNewWithConfig(cfg)
	if h == nil {
		t.Fatal("EngineNewWithConfig returned nil")
	}
	if rc := EngineFree(h); rc != ErrOK {
		t.Fatalf("EngineFree returned %d", rc)
	}

	h = EngineNewWithSourceConfig(`(defrule hello => (printout t "hi" crlf))`, cfg)
	if h == nil {
		t.Fatal("EngineNewWithSourceConfig returned nil")
	}
	defer EngineFree(h)

	fired, reason, rc := EngineRunEx(h, -1)
	if rc != ErrOK || fired != 1 || reason != HaltReasonAgendaEmpty {
		t.Fatalf("run = (%d, %d, %d), want one agenda-empty firing", fired, reason, rc)
	}
	if out, ok := EngineGetOutput(h, "t"); !ok || out != "hi\n" {
		t.Fatalf("stdout = (%q, %v), want hi", out, ok)
	}
	if rc := EngineClearOutput(h, "t"); rc != ErrOK {
		t.Fatalf("EngineClearOutput returned %d", rc)
	}
	if out, ok := EngineGetOutput(h, "t"); ok || out != "" {
		t.Fatalf("cleared stdout = (%q, %v), want empty false", out, ok)
	}

	diag := EngineNewWithSource(`(defrule boom => (/ 1 0))`)
	if diag == nil {
		t.Fatal("diagnostic engine returned nil")
	}
	defer EngineFree(diag)
	_, _, _ = EngineRunEx(diag, -1)
	count, rc := EngineActionDiagnosticCount(diag)
	if rc != ErrOK {
		t.Fatalf("EngineActionDiagnosticCount returned %d", rc)
	}
	for i := range count {
		if _, rc := EngineActionDiagnosticCopy(diag, i); rc != ErrOK {
			t.Fatalf("EngineActionDiagnosticCopy(%d) returned %d", i, rc)
		}
	}
	if rc := EngineClearActionDiagnostics(diag); rc != ErrOK {
		t.Fatalf("EngineClearActionDiagnostics returned %d", rc)
	}

	introspect := EngineNewWithSource(`(deftemplate sensor (slot id))`)
	if introspect == nil {
		t.Fatal("introspection engine returned nil")
	}
	defer EngineFree(introspect)
	name, rc := EngineTemplateName(introspect, 0)
	if rc != ErrOK || name != "sensor" {
		t.Fatalf("EngineTemplateName = (%q, %d), want sensor OK", name, rc)
	}

	if rc := EngineLoadString(introspect, "(defrule bad"); rc == ErrOK {
		t.Fatal("invalid source should fail")
	}
	if _, rc := EngineLastErrorCopy(introspect); rc != ErrOK {
		t.Fatalf("EngineLastErrorCopy returned %d", rc)
	}
	EngineClearError(introspect)
	if msg := EngineLastError(introspect); msg != "" {
		t.Fatalf("cleared engine error = %q, want empty", msg)
	}

	invalid := EngineNewWithSource("(defrule bad")
	if invalid != nil {
		_ = EngineFree(invalid)
		t.Fatal("invalid source unexpectedly created an engine")
	}
	if _, rc := LastErrorGlobalCopy(); rc != ErrOK {
		t.Fatalf("LastErrorGlobalCopy returned %d", rc)
	}
	ClearErrorGlobal()
	if msg := LastErrorGlobal(); msg != "" {
		t.Fatalf("cleared global error = %q, want empty", msg)
	}
}

func TestManualFactIDWrapperEdges(t *testing.T) {
	// Fact ID wrappers use a two-call C buffer pattern. Empty, nil-handle, and
	// populated calls prove the Go wrapper handles all caller-visible outcomes.
	lockThread(t)

	if ids, rc := EngineFactIDs(nil); rc == ErrOK || ids != nil {
		t.Fatalf("EngineFactIDs nil = (%v, %d), want nil error", ids, rc)
	}
	if ids, rc := EngineFindFactIDs(nil, "x"); rc == ErrOK || ids != nil {
		t.Fatalf("EngineFindFactIDs nil = (%v, %d), want nil error", ids, rc)
	}

	h := EngineNew()
	if h == nil {
		t.Fatal("EngineNew returned nil")
	}
	defer EngineFree(h)
	if rc := EngineReset(h); rc != ErrOK {
		t.Fatalf("EngineReset returned %d", rc)
	}
	if ids, rc := EngineFactIDs(h); rc != ErrOK || len(ids) != 0 {
		t.Fatalf("empty EngineFactIDs = (%v, %d), want empty OK", ids, rc)
	}
	if ids, rc := EngineFindFactIDs(h, "missing"); rc != ErrOK || len(ids) != 0 {
		t.Fatalf("empty EngineFindFactIDs = (%v, %d), want empty OK", ids, rc)
	}
	if _, rc := EngineAssertString(h, "(assert (color red))"); rc != ErrOK {
		t.Fatalf("EngineAssertString returned %d", rc)
	}
	if ids, rc := EngineFactIDs(h); rc != ErrOK || len(ids) != 1 {
		t.Fatalf("populated EngineFactIDs = (%v, %d), want one OK", ids, rc)
	}
	if ids, rc := EngineFindFactIDs(h, "color"); rc != ErrOK || len(ids) != 1 {
		t.Fatalf("populated EngineFindFactIDs = (%v, %d), want one OK", ids, rc)
	}
}

func TestManualSerializationWrappers(t *testing.T) {
	// Direct serde tests keep the internal FFI wrappers covered independently
	// from the higher-level Engine.Serialize tests in the parent package.
	lockThread(t)

	for _, format := range ffiFormats() {
		h := EngineNewWithSource(`(defrule r => (assert (ok)))`)
		if h == nil {
			t.Fatal("EngineNewWithSource returned nil")
		}

		data, rc := EngineSerializeAs(h, format)
		if rc != ErrOK {
			t.Fatalf("EngineSerializeAs(%d) returned %d", format, rc)
		}
		if len(data) == 0 {
			t.Fatalf("EngineSerializeAs(%d) returned empty data", format)
		}
		if rc := EngineFree(h); rc != ErrOK {
			t.Fatalf("EngineFree returned %d", rc)
		}

		restored, rc := EngineDeserializeAs(data, format)
		if rc != ErrOK || restored == nil {
			t.Fatalf("EngineDeserializeAs(%d) = (%v, %d), want handle OK", format, restored, rc)
		}
		if rc := EngineFree(restored); rc != ErrOK {
			t.Fatalf("EngineFree restored returned %d", rc)
		}
	}

	if data, rc := EngineSerializeAs(nil, FormatBincode); rc == ErrOK || data != nil {
		t.Fatalf("EngineSerializeAs nil = (%v, %d), want nil error", data, rc)
	}
	if h, rc := EngineDeserializeAs(nil, FormatBincode); rc != ErrInvalidArgument || h != nil {
		t.Fatalf("EngineDeserializeAs empty = (%v, %d), want invalid argument", h, rc)
	}
	if h, rc := EngineDeserializeAs([]byte("not a snapshot"), FormatJSON); rc == ErrOK || h != nil {
		t.Fatalf("EngineDeserializeAs invalid = (%v, %d), want nil error", h, rc)
	}
}

func TestManualCopyAndFreeBytesBranches(t *testing.T) {
	// Serialization copies Rust-owned bytes and must always invoke the provided
	// free callback, including the zero-length edge case.
	freed := false
	if got := copyAndFreeBytes(nil, 0, func() { freed = true }); got != nil || !freed {
		t.Fatalf("zero copy = (%v, freed=%v), want nil freed", got, freed)
	}

	src := []byte{1, 2, 3}
	freed = false
	got := copyAndFreeBytes(unsafe.Pointer(&src[0]), uintptr(len(src)), func() { freed = true })
	if !freed || len(got) != len(src) || got[0] != 1 || got[2] != 3 {
		t.Fatalf("copy = (%v, freed=%v), want copied bytes freed", got, freed)
	}
	src[0] = 9
	if got[0] != 1 {
		t.Fatalf("copy must be Go-owned, got mutated first byte %d", got[0])
	}
}

func TestManualStringFromTwoCallBranches(t *testing.T) {
	// The pure helper under getStringWith owns the allocation and error handling
	// contract for all string-copy FFI wrappers.
	if got, rc := stringFromTwoCall(func(_ []byte) (uintptr, ErrorCode) {
		return 0, ErrRuntimeError
	}); rc != ErrRuntimeError || got != "" {
		t.Fatalf("first-call failure = (%q, %d), want runtime error", got, rc)
	}

	if got, rc := stringFromTwoCall(func(_ []byte) (uintptr, ErrorCode) {
		return 0, ErrOK
	}); rc != ErrOK || got != "" {
		t.Fatalf("empty result = (%q, %d), want empty OK", got, rc)
	}

	calls := 0
	if got, rc := stringFromTwoCall(func(_ []byte) (uintptr, ErrorCode) {
		calls++
		if calls == 1 {
			return 2, ErrOK
		}
		return 2, ErrBufferTooSmall
	}); rc != ErrBufferTooSmall || got != "" {
		t.Fatalf("second-call failure = (%q, %d), want buffer-too-small", got, rc)
	}

	calls = 0
	if got, rc := stringFromTwoCall(func(buf []byte) (uintptr, ErrorCode) {
		calls++
		if calls == 1 {
			return 3, ErrOK
		}
		copy(buf, []byte{'o', 'k', 0})
		return 3, ErrOK
	}); rc != ErrOK || got != "ok" {
		t.Fatalf("copied string = (%q, %d), want ok", got, rc)
	}

	calls = 0
	if got, rc := stringFromTwoCall(func(buf []byte) (uintptr, ErrorCode) {
		calls++
		if calls == 1 {
			return 1, ErrOK
		}
		if len(buf) > 0 {
			buf[0] = 0
		}
		return 0, ErrOK
	}); rc != ErrOK || got != "" {
		t.Fatalf("zero-length post-copy = (%q, %d), want empty OK", got, rc)
	}
}

func TestManualUint64IDsFromTwoCallBranches(t *testing.T) {
	// Fact ID wrappers share this helper, so fake callbacks cover the count
	// query, empty result, second-call failure, and successful fill branches.
	if got, rc := uint64IDsFromTwoCall(func(_ []uint64) (uintptr, ErrorCode) {
		return 0, ErrRuntimeError
	}); rc != ErrRuntimeError || got != nil {
		t.Fatalf("first-call failure = (%v, %d), want nil runtime error", got, rc)
	}

	if got, rc := uint64IDsFromTwoCall(func(_ []uint64) (uintptr, ErrorCode) {
		return 0, ErrOK
	}); rc != ErrOK || got != nil {
		t.Fatalf("empty ids = (%v, %d), want nil OK", got, rc)
	}

	calls := 0
	if got, rc := uint64IDsFromTwoCall(func(_ []uint64) (uintptr, ErrorCode) {
		calls++
		if calls == 1 {
			return 2, ErrOK
		}
		return 2, ErrBufferTooSmall
	}); rc != ErrBufferTooSmall || got != nil {
		t.Fatalf("second-call failure = (%v, %d), want nil buffer-too-small", got, rc)
	}

	calls = 0
	if got, rc := uint64IDsFromTwoCall(func(dst []uint64) (uintptr, ErrorCode) {
		calls++
		if calls == 1 {
			return 3, ErrOK
		}
		copy(dst, []uint64{7, 8, 9})
		return 3, ErrOK
	}); rc != ErrOK || len(got) != 3 || got[0] != 7 || got[2] != 9 {
		t.Fatalf("filled ids = (%v, %d), want [7 8 9] OK", got, rc)
	}
}

func TestPropertyValueScalarAccessors(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		i := rapid.Int64().Draw(t, "integer")
		iv := ValueInteger(i)
		if ValueGetType(&iv) != ValueTypeInteger || ValueGetInteger(&iv) != i {
			t.Fatalf("integer value/accessor mismatch")
		}

		f := rapid.Float64().Filter(func(v float64) bool { return !math.IsNaN(v) }).Draw(t, "float")
		fv := ValueFloat(f)
		if ValueGetType(&fv) != ValueTypeFloat || ValueGetFloat(&fv) != f {
			t.Fatalf("float value/accessor mismatch")
		}

		s := rapid.String().Filter(func(v string) bool {
			for _, r := range v {
				if r == 0 {
					return false
				}
			}
			return true
		}).Draw(t, "string")
		sv := ValueString(s)
		if ValueGetType(&sv) != ValueTypeString || ValueGetStringPtr(&sv) != s {
			t.Fatalf("string value/accessor mismatch")
		}
		ValueFree(&sv)
	})
}

func TestPropertySerializationWrappersRoundTrip(t *testing.T) {
	lockThread(t)

	rapid.Check(t, func(t *rapid.T) {
		format := rapid.SampledFrom(ffiFormats()).Draw(t, "format")
		h := EngineNewWithSource(`(defrule r => (assert (ok)))`)
		if h == nil {
			t.Fatal("EngineNewWithSource returned nil")
		}
		data, rc := EngineSerializeAs(h, format)
		if rc != ErrOK {
			t.Fatalf("EngineSerializeAs returned %d", rc)
		}
		if rc := EngineFree(h); rc != ErrOK {
			t.Fatalf("EngineFree returned %d", rc)
		}

		restored, rc := EngineDeserializeAs(data, format)
		if rc != ErrOK || restored == nil {
			t.Fatalf("EngineDeserializeAs = (%v, %d), want handle OK", restored, rc)
		}
		if rc := EngineFree(restored); rc != ErrOK {
			t.Fatalf("EngineFree restored returned %d", rc)
		}
	})
}

func TestPropertyCopyAndFreeBytes(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		src := rapid.SliceOfN(rapid.Uint8(), 0, 32).Draw(t, "bytes")
		freed := false
		var ptr unsafe.Pointer
		if len(src) > 0 {
			ptr = unsafe.Pointer(&src[0])
		}
		got := copyAndFreeBytes(ptr, uintptr(len(src)), func() { freed = true })
		if !freed {
			t.Fatal("copyAndFreeBytes must always call free")
		}
		if !bytes.Equal(got, src) {
			t.Fatalf("copied bytes = %v, want %v", got, src)
		}
		if len(src) > 0 {
			src[0] ^= 0xff
			if got[0] == src[0] {
				t.Fatal("copyAndFreeBytes must return Go-owned bytes")
			}
		}
	})
}

func TestPropertyStringFromTwoCall(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		scenario := rapid.SampledFrom([]string{"ok", "empty", "first_error", "second_error", "zero_after_copy"}).Draw(t, "scenario")
		switch scenario {
		case "ok":
			s := rapid.String().Filter(func(v string) bool {
				for _, r := range v {
					if r == 0 {
						return false
					}
				}
				return true
			}).Draw(t, "string")
			calls := 0
			got, rc := stringFromTwoCall(func(buf []byte) (uintptr, ErrorCode) {
				calls++
				if calls == 1 {
					return uintptr(len(s) + 1), ErrOK
				}
				copy(buf, append([]byte(s), 0))
				return uintptr(len(s) + 1), ErrOK
			})
			if rc != ErrOK || got != s {
				t.Fatalf("stringFromTwoCall = (%q, %d), want %q OK", got, rc, s)
			}
		case "empty":
			got, rc := stringFromTwoCall(func([]byte) (uintptr, ErrorCode) { return 0, ErrOK })
			if rc != ErrOK || got != "" {
				t.Fatalf("empty stringFromTwoCall = (%q, %d)", got, rc)
			}
		case "first_error":
			_, rc := stringFromTwoCall(func([]byte) (uintptr, ErrorCode) { return 0, ErrRuntimeError })
			if rc != ErrRuntimeError {
				t.Fatalf("first error rc = %d", rc)
			}
		case "second_error":
			calls := 0
			_, rc := stringFromTwoCall(func([]byte) (uintptr, ErrorCode) {
				calls++
				if calls == 1 {
					return 1, ErrOK
				}
				return 1, ErrBufferTooSmall
			})
			if rc != ErrBufferTooSmall {
				t.Fatalf("second error rc = %d", rc)
			}
		case "zero_after_copy":
			calls := 0
			got, rc := stringFromTwoCall(func(buf []byte) (uintptr, ErrorCode) {
				calls++
				if calls == 1 {
					return 1, ErrOK
				}
				if len(buf) > 0 {
					buf[0] = 0
				}
				return 0, ErrOK
			})
			if rc != ErrOK || got != "" {
				t.Fatalf("zero-after-copy string = (%q, %d)", got, rc)
			}
		}
	})
}

func TestPropertyUint64IDsFromTwoCall(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		scenario := rapid.SampledFrom([]string{"ok", "empty", "first_error", "second_error"}).Draw(t, "scenario")
		switch scenario {
		case "ok":
			want := rapid.SliceOfN(rapid.Uint64(), 1, 16).Draw(t, "ids")
			calls := 0
			got, rc := uint64IDsFromTwoCall(func(dst []uint64) (uintptr, ErrorCode) {
				calls++
				if calls == 1 {
					return uintptr(len(want)), ErrOK
				}
				copy(dst, want)
				return uintptr(len(want)), ErrOK
			})
			if rc != ErrOK || len(got) != len(want) {
				t.Fatalf("ids result = (%v, %d), want %v OK", got, rc, want)
			}
			for i := range want {
				if got[i] != want[i] {
					t.Fatalf("ids[%d] = %d, want %d", i, got[i], want[i])
				}
			}
		case "empty":
			got, rc := uint64IDsFromTwoCall(func([]uint64) (uintptr, ErrorCode) { return 0, ErrOK })
			if rc != ErrOK || got != nil {
				t.Fatalf("empty ids = (%v, %d)", got, rc)
			}
		case "first_error":
			_, rc := uint64IDsFromTwoCall(func([]uint64) (uintptr, ErrorCode) { return 0, ErrRuntimeError })
			if rc != ErrRuntimeError {
				t.Fatalf("first error rc = %d", rc)
			}
		case "second_error":
			calls := 0
			_, rc := uint64IDsFromTwoCall(func([]uint64) (uintptr, ErrorCode) {
				calls++
				if calls == 1 {
					return 1, ErrOK
				}
				return 1, ErrBufferTooSmall
			})
			if rc != ErrBufferTooSmall {
				t.Fatalf("second error rc = %d", rc)
			}
		}
	})
}

func TestPropertyNativeFFISurfaceSweep(t *testing.T) {
	lockThread(t)

	source := `
		(defglobal ?*threshold* = 10)
		(deftemplate sensor (slot id (type INTEGER)) (slot value (type FLOAT)))
		(defrule bootstrap => (assert (booted)))
		(defrule color-seen (color ?c) => (assert (matched ?c)) (printout t ?c crlf))
		(defrule sensor-seen (sensor (id ?id) (value ?v)) => (assert (observed ?id)))
	`

	rapid.Check(t, func(rt *rapid.T) {
		id := rapid.Int64Range(1, 1000).Draw(rt, "id")
		value := rapid.Float64Range(0.1, 1000.0).Draw(rt, "value")
		format := rapid.SampledFrom(ffiFormats()).Draw(rt, "format")

		cfg := MakeConfig(StringEncodingUTF8, ConflictStrategyDepth, 512)
		if cfg == nil {
			rt.Fatal("MakeConfig returned nil")
		}
		if h := EngineNew(); h == nil {
			rt.Fatal("EngineNew returned nil")
		} else if rc := EngineFreeUnchecked(h); rc != ErrOK {
			rt.Fatalf("EngineFreeUnchecked returned %d", rc)
		}
		if h := EngineNewWithConfig(cfg); h == nil {
			rt.Fatal("EngineNewWithConfig returned nil")
		} else if rc := EngineFree(h); rc != ErrOK {
			rt.Fatalf("EngineFree config handle returned %d", rc)
		}
		if h := EngineNewWithConfigHelper(StringEncodingASCII, ConflictStrategyBreadth, 256); h == nil {
			rt.Fatal("EngineNewWithConfigHelper returned nil")
		} else if rc := EngineFree(h); rc != ErrOK {
			rt.Fatalf("EngineFree helper handle returned %d", rc)
		}
		if h := EngineNewWithSourceConfig(source, cfg); h == nil {
			rt.Fatal("EngineNewWithSourceConfig returned nil")
		} else if rc := EngineFree(h); rc != ErrOK {
			rt.Fatalf("EngineFree source-config handle returned %d", rc)
		}

		h := EngineNewWithSource(source)
		if h == nil {
			rt.Fatal("EngineNewWithSource returned nil")
		}
		defer EngineFree(h)

		if rc := EngineLoadString(h, `(defrule loaded => (assert (loaded)))`); rc != ErrOK {
			rt.Fatalf("EngineLoadString returned %d", rc)
		}
		if rc := EngineReset(h); rc != ErrOK {
			rt.Fatalf("EngineReset returned %d", rc)
		}
		if fired, rc := EngineStep(h); rc != ErrOK || fired == 0 {
			rt.Fatalf("EngineStep = (%d, %d), want fired OK", fired, rc)
		}

		if factID, rc := EngineAssertString(h, "(assert (color red))"); rc != ErrOK || factID == 0 {
			rt.Fatalf("EngineAssertString = (%d, %d), want id OK", factID, rc)
		}
		orderedVals := []Value{ValueInteger(id), ValueSymbol("ok")}
		orderedID, rc := EngineAssertOrdered(h, "data", orderedVals)
		for i := range orderedVals {
			ValueFree(&orderedVals[i])
		}
		if rc != ErrOK || orderedID == 0 {
			rt.Fatalf("EngineAssertOrdered = (%d, %d), want id OK", orderedID, rc)
		}
		templateVals := []Value{ValueInteger(id), ValueFloat(value)}
		templateID, rc := EngineAssertTemplate(h, "sensor", []string{"id", "value"}, templateVals)
		for i := range templateVals {
			ValueFree(&templateVals[i])
		}
		if rc != ErrOK || templateID == 0 {
			rt.Fatalf("EngineAssertTemplate = (%d, %d), want id OK", templateID, rc)
		}

		if count, rc := EngineFactCount(h); rc != ErrOK || count == 0 {
			rt.Fatalf("EngineFactCount = (%d, %d), want positive OK", count, rc)
		}
		if ids, rc := EngineFactIDs(h); rc != ErrOK || len(ids) == 0 {
			rt.Fatalf("EngineFactIDs = (%v, %d), want ids OK", ids, rc)
		}
		if ids, rc := EngineFindFactIDs(h, "data"); rc != ErrOK || len(ids) == 0 {
			rt.Fatalf("EngineFindFactIDs = (%v, %d), want ids OK", ids, rc)
		}
		if factType, rc := EngineGetFactType(h, orderedID); rc != ErrOK || factType != FactTypeOrdered {
			rt.Fatalf("EngineGetFactType ordered = (%d, %d)", factType, rc)
		}
		if relation, rc := EngineGetFactRelation(h, orderedID); rc != ErrOK || relation != "data" {
			rt.Fatalf("EngineGetFactRelation = (%q, %d)", relation, rc)
		}
		if count, rc := EngineGetFactFieldCount(h, orderedID); rc != ErrOK || count == 0 {
			rt.Fatalf("EngineGetFactFieldCount = (%d, %d)", count, rc)
		}
		if val, rc := EngineGetFactField(h, orderedID, 0); rc != ErrOK {
			rt.Fatalf("EngineGetFactField returned %d", rc)
		} else {
			ValueFree(&val)
		}
		if factType, rc := EngineGetFactType(h, templateID); rc != ErrOK || factType != FactTypeTemplate {
			rt.Fatalf("EngineGetFactType template = (%d, %d)", factType, rc)
		}
		if name, rc := EngineGetFactTemplateName(h, templateID); rc != ErrOK || name != "sensor" {
			rt.Fatalf("EngineGetFactTemplateName = (%q, %d)", name, rc)
		}
		if val, rc := EngineGetFactSlotByName(h, templateID, "id"); rc != ErrOK {
			rt.Fatalf("EngineGetFactSlotByName returned %d", rc)
		} else {
			ValueFree(&val)
		}

		if count, rc := EngineTemplateCount(h); rc != ErrOK || count == 0 {
			rt.Fatalf("EngineTemplateCount = (%d, %d)", count, rc)
		}
		if name, rc := EngineTemplateName(h, 0); rc != ErrOK || name == "" {
			rt.Fatalf("EngineTemplateName = (%q, %d)", name, rc)
		}
		if count, rc := EngineTemplateSlotCount(h, "sensor"); rc != ErrOK || count == 0 {
			rt.Fatalf("EngineTemplateSlotCount = (%d, %d)", count, rc)
		}
		if name, rc := EngineTemplateSlotName(h, "sensor", 0); rc != ErrOK || name == "" {
			rt.Fatalf("EngineTemplateSlotName = (%q, %d)", name, rc)
		}
		if count, rc := EngineRuleCount(h); rc != ErrOK || count == 0 {
			rt.Fatalf("EngineRuleCount = (%d, %d)", count, rc)
		}
		if name, _, rc := EngineRuleInfo(h, 0); rc != ErrOK || name == "" {
			rt.Fatalf("EngineRuleInfo = (%q, %d)", name, rc)
		}
		if count, rc := EngineModuleCount(h); rc != ErrOK || count == 0 {
			rt.Fatalf("EngineModuleCount = (%d, %d)", count, rc)
		}
		if name, rc := EngineModuleName(h, 0); rc != ErrOK || name == "" {
			rt.Fatalf("EngineModuleName = (%q, %d)", name, rc)
		}
		if name, rc := EngineCurrentModule(h); rc != ErrOK || name == "" {
			rt.Fatalf("EngineCurrentModule = (%q, %d)", name, rc)
		}
		if depth, rc := EngineFocusStackDepth(h); rc != ErrOK {
			rt.Fatalf("EngineFocusStackDepth returned %d", rc)
		} else if depth > 0 {
			if _, rc := EngineFocusStackEntry(h, 0); rc != ErrOK {
				rt.Fatalf("EngineFocusStackEntry returned %d", rc)
			}
		}
		if _, rc := EngineGetFocus(h); rc != ErrOK && rc != ErrNotFound {
			rt.Fatalf("EngineGetFocus returned %d", rc)
		}
		if _, rc := EngineAgendaCount(h); rc != ErrOK {
			rt.Fatalf("EngineAgendaCount returned %d", rc)
		}

		if fired, rc := EngineRun(h, 1); rc != ErrOK {
			rt.Fatalf("EngineRun = (%d, %d)", fired, rc)
		}
		if fired, reason, rc := EngineRunEx(h, -1); rc != ErrOK {
			rt.Fatalf("EngineRunEx = (%d, %d, %d)", fired, reason, rc)
		}
		if _, ok := EngineGetOutput(h, "t"); !ok {
			rt.Fatal("EngineGetOutput missing stdout")
		}
		if rc := EngineClearOutput(h, "t"); rc != ErrOK {
			rt.Fatalf("EngineClearOutput returned %d", rc)
		}
		if got, ok := EngineGetOutput(h, "missing"); ok || got != "" {
			rt.Fatalf("EngineGetOutput missing = (%q, %v), want empty false", got, ok)
		}
		if rc := EnginePushInput(h, "unused"); rc != ErrOK {
			rt.Fatalf("EnginePushInput returned %d", rc)
		}
		if val, rc := EngineGetGlobal(h, "threshold"); rc != ErrOK {
			rt.Fatalf("EngineGetGlobal returned %d", rc)
		} else {
			ValueFree(&val)
		}
		if count, rc := EngineActionDiagnosticCount(h); rc != ErrOK {
			rt.Fatalf("EngineActionDiagnosticCount returned %d", rc)
		} else if count > 0 {
			if _, rc := EngineActionDiagnosticCopy(h, 0); rc != ErrOK {
				rt.Fatalf("EngineActionDiagnosticCopy returned %d", rc)
			}
		} else {
			_, _ = EngineActionDiagnosticCopy(h, 0)
		}
		if rc := EngineClearActionDiagnostics(h); rc != ErrOK {
			rt.Fatalf("EngineClearActionDiagnostics returned %d", rc)
		}

		data, rc := EngineSerializeAs(h, format)
		if rc != ErrOK || len(data) == 0 {
			rt.Fatalf("EngineSerializeAs = (%d bytes, %d), want data OK", len(data), rc)
		}
		if badData, rc := EngineSerializeAs(nil, format); rc == ErrOK || badData != nil {
			rt.Fatalf("EngineSerializeAs nil = (%v, %d), want error", badData, rc)
		}
		if restored, rc := EngineDeserializeAs(data, format); rc != ErrOK || restored == nil {
			rt.Fatalf("EngineDeserializeAs = (%v, %d), want handle OK", restored, rc)
		} else if rc := EngineFree(restored); rc != ErrOK {
			rt.Fatalf("EngineFree restored returned %d", rc)
		}
		if restored, rc := EngineDeserializeAs(nil, format); rc != ErrInvalidArgument || restored != nil {
			rt.Fatalf("EngineDeserializeAs nil = (%v, %d), want invalid argument", restored, rc)
		}
		if restored, rc := EngineDeserializeAs([]byte("bad snapshot"), format); rc == ErrOK || restored != nil {
			rt.Fatalf("EngineDeserializeAs invalid = (%v, %d), want error", restored, rc)
		}

		EngineHalt(h)
		if halted, rc := EngineIsHalted(h); rc != ErrOK || !halted {
			rt.Fatalf("EngineIsHalted = (%v, %d), want true OK", halted, rc)
		}
		_ = EngineLoadString(h, "(this is not valid CLIPS")
		_ = EngineLastError(h)
		if _, rc := EngineLastErrorCopy(h); rc != ErrOK && rc != ErrNotFound {
			rt.Fatalf("EngineLastErrorCopy returned %d", rc)
		}
		if rc := EngineClearError(h); rc != ErrOK {
			rt.Fatalf("EngineClearError returned %d", rc)
		}
		_ = LastErrorGlobal()
		_ = EngineNewWithSource("(defrule bad")
		_ = LastErrorGlobal()
		if _, rc := LastErrorGlobalCopy(); rc != ErrOK && rc != ErrNotFound {
			rt.Fatalf("LastErrorGlobalCopy returned %d", rc)
		}
		ClearErrorGlobal()

		if rc := EngineRetract(h, orderedID); rc != ErrOK {
			rt.Fatalf("EngineRetract ordered returned %d", rc)
		}
		if rc := EngineClear(h); rc != ErrOK {
			rt.Fatalf("EngineClear returned %d", rc)
		}

		void := ValueVoid()
		if got := ValueGetStringPtr(&void); got != "" {
			rt.Fatalf("void string ptr = %q, want empty", got)
		}
		_ = ValueGetExternalPointer(&void)
		emptyMulti := ValueMultifield(nil)
		if ValueGetMultifieldLen(&emptyMulti) != 0 {
			rt.Fatal("empty multifield length mismatch")
		}
		symbol := ValueSymbol("sym")
		if ValueGetType(&symbol) != ValueTypeSymbol || ValueGetStringPtr(&symbol) != "sym" {
			rt.Fatal("symbol accessor mismatch")
		}
		ValueFree(&symbol)
		multi := ValueMultifield([]Value{ValueString("nested")})
		if ValueGetMultifieldLen(&multi) != 1 {
			rt.Fatal("multifield length mismatch")
		}
		elem := ValueGetMultifieldElement(&multi, 0)
		if ValueGetStringPtr(&elem) != "nested" {
			rt.Fatal("multifield element mismatch")
		}
		ValueFree(&multi)
		StringFree(nil)
		ValueArrayFree(nil, 0)
	})
}

package ffi

import (
	"errors"
	"go/ast"
	"go/parser"
	"go/token"
	"path/filepath"
	"strconv"
	"testing"
)

const embeddedNULFixture = `
	(deftemplate item (slot payload))
	(defglobal ?*answer* = 42)
	(defrule emit => (printout t "kept"))
`

func TestValidateCStringRejectsEmbeddedNUL(t *testing.T) {
	tests := []struct {
		name     string
		value    string
		wantByte int
	}{
		{name: "first", value: "\x00suffix", wantByte: 0},
		{name: "middle", value: "prefix\x00suffix", wantByte: 6},
		{name: "unicode byte offset", value: "pré\x00suffix", wantByte: 4},
		{name: "last", value: "prefix\x00", wantByte: 6},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := ValidateCString("argument", tc.value)
			var nulErr *CStringError
			if !errors.As(err, &nulErr) {
				t.Fatalf("ValidateCString error = %T %v, want *CStringError", err, err)
			}
			if nulErr.Argument != "argument" || nulErr.Byte != tc.wantByte {
				t.Fatalf("CStringError = %+v, want argument at byte %d", nulErr, tc.wantByte)
			}
			want := "argument contains embedded NUL at byte " + strconv.Itoa(tc.wantByte)
			if err.Error() != want {
				t.Fatalf("error = %q, want %q", err, want)
			}
		})
	}

	for _, value := range []string{"", "plain ASCII", "héllo"} {
		if err := ValidateCString("argument", value); err != nil {
			t.Fatalf("ValidateCString(%q) = %v, want nil", value, err)
		}
	}
}

func TestEngineConstructorsRejectEmbeddedNULSource(t *testing.T) {
	lockThread(t)
	source := `(deftemplate prefix (slot value))` + "\x00" + `(defrule truncated`

	if handle := EngineNewWithSource(source); handle != nil {
		_ = EngineFree(handle)
		t.Fatal("EngineNewWithSource accepted an embedded-NUL source")
	}

	config := MakeConfig(StringEncodingUTF8, ConflictStrategyBreadth, 64)
	if handle := EngineNewWithSourceConfig(source, config); handle != nil {
		_ = EngineFree(handle)
		t.Fatal("EngineNewWithSourceConfig accepted an embedded-NUL source")
	}
}

//nolint:funlen // The table intentionally locks every C.CString-bearing engine wrapper in one audit.
func TestEngineCStringWrappersRejectEmbeddedNUL(t *testing.T) {
	tests := []struct {
		name string
		run  func(*testing.T, EngineHandle)
	}{
		{
			name: "load source",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				before, rc := EngineTemplateCount(handle)
				requireFFIOK(t, rc)
				rc = EngineLoadString(handle, `(deftemplate alias (slot value))`+"\x00"+`(defrule truncated`)
				requireFFIInvalid(t, rc)
				after, rc := EngineTemplateCount(handle)
				requireFFIOK(t, rc)
				if after != before {
					t.Fatalf("template count changed from %d to %d", before, after)
				}
			},
		},
		{
			name: "assert source",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				before, rc := EngineFactCount(handle)
				requireFFIOK(t, rc)
				id, rc := EngineAssertString(handle, `(assert (alias kept))`+"\x00"+`(assert (truncated))`)
				requireFFIInvalid(t, rc)
				if id != 0 {
					t.Fatalf("fact ID = %d, want zero", id)
				}
				requireFactCount(t, handle, before)
			},
		},
		{
			name: "ordered relation",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				before, rc := EngineFactCount(handle)
				requireFFIOK(t, rc)
				id, rc := EngineAssertOrdered(handle, "alias\x00suffix", []Value{ValueInteger(7)})
				requireFFIInvalid(t, rc)
				if id != 0 {
					t.Fatalf("fact ID = %d, want zero", id)
				}
				requireFactCount(t, handle, before)
			},
		},
		{
			name: "template name",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				before, rc := EngineFactCount(handle)
				requireFFIOK(t, rc)
				id, rc := EngineAssertTemplate(handle, "item\x00suffix", []string{"payload"}, []Value{ValueInteger(7)})
				requireFFIInvalid(t, rc)
				if id != 0 {
					t.Fatalf("fact ID = %d, want zero", id)
				}
				requireFactCount(t, handle, before)
			},
		},
		{
			name: "slot name",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				before, rc := EngineFactCount(handle)
				requireFFIOK(t, rc)
				id, rc := EngineAssertTemplate(handle, "item", []string{"payload\x00suffix"}, []Value{ValueInteger(7)})
				requireFFIInvalid(t, rc)
				if id != 0 {
					t.Fatalf("fact ID = %d, want zero", id)
				}
				requireFactCount(t, handle, before)
			},
		},
		{
			name: "find relation",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				if _, rc := EngineAssertOrdered(handle, "alias", nil); rc != ErrOK {
					t.Fatalf("seed fact: %d", rc)
				}
				ids, rc := EngineFindFactIDs(handle, "alias\x00suffix")
				requireFFIInvalid(t, rc)
				if ids != nil {
					t.Fatalf("IDs = %v, want nil", ids)
				}
			},
		},
		{
			name: "named fact slot",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				id, rc := EngineAssertTemplate(handle, "item", []string{"payload"}, []Value{ValueInteger(7)})
				requireFFIOK(t, rc)
				value, rc := EngineGetFactSlotByName(handle, id, "payload\x00suffix")
				requireFFIInvalid(t, rc)
				if got := ValueGetType(&value); got != ValueTypeVoid {
					t.Fatalf("value type = %d, want Void", got)
				}
			},
		},
		{
			name: "global name",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				value, rc := EngineGetGlobal(handle, "answer\x00suffix")
				requireFFIInvalid(t, rc)
				if got := ValueGetType(&value); got != ValueTypeVoid {
					t.Fatalf("value type = %d, want Void", got)
				}
			},
		},
		{
			name: "get output channel",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				runFixtureToOutput(t, handle)
				output, ok := EngineGetOutput(handle, "t\x00suffix")
				if output != "" || ok {
					t.Fatalf("invalid-channel output = (%q, %v), want empty false", output, ok)
				}
				if output, ok = EngineGetOutput(handle, "t"); !ok || output != "kept" {
					t.Fatalf("valid output = (%q, %v), want kept true", output, ok)
				}
			},
		},
		{
			name: "copy output channel",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				output, ok, rc := EngineGetOutputCopy(handle, "t\x00suffix")
				requireFFIInvalid(t, rc)
				if output != "" || ok {
					t.Fatalf("invalid-channel output copy = (%q, %v), want empty false", output, ok)
				}
				output, ok, rc = EngineGetOutputCopy(handle, "missing")
				requireFFIOK(t, rc)
				if output != "" || ok {
					t.Fatalf("missing output copy = (%q, %v), want empty false", output, ok)
				}
			},
		},
		{
			name: "clear output channel",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				runFixtureToOutput(t, handle)
				requireFFIInvalid(t, EngineClearOutput(handle, "t\x00suffix"))
				if output, ok := EngineGetOutput(handle, "t"); !ok || output != "kept" {
					t.Fatalf("valid output = (%q, %v), want kept true", output, ok)
				}
			},
		},
		{
			name: "input line",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				requireFFIInvalid(t, EnginePushInput(handle, "prefix\x00suffix"))
				requireFFIOK(t, EnginePushInput(handle, "valid"))
			},
		},
		{
			name: "template slot count",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				count, rc := EngineTemplateSlotCount(handle, "item\x00suffix")
				requireFFIInvalid(t, rc)
				if count != 0 {
					t.Fatalf("slot count = %d, want zero", count)
				}
			},
		},
		{
			name: "template slot name",
			run: func(t *testing.T, handle EngineHandle) {
				t.Helper()
				name, rc := EngineTemplateSlotName(handle, "item\x00suffix", 0)
				requireFFIInvalid(t, rc)
				if name != "" {
					t.Fatalf("slot name = %q, want empty", name)
				}
			},
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			lockThread(t)
			handle := EngineNewWithSource(embeddedNULFixture)
			if handle == nil {
				t.Fatal("EngineNewWithSource returned nil")
			}
			t.Cleanup(func() {
				if rc := EngineFree(handle); rc != ErrOK {
					t.Errorf("EngineFree returned %d", rc)
				}
			})
			tc.run(t, handle)
		})
	}
}

func TestValueCStringWrappersRejectEmbeddedNUL(t *testing.T) {
	for _, tc := range []struct {
		name   string
		legacy func(string) Value
		bytes  func(string) (Value, ErrorCode)
	}{
		{name: "symbol", legacy: ValueSymbol, bytes: ValueSymbolBytes},
		{name: "string", legacy: ValueString, bytes: ValueStringBytes},
	} {
		t.Run(tc.name, func(t *testing.T) {
			legacy := tc.legacy("a\x00b")
			if got := ValueGetType(&legacy); got != ValueTypeVoid {
				ValueFree(&legacy)
				t.Fatalf("legacy value type = %d, want Void", got)
			}

			value, rc := tc.bytes("a\x00b")
			requireFFIInvalid(t, rc)
			if got := ValueGetType(&value); got != ValueTypeVoid {
				ValueFree(&value)
				t.Fatalf("checked value type = %d, want Void", got)
			}

			valid, rc := tc.bytes("héllo")
			requireFFIOK(t, rc)
			defer ValueFree(&valid)
			if got := ValueGetStringPtr(&valid); got != "héllo" {
				t.Fatalf("valid value = %q, want héllo", got)
			}
		})
	}
}

func TestCStringConversionIsCentralized(t *testing.T) {
	paths, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	fset := token.NewFileSet()
	var calls []string
	for _, path := range paths {
		if filepath.Ext(path) != ".go" || filepath.Base(path) == "embedded_nul_test.go" {
			continue
		}
		file, parseErr := parser.ParseFile(fset, path, nil, 0)
		if parseErr != nil {
			t.Fatal(parseErr)
		}
		for _, declaration := range file.Decls {
			function, ok := declaration.(*ast.FuncDecl)
			if !ok || function.Body == nil {
				continue
			}
			ast.Inspect(function.Body, func(node ast.Node) bool {
				call, ok := node.(*ast.CallExpr)
				if !ok || !isCCStringCall(call) {
					return true
				}
				position := fset.Position(call.Pos())
				calls = append(calls, position.String()+" in "+function.Name.Name)
				if function.Name.Name != "checkedCString" {
					t.Errorf("C.CString outside checkedCString: %s", calls[len(calls)-1])
				}
				return true
			})
		}
	}
	if len(calls) != 1 {
		t.Fatalf("C.CString calls = %v, want exactly one checked conversion", calls)
	}
}

func isCCStringCall(call *ast.CallExpr) bool {
	selector, ok := call.Fun.(*ast.SelectorExpr)
	if !ok || selector.Sel.Name != "CString" {
		return false
	}
	identifier, ok := selector.X.(*ast.Ident)
	return ok && identifier.Name == "C"
}

func requireFFIInvalid(t *testing.T, code ErrorCode) {
	t.Helper()
	if code != ErrInvalidArgument {
		t.Fatalf("error code = %d, want ErrInvalidArgument (%d)", code, ErrInvalidArgument)
	}
}

func requireFFIOK(t *testing.T, code ErrorCode) {
	t.Helper()
	if code != ErrOK {
		t.Fatalf("error code = %d, want ErrOK", code)
	}
}

func requireFactCount(t *testing.T, handle EngineHandle, want uintptr) {
	t.Helper()
	got, rc := EngineFactCount(handle)
	requireFFIOK(t, rc)
	if got != want {
		t.Fatalf("fact count = %d, want %d", got, want)
	}
}

func runFixtureToOutput(t *testing.T, handle EngineHandle) {
	t.Helper()
	fired, _, rc := EngineRunEx(handle, -1)
	requireFFIOK(t, rc)
	if fired != 1 {
		t.Fatalf("rules fired = %d, want 1", fired)
	}
}

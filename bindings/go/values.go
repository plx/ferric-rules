package ferric

import (
	"errors"
	"fmt"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

var (
	errUnsupportedGoTypeForFFI = errors.New("ferric: unsupported Go type for FFI conversion")
)

// Symbol is a distinct type representing a CLIPS symbol value.
// Symbols are unquoted identifiers (e.g. TRUE, FALSE, foo) as
// opposed to quoted string literals.
type Symbol string

// goToFFIValue converts a Go value to a C FerricValue for passing to the FFI layer.
func goToFFIValue(v any) (ffi.Value, error) {
	switch val := v.(type) {
	case int:
		return ffi.ValueInteger(int64(val)), nil
	case int64:
		return ffi.ValueInteger(val), nil
	case int32:
		return ffi.ValueInteger(int64(val)), nil
	case float64:
		return ffi.ValueFloat(val), nil
	case float32:
		return ffi.ValueFloat(float64(val)), nil
	case Symbol:
		return ffi.ValueSymbol(string(val)), nil
	case string:
		return ffi.ValueString(val), nil
	case bool:
		if val {
			return ffi.ValueSymbol("TRUE"), nil
		}
		return ffi.ValueSymbol("FALSE"), nil
	case nil:
		return ffi.ValueVoid(), nil
	case []any:
		elements := make([]ffi.Value, len(val))
		for i, elem := range val {
			ev, err := goToFFIValue(elem)
			if err != nil {
				for j := range i {
					ffi.ValueFree(&elements[j])
				}
				return ffi.Value{}, err
			}
			elements[i] = ev
		}
		return ffi.ValueMultifield(elements), nil
	default:
		return ffi.Value{}, fmt.Errorf("%w: %T", errUnsupportedGoTypeForFFI, v)
	}
}

// ffiValueToGo converts a C FerricValue to a native Go value.
// The caller retains ownership of v; it is not freed by this function.
func ffiValueToGo(v *ffi.Value) any {
	switch ffi.ValueGetType(v) {
	case ffi.ValueTypeVoid:
		return nil
	case ffi.ValueTypeInteger:
		return ffi.ValueGetInteger(v)
	case ffi.ValueTypeFloat:
		return ffi.ValueGetFloat(v)
	case ffi.ValueTypeSymbol:
		return Symbol(ffi.ValueGetStringPtr(v))
	case ffi.ValueTypeString:
		return ffi.ValueGetStringPtr(v)
	case ffi.ValueTypeMultifield:
		n := ffi.ValueGetMultifieldLen(v)
		result := make([]any, n)
		for i := range n {
			elem := ffi.ValueGetMultifieldElement(v, i)
			result[i] = ffiValueToGo(&elem)
			// Do NOT free elem here — it is a shallow copy of the parent's
			// array element and shares owned resources (string_ptr, etc.).
			// The parent's ValueFree handles recursive cleanup.
		}
		return result
	case ffi.ValueTypeExternalAddress:
		return ffi.ValueGetExternalPointer(v)
	default:
		return nil
	}
}

// ffiValueToGoAndFree converts a C FerricValue to a native Go value,
// then frees the FerricValue and any resources it owns.
func ffiValueToGoAndFree(v *ffi.Value) any {
	result := ffiValueToGo(v)
	ffi.ValueFree(v)
	return result
}

package ferric

import (
	"errors"
	"fmt"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

var (
	errUnsupportedGoTypeForFFI = errors.New("ferric: unsupported Go type for FFI conversion")
)

const maxMultifieldNestingDepth = 128

// Symbol is a distinct type representing a CLIPS symbol value.
// Symbols are unquoted identifiers (e.g. TRUE, FALSE, foo) as
// opposed to quoted string literals.
type Symbol string

// goToFFIValue converts a Go value to a Ferric-owned C FerricValue for passing
// to the FFI layer. Recursive multifields are constructed through Ferric's
// copy API; no Go- or C-allocated value array is transferred to Rust cleanup.
func goToFFIValue(v any) (ffi.Value, error) {
	return goToFFIValueAtPath(v, "value", 0)
}

func goToFFIValueAtPath(v any, path string, depth int) (ffi.Value, error) {
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
		return goStringToFFIValue(path, string(val), ffiValueSymbolBytes)
	case string:
		return goStringToFFIValue(path, val, ffiValueStringBytes)
	case bool:
		if val {
			return goStringToFFIValue(path, "TRUE", ffiValueSymbolBytes)
		}
		return goStringToFFIValue(path, "FALSE", ffiValueSymbolBytes)
	case nil:
		return ffi.ValueVoid(), nil
	case []any:
		if depth >= maxMultifieldNestingDepth {
			return ffi.Value{}, fmt.Errorf(
				"%w: %s multifield nesting exceeds %d levels",
				ErrInvalidArgument,
				path,
				maxMultifieldNestingDepth,
			)
		}
		elements := make([]ffi.Value, len(val))
		for i, elem := range val {
			ev, err := goToFFIValueAtPath(elem, fmt.Sprintf("%s[%d]", path, i), depth+1)
			if err != nil {
				// Free the elements converted before this failure. Goes through
				// the ffiValueFree seam (as AssertFact/AssertTemplate do) so the
				// cleanup is observable in tests.
				for j := range i {
					ffiValueFree(&elements[j])
				}
				return ffi.Value{}, err
			}
			elements[i] = ev
		}
		result, rc := ffiValueMultifieldCopy(elements)
		for i := range elements {
			ffiValueFree(&elements[i])
		}
		if rc != ffi.ErrOK {
			return ffi.Value{}, errorFromFFI(rc, nil)
		}
		return result, nil
	default:
		return ffi.Value{}, fmt.Errorf("%w at %s: %T", errUnsupportedGoTypeForFFI, path, v)
	}
}

func goStringToFFIValue(
	path string,
	value string,
	constructor func(string) (ffi.Value, ffi.ErrorCode),
) (ffi.Value, error) {
	if err := validateCStringArgument(path, value); err != nil {
		return ffi.Value{}, err
	}
	result, rc := constructor(value)
	if rc != ffi.ErrOK {
		return ffi.Value{}, fmt.Errorf("%s: %w", path, errorFromFFI(rc, nil))
	}
	return result, nil
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

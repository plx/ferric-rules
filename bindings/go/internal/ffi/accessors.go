package ffi

// #include "lib/ferric.h"
import "C"
import "unsafe"

// Value field accessors — needed because CGo types can't be accessed
// from outside the defining package.

// ValueGetType returns the value's discriminant tag.
func ValueGetType(v *Value) ValueType { return ValueType(v.value_type) }

// ValueGetInteger returns the integer payload.
func ValueGetInteger(v *Value) int64 { return int64(v.integer) }

// ValueGetFloat returns the float payload.
func ValueGetFloat(v *Value) float64 { return float64(v.float_) }

// ValueGetStringPtr returns the symbol/string payload as a Go string.
func ValueGetStringPtr(v *Value) string {
	if v.string_ptr == nil {
		return ""
	}
	return C.GoString(v.string_ptr)
}

// ValueGetMultifieldLen returns the number of multifield elements.
func ValueGetMultifieldLen(v *Value) int { return int(v.multifield_len) }

// ValueGetMultifieldElement returns the i-th multifield element.
func ValueGetMultifieldElement(v *Value, i int) Value {
	arr := unsafe.Slice(v.multifield_ptr, v.multifield_len)
	return Value(arr[i])
}

// ValueGetExternalPointer returns the external address payload.
func ValueGetExternalPointer(v *Value) unsafe.Pointer {
	return v.external_pointer
}

// MakeConfig creates a FerricConfig from Go-typed values.
func MakeConfig(encoding StringEncoding, strategy ConflictStrategy, maxCallDepth uintptr) *Config {
	c := C.struct_FerricConfig{
		string_encoding: C.uint32_t(encoding),
		strategy:        C.uint32_t(strategy),
		max_call_depth:  C.uintptr_t(maxCallDepth),
	}
	return &c
}

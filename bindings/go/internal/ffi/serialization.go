package ffi

/*
#define FERRIC_SERDE
#include "lib/ferric.h"
#include <stdlib.h>
*/
import "C"
import "unsafe"

// EngineSerializeAs serializes engine state to a byte slice in the given format.
// Uses the Rust-allocated path (null alloc_fn), copies to Go memory,
// then frees the Rust buffer.
func EngineSerializeAs(h EngineHandle, format SerializationFormat) ([]byte, ErrorCode) {
	var data *C.uint8_t
	var length C.uintptr_t

	rc := ErrorCode(C.ferric_engine_serialize_as(h, C.uint32_t(format), nil, nil, &data, &length)) //nolint:gocritic // dupSubExpr false positive in cgo-generated code
	if rc != ErrOK {
		return nil, rc
	}

	if length == 0 {
		C.ferric_bytes_free(data, length)
		return nil, ErrOK
	}

	// Copy the Rust-owned bytes into Go-managed memory.
	goBytes := C.GoBytes(unsafe.Pointer(data), C.int(length))

	// Free the Rust-allocated buffer.
	C.ferric_bytes_free(data, length)

	return goBytes, ErrOK
}

// EngineDeserializeAs creates an engine from previously serialized bytes
// in the given format.
// The returned handle is ready for use; its thread affinity is set to the calling thread.
func EngineDeserializeAs(data []byte, format SerializationFormat) (EngineHandle, ErrorCode) {
	if len(data) == 0 {
		return nil, ErrInvalidArgument
	}

	var engine EngineHandle
	rc := ErrorCode(C.ferric_engine_deserialize_as(
		(*C.uint8_t)(unsafe.Pointer(&data[0])),
		C.uintptr_t(len(data)),
		C.uint32_t(format),
		&engine, //nolint:gocritic // dupSubExpr false positive in cgo-generated code
	))
	return engine, rc
}

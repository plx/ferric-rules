package ffi

/*
#define FERRIC_SERDE
#include "lib/ferric.h"
#include <stdlib.h>
*/
import "C"
import "unsafe"

// EngineSerialize serializes engine state to a byte slice.
// Uses the Rust-allocated path (null alloc_fn), copies to Go memory,
// then frees the Rust buffer.
func EngineSerialize(h EngineHandle) ([]byte, ErrorCode) {
	var data *C.uint8_t
	var length C.uintptr_t

	rc := ErrorCode(C.ferric_engine_serialize(h, nil, nil, &data, &length)) //nolint:gocritic // dupSubExpr false positive in cgo-generated code
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

// EngineDeserialize creates an engine from previously serialized bytes.
// The returned handle is ready for use; its thread affinity is set to the calling thread.
func EngineDeserialize(data []byte) (EngineHandle, ErrorCode) {
	if len(data) == 0 {
		return nil, ErrInvalidArgument
	}

	var engine EngineHandle
	rc := ErrorCode(C.ferric_engine_deserialize(
		(*C.uint8_t)(unsafe.Pointer(&data[0])),
		C.uintptr_t(len(data)),
		&engine, //nolint:gocritic // dupSubExpr false positive in cgo-generated code
	))
	return engine, rc
}

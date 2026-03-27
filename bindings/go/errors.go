package ferric

import (
	"errors"
	"fmt"

	"github.com/prb/ferric-rules/bindings/go/internal/ffi"
)

// ---------------------------------------------------------------------------
// Base error type
// ---------------------------------------------------------------------------

// FerricError is the base error type for all errors returned by the ferric
// engine. It carries a numeric error code and a human-readable message.
//
//nolint:revive // Public API name retained for clarity and backward compatibility.
type FerricError struct {
	Code    int
	Message string
}

func (e *FerricError) Error() string {
	return fmt.Sprintf("ferric: %s", e.Message)
}

// ---------------------------------------------------------------------------
// Sentinel errors (for errors.Is)
// ---------------------------------------------------------------------------

// Sentinel errors for stable errors.Is matching across ferric APIs.
var (
	ErrParse           = errors.New("ferric: parse error")
	ErrCompile         = errors.New("ferric: compile error")
	ErrRuntime         = errors.New("ferric: runtime error")
	ErrNotFound        = errors.New("ferric: not found")
	ErrEncoding        = errors.New("ferric: encoding error")
	ErrIO              = errors.New("ferric: I/O error")
	ErrThreadViolation = errors.New("ferric: thread violation")
	ErrInvalidArgument = errors.New("ferric: invalid argument")
	ErrSerialization   = errors.New("ferric: serialization error")
)

// ---------------------------------------------------------------------------
// Concrete error types (for errors.As)
// ---------------------------------------------------------------------------

// ParseError is returned when the engine cannot parse CLIPS source.
type ParseError struct {
	FerricError
}

// Is reports whether target matches ErrParse.
func (e *ParseError) Is(target error) bool {
	return target == ErrParse
}

// CompileError is returned when rule compilation fails.
type CompileError struct {
	FerricError
}

// Is reports whether target matches ErrCompile.
func (e *CompileError) Is(target error) bool {
	return target == ErrCompile
}

// RuntimeError is returned when rule execution encounters an error.
type RuntimeError struct {
	FerricError
}

// Is reports whether target matches ErrRuntime.
func (e *RuntimeError) Is(target error) bool {
	return target == ErrRuntime
}

// NotFoundError is returned when a requested entity does not exist.
type NotFoundError struct {
	FerricError
}

// Is reports whether target matches ErrNotFound.
func (e *NotFoundError) Is(target error) bool {
	return target == ErrNotFound
}

// IOError is returned when an I/O operation fails.
type IOError struct {
	FerricError
}

// Is reports whether target matches ErrIO.
func (e *IOError) Is(target error) bool {
	return target == ErrIO
}

// SerializationError is returned when engine serialization or deserialization fails.
type SerializationError struct {
	FerricError
}

// Is reports whether target matches ErrSerialization.
func (e *SerializationError) Is(target error) bool {
	return target == ErrSerialization
}

// ThreadViolationError is returned when an engine is accessed from a thread
// other than the one that created it.
type ThreadViolationError struct {
	FerricError
}

// Is reports whether target matches ErrThreadViolation.
func (e *ThreadViolationError) Is(target error) bool {
	return target == ErrThreadViolation
}

// InvalidArgumentError is returned when an argument to an API call is invalid.
type InvalidArgumentError struct {
	FerricError
}

// Is reports whether target matches ErrInvalidArgument.
func (e *InvalidArgumentError) Is(target error) bool {
	return target == ErrInvalidArgument
}

// ---------------------------------------------------------------------------
// FFI error translation
// ---------------------------------------------------------------------------

// ffiMsg returns the FFI error message, falling back to fallback if empty.
func ffiMsg(h ffi.EngineHandle, fallback string) string {
	var msg string
	if h != nil {
		msg = ffi.EngineLastError(h)
	} else {
		msg = ffi.LastErrorGlobal()
	}
	if msg == "" {
		return fallback
	}
	return msg
}

// errorFromFFI translates an FFI error code into the appropriate Go error.
// If h is non-nil the per-engine error message is used; otherwise the global
// error channel is consulted.
func errorFromFFI(code ffi.ErrorCode, h ffi.EngineHandle) error {
	if code == ffi.ErrOK {
		return nil
	}
	c := int(code)
	switch code {
	case ffi.ErrParseError:
		return &ParseError{FerricError{Code: c, Message: ffiMsg(h, "parse error")}}
	case ffi.ErrCompileError:
		return &CompileError{FerricError{Code: c, Message: ffiMsg(h, "compile error")}}
	case ffi.ErrRuntimeError:
		return &RuntimeError{FerricError{Code: c, Message: ffiMsg(h, "runtime error")}}
	case ffi.ErrNotFound:
		return &NotFoundError{FerricError{Code: c, Message: ffiMsg(h, "not found")}}
	case ffi.ErrIOError:
		return &IOError{FerricError{Code: c, Message: ffiMsg(h, "I/O error")}}
	case ffi.ErrSerializationError:
		return &SerializationError{FerricError{Code: c, Message: ffiMsg(h, "serialization error")}}
	case ffi.ErrThreadViolation:
		return &ThreadViolationError{FerricError{Code: c, Message: ffiMsg(h, "thread violation")}}
	case ffi.ErrInvalidArgument:
		return &InvalidArgumentError{FerricError{Code: c, Message: ffiMsg(h, "invalid argument")}}
	default:
		return &FerricError{Code: c, Message: ffiMsg(h, fmt.Sprintf("error code %d", c))}
	}
}

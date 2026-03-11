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

var (
	ErrParse           = errors.New("ferric: parse error")
	ErrCompile         = errors.New("ferric: compile error")
	ErrRuntime         = errors.New("ferric: runtime error")
	ErrNotFound        = errors.New("ferric: not found")
	ErrEncoding        = errors.New("ferric: encoding error")
	ErrIO              = errors.New("ferric: I/O error")
	ErrThreadViolation = errors.New("ferric: thread violation")
	ErrInvalidArgument = errors.New("ferric: invalid argument")
)

// ---------------------------------------------------------------------------
// Concrete error types (for errors.As)
// ---------------------------------------------------------------------------

// ParseError is returned when the engine cannot parse CLIPS source.
type ParseError struct {
	FerricError
}

func (e *ParseError) Is(target error) bool {
	return target == ErrParse
}

// CompileError is returned when rule compilation fails.
type CompileError struct {
	FerricError
}

func (e *CompileError) Is(target error) bool {
	return target == ErrCompile
}

// RuntimeError is returned when rule execution encounters an error.
type RuntimeError struct {
	FerricError
}

func (e *RuntimeError) Is(target error) bool {
	return target == ErrRuntime
}

// NotFoundError is returned when a requested entity does not exist.
type NotFoundError struct {
	FerricError
}

func (e *NotFoundError) Is(target error) bool {
	return target == ErrNotFound
}

// IOError is returned when an I/O operation fails.
type IOError struct {
	FerricError
}

func (e *IOError) Is(target error) bool {
	return target == ErrIO
}

// ---------------------------------------------------------------------------
// FFI error translation
// ---------------------------------------------------------------------------

// errorFromFFI translates an FFI error code into the appropriate Go error.
// If h is non-nil the per-engine error message is used; otherwise the global
// error channel is consulted.
func errorFromFFI(code ffi.ErrorCode, h ffi.EngineHandle) error {
	if code == ffi.ErrOK {
		return nil
	}

	var msg string
	if h != nil {
		msg = ffi.EngineLastError(h)
	} else {
		msg = ffi.LastErrorGlobal()
	}

	c := int(code)

	switch code {
	case ffi.ErrParseError:
		if msg == "" {
			msg = "parse error"
		}
		return &ParseError{FerricError{Code: c, Message: msg}}

	case ffi.ErrCompileError:
		if msg == "" {
			msg = "compile error"
		}
		return &CompileError{FerricError{Code: c, Message: msg}}

	case ffi.ErrRuntimeError:
		if msg == "" {
			msg = "runtime error"
		}
		return &RuntimeError{FerricError{Code: c, Message: msg}}

	case ffi.ErrNotFound:
		if msg == "" {
			msg = "not found"
		}
		return &NotFoundError{FerricError{Code: c, Message: msg}}

	case ffi.ErrIOError:
		if msg == "" {
			msg = "I/O error"
		}
		return &IOError{FerricError{Code: c, Message: msg}}

	default:
		if msg == "" {
			msg = fmt.Sprintf("error code %d", c)
		}
		return &FerricError{Code: c, Message: msg}
	}
}

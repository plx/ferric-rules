package ffi

import (
	"fmt"
	"strings"
)

// CStringError reports an embedded NUL in a Go string that would otherwise
// cross a legacy NUL-terminated C string boundary.
type CStringError struct {
	Argument string
	Byte     int
}

// Error describes the rejected argument and the byte offset of its first NUL.
func (e *CStringError) Error() string {
	return fmt.Sprintf("%s contains embedded NUL at byte %d", e.Argument, e.Byte)
}

// ValidateCString rejects embedded NUL before value crosses a legacy
// NUL-terminated C string boundary.
func ValidateCString(argument, value string) error {
	if index := strings.IndexByte(value, 0); index >= 0 {
		return &CStringError{Argument: argument, Byte: index}
	}
	return nil
}

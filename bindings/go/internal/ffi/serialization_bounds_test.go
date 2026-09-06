package ffi

import (
	"testing"
)

func TestCopySnapshotRejectsLengthsOutsideCIntAndFrees(t *testing.T) {
	for _, length := range []uintptr{1 << 31, ^uintptr(0)} {
		freed := 0
		// No allocation is needed: rejection must precede any pointer read.
		got, rc := copyAndFreeBytes(nil, length, func() { freed++ })
		if got != nil || rc != ErrInvalidArgument || freed != 1 {
			t.Fatalf("length %d = (%v, %d), frees %d; want nil invalid argument and one free", length, got, rc, freed)
		}
	}
}

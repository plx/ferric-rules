package ffi

import (
	"os"
	"regexp"
	"testing"
)

func TestDarwinLDFLAGSIncludeObjC(t *testing.T) {
	source, err := os.ReadFile("ffi.go")
	if err != nil {
		t.Fatalf("read ffi.go: %v", err)
	}
	darwinFlags := regexp.MustCompile(`(?m)^#cgo darwin LDFLAGS:.*(?:^|\s)-lobjc(?:\s|$)`)
	if !darwinFlags.Match(source) {
		t.Fatal("Darwin cgo LDFLAGS must explicitly link libobjc")
	}
}

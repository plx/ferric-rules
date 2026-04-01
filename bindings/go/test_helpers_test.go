package ferric

import (
	"context"
	"runtime"
	"testing"
)

type testCloser interface {
	Close() error
}

// lockThread pins the current goroutine to its OS thread for the
// duration of the test, ensuring engine thread-affinity is satisfied.
func lockThread(t *testing.T) {
	t.Helper()
	runtime.LockOSThread()
	t.Cleanup(runtime.UnlockOSThread)
}

func mustNoError(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func mustClose(t *testing.T, c testCloser) {
	t.Helper()
	if err := c.Close(); err != nil {
		t.Fatalf("close failed: %v", err)
	}
}

func mustAssertFact(t *testing.T, e *Engine, relation string, fields ...any) uint64 {
	t.Helper()
	id, err := e.AssertFact(relation, fields...)
	if err != nil {
		t.Fatalf("assert fact failed: %v", err)
	}
	return id
}

func mustAssertTemplate(t *testing.T, e *Engine, templateName string, slots map[string]any) uint64 {
	t.Helper()
	id, err := e.AssertTemplate(templateName, slots)
	if err != nil {
		t.Fatalf("assert template failed: %v", err)
	}
	return id
}

func mustRun(ctx context.Context, t *testing.T, e *Engine) *RunResult {
	t.Helper()
	result, err := e.Run(ctx)
	if err != nil {
		t.Fatalf("run failed: %v", err)
	}
	return result
}

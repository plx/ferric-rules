package temporal

import "testing"

type testCloser interface {
	Close() error
}

func mustClose(t *testing.T, c testCloser) {
	t.Helper()
	if err := c.Close(); err != nil {
		t.Fatalf("close failed: %v", err)
	}
}

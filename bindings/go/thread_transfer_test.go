package ferric

import (
	"fmt"
	"strings"
	"sync"
	"testing"
)

func TestEngineSerializesConcurrentCalls(t *testing.T) {
	e, err := NewEngine()
	if err != nil {
		t.Fatal(err)
	}
	defer mustClose(t, e)

	const callers = 32
	ids := make(chan uint64, callers)
	errs := make(chan error, callers)
	start := make(chan struct{})
	var workers sync.WaitGroup
	workers.Add(callers)
	for index := range callers {
		go func() {
			defer workers.Done()
			<-start
			id, err := e.AssertFact("transferred", int64(index))
			if err != nil {
				errs <- err
				return
			}
			ids <- id
		}()
	}
	close(start)
	workers.Wait()
	close(ids)
	close(errs)
	for err := range errs {
		t.Errorf("concurrent assertion: %v", err)
	}
	seen := make(map[uint64]bool)
	for id := range ids {
		if seen[id] {
			t.Errorf("duplicate fact ID %d", id)
		}
		seen[id] = true
	}
	if len(seen) != callers {
		t.Fatalf("asserted %d facts, want %d", len(seen), callers)
	}
	if count, err := e.FactCount(); err != nil || count != callers {
		t.Fatalf("FactCount = (%d, %v), want (%d, nil)", count, err, callers)
	}
}

func TestConcurrentValueErrorsRetainTheirCallingThreadDiagnostic(t *testing.T) {
	const callers = 16
	results := make(chan string, callers)
	for index := range callers {
		go func() {
			e, err := NewEngine()
			if err != nil {
				results <- err.Error()
				return
			}
			_, conversionErr := e.AssertFact("invalid", strings.Repeat("x", index)+"\xff")
			if err := e.Close(); err != nil {
				results <- err.Error()
				return
			}
			// The diagnostic remains an owned Go string after another C call
			// and destruction. Distinct UTF-8 offsets identify the producing call.
			want := fmt.Sprintf("from index %d", index)
			if conversionErr == nil || !strings.Contains(conversionErr.Error(), want) {
				results <- fmt.Sprintf("conversion error = %v, want %q", conversionErr, want)
				return
			}
			results <- ""
		}()
	}
	for range callers {
		if message := <-results; message != "" {
			t.Error(message)
		}
	}
}

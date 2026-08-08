package ferric

import (
	"fmt"
	"runtime"
)

const workerPanicStackSize = 64 << 10

// PanicError reports a panic recovered while an internal worker was invoking
// an engine callback. Value is the recovered panic value, and Stack is a
// bounded snapshot of the worker goroutine's stack at recovery time.
//
// Engine changes completed before the panic are not rolled back.
type PanicError struct {
	Value any
	Stack []byte
}

// Error implements error.
func (e *PanicError) Error() string {
	return fmt.Sprintf("ferric: worker callback panicked with %T", e.Value)
}

func invokeWorkerCallback(engine *Engine, fn func(*Engine) error) (err error) {
	completed := false
	defer func() {
		value := recover()
		if completed {
			return
		}
		stack := make([]byte, workerPanicStackSize)
		stack = stack[:runtime.Stack(stack, false)]
		err = &PanicError{Value: value, Stack: stack}
	}()

	err = fn(engine)
	completed = true
	return err
}

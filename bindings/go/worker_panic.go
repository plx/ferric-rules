package ferric

import (
	"context"
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

type workerResponse[T any] struct {
	value T
	err   error
}

// workerCall type-erases a typed callback while its completion closure retains
// the concrete response channel. A dequeued call is completed exactly once by
// its worker.
type workerCall struct {
	complete func(*Engine, error)
}

func newWorkerCall[T any](fn func(*Engine) (T, error)) (*workerCall, <-chan workerResponse[T]) {
	responses := make(chan workerResponse[T], 1)
	call := &workerCall{
		complete: func(engine *Engine, dispatchErr error) {
			if dispatchErr != nil {
				responses <- workerResponse[T]{err: dispatchErr}
				return
			}
			responses <- invokeWorkerCallback(engine, fn)
		},
	}
	return call, responses
}

func waitWorkerResponse[T any](
	ctx context.Context,
	cancelQueued func() bool,
	responses <-chan workerResponse[T],
) workerResponse[T] {
	select {
	case response := <-responses:
		return response
	case <-ctx.Done():
		if cancelQueued() {
			return workerResponse[T]{
				err: fmt.Errorf(
					"ferric: request canceled while waiting for worker: %w",
					requestContextError(ctx),
				),
			}
		}
		// The worker won ownership. Its sole response is authoritative even if
		// cancellation and completion became observable at the same time.
		response := <-responses
		return response
	}
}

//nolint:nonamedreturns // Panic recovery must replace the typed response error.
func invokeWorkerCallback[T any](
	engine *Engine,
	fn func(*Engine) (T, error),
) (response workerResponse[T]) {
	completed := false
	defer func() {
		panicValue := recover()
		if completed {
			return
		}
		stack := make([]byte, workerPanicStackSize)
		stack = stack[:runtime.Stack(stack, false)]
		response.err = &PanicError{Value: panicValue, Stack: stack}
	}()

	response.value, response.err = fn(engine)
	completed = true
	return response
}

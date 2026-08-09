package ferric

import (
	"context"
	"errors"
	"sync"
)

const workerRequestQueueCapacity = 16

var (
	errRequestQueueClosed = errors.New("ferric: request queue is closed")
	errRequestContextDone = errors.New("ferric: request context is done")
)

// requestQueue is a bounded FIFO whose queued entries can be removed by their
// submitters. Removal and dequeue share one lock, making worker ownership a
// single, deterministic transition while immediately reclaiming capacity.
type requestQueue[T any] struct {
	mu       sync.Mutex
	items    []*queuedRequest[T]
	capacity int
	closed   bool
	changed  chan struct{}
}

// queuedRequest is a cancellation handle for one admitted queue entry. Its
// fields are protected by queue.mu.
type queuedRequest[T any] struct {
	queue  *requestQueue[T]
	value  T
	queued bool
}

type dequeuedRequest[T any] struct {
	value T
	ok    bool
}

func newRequestQueue[T any](capacity int) *requestQueue[T] {
	return &requestQueue[T]{
		items:    make([]*queuedRequest[T], 0, capacity),
		capacity: capacity,
		changed:  make(chan struct{}),
	}
}

// enqueue waits for capacity, returning a handle that can remove the request
// until dequeue transfers ownership to the worker.
func (q *requestQueue[T]) enqueue(ctx context.Context, value T) (*queuedRequest[T], error) {
	for {
		q.mu.Lock()
		if q.closed {
			q.mu.Unlock()
			return nil, errRequestQueueClosed
		}
		select {
		case <-ctx.Done():
			err := requestContextError(ctx)
			q.mu.Unlock()
			return nil, err
		default:
		}
		if len(q.items) < q.capacity {
			request := &queuedRequest[T]{queue: q, value: value, queued: true}
			q.items = append(q.items, request)
			q.notifyLocked()
			q.mu.Unlock()
			return request, nil
		}
		changed := q.changed
		q.mu.Unlock()

		select {
		case <-ctx.Done():
			return nil, requestContextError(ctx)
		case <-changed:
		}
	}
}

// dequeue blocks until it can transfer the oldest request to the worker. Once
// the queue is closed it drains admitted work and returns false when empty.
func (q *requestQueue[T]) dequeue() dequeuedRequest[T] {
	for {
		q.mu.Lock()
		if len(q.items) > 0 {
			request := q.removeLocked(0)
			value := request.value
			var zero T
			request.value = zero
			q.mu.Unlock()
			return dequeuedRequest[T]{value: value, ok: true}
		}
		if q.closed {
			q.mu.Unlock()
			return dequeuedRequest[T]{}
		}
		changed := q.changed
		q.mu.Unlock()
		<-changed
	}
}

// cancel removes this request if it is still queued. A false result means the
// worker already owns it, so the caller must wait for the worker response.
func (r *queuedRequest[T]) cancel() bool {
	q := r.queue
	q.mu.Lock()
	defer q.mu.Unlock()
	if !r.queued {
		return false
	}
	for index, request := range q.items {
		if request == r {
			q.removeLocked(index)
			var zero T
			r.value = zero
			return true
		}
	}

	// queued is cleared while holding the same lock that removes an entry, so
	// reaching this point would indicate an internal invariant violation.
	r.queued = false
	return false
}

func (q *requestQueue[T]) close() {
	q.mu.Lock()
	if !q.closed {
		q.closed = true
		q.notifyLocked()
	}
	q.mu.Unlock()
}

func (q *requestQueue[T]) len() int {
	q.mu.Lock()
	defer q.mu.Unlock()
	return len(q.items)
}

func (q *requestQueue[T]) removeLocked(index int) *queuedRequest[T] {
	request := q.items[index]
	copy(q.items[index:], q.items[index+1:])
	q.items[len(q.items)-1] = nil
	q.items = q.items[:len(q.items)-1]
	request.queued = false
	q.notifyLocked()
	return request
}

func (q *requestQueue[T]) notifyLocked() {
	close(q.changed)
	q.changed = make(chan struct{})
}

func requestContextError(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err //nolint:wrapcheck // Public dispatch helpers add operation context.
	}
	// Context implementations are required to report a non-nil Err after Done
	// closes, but keep dispatch deterministic for custom implementations that
	// violate that contract.
	return errRequestContextDone
}

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
	mu           sync.Mutex
	items        []*queuedRequest[T]
	capacity     int
	closed       bool
	notEmpty     chan struct{}
	notFull      chan struct{}
	closedSignal chan struct{}
	observer     *requestQueueObserver
}

// requestQueueObserver is a test-only observation seam. It must be installed
// before concurrent queue use and remain immutable afterward.
type requestQueueObserver struct {
	onCapacityWait func()
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
		items:        make([]*queuedRequest[T], 0, capacity),
		capacity:     capacity,
		notEmpty:     make(chan struct{}, 1),
		notFull:      make(chan struct{}, 1),
		closedSignal: make(chan struct{}),
	}
}

// enqueue waits for capacity, returning a handle that can remove the request
// until dequeue transfers ownership to the worker.
func (q *requestQueue[T]) enqueue(ctx context.Context, value T) (*queuedRequest[T], error) {
	wokeForCapacity := false
	for {
		q.mu.Lock()
		if q.closed {
			q.mu.Unlock()
			return nil, errRequestQueueClosed
		}
		select {
		case <-ctx.Done():
			if wokeForCapacity {
				q.signalNotFullLocked()
			}
			err := requestContextError(ctx)
			q.mu.Unlock()
			return nil, err
		default:
		}
		if len(q.items) < q.capacity {
			request := &queuedRequest[T]{queue: q, value: value, queued: true}
			q.items = append(q.items, request)
			q.signalNotEmptyLocked()
			if wokeForCapacity {
				q.signalNotFullLocked()
			}
			q.mu.Unlock()
			return request, nil
		}
		wokeForCapacity = false
		observer := q.observer
		q.mu.Unlock()
		if observer != nil && observer.onCapacityWait != nil {
			observer.onCapacityWait()
		}

		select {
		case <-ctx.Done():
			return nil, requestContextError(ctx)
		case <-q.closedSignal:
			// Loop so the closed check remains serialized with admission.
		case <-q.notFull:
			wokeForCapacity = true
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
		q.mu.Unlock()
		select {
		case <-q.notEmpty:
		case <-q.closedSignal:
		}
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
		close(q.closedSignal)
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
	q.signalNotFullLocked()
	return request
}

func (q *requestQueue[T]) signalNotEmptyLocked() {
	select {
	case q.notEmpty <- struct{}{}:
	default:
	}
}

func (q *requestQueue[T]) signalNotFullLocked() {
	if q.closed || len(q.items) >= q.capacity {
		return
	}
	select {
	case q.notFull <- struct{}{}:
	default:
	}
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

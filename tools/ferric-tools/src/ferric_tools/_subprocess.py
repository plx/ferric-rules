"""Parallel subprocess execution helpers."""

from __future__ import annotations

from collections.abc import Callable, Generator, Iterable
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor, as_completed


def parallel_run[T, R](
    fn: Callable[[T], R],
    items: Iterable[T],
    *,
    workers: int = 4,
) -> Generator[R, None, None]:
    """Run *fn* over *items* in parallel, yielding results as they complete.

    Tries ProcessPoolExecutor first; falls back to ThreadPoolExecutor if
    process spawning fails (e.g. restricted environments).
    """
    items_list = list(items)
    if not items_list:
        return

    max_workers = max(1, workers)

    try:
        process_executor = ProcessPoolExecutor(max_workers=max_workers)
    except (PermissionError, OSError):
        process_executor = None

    if process_executor is not None:
        # Do not reinterpret worker or submission failures as an unavailable
        # process pool. Retrying the complete item set in threads after any
        # process work may have started can execute compatibility fixtures
        # twice and turn a partial infrastructure failure into misleading
        # evidence.
        try:
            first_future = process_executor.submit(fn, items_list[0])
        except (PermissionError, OSError):
            # Process pools normally spawn lazily on the first submission.
            # Falling back is safe only while no submission has succeeded.
            process_executor.shutdown(wait=True, cancel_futures=True)
        else:
            with process_executor as ex:
                futures = [first_future]
                futures.extend(ex.submit(fn, item) for item in items_list[1:])
                for fut in as_completed(futures):
                    yield fut.result()
            return

    with ThreadPoolExecutor(max_workers=max_workers) as ex:
        futures = [ex.submit(fn, item) for item in items_list]
        for fut in as_completed(futures):
            yield fut.result()

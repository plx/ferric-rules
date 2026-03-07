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
        with ProcessPoolExecutor(max_workers=max_workers) as ex:
            futures = [ex.submit(fn, item) for item in items_list]
            for fut in as_completed(futures):
                yield fut.result()
        return
    except (PermissionError, OSError):
        pass

    with ThreadPoolExecutor(max_workers=max_workers) as ex:
        futures = [ex.submit(fn, item) for item in items_list]
        for fut in as_completed(futures):
            yield fut.result()

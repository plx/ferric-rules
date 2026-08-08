"""Tests for exactly-once parallel execution fallback behavior."""

from __future__ import annotations

from concurrent.futures import Future

import pytest

from ferric_tools import _subprocess as subprocess_module


class _FailingWorkerPool:
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def submit(self, _fn, _item):
        future: Future[int] = Future()
        future.set_exception(OSError("worker failed"))
        return future


class _UnavailableProcessPool:
    def submit(self, _fn, _item):
        raise PermissionError("process spawning is unavailable")

    def shutdown(self, *, wait, cancel_futures):
        assert wait is True
        assert cancel_futures is True


def test_first_submission_failure_falls_back_to_threads(monkeypatch):
    monkeypatch.setattr(
        subprocess_module,
        "ProcessPoolExecutor",
        lambda **_kwargs: _UnavailableProcessPool(),
    )

    assert list(subprocess_module.parallel_run(lambda item: item * 2, [1, 2], workers=1)) == [2, 4]


def test_worker_oserror_is_not_retried_in_threads(monkeypatch):
    monkeypatch.setattr(
        subprocess_module,
        "ProcessPoolExecutor",
        lambda **_kwargs: _FailingWorkerPool(),
    )

    def reject_thread_retry(**_kwargs):
        raise AssertionError("worker failure must not retry the complete item set")

    monkeypatch.setattr(subprocess_module, "ThreadPoolExecutor", reject_thread_retry)

    with pytest.raises(OSError, match="worker failed"):
        list(subprocess_module.parallel_run(lambda item: item, [1], workers=1))

"""Exercise foreign-thread finalization from CPython's atexit phase."""

import atexit
import gc
import os
import threading

import ferric


BASELINE = ferric.engine_instance_count()
engine = ferric.Engine()


def verify_reclaimed() -> None:
    gc.collect()
    remaining = ferric.engine_instance_count()
    print(f"baseline={BASELINE} remaining={remaining}", flush=True)
    if remaining != BASELINE:
        os._exit(86)


def drop_on_foreign_thread() -> None:
    global engine

    owned_engine = engine
    engine = None

    def worker(value) -> None:
        del value
        gc.collect()

    thread = threading.Thread(target=worker, args=(owned_engine,))
    del owned_engine
    thread.start()
    thread.join()


# atexit callbacks run in reverse registration order: drop first, verify next.
atexit.register(verify_reclaimed)
atexit.register(drop_on_foreign_thread)

# Ferric for Python

`ferric` is the CPython interface to the Ferric rules engine, an
almost-drop-in replacement for CLIPS implemented in Rust. The extension exposes
engine construction, rule loading and execution, typed fact inspection, output
capture, and snapshot serialization.

## Release status

The repository builds and verifies Python release artifacts, but stable PyPI
publication is gated on the independent production-readiness audit in
[issue #223](https://github.com/plx/ferric-rules/issues/223). The presence of
package metadata or CI artifacts does not mean that a stable registry release
has been authorized.

## Supported Python and platforms

The release contract supports GIL-enabled CPython 3.9 through 3.13. Wheels use
PyO3's stable ABI with a Python/ABI tag of `cp39-abi3`, so one wheel per native
target covers every supported minor.

Verified wheel targets are:

- glibc 2.17+ Linux on x86-64 and AArch64 (`manylinux2014`);
- musl 1.2+ Linux on x86-64 and AArch64 (`musllinux_1_2`);
- macOS 10.12+ on x86-64;
- macOS 11.0+ on Apple silicon; and
- 64-bit Windows on x86-64.

Python 3.14, PyPy, GraalPy, free-threaded CPython, CPython subinterpreters,
macOS universal2, Windows on Arm, and every unlisted platform or architecture
are outside this contract.
The machine-readable source of truth is
[`wheel-targets.json`](https://github.com/plx/ferric-rules/blob/main/crates/ferric-rules-python/wheel-targets.json).

## Example

```python
import ferric

source = """
(deffacts startup (ready))
(defrule complete (ready) => (assert (complete)))
"""

with ferric.Engine.from_source(source) as engine:
    engine.reset()
    result = engine.run()
    assert result.rules_fired == 1

    snapshot = engine.serialize(format=ferric.Format.JSON)

with ferric.Engine.from_snapshot(snapshot, format=ferric.Format.JSON) as restored:
    assert restored.fact_count == 2
```

## Threading, GIL, and lifecycle contract

An `Engine` is operationally bound to the OS thread that constructed it.
Every ordinary operation on an existing handle checks that affinity before
inspecting lifecycle state. A call from another thread raises
`FerricRuntimeError` with a `wrong thread` diagnostic and does not touch engine
state; this remains the result for a foreign-thread ordinary operation while
the owner is doing detached work and after the handle has been closed.

The following potentially long operations release the GIL around their native
CPU or filesystem phase:

- ruleset loading: `Engine.from_source()`, `load()`, and `load_file()`;
- execution: `run()`; and
- snapshots: `serialize()`, `Engine.from_snapshot()`, `save_snapshot()`, and
  `Engine.from_snapshot_file()` when snapshot support is built.

This list is exact. Fact APIs (including `assert_string()`), `step()`,
`reset()`, `clear()`, properties, introspection, protocols, and channel I/O
continue to execute while holding the GIL.

Ferric copies or extracts Python-owned source, snapshot, path, and format
inputs before releasing the GIL. An existing-handle call reserves the engine's
exclusive native-operation lease before detaching, then runs on the same OS
thread that admitted it. The native result or error becomes Rust-owned before
the GIL is reacquired and Python objects or exceptions are created. Releasing
the GIL therefore lets unrelated Python threads and independent engines make
progress without moving an `Engine` reference to another OS thread or making
ordinary operations cross-thread-safe.

`halt()` is a narrow control exception to affinity. It is prompt, idempotent,
and callable from any supported Python thread. It signals only a `run()` that
is already active; an idle, closing, or closed call is a no-op that returns
`None`, does not set `is_halted`, and does not affect a future run. It does not
wait for the active run to return.

An active `run()` checks the control signal before each chunk of at most 64
rule firings and before reporting finite-limit exhaustion. A chunk already in
progress finishes first. An engine error or a natural `AGENDA_EMPTY`,
`ACTION_ERROR`, or rule-side `HALT_REQUESTED` result fixed by that chunk is
preserved; otherwise a halt or close observed at the next check returns a
successful partial `RunResult` with `HaltReason.HALT_REQUESTED`. The bound is in
rule firings, not wall-clock time: one rule action can itself take an
unbounded amount of time.

`close()` and context-manager exit are the lifecycle exceptions to affinity.
The first close marks the handle closing, signals an active run, and releases
the GIL across the entire wait for an admitted native phase and the native
destruction itself. It is synchronous and idempotent; every concurrent closer
returns `None` only after the engine has been destroyed exactly once. A
previously admitted operation keeps its own native result or error. Close does
not cancel admitted load, serialization, or file work, so it waits for that
work to finish naturally and has no general wall-clock latency guarantee. Once
close wins admission, later creator-thread operations raise
`FerricRuntimeError("engine has been closed")`. Context-manager exit applies
the same barrier and still returns `False` so it does not suppress exceptions.

Detachment does not change the existing exception taxonomy. Parse and compile
failures retain their current Ferric subclasses, snapshot codec failures
remain `FerricError`, and snapshot-file access failures remain
`OSError`/`PyIOError`; `load_file()` retains its existing `FerricError`
mapping. `save_snapshot()` serializes before writing;
`from_snapshot_file()` reads before deserializing. A concurrent close never
replaces the result or error of work that was already admitted.

If the final Python reference is released without an explicit close, Ferric
destroys the native engine exactly once on whichever Python thread performs
deallocation. Cleanup does not wait for a future creator-thread call, a
thread-local cleanup pass, a worker thread, or a Python callback. This same
Rust-only path is safe when CPython deallocates an engine during supported
main-interpreter shutdown. The creator thread may exit while another thread
still owns the Python handle; ordinary operations remain unavailable from
other threads, but a later `close()` or final-reference drop still reclaims it.

The raw `Engine` has no asynchronous `aclose()` method, cross-thread ordinary
operation queue, or awaitable API. A public pinned facade with FIFO
cross-thread calls and queued cancellation remains tracked in
[issue #190](https://github.com/plx/ferric-rules/issues/190). CPython
subinterpreters, free-threaded CPython, and alternate Python interpreters are
outside the current support contract.

## Building from source

A source build requires a supported CPython, Rust 1.75 or newer, Maturin 1.x,
and the platform's native compiler and linker. Resolving build dependencies
also requires network access or pre-populated Python and Cargo caches.

From a repository checkout:

```sh
cd crates/ferric-rules-python
uv sync --locked
uv run --locked maturin develop --release
uv run --locked pytest tests/ -v
```

Release source distributions carry the required Rust workspace subset and a
lockfile normalized for that relocated workspace. The exact final archive is
built into a wheel and smoke-tested before it may join the verified dry-run
bundle; publishing Maturin's raw intermediate or any untested sdist is not
allowed.

See the repository's
[Python package release contract](https://github.com/plx/ferric-rules/blob/main/docs/python-package-release.md)
for the complete ABI, target, verification, and publication policy.

## License

Ferric is available under either the
[Apache License 2.0](https://github.com/plx/ferric-rules/blob/main/LICENSE-APACHE)
or the [MIT License](https://github.com/plx/ferric-rules/blob/main/LICENSE-MIT),
at your option. Both license texts are included in every distribution.

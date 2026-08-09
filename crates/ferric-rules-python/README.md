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

## Threading and lifecycle contract

An `Engine` is operationally bound to the OS thread that constructed it.
Every operation on an existing handle except `close()` and context-manager
exit checks that affinity before inspecting lifecycle state. A call from
another thread raises `FerricRuntimeError` with a `wrong thread` diagnostic and
does not touch engine state; this remains the result for a foreign-thread
operation even after the handle has been closed.

`close()` is the lifecycle exception: it may be called from any supported
Python thread. It is synchronous and idempotent, and returning `None` means
that the native engine has already been destroyed. An operation that has
already entered completes before `close()` can take ownership; once `close()`
wins, later creator-thread operations raise
`FerricRuntimeError("engine has been closed")`. Context-manager exit is also
allowed on any supported Python thread and applies the same close barrier.

If the final Python reference is released without an explicit close, Ferric
destroys the native engine exactly once on whichever Python thread performs
deallocation. Cleanup does not wait for a future creator-thread call, a
thread-local cleanup pass, a worker thread, or a Python callback. This same
Rust-only path is safe when CPython deallocates an engine during supported
main-interpreter shutdown. The creator thread may exit while another thread
still owns the Python handle; operations remain unavailable from other
threads, but a later `close()` or final-reference drop still reclaims it.

The raw `Engine` does not provide an asynchronous `aclose()` method and this
lifecycle guarantee does not make its ordinary operations cross-thread-safe.
GIL release for long operations and a serialized pinned/async facade are
separate work. CPython subinterpreters, free-threaded CPython, and alternate
Python interpreters are outside the current support contract.

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

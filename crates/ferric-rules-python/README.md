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

Python 3.14, PyPy, GraalPy, free-threaded CPython, macOS universal2, Windows on
Arm, and every unlisted platform or architecture are outside this contract.
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

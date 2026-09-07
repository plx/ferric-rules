# Ferric for Python

`ferric` is the CPython interface to the Ferric rules engine, an
almost-drop-in replacement for CLIPS implemented in Rust. The extension exposes
engine construction, rule loading and execution, typed fact inspection, output
capture, and snapshot serialization.

## Release status

The repository builds locally installable wheels and source distributions and
verifies their contents with clean consumer tests. Public registry publication
is outside the current rehabilitation scope; no tagged or stable PyPI release
is implied by those artifacts.

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

    snapshot = engine.serialize()  # Versioned CBOR is the recommended default.

with ferric.Engine.from_snapshot(snapshot) as restored:
    assert len(restored.find_facts("complete")) == 1
```

## Values, options, and snapshots

Plain Python `str` and `ferric.String` create CLIPS string literals. Use
`ferric.Symbol("ready")` for an unquoted CLIPS symbol. `bool` maps to the symbols
`TRUE`/`FALSE`; `int` must fit a signed 64-bit integer, `float` maps to a native
float, `None` to void, and lists/tuples to nested multifields. Conversion rejects
unsupported objects and external identities explicitly. Host multifields are
limited to 32 levels and 1,000,000 total values in an assertion.

Returned symbol/string values use owned `Symbol`/`String` wrappers. Equality
and hashing compare only wrappers of the same type and payload; neither equals
a plain Python string. Use `.value` or `str(value)` when comparing host text.
These rules also apply inside nested multifields and template slots.

`Engine(max_call_depth=64)` and `Engine.from_source(source, max_call_depth=64)`
accept a keyword-only integer from 0 through 4,294,967,295; booleans and
fractional values are rejected. Zero disallows user-function calls. The
read-only `engine.max_call_depth` property reports the requested value;
`engine.effective_max_call_depth` reports its enforced ceiling of 32 in every
build profile. Requested values survive snapshots. Active expression evaluation
is separately bounded at 64 frames, and translated expression trees at 16
levels, so nested bodies may reach their expression limit first. These limits
return owned action diagnostics instead of overflowing native recursion.
Release workers are tested with 512 KiB native stacks; unoptimized development
builds need at least the ordinary Rust 2 MiB stack for the tested runtime paths.
Arbitrarily tiny host thread stacks are unsupported.

Snapshots use the versioned CBOR envelope by default. Explicit `Format` values
remain available as experimental codecs inside the same envelope. Inputs are
limited to 16 MiB; file restore reads only the limit plus one byte before
validation. Unknown versions, corruption, unsupported external values, and
invalid restored state raise `FerricSerializationError`. File access errors
raise `OSError`. Legacy raw snapshots are rejected; use their producing Ferric
version to export durable application facts before updating. See the
[shared launch-selection rules](../../examples/embedding/launch-selection.clp)
for a deterministic embedding scenario exercised before and after restore.

Pre-1.0 migration: replace plain strings with `Symbol(...)` where a rule expects
a symbol; replace wrapper-to-str comparisons with typed wrappers or `.value`;
use explicit `Format.BINCODE` only for the experimental Bincode codec. Catch
`FerricSerializationError` for snapshot failures. `Interpret` errors now raise
`FerricParseError`, unsupported/invalid/validation load errors raise
`FerricCompileError`, and source-file I/O failures raise `OSError`. Every load
diagnostic remains in the message; parse takes precedence over compile errors,
then the first remaining error determines the class. Existing `halt()` behavior
is a signal to an active run, as documented below.

## Threading, GIL, and lifecycle contract

An `Engine` may be used and closed from any supported Python thread, including
after its creator thread exits. Calls on one engine serialize through its
native ownership mutex. Waiting for that mutex releases the GIL, so a reader
waiting behind `run()` cannot prevent the running call from returning. Calls
that reenter the same engine during Python value conversion or finalization
raise `FerricRuntimeError` instead of waiting for their own reservation.

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
inputs before releasing the GIL. Long operations acquire and release the
native mutex entirely while detached; native results and errors are owned
before Python objects are constructed. Short operations acquire the mutex
without blocking under the GIL, then build their Python results while attached.
Facts, values, and snapshot bytes returned to Python are owned independently
of the engine. Independent engines can execute native work concurrently.

`halt()` is prompt, idempotent, and callable from any supported Python thread.
It signals only a `run()` that is already active; an idle, closing, or closed
call is a no-op that returns `None`, does not set `is_halted`, and does not
affect a future run. It does not wait for the active run to return.

An active `run()` checks the control signal before each chunk of at most 64
rule firings and before reporting finite-limit exhaustion. A chunk already in
progress finishes first. An engine error or a natural `AGENDA_EMPTY`,
`ACTION_ERROR`, or rule-side `HALT_REQUESTED` result fixed by that chunk is
preserved; otherwise a halt or close observed at the next check returns a
successful partial `RunResult` with `HaltReason.HALT_REQUESTED`. The bound is in
rule firings, not wall-clock time: one rule action can itself take an
unbounded amount of time.

`close()` and context-manager exit are synchronous lifecycle barriers.
The first close marks the handle closing, signals an active run, and releases
the GIL across the entire wait for an admitted native phase and the native
destruction itself. It is synchronous and idempotent; every concurrent closer
returns `None` only after the engine has been destroyed exactly once. A
previously admitted operation keeps its own native result or error. Close does
not cancel admitted load, serialization, or file work, so it waits for that
work to finish naturally and has no general wall-clock latency guarantee. Once
close wins admission, later ordinary operations raise
`FerricRuntimeError("engine has been closed")`. Context-manager exit applies
the same barrier and still returns `False` so it does not suppress exceptions.

`save_snapshot()` validates and serializes before writing;
`from_snapshot_file()` reads within the byte limit before deserializing. A
concurrent close never replaces the result or error of work already admitted.

If the final Python reference is released without an explicit close, Ferric
destroys the native engine exactly once on whichever Python thread performs
deallocation. Cleanup does not wait for a future creator-thread call, a
thread-local cleanup pass, a worker thread, or a Python callback. This same
Rust-only path is safe when CPython deallocates an engine during supported
main-interpreter shutdown. The creator thread may exit while another thread
still owns the Python handle and continues to use or close it.

`Engine` provides synchronous serialized calls, not an asynchronous queue or
an `aclose()` method. Use a host executor to offload long operations when an
application needs an awaitable interface. CPython subinterpreters,
free-threaded CPython, and alternate Python interpreters remain outside the
current support contract.

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

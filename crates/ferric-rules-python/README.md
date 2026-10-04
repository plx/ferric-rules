# Ferric for Python

`ferric` is the CPython interface to the Ferric rules engine, an
almost-drop-in replacement for CLIPS implemented in Rust. The extension exposes
engine construction, rule loading and execution, typed fact inspection, output
capture, and snapshot serialization.

## Release status

The repository supports local source builds and host wheels. CI builds a host
wheel and exercises it from a fresh virtual environment outside the checkout.
Public registry publication is not currently planned; these checks do not imply
a tagged or stable PyPI release.

## Supported Python and platforms

The binding supports GIL-enabled CPython 3.9 through 3.14. Wheels use
PyO3's stable ABI with a Python/ABI tag of `cp39-abi3`, so one wheel per native
target covers every supported minor.

Python 3.15, PyPy, GraalPy, free-threaded CPython, CPython subinterpreters,
and cross-platform wheel distribution are outside the current support contract.
Local builds use the host's native compiler and linker; CI checks source tests
and the isolated host-wheel consumer on its configured runners.

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
`ferric.Symbol("ready")` for an unquoted CLIPS symbol and
`ferric.InstanceName("widget")` for the instance name `[widget]`. `bool` maps to the symbols
`TRUE`/`FALSE`; `int` must fit a signed 64-bit integer, `float` maps to a native
float, and lists/tuples to nested multifields. Fact inputs reject `None` (void),
including nested values, because void represents an absent result and cannot be
persisted as fact data. Use an explicit symbol such as `Symbol("nil")` when the
application needs a stored sentinel. Conversion rejects unsupported objects and
external identities explicitly. Host multifields are
limited to 32 levels and 1,000,000 total values in an assertion.

Returned symbol/string/instance-name values use owned `Symbol`/`String`/`InstanceName` wrappers. Equality
and hashing compare only wrappers of the same type and payload; neither equals
a plain Python string. Use `.value` or `str(value)` when comparing host text.
These rules also apply inside nested multifields and template slots. Owned
returned values remain usable after closing their engine and can be asserted
into another engine, which interns their symbol text in its own symbol table.

Fact IDs are opaque, engine-scoped unsigned 64-bit integers. Pass them back to
the engine that returned them; IDs from another engine or before reset/restore
do not identify live facts. Store application IDs in fact fields for persistence
instead of saving `Fact.id`. Reading a `Fact` returns an owned snapshot of its
fields, which remains readable after retraction or engine shutdown.

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

Snapshots use the versioned CBOR envelope by default. `Format.JSON` is also
available (for debugging and inspection) inside the same envelope. Inputs are
limited to 16 MiB; file restore reads only the limit plus one byte before
validation. Unknown versions, corruption, unsupported external values, and
invalid restored state raise `FerricSerializationError`. File access errors
raise `OSError`. Legacy raw snapshots are rejected; use their producing Ferric
version to export durable application facts before updating. See the
[shared launch-selection rules](../../examples/embedding/launch-selection.clp)
for a deterministic embedding scenario exercised before and after restore.

Pre-1.0 migration: replace stored `None` values with an application sentinel and
persist application IDs instead of native fact IDs. Replace plain strings with
`Symbol(...)` where a rule expects
a symbol; replace wrapper-to-str comparisons with typed wrappers or `.value`;
`Format.BINCODE`, `Format.MSGPACK` and `Format.POSTCARD` were removed, so use
`Format.CBOR` (the default) or `Format.JSON`. Catch
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

A source build requires a supported CPython, Rust 1.83 or newer, Maturin 1.x,
and the platform's native compiler and linker. Resolving build dependencies
also requires network access or pre-populated Python and Cargo caches.

From a repository checkout:

```sh
cd crates/ferric-rules-python
uv sync --locked
uv run --locked maturin develop --release
uv run --locked pytest tests/ -v
```

To check installation independently of the development environment, run from
the repository root:

```sh
scripts/python-consumer-smoke.sh
```

The script uses the locked Maturin dependency to build one release wheel for
the host, installs it into a fresh virtual environment outside the checkout,
and runs Python in isolated mode. The consumer verifies its import location,
rule execution, facts, output, and snapshot restoration. An optional interpreter
path selects the Python used for both the build and consumer:
`scripts/python-consumer-smoke.sh /path/to/python3.14`.

## License

Ferric is available under either the
[Apache License 2.0](https://github.com/plx/ferric-rules/blob/main/LICENSE-APACHE)
or the [MIT License](https://github.com/plx/ferric-rules/blob/main/LICENSE-MIT),
at your option. Both license texts are included in every distribution.

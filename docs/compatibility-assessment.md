# Compatibility assessment oracles

The `just compat-*` pipeline produces differential evidence between Ferric and
the pinned reference CLIPS container. Manifest schema v3 is intentionally
fail-closed: matching process output, including matching empty output, is not
compatibility evidence by itself.

## Fixture declarations

Executable evidence starts in
`tests/examples/compat-oracles.json`. The registry is versioned, rejects
duplicate JSON fields and duplicate fixture identities, and maps normalized
paths relative to `tests/examples/` to oracle-v1 declarations.

Each declaration binds:

- a protocol-safe fixture ID and a human-readable feature;
- the exact source and composed-input SHA-256 digests;
- the `load`, `reset`, `run` setup sequence;
- expected phase, firing count or ordered firing names, semantic effects,
  canonical final facts, stdout and stderr, diagnostic state, run termination,
  and any selected focus-stack or global state; and
- the fixture-specific normalizers that are allowed.

Oracle v1 supports an unlimited run, `agenda-empty` or `halt-requested`
completion, and exactly the `stdout` and `stderr` semantic channels. Firing
count must agree with the number of firing names when both are declared.
Unsupported declarations are invalid configuration, not engine divergence.

The only normalizers are:

- `fact-ids` — ignore engine-assigned fact identity;
- `fact-order` — compare final facts and their fact-derived effects without
  enumeration order; and
- `float-format` — compare finite decimal float spellings by numeric value.

No normalizer is applied globally. Duplicate facts and values remain
significant.

When a fixture or generated harness changes, update both digests in its
declaration. `just compat-scan` rejects a stale declaration before an engine is
started.

## Observation boundary

Every run receives a fresh 128-bit nonce. Both engines must produce exactly one
nonce- and digest-bound `START`/`COMPLETE` lifecycle and a complete post-run
observation.

Ferric exposes this through the hidden `ferric compat-observe` command, leaving
the public `ferric run --json` contract unchanged. Reference CLIPS is loaded by
a dedicated native embedding that owns the single reset/run boundary, counts
the agenda across every module, and enables typed post-run probes only after
`EnvRun` returns. It emits length-prefixed, nonce-bound records on the process
stderr boundary, separate from fixture router output. Each record also carries
a keyed authentication tag whose per-run key is never emitted. The nonce,
identity, and authentication binding are consumed before the CLIPS environment
is created and never enter the fixture-visible environment or command stream.
Parsing uses byte offsets, so UTF-8 values are framed by their encoded byte
length.

The native observer is built into `ferric-rules/clips-reference:latest` from
`docker/clips-reference/`. Rebuild that local image after changing the
observer:

```console
docker build -t ferric-rules/clips-reference:latest docker/clips-reference
```

Do not use `just clips-build` for a local-only rebuild: that recipe publishes
unless invoked with its explicitly local options.

Generated verifier records, facts, and firings are instrumentation, not
fixture effects. If either adapter cannot separate instrumentation from
feature behavior, the observation is invalid rather than equivalent.

## Classification and exit behavior

An entry is `equivalent` only when:

- its declaration is current and valid;
- both engines independently reach and complete the observation;
- each engine demonstrates the declared feature effect and all expectations;
  and
- the normalized observations agree with each other.

A valid semantic mismatch is `divergent`. A missing declaration is
`pending/oracle-missing`. Missing, stale, malformed, incomplete, spoofed, or
unsupported evidence is `pending/oracle-invalid:*`; the exact composed input
is retained under `.ferric-compat/failures/`, when one exists, and
`compat-run` exits nonzero. Expected diagnostics remain invalid until
phase-aware diagnostic classification is implemented.

Manifest-v2 output-based results are deliberately reset during schema-v3
migration. Undeclared fixtures remain pending instead of preserving legacy
`equivalent` or `divergent` claims.

## Maintainer workflow

Run the assessment from the repository root:

```console
cargo build --release -p ferric-cli
just compat-scan
just compat-run
just compat-report
```

`compat-report` exposes declaration, lifecycle, effect, version, and
normalization coverage. Compare a baseline and candidate with:

```console
just compat-diff BASE_MANIFEST HEAD_MANIFEST
```

For schema-v3 heads, the diff gate rejects every unverified equivalence claim,
including a legacy claim copied forward unchanged, as well as evidence-coverage
loss. The initial empty-output control is
`tests/examples/ferric-oracle/empty-output-state.clp`; it is equivalent only
because both engines prove the declared final fact, effect, and firing count.

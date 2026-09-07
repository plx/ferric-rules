# Snapshot contract

Enable the Rust `serde` feature and use `SerializationFormat::Cbor` (also
`SerializationFormat::RECOMMENDED`) for persistence. The recommended codec is
the existing `ciborium` dependency; the unmaintained bincode 1.x codec remains
available only as an experimental format. JSON, MessagePack, and Postcard also
remain experimental. This changes no format discriminants or method signatures.

The CLI built with `--features serde` also defaults to CBOR for both
`ferric snapshot rules.clp -o state.ferric` and
`ferric repl --snapshot state.ferric`. Existing callers choosing an experimental
codec must specify `--format` when saving and `--snapshot-format` when restoring.
The default change accompanies the legacy raw snapshot break below.

Snapshots retain facts and their internal rule-matching identities, templates, globals and their reset
initializers, deffacts, functions, rules, focus, queued activations, refraction,
halt state, buffered input/output, and diagnostics. Successful restoration
preserves subsequent rule behavior, including pending activations and the last
blocker/support transitions of `not`, `exists`, and negated conjunctions.
Compilation caches are retained and validated so later rule installation works.
An already fired activation does not reappear merely because the engine was
restored. No callbacks or host objects are installed by decoding. Host-facing fact handles
are recreated after restoration; query facts by durable application IDs instead
of persisting a handle. See [host-api.md](host-api.md).

## Versions and application updates

Schema 1 is the first versioned snapshot format. Builds supporting schema 1 must
keep its meaning and pass the stored schema fixture and resume regressions.
Changes to the serialized layout or runtime semantics that make an old state
invalid require a schema-version change, a documented compatibility decision,
and a fixture regression. Crate version and snapshot schema version are separate.
Snapshots are not a promise to migrate arbitrary RETE internals forever.

This is an explicit pre-1.0 break from legacy raw snapshots. Unversioned bytes
return `LegacySnapshot`; Ferric never guesses a codec, rebuilds an empty engine,
or silently drops persisted facts. The legacy fixture records the old raw CBOR
shape and is deliberately rejected by the public restore API. Before upgrading
an application that owns legacy data, use its producing Ferric version to
export that application's durable data, then assert it into the new engine.
Keep the original bytes until that application migration is verified.

Rebuilding a compiled engine cache from rule source and seed facts is useful
when the cache contains no unique application data. It is not recovery of
durable task or session state. Applications should retain their authoritative
data and rule/schema versions independently when they need a migration path
across unsupported snapshot versions. Ferric supplies no scheduler or general
migration framework.

## Envelope and limits

Every format uses the same binary envelope, including experimental JSON:

| Bytes | Meaning |
| --- | --- |
| 0–7 | Magic `FERRIC\0S` |
| 8–9 | Little-endian schema version (`1`) |
| 10 | Codec: bincode `0`, JSON `1`, CBOR `2`, MessagePack `3`, Postcard `4` |
| 11 | Capability flags (`0`; unknown flags are rejected) |
| 12–19 | Little-endian payload byte length |
| 20–51 | SHA-256 of bytes 0–19 followed by the payload |
| 52 onward | Codec payload |

The checksum detects accidental corruption; it is not authentication. The caller
selects the codec, which must match the header. Trailing bytes, corrupt lengths,
unknown versions/flags, and mismatched checksums are errors before state is
installed. Never accept snapshots as trusted executable policy solely because
their checksum is valid.

Supported persistence bounds are 16 MiB including the envelope, 128 Serde nesting
levels, and 1,000,000 decoded items across the whole payload. Collection length
hints are checked before allocation and do not control allocation capacity.
Runtime values allow 32 nested multifields; stored action/expression trees allow
16 levels, alpha paths 64 tests, beta parent paths 66 nodes (including root
and terminal), and NCC nesting 4. Requested call-depth configuration is
preserved; all restored engines apply the same effective 32-call ceiling and
64 active-expression-frame limit as fresh engines. NCC partner branches must share their declared prefix and cannot form callback cycles.
Graph validation has a 10,000,000-operation work allowance and a separate equal
allowance for compiler-cache validation. It charges cross-products and test/index
widths before evaluating them. A valid but unusually large engine can exceed
these persistence bounds; its direct engine API remains usable.

Writes use a bounded output buffer and the same read limits before returning
bytes. JSON explicitly rejects non-finite floats. Recommended CBOR preserves
NaN payload bits, infinities, negative zero, and signed 64-bit integers.
Host-owned `ExternalAddress` identities, including nested values, are unsupported
and rejected rather than changed to null.

## Validation and errors

Restoration checks symbol pools and value references; fact identities, timestamps
and indexes; template slot cardinalities and indexes; module and construct
ownership; runtime rule metadata; graph ancestry and memory ownership; exact
positive joins and their bindings; complete negative/exists support and NCC
ownership; token reverse indexes; activation identity, chronology, recency and
strategy keys; and compiler-cache references. It rejects unfinished predicate
work. Historical predicate outcomes are retained, since re-evaluating a predicate
against globals changed later would alter refraction and resume behavior.

These checks run at snapshot boundaries, not on ordinary evaluation paths.
Decoding creates a separate engine; an error cannot partially replace the caller's
existing engine. Callers receive owned errors distinguishing legacy data, unknown
version/capabilities, wrong format, limits, checksum failure, codec failure, and
invalid restored state. The C and language bindings preserve useful diagnostics
through their existing snapshot error categories.

Fact timestamps and beta node IDs use checked allocation. An exhausted counter
remains a valid snapshot state: reads, retraction, and persistence still work.
A new assertion returns an owned error; an unsuccessful `modify` preserves the
original fact. Reset starts a fresh fact chronology and the documented seed
state. Rule loading checks the complete definition (including all `or` variants)
before replacing an old rule, and reports exhaustion without partially changing
the network. A fresh engine is needed for further rule installation once beta
node IDs are exhausted. No counter wraps or silently reuses an old identity.

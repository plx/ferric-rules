# Snapshot contract

Enable the Rust `serde` feature and use `SerializationFormat::Cbor` (also
`SerializationFormat::RECOMMENDED`) for persistence, via the `ciborium` crate.
`SerializationFormat::Json` is also available for debugging and inspection.
The former experimental bincode, MessagePack, and Postcard codecs were removed;
their envelope and C/binding discriminants (`0`, `3`, `4`) are rejected and will
not be reused.

The CLI built with `--features serde` also defaults to CBOR for both
`ferric snapshot rules.clp -o state.ferric` and
`ferric repl --snapshot state.ferric`. JSON snapshots need `--format json` when
saving and `--snapshot-format json` when restoring.

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

The current schema is 13. Query members retain their ordered restriction
expressions, including dynamic and multiple targets. Restoration validates these
expressions without executing them; queries resolve their targets when called.
Active query cursors and their temporary target protection are not persisted.
Schema 12 stored only single template names and is rejected with
`UnsupportedVersion(12)`.

Snapshots retain each rule's CLIPS specificity and
each activation's conditional-element recencies, including absent positions.
LEX and MEA ordering keys use sorted recencies, specificity, and older
activation ties. Restore checks this metadata against terminals and the token
graph. Schema 11 used different ordering semantics and is rejected with
`UnsupportedVersion(11)`.

Snapshots retain each rule's auto-focus flag and its
resolved definition-time salience. Restoration rebuilds focus subscriptions
without evaluating the declaration again or pushing modules for pending
activations. Schema 10 lacks this rule metadata and is rejected with
`UnsupportedVersion(10)`.

Snapshots preserve the primary blocker and attachment
chronology of simple negative and NCC matches. Removing a blocker after restoring
therefore migrates or unblocks matches in the same order as uninterrupted
execution. Schema 9 lacks this history and is rejected with `UnsupportedVersion(9)`.

Snapshots retain the per-engine random generator's
position, construct declaration order for introspection, and the original order
of allowed slot values. Restoring never reseeds the random stream or sorts those
lists. Schema 8 lacks this state and is rejected with `UnsupportedVersion(8)`.

Templates retain allowed-value lists, numeric ranges,
multislot cardinality constraints, evaluated static defaults, and deferred dynamic
defaults with their defining module. Restoring a snapshot does not execute
defaults; omitted slots evaluate dynamic defaults when a fact is asserted.
Schema 7 lacks this metadata and is rejected with `UnsupportedVersion(7)`.

Expressions retain executable engine effects, and
source reset/clear now apply immediately while preserving active execution.
The chronology flag used after a refused source clear is persisted so restored
fact indices continue from zero. Schema 6 lacks this flag and the corrected
execution contract and is rejected with `UnsupportedVersion(6)`.

Typed fact addresses retain their assertion identity,
working-memory epoch, and public display index. Live, retracted, and dummy
addresses survive snapshots, including nested values and rule bindings. Restored
address metadata is checked against the working-memory chronology. Schema 5
lacks this representation and is rejected with `UnsupportedVersion(5)`.

Generic methods retain fixed-parameter queries,
wildcard type restrictions, and wildcard queries. Schema 4 lacks that metadata
and is rejected with `UnsupportedVersion(4)`. This version also uses corrected
loop, empty-callable, and wildcard argument semantics.

Deffacts store executable field initializers rather
than values computed at load time, so their expressions and global references
run on each reset. Schema 3 snapshots lack these initializers and are rejected
with `UnsupportedVersion(3)` before payload decoding.

Field-level `|` constraints remain a single
predicate in the compiled graph, including inside negated and quantified
patterns. Schema 2 could store expanded rule variants with incorrect
multiplicity and matching behavior; it cannot be resumed under the corrected
semantics. Schema 1 also predates field-count guards, multifield match plans,
and per-token capture lengths. Both versions are rejected with
`UnsupportedVersion(1)` or `UnsupportedVersion(2)` before payload decoding.
To upgrade application data, export it with the producing Ferric version and
assert it into a newly compiled engine; there is no automatic RETE-state
migration.

Builds supporting a schema must keep its meaning and pass the stored fixture
and resume regressions. Changes to the serialized layout or runtime semantics
that make an old state invalid require a schema-version change, a documented
compatibility decision, and a fixture regression. Crate version and snapshot
schema version are separate. Snapshots are not a promise to migrate arbitrary
RETE internals forever.

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

Every format uses the same binary envelope, including JSON:

| Bytes | Meaning |
| --- | --- |
| 0–7 | Magic `FERRIC\0S` |
| 8–9 | Little-endian schema version (`13`) |
| 10 | Codec: JSON `1`, CBOR `2` (`0`, `3`, `4` were removed codecs) |
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
16 levels, alpha paths 64 value tests plus one ordered field-count test,
beta parent paths 66 nodes (including root
and terminal), and NCC nesting 4. Requested call-depth configuration is
preserved; all restored engines apply the same effective 32-call ceiling and
64 active-expression-frame limit as fresh engines. NCC partner branches must share their declared prefix and cannot form callback cycles.
Derived template defaults have separate expansion bounds of 1,000,000 fields and
32 MiB of estimated storage, including repeated string payloads. These bounds are
checked before allocation and do not limit a declared maximum cardinality or
facts supplied explicitly. Static values already stored in a snapshot remain
subject to the ordinary snapshot byte and item limits.
A multifield join token is checked by rebuilding its recorded split; other splits
are not re-enumerated. The facts recorded as supporting a negated or existential
multifield pattern must be exactly those that match through some split. Each
capture length the split search tries is charged, plus the size of the fact
whenever a test or binding copies a capture. That search makes some engines too
large to save: a negated member test such as `(key ?k) (not (lst $? ?k $?))`
searches about N²/2 splits for a list of N keys, which exceeds the allowance
at about 1,500 keys (the positive form stays within it). In general the
validation of a negated or existential multifield pattern searches every fact
in its alpha memory for every parent token, about tokens × facts × splits per
fact: `(tag ?t) (exists (item (tags $? ?t $?)))` with four-value lists fails
to save at about 530 tags and 530 items, matching or not.
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
and indexes; template slot types, allowed values, ranges, cardinalities, defaults
and indexes; module and construct
ownership; runtime rule metadata; graph ancestry and memory ownership; exact
positive joins and their bindings; complete negative/exists support and NCC
ownership; token reverse indexes; activation identity, chronology, recency and
strategy keys; and compiler-cache references. It rejects unfinished predicate
work. Historical predicate outcomes are retained, since re-evaluating a predicate
against globals changed later would alter refraction and resume behavior.
Dynamic default expressions receive the same value-identity and depth checks as
other compiled expressions, and their owning module must match their template.
Literal portions of initializers are checked without executing calls or reading
globals; unresolved globals can remain deferred until assertion or reset.

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

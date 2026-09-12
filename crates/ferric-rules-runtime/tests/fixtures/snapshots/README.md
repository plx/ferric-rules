# Snapshot fixtures

`legacy-raw.cbor` uses the pre-envelope `EngineSnapshotRef` layout at
`56b39e7` and the existing ciborium 0.2.2 encoder. It contains the result of:

```clips
(defglobal ?*count* = 3)
(assert (durable 42))
```

The fixture was generated through that unchanged raw payload representation
while implementing the envelope. It is an old-format regression fixture, not a
historical user database. The test checks that the payload contains the durable
fact, then verifies public restore rejects its missing version envelope. The
declared migration is application-data export through the producing version;
Ferric does not automatically migrate raw RETE internals.


`schema-1.cbor` is a version-one CBOR envelope generated on September 6, 2026,
from the finalized ordered join membership, named seeds, typed template metadata,
and bounded evaluator configuration. The producing source is `schema-1.clp`:
construct with `Engine::with_rules`, set focus to `WORK`, run exactly one firing,
then call `serialize(SerializationFormat::Cbor)`. At this checkpoint item 3 is
`done`, global `seen` is 1, item 1 is pending, and item 2 is blocked.

The original schema-1 bytes remain unchanged and are rejected before payload
decoding. Its source regression still resumes item 1, retracts item 2's blocker,
checks output/globals/facts and quiescence, then resets and reruns named seeds
through all five current codecs.

`schema-2.cbor` is the PR351 ordered-cardinality fixture. Its source is
`schema-2.clp`: construct with `Engine::with_rules`, run exactly one firing,
and serialize as CBOR. Two one-field facts match `(row ?)`; a two-field fact
does not. One valid activation remains pending at the checkpoint. These
historical bytes are retained and rejected as schema 2. The source scenario
still runs through all five current codecs, checking wrong-width and valid
future assertions, late rule compilation, and reset.

`schema-3.cbor` stores ordered multifield plans and token capture lengths.
Its source `schema-3.clp` saves after one of three splits of `(row a b)` fires.
The original binary is rejected as version 3; all-five-codec source tests resume
the two remaining activations, check capture widths/refraction, retract and
replace the source fact, load a rule sharing the sequence join, and reset.

`schema-4.cbor` stores independent sequence segments for a template's scalar
slot and two multislots. Its source `schema-4.clp` saves after one of six matches
of one fact fires. The original binary is rejected as version 4. Current-codec
source tests check every capture-width combination, remaining activations,
refraction, source-fact retraction, replacement facts, later rule sharing and
reset. Malformed split identities, projections, logical selectors, physical
template source slots and cache plans retain explicit rejection tests.

`schema-5.cbor` is the PR378 runtime-constraint fixture. Its source is
`schema-5.clp`; both historical files remain unchanged, and the binary is
rejected as schema 5. It records retained local filter decisions and lazy
selected negative-conflict state before the ordered-cardinality composition.

`schema-6.cbor` is the byte-lexeme and typed-instance-name fixture. Its source
`schema-6.clp` supplies a typed INSTANCE-NAME template, a `[seed]` literal and
one pending capture rule. The scenario builder additionally asserts one payload
with STRING bytes `61 00 ff 7a`, SYMBOL bytes `73 ff` and INSTANCE-NAME bytes
`6e ff`. It runs one activation, leaving `TRUE\n` in the router. Its current-codec
resume tests run the remaining capture, check exact raw output and three distinct
global value types, retract the host fact and reset/replay the source seed.
The source protocol runs through every codec; the original schema-6 binary is
preserved and rejected before payload decoding. Text access remains checked;
snapshots do not substitute escaped or replacement text for bytes.

`schema-7.cbor` combines ordered field-count guards and runtime constraints.
The historical `schema-7.clp` is byte-identical to `schema-5.clp`, compiled with
that composed implementation. Construct with `Engine::with_rules`, run exactly
one activation, then serialize as CBOR. Both `data` facts have passed the local
filter; candidate 1 failed the join and candidate 2 is the selected blocker.
The fired rule has changed global `gate` to FALSE while historical local
membership remains accepted. Both callback counters are 2. Compiled ordered
paths also contain their explicit field-count guards.

The schema-7 source tests now encode the scenario in the current format. They
remove the selected blocker, verify that its rejected predecessor is not
reexamined, and resume the pending absence. A fresh outer fact uses the retained
local decision; a reasserted data fact runs the callback again with the changed
global. All-five-codec tests also cover pending/completed checkpoints, reset,
error blockers, initial versus replacement existence-support errors, and forged
role/scope metadata. Restoration never invokes callbacks to reconstruct these
historical decisions. Host handles are freshly queried after restore. The
original schema-7 stored bytes remain unchanged rejection controls.

## Current local integration format

The combined snapshot layout uses schema 8. All prior versions 1–7 are rejected before
payload decoding in every codec, including header-only inputs with missing
payloads or stale checksums. Separate sequence and template-multislot layouts
use historical versions 3 and 4; numbering does not make parallel layouts
interchangeable. The raw legacy fixture remains unchanged and explicitly rejected.

`schema-8.clp` is the new combined source. The Rust builder adds a raw payload
containing STRING `61 00 ff 7a`, SYMBOL `73 ff`, and INSTANCE-NAME `6e ff`, then
queues the framed lines `"queued words" ignored` and `second`. The highest
salience checkpoint rule runs once, changes the filter gate to FALSE, captures
the three distinct lexeme values, emits their exact raw bytes, and scans an
overflowing integer to retain a nonfatal scanner notice. The source also stores
a typed `[seed]` literal and callable bodies.

Checkpoint assertions require ten pending activations: three ordered splits,
six template splits, and one scalar-template runtime match. Two local filter
and two negative-join callbacks have run. Existence support, a runtime join
using an outer multifield capture, and the scalar-template filter have each run
one callback. Short/excess-width runtime facts do not invoke callbacks. The
stored warning uses the existing textual diagnostic alternative and preserves
its separate router bytes.

Both freshly encoded and stored-fixture tests use all five codecs. They take a
partial checkpoint, compare complete capture-width sets, remove the selected
negative conflict without reconsidering rejected predecessors, replace selected
existence support, remove a blocker whose predicate read a sequence capture,
and consume both queued lines through a stored callable. The checkpoint preserves the scanner diagnostic; subsequent runs clear that
history while warning-router bytes persist. Completed snapshots remain quiescent. Later rule installation reuses a restored sequence projection;
reset and clear verify their distinct source/input lifecycles. Host handles are
queried again after each restore. No same-salience firing order is asserted.

The committed `schema-8.cbor` contains 41,441 bytes (41,389 payload bytes), with
SHA-256 `05c1cab97837ca39040b6c68fa8c8449b6e3049308a9f0fc88392600453251cc`.
Its schema, codec, payload length, and envelope checksum are verified. Both the
source and stored-fixture resume protocols pass in all five codecs. To regenerate
a deliberate fixture update, first run the source test:

```sh
cargo test -p ferric-rules-runtime --features serde --lib \
  serialization::tests::combined_schema_eight_source_roundtrips_and_resumes_in_all_codecs -- --exact
cargo test -p ferric-rules-runtime --features serde --lib \
  serialization::tests::regenerate_schema_eight_fixture -- --ignored --exact
```

The single ignored maintenance test writes only `schema-8.cbor`. The ordinary
stored-fixture test reads that file at runtime, allowing the generator to compile
before the binary exists; absence fails the ordinary test. Follow generation
with the stored-fixture and serialization regressions and record the new hash.
Ordinary tests never regenerate fixtures. All historical sources and binaries
remain unchanged, and all retired fixture writers have been removed.

The CLI `snapshot` command loads, resets, and immediately serializes; it does
not run the one-activation checkpoint and therefore must not generate this
fixture. Use the explicit library maintenance test above.

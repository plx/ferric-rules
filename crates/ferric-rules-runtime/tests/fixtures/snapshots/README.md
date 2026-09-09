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

The local integration uses schema 8. All prior versions 1–7 are rejected before
payload decoding in every codec, including header-only inputs with missing
payloads or stale checksums. Separate sequence and template-multislot layouts
use historical versions 3 and 4; numbering does not make parallel layouts
interchangeable. The raw legacy fixture remains unchanged and explicitly rejected.

The combined schema-8 source and stored fixture have not yet been installed or
validated. They will be added after the complete integration layout is composed;
no existing fixture is a substitute and no placeholder is created. The current
maintenance test is the only fixture writer:

```sh
cargo test -p ferric-rules-runtime --features serde --lib \
  serialization::tests::regenerate_schema_eight_fixture -- --ignored --exact
```

It reads `schema-8.clp` at runtime, constructs the engine, runs exactly one
activation, and writes only `schema-8.cbor`. The ordinary stored-fixture test
also reads at runtime, so the generator can compile before the files exist;
missing files fail the relevant test. Follow generation with the stored-fixture
and serialization regressions, and record the new binary hash. Ordinary test
runs never regenerate fixtures. The retired schema-6 and schema-7 writers are
removed so they cannot overwrite historical bytes.

The CLI `snapshot` command loads, resets, and immediately serializes; it does
not run the one-activation checkpoint and therefore must not generate this
fixture. Use the explicit library maintenance test above. This local checkpoint
does not claim completed schema-8 fixture coverage or final integration gates.

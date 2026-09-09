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
under the current format.

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

Schema 7 combines both layouts. Versions 1–6 are rejected before payload
decoding in every codec; schema 3/4/6 belong to the separate sequence,
template-multislot, and byte-lexeme layouts and are not imported here.
Version 8 is reserved for their later composition, not accepted by this build.
The raw legacy fixture also remains unchanged and explicitly rejected.

The current source `schema-7.clp` is byte-identical to `schema-5.clp`, compiled
with the composed implementation. Construct with `Engine::with_rules`, run
exactly one activation, then serialize as CBOR. Both `data` facts have passed
the local filter; candidate 1 failed the join and candidate 2 is the selected
blocker. The fired rule has changed global `gate` to FALSE while historical
local membership remains accepted. Both callback counters are 2. Compiled
ordered paths now also contain their explicit field-count guards.

The current stored-byte test removes the selected blocker, verifies that its
rejected predecessor is not reexamined, and resumes the pending absence. A
fresh outer fact uses the retained local decision; a reasserted data fact runs
the callback again with the changed global. All-five-codec tests also cover
pending/completed checkpoints, reset, error blockers, initial versus replacement
existence support errors, and forged role/scope metadata. Restoration never
invokes callbacks to reconstruct these historical decisions. Host handles are
freshly queried after restore.

Generate only the current fixture from the final coherent implementation:

```sh
cargo test -p ferric-rules-runtime --features serde --lib \
  serialization::tests::regenerate_schema_seven_fixture -- --ignored --exact
```

This ignored maintenance test verifies the checkpoint and writes only
`schema-7.cbor`. The ordinary stored-fixture test reads that file at runtime,
so the maintenance test can compile before the new binary exists; absence of
the file fails the ordinary test. No placeholder or old-version binary is a
valid substitute. Follow generation with the stored-fixture and serialization
regressions, and record the new binary hash. Ordinary test runs never regenerate
fixtures. Retired schema-2 and schema-5 writers are intentionally removed or
retargeted to the current schema so they cannot overwrite historical bytes.

The CLI `snapshot` command loads, resets, and immediately serializes; it does
not run the one-activation checkpoint and therefore must not generate this
fixture. Use the explicit library maintenance test above.

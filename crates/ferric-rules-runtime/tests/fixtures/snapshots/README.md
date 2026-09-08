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

These original bytes are retained unchanged and now explicitly rejected as
schema 1 before payload decoding. The source regression remains: it resumes
item 1, retracts item 2's blocker and resumes it, checks output/globals/facts and
quiescence, then resets and reruns the named seeds under the current format.

`schema-5.cbor` records runtime pattern filtering and lazy negative join state.
Versions 2–4 are reserved for the separate ordered-cardinality,
ordered-multifield, and template-multislot compatibility layouts. This build
rejects all versions 1–4 before payload decoding; the header regression covers
every codec without importing those separate layouts. The schema-1 and raw
legacy fixture bytes remain unchanged.

Its source is `schema-5.clp`. Construct with `Engine::with_rules`, run exactly
one activation, then serialize as CBOR. Both `data` facts have passed the local
filter; candidate 1 failed the join and candidate 2 is the selected blocker.
The one fired rule has changed global `gate` to FALSE, while historical local
membership remains accepted. Both callback counters are 2.

The stored-byte regression removes the selected blocker, checks that its
rejected predecessor is not reexamined, and resumes the pending absence. A
fresh outer fact uses the retained local decision, while a reasserted data
fact runs the local callback again with the changed global. All-five-codec
tests additionally cover pending/completed checkpoints, reset, error blockers,
initial versus replacement existence support errors, and forged role/scope
metadata. Restore never invokes callbacks to rebuild
these historical decisions. Host handles are freshly queried after restore.

To regenerate only the current fixture reproducibly:

```sh
cargo test -p ferric-rules-runtime --features serde --lib \
  serialization::tests::regenerate_schema_five_fixture -- --ignored --exact
```

The ignored maintenance test writes `schema-5.cbor` and asserts the checkpoint
state before writing. Ordinary test runs never regenerate committed fixtures.
An incompatible future layout requires a new envelope version or an explicit
migration; retain prior fixtures to test the declared compatibility decision.

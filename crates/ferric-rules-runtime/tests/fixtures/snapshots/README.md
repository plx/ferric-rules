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

Schema 2 rejects these unchanged schema-1 bytes with `UnsupportedVersion(1)`.
The source still exercises resume/reset behavior after a current-format roundtrip.
Keep this prior fixture: replacing it would conceal an incompatible semantic change.

`schema-2.cbor` contains the explicit field-count tests required by ordered
patterns. Its source is `schema-2.clp`: construct with `Engine::with_rules`, run
exactly one firing, and serialize with CBOR. Two one-field facts match `(row ?)`;
a two-field fact does not. One valid activation remains pending at the checkpoint.
The committed-byte regression resumes it, asserts shorter and longer facts,
asserts a valid fact, installs another rule against the restored facts, then
resets and checks the named seeds. The same scenario runs through all codecs.

Regenerate only after an intentional schema change with:

```sh
cargo test -p ferric-rules-runtime --features serde regenerate_schema_two_cardinality_fixture -- --ignored
```

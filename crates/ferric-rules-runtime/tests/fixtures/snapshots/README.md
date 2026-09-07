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

Schema 4 rejects these unchanged schema-1 bytes with `UnsupportedVersion(1)`.
The source still exercises resume/reset behavior after a current-format roundtrip.
Keep this prior fixture: replacing it would conceal an incompatible layout change.

`schema-2.cbor` introduced ordered field-count guards. Its source is
`schema-2.clp`: construct with `Engine::with_rules`, run exactly one firing,
then serialize with CBOR. One fixed-width match has fired and another remains
pending. Schema 4 rejects these unchanged bytes with `UnsupportedVersion(2)`.
The source still exercises cardinality after resume, new assertions, later rule
loading, and reset through every current codec. Keep these bytes alongside the
schema-1 fixture; only the current schema fixture should be regenerated.

`schema-3.cbor` stores real ordered multifield match plans and token capture
lengths. Its source is `schema-3.clp`: construct with `Engine::with_rules`, run
exactly one firing, and serialize with CBOR. One of three splits of `(row a b)`
has fired; two activations remain pending. Schema 4 rejects these unchanged bytes
with `UnsupportedVersion(3)`. Current-codec tests still resume those activations,
check captured widths/refraction, retract the source fact, assert a replacement,
install a rule sharing the restored sequence join, and verify reset behavior.

`schema-4.cbor` stores independent sequence segments for a template's scalar
slot and two multislots. Its source is `schema-4.clp`: construct with
`Engine::with_rules`, run exactly one firing, and serialize with CBOR. Each
multislot has two captures, producing six matches of one fact; five remain
pending. The committed-byte regression checks every capture-width combination,
resume/refraction, source-fact retraction, replacement facts, later rule sharing,
and reset. All codecs run the same scenario.

Regenerate only after an intentional schema change with:

```sh
cargo test -p ferric-rules-runtime --features serde regenerate_schema_four_fixture -- --ignored
```

Malformed capture identities, projection bindings, logical selectors, template
source slots, and cache plans are rejected.

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

The source regression resumes item 1, retracts item 2's blocker and resumes it,
checks exact output/global/final facts and quiescence, then resets and reruns
the named seeds using current snapshots. The sealed version-one bytes are now
an explicit rejection control. An incompatible layout needs a new envelope
version or an explicit migration; old fixtures are never silently replaced.

## Schema 6: byte lexemes and typed instance names

Schema 6 is the current layout. Versions 2–5 are reserved by other incompatible
compatibility branches; this prerequisite uses a distinct version. All versions
1–5 are rejected before decoding payloads. The sealed `schema-1.cbor` and
`legacy-raw.cbor` bytes remain unchanged as explicit rejection controls; the
schema-1 source still exercises its behavior through newly produced snapshots.

`schema-6.clp` supplies a typed INSTANCE-NAME template, a `[seed]` literal and
one pending capture rule. The generator additionally asserts one payload with
STRING bytes `61 00 ff 7a`, SYMBOL bytes `73 ff` and INSTANCE-NAME bytes `6e ff`.
It runs one activation, leaving `TRUE\n` in the router. Restore runs
the remaining capture, checks exact raw output and three distinct global value
types, retracts the host fact and resets/replays the source seed. The same source
protocol is exercised with every codec.

Regenerate this unpublished layout deliberately with:

```sh
cargo test -p ferric-rules-runtime --all-features --lib \
  serialization::tests::generate_schema_six_fixture -- --ignored --exact
```

The schema now includes the interned byte pool, distinct INSTANCE-NAME values,
byte-string representation, and byte router/event payloads. Text access is
checked; snapshots do not substitute escaped or replacement text for bytes.

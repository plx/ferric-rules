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

The current schema rejects these unchanged bytes with `UnsupportedVersion(1)`. The source
still exercises resume/reset behavior after a current-format roundtrip. Keep
this fixture: replacing it would conceal an incompatible layout change.

`schema-2.cbor` preserves the previous format. It added ordered field-count guards,
sequence (multifield) match plans, and each token's capture lengths. Its source
is `schema-2.clp`: construct with `Engine::with_rules`, run exactly one firing,
then serialize with CBOR. `(row a b)` has three splits and the `bag` fact six
slot-segment splits; one of the nine has fired. Schema 3 rejects these unchanged
bytes with `UnsupportedVersion(2)`: schema 2 could store field-level disjunctions
as separate rule variants with incorrect matching and firing behavior. Keep
both its source and bytes as a compatibility boundary regression.

`schema-3.cbor` preserves the previous format. Its source, `schema-3.clp`, retains the
same split checkpoint and adds dormant scalar and sequence rules containing
`~x|y` field constraints. These store disjunctive predicates in the compiled
graph. Construct with `Engine::with_rules`, run exactly one firing, then
serialize with CBOR. The committed-byte regression resumes the other eight
splits and checks that together they cover every split once, retracts and
replaces facts, installs rules sharing the restored joins, and resets. It then
asserts facts that exercise both disjunctions, verifies overlapping alternatives
fire once per matching field, replaces a supporting fact, and installs rules
sharing the restored disjunctive paths. Schema 4 rejects these unchanged bytes
with `UnsupportedVersion(3)`: schema 3 stored seed facts computed at load time,
without the field initializers needed for correct reset-time evaluation.

`schema-4.cbor` preserves the previous format. Its source, `schema-4.clp`, keeps the
same pending split and disjunction checkpoint, but supplies ordered and
template seed fields through expressions. An additional ordered seed reads a
global in an arithmetic expression. Construct with `Engine::with_rules`, run
one firing, then serialize with CBOR. The committed-byte test checks the
existing resume behavior, then separately replaces the seed function after
restoring the fixture and verifies that reset uses its new values. Keep all
older fixture bytes unchanged. Schema 5 rejects this fixture with
`UnsupportedVersion(4)` because method restrictions now include query
expressions and wildcard types.

`schema-5.cbor` records generic method queries and is explicitly rejected by
the current runtime. Its source, `schema-5.clp`, extends the
schema-4 checkpoint with a generic whose fixed parameter has a query and whose
wildcard has both type and query restrictions. The committed-byte test resumes
pending matches, exercises reset-time initializers after callable replacement,
and checks method selection and wildcard bindings after restoration.
`schema-6.cbor` records typed fact addresses and is explicitly rejected by
the current runtime. Its source, `schema-6.clp`, adds a typed
fact address captured by the first firing. Restoration preserves that address,
eight pending split matches, deferred seed expressions, and method restrictions.
The previous fixture bytes remain unchanged.

`schema-7.cbor` preserves the previous format. `schema-7.clp` preserves a dormant
assertion expression in its seed initializer and adds a function, method, and
rule that exercise mutation and action-query return values after restoration.
Separate round-trip tests preserve fact numbering and refraction after source
clear refuses construct removal. Schema 8 rejects these unchanged bytes with
`UnsupportedVersion(7)` because template constraints and deferred dynamic defaults
add persisted metadata.

`schema-8.cbor` is the current format. Its source, `schema-8.clp`, retains the
same one-fired/eight-pending checkpoint and adds a constrained template with
allowed symbols, a numeric range, multislot cardinality, a computed static
default, and a dynamic default. The latter increments a counter when an omitted
slot is asserted. The resume test checks that restoration does not increment
that counter, that supplied slots skip the dynamic call, that static values
survive callable replacement, and that later dynamic calls use the replacement
function. Invalid assertions continue to fail after restoration. All older
fixture bytes remain unchanged.

Regenerate schema 8 only after an intentional change to its unreleased layout:

```sh
cargo test -p ferric-rules-runtime --features serde regenerate_schema_eight_fixture -- --ignored
```

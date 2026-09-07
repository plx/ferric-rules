# Host values and fact handles

The pre-1.0 host API separates values supplied by applications from the local
keys used inside RETE. Use names to create facts, `engine.symbol_value` for
symbols, and returned `FactHandle`s for lookup/retraction.

```rust
use ferric_rules_runtime::{Engine, EngineConfig, HostValue};
let mut engine = Engine::new(EngineConfig::default());
let ready = engine.symbol_value("ready")?;
let id = engine.assert_ordered("candidate", vec![HostValue::from(42_i64), ready])?;
let copy = engine.get_fact_owned(id)?.unwrap();
engine.retract(id)?;
let replacement = engine.assert(copy)?;
# Ok::<(), ferric_rules_runtime::EngineError>(())
```

Primitive integers, floats and strings remain ordinary inputs. A raw core
`Value::Symbol` has no engine provenance and is rejected, including inside a
multifield. `HostValue::multifield` retains its elements' ownership and rejects
mixed origins. Nested void values and invalid string encodings are rejected
before assertion. Host values allow 32 multifield levels and one million total
values per assertion. Opaque external tokens remain valid in memory and are
explicitly rejected by snapshot serialization.

`SymbolHandle` and `HostValue` can move between threads along with their engine.
Symbols survive `reset`. They are invalid after `clear` or in a separately
restored engine. Clone an owned fact value with `HostFact::value` when it needs
to escape the fact's borrow; ordinary `facts`/`get_fact` queries continue borrowing
stored facts without cloning their values. An owned template fact is rejected
if its template's name, slot meanings or constraints changed after capture.

`FactHandle::as_raw` and `from_raw` serve numeric embedding APIs. Numbers are
process-unique unsigned 64-bit handles (including values above signed 64-bit and
JavaScript safe-integer ranges), stable while a fact lives in one engine, and checked by that
engine. A foreign or stale handle cannot select a numerically colliding RETE
slot. Retraction removes its mapping; reset clears fact handles; clear and
restoration establish fresh host identities. Snapshot facts and subsequent rule
behavior survive, but cached host handles do not. Put durable application IDs
in fact fields and query them after restoration.

This is a deliberate API break: `Engine::assert` accepts an owned `HostFact`,
not an arbitrary core `Fact`; use `assert_ordered` or `assert_template_slots`
for new facts. Use `()` for empty ordered fields. Template slot/value pairs
avoid mismatched parallel arrays, and duplicate slots are rejected. Scalar
values for multislots become one-element multifields. Ordered assertion and
owned ordered reassertion reject names belonging to explicit templates. The
current ordered relation identity is global, so this guard also covers private
and module-qualified templates by local name; use the template assertion API.

The `core` crate and borrowed RETE inspection remain low-level facilities.
`resolve_core_symbol` is an explicit adapter for a raw key obtained from the
same engine's borrowed state. Core keys alone are not portable input values.
Ordinary symbol lookup uses `resolve_symbol(SymbolHandle)` and checks ownership.

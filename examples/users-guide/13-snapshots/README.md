# 13 — Snapshots and warm starts

Companion code for [`docs/users-guide.md` §14](../../../docs/users-guide.md#14-snapshots-and-warm-starts).

Run from this directory:

```sh
just check-example
```

What it shows:

- `Engine::serialize(SerializationFormat::Bincode)` produces a byte stream.
- `Engine::deserialize(&bytes, SerializationFormat::Bincode)` thaws it
  without re-parsing or recompiling the rules.
- The thawed engine still has the global `?*threshold*` and the rule
  ready to fire.

This example requires the `serde` Cargo feature on the facade crate; it is
already enabled in `Cargo.toml`.

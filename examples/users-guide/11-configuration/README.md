# 11 — Configuration

Companion code for [`docs/users-guide.md` §12](../../../docs/users-guide.md#12-configuration).

Run from this directory:

```sh
just check-example
```

What it shows:

- `EngineConfig::default()`, `::ascii()`, and `::utf8()` factories.
- `with_strategy(ConflictResolutionStrategy::Lex)` builder method.
- Manually setting `max_call_depth`.
- `Engine::with_rules_config(source, config)` for combining config and load.

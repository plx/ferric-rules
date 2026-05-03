# 09 — Driving the engine from Rust

Companion code for [`docs/users-guide.md` §10](../../../docs/users-guide.md#10-driving-the-engine-from-rust).

Run from this directory:

```sh
just check-example
```

What it shows:

- A `Classifier` struct that owns one `Engine` and reuses it across requests.
- `engine.reset()` between requests, plus `clear_output_channel` and
  `clear_action_diagnostics` to scope captured state per decision.
- A bounded run with `RunLimit::Count(1_000)`.
- Pulling the decision back out of working memory with `find_facts`.

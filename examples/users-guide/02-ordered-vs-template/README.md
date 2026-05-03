# 02 — Ordered facts vs. template facts

Companion code for [`docs/users-guide.md` §3](../../../docs/users-guide.md#3-ordered-facts-vs-template-facts).

Run from this directory:

```sh
just check-example
```

What it shows:

- A `deftemplate` with `slot`/`multislot` and a `default` for `age`.
- `assert_template(name, slot_names, values)` for building template facts from Rust.
- A partial pattern that only matches the slots the rule cares about.

# 14 — A larger worked example: phased reading pipeline

Companion code for [`docs/users-guide.md` §15](../../../docs/users-guide.md#15-a-larger-worked-example-phased-reading-pipeline).

Run from this directory:

```sh
just check-example
```

What it shows:

- A `deftemplate` (`reading`) with `slot id / kind / value`, populated from
  Rust via `assert_template`.
- A `defglobal` (`?*scale*`) used as a tuning knob inside `modify`.
- A `deffunction` (`f-to-c`) called from a rule's RHS.
- Two-phase processing controlled by salience: Fahrenheit readings are
  rewritten to Celsius (`normalize`, salience 100), then each Celsius
  reading is classified (`overheat` / `nominal`, salience 50).
- Diagnoses come out as ordered facts so the host can read them via
  `find_facts("diagnosis")`.

> **Note:** an earlier version split `NORMALIZE` and `DIAGNOSE` into separate
> modules. That shape currently misses the second phase because `NORMALIZE`
> modifies the facts that `DIAGNOSE` should consume. Salience-ordered phases
> inside one module produce the same result and work reliably today.

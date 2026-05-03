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

> **Note:** the original prose for this section described an architecture
> where `NORMALIZE` and `DIAGNOSE` were separate modules and `MAIN::go`
> pushed them onto the focus stack. ferric does not currently re-evaluate
> a module's agenda for facts created by rules in another module, so the
> focus-stack version of the example would not chain. Salience-ordered
> phases inside a single module produce the same end result and work
> reliably today.

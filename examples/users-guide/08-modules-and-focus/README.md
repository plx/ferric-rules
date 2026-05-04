# 08 — Modules and focus stacks

Companion code for [`docs/users-guide.md` §9](../../../docs/users-guide.md#9-modules-and-focus-stacks).

Run from this directory:

```sh
just check-example
```

What it shows:

- Two modules (`SENSORS`, `ALERTS`) export and import a shared `reading`
  template, then each define a rule that matches it.
- The host pre-asserts one `(reading ...)` fact and uses
  `push_focus` to put ALERTS on the bottom of the stack and SENSORS on
  top. Run drains SENSORS first, then continues with ALERTS.
- Facts are global — both modules see the same fact — but rule eligibility
  is module-scoped.

> **Note:** pre-asserted facts work well with focus stacks. Chained
> cross-module phases are more limited: facts asserted or modified while one
> focused module is running do not reliably create activations for a later
> focused module. For those pipelines, assert from the host between focus
> changes or keep the chain in one module and order phases with salience.

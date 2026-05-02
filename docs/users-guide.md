# Ferric User's Guide

This guide shows how to embed **ferric-rules** in a Rust application, starting
from a minimal rule set and building up through the CLIPS features you are
likely to reach for: templates, negation and existential patterns, salience,
`deffunction` and `defgeneric`, `defglobal`, and multi-module rule packs with
a focus stack. Each section shows both the CLIPS-side definition and the Rust
code that drives it.

This document is a practical guide, not a reference. For the full surface see:

- [`compatibility.md`](compatibility.md) — CLIPS feature compatibility matrix.
- [`migration.md`](migration.md) — CLIPS → ferric-rules migration notes.
- [`project-overview.md`](project-overview.md) — where things live in the
  workspace.

---

## 1. How ferric-rules fits in your app

A ferric-rules `Engine` is a self-contained forward-chaining production
system. You load CLIPS source into it once, and at runtime:

1. **Assert** facts that describe the current situation (user state, sensor
   readings, parsed requests, whatever your domain calls for).
2. **Run** the engine. It matches the asserted facts against the rules you
   loaded, fires them in priority order, and records their effects: new
   facts, modified facts, printed output, updated globals, module focus
   changes.
3. **Read** the results — either by pulling facts back out of working memory
   or by reading captured `printout` channels.

Each `Engine` is `!Send + !Sync`: it lives on a single thread. Create one per
decision context (per session, per request, per worker) or reset and reuse.

The facade crate re-exports everything you need:

```toml
# Cargo.toml
[dependencies]
ferric = "0.1"
```

```rust
use ferric::runtime::{Engine, EngineConfig, RunLimit};
```

If you need engine serialization (see §13), turn on the `serde` feature:

```toml
ferric = { version = "0.1", features = ["serde"] }
```

---

## 2. A minimal embedding

Here is the smallest useful program: one rule, one fact, one printout.

```rust
use ferric::runtime::{Engine, RunLimit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::with_rules(r#"
        (defrule greet
            (user ?name)
            =>
            (printout t "Hello, " ?name "!" crlf))
    "#)?;

    engine.assert_ordered_symbol("user", "Alice")?;
    engine.run(RunLimit::Unlimited)?;

    assert_eq!(engine.get_output("t"), Some("Hello, Alice!\n"));
    Ok(())
}
```

A few things are worth noticing:

- `Engine::with_rules` is the one-line constructor: it parses the source,
  compiles it into the Rete network, and returns an engine ready to use.
  It applies `(reset)`, so `(initial-fact)` and any `deffacts` groups are
  already asserted by the time it returns.
- `assert_ordered_symbol("user", "Alice")` is a convenience for the common
  "relation with a single symbol field" case. The longer form,
  `engine.assert_ordered("user", fields)`, takes anything that converts
  into a field list: a `Vec<Value>`, a single primitive (`i64`, `f64`), a
  `Symbol`, or a `FerricString`.
- `RunLimit::Unlimited` runs until the agenda drains. `RunLimit::Count(n)`
  caps the run at `n` firings — useful when you need bounded execution in
  a request handler.
- `get_output("t")` returns whatever rules wrote to the standard output
  channel (`t` in CLIPS). If nothing was written, it returns `None`.

---

## 3. Ordered facts vs. template facts

CLIPS has two kinds of facts. Ferric supports both.

**Ordered facts** are positional. They are the right choice for transient
flags, simple tuples, and anything where the shape is stable and the name
makes the meaning obvious:

```clips
(color red)
(reading temperature 72.4)
(pair 1 2 3)
```

**Template facts** have named slots. Use them when the shape is more than a
couple of fields, when fields have defaults, or when you expect to `modify`
them later:

```clips
(deftemplate person
    (slot name)
    (slot age (default 0))
    (multislot hobbies))

(assert (person (name Alice) (age 30) (hobbies chess hiking)))
```

From Rust, template facts can be built by name:

```rust
use ferric::core::Value;
use ferric::runtime::Engine;

fn assert_person(engine: &mut Engine, name: &str, age: i64) -> anyhow::Result<()> {
    let name_sym = engine.symbol_value(name)?;
    engine.assert_template(
        "person",
        &["name", "age"],
        vec![name_sym, Value::Integer(age)],
    )?;
    Ok(())
}
```

Unspecified slots pick up their declared defaults (or an empty multifield
for `multislot`). The `person` template above will assert with
`(age 0)` if you leave `age` out.

Partial patterns let rules match on just the slots they care about, which
is usually what you want:

```clips
(defrule adult
    (person (name ?n) (age ?a))
    (test (>= ?a 18))
    =>
    (printout t ?n " is an adult" crlf))
```

---

## 4. Priority and mutual exclusion: salience + guard facts

Salience is the primary knob for rule priority. Higher salience wins, and
within a salience tier the active conflict-resolution strategy decides
(Depth by default — most recent activations fire first).

A common idiom is *priority-ordered suppression*: you have several
candidate actions, you want exactly one, and the rules that "win" assert a
guard fact that disqualifies the others. The mobile-engagement ruleset in
the README is a worked example of this pattern. A small version:

```clips
(defrule alarm-on-fire
    (declare (salience 100))
    (sensor smoke high)
    (not (decision-made))
    =>
    (assert (alert evacuate))
    (assert (decision-made)))

(defrule warn-on-heat
    (declare (salience 50))
    (sensor temperature ?t)
    (test (> ?t 90))
    (not (decision-made))
    =>
    (assert (alert high-temp))
    (assert (decision-made)))

(defrule monitor
    (declare (salience 10))
    (not (decision-made))
    =>
    (assert (alert none))
    (assert (decision-made)))
```

From the host side you assert your sensor readings, run, and inspect
working memory for the `(alert ...)` fact:

```rust
use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

fn classify(
    engine: &mut Engine,
    smoke: &str,
    temperature: f64,
) -> anyhow::Result<Option<String>> {
    engine.reset()?;

    let smoke_kind = engine.symbol_value("smoke")?;
    let smoke_level = engine.symbol_value(smoke)?;
    engine.assert_ordered("sensor", vec![smoke_kind, smoke_level])?;

    let temp_kind = engine.symbol_value("temperature")?;
    engine.assert_ordered("sensor", vec![temp_kind, Value::Float(temperature)])?;

    engine.run(RunLimit::Unlimited)?;

    for (_, fact) in engine.find_facts("alert")? {
        if let ferric::core::Fact::Ordered(of) = fact {
            if let Some(Value::Symbol(sym)) = of.fields.first() {
                if let Some(name) = engine.resolve_symbol(*sym) {
                    return Ok(Some(name.to_string()));
                }
            }
        }
    }
    Ok(None)
}
```

Two points about this shape:

- `engine.reset()` is the usual "clean slate" between decisions. It retracts
  user facts, re-asserts `(initial-fact)` and any `deffacts` groups, and
  restores globals to their declared values.
- `find_facts("alert")` is a read-only lookup by relation name. It does not
  intern the symbol, so a missing relation just returns an empty vector.

---

## 5. Negation, existentials, and the pattern menagerie

Ferric supports the CLIPS pattern connectives you are likely to need:
`not`, `exists`, `forall`, NCC (negated conjunction via `(not (and ...))`),
and constraint connectives (`&`, `|`, `~`) inside patterns. A compact tour:

```clips
(deftemplate task (slot id) (slot done (default FALSE)))

;; not — fires when nothing matches
(defrule report-no-tasks
    (start)
    (not (task))
    =>
    (printout t "queue is empty" crlf))

;; exists — fires once when at least one match exists
(defrule have-work
    (exists (task (done FALSE)))
    =>
    (printout t "work pending" crlf))

;; forall — fires when every task is done (vacuously true when none exist)
(defrule all-complete
    (ready)
    (forall (task (id ?i)) (task (id ?i) (done TRUE)))
    =>
    (printout t "everything done" crlf))

;; NCC — negate a conjunction
(defrule no-overdue-critical
    (now ?t)
    (not (and (task (id ?i)) (deadline ?i ?d) (test (< ?d ?t))))
    =>
    (printout t "no overdue critical items" crlf))

;; constraint connectives inside a pattern
(defrule escalate
    (ticket (severity ~low) (status open|in-progress))
    =>
    (printout t "needs attention" crlf))
```

A few rules of thumb:

- `forall (P) (Q)` is "for every P, some Q." If there are no P facts, it is
  vacuously true. That usually matches what you want for "all tasks
  complete," but read the logic carefully before wiring it up.
- Pattern nesting is single-level. Triple negations, `(exists (not ...))`,
  and nested `forall` are rejected by the compiler. The typical workaround
  is a helper rule that asserts an intermediate "flag" fact
  (see [`migration.md`](migration.md) §3).

---

## 6. RHS actions: modify, retract, duplicate, bind

Rule right-hand sides can assert, retract, modify, and duplicate facts, and
`bind` local variables or globals. Fact-address variables (`?f <- (pattern)`)
capture a fact's identity so you can hand it to `retract` or `modify`:

```clips
(deftemplate counter (slot value (default 0)))

(defrule increment
    ?c <- (counter (value ?v))
    ?t <- (tick)
    =>
    (modify ?c (value (+ ?v 1)))
    (retract ?t))
```

`modify` rewrites slots in place on a template fact. `retract` removes a
fact by address. `duplicate` creates a copy with slot overrides.

For control flow inside the RHS, ferric supports the action-level forms:
`if/then/else`, `while/do`, `loop-for-count`, `progn$`/`foreach`, and
`switch/case/default`. Function bodies themselves are pure expressions — if
you want side effects (printing, asserting), keep them in the rule's RHS.

---

## 7. User functions and generics

`deffunction` lets you factor a computation out of your rules:

```clips
(deffunction celsius-to-fahrenheit (?c)
    (+ (* ?c 1.8) 32))

(defrule report-temp
    (reading celsius ?c)
    =>
    (printout t ?c "C = " (celsius-to-fahrenheit ?c) "F" crlf))
```

`defgeneric` + `defmethod` give you type-dispatched generics with a
most-specific-wins rule. Methods can call `(call-next-method)` to chain to
the less-specific method:

```clips
(defgeneric describe)

(defmethod describe ((?x NUMBER))
    (str-cat "number(" ?x ")"))

(defmethod describe ((?x INTEGER))
    (str-cat "int/" (call-next-method)))

(defrule show
    (value ?v)
    =>
    (printout t (describe ?v) crlf))
```

`(describe 7)` chains through the INTEGER method into NUMBER and yields
`"int/number(7)"`; `(describe 3.14)` skips straight to NUMBER and yields
`"number(3.14)"`.

Both `deffunction` and `defmethod` bodies are **pure expressions** — no
`assert`/`retract`/`printout` inside. If your CLIPS code relies on side
effects in functions, keep the computation in the function and move the
effect to the calling rule.

Accepted parameter types in `defmethod`: `INTEGER`, `FLOAT`, `NUMBER`,
`SYMBOL`, `STRING`, `LEXEME`, `MULTIFIELD`, or unrestricted `((?x))`.

---

## 8. Globals

`defglobal` declares named mutable state. Globals have `?*name*` syntax and
must be updated with `(bind ?*name* ...)` (bind never *creates* globals; it
only updates them).

```clips
(defglobal ?*session-count* = 0)

(defrule count-session
    (session-start)
    =>
    (bind ?*session-count* (+ ?*session-count* 1))
    (printout t "session " ?*session-count* crlf))
```

From the host side:

```rust
use ferric::core::Value;

if let Some(Value::Integer(n)) = engine.get_global("*session-count*") {
    println!("engine has counted {n} sessions");
}
```

Note the leading/trailing `*` — globals are named `*session-count*` in
lookups. Globals reset to their declared initial values on `(reset)`.

---

## 9. Modules and focus stacks

Non-trivial rule sets benefit from being split into modules with explicit
focus control. Modules declare what they export; other modules import what
they need; facts remain global but only rules in the currently focused
module are eligible to fire.

```clips
;;; -- SENSORS: ingest raw readings, publish derived facts
(defmodule SENSORS
    (export deftemplate reading)
    (export deftemplate derived))

(deftemplate SENSORS::reading (slot kind) (slot value))
(deftemplate SENSORS::derived (slot kind) (slot value))

(defrule SENSORS::to-celsius
    (reading (kind fahrenheit) (value ?f))
    =>
    (assert (derived (kind celsius) (value (/ (- ?f 32) 1.8)))))

;;; -- ALERTS: consume derived facts, decide
(defmodule ALERTS
    (import SENSORS deftemplate ?ALL))

(defrule ALERTS::overheat
    (derived (kind celsius) (value ?c))
    (test (> ?c 40))
    =>
    (assert (alert overheat ?c)))

;;; -- MAIN: coordinate
(defrule MAIN::go
    (begin)
    =>
    (focus ALERTS)
    (focus SENSORS))
```

The `focus` action pushes modules onto a LIFO stack. In the example above,
`MAIN::go` pushes `ALERTS` first and `SENSORS` second, so `SENSORS` fires
first (top of stack), then `ALERTS` once `SENSORS` drains, then finally
`MAIN` resumes. You can also push focus from Rust:

```rust
engine.push_focus("SENSORS")?;   // top of stack
engine.push_focus("ALERTS")?;    // now top; runs first
engine.run(RunLimit::Unlimited)?;

println!("finished in module {:?}", engine.get_focus());
```

Focus-based decomposition is the recommended way to enforce phases in a
reasoning process — sensor fusion first, then business rules, then alert
generation, for example.

---

## 10. Driving the engine from Rust

A typical request-handler shape looks like this:

```rust
use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

pub struct Classifier {
    engine: Engine,
}

impl Classifier {
    pub fn new(rules: &str) -> anyhow::Result<Self> {
        Ok(Self { engine: Engine::with_rules(rules)? })
    }

    pub fn classify(&mut self, req: &Request) -> anyhow::Result<Decision> {
        // Start from a clean slate: reset reapplies deffacts + initial-fact.
        self.engine.reset()?;

        // Project the request onto facts. Note that `symbol_value` and
        // `assert_ordered` both borrow the engine mutably, so bind first.
        let tier = self.engine.symbol_value(&req.tier)?;
        self.engine.assert_ordered("user-tier", tier)?;
        self.engine.assert_ordered("session-count", req.session_count)?;
        if req.has_crashed {
            self.engine.assert_ordered_symbol("has-crashed", "yes")?;
        }

        // Bounded run — production handlers should always cap iterations.
        let result = self.engine.run(RunLimit::Count(1_000))?;

        // Pick up the decision either from a fact or a printout channel.
        let decision = self.read_decision()?;

        // Surface non-fatal rule warnings if you want to log them.
        for diag in self.engine.action_diagnostics() {
            tracing::warn!(?diag, "action diagnostic");
        }
        self.engine.clear_action_diagnostics();
        self.engine.clear_output_channel("t");

        Ok(decision)
    }

    fn read_decision(&self) -> anyhow::Result<Decision> {
        for (_, fact) in self.engine.find_facts("decision")? {
            // ... pull the slot/field you care about ...
        }
        Ok(Decision::default())
    }
}
```

A few useful patterns:

- **Reset + reuse beats rebuild.** Rule compilation is a one-time cost.
  Keep the engine, call `reset()` between decisions. `clear()` is the
  heavier "forget everything including user-registered constructs" hammer;
  use sparingly.
- **Prefer facts over printout for decisions.** `printout` is great for
  logging and debugging, but machine-readable output should come from
  facts you can inspect with `find_facts` or `facts()`.
- **Cap the run.** Even a correct rule set can loop under pathological
  inputs; `RunLimit::Count(n)` is cheap insurance. Check the
  `HaltReason` in the returned `RunResult` to distinguish normal
  completion from reaching the cap.
- **Step when you need to observe.** For debugging or when you want to
  interleave rule firing with external I/O, `engine.step()` fires exactly
  one activation and returns it.

---

## 11. Input and output channels

`printout t "..."` writes to a channel called `t`. Ferric captures channel
output in an in-memory buffer; read it back with `get_output(name)` and
clear it with `clear_output_channel(name)`. You can write to any channel
name — `t` is the conventional "standard output," but using a dedicated
channel per concern (`audit`, `trace`, etc.) and reading them separately
is often cleaner.

For *input*, `(read)` and `(readline)` consume from an engine-managed
input buffer. Push lines from Rust before the run:

```rust
engine.push_input("42");
engine.push_input("hello world");
engine.run(RunLimit::Unlimited)?;
```

`format` works as an evaluator-only function in ferric: it returns a
string. To actually print it, pipe it through `printout`:

```clips
(printout t (format nil "n=%d" 42) crlf)
```

---

## 12. Configuration

`EngineConfig` controls string encoding, conflict resolution strategy,
and the user-function recursion limit. The factory helpers cover the
common cases:

```rust
use ferric::core::ConflictResolutionStrategy;
use ferric::runtime::{Engine, EngineConfig};

// UTF-8 symbols and strings, Depth strategy, 64-frame recursion limit.
let engine = Engine::new(EngineConfig::default());

// CLIPS-strict ASCII mode with LEX strategy.
let engine = Engine::new(
    EngineConfig::ascii().with_strategy(ConflictResolutionStrategy::Lex),
);

// Increase recursion depth for deeply recursive deffunctions.
let mut cfg = EngineConfig::utf8();
cfg.max_call_depth = 256;
let engine = Engine::new(cfg);
```

Available strategies: `Depth` (default), `Breadth`, `Lex`, `Mea`.
`Simplicity`, `Complexity`, and `Random` are not implemented — they are
rarely needed in practice and their behavior is under-specified in the
CLIPS literature.

If you pass source via `Engine::with_rules_config(source, config)`,
configuration and rule loading happen in one call.

---

## 13. Error handling

Two categories of things can go wrong:

**Fatal errors** return `Err` from the fallible engine methods.
`Engine::with_rules` returns `InitError` on parse or compilation failure;
`assert_*`, `retract`, `run`, and friends return `EngineError` for
runtime problems (template not found, encoding violations, thread-affinity
violations, recursion-limit exceeded).

**Non-fatal action diagnostics** are warnings collected during rule
execution — for example, an unresolved module reference in a `focus`
action. They don't halt the run; they accumulate in the engine and you
can inspect them after `run` returns:

```rust
let result = engine.run(RunLimit::Unlimited)?;
for diag in engine.action_diagnostics() {
    tracing::warn!("rule warning: {diag}");
}
engine.clear_action_diagnostics();
```

Clear diagnostics between runs if you want them scoped per decision — they
are **not** cleared by `reset()`.

---

## 14. Snapshots and warm starts

With the `serde` feature enabled, you can freeze an engine to bytes and
thaw it later. This is useful for:

- **Warm starts.** Compile the rules and pre-populate `deffacts` once,
  serialize, then deserialize per worker/request to skip compilation.
- **Snapshots.** Capture a running engine's state for replay or audit.
- **Hot handoff.** Move an engine across processes or tasks that don't
  share memory.

```rust
use ferric::runtime::{Engine, SerializationFormat};

// Offline: compile once, save a baseline snapshot.
let engine = Engine::with_rules(include_str!("rules.clp"))?;
let bytes = engine.serialize(SerializationFormat::Bincode)?;
std::fs::write("rules.snapshot", &bytes)?;

// Online: fast path — no parsing, no compilation.
let bytes = std::fs::read("rules.snapshot")?;
let engine = Engine::deserialize(&bytes, SerializationFormat::Bincode)?;
```

Available formats: `Bincode` (default, compact), `Json` (human-readable),
`Cbor`, `MessagePack`, `Postcard`. The on-disk payload is prefixed with
an 8-byte header (`FRSE` + little-endian version) so a stale snapshot is
rejected cleanly instead of silently misinterpreted.

`ExternalAddress` values in working memory are rejected at serialize time
— they reference host pointers that can't meaningfully round-trip.

---

## 15. A larger worked example: phased sensor pipeline

Pulling the pieces together. A sketch of a small diagnosis pipeline that
uses templates, modules, focus, functions, and globals:

```clips
;;; ============================================================
;;; globals
;;; ============================================================
(defglobal ?*scale* = 1.0)

;;; ============================================================
;;; TEMPLATES module — shared shapes
;;; ============================================================
(defmodule TEMPLATES
    (export deftemplate reading)
    (export deftemplate diagnosis))

(deftemplate TEMPLATES::reading
    (slot id)
    (slot kind)
    (slot value))

(deftemplate TEMPLATES::diagnosis
    (slot id)
    (slot level (default info))
    (slot message))

;;; ============================================================
;;; NORMALIZE module — unit conversion
;;; ============================================================
(defmodule NORMALIZE (import TEMPLATES deftemplate ?ALL))

(deffunction f-to-c (?f) (/ (- ?f 32.0) 1.8))

(defrule NORMALIZE::scale
    ?r <- (reading (kind fahrenheit) (value ?v))
    =>
    (modify ?r (kind celsius) (value (* ?*scale* (f-to-c ?v)))))

;;; ============================================================
;;; DIAGNOSE module — the actual rules
;;; ============================================================
(defmodule DIAGNOSE (import TEMPLATES deftemplate ?ALL))

(defrule DIAGNOSE::overheat
    (reading (id ?i) (kind celsius) (value ?c))
    (test (> ?c 40))
    =>
    (assert (diagnosis (id ?i) (level alert) (message "overheat"))))

(defrule DIAGNOSE::nominal
    (reading (id ?i) (kind celsius) (value ?c))
    (test (<= ?c 40))
    (not (diagnosis (id ?i)))
    =>
    (assert (diagnosis (id ?i) (level info) (message "nominal"))))

;;; ============================================================
;;; MAIN — coordinate the passes
;;; ============================================================
(defrule MAIN::go
    (begin)
    =>
    (focus DIAGNOSE)
    (focus NORMALIZE))
```

Driving it from Rust:

```rust
use ferric::core::{Fact, Value};
use ferric::runtime::{Engine, RunLimit};

fn run(engine: &mut Engine, inputs: &[(i64, &str, f64)]) -> anyhow::Result<()> {
    engine.reset()?;
    engine.assert_ordered("begin", vec![])?;

    for (id, kind, value) in inputs {
        let kind_sym = engine.symbol_value(kind)?;
        engine.assert_template(
            "reading",
            &["id", "kind", "value"],
            vec![Value::Integer(*id), kind_sym, Value::Float(*value)],
        )?;
    }

    let result = engine.run(RunLimit::Count(10_000))?;
    tracing::info!(fired = result.rules_fired, "pipeline complete");

    for (_, fact) in engine.find_facts("diagnosis")? {
        if let Fact::Template(_) = fact {
            // extract slots via get_fact_slot_by_name, etc.
        }
    }
    Ok(())
}
```

What's on display here:

- `NORMALIZE` fires before `DIAGNOSE` because of the focus-stack order
  (`MAIN::go` pushes DIAGNOSE first, then NORMALIZE; NORMALIZE ends up on
  top and runs first).
- `f-to-c` is a `deffunction` — a pure expression used from a rule's RHS.
- `?*scale*` is a global that normalization uses as a tuning knob. Set
  it from CLIPS via `(bind ?*scale* 1.05)` or inspect it from Rust via
  `engine.get_global("*scale*")`.
- Templates live in a single module and are imported where needed, so
  facts are shareable but rule ownership is clear.

---

## 16. Tips, gotchas, and idioms

A non-exhaustive list worth internalizing:

- **`=` is numeric; `eq` is type-sensitive.** `(= 1 1.0)` is `TRUE`;
  `(eq 1 1.0)` is `FALSE`. Use `eq` for symbol/string compares.
- **`format` returns a string.** Wrap it in `printout` to actually write.
- **`run` from a rule RHS is a no-op.** Don't try to trigger another run
  mid-firing; use focus/salience instead.
- **`reset` and `clear` from RHS are deferred.** They set a flag that is
  checked between activations, not applied mid-action.
- **Activation order is total per run, not reproducible across runs.**
  Don't rely on two independent runs producing the same interleaving;
  encode precedence with salience or focus if order matters.
- **Prefer `find_facts` and `facts()` to `printout` for machine output.**
  Printouts are strings; facts have types.
- **`reset()` does not clear captured output or action diagnostics.**
  Clear those explicitly at the boundaries of a decision.
- **Templates are module-scoped.** Export them from the module that owns
  the shape; import them from modules that need them.
- **Symbols are interned per-engine.** Use `engine.symbol_value("foo")`
  when you need to build a `Value` with a specific symbol; `resolve_symbol`
  goes the other way.

---

## 17. Beyond Rust: other language bindings

Ferric's engine core is reachable from other languages via `ferric-ffi`
(C ABI) and the higher-level wrappers built on it:

- **C / C++ / Swift / Kotlin**: link against `libferric_ffi` and include
  the generated `ferric.h`. See [`compatibility.md`](compatibility.md)
  §16.13 for the C contract.
- **Go**: `bindings/go` provides an idiomatic façade (`Engine`,
  `Coordinator`, `Manager`) plus a Temporal activity wrapper.
- **Python**: `crates/ferric-python` ships a PyO3 extension module; build
  it with `maturin` and `import ferric` from Python.
- **CLI**: the `ferric` binary (`crates/ferric-cli`) runs `.clp` files
  batch-style or drops you into a REPL. `ferric check [--json] file.clp`
  validates without running; `ferric run` executes.

For anything not covered here, start with [`compatibility.md`](compatibility.md)
for "what's supported" and [`migration.md`](migration.md) for the differences
a CLIPS veteran would notice.

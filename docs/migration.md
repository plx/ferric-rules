# Migrating from CLIPS to Ferric

This guide covers practical steps for migrating existing CLIPS applications
to Ferric. For detailed feature compatibility, see [compatibility.md](compatibility.md).

---

## Pre-1.0 seed and reset changes

Loading `deffacts` now registers a named definition without asserting its facts.
Call `reset()` after loading startup rules and seeds, or use Rust
`Engine::with_rules`, which already loads and resets. A later load leaves
existing application facts unchanged until the next reset. `LoadResult` no
longer reports deffacts seeds as newly asserted facts. Explicit `assert` and
`load-facts` continue to add facts immediately.

Top-level `assert` now evaluates field expressions and globals, including
through string-assert binding APIs. Invalid fields cause an error rather than
being dropped. Deffacts retain their expressions and evaluate them at each
reset after global initialization; globals defined later and callable
replacements affect those values. Loading a deffacts definition has no expression side
effects. `load-facts` accepts literal data only.

A definition is identified by module and local name. Successful replacement
moves it to the end of that module's definition order; reset visits modules
in creation order, then their definitions in order. `undeffacts` removes
definitions without retracting current facts; its `*` selector applies only to
the current module. An invalid individual definition
leaves its previous definition intact. This atomic replacement is deliberately
stronger than CLIPS 6.30, which removes the old same-name definition before
reporting some replacement errors.

The internal `(initial-fact)` remains matchable by rules but is hidden from
host fact lookup/enumeration and protected against retract, modify, and
duplicate. Replacing `MAIN::initial-fact` with a user `deffacts` is rejected.
These restrictions differ from CLIPS; keep application bootstrap state in
ordinary named facts. Empty and leading-negative rule conditions use the
independent RETE root token. Reset establishes that root and the internal
initial fact before asserting application seeds.

Core-internal consumers of `AlphaMemory::lookup_by_slot` now receive an
iterator in insertion order instead of a borrowed hash set. Collect that
iterator when a materialized collection is needed. Public engine fact APIs
retain their existing result types.

## Pre-1.0 snapshot codec removal

The experimental bincode, MessagePack and Postcard snapshot codecs were removed
from every surface: Rust `SerializationFormat`, the CLI `--format` /
`--snapshot-format` values, the C ABI (`FERRIC_SERIALIZATION_FORMAT_BINCODE`,
`_MESSAGE_PACK`, `_POSTCARD` and the `ferric_engine_{serialize,deserialize}_{bincode,msgpack,postcard}`
exports), and the TypeScript, Python and Go `Format` enums. CBOR (the default)
and JSON remain, with unchanged numeric values (JSON `1`, CBOR `2`); the removed
values `0`, `3` and `4` are now rejected. To keep a snapshot written with a
removed codec, restore it with the producing version and re-save it as CBOR.

## Pre-1.0 instance names

`Value` has a new `InstanceName` variant for CLIPS instance names such as
`[widget]` (Ferric still has no object system), and the parser's
`LiteralKind`/`Atom` and `SlotValueType` gained matching variants. Exhaustive
matches on these enums need a new arm. The C ABI adds value type
`FERRIC_VALUE_TYPE_INSTANCE_NAME = 7` (a `string_ptr` holding the spelling
without brackets) and `ferric_value_instance_name_bytes`; C and Go hosts that
switch on `value_type` should handle it. The bindings return instance names as
`InstanceName` values (`FerricInstanceName` in TypeScript,
`Value.instanceName` in Swift). Earlier versions returned `[widget]` as a
SYMBOL spelled with its brackets, so a host that compares with that symbol
(`Symbol("[widget]")`, `FerricSymbol("[widget]")`, `.symbol("[widget]")`) no
longer matches and should compare with the instance-name value instead.

In the parser, the lexer has a new `Token::InstanceName`, and
`SlotConstraint::constraint` is now `constraints: Vec<Constraint>`, because a
multislot pattern holds a sequence of field constraints. In the core,
`CompilablePattern` has a new `sequence` field, so struct literals need it
(`sequence: None` for a pattern without multifield fields).

## Pre-1.0 procedural and callable changes

Anonymous count loops now use CLIPS syntax: replace `(loop-for-count (5) ...)`
with `(loop-for-count 5 ...)`. Empty callable bodies, unmatched `if`/`switch`
branches, and completed `while`/`loop-for-count` expressions return `FALSE`.
Wildcard parameters flatten excess multifield arguments; fixed parameters
preserve multifields as single arguments. Unknown calls in function and method
bodies now fail during loading, including calls in branches that never execute.
Forward references within a load remain supported.

Parser `MethodParameter` struct literals need a `query` field. `MethodConstruct`
and runtime `RegisteredMethod` also store wildcard type/query restrictions;
`RegisteredMethod` stores one optional query per fixed parameter. Use `None`
and empty restriction vectors when there are no queries or wildcard types.
The existing `GenericRegistry::register_method` API retains its signature;
`register_restricted_method` accepts the additional restrictions.

## Expression effects and source lifecycle

Fact mutation, `halt`, `focus`, `reset`, `clear`, and action queries now work
inside expressions, deffunctions, and methods. `assert`, `modify`, and
`duplicate` return typed fact addresses or `FALSE` when insertion is
suppressed. `modify` replaces the original assertion even when its values do
not change. Missing mutation targets return `FALSE` for modify/duplicate
and are ignored by retract. Effects completed before a later evaluation error
remain visible.

Source `(reset)` now executes immediately, preserves output and active local
bindings, and continues the current RHS or callable. It also allows the run to
select new activations. A nested reset inside reset-time initialization is
ignored. Source `(clear)` removes facts and restarts public fact numbering at
zero, then preserves constructs because they are in active use. It continues
execution instead of stopping the run. Public host `reset()` and `clear()`
retain their output-clearing and construct-removal contracts.

Action queries return their last body value, `FALSE` when no body runs, or no
value after `break`. Ordinary action queries stop selecting members after a
reset; delayed queries retain their captured tuples and slot values. Old
addresses remain stale. Mutation during rule match conditions remains
unsupported, including through called functions. Modify/duplicate of already
stale addresses cannot recover their old fact contents; retain field values
explicitly when constructing a replacement.

## Typed fact addresses

`Value` and `AtomKey` have a new `FactAddress` variant. Exhaustive matches must
handle it. Rule variables bound to facts and fact-query results now carry this
type instead of integers containing arena keys. It prints `<Fact-N>` using the
public index retained at assertion time; FACT-ADDRESS slot defaults print
`<Dummy Fact>`. Integer fact designators always mean public indices.

Addresses are neither INTEGER nor NUMBER. Arithmetic, `str-cat`, and `sym-cat`
reject them. Missing or negative indices make `retract` a no-op and
`fact-slot-value` return `FALSE`; invalid operand types and live invalid slots
still report errors. `FactAddress` equality uses the assertion identity and
working-memory epoch, so stale addresses cannot alias facts created after reset.

Rust host assertions reject fact-address values even when nested or copied from
an owned fact. C, Python, and Node value conversion also rejects them. Use host
fact handles for embedding operations; do not persist or decode runtime addresses
as host handles. Snapshots retain internal addresses as described below.

## Pre-1.0 snapshot schema 12

Snapshots are written with schema 12. Schema 11 snapshots are rejected with
`UnsupportedVersion(11)` because LEX/MEA activations now retain absent-CE recency
positions and rule complexity, with CLIPS-compatible ordering keys. Rule
complexity is computed at definition and restored without reevaluating rules.
Schema 10 snapshots are rejected with
`UnsupportedVersion(10)` because rule auto-focus metadata now persists.
Definition-time salience is stored as its resolved integer; neither it nor
pending activations cause new focus changes during restore. Schema 9 snapshots
are rejected with
`UnsupportedVersion(9)` because negative and NCC blocker attachment histories
now persist to preserve activation order after retraction. Schema 8 snapshots
are rejected with `UnsupportedVersion(8)` because random-generator state,
construct declaration order, and allowed-value source order now persist.
Schema 7 snapshots are rejected with
`UnsupportedVersion(7)` because template constraints and dynamic default
expressions now persist in registered template metadata. Schema 6 snapshots are rejected with
`UnsupportedVersion(6)` because executable effects and immediate lifecycle
semantics change restored behavior, and cleared fact chronology is now
persisted. Schema 5 snapshots are rejected with
`UnsupportedVersion(5)` because fact addresses now have a distinct persisted
identity and reset epoch. Schema 4 snapshots are rejected with
`UnsupportedVersion(4)` because generic methods now retain parameter queries
and typed wildcard restrictions. Callable control-flow and wildcard argument
semantics have also been corrected. Schema 3 snapshots are rejected with
`UnsupportedVersion(3)` because deffacts now preserve field initializers and
evaluate them at reset rather than retaining values computed during loading.
Schema 2 snapshots are rejected with
`UnsupportedVersion(2)`: their compiled graphs could expand field-level `|`
constraints into rule variants with incorrect matching and firing behavior.
Schema 1 snapshots remain rejected with `UnsupportedVersion(1)` because their
compiled patterns did not check field counts. Restore an old snapshot with the
version that produced it, export the application data, and assert it into a
new engine; see [snapshots.md](snapshots.md).

## Pre-1.0 CLIPS behavior fixes

The fixes for issues #320 to #346 make these cases behave like CLIPS 6.30.
Programs that relied on the earlier behavior need changes:

- An ordered pattern matches only facts with the same number of fields:
  `(data ?x)` no longer matches `(data 1 2)`. Use `$?` to match the rest.
- A multislot pattern matches the whole multislot: `(tags ?t)` needs exactly
  one value. Use `(tags $? ?t $?)` to match any member.
- `sort` asks its predicate whether two fields should be exchanged, so
  `(sort > ...)` sorts ascending and `(sort < ...)` descending.
- `string-to-field`, `explode$` and `read` use the CLIPS field scanner: a
  quoted string that contains spaces stays one STRING field. `read` still
  returns only the first field of its line.
- `format` rejects an argument count that does not match its directives, `%s`
  of a number, and a malformed directive such as `%5-3d`.
- `str-cat` and `sym-cat` spell FLOATs like `printout` (`(str-cat 1e20)` is
  `"1e+20"`), and `printout` quotes STRING fields inside a multifield.
- `round` breaks half ties toward the lower integer, and `min`/`max` return the
  selected operand with its own type.
- `str-length`, `sub-string` and `str-index` count characters and accept
  SYMBOLs, `sub-string` clips out-of-range positions, and `nth$` returns `nil`
  for a missing position.
- `fact-index` returns the public assertion index, and `fact-existp`,
  `fact-relation`, `fact-slot-value`, `fact-slot-names` and `retract` accept
  that index as well as a fact address. An INTEGER was previously read as an
  internal fact handle.
- `halt` inside a loop or query body no longer skips the rest of the body or
  later loops; the run stops when the RHS finishes.
- Each fact visited by a query counts against
  `EngineConfig::max_action_loop_iterations`, like a loop iteration.
- These are now load errors, as in CLIPS: a parenthesized `defmethod`
  parameter such as `((?x))` (write `(?x)`), a single-field slot pattern with
  several field constraints such as `(color red green)`, and a slot that
  appears twice in one template pattern.

## Step 1: Check Feature Coverage

Review your CLIPS codebase for features that Ferric does not support:

**Not supported (will not compile):**
- COOL object system (`defclass`, `definstances`, `defmessage-handler`,
  `send`, `make-instance`)
- Certainty factors
- Conflict strategies: `simplicity`, `complexity`, `random`

**Partially supported:**
- Pattern nesting: up to four combined `not`/`exists`/`forall` levels are
  supported, subject to compiled-condition limits. Triple and four-deep
  negation work. Single-operand `(exists (not fact-pattern))`, nested `forall`,
  and `forall` under `not` or `exists` remain unsupported. `forall` takes exactly one fact
  condition and one fact or test-only requirement; other operands are rejected
  with a source location. Positive `and`/`or` groups accept fact-address
  bindings, and `(not (or ...))` is supported.
  Snapshot validation has a separate four-level NCC dependency limit; nested
  multi-pattern `exists` can load successfully yet exceed that persistence
  limit. See [the compatibility limits](compatibility.md#source-and-compiled-network-limits).

If your rules use only `defrule`, `deftemplate`, `deffacts`, `deffunction`,
`defglobal`, `defmodule`, `defgeneric`, and `defmethod` with standard
library functions, you are likely in the supported subset.

## Step 2: Validate with ferric check

Use the CLI to validate your files without executing:

```bash
ferric check rules.clp
```

Or with machine-readable output for CI integration:

```bash
ferric check --json rules.clp 2> errors.json
```

Fix any reported parse or compilation errors before proceeding.

## Step 3: Review Rule Patterns and Actions

### Reduce excessive nesting

For a boolean condition without new variable bindings, redundant negations
can be simplified:

```clp
;; Exceeds Ferric's four-level source limit
(not (not (not (not (not (condition))))))

;; Same boolean condition within the limit
(not (condition))
```

### Use if/then/else in actions and callable bodies

The ordinary CLIPS `if`/`then`/`else` form works in rule RHS actions and callable
bodies (`deffunction` and `defmethod`). A direct `if` expression inside an LHS
`test` CE is unsupported. For example, this rule action is supported:

```clp
;; Supported in Ferric
(defrule classify
    (value ?x)
    =>
    (if (> ?x 10) then (printout t "big") else (printout t "small")))
```

To select matching rules with the condition instead, use a `test` CE:

```clp
(defrule classify-big
    (value ?x) (test (> ?x 10)) => (printout t "big" crlf))
(defrule classify-small
    (value ?x) (test (<= ?x 10)) => (printout t "small" crlf))
```

### Replace (exists (not ...))

```clp
;; CLIPS (unsupported nesting)
(exists (not (done ?x)))

;; Ferric: use a helper rule
(defrule find-undone
    (item ?x) (not (done ?x))
    => (assert (has-undone-item)))

(defrule process-undone
    (has-undone-item) => ...)
```

## Step 4: Review format Usage

Like CLIPS, `format` writes to its logical name and returns the formatted
string. Use `nil` when only the return value is needed:

```clp
;; Write a line and return its text.
(format t "value=%d%n" 42)

;; Format without writing, then include the result in another output call.
(printout t (format nil "value=%d" 42) crlf)
```

## Step 5: Understand Comparison Semantics

Ferric implements two distinct equality operators:

- `=` performs **numeric** equality. It coerces types: `(= 1 1.0)` is TRUE.
- `eq` performs **value** equality. It is type-sensitive: `(eq 1 1.0)` is FALSE.

This matches CLIPS semantics, but is a common source of bugs when migrating.
Use `=` for numeric comparisons and `eq` when you need exact type+value matching
(e.g., comparing symbols or strings).

## Step 6: Review Execution Effects

Fact mutation works inside deffunctions, methods, and ordinary expressions:

```clp
(deffunction record-and-double (?x)
    (assert (saw ?x))
    (* ?x 2))

(defrule compute
    (value ?x)
    =>
    (printout t (record-and-double ?x) crlf))
```

`(run)` called from a rule's RHS remains a documented no-op. Source `(reset)`
executes immediately, preserves output and active locals, and can reactivate
rules before the run ends. Source `(clear)` removes facts but preserves
constructs in active use. Both continue the current action sequence.

## Step 7: Review String Handling

Ferric uses byte-equality comparison with no Unicode normalization:

- ASCII content: behavior identical to CLIPS.
- Non-ASCII content: ensure inputs are normalized to a consistent form
  (e.g., NFC) before asserting.
- `sub-string` counts Unicode scalar values with one-based, inclusive positions.
  It clips starts below one and ends beyond the STRING or SYMBOL text.

## Step 8: Test Incrementally

1. Start with `ferric check` to validate syntax.
2. Run with `ferric run` and compare output to CLIPS.
3. Compare working-memory state, firing counts and observable output. Supported
   depth/breadth ordering follows activation creation chronology; LEX uses
   sorted fact recencies, specificity, and older-activation ties, while MEA
   compares the first pattern's recency before LEX.
4. Use `(declare (salience ...))` and `(focus ...)` to enforce ordering
   where side-effect order matters.

## Step 9: Embed via FFI (Optional)

If your application embeds CLIPS via its C API, Ferric provides a similar
C FFI surface. Key differences:

- Raw engines may move between OS threads; the host must serialize runtime
  calls and protect borrowed-pointer use and destruction. Per-engine error
  copies are separately synchronized. Both free entry points now have the same
  lifetime contract. The legacy thread-violation error code is reserved but
  never returned.
- Error handling uses return codes plus synchronized error channels. A failure
  involving a validated raw-engine handle updates both its per-engine snapshot
  and the calling thread's global fallback; pre-handle failures update only
  global state.
- Prefer `ferric_engine_get_output_copy` for captured output. The legacy
  `ferric_engine_get_output` pointer is an engine-owned, per-channel snapshot:
  another engine cannot invalidate it, but a later borrowed read for the same
  engine/channel can replace it; output clear/reset operations and engine
  destruction invalidate the relevant snapshot.
- Include `ferric.h` and link against the Ferric shared library.

See [compatibility.md](compatibility.md) Section 16.13 for the full FFI
contract.

Python `Engine` ordinary operations now serialize across threads. Code that
previously expected a `wrong thread` exception should use the normal result
or operation error; a closed engine reports `engine has been closed` on every
thread. `halt()` still cancels only an active run, and `close()` remains an
idempotent destruction barrier. Same-engine reentry during Python conversion
is explicitly rejected. This pre-1.0 change removes the creator-thread
requirement without adding an asynchronous operation queue.

Go `Engine` now serializes its complete native operation and diagnostic-copy
window internally, so callers may use it from different goroutines without a
lifetime `runtime.LockOSThread`. Constructors pin temporarily for thread-local
error retrieval. `Halt` queues behind an active operation; use a cancelable
run context for active-run cancellation.

Pre-1.0 breaking changes to the Go binding (September 2026): the package is
now just the core `Engine`. `PinnedEngine`, `Coordinator`, `Manager`,
`NewManager`, `Manager.Evaluate`/`EvaluateNative`, the `Wire*` types and
conversion helpers, `PanicError`, the `WithLogger`/`WithTracerProvider`/
`WithMeterProvider` observability options, and the `bindings/go/temporal`
package were removed, along with their OpenTelemetry and Temporal module
dependencies. Share one `Engine` across goroutines, or create one per
goroutine for parallel work, and wrap it in your own queue or activity if you
need one. `Engine.Clear` now returns an `error` (including `ErrEngineClosed`)
instead of discarding it. `Engine.Step` now returns `(bool, error)`, reporting
whether a rule fired; the `FiredRule` type was removed because its `RuleName`
was never populated.

---

## Common Gotchas

| Gotcha | Detail |
|--------|--------|
| `=` vs `eq` | `=` is numeric (coerces types); `eq` is value+type sensitive |
| `format` writes and returns | Use `nil` to format a string without also writing it |
| `sub-string` positions | One-based, inclusive Unicode scalar positions; bounds clip to the text |
| Function bodies can mutate facts | Effects complete immediately; earlier effects survive a later evaluation error |
| `run` from RHS is a no-op | `(run)` inside a rule action does nothing |
| Source `reset`/`clear` continue execution | Reset immediately restores working state; clear removes facts and retains active constructs |
| LHS guards belong in patterns/tests | Use RHS `if/then/else` for action control; use `(test ...)` CEs for match-time guards |
| Activation order | Depth/breadth compare activation chronology; LEX/MEA compare fact recencies and specificity before older-activation ties |

---

## Quick Reference: CLIPS to Ferric

| CLIPS Feature | Ferric Status |
|---------------|---------------|
| `defrule` | Supported |
| `deftemplate` | Supported |
| `deffacts` | Supported |
| `deffunction` | Supported (evaluator expressions; no fact mutation/control actions) |
| `defglobal` | Supported |
| `defmodule` | Supported |
| `defgeneric` / `defmethod` | Supported (evaluator expressions; no fact mutation/control actions) |
| `assert` / `retract` / `modify` / `duplicate` | Supported |
| `printout` / `format` / `read` / `readline` | Supported |
| `not` / `exists` / `forall` / `test` | Supported within the four-level source nesting and quantified-operand limits above |
| Salience | Supported |
| Focus stack | Supported |
| Depth / Breadth | Supported |
| LEX / MEA | Supported through host configuration; source strategy commands remain unsupported |
| `defclass` / COOL | Not supported |
| `if` / `then` / `else` | Supported in rule RHS actions and callable bodies; direct use in `test` CEs is unsupported |
| Certainty factors | Not supported |

## Template constraints and computed defaults

Parser `SlotDefinition` struct literals now require a `constraints` field;
use `SlotConstraints::default()` when unconstrained. `DefaultValue` has new
`Expressions(Vec<ActionExpr>)` and `Dynamic(Vec<ActionExpr>)` variants;
exhaustive consumers must handle them without treating dynamic expressions
as definition-time values.

Ferric retains primitive `(type ...)` unions, `allowed-symbols`,
`allowed-strings`, `allowed-lexemes`, `allowed-integers`, `allowed-floats`,
`allowed-numbers`, `allowed-values`, numeric `range`, and multislot
`cardinality`. Category-specific allowed lists constrain only their value
kinds; `(allowed-integers 1)` still permits symbols unless a type restriction
excludes them. Integer and float allowed values remain distinct. Conflicting
facets fail at load time. Class constraints (`allowed-classes` and
`allowed-instance-names`) remain unsupported.

Static `(default ...)` expressions evaluate once during template definition.
`(default-dynamic ...)` evaluates for each omitted slot on each assertion,
including host assertions and `load-facts`. Supplied values skip defaults;
`modify` and `duplicate` retain omitted values. Dynamic defaults resolve
callables in the template's module and direct global references in the
assertion caller's module. Slot evaluation follows declaration order.
`?DERIVE` selects a constraint-valid value; multislot minimum cardinality can
produce a nonempty default. `?NONE` requires an explicit value. Automatically
expanded defaults are limited to 1,000,000 fields and 32 MiB estimated storage.

Known invalid literal assertions, LHS constraints and defaults reject the
construct before installation. Runtime assertions, `modify`, `duplicate`,
and host template assertions validate complete values before publishing the
fact. This remains intentionally stricter than CLIPS 6.30's default `FALSE`
dynamic-constraint setting. A failed `modify` leaves the original fact intact;
a failed RHS action produces a diagnostic and stops that RHS. Effects from
expressions evaluated before an error remain visible.

`FACT-ADDRESS` and `INSTANCE-NAME` values and constraints are supported;
external-address slots still require `(default ?NONE)` or an explicit valid
value because Ferric does not manufacture host identity tokens. For the few
CLIPS 6.30 derivation cases that produce a value violating their own constraint,
Ferric chooses a valid default; see [compatibility.md](compatibility.md).

## September 2026 embedding API changes

- Rust assertions accept engine-scoped host values and opaque `FactHandle`s.
  Use `engine.symbol_value`, `HostValue::multifield`, named template slots, and
  `()` for empty fields. Raw core symbols cannot be used as portable input.
  Re-query fact handles after reset or restore; persist application IDs in facts.
  See [host-api.md](host-api.md).
- Snapshots use a bounded, versioned envelope (schema 12); CBOR is recommended
  and is the default for CLI, TypeScript, Python and Swift consumers. Legacy
  unversioned, schema-1, schema-2 and schema-3 snapshots are rejected explicitly. Export durable
  application data through the producing version before upgrading; see
  [snapshots.md](snapshots.md).
- Python plain `str` now means a CLIPS string. Use `ferric.Symbol` for symbols.
  Typed strings and symbols compare distinctly from each other and plain strings.
  Python `None`, Node `null`, and Swift `.void` cannot be stored in facts.
  Use an explicit application sentinel instead. Unsupported external values
  produce errors rather than null conversions. Owned Python fact values remain
  readable after closing their source engine and can be asserted into another
  engine, which interns symbol text into its own symbol table.
- Node requires version 22 or newer. Integers and fact IDs use `bigint` when they
  exceed safe-number precision; run counts accept exact safe integers. Closing a
  worker, handle or pool waits for supported native work and cleanup to finish.
- Requested callable depth remains configurable and persists, while actual
  evaluation is capped at 32 calls and 64 expression frames. Excessive recursive
  work returns an action diagnostic; deeply nested definitions are rejected.
- Go source imports use `github.com/plx/ferric-rules/bindings/go`. Engine
  operations serialize across goroutines; the worker/pool APIs were removed.
  Swift's local package uses Swift 6, macOS 15 or iOS 18, with asynchronous native
  work and owned results; see [its build instructions](../bindings/swift/README.md).
- The `ferric-rules-pinned` crate and the `ferric_pinned_*` C API are removed.
  They only worked around the old thread-affine engine; `Engine` is now
  `Send + Sync`, so move it to the thread that should run it or wrap it in your
  own worker. The no-op `Engine::check_thread_affinity` and
  `Engine::move_to_current_thread` shims and `EngineError::WrongThread` are
  gone too. In C, `FERRIC_ERROR_THREAD_VIOLATION` (2) stays defined but is
  never returned, and the former pinned error codes 11-15 are retired and will
  not be reused.

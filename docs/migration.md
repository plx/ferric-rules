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

Rust code that matches or constructs parser fact types must handle the new
`FactValue::Expression` variant and the `FactSlotValue::ordered_expression`
field. `EngineError` has a new `FactInitialization { definition, reason }`
variant, which `Engine::reset()` now returns when a deffacts initializer fails.

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

Calls to a deffunction or generic from rule RHS actions, function and method
bodies, and method queries now fail during loading unless the callable is
visible in the calling module: defined there, or exported by its module and
imported by the caller (see [Export/Import](compatibility.md#exportimport)).
Previously the loader accepted an unqualified call to a callable defined in
any module. Module-qualified calls such as `(OTHER::f)` to a nonexistent
module or callable also fail during loading instead of at run time. Add the missing
`export`/`import` declarations to programs that relied on the old lookup.

Parser `MethodParameter` struct literals need a `query` field. `MethodConstruct`
and runtime `RegisteredMethod` also store wildcard type/query restrictions;
`RegisteredMethod` stores one optional query per fixed parameter. Use `None`
and empty restriction vectors when there are no queries or wildcard types.
The existing `GenericRegistry::register_method` API retains its signature;
`register_restricted_method` accepts the additional restrictions.

## Typed fact addresses

`Value` and `AtomKey` have a new `FactAddress` variant. Exhaustive matches must
handle it. Rule variables bound to facts and fact-query results now carry this
type instead of integers containing arena keys. It prints `<Fact-N>` using the
public index retained at assertion time; FACT-ADDRESS slot defaults print
`<Dummy Fact>`. Integer fact designators always mean public indices.

Addresses are neither INTEGER nor NUMBER. Arithmetic, `str-cat`, and `sym-cat`
reject them. Missing or negative indices, stale addresses, and designators of
any other type make `fact-existp`, `fact-relation`, `fact-slot-names`, and
`fact-slot-value` return `FALSE` (`fact-index` returns `-1` for anything but an
address), and the rule continues; `fact-slot-value` does not evaluate its slot
argument in that case. Only an invalid slot, or slot argument type, on a live
fact stops the rule. Missing or negative indices make `retract` a no-op.
`FactAddress` equality uses the assertion identity and working-memory epoch, so
stale addresses cannot alias facts created after reset.

Rust host assertions reject fact-address values even when nested or copied from
an owned fact. C, Python, and Node value conversion also rejects them. Use host
fact handles for embedding operations; do not persist or decode runtime addresses
as host handles. Snapshots retain internal addresses as described below.

## Pre-1.0 snapshot schema 6

Snapshots are written with schema 6. Schema 5 snapshots are rejected with
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

The fixes for issues #320 to #346, #395, #396, #404 and #406 make these cases behave
like CLIPS 6.30. Programs that relied on the earlier behavior need changes:

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
- `format` writes its result to its logical name unless that name is `nil`, so
  `(printout t (format t ...) crlf)` now prints the text twice; use `nil` when
  the result goes into another output call. `printout` and `println` write each
  argument as soon as it is evaluated, so output from nested calls appears in
  place and text written before an argument error stays visible.
  `(printout nil ...)` no longer evaluates its arguments.
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
- A variable must be bound before an `|` alternative uses it, so
  `(item ?x|99)` and `(mnj (x ?x|?y) (y ?x|?y))` are now load errors, as in
  CLIPS. `?x&a|b` binds `?x` for every alternative. Overlapping alternatives
  no longer fire twice, and `not`, `exists` and `forall` test the whole
  disjunction.
- Source files, the REPL and `load-facts` scan numbers like `explode$`: a
  lexeme that starts with a digit, sign or `.` runs to the next CLIPS
  delimiter. `(place 1st)` now has one field, not `1 st`; `1-2`, `0x10`,
  `12abc` and `5e` are single SYMBOLs; `1.`, `.5` and `1.e3` are FLOATs (`.5`
  was a SYMBOL). Integers outside the signed 64-bit range saturate instead of
  rejecting the file. A `;` comment ends at CR as well as LF. The parser no
  longer reports `ParseErrorKind::InvalidNumber`.
- Top-level `assert` evaluates field expressions and globals in the module
  current at its source position, and deffacts evaluate theirs at each reset
  (see [the seed and reset changes](#pre-10-seed-and-reset-changes)). A
  statically invalid field rejects the whole `assert` command; an evaluation
  error keeps the facts it asserted earlier.

## Step 1: Check Feature Coverage

Review your CLIPS codebase for features that Ferric does not support:

**Not supported (will not compile):**
- COOL object system (`defclass`, `definstances`, `defmessage-handler`,
  `send`, `make-instance`)
- Certainty factors
- Conflict strategies: `simplicity`, `complexity`, `random`

**Partially supported:**
- Pattern nesting: single-level `not`, `exists`, `forall`, and NCC are
  supported. Triple-nested negation, `(exists (not ...))`, and nested
  `(forall ...)` are not.

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

### Replace nested negation

```clp
;; CLIPS (unsupported triple nesting)
(not (not (not (condition))))

;; Ferric: use an intermediate fact
(defrule detect-condition
    (condition) => (assert (condition-present)))
(defrule no-condition
    (not (condition-present)) => ...)
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

## Step 6: Move Fact Mutation Out of Functions

In Ferric, `deffunction` and `defmethod` bodies are evaluator expressions, not
full RHS action lists. They can call expression functions such as `str-cat`,
`format`, and `printout`, but fact mutation and agenda/focus control belong in
the calling rule's RHS:

```clp
;; CLIPS (fact mutation inside deffunction)
(deffunction record-and-double (?x)
    (assert (saw ?x))
    (* ?x 2))

;; Ferric: split into expression + RHS action
(deffunction double (?x) (* ?x 2))

(defrule compute
    (value ?x)
    =>
    (assert (saw ?x))
    (printout t (double ?x) crlf))
```

Also note: `(run)` called from a rule's RHS is a documented no-op in Ferric.
Use `(reset)` and `(clear)` from RHS with care -- they are deferred and take
effect after the current action sequence completes.

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
   depth/breadth ordering follows activation creation chronology; LEX/MEA remain
   experimental and have documented CLIPS differences.
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
| Function bodies are evaluator expressions | Put fact mutation and agenda/focus control in rule RHS code |
| `run` from RHS is a no-op | `(run)` inside a rule action does nothing |
| `reset`/`clear` are deferred | Flag is set and checked after the current action sequence completes |
| LHS guards belong in patterns/tests | Use RHS `if/then/else` for action control; use `(test ...)` CEs for match-time guards |
| Activation order | Depth/breadth follow activation creation chronology; LEX/MEA are experimental |

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
| `not` / `exists` / `forall` / `test` | Supported (single-level nesting) |
| Salience | Supported |
| Focus stack | Supported |
| Depth / Breadth | Supported |
| LEX / MEA | Experimental; documented CLIPS ordering differences |
| `defclass` / COOL | Not supported |
| `if` / `then` / `else` | Supported in rule RHS actions and callable bodies; direct use in `test` CEs is unsupported |
| Certainty factors | Not supported |

## Primitive template slot types

Ferric now retains `(type ...)` declarations for `SYMBOL`, `STRING`, `INTEGER`,
`FLOAT`, `NUMBER`, `LEXEME`, and `EXTERNAL-ADDRESS`. Type lists form a union;
`NUMBER` means integer or float, and `LEXEME` means symbol or string. Omitted
constraints and `(type ?VARIABLE)` permit any supported value kind. Each field
of a constrained multislot must satisfy its declared union.

Defaults follow CLIPS' primitive preference: symbol `nil`, empty string,
integer `0`, then float `0.0`, independent of the type list's spelling order.
Multislot defaults are empty unless specified. Literal multifield defaults,
including literal `create$` forms, retain every field. External-address slots
require `(default ?NONE)`; Ferric does not manufacture host identity tokens.

Invalid literal assertions reject a rule before installation or replacement.
Defaults and named deffacts are checked before registration. Runtime assertions,
`modify`, `duplicate`, and host template assertions also validate types before
changing facts. This runtime checking is intentionally stricter than CLIPS
6.30's default `FALSE` dynamic-constraint setting: applications must supply
values matching their declarations. A failed `modify` leaves the original fact
intact; a failed RHS action produces an action diagnostic and stops that RHS.

Previously ignored `range`, `allowed-*`, `cardinality`, `default-dynamic`, and
other optional slot attributes now produce an explicit unsupported error.
Arbitrary computed defaults are also unsupported; use literal defaults or
`?DERIVE`, and calculate dynamic values before assertion. `FACT-ADDRESS` and
instance type declarations are rejected because the supported value model has
no corresponding tagged value. These restrictions do not add CLIPS class
constraints, general static type inference, or dynamic constraint toggles.

## September 2026 embedding API changes

- Rust assertions accept engine-scoped host values and opaque `FactHandle`s.
  Use `engine.symbol_value`, `HostValue::multifield`, named template slots, and
  `()` for empty fields. Raw core symbols cannot be used as portable input.
  Re-query fact handles after reset or restore; persist application IDs in facts.
  See [host-api.md](host-api.md).
- Snapshots use a bounded, versioned envelope (schema 6); CBOR is recommended
  and is the default for CLI, TypeScript, Python and Swift consumers. Legacy
  unversioned, schema-1, schema-2, schema-3 and schema-4 snapshots are rejected
  explicitly. Export durable application data through the producing version
  before upgrading; see
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

# Ferric Compatibility with CLIPS

This document details Ferric's compatibility with CLIPS (C Language Integrated
Production System). Each section covers a major CLIPS language area and
documents supported features, behavioral differences, and any restrictions.

Ferric targets semantic compatibility with the CLIPS Basic Programming Guide
for the supported subset. "Supported" means that the language area is
implemented, not that every rule set in that area has been proven equivalent.
Exact CLIPS compatibility claims are limited to the reviewed differential
policy cases and the granular corpus programs, and are qualified by the known
gaps below.

## Reviewed Compatibility Evidence

The blocking pinned-CLIPS lane requires equivalent observations for every
reviewed policy case and rejects unexplained divergences. The LEX and MEA
`FR-RETE-009` cases now match the canonical recency ordering verified in
[#412](https://github.com/plx/ferric-rules/issues/412).

The reviewed differential policy covers 57 scenarios: the existing 22 cases
and 35 distinct rehabilitation scenarios, plus a generated-harness control.
All 57 cases are equivalent. This does not turn undeclared corpus fixtures
into compatibility claims; those remain unassessed
until they receive a structured oracle and reviewed policy entry. See
[Compatibility assessment oracles](compatibility-assessment.md) for the exact
evidence boundary.

### Granular corpus

The broadest evidence for the language behavior in this document is
[`tests/clips_compat/corpus/`](../tests/clips_compat/corpus/README.md): 1266
small programs, each with the exact output of CLIPS 6.30 as its golden.
`cargo test --workspace` runs all of them. A conforming program must reproduce
its golden byte for byte, and again after a CBOR snapshot round trip (and a
JSON one, unless it holds a non-finite float). When its deffacts precede its
rules, it must also print the same lines with the rules loaded after `reset`,
in any order, since CLIPS orders those activations differently. A program that
CLIPS rejects must fail in Ferric at the same stage (load or run).
`just compat-corpus-reference` rechecks every golden against the pinned CLIPS
6.30 Docker image. The standalone and PR comparison reports lead with the
manifest's declared case, conformance (including expected-error), and known-gap
counts. They separately show whether matching Ferric and reference runs
verified that revision; a manifest declaration or historical verification
stamp alone is not a passed run. Main CI verifies the goldens automatically.

A known difference is recorded on its case as a `gap` entry holding Ferric's
exact current output, so the test fails if the behavior changes in either
direction. Four cases track output/format differences in
[#394](https://github.com/plx/ferric-rules/issues/394), and nine track network
topology and installation-history differences: five equal-salience ties
described with [#400](https://github.com/plx/ferric-rules/issues/400) and four
auto-focus histories tracked in
[#480](https://github.com/plx/ferric-rules/issues/480), three from late
installation and one from a queued test CE:

| Area | Difference from CLIPS 6.30 | Cases |
|------|----------------------------|-------|
| Output that is not UTF-8 | CLIPS emits raw bytes for `%c` of a byte of 128 or more, for `%.Ns` that cuts a multibyte character, and for a scanned string that ends in an escaped end of input. Ferric strings are always UTF-8 and hold U+FFFD instead. | `stdlib/121_format_character_nul_and_bytes`, `stdlib/116_format_unicode_width_and_precision`, `io/read-unterminated-terminal-backslash` |
| Malformed `format` directives | CLIPS passes a directive such as `%5-3d` to `printf`, which echoes it; Ferric reports a format error. | `stdlib/120_format_repeated_and_misordered_modifiers` |
| Identical negative/NCC joins | CLIPS shares these joins across rules; Ferric compiles them separately, changing selected equal-salience ties (under LEX/MEA, only when recency and specificity are also equal). | `patterns/400o_gap_shared_negative_assert_depth`, `patterns/400o_gap_shared_negative_retract_depth`, `patterns/400o_gap_identical_ncc_depth` |
| Multi-pattern `exists` | Lowering a conjunction through nested NCC nodes can visit independent supports in a different order. | `patterns/400o_gap_independent_multi_exists_depth` |
| Nested NCC on a shared subnetwork entry | CLIPS can decide a nested NCC before its shared entry join has seen the token, transiently retracting and refiring the enclosing rule; Ferric waits for the entry and does not refire it. | `patterns/400o_gap_nested_ncc_shared_entry_refire_depth` |
| Late-installed blocked NCC with auto-focus | Building an auto-focus rule whose NCC is blocked when it is installed can miss the transient focus push CLIPS performs during installation: a fresh subnetwork does not replay historical fact order, and a test CE after an NCC that shares the rule's left prefix is only queued before the subnetwork blocks the token. | `modules/398_gap_late_ncc_fresh_parent_first`, `modules/398_gap_late_ncc_shared_prefix_trailing_test` |
| Late-installed `(exists (and ...))` holding a negation | Building an auto-focus rule whose `(exists (and ...))` conjunction contains a `not` and does not hold when it is installed can make a transient focus push that CLIPS does not make. | `modules/398_gap_late_exists_conjunction_negation` |
| Test CE after an NCC cancelled before it runs | During reset or assertion, a test CE after an NCC is only queued; when the NCC's subnetwork completes first and retracts the token, Ferric never creates the transient activation, so an auto-focus rule misses the focus push CLIPS keeps. | `modules/398_gap_ncc_deferred_test_cancelled_reset` |

Some CLIPS-valid programs are rejected at load instead of running
differently. The main case is a complex non-linear predicate or return-value
constraint inside a negated ordered pattern, tracked in
[#300](https://github.com/plx/ferric-rules/issues/300) (see
[Template Facts](#template-facts)). Ten further `gap` cases from
[#405](https://github.com/plx/ferric-rules/issues/405) hold Ferric's exact,
located load error for the explicit conditional-element nesting and operand
limits listed under [Pattern Nesting Restrictions](#pattern-nesting-restrictions).

---

## 16.1 Facts

Ferric supports both ordered (positional) and template (slot-based) facts with
the same value types and working-memory semantics as CLIPS.

### Value Types

| Type | Description |
|------|-------------|
| `INTEGER` | 64-bit signed integer |
| `FLOAT` | 64-bit IEEE 754 double |
| `SYMBOL` | Interned identifier (e.g., `red`, `TRUE`) |
| `STRING` | Quoted string (e.g., `"hello"`) |
| `INSTANCE-NAME` | Bracketed name (e.g., `[widget]`), distinct from SYMBOL |
| `MULTIFIELD` | Ordered sequence of values |
| `FACT-ADDRESS` | Opaque fact identity, displayed as `<Fact-N>` or `<Dummy Fact>` |

Instance names are values only: Ferric has no object system, so `[widget]`
names no instance. They match, compare, print and round-trip through
`explode$`/`implode$` as in CLIPS, and a template slot may declare
`(type INSTANCE-NAME)` (derived default `[nil]`). Hosts create them with
`Engine::instance_name_value`; the bindings expose an `InstanceName` type and
the C ABI uses value type `7` with the spelling (without brackets) in
`string_ptr`.

### Ordered Facts

Ordered facts are positional sequences of values:

```clp
(assert (color red))
(assert (data 10 20 30))
```

Top-level assertions in source, the REPL, and string-assert binding APIs
evaluate expressions and globals, just as rule actions do. For example,
`(assert (data (+ 1 2) (create$ a b)))` asserts `(data 3 a b)`. An invalid
field reports an error instead of being silently omitted. Ordered fields and
multislots splice multifield results; single slots reject them even when the
multifield contains exactly one value. `load-facts` accepts literal fact data
only, and stops at an invalid fact while retaining earlier valid facts.

As in CLIPS, every fact of an `assert` command is parsed before any is
asserted. A statically invalid field (an unknown function, an unknown slot, or
a static multifield in a single slot) rejects the whole command. An evaluation
error, such as `(/ 1 0)`, an unbound local variable or an undefined global,
stops the command and keeps the facts it completed earlier. CLIPS 6.30 still
inserts the failing fact: an ordered fact loses all of its fields (`(bad)`),
and a template fact keeps its other slots while the failing slot holds an
unspecified value. Ferric inserts none of it. Each top-level assertion is evaluated in the module current
at its position in the source, and the last `defmodule` stays current
afterwards. Void results, such as those of `printout`, are omitted from
ordered fields and multislots. Template slot expressions are evaluated in the
template's slot declaration order, not the order the source writes them.

Ordered patterns consume every field: `?` and `?name` match one field, while
`$?` and `$?name` match zero or more fields at any position. For example,
`(row head $?values tail)` captures `(a b)` from `(row head a b tail)` and an
empty multifield from `(row head tail)`. Multiple multifield fields produce a
separate match for each valid partition. Named captures are available to later
patterns, test conditions, and rule actions.

### Template Facts

Template facts use named slots defined by `deftemplate`:

```clp
(deftemplate person (slot name) (slot age (default 0)))
(assert (person (name Alice) (age 30)))
```

Each constrained multislot matches its complete sequence using the same field
and multifield rules as ordered patterns. `(tags ?value)` requires exactly one
value and binds a scalar; `(tags $?values)` binds the entire multifield, and
`(tags head $?values tail)` captures the values between the fixed fields.
An explicit `(tags)` requires an empty multislot; omitting `tags` leaves it
unconstrained. A restriction with only fixed fields whose count violates the
slot's `cardinality` rejects the rule (`[CSTRNCHK1]`). As in CLIPS 6.30, an
empty `(tags)` still loads when the minimum cardinality is above zero; it can
never match a valid fact, so its `(not ...)` form is always satisfied. Ambiguous splits in multiple multislots produce every valid
combination, in written slot-constraint order. Single-valued slots require one
field constraint and cannot bind a named multifield capture.

RHS assertions resolve declared templates in the rule's module, evaluate supplied
values and missing dynamic defaults in template slot declaration order, and
propagate template matches. Multislots splice supplied
multifield values; both `?items` and `$?items` read the same bound value. Invalid
slot names, repeated slots and statically invalid cardinality reject the rule
before installation. A dynamic single-slot cardinality error stops that RHS
without asserting a partial fact. Void expression results are omitted from
multislots while their output effects remain observable.

Pre-1.0 migration: template metadata now records slot cardinality. Unversioned
(legacy raw) engine snapshots are rejected; persist engines with the versioned
CBOR snapshot envelope described in [Snapshots](snapshots.md), and keep
application facts/rule source if older data must be rebuilt.

Complex non-linear predicate or return-value constraints inside negated ordered
patterns are CLIPS-valid but explicitly rejected during load. PR #254 removed an
incorrect firing-time fallback; it did not complete that optional language
feature. The remaining gap is tracked in [#300](https://github.com/plx/ferric-rules/issues/300).

### Fact Identity

Each successfully asserted fact receives a unique fact index. Fact addresses
can be captured via pattern-binding variables (`?f <- (pattern)`) and used in
`retract`, `modify`, and `duplicate` actions.

### Fact Duplication

Like CLIPS, Ferric disables fact duplication by default. With duplication
disabled, an assertion is rejected if an equivalent active fact exists; it
creates no new fact ID, RETE token, activation, or existential support.

Structural equality is defined as follows:

- Ordered facts compare their relation and every positional field.
- Template facts compare their template identity and every slot value in the
  template's canonical slot order.
- Nested multifields compare recursively. Floats compare by IEEE-754 bit
  representation, including distinct signed-zero and NaN representations.
- Fact IDs and assertion timestamps are never part of structural equality.

`(get-fact-duplication)` reports the current policy.
`(set-fact-duplication TRUE|FALSE)` changes it immediately and returns the
previous setting. Existing duplicate facts are not collapsed when duplication
is disabled. The setting survives `reset`, `clear`, and engine snapshot
round-trips.

### initial-fact

On `(reset)`, Ferric asserts the protected `(initial-fact)` first and then the
registered `deffacts`, the same order as CLIPS. The fact supports explicit
`(initial-fact)` patterns; rules with no patterns or a leading negation match
without it. Host fact queries do not return it, and it cannot be retracted,
modified, or duplicated. Its fact index is 0 and, as for CLIPS's slotless
`initial-fact` deftemplate, `fact-slot-names` and `deftemplate-slot-names` of
it are `()`. An `initial-fact`
the host asserts through the engine API is an ordinary user fact with an
ordinary index; CLIPS gives it index 0.

### Behavioral Notes

- Duplicate fact detection: asserting an identical fact to one already in
  working memory is rejected by default (no new fact created).
- Refraction: a rule fires at most once per unique token (set of matching
  facts). Retracting and re-asserting the same content creates a new fact
  identity, allowing re-firing.

---

## 16.2 Rules

Ferric implements `defrule` with the same syntax and semantics as CLIPS,
including all commonly used conditional elements and RHS actions.

### defrule Syntax

```clp
(defrule rule-name
    "optional comment"
    ;; salience is evaluated once, when the rule is defined
    (declare (salience <integer-expression>) (auto-focus TRUE|FALSE))
    ;; LHS patterns
    (pattern-1)
    ?var <- (pattern-2)
    (test (> ?x 10))
    =>
    ;; RHS actions
    (printout t "fired" crlf))
```

### Conditional Elements

| CE | Syntax | Notes |
|----|--------|-------|
| Ordered pattern | `(fact-name ?x ?y)` | Positional field matching |
| Template pattern | `(template (slot-name ?v))` | Slot-based matching |
| test | `(test (expr))` | Boolean guard expression |
| not | `(not (pattern))` | Single-pattern negation |
| exists | `(exists (pattern))` | Fires once when any match exists |
| forall | `(forall (P) (Q))` | Universal quantification |
| NCC | `(not (and (P) (Q)))` | Negated conjunction |

### Variable Binding

- Variables bind on first occurrence and must be consistent across all
  patterns in the rule.
- Fact-address variables (`?f <- (pattern)`) capture the fact identity for
  use in `retract`/`modify`/`duplicate`.

### Constraint Connectives

| Connective | Meaning | Example |
|------------|---------|---------|
| `~` | Negation | `(color ~red)` |
| `\|` | Disjunction | `(color red\|blue)` |
| `&` | Conjunction | `(value ?x&~0)` |

Precedence is `~` > `&` > `|`, except that a leading `?x&` binds over the
rest of the field: `?x&a|b` means `?x&(a|b)`. Variables used inside
alternatives must already be bound.

### Conflict Resolution Strategies

The host API supports CLIPS depth, breadth, LEX, and MEA ordering. Salience
takes precedence in every strategy:

| Strategy | Description |
|----------|-------------|
| **Depth** | Most recent activation fires first (default) |
| **Breadth** | Oldest activation fires first |
| **LEX** | Sorted fact recencies, then specificity, then older activations |
| **MEA** | First-pattern recency, then the LEX comparison |

LEX sorts each activation's fact recencies from newest to oldest. Negated and
existential conditions contribute absence entries, which are older than every
real fact. If one recency vector is a prefix of another, the longer vector
wins. Equal vectors compare rule specificity, then prefer the older activation.
Specificity counts source patterns and comparisons separately for each expanded
OR branch. Each predicate or return-value call counts once and its ordinary
arguments add nothing, but each call nested in `and`, `or`, or `not` counts
separately (`agenda/412_predicate_call_complexity_*`). MEA first compares the
first outer pattern's recency, including a leading absence, then uses the same
LEX comparison. These rules also order multiple partitions of one multifield
fact.

`Simplicity`, `Complexity`, and `Random` are not implemented.
`(get-strategy)` returns the current strategy symbol. `(set-strategy breadth)`
returns the previous strategy and reorders pending activations without changing
their identity or creation order. The supported names are `depth`, `breadth`,
`lex`, and `mea`; the setting survives reset, clear, and snapshots. Changing it
from a match condition is rejected. `complexity`, `simplicity`, and `random` are
rejected when the call is loaded, or with an error at run time. As in CLIPS, any
other symbol writes an `[ARGACCES5]` notice, keeps the current strategy, and
returns it, and evaluation continues. Bindings reject unknown enum/name values.

For equal salience, activation creation order breaks ties: depth selects the
newest activation and breadth the oldest. Shared beta successors and mixed
positive/negative/exists subscriptions are visited newest first. Retraction
handles the fact's pattern matches one alpha memory at a time, oldest memory
first. Within a memory, the positive cascade unblocks NCC matches first, then
that memory's blocked simple negative matches are released in reverse
primary-blocker attachment order; moving to another blocker updates that
attachment order. See the
[activation ordering contract](#activation-ordering-contract) for the remaining
node-sharing boundary.

### Salience

Rules may declare one integer or expression salience. Expressions evaluate once
when the rule is defined, in its owning module, using the globals and callables
available at that point in the source. The result must be an integer in
-10000 through 10000. Later global changes, activations, and reset do not
reevaluate it; OR branches share the single resolved value. Higher salience
fires first within the chosen conflict resolution strategy:

```clp
(defrule high-priority
    (declare (salience 100))
    (go) => (printout t "high" crlf))

(defrule low-priority
    (declare (salience 10))
    (go) => (printout t "low" crlf))
```

For example, `(declare (salience (+ ?*BASE* 5)))` uses the value of `BASE`
at definition time. Local bindings in a salience expression have a fresh scope
and do not bind RHS variables. Evaluation preserves prior expression effects
and output if a later operation or the final result is invalid; the invalid
rule is not installed. Failed replacements retain the previous rule.

`(declare (auto-focus TRUE))` pushes the rule's owning module whenever a new
activation is created, including during reset, assertion, retraction, or online
rule installation. `FALSE` is the default; only these two literal symbols are
accepted. Removing an activation does not undo its focus change. Both explicit
`focus` and auto-focus skip a push when that module is already on top, while a
module deeper in the stack may appear again. Predicate and NCC evaluation
follow depth-first traversal order whether or not auto-focus is used, so
declaring auto-focus on one rule does not reorder unrelated rules. A rule
guarded by a pure double negation, `(exists (and ...))` or the equivalent
`(not (and (not (and ...))))`, pushes its module during reset and assertion
only once the conjunction holds, even when the conjunction's first join is
shared with an older rule. Online installation of such a rule does the same
when the conjunction holds no negation; one with a `not` inside can still
push transiently when it is installed blocked (see below). An outer `(not (and ...))` with more conditions
after its nested one, such as `(not (and (not (and (a) (b))) (c)))`, keeps
CLIPS's transient admission and focus push before those conditions block it.
A `(not (and ...))` whose first join is shared with an older rule also makes
CLIPS's transient admission, and focus push, before that join blocks it.
Snapshots preserve the focus stack without replaying notices for existing
activations.

Late installation of a blocked NCC rule follows CLIPS's transient focus push
in two situations: when its fresh subnetwork shares the rule's left prefix with
an older rule, unless a test CE follows the NCC; and when a fresh join inside
the NCC follows a shared, already populated subnetwork entry join. There is no
push when the only thing after that populated shared join is a test CE inside
the NCC, whether or not the older rule also defers that test. Three
characterized late-installation differences remain, tracked in
[#480](https://github.com/plx/ferric-rules/issues/480): with a shared prefix and
a trailing test CE, the test is only queued before the fresh subnetwork blocks
the token, so the transient activation and its focus push never happen; a
fresh subnetwork with no shared prefix does not reconstruct every transient
activation from the historical fact assertion order, so such a rule can miss a
historical focus push; and a rule guarded by `(exists (and ...))` whose
conjunction contains a `not` and does not hold at installation can admit its
token before the conjunction is primed, making a transient focus push that
CLIPS does not make.

A queued test CE loses CLIPS's transient focus push during ordinary reset and
assertion too. When a test CE follows an NCC and the NCC's subnetwork
completes and retracts the token before the queued test is evaluated, as for
`(p) (not (and (a) (b))) (a) (test ...)` with `(a)` asserted after `(b)` and
`(p)`, no activation is created, so the push CLIPS keeps never happens (also
tracked in #480). Other assertion, reset, and blocker-retraction focus
behavior has conforming coverage.

`when-activated` and `every-cycle` salience evaluation, `set-salience-evaluation`,
and `refresh-agenda` remain unsupported. Invalid or duplicate declarations
reject the construct; they never silently become salience zero.

### Fact-query expressions

`do-for-fact`, `do-for-all-facts` and `delayed-do-for-all-facts` work in RHS
actions, expressions, deffunctions, and methods. Each member accepts one or
more restriction expressions naming visible, unqualified deftemplates or
previously declared ordered relations. A symbol literal resolves at load time;
other expressions evaluate once when the query starts and must return a symbol
or a nonempty multifield of symbols. Restrictions evaluate in source order
before traversal, including for an empty query or an early `any-factp` result.
Alternatives retain their order and duplicates. Each alternative visits live
facts in assertion order, with the last member varying fastest. An immediate
query sees facts its bodies assert and skips facts they retract;
`delayed-do-for-all-facts` selects every tuple before it runs a body, and its
members keep their slot values after a body retracts them. Predicates and
bodies can read `?f:slot`, and bodies can `retract`, `modify` or `duplicate`
query members. `any-factp`, `find-fact` and `find-all-facts` work in RHS
expressions, deffunctions and methods; the find forms return a multifield of
[fact addresses](#fact-addresses).

Queries, `if`, and `switch` also work in LHS test CEs and predicate/return-value
constraints. As in CLIPS, a test CE observes facts when the matching token
reaches the test; a later change to a queried relation does not independently
reevaluate an existing token. Queries can compare members with fact addresses
bound by earlier patterns. Unbound or later-bound variable references in these
queries are load errors. Match-time expressions cannot mutate the engine.

Ferric evaluates predicate and return-value constraints the same way, when the
token reaches them. CLIPS instead evaluates a constraint that references only
its own pattern's variables once, in the pattern network, when the fact is
asserted, so a query or global read in such a constraint can see different
facts or values in CLIPS
([#488](https://github.com/plx/ferric-rules/issues/488)).

Each visited query member costs one iteration of the action-loop budget
(`EngineConfig::max_action_loop_iterations`), as does each delayed body.
`halt` in a query body lets the current call and RHS finish. `reset` takes
effect immediately and the current body continues, retaining its locals and
member slot values. Its old fact addresses become stale. Immediate traversal
stops reading the current alternative's old fact list, then visits later
alternatives against the reset facts. Outer query members retain their selected
payloads while inner alternatives advance. A delayed query still visits the
tuples it captured before reset. A query returns its last body
value, `FALSE` if no body ran, or no value after `break`.

Literal names follow source declaration order, including within one rule or
function. An ordered assertion declares its relation before its initializer
expressions. In RHS actions, deffunctions and methods, dynamic names use the
current callable's definition module and remain fixed for that query
invocation. In LHS expressions Ferric resolves dynamic names in the rule's
module, while CLIPS uses the module current when the match runs (typically the
module of the rule whose action asserted the fact)
([#489](https://github.com/plx/ferric-rules/issues/489)). Resolved targets
remain in use until the query finishes, even if its body retracts every
matching fact.

Query members are scoped to their predicate and body. Restriction expressions
can read caller locals and bind locals; nested query and iterator bindings can
shadow member names. Unshadowed reads of the current query's own members in its
restrictions are rejected explicitly: those forms crash pinned CLIPS 6.30.
Binding a query member in its body, or binding a local in its predicate, is a
load error. Qualified target names remain unsupported. Queries and engine
effects in defglobal initializers run once at load; on reset Ferric restores
the stored value instead of re-evaluating the initializer (CLIPS 6.30
re-evaluates; [#451](https://github.com/plx/ferric-rules/issues/451)), so
captured fact addresses become stale and effects are not repeated. Ferric
retains owned candidate payloads across delayed query resets instead of
reproducing CLIPS's stale-pointer reuse behavior.

### Activation Ordering Contract

Depth and breadth follow CLIPS 6.30 for the reference-verified shared-positive,
mixed-subscription, empty-LHS, OR-variant, and blocker-retraction cases in the
corpus. Creation order follows network traversal, rather than rule name or
source order alone. Each blocked match initially attaches to its oldest
support. Retracting a fact processes its pattern matches as CLIPS does: alpha
memories in creation order, and within each memory the positive cascade (NCC
unblocks) before that memory's block list, newest attachment first. Matches
with another support attach to the oldest survivor. This history survives
snapshots and is cleared on reset.

CLIPS also shares identical negative and NCC joins between rules; Ferric
currently shares positive joins only. Equal-salience ties involving those
separately compiled negative joins can therefore differ. This is the explicit
node-sharing boundary of [#400](https://github.com/plx/ferric-rules/issues/400),
recorded by exact corpus characterizations. A further pre-existing difference
occurs for selected multi-pattern `exists` ties: Ferric lowers the conjunction
through nested NCC nodes, while CLIPS uses a distinct existential join topology.
Its independent-support example remains characterized. Ties among subscribers
of different patterns, for example a later rule reusing an earlier rule's
pattern behind a different first pattern, are ordered by node age rather than
pattern by pattern as in CLIPS, and may differ. A nested NCC whose subnetwork
entry is shared with an older rule settles after that entry, so Ferric never
transiently retracts and refires the enclosing rule, as CLIPS can when another
successor was linked to the same parent in between. Nested NCC chains, such as
`(exists (exists ...))`, settle depth first as in CLIPS: when several rules'
chains share one subnetwork, each chain finishes before the next one starts,
so deeper and shallower nestings tie as the reference does. Equal-salience ties
in network topologies the corpus does not cover are not a promise of
replay-identical order across engines or versions.

LEX and MEA first compare sorted fact recency and rule specificity, as described
under [Conflict Resolution Strategies](#conflict-resolution-strategies).
Activations equal in both fall back to creation order, oldest first, so they
inherit the creation-order boundaries described above.

For application semantics that require precedence independently of network
construction, use salience, `focus`, or phase facts. Within an engine, the
agenda maintains a total order and restoring a snapshot preserves pending
activation order and the blocker history used for future activations.

### RHS Actions

| Action | Notes |
|--------|-------|
| `assert` | Assert ordered or template facts; return the last address, or `FALSE` if that assertion is a duplicate |
| `retract` | Retract by address or public index; return no value |
| `modify` | Retract the original template fact and assert its replacement; return the new address or `FALSE` for a duplicate |
| `duplicate` | Assert a template copy with slot overrides; return its address or `FALSE` for a duplicate |
| `printout` | Write to a named channel (`t` for stdout) |
| `halt` | Stop the run once the current RHS finishes (loops and queries in it run to completion) |
| `focus` | Push one or more modules onto the focus stack, skipping a module already on top; return `TRUE`, or `FALSE` for a missing module (without CLIPS's `[PRNTUTIL1]` notice) |
| `bind` | Bind a variable or update a global |
| `list-focus-stack` | Print the current focus stack |
| `agenda` | Print the current module's activations as CLIPS rows with fact bases (`*` for a negated condition or an empty LHS) and a tally, or every module's under headings with `*`; an empty agenda prints nothing |
| `run` | No-op when called from RHS (documented behavior) |
| `reset` | Reset working memory and globals immediately, preserving current execution and printed output; return no value |
| `clear` | During execution, retract all facts and refuse construct removal; continue execution and return no value |
| `if`/`then`/`else` | Conditional execution; an unmatched condition without `else` returns `FALSE` |
| `while` | Conditional loop with `do`; returns `FALSE` and shares the configured per-activation action-loop budget |
| `loop-for-count` | Inclusive count loop with a bare literal, variable, or expression bound, or `(?i end)` / `(?i start end)`; returns `FALSE` and shares the configured per-activation action-loop budget |
| `progn$` / `foreach` | Multifield iteration with element and index binding; returns the last body value, `FALSE` for an empty collection, or no value after `break` |
| `switch`/`case`/`default` | Multi-branch dispatch; an unmatched value without `default` returns `FALSE` |
| `break` | Exit the nearest enclosing loop or action fact query and continue after it |
| `do-for-fact` | Iterate first matching fact |
| `do-for-all-facts` | Iterate all matching facts |
| `delayed-do-for-all-facts` | Deferred all-facts iteration |
| `any-factp` | Boolean fact existence check |
| `find-fact` | Find first matching fact |
| `find-all-facts` | Find all matching facts |

`assert`, `retract`, `modify`, `duplicate`, `halt`, `focus`, `reset`, and
`clear` also work as expressions and in deffunction/method bodies. Effects
happen as their expressions are evaluated. `focus` evaluates and pushes its
module operands from right to left, stopping at a missing module while keeping
modules already pushed. `modify` creates a new fact identity even
when slot values are unchanged; if its replacement is a duplicate, the
original is still retracted. Earlier assertions in a multi-fact `assert`
remain when its last assertion returns `FALSE`.

Reset preserves active parameters, local bindings, loop iterators, and output
already written. It can repopulate the agenda, so a rule that resets must
arrange to terminate. A halt requested earlier in the same RHS, directly or in
a callable, survives the reset: the RHS finishes and the run stops. A nested
reset during reset-time initialization is ignored. Clear during active
execution emits a recoverable refusal (`[CONSTRCT1]`), retains constructs and
refraction, removes facts, and restarts public fact indices at zero. During
fact initialization, while the running top-level expression or `eval`
source names a template or ordered relation (in a fact assertion or as a
literal fact-query restriction; a computed restriction names nothing until it
runs) or calls a deffunction or generic function, or while a callable runs
outside any rule, it refuses before removing facts. Later actions and eligible
activations continue. At an ordinary expression root with no constructs in
use, clear removes constructs, restores `initial-fact` as f-0, and selects
MAIN; the next user assertion is f-1. As in CLIPS, a top-level expression
follows the current module while it runs: a root clear or reset, including
one inside `eval` source or a loop body, selects MAIN, and a built
`defmodule` selects itself. Dynamic source that the expression evaluates
afterwards (`build`, `eval`, `assert-string`, fact files, `save-facts`
selectors), construct lookups by name (`funcall`, `sort`, construct lists,
computed fact-query restrictions) and defglobal reads resolve in that module,
so a later `build` defines into it. The expression's own function and
template references, literal query restrictions and `bind` targets stay bound
to the module it was parsed in; after a root clear deletes that module, `bind`
also uses the new MAIN. A deffunction or method restores the current module
when it returns, so a reset or `defmodule` build in its body does not move its
caller. Inside a rule, later dynamic source still resolves in the rule's
module, where CLIPS uses MAIN after the reset. Source
clear preserves queued input, printed output, and watch settings. Unlike CLIPS, a fact asserted earlier by
the same expression through `assert-string` does not keep its relation in use.

`break` is valid only inside the body of `while`, `loop-for-count`,
`progn$`, `foreach`, or an action fact query. Loop conditions, count bounds,
multifield collection expressions, and query predicates cannot contain
`break`, even when the construct is nested inside another loop. Invalid
placement is rejected at load, including a `break` outside a loop in a rule's
`test` CE or `:`/`=` pattern constraint (`[PRCDRPSR2]`). `while` and `loop-for-count` return `FALSE`
after `break` as well as after normal completion.

### RHS evaluation errors

Ferric matches CLIPS when evaluating an RHS action fails:

- actions before the failure keep their effects;
- the first failure is retained in `action_diagnostics()` with its original
  evaluator category and cause;
- later actions in that activation do not run;
- the failing activation is consumed and the current `run()` returns
  `HaltReason::ActionError`; and
- lower-priority activations remain on the agenda. A subsequent `run()` clears
  the previous diagnostic and continues that work. `reset()` instead rebuilds
  the original agenda, so the failing activation can be encountered again.

An action error does not set the engine's persistent halt flag. `step()` still
returns the processed activation and exposes the diagnostic without consuming
the next activation.

**Example -- modify and retract:**

```clp
(deftemplate person (slot name) (slot age (default 0)))

(defrule birthday
    ?ctrl <- (do-birthday)
    ?p <- (person (name ?n) (age ?a))
    =>
    (retract ?ctrl)
    (modify ?p (age (+ ?a 1)))
    (printout t ?n " is now " (+ ?a 1) crlf))
```

---

## 16.3 Deftemplates

Ferric supports named single slots and multislots, primitive type restrictions,
allowed-value lists, numeric ranges, cardinality bounds, and static or dynamic
defaults.

```clp
(deftemplate person
    (slot name)
    (slot age (default 0))
    (multislot hobbies))
```

### Slots

- **slot**: Single-valued field. A multifield result is invalid even when it
  contains exactly one value.
- **multislot**: Multi-valued field. Supplied and default expressions splice
  multifield results before cardinality is checked.

The supported constraint attributes are `type`, `allowed-symbols`,
`allowed-strings`, `allowed-lexemes`, `allowed-integers`, `allowed-floats`,
`allowed-numbers`, `allowed-values`, `range`, and multislot `cardinality`.
Category-specific lists restrict their named value kinds; other kinds remain
permitted unless `type` excludes them. Membership distinguishes integers from
floats and positive from negative floating zero. `allowed-values` also covers
instance-name literals; fact addresses are outside its literal whitelist.
Overlapping attributes, incompatible types, reversed bounds, and invalid
literal values are rejected. `?VARIABLE` removes the corresponding value or
bound restriction, while duplicate and overlapping attributes remain invalid.

`(default <expression> ...)` evaluates once when the template is registered.
`(default-dynamic <expression> ...)` evaluates whenever an assertion omits that
slot. Supplying a slot suppresses its dynamic default; `modify` and `duplicate`
preserve unmodified source values. Function calls in a dynamic default resolve
in the template's definition module, while a direct global reference uses the
assertion's current module. A global read inside a called function follows that
function's module. Ordinary local variable reads are invalid in defaults;
lexical loop and fact-query bindings are supported. Direct `return` expressions
in defaults are rejected; a called function may return normally. A static void
default is invalid, including a void element of a static multislot default
(CLIPS 6.30 reports `[CSTRNCHK1]` after evaluating every element). A dynamic
scalar void result fills the slot with `nil`, and dynamic multislot defaults
omit void elements.
CLIPS 6.30 removes a template's old definition before it parses a redefinition's
body, so a default there that asserts the replaced template with slot syntax
(`[EXPRNPSR3]`) or queries it (`[PRNTUTIL1]`) is rejected at load in both
engines. An ordered-form `(assert (item))` is different: CLIPS creates a second,
implied `item` deftemplate, a state Ferric does not model, so Ferric rejects any
assertion or query of the template being redefined. For the same reason, Ferric
rejects a new template whose `default-dynamic` asserts its own name in ordered
form, which CLIPS accepts by adding the shadowing implied template. Ferric
checks each slot's default before evaluating it; static defaults of earlier
slots have already run, as in CLIPS. Ferric keeps the previous definition installed and redefinable,
where CLIPS has already removed it.

Without an explicit default, or with `(default ?DERIVE)`, Ferric derives a value
from the constraints. Allowed lists retain their order within a value kind;
primitive type priority selects symbols before strings, integers, and floats.
Numeric ranges prefer the lower finite endpoint, then the upper endpoint.
Multislot defaults repeat the derived element up to the minimum cardinality,
or are empty when the minimum is zero. `(default ?NONE)` requires the caller to
supply the slot.
Derived multislot allocation is limited to one million fields and 32 MiB;
larger requested defaults fail before allocation.

Ferric always validates constraints before publishing a fact, including values
computed at runtime and host assertions. CLIPS 6.30 disables dynamic checking
by default, so it can accept computed violations that Ferric rejects. Ferric
also keeps derived defaults valid in two CLIPS edge cases: a `NUMBER` range
with fractional endpoints selects an in-range integer or float, and an
untyped `allowed-values` list containing only instance names derives an allowed
instance name. CLIPS may instead derive an out-of-range truncated integer or
`nil`, respectively.

`allowed-classes`, `allowed-instance-names`, and the `deftemplate-slot-*`
introspection functions remain unsupported.

### Behavioral Notes

- Templates must be defined before use in patterns or assertions. A new
  explicit template is rejected while existing facts, seed definitions, or
  constructs depend on an ordered relation with the same local name. Earlier
  ordered uses in the same load are protected as well. Ordered relations have
  global identities in Ferric, so this guard also applies across modules and
  to module-qualified spellings; separate already-explicit template identities
  remain module-scoped. The internal `initial-fact` identity cannot be shadowed.
- Template names are module-scoped and follow import/export visibility rules.
- Asserting a template fact with missing slots uses declared defaults.
- Template facts can be matched with partial slot patterns (unmentioned
  slots match anything).

---

## 16.4 Deffacts

`deffacts` groups define facts that are asserted automatically during
`(reset)`.

```clp
(deffacts startup
    (color red)
    (color blue)
    (person (name Alice) (age 30)))
```

### Semantics

- All `deffacts` groups are processed during `(reset)`, after
  `(initial-fact)` is asserted (as in CLIPS): modules in creation order, then
  definition order within each module. Replacing a named `deffacts` moves it
  to the end of its module's order.
- Multiple `deffacts` groups may exist; all are processed.
- `deffacts` groups are module-scoped. Use `MODULE::name` syntax to define
  deffacts in a specific module context.
- On each `(reset)`, existing user facts are retracted and deffacts are
  reasserted.
- Field expressions and globals are evaluated on every reset, after globals
  are restored, in the definition's module. Loading a definition does not
  execute its expressions. A global may be defined after the deffacts, and
  replacing a called function affects the next reset. Template slot
  expressions run in slot declaration order, as for `assert`. Local variables
  and unknown calls are rejected during loading.
- An evaluation error during reset stops the reset at that fact. Facts already
  asserted, including those of earlier definitions, remain; later facts and
  definitions are not asserted. CLIPS 6.30 behaves the same, except that it
  still inserts the failing fact: an ordered fact loses all of its fields, and
  a template fact keeps its other slots while the failing slot holds an
  unspecified value. Ferric inserts none of it. Rust `reset()`
  returns `EngineError::FactInitialization { definition, reason }`; Python and
  Node raise `FerricRuntimeError`; C (and Go through it) returns
  `FERRIC_ERROR_RUNTIME_ERROR`.

---

## 16.5 Defrules

This section covers the full `defrule` syntax reference. See Section 16.2 for
high-level rule semantics.

### LHS Conditional Element Coverage

All of the following are supported:

- **Ordered patterns**: `(fact-name ?x ?y)`
- **Template patterns**: `(template (slot ?v))`
- **Fact-address binding**: `?f <- (pattern)`, including positive `and`/`or` operands
- **test CE**: `(test (> ?x 10))`
- **not CE**: `(not (pattern))`
- **exists CE**: `(exists (pattern) ...)`, including negated operands such as
  `(exists (not (P)))`. A body containing `or` is one condition, as in CLIPS:
  `(exists (or (P) (Q)))` fires once however many branches hold.
- **forall CE**: `(forall (P) (Q))` or `(forall (P) (test expression))`
- **Negated conjunction**: `(not (and (P) (Q)))`, and its double negation
  `(not (not (and (P) (Q))))`
- **Negated disjunction**: `(not (or (P) (Q)))`
- **Nested groups**: `and` and `or` nest to any depth, inside each other and
  inside `not` and `exists`, e.g. `(or (and (or (P) (Q)) (R)) (S))` or
  `(not (exists (P) (or (Q) (R))))`. Like CLIPS, Ferric flattens `and` groups
  and turns every positive `or` into rule-level alternatives, so each true
  branch of a positive `or` is its own activation.
- **Constraint connectives**: `&`, `|`, `~`

### Source and compiled network limits

Source loading rejects input above 16 MiB before parsing. Rule normalization
and disjunction expansion use checked, conservative work estimates: at most
256 CE alternatives, 16,384 expanded pattern/constraint nodes, and 8 MiB of
expanded source per rule. Both normalization passes also share a per-load
budget of 1,048,576 estimated nodes and 32 MiB of expanded source. The estimate
does not count field-level `|` constraints, which compile to one test on
their field rather than to rule alternatives. The estimate may reject an
unusually redundant `or` CE expression that could be optimized to less work;
Ferric does not perform that optimization implicitly.

Each compiled rule allows at most 64 condition nodes, counting predicates and
nested NCC wrappers/children, and each alpha path allows at most 64 constant
tests, including the children of compound field tests. When a pattern's
field disjunctions would take it past that alpha budget, the widest ones are
evaluated as match-time predicates instead. These bounds keep recursive propagation practical without adding a
resumable execution subsystem.
Boundary regressions exercise combined alpha
and beta depth, assertion, run, reset, and retraction on a 512 KiB native stack.
Over-limit constructs fail before installation; previously installed rules and
facts remain usable. Loading multiple constructs remains incremental, so a
later failure does not roll back earlier successful constructs.

These are implementation limits for the current pre-1.0 engine, not CLIPS
language limits or a guarantee that arbitrary large fact populations fit a
host's memory. The recommended snapshot envelope applies its own input and
restored-graph validation limits.

The snapshot validator allows compiled NCC dependency depth up to eight. Each
`exists` compiles to a double negation, so this covers every rule within the
four-level source nesting limit: for example `(exists (exists (exists (a))))`,
`(exists (a) (exists (b) (exists (c) (d))))`, `exists` over `or` inside further
nesting, and an `exists` at the bottom of a deep `not (and ...)` chain all
serialize and restore. Restoration also checks partner ancestry and rejects
cyclic NCC callback dependencies.

### Pattern Nesting Restrictions

Ferric accepts up to four nested `not`, `exists`, and `forall` operators along
each source pattern path. `and` and `or` do not add a level; triple and
four-deep `not` are supported. Compiled condition limits still apply after
normalization. The following forms are **not** supported:

| Unsupported Pattern | Rationale |
|---------------------|-----------|
| Five or more nested quantifiers | Reduce combined `not`/`exists`/`forall` depth to four |
| Nested `(forall ...)` | Decompose into multiple rules with phase facts |
| `forall` under `not` or `exists`, including through `and`/`or` wrappers | Universal quantification is supported only in positive rule conditions |
| `forall` with more than two operands, a non-fact first operand, or a second operand other than a fact pattern or test-only expression | Use one fact condition and one fact/test requirement |

Fact-address bindings must target fact patterns. They may occur inside positive
`and`/`or` groups, but not inside `not`, `exists`, or `forall`, or around an
entire conditional element. CLIPS 6.30 rejects the same placements
(`[RULELHS2]` inside `not`/`exists`/`forall`, `[PRNTUTIL2]` around a group), so
these are not Ferric restrictions; see the `patterns/405_assigned_*_rejected`
corpus cases. An address bound in only some branches of an `or` may be used
only where every alternative binds it: like CLIPS, Ferric rejects a RHS that
uses it (`[PRCCODE3]`). Pure-test `not`/`exists` wrappers are boolean
conditions and do not introduce fact bindings.

A variable first bound inside `not`, `exists`, or `forall`, including inside
an `or` or `and` beneath them, is local to that conditional element. A `test`,
at rule level or nested inside `not`, `exists`, or `forall`, may read only
variables bound before it in its own scope: by an earlier positive pattern at
its level or in an enclosing group, by its `forall` antecedent, or by every
branch of an earlier `or`. Like CLIPS (`[ANALYSIS4]`), Ferric rejects at load a
`test` that reads a variable local to an earlier negation or quantifier, a
variable bound nowhere before it, or one bound in only some `or` branches. It
also rejects a RHS reference unless a positive pattern binds it (`[PRCCODE3]`).

### Logical support

Every `logical` CE is rejected during rule loading, including nested and
disjunctive positions. Ferric does not track the support that CLIPS uses to
retract derived facts automatically. A rejected replacement preserves the
previous installed rule. Use ordinary stated facts and explicit retraction
when the application owns that lifecycle.

### forall Semantics

`forall` requires exactly two operands. The first is one ordered or template
fact pattern; the second is one fact pattern or a test-only expression. The
latter may combine `test` CEs with `and`, `or`, `not`, and `exists`. With a fact requirement,
it means "for every fact matching P, there also exists a matching Q." With a
test requirement, the expression must hold for every matching P. Variables
bound by P are available to its requirement and do not escape the `forall`.

Vacuous truth: when no facts match P, the forall condition holds:

```clp
;; Fires because no (task ?x) facts exist
(defrule all-tasks-done
    (ready)
    (forall (task ?x) (done ?x))
    =>
    (printout t "all done" crlf))
```

For example, `(forall (task ?p) (test (> ?p 0)))` holds when every matching
task has positive priority, including when there are no tasks.

### Module Scoping

- Rules are scoped to the module in which they are defined.
- Only rules in the current focus-stack module are eligible to fire.
- Module-qualified syntax: `(defrule MODULE::rule-name ...)`
- Focus stack controls module execution order via `(focus MODULE)` action.

**Example:**

```clp
(defmodule A)
(defmodule B)

(defrule MAIN::start
    (go) => (focus A) (printout t "MAIN" crlf))

(defrule A::do-a
    (initial-fact) => (focus B) (printout t "A" crlf))

(defrule B::do-b
    (initial-fact) => (printout t "B" crlf))
```

### Pattern Restriction Diagnostics

Unsupported constructs produce diagnostics with the restriction and source
location. `ferric run` and `ferric check` retain this detail in both ordinary
stderr and the `message` field of `--json` diagnostics. Ferric does not silently
ignore invalid patterns.

---

## 16.6 Defglobals

Ferric supports `defglobal` with the `?*name*` naming convention.

Each named global is installed after its initializer succeeds. Earlier names in
one `defglobal` group remain available to later initializers. If an initializer
fails, including when a query in it names an unknown deftemplate, its name and
later names in that group are not installed; earlier globals and following
top-level constructs retain their incremental load behavior.

Known difference: callable bodies are validated after the whole source is
read, but initializers run in source order. An initializer can therefore call
a deffunction or method defined earlier in the same load before that
callable's validation finishes. If the callable is then rejected (for
example, for an unknown call in a branch that never ran), the global keeps
the initializer's value and any side effects of the call remain. CLIPS 6.30
rejects the callable first and then rejects the initializer, so the global is
never defined.

```clp
(defglobal ?*count* = 0)
(defglobal ?*label* = "default")
```

### Module Scoping

- Globals are scoped to the module in which they are defined.
- Cross-module access requires `import`/`export` declarations.
- Module-qualified references use `?*MODULE::name*` syntax:

```clp
(defmodule CONFIG (export defglobal ?ALL))
(defglobal ?*base-value* = 10)

(defmodule MAIN (import CONFIG defglobal ?ALL))

(defrule MAIN::update
    (run-it)
    =>
    (bind ?*CONFIG::base-value* (* ?*CONFIG::base-value* 3))
    (printout t "value: " ?*CONFIG::base-value* crlf))
```

### Mutation via bind

- `(bind ?*name* <value>)` updates an existing global variable.
- A global target must already exist; global `bind` does not create a new global.
- Globals are accessible from rule RHS actions and function bodies.

### Reset Behavior

On `(reset)`, globals are restored to their declared initial values. Ferric
restores the value each initializer produced at load instead of re-evaluating
it (CLIPS 6.30 re-evaluates; [#451](https://github.com/plx/ferric-rules/issues/451)),
so an initializer's queries and engine effects are not repeated and a fact
address it captured becomes stale.

---

## 16.7 Deffunctions

Ferric supports user-defined functions via `deffunction`.

Within a deffunction or method, `(bind ?name <value>)` creates or updates a
local that lasts for the rest of the call, and can rebind a parameter; each
call has its own locals. `(bind ?name)` removes the local, so a parameter reads
its argument again. Loop iterators cannot be rebound.

```clp
(deffunction double (?x) (* ?x 2))
(deffunction greet (?name)
    (str-cat "Hello, " ?name "!"))
```

### Parameters

- **Regular parameters**: `?x`, `?y`
- **Wildcard parameter**: `$?rest` (collects remaining arguments as a
  multifield; must be the last parameter)

A wildcard flattens multifield arguments into its collected sequence. A fixed
parameter preserves a supplied multifield as one argument. For example,
`(?first $?rest)` called with `(create$ 1 2) 3 (create$ 4 5)` binds `?first`
to `(1 2)` and `?rest` to `(3 4 5)`. An empty multifield adds no elements to
the wildcard binding.

### Evaluation

Function bodies are expression sequences. The value of the last expression is
the return value; an empty body returns `FALSE`. `(return)` and
`(return <expression>)` immediately unwind the current deffunction or
generic-method call; an inner callable's return does not
unwind its caller. A top-level return is an evaluation error. On a rule RHS,
`return` follows CLIPS behavior and stops only the remaining actions in that
activation.

Loop bodies support `break`, which exits their nearest loop and continues the
current call. Unknown function calls in deffunction and method bodies are
load errors. Ferric resolves calls against the declarations in the loaded
source, including forward references; CLIPS requires a prior declaration,
which can be an empty deffunction body replaced by a later definition.

Bodies evaluate ordinary expressions and engine effects, including assertions,
retraction, modification, duplication, action fact queries, halt, focus, and
reset. Effects act on the calling engine immediately and return their CLIPS
values. Read-only matching contexts do not permit engine mutation.

### Module Scoping

- Functions are registered in their defining module.
- Cross-module calls require `import`/`export`:

```clp
(defmodule UTILS (export deffunction ?ALL))
(deffunction square (?x) (* ?x ?x))

(defmodule MAIN (import UTILS deffunction ?ALL))
(defrule MAIN::compute
    (compute) => (printout t "result: " (square 5) crlf))
```

### Recursive Calls

Recursive calls are supported with a requested maximum call depth, capped at
32 effective callable frames in every build profile. Active expression evaluation
is bounded at 64 frames across nested bodies and calls; expression translation
and cloning accept trees up to 16 levels. Limit violations report an execution
or load diagnostic before continuing recursive evaluation. Callable bodies
are checked before registration; a failed function redefinition or implicit
generic registration preserves the previous registry state.

Known difference: match conditions and deffacts or defglobal initializers
evaluated by an engine effect (`assert`, `retract`, `modify`, `duplicate`,
`reset`), or by the source loading of `build`, continue that call's depth
rather than starting from zero. An effect run from deep inside nested calls can therefore push such
a condition or initializer past these limits. Ferric reports that as a
diagnostic, and an affected match condition does not match; CLIPS 6.30 has no
such limit and evaluates them normally.

Known difference: when a deffunction redefinition fails, CLIPS 6.30 removes
the deffunction, so later callers are rejected at load; Ferric keeps the
previous body and its callers still load. Likewise, CLIPS 6.30 removes an
existing defrule before parsing its redefinition, so a replacement that fails
any load check leaves no rule of that name; Ferric keeps the previous rule
installed and it still fires. Both keep the previous method when
a `defmethod` redefinition fails. Until a rejected callable is removed at the
end of the load, it still counts as using the templates it references, so a
`deftemplate` replacement later in the same source is refused with
`[CSTRCPSR4]` where CLIPS accepts it.

Known difference: an undefined global variable in a deffunction body, a
defmethod body, or a method restriction query does not stop the load. Ferric
reports it only when the expression is evaluated; CLIPS 6.30 rejects the
construct at load with `[GLOBLPSR1]`.

Embedding note: release recursion regressions run on 512 KiB native stacks;
unoptimized development regressions use 2 MiB. These are supported test
baselines, not a promise for arbitrarily small host stacks. Larger requested
call limits remain readable and persistable but cannot raise the enforced
ceiling. This pre-1.0 change replaces unsafe high-depth behavior with explicit
execution errors.

### Conflict with defgeneric

Defining a `deffunction` and `defgeneric` with the same name in the same
module is a compile error with a diagnostic message.

---

## 16.8 Generic Functions and Methods

Ferric supports generic function dispatch via `defgeneric` and `defmethod`.

### Syntax

```clp
(defgeneric describe)
(defmethod describe ((?x INTEGER)) (str-cat "integer: " ?x))
(defmethod describe ((?x STRING)) (str-cat "string: " ?x))
(defmethod describe ((?x NUMBER)) (str-cat "number: " ?x))
(defmethod describe ((?x SYMBOL (eq ?x special))) "special symbol")
(defmethod describe (($?items SYMBOL)) (length$ ?items))
```

### Method Specificity

Methods are ranked by type specificity. More specific types win:
`INTEGER` > `NUMBER`, `FLOAT` > `NUMBER`, etc. When multiple methods could
match, the most specific applicable method is selected. Restrictions are
compared left to right, with a wildcard counting as its method's last
restriction. A wildcard loses at once to a regular parameter in the same
position when that parameter's method has no wildcard. Otherwise, as in
CLIPS 6.30, the two type lists are compared in written order:

- Any type list outranks an unrestricted parameter, so a type restriction
  outranks an otherwise unrestricted parameter with a query.
- At the first position where one listed type is a subclass of the other in
  the CLIPS class hierarchy, the subclass wins: `INTEGER` and `FLOAT` under
  `NUMBER`, and `SYMBOL` and `STRING` under `LEXEME`. `INSTANCE-NAME` is not
  a subclass of `SYMBOL`. So
  `((?x INTEGER SYMBOL))` outranks `((?x NUMBER))`, although it covers more
  types.
- Otherwise the shorter list wins: `((?x INTEGER))` outranks
  `((?x INTEGER SYMBOL))`.
- Lists of the same length that differ anywhere, such as `(INTEGER SYMBOL)`
  and `(INTEGER STRING)`, or `(INTEGER SYMBOL)` and `(SYMBOL INTEGER)`, leave
  the two methods unranked: neither outranks the other, and their queries and
  later restrictions are not compared.

A query adds specificity only when the type lists are identical. When every
shared position ties, a method without a wildcard wins, then the method with
more restrictions. For example, `(($?xs INTEGER))` outranks `(?x $?xs)`, while
`(?x ?y)` outranks `(($?xs INTEGER))` and `(?x)` outranks `(?x $?xs)`.

This ranking can be cyclic: `(($?x INTEGER))` outranks `((?x NUMBER) $?y)`,
which outranks `(?x)`, which outranks `(($?x INTEGER))`. As in CLIPS 6.30,
methods are not sorted. Each method is inserted before the first existing
method it outranks, or after all of them, so the final order can depend on
definition order, both for cyclic rankings and for unranked methods. Ferric
inserts methods in index order, which is definition order unless explicit
indices are given out of order.

Known differences:

- When explicit indices are given out of definition order, CLIPS 6.30 still
  inserts methods in definition order. Its dispatch order can then differ from
  Ferric's whenever those methods do not strictly outrank each other, either
  because the ranking is cyclic or because their restrictions differ without
  one outranking the other. For example, after
  `(defmethod g 2 ((?x INTEGER SYMBOL)) A)` and
  `(defmethod g 1 ((?x INTEGER FLOAT)) B)`, CLIPS 6.30 returns `A` for
  `(g 1)` and Ferric returns `B`.
- A `defmethod` whose restrictions are identical to an existing method's adds
  a second method after it, where CLIPS 6.30 replaces the existing method.
- Only `INTEGER`, `FLOAT`, `NUMBER`, `SYMBOL`, `STRING`, `LEXEME`,
  `INSTANCE-NAME`, `MULTIFIELD`, `EXTERNAL-ADDRESS` and `FACT-ADDRESS` match
  as type restrictions. Other CLIPS class names, such as `PRIMITIVE`,
  `OBJECT`, `ADDRESS`, `INSTANCE` and `INSTANCE-ADDRESS`, load as method
  restrictions but never match, so a restriction that names only such classes
  makes its method never applicable. In particular, an `ADDRESS`, `PRIMITIVE`
  or `OBJECT` restriction does not match a fact address in Ferric, although
  CLIPS 6.30 matches it.

A parameter query follows its optional type restrictions. It is a function
call or a global variable such as `((?x INTEGER ?*enabled*))`; a global is
read again at each dispatch. A query can reference any method parameter,
including later ones: all arguments are bound before queries run. Queries
use CLIPS truthiness and are evaluated only as dispatch searches for the next
applicable method. For each candidate, dispatch walks
the arguments left to right: it checks the argument's type and then runs its
restriction's query, stopping at the first failure. A query therefore runs,
with its side effects, even when a later argument's type rules the method
out. Excess arguments share the wildcard restriction, so a wildcard query
runs once per excess argument and not at all when there are none.
Lower-priority method queries are not evaluated after a match is found. A
query error stops dispatch instead of trying a fallback. Queries may bind
globals, but cannot bind local variables or parameters. `return` anywhere in
a query is rejected at load (`[PRCDRPSR2]`). Undefined variables
and templates unavailable when the method is defined are load errors. An
undefined global in a query is reported only when the query runs, which stops
dispatch; CLIPS 6.30 rejects the method at load (`[GLOBLPSR1]`), as it does
for an undefined global in a deffunction or method body.

```clp
(defgeneric classify)
(defmethod classify ((?x NUMBER)) (str-cat "number"))
(defmethod classify ((?x INTEGER)) (str-cat "integer"))
;; (classify 5) => "integer" (INTEGER is more specific than NUMBER)
```

### call-next-method

Within a method body, `(call-next-method)` invokes the next less-specific
applicable method in the dispatch chain. Each invocation searches again and
reevaluates candidate queries, including their side effects:

```clp
(defgeneric annotate)
(defmethod annotate ((?x NUMBER)) (str-cat "num(" ?x ")"))
(defmethod annotate ((?x INTEGER)) (str-cat "int+" (call-next-method)))
;; (annotate 7) => "int+num(7)"
```

`(next-methodp)` tests for another applicable method without advancing the
current chain; its parameter queries can have side effects.
`(override-next-method <args>...)` searches less-specific methods using replacement
arguments. `(call-specific-method <generic> <index> <args>...)` invokes the
selected method when its restrictions match. Nested calls restore the caller's
method-chain context afterward.

### Wildcard Parameters

Methods support wildcard parameters for variable-arity dispatch, with the
same flattening behavior as deffunction wildcards. Optional type restrictions
apply to each original supplied argument before flattening; an `INTEGER`
wildcard therefore rejects a multifield argument containing integers. A call
with no remaining arguments satisfies the type restriction vacuously, while
an explicitly supplied empty multifield still has type `MULTIFIELD`. The
wildcard query and method body receive the flattened binding.

### Auto-indexing

Method indices are auto-assigned when not explicitly provided. Explicit
indices are also supported.

### Module Scoping

Generic functions are module-scoped and follow the same import/export
visibility rules as deffunctions.

### Interaction with deffunction

A `defgeneric` and `deffunction` with the same name in the same module
is a compile error with a diagnostic message.

### Method Bodies

Method bodies use the same evaluator expression model as deffunction bodies,
including empty bodies returning `FALSE` and load validation of function calls.
Fact mutation and execution/focus control are available directly in the body.

---

## 16.9 Modules

Ferric supports the CLIPS module system with `defmodule`, import/export
visibility, and focus-stack-driven execution.

### defmodule Syntax

```clp
(defmodule SENSORS (export deftemplate reading))
(defmodule MAIN (import SENSORS deftemplate reading))
```

### Export/Import

- `(export <construct-type> ?ALL)` -- export all constructs of a type
- `(export <construct-type> <name>)` -- export a specific construct
- `(import <module> <construct-type> ?ALL)` -- import all exports of a type
- `(import <module> <construct-type> <name>)` -- import a specific construct

Supported construct types for import/export: `deftemplate`, `deffunction`,
`defglobal`, `defgeneric`.

### Module-Qualified Names

Constructs can be referenced with `MODULE::name` syntax:

```clp
(deffacts MAIN::startup (go))
(defrule MAIN::start (go) => (printout t "started" crlf))
(bind ?*CONFIG::base-value* 42)
```

### Focus Stack

- `MAIN` is the default focus module after `(reset)`.
- `(focus MODULE)` pushes a module onto the focus stack. Pushing the module
  already at the top leaves the stack unchanged; a module deeper in the stack
  may be pushed again. The host `push_focus`/`pushFocus` APIs behave the same.
- Only rules in the current focus-stack module are eligible to fire.
- When a module's agenda is empty, it is popped and the next module resumes.

### Template declaration spellings across modules

CLIPS 6.30 accepts the same unqualified template declaration name in different
modules. Ferric currently requires distinct public declaration spellings: after
`(defmodule A) (deftemplate item ...)`, declare a second identity as
`(defmodule B) (deftemplate B::item ...)`. Unqualified references inside module B
still resolve its local template. A second conflicting unqualified declaration
is rejected before installing metadata; previously it could overwrite the
public name index and make snapshots invalid. This is an explicit module support
limitation, not a claim of CLIPS equivalence. Existing qualified identities and
same-module unused-template replacement retain their behavior.

### Facts Are Global

Facts exist in a single global working memory. Module scoping affects only
which rules are eligible to fire, not which facts are visible.

---

## 16.10 Standard Library

Ferric implements the functions below, with CLIPS 6.30 reference cases for
supported behavior. Compatibility boundaries are called out here and in §16.11;
this is not a claim that every built-in or argument combination is identical.

Known arity and literal argument types are checked while loading constructs.
For example, `(abs 1 2)`, `(eq a)`, and `(min 1 a)` reject the containing rule
with an `ARGACCES4` or `ARGACCES5` diagnostic. An invalid replacement leaves
Ferric's previous rule or callable installed; CLIPS 6.30 removes the old
defrule or deffunction before parsing its redefinition, so its rule no longer
fires (see the known difference in Recursive Calls). Independent later
definitions can still load. Computed values and dynamically selected `funcall`
targets retain runtime checks. Ferric does not check the declared result type
of a nested built-in call at load: CLIPS rejects `(abs (str-cat a))`,
`(str-length (+ 1 2))` or `(+ 1 (sym-cat a))` with the containing rule, and
later rules still run, whereas Ferric loads the rule and stops the run when the
call is evaluated. An explicit `expand$` defers the surrounding call's arity check until
its fields have been expanded.

### Math Functions

| Function | Description | Example |
|----------|-------------|---------|
| `+` | Addition | `(+ 1 2)` => `3` |
| `-` | Subtraction | `(- 10 3)` => `7` |
| `*` | Multiplication | `(* 4 5)` => `20` |
| `/` | Division | `(/ 10 3)` => `3.333...` |
| `div` | Integer division | `(div 10 3)` => `3` |
| `mod` | Remainder; FLOAT `a - trunc(a / b) * b` if either operand is FLOAT, as in CLIPS (not C `fmod`) | `(mod 7.5 2)` => `1.5` |
| `abs` | Absolute value | `(abs -5)` => `5` |
| `min` | Minimum | `(min 3 7)` => `3` |
| `max` | Maximum | `(max 3 7)` => `7` |
| `**` | Power | `(** 2 10)` => `1024.0` |
| `sqrt` | Square root | `(sqrt 16)` => `4.0` |
| `round` | Round to nearest integer; half ties choose the lower integer, INTEGER inputs stay exact | `(round 2.5)` => `2`; `(round -2.5)` => `-3` |
| `ceiling` | Round up to integer | `(ceiling 3.1)` => `4` |
| `floor` | Round down to integer | `(floor 3.9)` => `3` |
| `pi` | Pi constant | `(pi)` => `3.14159...` |
| `exp` | e^x | `(exp 1)` => `2.718...` |
| `log` | Natural logarithm | `(log 2.718)` => `~1.0` |
| `log10` | Base-10 logarithm | `(log10 100)` => `2.0` |
| `sin`, `cos`, `tan` | Trigonometric | `(sin 0)` => `0.0` |
| `asin`, `acos`, `atan` | Inverse trigonometric | `(acos 1)` => `0.0` |
| `atan2` | Two-argument arctangent | `(atan2 1 1)` => `0.785...` |
| `sinh`, `cosh`, `tanh` | Hyperbolic | `(cosh 0)` => `1.0` |
| `asinh`, `acosh`, `atanh` | Inverse hyperbolic | `(asinh 0)` => `0.0` |
| `deg-rad`, `rad-deg` | Angle conversion | `(deg-rad 180)` => `3.14159...` |
| `deg-grad`, `grad-deg` | Degree/gradian conversion | `(deg-grad 90)` => `100.0` |

`min` and `max` return the selected operand with its own type, the first on a
tie: `(max 1 1.0)` is `1`. For a FLOAT, `round` computes `ceil(x - 0.5)` as
CLIPS does, so `(round -0.49999999999999994)` is `-1`.

Domain errors in `sqrt`, `asin`, `acos`, `acosh`, `atanh`, `log`, `log10`,
and `**` stop the current run with an `EMATHFUN1` diagnostic. Zero logarithm
arguments report `EMATHFUN2`; a `tan` asymptote reports `EMATHFUN3`. Following
CLIPS, overflow from functions such as `(exp 1000)` can still return `inf.0`,
and a NaN argument is not out of range, so these functions return `nan.0`
for it (except `**` with a negative base). These errors preserve output already produced and stop later RHS actions.

### Type Conversion

| Function | Description |
|----------|-------------|
| `integer` | Convert to integer (truncates floats) |
| `float` | Convert to float |

### Comparison Functions

| Function | Description |
|----------|-------------|
| `=` | First numeric operand equals every subsequent operand |
| `!=` / `<>` | First numeric operand differs from every subsequent operand |
| `>`, `<`, `>=`, `<=` | Each adjacent numeric pair satisfies the ordering |
| `eq` | First operand equals every subsequent operand (type-sensitive) |
| `neq` | First operand differs from every subsequent operand (type-sensitive) |

Numeric comparisons take two or more operands and stop at the first failed
comparison: `(< 2 1 (later-call))` returns FALSE without calling `later-call`.
`eq` and `neq` also take two or more operands and stop at the first failed
comparison. `(neq a b b)` is TRUE: the later operands need not differ from
each other.

### Logical Functions

| Function | Description |
|----------|-------------|
| `and` | Logical AND |
| `or` | Logical OR |
| `not` | Logical NOT |

### Predicate / Type-Checking Functions

| Function | Returns TRUE when |
|----------|-------------------|
| `integerp` | Argument is an INTEGER |
| `floatp` | Argument is a FLOAT |
| `numberp` | Argument is INTEGER or FLOAT |
| `symbolp` | Argument is a SYMBOL |
| `stringp` | Argument is a STRING |
| `lexemep` | Argument is a SYMBOL or STRING |
| `instance-namep` | Argument is an INSTANCE-NAME |
| `multifieldp` | Argument is a MULTIFIELD |
| `evenp` | Argument is an even integer |
| `oddp` | Argument is an odd integer |

### String / Symbol Functions

| Function | Description | Example |
|----------|-------------|---------|
| `str-cat` | Concatenate to string | `(str-cat "a" "b")` => `"ab"` |
| `sym-cat` | Concatenate to symbol | `(sym-cat a b)` => `ab` |
| `str-length` | Character length of a STRING, SYMBOL or INSTANCE-NAME | `(str-length "hello")` => `5`; `(str-length [abc])` => `3` |
| `sub-string` | Extract a STRING from a STRING, SYMBOL or INSTANCE-NAME (1-indexed, inclusive, clipped bounds) | `(sub-string 0 2 abc)` => `"ab"` |
| `str-index` | First substring position (1-indexed), FALSE if not found; empty needle returns length + 1 | `(str-index "" "abc")` => `4` |
| `upcase` | Convert ASCII letters to uppercase (preserves type) | `(upcase [abc])` => `[ABC]`; `(upcase "é")` => `"é"` |
| `lowcase` | Convert ASCII letters to lowercase (preserves type) | `(lowcase "HELLO")` => `"hello"` |
| `str-compare` | Lexicographic comparison (-1, 0, or 1) | `(str-compare "a" "b")` => `-1` |
| `string-to-field` | First CLIPS field of a STRING, SYMBOL or INSTANCE-NAME | `(string-to-field "42 rest")` => `42` |
| `explode$` | Every CLIPS field of a STRING, as a multifield | `(explode$ "a \"b c\" 3")` => `(a "b c" 3)` |
| `symbol-to-instance-name` | SYMBOL to INSTANCE-NAME | `(symbol-to-instance-name x)` => `[x]` |
| `instance-name-to-symbol` | INSTANCE-NAME or SYMBOL to SYMBOL | `(instance-name-to-symbol [x])` => `x` |
| `funcall` | Call function by name at runtime | `(funcall + 1 2)` => `3` |

`str-cat` and `sym-cat` require at least one argument. They accept STRING,
SYMBOL, INSTANCE-NAME, INTEGER, and FLOAT values; a multifield, address, or
VOID result is an error. A failing operand prevents evaluation of later
operands. Use `implode$` when a multifield's printed fields are wanted.

Character-oriented string functions read an INSTANCE-NAME without brackets
and count Unicode scalar values. `length` and `length$` instead accept a
MULTIFIELD, SYMBOL, or STRING: they count fields for a multifield and bytes
for a lexeme. Both return `6` for `"héllo"`.

`string-to-field`, `explode$` and `read` use the CLIPS 6.30 field scanner:
quoted strings (with `\` escapes) are one STRING field, numbers keep their
INTEGER or FLOAT type, `[name]` is an INSTANCE-NAME, `;` starts a comment, and
tokens that are not values, such as `(` or `?x`, become STRINGs of their
spelling. `string-to-field` ignores the text after the first field and returns
`EOF` for empty input. Integers outside the 64-bit range saturate, and an
unterminated string keeps its text; for these CLIPS writes a `[SCANNER1]`
notice to the `wwarning` or `werror` router, and so does Ferric (`ferric run`
prints only `t`, so it does not show them). A string that ends in a backslash
at the end of input gives CLIPS a byte that is not UTF-8, which Ferric holds as
U+FFFD.

Source text and `load-facts` share the field scanner's numeric grammar. Forms
such as `1.`, `.5`, and `1.e3` are floats; `1st`, `0x10`, and incomplete
exponents such as `5e` are single symbols. Source integers outside the signed
64-bit range saturate too, but the source lexer has no warning channel and
does not emit the `[SCANNER1]` notice. This includes `load-facts` at run
time, where CLIPS prints that warning within the program's output for each
overflowing integer and Ferric prints nothing. Comments end at CR or LF,
including files that use CR-only line endings.

### Multifield Functions

| Function | Description | Example |
|----------|-------------|---------|
| `create$` | Create a multifield | `(create$ a b c)` |
| `implode$` | Convert multifield fields to a STRING | `(implode$ (create$ a 3))` => `"a 3"` |
| `length`, `length$` | Multifield field count or SYMBOL/STRING byte length | `(length$ (create$ a b c))` => `3` |
| `nth$` | Get nth element (1-indexed), `nil` if absent | `(nth$ 2 (create$ a b c))` => `b` |
| `member$` | Find element position or contiguous subsequence range | `(member$ b (create$ a b c))` => `2` |
| `subsetp` | Subset test | `(subsetp (create$ a) (create$ a b))` => `TRUE` |
| `insert$` | Insert values at position | `(insert$ (create$ a c) 2 b)` => `(a b c)` |
| `delete$` | Remove range (1-indexed, inclusive) | `(delete$ (create$ a b c) 2 2)` => `(a c)` |
| `replace$` | Replace range with values | `(replace$ (create$ a b c) 2 2 x)` => `(a x c)` |
| `first$` | First element as multifield | `(first$ (create$ a b c))` => `(a)` |
| `rest$` | All but first as multifield | `(rest$ (create$ a b c))` => `(b c)` |
| `expand$` | Expand a multifield into an ordinary call's argument list | `(+ (expand$ (create$ 1 2 3)))` => `6` |
| `delete-member$` | Remove matching fields or contiguous subsequences | `(delete-member$ (create$ a b a c) a)` => `(b c)` |
| `replace-member$` | Replace matching fields or subsequences | `(replace-member$ (create$ a b a) x a)` => `(x b x)` |
| `sort` | Stable predicate sort of scalar and multifield arguments | `(sort > (create$ 3 1 2))` => `(1 2 3)` |

`nth$` returns `nil` for a position that is zero, negative or past the end.
Literal FLOAT positions are rejected at load; computed FLOAT positions are
truncated at runtime, following CLIPS. `member$` returns an INTEGER for a single-field match and a `(start end)`
pair for a longer contiguous one: `(member$ (create$ b c) (create$ a b c d))`
is `(2 3)`.

`implode$` quotes STRING fields and escapes their quotes and backslashes, so
`explode$` of its result gives back the original fields:
`(implode$ (create$ a "b c" 3))` is `"a \"b c\" 3"`.

`(sort <predicate> <value>...)` sorts the fields of its values with a function
named by a SYMBOL (a builtin, deffunction or defgeneric). As in CLIPS, the
predicate answers "should these two be exchanged?", so `(sort > (create$ 3 1
2))` returns `(1 2 3)`. It is a stable merge sort that calls the predicate in
the same order as CLIPS 6.30. A predicate error or an unknown name is an action
error; CLIPS instead reports an unknown name and continues with `FALSE`.

Explicit `expand$` operands are evaluated first, in source order; remaining
operands keep their ordinary evaluation order and short-circuit behavior.
Expansion applies to function argument lists, not standalone body actions or
raw `assert` fact fields. Ordinary `progn` evaluates its expressions in order
and returns the last value, or FALSE for an empty body; it does not accept
sequence expansion directly in its body.

`funcall` evaluates all its operands before invoking the selected function,
including operands of short-circuit targets such as `and` and `eq`. Argument
effects can install a new callable definition before that invocation begins.
A module-qualified name, or a name that reaches no visible function, prints
CLIPS's `[ARGACCES5]` notice and returns FALSE without evaluating the
operands; as in CLIPS 6.30, `funcall` never resolves a qualified name, even one
that names a visible function.

### Construct Introspection

| Functions | Result |
|-----------|--------|
| `get-deftemplate-list`, `get-defglobal-list`, `get-defrule-list` | Names owned by the current or specified module; `*` returns qualified names from all modules |
| `deftemplate-slot-names` | Slot names, including `implied` for an ordered relation |
| `deftemplate-slot-existp`, `deftemplate-slot-multip`, `deftemplate-slot-singlep` | Slot existence or cardinality kind |
| `deftemplate-slot-types`, `deftemplate-slot-allowed-values` | Declared types or allowed values |
| `deftemplate-slot-range`, `deftemplate-slot-cardinality` | Numeric or field-count bounds |
| `deftemplate-slot-defaultp` | `static`, `dynamic`, or FALSE when no default exists |
| `deftemplate-slot-default-value` | Stored static value, evaluation of the dynamic default, or `?NONE` |

Ordered relation declarations remain available to introspection after their
last fact is retracted and across reset. Parsing an assertion in `eval` or
`assert-string` source declares its relation even when the assertion does not
run, as loading source does. The built-in `initial-fact` is a deftemplate
without slots, not an ordered relation. Querying a dynamic default evaluates
its expression, including side effects, and returns its value without checking
the slot's constraints or shape: a void or multifield result of a single-field
slot's default is returned as is, and a multislot default omits void elements.
Merely listing slots or asking the default kind does not evaluate it.

`deftemplate-slot-names` loads with an INTEGER or SYMBOL literal argument, as
in CLIPS 6.30. As in CLIPS, a missing deftemplate prints a `PRNTUTIL1` notice,
and a template or module argument that is not a SYMBOL when evaluated prints an
`ARGACCES5` notice; the rule continues. The query then returns `()` for
`deftemplate-slot-types`, `-allowed-values`, `-range`, `-cardinality` and the
construct lists, and FALSE for the other template queries. A slot name that is
not a SYMBOL stops the rule.

### Dynamic Source, Randomness, and Time

| Function | Behavior |
|----------|----------|
| `eval` | Evaluate the first expression in a STRING or SYMBOL and return its value |
| `build` | Load the first construct in a STRING or SYMBOL; return TRUE or FALSE |
| `assert-string`, `str-assert` | Assert the first fact in a STRING; return its address or FALSE for a suppressed duplicate |
| `seed` | Set the random stream from one INTEGER seed; returns VOID |
| `random` | Return an INTEGER, optionally within inclusive INTEGER bounds |
| `time` | Return wall-clock seconds since the Unix epoch as a FLOAT |

Dynamic source uses the active module and does not capture surrounding local
variables or method-chain state. Normal expression, source-size, and execution
limits still apply. `build` is unavailable while another construct is being
loaded, including in a construct initializer; pinned CLIPS 6.30 crashes for
these reentrant initializer cases. Use a host load call or a later RHS action.
A failed `build` returns FALSE and reports its load diagnostics; earlier effects
of incremental loading are retained. A currently executing rule or callable
cannot be replaced, including through a helper's `build` call; the original
definition remains installed. Likewise, while a fact is being asserted,
modified, or duplicated, a `build` in its slot or field expressions cannot
redefine its template or give its ordered relation an explicit template. As in
CLIPS, code that is running keeps every template and ordered relation it names
in use: all facts of one `assert` command, an `eval` or `assert-string`
expression, and the actions of a rule that has removed itself with
`undefrule`. CLIPS instead refuses to remove an executing rule.

Each engine owns its random state, and snapshots preserve that state. Seeded
explicit draws match the pinned glibc-based CLIPS 6.30 reference where its
signed range arithmetic is defined. Ferric supports the full inclusive i64
range using widened arithmetic. CLIPS 6.30 overflows when the inclusive width
exceeds `i64::MAX`, and can crash; those ranges are not a conformance claim.
CLIPS 6.30 also draws one value for every new activation, whatever the
strategy; Ferric consumes the stream only for explicit `random` calls. A seeded
sequence therefore matches only while no activation is created between `seed`
and a draw (see §16.11). This does not add support for the Random conflict
strategy.
`time` is inherently nondeterministic. Reversed `random` bounds produce a
recoverable `MISCFUN3` notice and return the unbounded draw. A wrong argument
count that reaches execution likewise consumes and returns a draw, emits
`MISCFUN2`, and skips its operands. Literal calls with more than two arguments
are rejected at load, following CLIPS's source restriction.

### Fact Introspection Functions

| Function | Description | Example |
|----------|-------------|---------|
| `fact-existp` | Whether a fact address or index is live | `(fact-existp 1)` => `TRUE` |
| `fact-index` | Public assertion index (zero for the protected initial fact, -1 for a retracted address) | `(fact-index ?f)` => `1` for the first user fact |
| `fact-relation` | Get relation name as symbol | `(fact-relation 1)` => `person` |
| `fact-slot-value` | Get named slot value | `(fact-slot-value 1 name)` => `"Alice"` |
| `fact-slot-names` | Get slot names as multifield | `(fact-slot-names 1)` => `(name age)` |

#### Fact addresses

A fact address (`?f <- (...)`, a query member, or an element of a `find-fact`
result) has type `FACT-ADDRESS`. It prints `<Fact-N>` using the public assertion
index, works with `retract`, `modify`, `duplicate`, and fact introspection,
and compares by identity with `eq` and `neq`. It is neither an INTEGER nor a
NUMBER: arithmetic, `str-cat`, and `sym-cat` reject it. Addresses can be stored
in fact fields, slots, multifields, and globals; engine snapshots preserve
their identities.

An INTEGER designator always means a public fact index. It cannot be decoded
as an internal address. `(fact-relation (fact-index ?f))` therefore names
`?f`'s relation while the fact is live.

Retraction preserves an address's printed `<Fact-N>` identity. `fact-index`
then returns `-1`; `fact-existp`, `fact-relation`, `fact-slot-names`, and
`fact-slot-value` return `FALSE`. An address does not become an address to a
replacement fact. A runtime assertion using the derived default for a
`FACT-ADDRESS` slot receives `<Dummy Fact>`, a distinct address value with no
referenced fact; its introspection results are the same as a stale address.

The run-time rules below apply to computed designators and slot names. As in
CLIPS 6.30, a literal STRING or FLOAT designator to `retract`, `fact-existp`,
`fact-relation`, `fact-slot-names` or `fact-slot-value`, any literal argument
to `fact-index`, and a literal STRING slot name to `fact-slot-value` reject the
containing rule at load with `ARGACCES5`. A SYMBOL literal designator loads and
is checked when evaluated.

A missing or negative fact index, or a designator that is neither an address
nor an INTEGER, also returns `FALSE` from `fact-existp`, `fact-relation`,
`fact-slot-names`, and `fact-slot-value`, and the rule continues. `fact-index`
returns `-1` for any argument that is not a fact address, including an INTEGER.
`fact-slot-value` resolves its designator before evaluating the slot argument,
so the slot argument is not evaluated when the designator names no live fact.
As in CLIPS, a slot argument that resets or retracts the fact still reads the
record the designator named, not a fact asserted afterwards. On a live fact, an invalid slot name or a slot argument that is not a symbol,
string, or instance name stops the rule. CLIPS 6.30 accepts only a SYMBOL slot
argument: it rejects a literal STRING at load, as Ferric does, and stops the
rule for a computed one, whereas Ferric also accepts a computed STRING or
INSTANCE-NAME slot name.

`retract` skips a missing index or a stale address and goes on to its next
target. A negative index ends that `retract` call: later targets are neither
evaluated nor retracted, and the rule continues. A target of any other type
stops the rule, after the remaining targets have been retracted. As in CLIPS,
a later target that calls a deffunction or generic function is not called and
retracts nothing, while variables, literals, and builtin calls are still
evaluated and retracted. CLIPS 6.30 also fails some builtin targets in that
state, such as `progn$`, `switch`, `funcall`, or `(nth$ 1 (create$ ?f))`, and
keeps their facts, whereas Ferric still evaluates and retracts them. A
deffunction or generic function reached through such a builtin, whether nested
in it or called through `funcall`, still runs in Ferric; CLIPS 6.30 does not
call it. `modify` and `duplicate` given a missing index do nothing, without
evaluating their slot overrides, and the rule continues; a negative index or a
target of another type stops the rule, as in CLIPS. Given a stale address,
`modify` and `duplicate` also stop the rule, whereas CLIPS 6.30 asserts a new
fact from the retracted fact's data.

CLIPS emits recoverable `[PRNTUTIL1]` or `[ARGACCES5]` notices for these
calls; Ferric omits those notices.

`save-facts` renders addresses as quoted strings, such as `"<Fact-1>"` or
`"<Dummy Fact>"`, matching CLIPS. These fact files do not preserve address
identity; use an engine snapshot when identity must survive persistence.

### I/O Functions

| Function | Description |
|----------|-------------|
| `printout` | Write to a named channel |
| `format` | Printf-style formatting; write to a named channel and return the string (`nil` returns only) |
| `read` | Read the first CLIPS field of the next nonblank input line |
| `readline` | Read a line from input |
| `load-facts` | Load facts from a `.fct` file into working memory |
| `save-facts` | Save local or visible facts, optionally restricted to named templates |

`save-facts` and `load-facts` work both as RHS actions and ordinary expressions,
including at the REPL. Fact files contain literal facts with real template and
slot names, without an `assert` wrapper. Saving defaults to `local`; use
`(save-facts "facts.fct" visible template-name ...)` to select visible templates.
The templates must already exist when loading their facts. A file that cannot
be opened, a symbol `save-facts` mode other than `local` or `visible`, and a
template selector that is not a symbol or names no matching template return
`FALSE` and write a diagnostic, and evaluation continues. As in CLIPS, a lexical
(such as an unterminated string), syntax, template, or value error in a fact
file and a `save-facts` mode that is not a symbol stop the enclosing evaluation
(an RHS halts the run); facts loaded before the bad one stay asserted. The first
standalone token that does not open a fact, such as a word, number, string, or
stray `)`, quietly ends the file, even before a later lexical error:
`load-facts` returns `TRUE` and ignores the rest. Saving escapes strings for reloading; fact
addresses remain the lossy quoted representation described above.

`printout` writes a top-level STRING without quotes, and a multifield in
parentheses with its STRING fields quoted but not escaped:
`(printout t (create$ "a" "two words") crlf)` writes `("a" "two words")`. Only
top-level `crlf`, `tab`, `vtab` and `ff` expand; inside a multifield they stay
symbols.

FLOATs print with up to 15 significant digits (CLIPS's `%.15g`), with `.0` on
integral values: `1.0`, `1e-05`, `1e+15`. Non-finite values print as `nan.0`,
`inf.0` and `-inf.0`. `str-cat` and `sym-cat` spell FLOATs the same way.

NaN generation and C printing vary across platforms: CLIPS may render
`(sin 1e309)` as `nan.0` or `-nan.0`. Ferric renders both NaN signs as `nan.0`
(or `nan` for numeric `format` conversions). The portable corpus checks exact
`format` padding and `implode$` consistency with the displayed scalar spelling.
Its direct `printout` non-finite fixture covers positive and negative infinity,
and the math NaN-propagation fixture prints each result's NaN kind rather than
its sign;
Rust host-value tests separately check scalar and multifield output for both
explicit NaN bit signs. Corpus output comparison remains byte-exact, with no
NaN normalization.

`format` writes its completed string to the named channel and returns the
same string. `(format t "n=%d%n" 42)` writes `n=42` followed by a newline;
`(format nil "n=%d" 42)` returns the string without writing it. `printout`
writes each argument before evaluating the next, including inside callable
bodies. Output from nested calls appears in evaluation order, and an error
in a later argument preserves the output already written. `printout` to `nil`
writes nothing and evaluates none of its arguments.

`format` follows CLIPS 6.30 and C `printf`: `%d %o %x %u` (FLOATs truncate),
`%f %e %g` (INTEGERs convert), `%s` (STRING, SYMBOL or INSTANCE-NAME; a number
is an error), `%c`, and `%n %r %t %v %%`, with `-` and `0` flags, width and
precision. The argument count must match the directives. Width and precision
count bytes, as in C, so `%.Ns` that cuts a multibyte character, and `%c` of a
byte of 128 or more, produce U+FFFD where C emits bytes that are not UTF-8.
CLIPS hands a malformed directive such as `%5-3d` to `printf`, which echoes it;
Ferric reports a format error. Ferric also rejects a width or precision above
4096 (CLIPS 6.30 crashes on `%5000d`).

### Command-line evaluation and inspection

`ferric run file.clp` reads non-terminal standard input line by line, only
when a `read` or `readline` (including one in a deffacts or defglobal
initializer) asks for it, as CLIPS does. A program that never reads neither
waits for nor consumes its caller's input. A read error or invalid UTF-8 on
standard input prints one warning and then reads as end of input. Construct-only files receive an implicit reset and unlimited run. A file
containing any procedural form, including `assert`, is a script: its forms
execute in source order, with no additional reset/run or expression-result echo.
Use explicit `(reset)` and `(run)` where needed. The entire bounded file is
parsed before execution; later evaluation/loading failures retain earlier
effects and make the command exit unsuccessfully.

The CLI writes captured standard channels to process stdout in emission order:
`t`, `stdin`, `stdout`, `stderr`, `wclips`, `wdialog`, `wdisplay`, `werror`,
`wtrace`, and `wwarning`. These are Ferric's channel names; native CLIPS 6.30
does not accept all of them as output destinations. Host-facing CLI diagnostics
retain their usual text/JSON error handling.

The REPL accepts constructs, shell commands such as `(run)` and `(facts)`, and
ordinary expressions. It echoes non-void values, including assertion addresses,
and uses the runtime's CLIPS value/fact formatter. `(facts)` lists working
memory by fact index, including `f-0     (initial-fact)`, with CLIPS's
`For a total of N fact(s).` tally. A fresh Ferric engine has no
`initial-fact` until it is reset or cleared or loads a rule, where CLIPS
lists f-0 from start-up. `(rules)` lists the current module's rules
unqualified, in definition order, with a `defrule(s)` tally. As in CLIPS,
an empty `(facts)`, `(rules)`, or `(agenda)` listing prints nothing.
`(agenda [module])` prints ordered rows with fact bases; `*` lists all modules. `(watch facts)` records each
assertion and retraction, including facts created and removed within one run.
`(watch rules)` includes each firing's fact basis. Watch settings are transient
host state and are disabled when restoring a snapshot.

`watch` and `unwatch` accept every CLIPS 6.30 watch item: `facts`, `instances`,
`slots`, `rules`, `activations`, `messages`, `message-handlers`,
`generic-functions`, `methods`, `deffunctions`, `compilations`, `statistics`,
`globals`, `focus`, and `all`. They return no value. Only `facts` and `rules`
(and `all`) produce trace output; the other items are accepted without effect,
so batch files that begin with `(unwatch compilations)` or `(watch statistics)`
run normally. Trailing construct names, as in `(watch facts item)` or
`(watch rules r)`, must name an existing deftemplate, defrule, deffunction,
defglobal, or defgeneric, as CLIPS requires; Ferric has no COOL classes, so names
after `instances`, `slots`, and `message-handlers` are not checked, and `methods`
takes generic function names but not method indices. Tracing stays global:
`(watch facts item)` traces every fact, not only `item` facts. An unknown item or
construct name, a non-symbol item, or a name after `messages`, `focus`,
`compilations`, `statistics`, or `all` is an error that stops the enclosing
evaluation, as in CLIPS.

`Engine::eval_str` evaluates exactly one expression with fresh local bindings.
Globals, engine changes, and output persist, including changes before a runtime
error. `(bind ?x 3)` returns `3`, but a later call cannot read `?x`; bind and read
within one `progn` to share a local. This differs from CLIPS's persistent prompt
locals. The Ferric REPL also accepts multiple forms on one input line.
`Engine::load_str` remains the construct-loading API. A root `(reset)` selects
`MAIN` for subsequent shell commands and remaining root-expression operands.
Nested callable evaluation retains its lexical module for dynamic `build` and
query target resolution; it does not reproduce CLIPS's temporary ambient-module
switch after a reset inside a callable.

For hosts that need cross-channel order, call `enable_output_events()` before
evaluation and consume `(channel, text)` chunks with `drain_output_events()`.
Draining clears the captured per-channel buffers. Reset and buffer clearing
preserve undelivered events. Output already buffered when observation starts is
delivered in channel-name order; snapshots retain those buffers but do not retain
the live event queue or observation setting.

### Agenda / Focus Functions

| Function | Description |
|----------|-------------|
| `get-focus` | Return the current focus module name |
| `get-focus-stack` | Return the focus stack as a multifield |
| `get-strategy` | Return the current conflict-resolution strategy symbol |
| `set-strategy` | Select `depth`, `breadth`, `lex`, or `mea`, reorder pending activations, and return the previous strategy (see [Conflict Resolution Strategies](#conflict-resolution-strategies)) |
| `watch` | Trace `facts`, `rules`, or `all`; other CLIPS 6.30 items are accepted no-ops; return no value |
| `unwatch` | Stop tracing `facts`, `rules`, or `all`; other CLIPS 6.30 items are accepted no-ops; return no value |

---

## 16.11 Unsupported Features

The following features are explicitly out of scope.

| Feature | Status | Notes |
|---------|--------|-------|
| COOL object system | Not planned | Classes, instances, message-passing; INSTANCE-NAME values exist without objects |
| Certainty factors | Not planned | Probabilistic/fuzzy reasoning |
| Distributed evaluation | Not planned | Networked rule engines |
| `Simplicity` strategy | Deferred | Until fully specified |
| `Complexity` strategy | Deferred | Until fully specified |
| `Random` strategy | Deferred | Until fully specified |
| General cross-engine tie equivalence | Partial | Depth/breadth traversal and blocker history match the covered cases; identical negative/NCC node sharing remains a documented boundary, shared by LEX/MEA ties whose recency and specificity are also equal |
| Truth maintenance (`logical` CE) | Explicitly rejected | Logical support is outside the current supported subset; no performance claim is implied |
| More than four nested `not`/`exists`/`forall` operators | Not supported | Reduce combined source nesting depth |
| Nested `(forall ...)` | Not supported | Decompose with phase facts |
| `forall` under `not`/`exists`, or with unsupported operands | Not supported | Use one fact condition and one fact/test requirement in a positive rule condition |
| File routers (`open` and file-backed logical-name I/O) | Not supported | `close` is a compatibility stub; use host I/O, captured output, `load-facts`, or `save-facts` |
| Source command `load` | Compatibility stub returning FALSE | Use host `Engine::load_str` / `load_file`, or `build` for one construct |
| Environment commands `load*`, `facts`, `batch*`, `exit`, `ppfact` | Not supported | Drive loading, inspection, batching, and process lifetime from the host |
| Remaining `ppdef*`, `list-def*`, and `undef*` commands | Not supported | `ppdefrule`, `rules`, and single-name `undefrule` are the implemented exceptions; construct-list getters are listed in §16.10 |
| Legacy aliases `mv-append`, `str-implode`, `wordp`, `subset` | Not supported | Use `create$`, `implode$`, `symbolp`, and `subsetp` |
| `set-salience-evaluation` | Not supported | Only definition-time salience evaluation is supported; when-activated/every-cycle modes are unavailable |
| Random draws consumed by activation creation | Not supported | CLIPS 6.30 draws once per new activation; Ferric's seeded stream matches only when no activation is created between `seed` and a draw |
| Load-time result-type checks of nested built-in calls | Not supported | CLIPS rejects e.g. `(abs (str-cat a))` at load; Ferric checks the value when evaluated, so the run stops there |
| Reentrant `build` during construct initialization | Explicitly rejected | Invoke it after loading; the pinned reference crashes on these initializer cases |
| Generic and method redefinition through `build` or source loading | Partial support | Repeated `defgeneric` declarations and an occupied explicit method index are rejected. An implicit method with equivalent restrictions is added with a new index instead of replacing the prior method, unlike CLIPS. New methods with distinct restrictions are supported. |
| Other absent non-COOL built-ins | Not supported | A name omitted from the supported surface is not implicitly provided; unknown calls report `EXPRNPSR3` |

---

## 16.12 String and Symbol Comparison Semantics

### Byte-Equality Comparison

Ferric uses **byte-equality comparison** for strings and symbols. Two values
are equal if and only if their byte sequences are identical.

- No Unicode normalization is performed. NFC and NFD representations of the
  same character are treated as distinct values.
- No collation or locale-aware ordering.
- No case-insensitive comparison built in.

### sub-string Indexing

`sub-string` counts characters (Unicode scalar values) from one, includes
both ends, clips out-of-range bounds, and always returns a STRING:
`(sub-string 0 2 abc)` is `"ab"` and `(sub-string 3 2 abc)` is `""`.

### Compatibility with CLIPS

Like CLIPS 6.30, `str-length`, `sub-string` and `str-index` count characters
of UTF-8 text, and comparisons use the bytes. Ferric strings and symbols are
always valid UTF-8, while CLIPS can build byte strings that are not. Where
CLIPS would produce such bytes (`%c` of a byte of 128 or more, `%.Ns` that cuts
a multibyte character, or a scanned string that ends in an escaped end of
input), Ferric holds U+FFFD instead; the corpus records each of these as a gap
case (see [Granular corpus](#granular-corpus)).

### Guidance for Unicode Users

If your application requires normalization-aware comparison, normalize strings
to a canonical form (e.g., NFC) before asserting them as facts. This ensures
consistent matching regardless of input source.

---

## 16.13 External Interface Contracts (FFI + Embedding)

Ferric provides a C-compatible FFI layer (`ferric-rules-ffi`) for embedding into
C, C++, Swift, Kotlin (NDK), and other languages with C FFI support.

### Engine Lifecycle

```c
#include "ferric.h"
#include <stdlib.h>

// Create engine with defaults
FerricEngine* engine = ferric_engine_new();

// Or with configuration
FerricConfig cfg = {
    .string_encoding = FERRIC_STRING_ENCODING_UTF8,
    .strategy = FERRIC_CONFLICT_STRATEGY_DEPTH,
    .max_call_depth = 256
};
FerricEngine* engine = ferric_engine_new_with_config(&cfg);

// Load, reset, run
ferric_engine_load_string(engine, "(defrule r (go) => (printout t \"hello\" crlf))");
ferric_engine_reset(engine);

uint64_t fired;
ferric_engine_run(engine, -1, &fired);

// Read output into caller-owned storage
size_t output_len = 0;
if (ferric_engine_get_output_copy(engine, "t", NULL, 0, &output_len) ==
    FERRIC_ERROR_OK) {
    char* output = malloc(output_len);
    if (output != NULL &&
        ferric_engine_get_output_copy(engine, "t", output, output_len,
                                      &output_len) == FERRIC_ERROR_OK) {
        // use output
    }
    free(output);
}

// Clean up
ferric_engine_free(engine);
```

### Logical Run Continuation

`ferric_engine_run_ex` always begins a *fresh logical run*: it clears any
pending halt request and the accumulated action diagnostics, but leaves working
memory, the agenda, globals, and captured output untouched. Hosts that split
one long run into bounded chunks — to poll for cancellation between them —
must not use repeated `ferric_engine_run_ex` calls for that, because each call
discards the halt flag and diagnostics that the previous chunk produced. A rule
set that halts on its 100th activation then fires 101 rules through a
100-activation chunk loop.

`ferric_engine_continue_run_ex` resumes the current logical run instead:

```c
uint64_t chunk_fired = 0, total_fired = 0;
FerricHaltReason reason;

if (ferric_engine_run_ex(engine, CHUNK, &chunk_fired, &reason) != FERRIC_ERROR_OK)
    return -1;
total_fired += chunk_fired;

while (reason == FERRIC_HALT_REASON_LIMIT_REACHED) {
    if (host_canceled())   // cancellation is the host's outcome, not the engine's
        break;
    if (ferric_engine_continue_run_ex(engine, CHUNK, &chunk_fired, &reason) !=
        FERRIC_ERROR_OK)
        return -1;
    total_fired += chunk_fired;
}
```

Contract:

- Continuation is legal only after `ferric_engine_run_ex` or a previous
  continuation returned `FERRIC_HALT_REASON_LIMIT_REACHED`. Any other call
  returns `FERRIC_ERROR_INVALID_ARGUMENT`, records a per-engine message, and
  leaves `*out_fired` and `*out_reason` unmodified.
- `FERRIC_HALT_REASON_AGENDA_EMPTY`, `FERRIC_HALT_REASON_HALT_REQUESTED`, and
  `FERRIC_HALT_REASON_ACTION_ERROR` are terminal and end continuation
  eligibility.
- `*out_fired` is that chunk's count, not a running total. Hosts accumulate it.
- Read-only queries and `ferric_engine_clear_error` may be interleaved between
  chunks. Any other call that reaches engine state ends the logical run,
  whether or not it then succeeds — a rejected `ferric_engine_retract` counts.
- A call rejected *before* it reaches engine state changes no runtime or
  continuation state, though it still publishes its documented error on the
  channels described under Error Handling. A null handle or an overlapping or
  reentrant call leaves the logical run intact for a later serialized
  continuation.
- Absent host cancellation, a chunked run and an equivalent one-shot run report
  the same total fired count, halt reason, agenda state, and action
  diagnostics.
- Host cancellation is not `FERRIC_HALT_REASON_HALT_REQUESTED`. The ABI has no
  canceled state: a canceling host stops submitting chunks, reports its own
  outcome, and starts any later logical run with `ferric_engine_run_ex`. The
  agenda is left intact.
- Cancellation does **not** guarantee an un-halted engine. The halt flag
  reflects whatever the chunks that did run executed. If a chunk landed exactly
  on an activation that called `(halt)`, that chunk still reports
  `FERRIC_HALT_REASON_LIMIT_REACHED` — the pending halt only surfaces as
  `HALT_REQUESTED` on the next chunk that can run — so a host canceling at that
  boundary leaves a halted engine. Query `ferric_engine_is_halted` if the
  distinction matters; `ferric_engine_run_ex` clears the flag either way when it
  starts the next logical run.
- Continuation eligibility is per-handle and is not serialized. A handle
  produced by `ferric_engine_deserialize_*` always begins a fresh logical run.

### Thread transfer and serialization

Rust `Engine` is structurally `Send + Sync`: ownership can transfer, shared
reads may run concurrently, and mutation requires exclusive access. A raw C
handle may move between OS threads for use and destruction, but the C host
must serialize all
runtime calls, including reads, and protect the allocation's lifetime:

- An atomic admission guard rejects overlapping runtime calls and same-engine
  reentry from a host callback with `FERRIC_ERROR_INTERNAL_ERROR`.
- `ferric_engine_last_error_copy` is separately synchronized and copies one
  coherent snapshot per call, including during callbacks or other calls.
- `ferric_engine_last_error` may be called from any thread. The host must
  protect use of its borrowed pointer against another borrowed error read or
  destruction. Prefer the copy API when those windows could overlap.
- Borrowed output strings must also be copied or consumed before another call
  can invalidate them; transfer does not extend their documented lifetime.
- Successful destruction must not overlap any access, including diagnostics
  and use of borrowed pointers. Admission is not a handle registry and cannot
  make a stale pointer safe. `ferric_engine_free_unchecked` remains an ABI
  compatibility alias with the same lifetime obligations as ordinary free.
- `FERRIC_ERROR_THREAD_VIOLATION` keeps its numeric ABI value but is never
  returned.

Global error functions use thread-local storage. Retrieve or copy a global
error on the same OS thread as the failing call, before another call can
replace it. A transferred handle carries its per-engine error snapshot, not
the previous thread's global error. Bindings should copy output and errors
before releasing the host-side protection that makes them valid.

### Error Handling

Two error channels exist:

1. **Per-engine errors**: `ferric_engine_last_error()` /
   `ferric_engine_last_error_copy()` / `ferric_engine_clear_error()`
2. **Global (thread-local) errors**: `ferric_last_error_global()` /
   `ferric_last_error_global_copy()` / `ferric_clear_error_global()`

Failures involving a validated raw-engine handle publish the same current
message to both that engine's snapshot and the calling thread's global
fallback. Failures before handle validation (for example, creation or
null-handle errors) update only the global channel. Engine snapshots are
independent: an operation on one engine does not overwrite another engine's
message.

Bindings should prefer the per-engine channel for engine operations and use the
global channel as a fallback or for pre-engine failures.

### Embedded-NUL String Policy

Ferric's Rust strings can contain `\0`, but the legacy C ABI represents input
strings and `FerricValue` Symbol/String payloads as NUL-terminated C strings.
The C ABI therefore uses an explicit-rejection policy at that legacy boundary:

- A legacy `const char *` input ends at its first NUL by definition; bytes
  after it are not part of the C string. Bindings starting from a
  pointer-plus-length string must reject embedded NUL before calling any such
  entry point.
- `ferric_value_symbol_bytes` and `ferric_value_string_bytes` accept an
  explicit UTF-8 byte span and return `FERRIC_ERROR_INVALID_ARGUMENT` if it
  contains embedded NUL. Their output remains Void on failure.
- Fact-field, global, and named-slot queries return
  `FERRIC_ERROR_INVALID_ARGUMENT` (with a diagnostic) instead of converting a
  stored NUL-bearing Symbol/String to empty or truncated `FerricValue` data.
  This rule applies recursively to multifields.
- `ferric_engine_get_output` returns NULL and records
  `FERRIC_ERROR_INVALID_ARGUMENT` when captured output contains embedded NUL.
  Use `ferric_engine_get_output_copy` for exact access.
- Length-reporting copy APIs preserve every source byte, including embedded
  NUL. Their reported length includes one additional trailing terminator, so
  callers must use `out_len` rather than `strlen`.
- Snapshot serialization/deserialization APIs are byte-oriented and preserve
  their serialized bytes exactly. If a restored engine contains NUL-bearing
  values, the same legacy-egress rejection rules apply.

The Go binding rejects embedded NUL before every legacy `C.CString`
conversion. Rejections return `ErrInvalidArgument` through error-bearing APIs;
their diagnostic identifies the offending argument and the byte offset of the
first NUL. `Engine.GetOutputE`, `Engine.ClearOutputE`, and
`Engine.PushInputE` expose these errors for I/O arguments. The older
convenience methods delegate to those error-aware methods and intentionally
discard the error for compatibility; an invalid channel never aliases its
prefix, and invalid input is not queued.

Snapshot payloads remain byte-oriented and may contain NUL. Snapshot file
paths are handled by Go's filesystem APIs rather than `C.CString`; invalid
paths therefore retain the platform's `os.PathError` behavior.

### Copy-to-Buffer Contract

The `*_copy` functions follow a uniform contract:

| Condition | Return Code | `*out_len` |
|-----------|-------------|------------|
| No source value (for example, no error or no channel output) | `FERRIC_ERROR_NOT_FOUND` | 0 |
| `out_len` is NULL | `FERRIC_ERROR_INVALID_ARGUMENT` | (not written) |
| `buf` is NULL, `buf_len` is 0 (size query) | `FERRIC_ERROR_OK` | Required size (incl. NUL) |
| `buf` non-null, `buf_len` >= needed | `FERRIC_ERROR_OK` | Bytes written (incl. NUL) |
| `buf` non-null, `buf_len` < needed | `FERRIC_ERROR_BUFFER_TOO_SMALL` | Full needed size (incl. NUL) |

On truncation, the buffer receives `buf_len - 1` bytes followed by a NUL
terminator and the function returns `FERRIC_ERROR_BUFFER_TOO_SMALL`; truncation
is never reported as success. `ferric_engine_get_output_copy` follows this
contract and is the preferred output accessor for caller-owned storage. The
reported length is authoritative even when the copied payload contains an
embedded NUL.

### Fact Lifecycle

```c
// Assert and get fact ID
uint64_t fact_id;
ferric_engine_assert_string(engine, "(color red)", &fact_id);

// Retract by ID
ferric_engine_retract(engine, fact_id);

// Query facts
size_t count;
ferric_engine_fact_count(engine, &count);

size_t field_count;
ferric_engine_get_fact_field_count(engine, fact_id, &field_count);

FerricValue val;
ferric_engine_get_fact_field(engine, fact_id, 0, &val);
// ... use val ...
ferric_value_free(&val);
```

### Action Diagnostics

Rule-action evaluation failures are collected as action diagnostics, distinct
from API/ABI failures. They do not invalidate the engine, but they stop the
current activation and `run()` as described above:

```c
size_t diag_count;
ferric_engine_action_diagnostic_count(engine, &diag_count);

for (size_t i = 0; i < diag_count; i++) {
    size_t needed;
    // Size query
    ferric_engine_action_diagnostic_copy(engine, i, NULL, 0, &needed);
    char* buf = malloc(needed);
    ferric_engine_action_diagnostic_copy(engine, i, buf, needed, &needed);
    printf("warning: %s\n", buf);
    free(buf);
}
ferric_engine_clear_action_diagnostics(engine);
```

### Value and Memory Management

| Function | Purpose |
|----------|---------|
| `ferric_string_free` | Free a Ferric-allocated C string |
| `ferric_value_multifield_copy` | Deep-copy a borrowed `FerricValue` array and its nested tree into one Ferric-owned multifield; external-address payload pointers remain caller-owned |
| `ferric_value_free` | Free a `FerricValue` and its owned resources (recursive); returns `FERRIC_ERROR_INVALID_ARGUMENT` if any `value_type` tag is unknown (that value's payload is left untouched; known siblings and owned arrays are still freed) |
| `ferric_value_array_free` | Free an array of `FerricValue`s; same unknown-tag contract as `ferric_value_free` |

Structured values passed to assertion APIs and
`ferric_value_multifield_copy` are borrowed for the call: Ferric never retains
or frees their strings or arrays. The copy constructor returns an independent
Ferric-owned tree that must be released with `ferric_value_free`; only
Ferric-owned trees may be passed to Ferric value cleanup APIs. External-address
payload pointers are shallow and caller-owned in this legacy copy helper.
Engine assertion and value-conversion APIs reject `ExternalAddress`; copying
this field does not create a transferable Rust host token. Multifield-copy inputs
must be acyclic and no deeper than 128 nested multifield levels.

Other borrowed pointers must **not** be freed by the caller.
`ferric_engine_last_error` remains valid until the next borrowed last-error
read on that engine or engine destruction; error writers and the copy API do
not invalidate it. `ferric_engine_get_output` returns a per-engine,
per-channel snapshot that remains valid until a later borrowed read for the
same engine and channel replaces it; that channel is cleared; the engine is
reset or cleared; or until engine destruction. Reads on other engines do not
invalidate it, and later output writes are not reflected in an existing
snapshot. Use `ferric_engine_get_output_copy` when the caller should own the
bytes.

### Panic Policy

Every public C function is generated as a small `extern "C"` wrapper around a
non-extern Rust implementation. The `ffi-dev` and `ffi-release` profiles retain
unwind support, and each wrapper catches ordinary Rust panics before they can
reach the C ABI.

A contained panic records a stable message naming the export, without
formatting or downcasting the panic payload. The calling thread's global error
channel is always updated; a supplied live engine also receives
the same per-engine message. Ownership-consuming free functions update only
the global channel because a panic can make the handle's remaining lifetime
indeterminate.

Return sentinels are fixed by category:

| Return category | Panic sentinel |
|-----------------|----------------|
| `FerricError` | `FERRIC_ERROR_INTERNAL_ERROR` |
| Any pointer | NULL |
| `FerricValue` | Void |
| `void` | Return after recording the diagnostic |

Containment does not cover non-unwinding termination such as allocator
abort/OOM or an explicit process abort. Foreign callbacks must still return
normally and obey their own no-unwind contract.

---

## 16.14 Machine-Readable CLI Diagnostics

The `ferric` CLI supports `--json` mode for structured diagnostics on stderr.

### Commands

```
ferric run [--json] <file>    # load, reset, run, print output
ferric check [--json] <file>  # load and validate without executing
```

### JSON Diagnostic Format

Each diagnostic is a single JSON object on one line of stderr:

```json
{"command":"run","level":"error","kind":"load_error","message":"Unexpected token at line 3"}
```

### Field Descriptions

| Field | Type | Values |
|-------|------|--------|
| `command` | string | `"run"` or `"check"` |
| `level` | string | `"error"` or `"warning"` |
| `kind` | string | Diagnostic category (see below) |
| `message` | string | Human-readable diagnostic text |

### Diagnostic Kinds

| Kind | Emitted by | Description |
|------|-----------|-------------|
| `io_error` | run, check | File not found or I/O failure |
| `load_error` | run, check | Parse or compilation error |
| `runtime_error` | run | Execution failure |
| `action_warning` | run | Non-fatal action diagnostic |

### Evolution Contract

- New fields may be added to diagnostic objects in future versions.
- Existing documented fields will not be removed or repurposed.
- Parsers should ignore unknown fields for forward compatibility.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime or load error |
| 2 | Usage error (missing argument) |

### Example: CI Integration

```bash
# Check syntax and capture errors as JSON
ferric check --json rules.clp 2> errors.json
if [ $? -ne 0 ]; then
    cat errors.json | jq '.message'
fi

# Run and capture warnings
ferric run --json rules.clp 2> diagnostics.json
```

Standard output (stdout) contains the rule engine's normal output. All
diagnostics are emitted to stderr.

### Template constraints

Primitive template slot unions (`SYMBOL`, `STRING`, `INTEGER`, `FLOAT`,
`NUMBER`, `LEXEME`, `INSTANCE-NAME`, `FACT-ADDRESS`, `EXTERNAL-ADDRESS`) are
retained and checked, together with allowed-value lists, numeric ranges, and
multislot cardinality. Default values, seed facts, literal rule assertions, and
literal pattern restrictions are validated before their construct is installed;
runtime values and host template assertions are always validated.
Unlike CLIPS 6.30 with its default dynamic checking disabled, Ferric rejects
runtime values that violate a declared slot constraint. See
[the migration notes](migration.md#template-constraints-and-computed-defaults)
for default derivation, external-address handling, and the remaining
unsupported class attributes (`allowed-classes`, `allowed-instance-names`).

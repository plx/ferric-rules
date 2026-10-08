# CLIPS compatibility coverage map

This corpus turns the supported surface in
[the compatibility reference](../../../docs/compatibility.md) into small,
independently observable programs. It is a growing characterization suite,
not an exhaustive proof of compatibility. A case's presence means the behavior
is exercised; it does not mean Ferric currently agrees with CLIPS. Consult
[manifest.json](manifest.json) for the current oracle and gap classification.

The initial inventory contains **219 programs**: `facts` 16, `patterns` 35,
`agenda` 10, `modules` 16, `generics` 14, `procedural` 28, `queries` 23,
`stdlib` 66, `io` 7, and `lifecycle` 4. Counts describe programs, not independent
language guarantees. The manifest is authoritative as the corpus grows.

## Progression

Every case has a `basic`, `boundary`, or `interaction` level. These levels
support incremental selection; numeric filename prefixes are local to an area
and are not a global dependency order.

- **Basic:** one valid construct or ordinary input establishes a small control.
- **Boundary:** empty input, type distinctions, lengths, inclusive endpoints,
  missing matches, and similar edge conditions refine that control.
- **Interaction:** joins, mutation, repeated reset, scope, and control flow test
  how individually supported behaviors combine.

For example, ordered multifield coverage advances from empty and terminal
capture to prefix/middle capture and split bindings. Negation advances from
absence to correlated blockers and activation cancellation. Query coverage
advances from empty/existing matches to filtering, multiple fact variables,
ordered traversal, and mutation during traversal. A passing nearby control is
useful evidence when a boundary case reveals a gap.

All six fact-query forms, expression and action, run as conformance cases,
including empty results, ordering after mutation, and rejected declarations.

## Compatibility reference mapping

| Reference area | Current characterization | Remaining coverage to add |
|---|---|---|
| **16.1 Facts** | [facts/](facts/): zero-field and typed ordered facts, exact arity, template slot defaults/order, duplicate suppression, retract, modify, duplicate. [queries/](queries/): live fact checks, indices, relations, and slot introspection. [agenda/](agenda/): refraction and retract/reassert identity. | Index monotonicity across longer churn sequences; full initial-fact ordering relative to multiple deffacts groups; mixed template/ordered mutations; fact identity and introspection after repeated modification. |
| **16.2 Rules** | [patterns/](patterns/): variable equality, typed joins, `~`, `|`, `&`, wildcards, predicate and return-value constraints, test, not, exists, forall, and NCC. [agenda/](agenda/): literal/global/expression salience evaluated at definition, same-rule depth recency, activation cancellation, chaining, halt, and nested run; the `_breadth` pattern cases run under breadth. Shared-successor, mixed positive/negative/exists, OR-variant, nested `exists` chain, and simple-negative/NCC retraction ties, including one blocker matched through several alpha memories, have exact oracles. [procedural/](procedural/), [queries/](queries/), and [modules/](modules/) exercise RHS actions. | LEX and MEA through configurable host execution; additional multi-pattern recency combinations; all supported constraint combinations; RHS reset/clear timing; textual agenda and focus-stack rendering. |
| **16.3 Deftemplates** | [facts/](facts/): all supported allowed-value facets, range/cardinality derivation, aggregate multislot values, static/dynamic default timing, supplied-slot suppression, declaration-order evaluation, module lookup, lexical defaults, omitted-slot preservation on modify/duplicate, and isolated literal/LHS/declaration rejections. [patterns/](patterns/): repeated slot bindings and multislot sequence matching. | Slot aliases, duplicate slot declarations, more unknown-slot diagnostics, oversized bounds, and computed-invalid values under Ferric's stricter runtime policy. CLIPS invalid derived-default edge cases are documented rather than marked conforming. |
| **16.4 Deffacts** | [facts/](facts/): multiple groups and duplicate assertions. [modules/qualified-deffacts.clp](modules/qualified-deffacts.clp): module qualification. [lifecycle/](lifecycle/): deffacts restoration and derived-fact removal on repeated reset. | Explicit initial-fact/deffacts ordering, several module-scoped groups in one reset, additional expression/error combinations and construct replacement. |
| **16.5 Defrules** | [patterns/](patterns/) covers the documented CEs up to four combined quantifier levels, including forall vacuity/missing witnesses and NCC correlation. The `patterns/405_*` cases cover fact-address bindings in positive and/or groups (including an address bound in only some `or` branches), negated disjunctions, `and`/`or` groups nested inside each other, `not`, and `exists`, `exists` over `or` and over `not` as one condition, forall test requirements, test-only quantifier wrappers, four-deep negation, binding-scope rejections (including a later test reading a variable local to `not`, `not (or ...)` or `exists (or ...)`), empty-`exists` rejections, and located-limit characterizations. [modules/](modules/) covers focused module execution, auto-focus on activation creation, deferred-predicate event order, repeated module pushes, and cancellation history; [agenda/](agenda/) and [modules/](modules/) also cover depth-first test CE and NCC admission tie order with and without auto-focus, and auto-focus rules guarded by a multi-pattern `exists`, including one whose first join is shared with an older rule, next to the transient focus push of a `not (and ...)` over a shared join or with conditions after a nested `not (and ...)`, and online installation of such `exists` rules whether or not the conjunction holds. [agenda/001_empty_lhs.clp](agenda/001_empty_lhs.clp) covers implicit startup activation. | All supported connective precedence combinations and rule comments/redefinition. Unsupported nesting is excluded below. |
| **16.6 Defglobals** | [modules/](modules/): literals, expression and multifield initializers, multiple globals, bind, function access, and named imports. [lifecycle/reset-restores-global.clp](lifecycle/reset-restores-global.clp): repeated initialization. | Qualified global references and mutation across multiple modules; ambiguous/private access; missing-global diagnostics; initializer dependency order and reset interactions. |
| **16.7 Deffunctions** | [procedural/](procedural/): zero/fixed/variadic arguments, expression sequences, recursion, local bind and parameter rebinding, function control flow, empty bodies, missing branches, wildcard multifield flattening, break scopes, rejected unknown calls, explicit forward declarations, and dynamic calls. Callable and expression engine effects ([procedural/093-119](procedural/) and the `procedural/401_*` cases) have exact return-value and lifecycle cases, including reset, clear refusal, a halt that survives a later reset, negative mutation targets, and a `fact-slot-value` slot argument that resets or retracts its fact. [modules/](modules/): named imports and qualified function calls. | Recursion/call-depth boundaries, malformed arity, additional wildcard/type combinations, lexical scope interactions, inaccessible functions, deffunction/defgeneric name conflicts, and callable output/early-return interactions. |
| **16.8 Generic functions and methods** | [generics/](generics/): scalar/abstract type dispatch, specificity, type-list precedence (subclass positions, shorter lists, and unranked different lists that skip queries and later parameters), multiple arguments/types, fallback, variadic methods and original-argument type restrictions, typed/queried wildcard precedence, cyclic precedence resolved by definition order, parameter queries over later parameters and flattened wildcards, global-variable queries, argument-order type/query checks and per-argument wildcard queries, lazy query side effects/errors, direct global binding, rejected local binding, rejected direct and nested `return`, and inaccessible variables/templates, repeated next-method queries, explicit indices, implicit generic declaration, and imports. | Several chained next methods; equal-specificity ambiguity; method redefinition; argument-count boundaries; qualified/dynamic generic dispatch; missing next method and construct-name conflict diagnostics. |
| **16.9 Modules** | [modules/](modules/): default focus, transfers, focus argument ordering and observation, template/function/global imports, import-all, and qualified constructs. [generics/import-generic.clp](generics/import-generic.clp): generic import. | Export visibility and ambiguous name rejection, separate modules with same-named constructs, nested focus restoration, empty-module popping, qualified negative patterns, and module-local deffacts interactions. |
| **16.10 Standard library** | [stdlib/](stdlib/): arithmetic/conversion/comparison/predicate functions, variadic equality, mixed-number modulo, selected domain/singularity errors and NaN propagation, load-time arity/literal-type checks, byte lengths, strings/multifields, expansion order, sorting, dynamic calls/source, seeded random draws, template/construct metadata and its recoverable notices, formatting, and printout. [generics/](generics/): next/specific/override method control. [procedural/](procedural/): ordinary progn and expansion placement. [queries/](queries/): fact introspection and all six query forms. [io/](io/): typed tokens, sequential reads, line whitespace, and EOF. [modules/](modules/): focus queries. | Complete arity/type/domain error matrices, overflow and extreme floating inputs, transcendental nonzero inputs, exhaustive format directives, input lexical forms/routers, and fact-file round trips. Random draws interleaved with agenda activation creation differ; time has only type/range checks. `floor`, `ceiling`, and `atan2` lack a CLIPS 6.30 oracle here; see reference limits below. |
| **16.11 Unsupported features** | Scope boundaries below are deliberate exclusions, not passing compatibility cases. | Keep deferred and unsupported constructs distinct from untested supported features. Add rejection diagnostics separately when useful. |
| **16.12 String/symbol comparison** | [stdlib/](stdlib/): ASCII length, substring/index bounds, empty needles, value/type-sensitive comparison, case conversion, quoted fields, and one Unicode length probe. | Unicode substring/index positions, multibyte case conversion, composed/decomposed forms, symbol escaping and unusual characters. The Unicode probe characterizes the pinned reference build, not a universal CLIPS Unicode contract. |

## Additional feature inventory

The following inventory records coverage in this granular corpus. Implementation
status and coverage in other suites do not establish a CLIPS oracle here.

| Feature | Corpus status |
|---|---|
| Engine effects in expressions | Assert/retract/modify/duplicate return values, callable/method mutation, action-query expression results, halt/focus, immediate reset preserving locals/output/query slots, clear refusal and index reset, and nested-reset initialization guards have CLIPS oracles. Stale-source modify/duplicate remains an explicit boundary. |
| Fact addresses | Distinct values print public indices, compare by identity, reject numeric/string coercion, dispatch as FACT-ADDRESS, and survive in slots, multifields, globals, and snapshot replays. Stale/dummy values, missing/negative integer indices, raw-key-looking integers, recoverable notices, and the deffunction and generic retract targets that a wrong-type target skips have exact CLIPS oracles. |
| Assertion expressions | Ordered/template deffacts evaluate arithmetic, globals, and user functions; multifield results splice into ordered fields and multislots, and void results are omitted from both. Repeated reset covers `gensym*`, late global definitions, function redefinitions, and evaluation order; template slot expressions run in declaration order, not source order. Unknown functions, local variables, and statically invalid scalar multifields fail load; globals can be defined after their seed expressions. A seed's fact query keeps its template in use, so a later redefinition in the same source fails load. Host tests cover top-level assertions and reset failures. |
| Template constraints and defaults | All seven allowed-list facets, category scope, numeric kind and signed-zero distinctions, range/cardinality bounds, derived defaults, static computation, dynamic suppression and repeated reset, slot declaration order, function redefinition and module/global resolution have CLIPS oracles. Invalid literals in facts, assertions, disjunctions and negated patterns, conflicting facets, malformed bounds, and default-expression errors are isolated load cases. Static void defaults, including a void element of a static multislot default, and direct return defaults reject; dynamic scalar void yields `nil`, and returns within called functions stay local. A redefinition whose default uses slot syntax to assert the template it replaces is rejected at load, as in CLIPS. An empty multislot restriction loads under any cardinality, and the unmodified official `mab.clp` example runs to its CLIPS output. |
| Format and printout output | `format t` writes and returns its string; nested format/printout/deffunction calls preserve argument evaluation order, and output queued in nested RHS bodies (`if`, loops, `switch`, queries) or by an `if` condition precedes later direct writes such as `list-focus-stack`. Runtime errors preserve partial output, `format nil` suppresses only its own write, and `printout nil` evaluates none of its operands. |
| Source numeric scanner | Delimiter-bounded numeric candidates cover trailing/leading decimal points, signed zero, exponents, numeric-looking symbols, number-, sign- and dot-led symbol continuation, incomplete exponents, source integer clamping, compact constraint connectives, and CR-only comments. Nonoverflow cases compare source values with `explode$`; general symbol tokenization remains outside this coverage. |
| Brackets in symbols | No dedicated granular program yet; existing lexer tests cover this separately. |
| Connective constraints | Literal, variable, predicate, and return-value disjunctions cover overlapping alternatives, leading bindings, joins, negation, exists, and forall. Alternatives that reference variables bound by other patterns are covered in ordered and template sequence fields and template single slots, and predicate alternatives share a leading multifield binding. More complex nested expressions remain incomplete. |
| Implicit initial-fact for empty rules | Empty-LHS startup and repeated-reset refraction cases exist; direct initial-fact identity/order still needs coverage. |
| Explicit `and` CE | NCC uses `not (and ...)`; an independent top-level `and` control is still missing. |
| `field` slot alias | No dedicated corpus case yet; parser tests exist separately. |
| Fact query macros | All six CLIPS query forms have focused cases plus empty/filter/order/mutation combinations and rejected declarations. The cross-product of conditions and mutation behavior remains incomplete. |
| `if`, loops, foreach/progn$, switch | Basic, empty-range/iteration, truthiness, bare/expression count bounds, nested count, missing branches, exact normal/break return values, and function/method bodies. Break exits the nearest loop or action query; illegal placement in initializers, conditions, and predicates is rejected. |
| Math, string, multifield, introspection, funcall | Broad ordinary-input coverage with selected boundary controls; function-by-function error matrices and generic funcall still need work. |
| `load-facts` / `save-facts` | Not represented in this portable program corpus; runtime file-roundtrip tests exist separately. |
| Complex constraints / compile-time function validation | Unknown calls in deffunction and method bodies reject at load; an explicit empty forward declaration can be replaced before execution. More diagnostic and expression-depth boundaries remain to characterize. |
| `eval`/`build` | Dynamic source values, first-form parsing, isolation from caller locals, implied-template declaration, and `build` rejections of templates and ordered relations in use, including a later fact of the same assertion and an `eval` expression's own references. Top-level assertions are Rust tests in [`dynamic_source.rs`](../../../crates/ferric-rules-runtime/tests/dynamic_source.rs). |
| Batch interpreter, file handles | Not represented in this corpus; no compatibility conclusion is inferred. |

## Harness and reference limits

The pinned reference is **CLIPS 6.30 (3/17/15)**. `floor`, `ceiling`, and `atan2`
are listed in Ferric's compatibility reference but are unavailable in this
CLIPS executable. They are absent from the reference-validated corpus rather
than assigned guessed oracle outputs. A later reference target or an explicit
extension contract is needed to characterize them.

Portable fixtures currently describe constructs with load/reset/run execution,
optional deterministic input, selected repeated reset/run cycles, the breadth
strategy, and programs CLIPS rejects at load or halts at run time. Ferric also
replays each conforming program from restored snapshots and with rules loaded
after reset, against the same CLIPS golden (the late-rule replay in any line
order). The fixtures do not describe other
host-driven sequences such as clear/reload, run limits followed by resume, or
inspecting host API return values; a few such lifecycles are Rust tests in
[`host.rs`](../../../crates/ferric-rules/tests/compat_corpus/host.rs). File paths
and file-router lifetimes are also outside the present corpus protocol. These
are coverage holes in the portable corpus, not evidence that the implementation
lacks those features.

Existing tests remain complementary: runtime
[phase 2](../../../crates/ferric-rules-runtime/src/phase2_integration_tests.rs) and
[phase 3](../../../crates/ferric-rules-runtime/src/phase3_integration_tests.rs) exercise
engine/reset state; [phase 4](../../../crates/ferric-rules-runtime/src/phase4_integration_tests.rs)
contains `load-facts`, `save-facts`, and file-roundtrip tests. Core
[strategy tests](../../../crates/ferric-rules-core/src/strategy.rs), parser tests, and
the [Ferric semantic regressions](../../../crates/ferric-rules/tests/ferric_semantic_regressions.rs)
cover additional behavior without executing reference CLIPS themselves.

The separate [semantic differential lane](../../examples/ferric-semantic/README.md)
does provide pinned-CLIPS evidence for declared scenarios, including staged
loading, construct replacement, strategy selection, and final working-memory
state. Its [structured oracle protocol](../../../docs/compatibility-assessment.md)
and blocking policy remain separate from this exact-output corpus. The holes
above describe this corpus; consult that lane's declarations for overlapping
coverage before adding another execution protocol.

## Deliberate exclusions

COOL, certainty factors, distributed evaluation, and truth maintenance via
`logical` are outside the declared target. Simplicity, Complexity, and Random
strategies remain deferred. More than four nested quantifiers, nested forall,
forall beneath `not`/`exists`, and unsupported forall operands remain excluded. Identical negative/NCC join sharing, selected multi-pattern
`exists` ties, and CLIPS's transient nested NCC refire remain characterized
topology gaps.
Late-installed auto-focus NCC rules also retain characterized differences in
fresh-subnetwork activation history, in a shared-prefix NCC followed by a
test CE, and in a blocked `exists (and ...)` conjunction that contains a `not`;
during ordinary reset and assertion, a test CE after an NCC that the NCC
cancels before it runs loses its transient focus push (#480).
General cross-engine replay-identical activation order is not promised; use
explicit salience/phases when application precedence must be independent of
network construction. These exclusions should not be
silently counted as uncovered supported requirements.

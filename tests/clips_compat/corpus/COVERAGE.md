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

The expression fact-query forms `any-factp`, `find-fact`, and `find-all-facts`
are explicitly rejected by the current supported subset. Their retained
programs characterize this boundary against valid CLIPS behavior, including
empty-result controls. The three action query forms are exercised separately.

## Compatibility reference mapping

| Reference area | Current characterization | Remaining coverage to add |
|---|---|---|
| **16.1 Facts** | [facts/](facts/): zero-field and typed ordered facts, exact arity, template slot defaults/order, duplicate suppression, retract, modify, duplicate. [queries/](queries/): live fact checks, indices, relations, and slot introspection. [agenda/](agenda/): refraction and retract/reassert identity. | Index monotonicity across longer churn sequences; full initial-fact ordering relative to multiple deffacts groups; mixed template/ordered mutations; fact identity and introspection after repeated modification. |
| **16.2 Rules** | [patterns/](patterns/): variable equality, typed joins, `~`, `|`, `&`, wildcards, predicate and return-value constraints, test, not, exists, forall, and NCC. [agenda/](agenda/): salience, same-rule depth recency, activation cancellation, chaining, halt, and nested run. [procedural/](procedural/), [queries/](queries/), and [modules/](modules/) exercise RHS actions. | Breadth, LEX, and MEA through configurable host execution; explicit tie cases and multi-pattern recency; all supported constraint combinations; RHS reset/clear timing; textual agenda and focus-stack rendering. |
| **16.3 Deftemplates** | [facts/](facts/): explicit/implicit defaults, partial slot patterns, slot order, omitted-slot preservation on modify/duplicate. [patterns/](patterns/): repeated slot bindings and multislot sequence matching. | Slot aliases, empty explicit multislots, dynamic/derived defaults and constraint facets where supported, multifield ambiguity, duplicate slot declarations, unknown slot and wrong-cardinality diagnostics. |
| **16.4 Deffacts** | [facts/](facts/): multiple groups and duplicate assertions. [modules/qualified-deffacts.clp](modules/qualified-deffacts.clp): module qualification. [lifecycle/](lifecycle/): deffacts restoration and derived-fact removal on repeated reset. | Explicit initial-fact/deffacts ordering, several module-scoped groups in one reset, expression evaluation during repeated resets, and construct replacement. |
| **16.5 Defrules** | [patterns/](patterns/) covers the documented single-level CEs, including forall vacuity/missing witnesses and NCC correlation. [modules/](modules/) covers focused module execution. [agenda/001_empty_lhs.clp](agenda/001_empty_lhs.clp) covers implicit startup activation. | Explicit top-level `and`, all supported connective precedence combinations, rule comments/redefinition, CE binding-scope rejection, and source-located invalid-pattern diagnostics. Unsupported nesting is excluded below. |
| **16.6 Defglobals** | [modules/](modules/): literals, expression and multifield initializers, multiple globals, bind, function access, and named imports. [lifecycle/reset-restores-global.clp](lifecycle/reset-restores-global.clp): repeated initialization. | Qualified global references and mutation across multiple modules; ambiguous/private access; missing-global diagnostics; initializer dependency order and reset interactions. |
| **16.7 Deffunctions** | [procedural/](procedural/): zero/fixed/variadic arguments, expression sequences, recursion, local bind and parameter rebinding, function control flow, dynamic calls. [modules/](modules/): named imports and qualified function calls. | Recursion/call-depth boundaries, malformed arity, wildcard edge combinations, lexical scope interactions, inaccessible functions, deffunction/defgeneric name conflicts, and callable output/early-return interactions. The documented body model allows evaluator expressions such as `printout`; fact mutation and agenda/focus actions belong in the calling rule's RHS. |
| **16.8 Generic functions and methods** | [generics/](generics/): scalar/abstract type dispatch, specificity, multiple arguments/types, fallback, variadic methods, explicit indices, implicit generic declaration, next method, and imports. | Several chained next methods; equal-specificity ambiguity; method redefinition; argument-count boundaries; qualified/dynamic generic dispatch; missing next method and construct-name conflict diagnostics. |
| **16.9 Modules** | [modules/](modules/): default focus, transfers, focus argument ordering and observation, template/function/global imports, import-all, and qualified constructs. [generics/import-generic.clp](generics/import-generic.clp): generic import. | Export visibility and ambiguous name rejection, separate modules with same-named constructs, nested focus restoration, empty-module popping, qualified negative patterns, and module-local deffacts interactions. |
| **16.10 Standard library** | [stdlib/](stdlib/): arithmetic/conversion/comparison/predicate functions, ordinary and selected boundary string/multifield inputs, sorting, dynamic calls, formatting, and printout. [queries/](queries/): introspection and all six fact query forms. [io/](io/): typed tokens, sequential reads, line whitespace, and EOF. [modules/](modules/): focus queries. | Complete arity/type/domain error matrices, overflow and extreme floating inputs, transcendental nonzero inputs, exhaustive format directives, input lexical forms/routers, and fact-file round trips. `floor`, `ceiling`, and `atan2` lack a CLIPS 6.30 oracle here; see reference limits below. |
| **16.11 Unsupported features** | Scope boundaries below are deliberate exclusions, not passing compatibility cases. | Keep deferred and unsupported constructs distinct from untested supported features. Add rejection diagnostics separately when useful. |
| **16.12 String/symbol comparison** | [stdlib/](stdlib/): ASCII length, substring/index bounds, empty needles, value/type-sensitive comparison, case conversion, quoted fields, and one Unicode length probe. | Unicode substring/index positions, multibyte case conversion, composed/decomposed forms, symbol escaping and unusual characters. The Unicode probe characterizes the pinned reference build, not a universal CLIPS Unicode contract. |

## Additional feature inventory

The following inventory records coverage in this granular corpus. Implementation
status and coverage in other suites do not establish a CLIPS oracle here.

| Feature | Corpus status |
|---|---|
| Brackets in symbols | No dedicated granular program yet; existing lexer tests cover this separately. |
| Connective constraints | Basic literal disjunction/conjunction/negation and predicate/return-value cases exist; precedence and complex nested expressions remain incomplete. |
| Implicit initial-fact for empty rules | Empty-LHS startup and repeated-reset refraction cases exist; direct initial-fact identity/order still needs coverage. |
| Explicit `and` CE | NCC uses `not (and ...)`; an independent top-level `and` control is still missing. |
| `field` slot alias | No dedicated corpus case yet; parser tests exist separately. |
| Fact query macros | All six CLIPS query forms have focused cases plus empty/filter/order/mutation combinations. Expression forms characterize explicit subset rejection; action forms exercise execution. The cross-product of conditions and mutation behavior remains incomplete. |
| `if`, loops, foreach/progn$, switch | Basic, empty-range/iteration, truthiness, nested count, expression-return, and function-body cases exist. Early exit and error paths need further probes. |
| Math, string, multifield, introspection, funcall | Broad ordinary-input coverage with selected boundary controls; function-by-function error matrices and generic funcall still need work. |
| `load-facts` / `save-facts` | Not represented in this portable program corpus; runtime file-roundtrip tests exist separately. |
| Complex constraints / compile-time function validation | Small positive examples do not cover the full accepted/rejected syntax matrix. Dedicated diagnostic and expression-depth boundary characterization remains necessary. |
| Batch interpreter, file handles, eval/build | Not represented in this corpus; no compatibility conclusion is inferred. |

## Harness and reference limits

The pinned reference is **CLIPS 6.30 (3/17/15)**. `floor`, `ceiling`, and `atan2`
are listed in Ferric's compatibility reference but are unavailable in this
CLIPS executable. They are absent from the reference-validated corpus rather
than assigned guessed oracle outputs. A later reference target or an explicit
extension contract is needed to characterize them.

Portable fixtures currently describe constructs with load/reset/run execution,
optional deterministic input, and selected repeated reset/run cycles. They do
not yet describe host-driven sequences such as incremental loading after facts
already exist, clear/reload, run limits followed by resume, strategy changes,
or inspecting host API return values. File paths and file-router lifetimes are
also outside the present corpus protocol. These are coverage holes in the
portable corpus, not evidence that the implementation lacks those features.

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
strategies remain deferred. Triple-nested negation, `exists (not ...)`, and
nested forall remain unsupported. Cross-run replay-identical activation order
is not promised; order-sensitive cases should use explicit salience/phases or
exercise a narrowly specified strategy rule. These exclusions should not be
silently counted as uncovered supported requirements.

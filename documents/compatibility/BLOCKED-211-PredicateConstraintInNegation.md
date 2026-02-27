BLOCKED: # 211 Predicate Constraint In Negation And Complex Patterns

## Scope And Intent
This document describes what is required to close the remaining CLIPS compatibility gaps around complex field/slot constraints, especially where predicate-style forms, forward references, and intra-pattern variable reuse are involved.

## CLIPS Behavior (Reference)
CLIPS supports rich constraint expressions inside pattern fields/slots, including combinations like `&`, `|`, `~`, predicate constraints `:(...)`, and return-value constraints `=(...)`.

In practical terms:
- CLIPS accepts patterns that reference variables multiple times within a single pattern, including same-variable reuse across slots/fields (intra-pattern equality constraints).
- CLIPS accepts certain forward-reference forms in connected constraints and defers resolution to matching logic.
- CLIPS handles predicate constraints as executable boolean tests in the constraint chain instead of reducing them to wildcards.
- CLIPS may emit warnings for some constraint inconsistencies but still compile the rule set.

Representative examples:
```clips
(avh (a pet) (v fox)
     (h ?p3&~?p2&~?p1&:(or (= ?s2 (- ?p3 1)) (= ?s2 (+ ?p3 1)))))

?f <- (x ?y&?x)

(foo (x ?x) (y ?x))
```

## Incompatible Ferric Behavior
Ferric still diverges in key areas:
- Predicate/return-value constraints are preserved and enforced for positive ordered/template patterns, and negated support now covers simple comparison/literal-variable shapes, linear integer arithmetic around a single variable (including slot-side and nested `(+/- ...)` forms), and selected `str-compare` comparison shapes, but full negated expression support is still missing.
- Full CLIPS behavior for predicate constraints inside negation/NCC contexts is still missing.
- Diagnostics still differ from CLIPS warning-vs-error behavior in several files.

Observed current behavior (2026-02-26, updated after disjunctive expansion and broader linear negated lowering):
- `?f <- (x ?y&?x)` now compiles.
- `(foo (x ?x) (y ?x))` now compiles and enforces same-slot equality semantics.
- `:(...)` and `=(...)` constraints are preserved in Stage 2 AST and enforced for positive patterns.
- Simple negated predicate comparisons (for example `?x&:(> ?x ?min)`) are now lowered to join/alpha tests and no longer require unsupported diagnostics.
- Simple negated return-value constraints using literal/variable expressions (for example `=?x`) are now lowered and enforced.
- Negated predicate/return constraints using linear variable-offset expressions (for example `:(> ?x (+ ?min 1))`, `=(+ ?x 1)`) are now lowered to offset-aware join/alpha tests.
- Negated predicate/return constraints continue to lower when the current slot variable is itself wrapped in linear integer arithmetic (for example `:(> (+ ?x 1) ?min)`, `=(+ (+ ?x 1) 1)`), including nested `+`/`-` forms that normalize to a single-variable affine form.
- Negated predicate constraints using `str-compare`-vs-zero forms (for example `:(> (str-compare ?a ?b) 0)`) are now lowered to lexeme join tests (string comparisons).
- Complex negated predicate/return forms tied to a slot-local variable (for example nested non-linear function expressions such as `?x&:(> (* ?x ?x) (* ?min ?min))` or `?x&=(* ?x ?x)`) now compile via a runtime fallback check.
- Negated predicate/return forms that do not involve a slot-local variable still produce explicit compile-time unsupported diagnostics (for example CEERR-style unbound variable expressions).

## Root Cause For Ferric Divergence
The divergence is split across parser/runtime boundaries:

1. Negation path still lacks full executable predicate constraint support.
- In `crates/ferric-runtime/src/loader.rs`, binary comparisons (including normalized single-variable linear integer arithmetic and selected `str-compare`-vs-zero shapes) are lowered to core join/alpha tests, and complex slot-local expressions now have a runtime fallback evaluator in action-time test conditions. Full parity is still missing for broader forms (for example expressions without a slot-local variable anchor and template-side equivalents).

2. Or-constraint/predicate combinations are still only partially aligned.
- Slot-level/top-level `Constraint::Or` disjunctions are now distributed into rule variants via Cartesian expansion, but full CLIPS backtracking parity for all mixed alternatives is not yet guaranteed.

3. Warning-vs-error compatibility is incomplete.
- Some constructs that CLIPS compiles with warnings are still hard errors in Ferric; this document remains blocked until diagnostics policy is aligned.

## High-Level Sketch Of Required Changes
To align with CLIPS behavior, Ferric needs all of the following, not just one local patch:

1. Preserve full constraint intent in AST.
- Extend parser constraint AST to retain predicate/return-value nodes (instead of collapsing to wildcard).

2. Extend compiled pattern model for intra-pattern relational checks.
- Add a representation for field/slot comparisons and predicate tests that can be evaluated against a single candidate fact.

3. Update compile pipeline to stop rejecting valid CLIPS forms.
- Replace current hard rejection of same-pattern variable reuse with explicit generated equality/inequality tests.

4. Introduce robust diagnostics policy.
- Separate hard errors from CLIPS-style warnings where CLIPS compiles but warns.

## Tentative Implementation Plan (Session-Sized Passes)

### Pass 1: Characterization Harness For Remaining Failing Shapes
Goal: lock down current failing and expected behavior before semantic changes.

Changes:
- Add targeted parser/loader/core tests for:
  - forward reference constraints,
  - same-variable cross-slot reuse,
  - predicate-in-chain constraints with `:(...)` and `=(...)`.
- Add focused compatibility fixtures for zebra/misclns/fctpcstr slices.

Validation:
- `cargo test -p ferric-parser` (new constraint parser tests)
- `cargo test -p ferric-core` (compiler structure tests)
- `cargo test -p ferric-runtime` (loader/integration tests)

Expected end-of-pass state:
- No behavior changes yet, but test coverage identifies exact target deltas.

### Pass 2: Non-Lossy Constraint AST Representation
Goal: preserve predicate/return-value constraints through Stage 2 interpretation.

Changes:
- Introduce explicit constraint variants for predicate and return-value forms.
- Preserve embedded expression payloads needed for later evaluation.
- Keep backward-compatible parsing for already-supported forms.

Validation:
- Parser tests assert AST includes new nodes (not wildcard fallback).

Expected end-of-pass state:
- Parser/interpreter compiles and all existing behavior remains stable.

### Pass 3: Runtime/Compile Data Model For Intra-Pattern Tests
Goal: represent per-pattern relational tests without overloading cross-pattern beta joins.

Changes:
- Extend `CompilablePattern` (or adjacent structure) with intra-pattern test descriptors.
- Define test types: slot-equals-slot, slot-not-equals-slot, slot-vs-expression predicate.

Validation:
- Core unit tests for compiling patterns with repeated symbols in one pattern.

Expected end-of-pass state:
- Data path exists, execution may still be stubbed/partial.

### Pass 4: Match-Time Execution Of Intra-Pattern Relational Tests
Goal: enforce same-pattern variable/predicate semantics during matching.

Changes:
- Evaluate new per-pattern tests in alpha-side filtering or equivalent single-fact validation stage.
- Remove compiler hard-fail path for supported intra-pattern reuse forms.

Validation:
- Existing regressions from Pass 1 flip to passing for supported forms.

Expected end-of-pass state:
- Forward-reference and intra-pattern equality cases load and match correctly for targeted shapes.

### Pass 5: Predicate Constraint Execution Semantics
Goal: execute preserved predicate/return-value constraints with CLIPS-like truth behavior.

Changes:
- Wire predicate expressions into evaluation path with proper variable binding context.
- Ensure distinction between connective syntax (`|`) and function symbol calls (`or`) remains intact.

Validation:
- Zebra-style and rulemisc fixture subsets produce expected pass/fail behavior.

Expected end-of-pass state:
- Predicate constraints are semantically meaningful, not wildcard no-ops.

### Pass 6: Diagnostics Alignment (Error vs Warning)
Goal: move incompatibilities that CLIPS treats as warnings out of hard-failure path.

Changes:
- Add warning emission where CLIPS warns and continues.
- Keep genuinely un-compilable forms as hard errors.

Validation:
- Compatibility runs show fewer false hard-fails and better category alignment.

Expected end-of-pass state:
- Behavior and diagnostics are both closer to CLIPS for this feature area.

## Collateral Compatibility Damage Risks
Attempting to close this gap carries real regression risk:

1. Matching semantics regressions.
- Any change to variable binding and connected-constraint evaluation can alter activation sets for existing rules that currently pass.

2. Performance regressions.
- Added per-candidate-fact predicate checks may increase alpha-side cost significantly on large working memories.

3. Over-correction risk.
- If warning/error boundaries are implemented too aggressively, Ferric may start accepting constructs that should still be rejected, or emit noisy warnings that break existing test baselines.

4. Rete/compiler invariants risk.
- Relaxing same-pattern variable restrictions without a sound execution model can produce subtle false matches.

5. Cross-feature interaction risk.
- Changes here can impact `not`, `exists`, NCC behavior, and any logic that depends on current constraint flattening assumptions.

## Cost Of Doing Nothing
Leaving this compatibility gap open has concrete user cost:

1. Some CLIPS rule sets cannot be loaded without source rewrites.
- Users porting constraint-heavy suites (for example zebra/misclns variants) hit hard compile failures.

2. Rule authors lose expressive CLIPS constraint idioms.
- Intra-pattern equality and connected predicate constraints are common in expert-system code.

3. Silent semantic drift remains possible.
- Predicate forms reduced to wildcard-like behavior can produce false positives (rules firing when they should not).

4. Increased migration burden.
- Workarounds often require rewriting constraints as explicit `test` CEs or splitting a single pattern into multiple patterns, which is error-prone and reduces drop-in compatibility.

Plausible blocked scenarios and workarounds:
- Scenario: a user imports a CLIPS puzzle/diagnostic knowledge base using connected constraints and forward references.
  - Likely outcome today: load-time failure or behavior drift.
  - Workaround: manual rule rewrites to avoid connected constraints and repeated variable use.
  - Practical downside: significant time cost, high risk of changing logic unintentionally.

- Scenario: a user relies on compact single-pattern consistency checks across multiple slots.
  - Likely outcome today: compile rejection.
  - Workaround: decompose into broader matches plus explicit runtime tests.
  - Practical downside: more complex rules, potentially worse performance, less CLIPS parity.

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
- Some predicate-style constraints are accepted syntactically but not preserved semantically as true predicate checks.
- Forward-reference/same-pattern variable reuse cases can fail with `pattern validation failed`.
- Same-variable reuse across multiple slots/fields in a single pattern is rejected by core compile-time structure checks.

Observed current behavior (2026-02-25):
- Minimal `:(or (= ...) (= ...))` chain can load.
- `?f <- (x ?y&?x)` fails.
- `(foo (x ?x) (y ?x))` fails.

## Root Cause For Ferric Divergence
The divergence is split across parser/runtime boundaries:

1. Parser representation is lossy for predicate/return-value constraints.
- In `crates/ferric-parser/src/stage2.rs`, unary `:` and `=` forms are currently consumed and reduced to wildcard placeholders in constraint parsing, rather than represented as first-class predicate constraints.
- This means Ferric often accepts syntax but cannot enforce CLIPS-equivalent semantics at match time.

2. Core compiler forbids intra-pattern variable reuse.
- In `crates/ferric-core/src/compiler.rs`, `validate_pattern_structure()` rejects variable symbol reuse across different slots in one pattern (`intra-pattern equality is not supported at core compile stage`).
- The current `CompilablePattern` model and compilation flow are oriented around cross-pattern beta joins, not same-pattern relational checks.

3. Constraint execution model lacks a dedicated path for rich per-pattern predicate evaluation.
- Current translation and rete compile phases primarily support literals, simple variable bindings, and a limited set of connected constraints.
- Complex per-slot boolean predicates need explicit runtime/match-time representation and execution.

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

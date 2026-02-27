BLOCKED: # 214 Missing Compile-Time Validation Warnings

## Scope And Intent
This document describes how Ferric could close the remaining compatibility gap around CLIPS-style compile-time warnings (as opposed to hard compile errors or runtime failures).

## CLIPS Behavior (Reference)
CLIPS performs a large amount of static validation during load/compile and emits diagnostic codes while still continuing compilation in many cases.

Common warning/error classes relevant here:
- `[RULECSTR2]` incompatible slot constraints across patterns.
- `[CSTRNPSR1]` conflicting slot facets (for example `allowed-values` vs `type`).
- `[MODULPSR1]` invalid import/export module references.
- `[DEFAULT1]` invalid default value shape for slot cardinality/type.
- `[ARGACCES*]` function argument access/type diagnostics.

Key CLIPS property:
- Many diagnostics are warnings (or non-fatal errors in context), and valid constructs in the same load continue processing.

## Incompatible Ferric Behavior
Ferric currently has limited compile-time warning coverage and often either:
- silently accepts constructs CLIPS would warn about, or
- defers failures to runtime where CLIPS would provide an earlier compile-time diagnostic.

Current loader warnings are mostly focused on encoding/unsupported fact literal details, not full CLIPS compatibility diagnostics.

## Root Cause For Ferric Divergence
This is primarily a missing static-analysis and diagnostics-model capability issue.

1. Stage 2 AST does not carry enough facet metadata.
- `deftemplate` slot parsing currently keeps basic slot type and default, but not full CLIPS facet model needed for rich static checks.

2. Default-expression detail is collapsed.
- Function-call defaults are normalized (for example to `Derive`), which loses shape information required for `[DEFAULT1]`-class checks.

3. No dedicated compile-time diagnostics engine.
- Loader warnings are ad-hoc strings, not a structured compatibility diagnostics pipeline with code/severity/category.

4. Constraint propagation is largely absent.
- `[RULECSTR2]`-class warnings require variable/slot constraint flow across patterns and joins, which Ferric does not yet model statically.

5. Module parse-time linting is limited.
- Visibility is enforced dynamically in many paths, but CLIPS-style module diagnostics at definition/load time are not fully mirrored.

## High-Level Sketch Of Required Changes
Closing this gap requires a staged diagnostics/lint subsystem, not just one patch.

1. Introduce structured load diagnostics.
- Move from plain warning strings to structured diagnostics with code, severity, span, and message templates.

2. Preserve richer parse metadata.
- Extend template slot AST to retain relevant facet declarations and raw default-expression form.

3. Add compile-time validators by domain.
- Module validator, template/facet validator, default-value validator, and (later) rule-constraint propagation validator.

4. Define compatibility policy boundaries.
- For each diagnostic class, decide: hard error, warning, or informational, with CLIPS-guided defaults.

## Tentative Implementation Plan (Session-Sized Passes)

### Pass 1: Diagnostic Infrastructure Foundation
Goal: establish a consistent diagnostics representation without changing core semantics.

Changes:
- Add structured diagnostic type for loader output (`code`, `severity`, `span`, `message`).
- Keep legacy warning strings temporarily for compatibility with existing APIs/tests.

Validation:
- Loader tests for new diagnostic format serialization/display.

Expected end-of-pass state:
- Engine still behaves the same; diagnostics are now ready for extension.

### Pass 2: Module Import/Export Static Validation
Goal: cover highest-value low-risk warning class first.

Changes:
- Validate `defmodule` imports/exports against known module/construct declarations where determinable.
- Emit `[MODULPSR1]`-like diagnostics for invalid references.

Validation:
- Add module-focused fixtures and verify diagnostics emitted at load time.

Expected end-of-pass state:
- Better early feedback for cross-module wiring errors without runtime execution.

### Pass 3: Preserve Template Facet Metadata
Goal: make slot-level static checks possible.

Changes:
- Extend Stage 2 slot representation to retain facet declarations relevant to CLIPS checks (`type`, `allowed-values`, cardinality-related info, etc.).
- Preserve raw default expression forms needed for default-shape validation.

Validation:
- Parser/interpreter tests assert facet/default metadata survives interpretation.

Expected end-of-pass state:
- No behavior changes yet, but data required for warning rules is available.

### Pass 4: Default-Value And Facet Consistency Checks
Goal: implement `[DEFAULT1]` and `[CSTRNPSR1]`-class diagnostics.

Changes:
- Validate single-slot vs multislot default value shape.
- Validate obvious facet conflicts (`type` vs `allowed-values`, impossible combinations).
- Emit non-fatal compatibility warnings where CLIPS warns.

Validation:
- Dedicated template warning tests and selected compatibility fixtures.

Expected end-of-pass state:
- Template-time warnings available with minimal runtime behavior impact.

### Pass 5: Limited Static Function Call Linting
Goal: add pragmatic compile-time call-site diagnostics without full type inference.

Changes:
- Add literal-argument arity/type linting for selected builtins where safe.
- Emit `[ARGACCES*]`-style diagnostics only when deterministically known.

Validation:
- Unit tests for literal call-site lint behavior.

Expected end-of-pass state:
- Better early diagnostics without high false-positive rate.

### Pass 6: Constraint Propagation Pilot (`[RULECSTR2]` Subset)
Goal: tackle the hardest class in a narrow, controlled scope.

Changes:
- Implement minimal variable constraint graph for common two-pattern/shared-variable cases.
- Emit warnings only for high-confidence incompatibilities.

Validation:
- Focused compatibility fixtures for known `[RULECSTR2]` scenarios.

Expected end-of-pass state:
- Partial, useful coverage of cross-pattern type mismatch warnings.

### Pass 7: Expand Coverage Or Freeze With Explicit Boundaries
Goal: prevent perpetual half-implementation ambiguity.

Changes:
- Either broaden propagation support incrementally, or document intentionally unsupported warning classes and keep diagnostics conservative.

Validation:
- Compatibility report deltas reviewed and categorized.

Expected end-of-pass state:
- Clear boundary between implemented and intentionally deferred warning behaviors.

## Collateral Compatibility Damage Risks

1. False-positive warning noise.
- Over-eager static checks can flood users with warnings CLIPS would not emit, reducing trust in diagnostics.

2. False confidence from shallow checks.
- Underpowered checks can produce misleadingly sparse warnings, encouraging users to assume stronger static guarantees than Ferric actually provides.

3. API/format churn.
- Moving from plain strings to structured diagnostics may impact tools/scripts that parse current output text.

4. Parser representation risk.
- Extending AST/facet parsing may affect existing interpreted output and tests in unrelated template paths.

5. Runtime behavior coupling risk.
- If warning work accidentally mutates compile-time acceptance rules, compatibility could regress from warning-only differences to hard-failure differences.

## Cost Of Doing Nothing

1. Users lose early detection for constraint/configuration mistakes.
- Problems that CLIPS flags up front may appear only at runtime in Ferric, or not at all.

2. Migration from CLIPS becomes harder to trust.
- Teams comparing CLIPS and Ferric output see missing diagnostics and may treat Ferric as less transparent for validation workflows.

3. Increased debugging cost.
- Without compile-time guidance, users spend more time tracing downstream behavior anomalies.

4. Compliance/testing workflows remain noisy.
- Compatibility harnesses and regression reports continue to contain warning-class mismatches, obscuring genuinely functional incompatibilities.

Plausible blocked scenarios and workarounds:
- Scenario: a user relies on CLIPS warning output as a lint gate in CI.
  - Likely outcome today: Ferric under-reports lint issues.
  - Workaround: run CLIPS itself in parallel as authoritative linter.
  - Practical downside: dual-toolchain complexity and slower pipelines.

- Scenario: a user ports a large rule base with many templates/modules and wants confidence before runtime tests.
  - Likely outcome today: latent errors discovered later.
  - Workaround: heavy custom test harnesses and manual review.
  - Practical downside: longer feedback loops and higher maintenance effort.

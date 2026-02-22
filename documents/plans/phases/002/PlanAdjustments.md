# Phase 002 Plan Adjustments

This document describes updates needed in `documents/FerricImplementationPlan.md` so the master plan reflects the architecture and scope decisions introduced during Phase 002.

## 1) Clarify the parser-to-runtime-to-core compilation boundary (Sections 6.7, 8.3, 9.2)

### Required adjustment

Document the implemented layering explicitly:

- Stage 2 typed constructs are produced in `ferric-parser`.
- `ferric-runtime` translates parser patterns into `ferric-core::CompilableRule/CompilablePattern`.
- `ferric-core::ReteCompiler` remains parser-agnostic and compiles the translated model.

### Why

This decoupling is now the concrete implementation shape and is a meaningful architectural choice for crate boundaries.

### Phase 3 implication

Language-completion work should continue by extending the runtime translation layer and core compilable model, not by introducing parser dependencies into `ferric-core`.

## 2) Record exists-node design as a first-class implementation (Sections 7.4, 15 Phase 2)

### Required adjustment

Update text to state that exists is implemented as a dedicated support-counting node, not as a literal `not(not(...))` lowering.

### Why

That is the implemented strategy and matches the plan's efficiency rationale, but the plan should describe it as the canonical implementation choice.

### Phase 3 implication

Future forall/NCC work should interoperate with this exists memory model rather than replacing it.

## 3) Add an explicit status split between NCC infrastructure and full NCC semantics (Sections 7.3, 15 Phase 2/3)

### Required adjustment

Revise milestone wording so there is no ambiguity between:

- NCC scaffolding (memory/node types), and
- Full `(not (and ...))` runtime semantics (subnetwork result wiring, partner callbacks, end-to-end tests).

If full NCC remains deferred, move that semantic completion item to a clearly named early-Phase-3 deliverable.

### Why

Current implementation includes scaffolding but not full behavior. The existing Phase 2 wording reads as fully complete and creates status drift.

### Phase 3 implication

Phase 3 sequencing must schedule NCC semantic completion before forall work that depends on NCC behavior.

## 4) Tighten unsupported-construct policy wording to forbid silent degradation (Sections 2.3, 7.7, 15)

### Required adjustment

Add explicit language that unsupported pattern/constraint forms must emit compile-time diagnostics and fail rule load, and must never be silently dropped from a rule during translation.

### Why

This is already the intended policy in Sections 2.3/7.7, but Phase 2 implementation exposed the need for stricter wording and test gating.

### Phase 3 implication

New language features can be rolled out incrementally without semantic ambiguity, because unsupported edges fail loudly until implemented.

## 5) Clarify validation ownership contract (Sections 6.7, 7.7)

### Required adjustment

Pick and document one authoritative validation location:

- Either `ReteCompiler::compile_rule` owns mandatory validation, or
- Runtime pre-validation is the canonical gate and core compiler is intentionally "raw".

### Why

Phase 2 currently validates in runtime loader. The plan's Section 6.7 sketch places validator ownership in the compiler.

### Phase 3 implication

Whichever contract is chosen must be used consistently for new pattern forms (forall, nested constructs) to keep error codes stable and predictable.

## 6) Update node-sharing language to match delivered guarantees (Sections 6.2, 6.7, 15)

### Required adjustment

If join-node sharing is not implemented immediately, revise Phase 2 wording from "alpha/join sharing complete" to "alpha-path sharing complete; join sharing tracked separately" and place join sharing in a later optimization milestone.

### Why

Alpha sharing is delivered today; join canonicalization remains a separate decision.

### Phase 3 implication

Performance planning for larger rulebases should include join-sharing work as an explicit backlog item rather than an assumed completed property.

## 7) Narrow and annotate Phase 2 action semantics (Sections 9.2, 10, 15)

### Required adjustment

Annotate the Phase 2 RHS action subset to reflect implemented behavior:

- `assert` and `retract` are operational.
- `modify`/`duplicate` are currently ordered-fact oriented.
- Template-aware `modify`/`duplicate` remains later-phase scope.
- `printout` is a placeholder/no-op pending I/O infrastructure.

### Why

Phase 2 introduced action execution, but behavior is intentionally narrower than full CLIPS semantics.

### Phase 3 implication

Template metadata integration and I/O plumbing should be planned as explicit prerequisites for full action compatibility.

## 8) Strengthen exit checklist wording to require evidence artifacts (Section 15 Phase 2 exits)

### Required adjustment

Add explicit evidence requirements to the Phase 2 exit criteria:

- Real `.clp` fixture coverage for every claimed Phase 2 semantic area.
- Validation-failure regression tests for unsupported constructs.
- A "no open TODOs on required semantics" gate before marking phase complete.

### Why

Phase status drift occurred because completion language outpaced required semantic coverage.

### Phase 3 implication

Handoff quality improves and reduces rework when entering language-completion work.

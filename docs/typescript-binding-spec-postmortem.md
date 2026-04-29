# TypeScript Binding Spec Post-Mortem

Date: 2026-04-11

Primary artifact reviewed:
- [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md)

## Executive Summary
The implementation quality problems are less about one-off coding mistakes and more about a spec-to-execution gap:
- The spec is strong as an architectural narrative, but weak as an executable contract.
- Multiple high-risk behaviors are described informally, without unambiguous pass/fail criteria.
- The scope is very broad (native API + worker protocol + cancellation + pooling + package/distribution), but not staged with hard checkpoints.
- No required conformance test suite is defined in the spec, so “looks right” implementations can ship with core semantic drift.

The result is predictable: surface-level API shape got built, but boundary semantics (symbols, errors, cancellation, lifecycle) drifted badly.

## What Likely Went Wrong

### 1) The spec reads like a design doc, not a normative contract
The document excels at intent and architecture, but many sections use descriptive language where RFC-style MUST/SHOULD constraints were needed.

Consequence:
- Implementer optimizes for “matching concepts” rather than strict behavior.
- Drift accumulates at boundaries (wire format, error type reconstruction, shutdown semantics).

### 2) Key behavior contracts are ambiguous or internally uneven
Several requirements are underspecified in ways that make “reasonable” but incompatible implementations likely.

Examples:
- `limit` semantics:
  - `Engine.run(limit?)` does not explicitly define behavior for `limit = 0`.
  - `EvaluateRequest.limit` explicitly says `0` means unlimited.
  - Reference: [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:291), [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:527)
- `EnginePool.do()` cancellation:
  - Spec says `EnginePool.evaluate() / EnginePool.do()` use same cancellation semantics including queue/dequeue and in-execution behavior, but does not define exactly how a callback-style `do()` should abort and what promise outcome is normative.
  - Reference: [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:703)
- `close()` semantics:
  - Spec says pool close blocks until in-flight complete; many engineers would default to terminate/reject semantics unless explicitly tested/enforced.
  - Reference: [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:584)

### 3) Multi-layer boundary model was too implicit
The hardest parts here are boundary conversions (JS <-> native, main thread <-> worker, structured clone).

The spec does mention symbol serialization (`{ __type: "FerricSymbol", value }`) and reconstruction, but does not define a single canonical schema document + lifecycle across all call paths.
- Reference: [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:679), [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:883)

Consequence:
- Multiple competing symbol representations emerged (`FerricSymbol` instance vs different marker object forms), and conversions were implemented inconsistently.

### 4) Error model was conceptually specified, not mechanically specified
The spec defines a rich hierarchy and says workers should reconstruct by class name, but does not strictly define the transport/source-of-truth mapping table and fallback policy.
- Reference: [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:194), [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:671), [typescript-binding-api.md](/Users/prb/conductor/workspaces/ferric-rules/santo-domingo/docs/typescript-binding-api.md:851)

Consequence:
- Implementation satisfied “errors are thrown” but missed “correct typed errors are reconstructible and stable.”

### 5) Public TypeScript contract and runtime loading contract were not frozen
The spec shows concrete exports (`class Engine`, `class FerricSymbol`, `ClipsValue` includes symbol type), but does not include a required type-conformance section (strict compile checks of spec examples).

Consequence:
- API can compile internally but still expose optional/incorrect external types for consumers.

### 6) No mandatory acceptance suite embedded in the spec
The doc has examples but no required conformance checklist or canonical test vectors.

Consequence:
- Zero-test or shallow-test implementations can appear complete.
- Regressions in semantics are found only by manual audit.

## Why This Session Felt “Uniquely Off-Base”
Compared with ordinary implementation misses, this one failed in multiple foundational seams simultaneously:
- Value representation seam (symbols)
- Error seam (typed classes)
- Cancellation seam (queued/in-flight consistency)
- Lifecycle seam (`close` and disposal behavior)
- Type contract seam (consumer-facing TS types)

That pattern usually indicates a spec that didn’t force alignment at seams, not just weak coding in one module.

## What To Do Differently Next Time

### 1) Split spec into two documents: architecture + normative contract
Keep the existing narrative, but add a companion “Normative Contract” with strict MUST language.

Minimum required sections:
- Canonical wire schema (single source of truth)
- Error mapping table (native error -> wire payload -> JS class)
- Cancellation state machine per API (`run`, `evaluate`, `do`)
- Lifecycle state machine (`open`, `closing`, `closed`) and promise outcomes
- Value conversion truth table with edge cases

### 2) Add a conformance matrix that is part of definition-of-done
Each matrix row should be a hard behavior, with expected runtime and type-level outcomes.

Example rows:
- `FerricSymbol` round-trip across `Engine`, `EngineHandle`, and `EnginePool`
- Parse error -> expected error class + code + message prefix
- `close()` during in-flight request -> exact expected promise result
- `limit = 0` behavior for each API

### 3) Require spec examples to type-check under strict mode
Add “Spec Examples Compile Gate”:
- `tsc --strict` must pass on all example snippets copied from spec
- This would have caught API-shape drift quickly.

### 4) Stage the project into milestones with exit criteria
Instead of implementing all layers at once:
1. Sync native surface + value/error fidelity
2. Worker handle + transport fidelity
3. Pool + cancellation semantics
4. Packaging/distribution

Each stage must pass a focused conformance subset before proceeding.

### 5) Define non-negotiable “risk seams” up front
For this binding, the high-risk seams were obvious:
- symbol representation
- typed errors
- cancellation
- close semantics

Require explicit test coverage and reviewer sign-off for each seam before merge.

### 6) Add a pre-merge adversarial review checklist
Checklist should ask:
- “Can this behavior be interpreted in two reasonable ways?”
- “Do we have a failing test for the wrong interpretation?”
- “Is this represented identically across sync and async layers?”

## Immediate Process Changes Recommended
1. Create `docs/typescript-binding-conformance-matrix.md` and derive tests directly from it.
2. Add TS binding tests to package scripts and CI; fail if test count is zero.
3. Add one golden integration test per risk seam before any remediation coding.
4. Update the spec with explicit decision notes for ambiguous points (`limit=0`, `do()` cancellation outcomes, `close()` contract).

## Bottom Line
The session likely went poorly because the spec optimized for design clarity, not enforcement clarity. For complex multi-boundary bindings, those are different documents. You need both.

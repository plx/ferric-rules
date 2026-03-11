# Evaluator Recursion Hardening Plan (2026-03-09)

## Context
`eval` currently performs recursive expression evaluation, and user-function/generic dispatch also recurses through nested calls. Tracing instrumentation made this stack depth pressure visible sooner. The immediate mitigation is already in place:

- `eval` is instrumented once at the public entrypoint (`eval_root` span).
- recursive evaluation flows through a private helper (`eval_inner`).
- per-call observability uses events (`eval_call`) instead of nested spans.
- tracing builds apply a temporary conservative call-depth clamp to avoid stack overflow before long-term iterative work lands.

## Goals
- Eliminate stack-overflow sensitivity for deep but valid workloads.
- Preserve current semantics (CLIPS-compatible behavior, diagnostics, module visibility).
- Keep tracing informative with bounded overhead.

## Non-goals
- No language-surface changes.
- No rewrite of unrelated rete/action subsystems.

## Track 1: Callable Body Precompilation (Low Risk)
### Why
Today callable bodies (`deffunction`/`defmethod`) are translated from `ActionExpr` to `RuntimeExpr` on every invocation. This is avoidable work and makes later iterative execution harder.

### Scope
- Extend function/generic runtime data to cache precompiled `RuntimeExpr` bodies.
- Compile once at load/registration time.
- Remove per-call translation in `execute_callable_body`.

### Steps
1. Add precompiled body fields to `UserFunction` and registered methods.
2. Compile bodies during load with source-span-preserving errors.
3. Keep a fallback path behind a short-lived compatibility flag during rollout.
4. Delete fallback once all tests pass.

### Validation
- Existing phase3/phase4 tests unchanged and passing.
- New tests for compile-time body translation failure paths.
- Bench comparison: reduced overhead on deffunction-heavy workloads.

## Track 2: Explicit Callable Frame Stack (Medium Risk)
### Why
Current callable recursion consumes Rust call stack frames. We can replace callable-to-callable recursion with an explicit frame stack while keeping expression recursion local.

### Scope
- Introduce an internal `EvalFrame` stack for user-function/generic/call-next-method transitions.
- Convert dispatch recursion into iterative loop over frames.
- Keep non-call expression evaluation behavior equivalent.

### Steps
1. Design `EvalFrame` state (bindings, module, method-chain, PC over body expressions).
2. Implement iterative dispatcher loop for callable invocation.
3. Preserve `max_call_depth` behavior using explicit depth counter.
4. Keep root entrypoint tracing at one span; emit per-frame events from loop.

### Validation
- Regression suite for recursion-limit semantics and error spans.
- New stress tests for very deep callable recursion without stack overflow.
- Perf check with tracing off/on against current baseline.

## Track 3: Full Evaluator Trampoline / Bytecode VM (High Risk)
### Why
If expression recursion depth remains a risk (beyond call dispatch), a full iterative evaluator removes dependence on Rust stack entirely.

### Scope
- Replace recursive `eval_inner` with a trampoline/VM-style interpreter over explicit value/control stacks.
- Maintain source-span-rich errors and CLIPS semantics.

### Steps
1. Prototype minimal trampoline for call/if/while/loop-for-count/progn$/switch.
2. Add compatibility test harness comparing old/new evaluator outputs.
3. Migrate gradually behind a feature flag, then flip default.

### Validation
- Differential tests across random expression trees.
- Deep nesting stress tests (calls + control-flow nesting).
- Perf and allocation profiling before/after.

## Recommended Order
1. Track 1 first (wins now, enables later tracks).
2. Track 2 second (largest reliability gain per complexity).
3. Track 3 only if Track 2 + depth guards are still insufficient.

## Exit Criteria
- Deep recursion scenarios no longer fail due to Rust stack overflow.
- Tracing remains usable at depth without destabilizing execution.
- Non-tracing performance regression within agreed CI budget.

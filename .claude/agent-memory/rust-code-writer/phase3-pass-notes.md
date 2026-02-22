# Phase 3 Pass Implementation Notes

## Pass 002: Expression Evaluation Path for RHS and Test

Key design decisions and lessons:

### Test CE Semantics
- Test CEs (e.g., `(test (> ?x 0))`) are NOT compiled into Rete alpha/beta nodes.
- They are collected during `translate_rule_construct` into `TranslatedRule::test_conditions`.
- They are evaluated at rule-firing time in `execute_actions`.
- If a test CE evaluates falsy, the rule does NOT logically fire.

### Critical: rules_fired counting
- `run()` in `engine.rs` uses `rules_fired` which should only count activations where
  test CEs all passed. A test CE failure means the activation is suppressed.
- `execute_actions` returns `(bool, Vec<ActionError>)` where the bool indicates
  "did all test CEs pass and actions execute?"
- `run()` only increments `rules_fired` when that bool is `true`.
- `step()` always returns `Some(FiredRule)` because it represents popping from agenda,
  regardless of test CE outcome.

### Borrow splitting for EvalContext
- `from_action_expr` needs `&mut SymbolTable` to intern symbols for literals.
- `EvalContext` also holds `&'a mut SymbolTable`.
- These cannot coexist as simultaneous borrows.
- Solution: call `from_action_expr` FIRST (produces `RuntimeExpr`), THEN construct
  `EvalContext` for the `eval` call.
- This two-step translation → evaluation pattern is established in `eval_expr` in actions.rs.

### Pattern::Test handling location
- `Pattern::Test` is intercepted in `translate_rule_construct` BEFORE any call to
  `translate_condition` or `translate_pattern`.
- The `continue` statement skips the fact-index increment (test CEs are not fact patterns).
- The fallback `Pattern::Test` arm in `translate_pattern` is a safety net that should
  never be reached under normal operation.

### File changes for Pass 002
- `crates/ferric-runtime/src/actions.rs`: added `test_conditions` to `CompiledRuleInfo`,
  `EvalError` variant to `ActionError`, replaced `eval_expr` inline logic with evaluator
  pipeline, changed `execute_actions` return type to `(bool, Vec<ActionError>)`,
  removed unused `literal_to_value` function and `FerricString`/`LiteralKind` imports.
- `crates/ferric-runtime/src/loader.rs`: added `test_conditions` to `TranslatedRule`,
  wired up `test_conditions` in `compile_rule_construct`, updated `translate_rule_construct`
  to handle `Pattern::Test` early, updated loader test.
- `crates/ferric-runtime/src/engine.rs`: updated `execute_activation_actions` to return
  `bool`, updated `run()` to only count logically-fired rules.
- `crates/ferric-runtime/src/phase3_integration_tests.rs`: enabled two Pass 002 tests.

## Pass 010: Forall Limited Semantics

### Desugaring
`forall(P, Q)` desugars to `NCC([P, neg(Q)])` (not(and(P, not(Q))) in Rete terms).

### NCC Validation Change
Changed NCC subpattern validation from `allow_negated: false` to `allow_negated: true` in
`ferric-core/src/compiler.rs`. Negated subpatterns inside NCC are valid for forall desugaring.

### Initial-Fact Mechanism
- NCC at beta-root level needs a parent token for vacuous truth (when no P facts exist)
- Solution: inject synthetic `(initial-fact)` join as first condition when NCC is first CE
- `ensure_initial_fact()` asserts `(initial-fact)`, filtered from `facts()` via `initial_fact_id: Option<FactId>` on Engine
- Timing: call AFTER rule compilation but BEFORE deffacts assertion
- `reset()` must re-assert `(initial-fact)` if `initial_fact_id.is_some()`

### Critical Bug in rete.rs: retract_token_cascade Missing NCC Handling
**BUG SYMPTOM**: forall fires for vacuous truth (no P facts), but does NOT fire when all P have Q counterparts.

**ROOT CAUSE**: `retract_token_cascade` did not call `ncc_handle_result_retraction`. When the
negative node inside the NCC subnetwork blocked its pass-through (because Q fact arrived),
that pass-through was tracked as an NCC subnetwork result. The NCC result count never decremented.

**FIX** (in `ferric-core/src/rete.rs`):
1. Add `fact_base: &FactBase` and `new_activations: &mut Vec<ActivationId>` to `retract_token_cascade`
2. In the loop, call `ncc_handle_result_retraction(tid, fact_base, new_activations)` first
3. Update all callers: `negative_right_activate`, `ncc_partner_receive_result`, `exists_handle_retraction`
4. Update `assert_fact` to pass `fact_base` and `&mut new_activations` to `negative_right_activate`
5. Update `propagate_token` to pass `new_activations` to `ncc_partner_receive_result`

**KEY INSIGHT**: `negative_right_activate` can now produce new activations (via NCC unblocking),
so its results must be threaded back via `new_activations`.

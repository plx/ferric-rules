BLOCKED: # 216 Runtime Output Divergences (globltst, drtest03, dfrulcmd)

## Behavioral Divergence
A small set of files produce output from both engines but the output differs due to missing ferric runtime features. These are genuine behavioral gaps, not infrastructure issues.

### Case 1: globltst.clp — partial execution
Ferric produces partial output with errors:
```
Test Completed - Errors detected
```
The file uses a custom `printem` deffunction that references a global set by another rule's execution. Ferric reports `unknown function: printem` or `unbound variable: ?x` in contexts where CLIPS succeeds.

### Case 2: drtest03-02/03 — bind and multifield behavior
Files that test `(bind ?var ...)` on the RHS with multifield values show output differences. Ferric may handle `(bind ?var (create$ a b c))` differently from CLIPS in terms of the resulting multifield representation.

### Case 3: dfrulcmd-03 — undefrule and rules command
The file uses `(undefrule ...)` and `(rules)` introspection commands that ferric does not implement. Ferric outputs separator lines but skips the rule listings.

## Affected Files (~12)
- `clips-official/test_suite/globltst.clp` and telefonica equivalents (4 files)
- `generated/test-suite-segments/co-drtest03-02.clp` / `t64x-drtest03-02.clp`
- `generated/test-suite-segments/co-drtest03-03.clp` / `t64x-drtest03-03.clp`
- `generated/test-suite-segments/co-dfrulcmd-03.clp` / `t64x-dfrulcmd-03.clp` / `t65x-dfrulcmd-03.clp`

## Apparent Ferric-Side Root Cause
Multiple runtime gaps:

1. **Cross-rule deffunction resolution:** When a deffunction is defined in the same file as rules that call it, ferric may not resolve the deffunction when called from a rule's RHS if the deffunction definition is processed after the rule.

2. **Multifield bind semantics:** `(bind ?x (create$ a b c))` on the RHS — ferric's bind implementation may differ from CLIPS in how it stores and retrieves multifield values.

3. **Introspection commands:** `(undefrule ...)`, `(rules)`, `(ppdefrule ...)` are not implemented as callable functions from the RHS.

## Implementation Plan
These are three distinct, smaller fixes:

1. Verify deffunction resolution ordering.
   - Ensure deffunctions are registered before rule execution begins (after load, during reset or run).
   - Caveat: may already work; the globltst failure might be caused by a different issue.

2. Verify multifield bind semantics.
   - Test `(bind ?var (create$ a b c))` followed by `(printout t ?var)` — ensure multifield display matches CLIPS format.
   - Caveat: CLIPS displays multifields as space-separated values `(a b c)` vs. individual elements.

3. Implement `(undefrule ...)` (deferred).
   - Lower priority — only needed for introspection/debugging.
   - Caveat: requires removing compiled Rete network nodes, which is architecturally complex.

## Test And Verification
```bash
cargo run -p ferric -- run tests/examples/clips-official/test_suite/globltst.clp
cargo run -p ferric -- run tests/generated/test-suite-segments/co-drtest03-02.clp
```
Expected: investigate specific output differences and address root cause for each case.

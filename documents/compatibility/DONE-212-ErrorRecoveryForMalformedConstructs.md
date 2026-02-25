# 212 Error Recovery For Malformed Constructs

## Behavioral Divergence
CLIPS reports malformed constructs and continues processing later constructs.
Ferric was previously flagged as too fatal in this area.

This affects files that intentionally test error handling — they contain deliberately malformed constructs followed by valid ones that test the post-error state.

Examples:
```clips
;; bpgf3err — malformed deffunction:
(deffunction foo ())       ;; CLIPS: error, continues
(deffunction bar () 42)    ;; CLIPS: loads successfully

;; drtest03-08 — unknown slot:
(deftemplate a (slot one) (slot two))
(defrule r1 (a (three 3)) => )   ;; CLIPS: error "unknown slot three", continues
(defrule r2 (a (one 1)) => )     ;; CLIPS: loads successfully

;; drtest06-16 — undefined global reference:
(defglobal ?*x* = ?*r*)    ;; CLIPS: error, continues
```

Ferric now accumulates errors and continues loading subsequent constructs in the
same source stream for these malformed-construct scenarios.

## Affected Files (~40)
- `generated/test-suite-segments/co-bpgf3err-02.clp` through `co-bpgf3err-14.clp` (13 files)
- `generated/test-suite-segments/t64x-bpgf3err-02.clp` through `t64x-bpgf3err-14.clp` (13 files)
- `generated/test-suite-segments/co-drtest03-08.clp`
- `generated/test-suite-segments/t64x-drtest03-08.clp`
- `generated/test-suite-segments/co-drtest06-16.clp`
- `generated/test-suite-segments/t64x-drtest06-16.clp`
- `generated/test-suite-segments/co-drtest08-41.clp`
- `generated/test-suite-segments/t64x-drtest08-41.clp`
- `generated/test-suite-segments/jnftrght-01.clp`
- `generated/test-suite-segments/modlmisc-01.clp`
- `generated/test-suite-segments/modlmisc-02.clp`
- Various `sfmfmix-*.clp` files

## Implementation Summary
1. Recovery behavior was verified and covered with explicit regression tests in
`crates/ferric-runtime/src/loader.rs`:
- `load_recovers_after_malformed_deffunction_and_runs_later_constructs`
- `load_recovers_after_bad_rule_and_runs_other_rules`

2. These tests confirm:
- malformed constructs still produce diagnostics (`load_str` returns `Err(...)`);
- later valid constructs are still loaded into the engine;
- those valid constructs remain executable after `reset`/`run`.

## Scope Notes
- `ferric check` and `ferric run` still return a non-zero status when load errors
  are present; this is a CLI policy choice.
- Engine-level recovery for malformed constructs in the same input stream is
  now explicitly tested and preserved.

## Test And Verification
1. Unit tests:
```bash
cargo test -p ferric-runtime load_recovers_after
cargo test -p ferric-runtime loader::tests
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/co-bpgf3err-02.clp
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest03-08.clp
```
Expected: ferric reports errors for malformed constructs but continues loading subsequent valid constructs. Error messages should be similar (not necessarily identical) to CLIPS error messages.

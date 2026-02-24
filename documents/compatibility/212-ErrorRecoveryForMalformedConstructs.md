# 212 Error Recovery For Malformed Constructs

## Behavioral Divergence
CLIPS has robust error recovery: when it encounters a malformed construct (missing function body, invalid slot reference, undefined global reference, etc.), it reports an error and continues parsing subsequent constructs. Ferric often treats the first error as fatal, preventing later valid constructs from being processed.

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

Ferric's errors for these vary but commonly result in cascade failures where later valid constructs are also rejected.

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

## Apparent Ferric-Side Root Cause
Multiple locations across the parser and loader:

1. `crates/ferric-parser/src/stage2.rs` — when a construct parse fails, the parser does not skip to the next top-level `(` to attempt recovery.
2. `crates/ferric-runtime/src/loader.rs` — when a construct interpretation fails, the loader may abort rather than logging the error and continuing with the next construct.

CLIPS uses a "skip to matching close paren" strategy: when a construct fails to parse, it discards tokens until it reaches the balancing `)` of the current construct, then attempts to parse the next construct.

## Implementation Plan
1. Add skip-to-close-paren error recovery in the parser.
   - When a construct parse fails (deffunction, defrule, deftemplate, etc.), catch the error, record it, and advance the token stream to the matching `)` for the current construct.
   - Continue parsing at the next top-level `(`.
   - Caveat: paren balancing must handle nested `()` within the skipped construct.

2. Add error accumulation in the loader.
   - Instead of returning the first error, collect all errors and report them at the end.
   - Successfully-parsed constructs should still be loaded even if earlier constructs failed.
   - Caveat: a failed construct may define a template or function that later constructs depend on, causing cascade failures. This is acceptable — CLIPS has the same behavior.

3. Decide on exit code behavior.
   - When errors are encountered but some constructs load successfully: `ferric check` should report errors and exit with non-zero status.
   - `ferric run` should report errors but still execute the rules that loaded successfully (matching CLIPS behavior).
   - Caveat: this is a policy decision about strictness vs. compatibility.

## Test And Verification
1. Unit tests:
```bash
cargo test -p ferric-runtime error_recovery
```

2. Compatibility smoke checks:
```bash
cargo run -p ferric -- check tests/generated/test-suite-segments/co-bpgf3err-02.clp
cargo run -p ferric -- check tests/generated/test-suite-segments/co-drtest03-08.clp
```
Expected: ferric reports errors for malformed constructs but continues loading subsequent valid constructs. Error messages should be similar (not necessarily identical) to CLIPS error messages.

## Priority Note
This is a lower-priority fix compared to the others in this batch. The affected files are intentionally malformed test cases; fixing error recovery does not add new feature capability but improves compatibility with CLIPS's error-handling behavior. Files that depend on error recovery to produce meaningful output are inherently testing error handling, not rule execution.

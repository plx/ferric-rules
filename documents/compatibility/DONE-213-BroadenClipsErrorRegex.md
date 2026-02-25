# 213 Broaden CLIPS Error Regex In classify_results

## Behavioral Divergence
The compatibility runner's `classify_results()` function uses a regex to detect CLIPS-side errors in reference output. Currently it only matches `[PRNTUTIL]` and a few other error codes. Many files where CLIPS outputs error/warning messages (e.g., `[RULECSTR2]`, `[CSTRNPSR1]`, `[MODULPSR1]`, `[DEFAULT1]`, `[ARGACCES]`, `[GENRCPSR]`) are classified as `output-mismatch` when they should be `clips-error` (and thus reclassified as `incompatible`).

This means ~19 output-mismatch files are actually CLIPS-side errors that ferric handles differently (typically by silently accepting constructs that CLIPS warns about).

## Affected Files (~19 output-mismatch files that should be reclassified)
- `co-drtest08-13.clp` / `t64x-drtest08-13.clp` — `[RULECSTR2]` warning
- `co-drtest08-14.clp` / `t64x-drtest08-14.clp` — `[RULECSTR2]` warning
- `co-drtest08-33.clp` / `t64x-drtest08-33.clp` — type constraint conflict
- `co-drtest08-35.clp` / `t64x-drtest08-35.clp` — type constraint conflict
- `co-misclns4-05.clp` / `t63x-misclns4-05.clp` / `t64x-misclns4-05.clp` — RHS type warning
- `co-modulprt-01.clp` through `co-modulprt-14.clp` + `t63x-modulprt-*` — `[MODULPSR1]` import violation
- `co-drtest03-14.clp` / `t64x-drtest03-14.clp` — `[ARGACCES]` argument error
- `co-drtest07-70.clp` / `t64x-drtest07-68.clp` — system function override
- `drtest04-05.clp` — `[DEFAULT1]` multi-value default for single-field
- `misclns2-12.clp` — RHS type warning

## Apparent Root Cause
`scripts/compat-run.py` — the `classify_results()` function has a narrow error regex:
```python
clips_error_pattern = re.compile(r'\[PRNTUTIL\d+\]|\[PRCCODE\d+\]')
```

This misses many CLIPS error/warning codes. When CLIPS outputs warnings that ferric doesn't output, the result is classified as `output-mismatch` rather than recognizing that CLIPS itself had issues loading the file.

## Implementation Plan
1. Expand the CLIPS error regex to catch all standard CLIPS error codes.
   - CLIPS error codes follow the pattern `[XXXXXX#]` where X is uppercase letters and # is digits.
   - A comprehensive regex: `r'\[[A-Z]+\d+\]'`
   - Alternatively, enumerate known codes: `RULECSTR`, `CSTRNPSR`, `MODULPSR`, `DEFAULT`, `ARGACCES`, `GENRCPSR`, `PRNTUTIL`, `PRCCODE`, `DFFNXFUN`, `EXPRNPSR`, `FACTRHS`, `INSCOM`, `TMPLTDEF`, etc.
   - Caveat: the broad regex `\[[A-Z]+\d+\]` might match non-error output (unlikely but possible).

2. Reclassify affected files.
   - After broadening the regex, re-run `compat-run.py` to reclassify the ~19 files.
   - Files where CLIPS outputs errors/warnings should be classified as `clips-error` → `incompatible` rather than `output-mismatch` → `divergent`.
   - This should reduce the output-mismatch count from 48 to ~29.

## Implementation Note
This is an infrastructure fix in the compatibility tooling, not a ferric engine fix. It improves the accuracy of compatibility reporting by correctly classifying files where CLIPS itself has issues.

## Test And Verification
```bash
python3 scripts/compat-run.py --only-pending
python3 scripts/compat-report.py
```
Expected: ~19 files move from `output-mismatch` to `clips-error`, reducing the divergent count.

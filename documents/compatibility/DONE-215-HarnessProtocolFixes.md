# 215 Harness Protocol Fixes For Output-Mismatch Files

## Behavioral Divergence
Several output-mismatch files show differences not because of ferric engine bugs but because the CLIPS Docker reference harness does not properly execute them. The reference harness sends `(batch* "file.clp")` to CLIPS, which loads and executes constructs but does not automatically call `(reset)` and `(run)`. For files that define deffacts and rules but rely on `(reset)(run)` to execute, the CLIPS reference produces no output while ferric (which does `load → reset → run` automatically) produces correct output.

Examples:
- `wordgame.clp`: Ferric correctly outputs the word puzzle solution. CLIPS reference shows only banner text (no `(reset)(run)` was sent).
- `drtest10-07.clp`: Ferric correctly outputs `US`. CLIPS reference shows only banner text.

## Affected Files (~9)
- `clips-official/examples/wordgame.clp`
- `clips-official/test_suite/wordgame.clp`
- `telefonica-clips/branches/63x/examples/wordgame.clp`
- `telefonica-clips/branches/63x/test_suite/wordgame.clp`
- `telefonica-clips/branches/65x/test_suite/wordgame.clp`
- `generated/test-suite-segments/t63x-drtest10-07.clp`
- `generated/test-suite-segments/t64x-drtest10-07.clp`
- `generated/test-suite-segments/t65x-drtest10-07.clp`

## Apparent Root Cause
`scripts/clips-reference.sh` — the CLIPS Docker harness uses `(batch* "file.clp")` which loads all constructs and executes top-level commands, but deffacts and rules require `(reset)` to assert initial facts and `(run)` to fire rules.

The ferric CLI's `run` command does `load → reset → run` automatically, so ferric correctly executes these files.

## Implementation Plan
1. Update the CLIPS reference harness to send `(reset)(run)` after `(batch*)`.
   - Modify `scripts/clips-reference.sh` to append `(reset)\n(run)\n` after the `(batch* "file")` command.
   - This matches what ferric does automatically and what the CLIPS `manners*.bat` benchmark scripts do.
   - Caveat: some files already call `(reset)` and `(run)` internally (e.g., from deffacts or explicit top-level commands). Sending them again should be harmless — `(reset)` clears and reasserts, `(run)` with an empty agenda does nothing.

2. Alternative: add `(reset)(run)` only for files that need it.
   - Track in the manifest whether a file needs explicit `(reset)(run)`.
   - Caveat: more complex; the universal approach is simpler and should work for all files.

3. Re-run affected files after the harness fix.
   - Verify that CLIPS and ferric now produce equivalent output.

## Implementation Note
This is an infrastructure fix in the compatibility tooling, not a ferric engine fix. It corrects the test protocol so that CLIPS and ferric execute files under equivalent conditions.

## Test And Verification
```bash
# After fixing the harness:
python3 scripts/compat-run.py --files clips-official/examples/wordgame.clp
```
Expected: CLIPS now outputs the word puzzle solution, matching ferric's output. These files move from `output-mismatch` to `equivalent`.

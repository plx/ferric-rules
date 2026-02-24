# Future: CSP-Rules-V2.1 Integration Test

## Summary

[CSP-Rules-V2.1](https://github.com/denis-berthier/CSP-Rules-V2.1) is a large
CLIPS-based constraint satisfaction problem solver by Denis Berthier. It
includes solvers for Sudoku, Futoshiki, Kakuro, Latin Squares, Map Colouring,
Hidato, Numbrix, and Slitherlink puzzles. The corpus contains ~4,300 `.clp`
files totalling ~155 MB, plus ~312 example/data files in a companion
`csp-rules-examples` directory.

This would be an excellent end-to-end integration test for ferric once we
support the necessary features, as it exercises templates, rules, globals,
deffunctions, modules, and complex pattern matching in a real-world application.

## Why It's Not Testable Today

The corpus is **not a collection of standalone files**. It's a single
integrated application with strict inter-file dependencies:

1. **Config files** (e.g. `SudoRules-V20.1-config.clp`) call `(clear)`, set up
   global path variables (`?*CSP-Rules*`, `?*Application-Dir*`,
   `?*Directory-symbol*`), then orchestrate loading.

2. **Loader chains** use dynamic path construction:
   ```clips
   (load (str-cat ?*CSP-Rules-Generic-Dir* "GENERAL" ?*Directory-symbol* "globals.clp"))
   ```

3. **Conditional loading** based on feature-flag globals:
   ```clips
   (if ?*Whips[1]* then
       (load (str-cat ?*CSP-Rules-Generic-Dir* "CHAIN-RULES" ...)))
   ```

4. **Individual files reference templates and functions** defined in
   earlier-loaded files. Running any single file in isolation fails because
   templates like `(rank ...)`, `(unsolved ...)`, etc. are undefined.

When tested as standalone files, ~4,180 fail in **both** ferric and CLIPS
("both-error"), confirming this is a dependency issue, not a ferric bug.

## Prerequisites for Testing

To properly test this corpus, ferric would need:

- **`load` / `batch*` commands** -- currently classified as unsupported. These
  are the mechanism for multi-file loading.
- **`str-cat` in top-level expressions** -- for dynamic path construction
  during the load sequence.
- **`clear` command** -- to reset the engine state before loading (we have
  deferred-clear support in RHS actions, but not as a top-level command).
- **Global variable evaluation during loading** -- the conditional `(if
  ?*flag* then (load ...))` pattern requires evaluating globals at load time.

## Recommended Approach

1. Implement `load` / `batch*` as top-level commands.
2. Create a test harness that runs one of the config files (e.g.
   `SudoRules-V20.1-config.clp`) which bootstraps the full load sequence.
3. Feed a known Sudoku puzzle and verify the resolution path matches CLIPS
   output.
4. This tests the entire stack: parsing, template definitions, globals,
   deffunctions, modules, Rete network, and conflict resolution -- all
   exercised by a real application.

## References

- **Repository**: https://github.com/denis-berthier/CSP-Rules-V2.1
- **Documentation**: [User Manual and Research Notebooks for CSP-Rules](https://www.researchgate.net/publication/372364607_User_Manual_and_Research_Notebooks_for_CSP-Rules)
- **Removed from**: `tests/examples/csp-rules-v2.1/` and
  `tests/examples/csp-rules-examples/` (removed to reduce test corpus noise;
  re-clone from the repository above when ready to integrate)

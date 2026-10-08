# CLIPS Example Sources

Examples gathered for ferric-rules compatibility testing against classic CLIPS.

## Inventory

| Directory | .clp files | Source | Description |
|-----------|-----------|--------|-------------|
| `ferric-semantic/` | 57 | First-party | Structured semantic scenarios: 22 `fr-*` and 35 `rh-core-*` programs. The 21 companion `.stage` files are digest-bound scenario sources, not standalone entries. |
| `ferric-oracle/` | 1 | First-party | Empty-output semantic control; equivalence requires the declared final state, not matching empty output. |
| `clips-official/` | 126 | [smarr/CLIPS](https://github.com/smarr/CLIPS) | Official CLIPS source mirror with bundled examples (waltz, sudoku, circuit, manners, etc.) and test suite. Also includes 185 companion files (.bat, .tst, .fct, .dat). |
| `telefonica-clips/` | 520 | [Telefonica/clips](https://github.com/Telefonica/clips) | CLIPS fork with 63x, 64x, and 65x branches, examples, test suites, and clipsjni demos. 601 companion files. |
| `clips-executive/` | 64 | [carologistics/clips_executive](https://github.com/carologistics/clips_executive) | CLIPS executive framework for ROS-based robot planning (goal reasoning, plan execution). |
| `fawkes-robotics/` | 85 | [fawkesrobotics/fawkes](https://github.com/fawkesrobotics/fawkes) | Fawkes robotics framework; CLIPS-based agent reasoning and skill execution. |
| `rcll-refbox/` | 45 | [robocup-logistics/rcll-refbox](https://github.com/robocup-logistics/rcll-refbox) | RoboCup Logistics League referee box; game state management in CLIPS. |
| `labcegor/` | 6 | [carologistics/labcegor](https://github.com/carologistics/labcegor) | Lab robot CLIPS rules for Carologistics team. |
| `small-clips-examples/` | 4 | [garydriley/SmallCLIPSExamples](https://github.com/garydriley/SmallCLIPSExamples) | Small, self-contained CLIPS examples (from a CLIPS maintainer). |
| `learn-clips/` | 8 | [seanpm2001/Learn-CLIPS](https://github.com/seanpm2001/Learn-CLIPS) | Educational CLIPS examples and license/attribution files with `.clp` names. |
| `galletas/` | 3 | [Proyectos-Alejandro-BR-y-Elias-RR/Galletas_CLIPS](https://github.com/Proyectos-Alejandro-BR-y-Elias-RR/Galletas_CLIPS) | Cookie recipe expert system. |
| `diagnostico-covid/` | 2 | [carlospgraciano/se-diagnostico-covid](https://github.com/carlospgraciano/se-diagnostico-covid) | COVID diagnostic expert system. |
| `troubleshooting/` | 2 | [carlospgraciano/se-troubleshooting](https://github.com/carlospgraciano/se-troubleshooting) | PC troubleshooting expert system. |
| `missionaries-cannibals/` | 1 | [shahriar-rahman/CLIPS-Programming-Missionaries-Cannibals-Problem](https://github.com/shahriar-rahman/CLIPS-Programming-Missionaries-Cannibals-Problem) | Classic missionaries & cannibals puzzle solver. |
| `decision-tree-family/` | 1 | [shahriar-rahman/Clips-Programming-Decision-Tree-Family](https://github.com/shahriar-rahman/Clips-Programming-Decision-Tree-Family) | Family relationship decision tree. |
| `language-deficit-screener/` | 1 | [pierclgr/Language-Deficit-Screener](https://github.com/pierclgr/Language-Deficit-Screener) | Language deficit screening expert system. |

**Tracked source inventory: 926 `.clp` files + 786 imported companion files (338 `.bat`, 402 `.tst`, 42 `.fct`, 4 `.dat`) + 21 first-party `.stage` files.**

Counts come from `git ls-files tests/examples`, excluding four root JSON metadata
files and two Markdown files from the source/companion totals. There are 1,739
tracked files in this tree. These are physical path counts, including retained
copies in separate upstream bundles; they do not measure executable coverage.
The assessment scanner separately canonicalizes 1,264 `.clp`/`.bat` paths into
642 rows with 622 aliases, preserving all 58 oracle identities and all physical
bundle paths. See [compatibility assessment](../../docs/compatibility-assessment.md).

## Notes

- `clips-official/` and `telefonica-clips/` both contain the **official CLIPS test suite** under `test_suite/` — these are the most authoritative reference for correctness testing.
- The robotics sources (`fawkes-robotics/`, `clips-executive/`, `rcll-refbox/`, `labcegor/`) use CLIPS in real-world embedded contexts with modules, deftemplates, and complex control flow.
- Companion `.bat` files are CLIPS batch scripts (not Windows batch files). `.tst` files are CLIPS test-driver scripts that load sources, run batches, and capture results. `.fct` files contain fact data; `.dat` files contain benchmark/test data.
- Identical files in different imported bundles retain their paths for relative loads, companion resources, and attribution. Unassessed inventory does not imply incompatibility or successful execution.

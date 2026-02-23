# CLIPS Example Sources

Examples gathered for ferric-rules compatibility testing against classic CLIPS.

## Inventory

| Directory | .clp files | Source | Description |
|-----------|-----------|--------|-------------|
| `clips-official/` | 126 | [smarr/CLIPS](https://github.com/smarr/CLIPS) | Official CLIPS source mirror with bundled examples (waltz, sudoku, circuit, manners, etc.) and test suite. Also includes 185 companion files (.bat, .tst, .fct). |
| `telefonica-clips/` | 520 | [Telefonica/clips](https://github.com/Telefonica/clips) | Another CLIPS source fork (64-bit branch). Includes examples, test suite, and clipsjni demos. 601 companion files. |
| `csp-rules-v2.1/` | 4,283 | [denis-berthier/CSP-Rules-V2.1](https://github.com/denis-berthier/CSP-Rules-V2.1) | Constraint satisfaction puzzle solver (Sudoku, Slitherlink, etc.). Large parameterized rule sets exercising advanced CLIPS features. |
| `csp-rules-examples/` | 312 | [denis-berthier/CSP-Rules-Examples](https://github.com/denis-berthier/CSP-Rules-Examples) | Example puzzles and configurations for CSP-Rules. |
| `clips-official/` | 126 | [smarr/CLIPS](https://github.com/smarr/CLIPS) | Official CLIPS examples and test suite mirror. |
| `clips-executive/` | 64 | [carologistics/clips_executive](https://github.com/carologistics/clips_executive) | CLIPS executive framework for ROS-based robot planning (goal reasoning, plan execution). |
| `fawkes-robotics/` | 85 | [fawkesrobotics/fawkes](https://github.com/fawkesrobotics/fawkes) | Fawkes robotics framework; CLIPS-based agent reasoning and skill execution. |
| `rcll-refbox/` | 45 | [robocup-logistics/rcll-refbox](https://github.com/robocup-logistics/rcll-refbox) | RoboCup Logistics League referee box; game state management in CLIPS. |
| `labcegor/` | 6 | [carologistics/labcegor](https://github.com/carologistics/labcegor) | Lab robot CLIPS rules for Carologistics team. |
| `small-clips-examples/` | 4 | [garydriley/SmallCLIPSExamples](https://github.com/garydriley/SmallCLIPSExamples) | Small, self-contained CLIPS examples (from a CLIPS maintainer). |
| `learn-clips/` | 8 | [seanpm2001/Learn-CLIPS](https://github.com/seanpm2001/Learn-CLIPS) | Educational CLIPS examples. |
| `galletas/` | 3 | [Proyectos-Alejandro-BR-y-Elias-RR/Galletas_CLIPS](https://github.com/Proyectos-Alejandro-BR-y-Elias-RR/Galletas_CLIPS) | Cookie recipe expert system. |
| `diagnostico-covid/` | 2 | [carlospgraciano/se-diagnostico-covid](https://github.com/carlospgraciano/se-diagnostico-covid) | COVID diagnostic expert system. |
| `troubleshooting/` | 2 | [carlospgraciano/se-troubleshooting](https://github.com/carlospgraciano/se-troubleshooting) | PC troubleshooting expert system. |
| `missionaries-cannibals/` | 1 | [shahriar-rahman/CLIPS-Programming-Missionaries-Cannibals-Problem](https://github.com/shahriar-rahman/CLIPS-Programming-Missionaries-Cannibals-Problem) | Classic missionaries & cannibals puzzle solver. |
| `decision-tree-family/` | 1 | [shahriar-rahman/Clips-Programming-Decision-Tree-Family](https://github.com/shahriar-rahman/Clips-Programming-Decision-Tree-Family) | Family relationship decision tree. |
| `language-deficit-screener/` | 1 | [pierclgr/Language-Deficit-Screener](https://github.com/pierclgr/Language-Deficit-Screener) | Language deficit screening expert system. |

**Total: 5,463 .clp files + 800 companion files (.bat, .tst, .fct, .dat)**

## Notes

- `clips-official/` and `telefonica-clips/` both contain the **official CLIPS test suite** under `test_suite/` — these are the most authoritative reference for correctness testing.
- `csp-rules-v2.1/` is by far the largest collection. Many files are parameterized variants (e.g., `whips[1].clp` through `whips[36].clp`). They exercise complex pattern matching, modules, and salience heavily.
- The robotics sources (`fawkes-robotics/`, `clips-executive/`, `rcll-refbox/`, `labcegor/`) use CLIPS in real-world embedded contexts with modules, deftemplates, and complex control flow.
- Companion `.bat` files are CLIPS batch scripts (not Windows batch files). `.tst` are test expected-output files. `.fct` are fact data files.

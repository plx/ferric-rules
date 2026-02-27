# Batch Mode Prerequisites

This document describes what ferric needs to support in order to run CLIPS `.bat`
batch test scripts. These scripts are multi-step REPL sessions that interleave
construct definitions with immediate-mode commands.

## Current ferric REPL capabilities

The ferric REPL (`ferric repl`) currently supports:

- `(load "file.clp")` — load constructs from file
- `(clear)` — clear all constructs and facts
- `(reset)` — assert initial facts and deffacts
- `(run)` / `(run N)` — fire rules
- `(facts)` — list current facts
- `(agenda)` — show current agenda
- `(rules)` — list defined rules
- `(watch facts)` / `(watch rules)` / `(unwatch ...)` — toggle tracing
- `(save "file.clp")` — save constructs
- `(exit)` — quit REPL
- Inline construct definitions (defrule, deftemplate, etc.) via `load_str()`

## Missing features for .bat file support

### Tier 1: Top-level function calls (highest impact)

These are the most commonly used REPL commands in the test suite:

| Command | Usage count | Description |
|---------|-------------|-------------|
| `(assert ...)` | Very high | Assert facts at top level |
| `(retract ...)` | High | Retract facts by index |
| `(bind ?var ...)` | Medium | Bind variables in top-level scope |
| `(printout t ...)` | Medium | Print from top level (already works in RHS) |

**Implementation:** Extend the REPL's `load_str()` fallback to evaluate arbitrary
expressions via the expression evaluator when they don't parse as constructs.
This is the single most impactful change — it would make ~70% of non-extractable
.bat cycles feasible.

### Tier 2: Introspection commands

| Command | Description |
|---------|-------------|
| `(facts)` | Already implemented |
| `(agenda)` | Already implemented |
| `(matches <rule-name>)` | Show which facts match a rule's patterns |
| `(ppdefrule <name>)` | Pretty-print a rule definition |
| `(ppdeffacts <name>)` | Pretty-print a deffacts definition |
| `(ppdeftemplate <name>)` | Pretty-print a deftemplate definition |
| `(ppdeffunction <name>)` | Pretty-print a deffunction definition |
| `(ppdefglobal <module>)` | Pretty-print defglobals |
| `(list-defrules)` | List all rule names |
| `(list-deffacts)` | List all deffacts names |
| `(list-deftemplates)` | List all template names |
| `(list-deffunctions)` | List all function names |
| `(list-defglobals)` | List all global names |
| `(list-defmodules)` | List all module names |

### Tier 3: Control and strategy commands

| Command | Description |
|---------|-------------|
| `(set-strategy depth\|breadth)` | Change conflict resolution strategy |
| `(get-strategy)` | Query current strategy |
| `(refresh <rule-name>)` | Re-activate a rule that already fired |
| `(set-break <rule-name>)` | Set breakpoint on a rule |
| `(remove-break <rule-name>)` | Remove breakpoint |
| `(halt)` | Stop rule execution (already works in RHS) |
| `(set-salience-evaluation ...)` | Change salience evaluation timing |

### Tier 4: Construct manipulation

| Command | Description |
|---------|-------------|
| `(undefrule <name>)` | Remove a rule |
| `(undeffacts <name>)` | Remove a deffacts |
| `(undeftemplate <name>)` | Remove a template |
| `(undeffunction <name>)` | Remove a function |
| `(assert-string "(fact ...)")` | Assert a fact from a string |

### Tier 5: Fact I/O

| Command | Description |
|---------|-------------|
| `(load-facts "file.fct")` | Load facts from a file |
| `(save-facts "file.fct")` | Save facts to a file |

### Tier 6: Batch execution

| Command | Description |
|---------|-------------|
| `(batch "file.bat")` | Execute a batch file |
| `(batch* "file.bat")` | Execute a batch file (silent) |

## Test suite coverage by tier

Based on analysis of ~57 unique REPL test .bat files (excluding benchmarks
and cross-branch duplicates):

- **Tier 1 alone** would unlock the majority of non-extractable test cycles
- **Tiers 1+2** would cover most of the official CLIPS test suite
- **Tiers 1-4** would cover nearly all non-COOL test files
- **Tier 5** is needed only for `factsav.bat` and similar I/O tests
- **Tier 6** is needed for nested batch execution (rare)

## Recommended implementation order

1. **Top-level expression evaluation** (Tier 1) — biggest bang for the buck
2. **`(matches ...)`** — heavily used in rule-testing .bat files
3. **Pretty-print commands** (`pp*`) — needed for output comparison
4. **List commands** (`list-*`) — straightforward to implement
5. **Strategy/control** — needed for specific test files
6. **Construct removal** (`undef*`) — needed for dynamic test scenarios

## Relationship to `ferric run`

The `ferric run` command does load→reset→run and exits. It cannot support
interactive .bat scripts. Full .bat support requires the REPL to process
commands sequentially, maintaining state between commands.

An alternative approach is a `ferric batch` command that:
1. Reads a .bat file line by line
2. Processes each top-level form through the REPL command handler
3. Captures output for comparison

This would be simpler than full interactive REPL support while covering
the test suite use case.

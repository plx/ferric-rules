---
title: Internals
description: The main crates and the path from source to rule execution.
---

## Crates

| Crate                  | Responsibility                                               |
| ---------------------- | ------------------------------------------------------------ |
| `ferric-rules-parser`  | Lexer, S-expression parser, and CLIPS syntax tree.           |
| `ferric-rules-core`    | Rete network, agenda, facts, values, and matching.           |
| `ferric-rules-runtime` | Loading, execution, evaluation, modules, I/O, and snapshots. |
| `ferric-rules`         | Public facade re-exporting core, parser, and runtime.        |

The [workspace source](https://github.com/plx/ferric-rules/tree/main/crates) also
contains the CLI, FFI, and binding crates.

## From source to execution

The parser turns CLIPS source into a syntax tree. The loader registers
constructs such as templates and functions, then compiles rule patterns into a
Rete network.

As facts are asserted or retracted, the network updates its matches. Alpha nodes
test individual facts; beta nodes combine partial matches across patterns.
Complete matches produce activations on the agenda.

`run` takes activations from the agenda and executes rule actions. Those actions
can change facts, producing further matches, or write to output channels. The
loop ends when the agenda empties, a firing limit is reached, a halt is requested,
or an action fails.

## Tests

The repository contains parser and runtime tests, comparisons against CLIPS,
FFI and binding tests, and scaling checks. See the
[compatibility assessment](https://github.com/plx/ferric-rules/blob/main/docs/compatibility-assessment.md)
for how the CLIPS comparisons are evaluated, and [Performance](../performance/)
for benchmark commands.

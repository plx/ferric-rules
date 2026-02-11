---
name: undertake-plan-phase
description: Tells model to undertake a phase of the plan.
model: opus
disable-model-invocation: true
argument-hint: [phase-number]
---

You're being asked to undertake a phase of the implementation plan. Phases in this project are described in the `documents/plans/phases` directory.
Each phase is contained within a directory (e.g. `001` for the first phase).
Within that directory is a `Plan.md` file that describes the overall phase, a sequence of "passes" described in markdown files in the `passes` directory, and a `Progress.txt` file used to track describes the progress made so far.
The pass names start with a 3-digit number and then a short description of the pass (e.g. `001-WorkspaceBootstrapAndCI.md` for the first pass of the first phase); they are intended to be completed in ascending order.

When undertaking a phase, here's what you should do:

- locate the phase directory in `documents/plans/phases`
- read the `Plan.md` file to understand the overall phase
- read the `Progress.txt` file to understand the current progress
- read the next pass in the sequence that has not been completed (i.e. is not mentioned in `Progress.txt`)
- spawn a suitable agent team to implement the pass
- have the agent team work together to implement the pass
- update the `Progress.txt` file to mark the pass as completed
- shut down the agent team, leaving behind notes in a plan-specific `Notes.md` file about:
  - what was done
  - any remaining TODOs or FIXMEs
  - any lingering questions or uncertainties
  - any noteworthy decisions or trade-offs made
  - any updates to the overall plan that may need to be made
  - any suggestions for improvements to agents, tools, or processes
- continue to the next pass, etc., until the phase is complete

Note that unless explicitly directed *not* to do so, you should aim to include both "manual tests"—often direct translation of the examples from the CLIPS documentation—*and* property-based tests that thoroughly exercise the code vis-a-vis its expected behavior and invariants; additionally, these tests would be in addition to any "verification"-style tests that directly replicate tests from the CLIPS test suite (or from its documentation, etc.).

The reason for such an emphasis on testing is due to CLIPs being a very intricate and subtle system for which it's very, very important that each layer of code provide a robust, verified, trustworthy foundation for the layers above it.

Take your time, be careful, and make sure we're getting it right, layer by layer, piece by piece.

Without further ado, the specific phase you're being tasked with is: $ARGUMENTS

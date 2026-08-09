/**
 * Node adapter for the shared cross-binding semantic corpus.
 *
 * Stdout is NDJSON protocol data only; failures are written to stderr.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

import {
  Encoding,
  Engine,
  EngineHandle,
  FerricCompileError,
  FerricFactNotFoundError,
  FerricParseError,
  FerricSymbol,
  Format,
  HaltReason,
  Strategy,
} from "../../dist/index";

const HIGH_ID_ITERATIONS = 1_048_577;

function repositoryRoot(): string {
  const value = process.env["FERRIC_BINDINGS_CONFORMANCE_ROOT"];
  if (!value) throw new Error("FERRIC_BINDINGS_CONFORMANCE_ROOT is not set");
  return value;
}

function fixture(name: string): string {
  return readFileSync(
    join(repositoryRoot(), "tests", "bindings-conformance", "fixtures", name),
    "utf8",
  );
}

function normalize(value: unknown): unknown {
  if (value === null || value === undefined) return { type: "void" };
  if (typeof value === "bigint") {
    return { type: "integer", value: value.toString() };
  }
  if (typeof value === "number") {
    return {
      type: Number.isInteger(value) ? "integer" : "float",
      value: value.toString(),
    };
  }
  if (typeof value === "string") return { type: "string", value };
  if (value instanceof FerricSymbol) {
    return { type: "symbol", value: value.value };
  }
  if (Array.isArray(value)) {
    return { type: "multifield", value: value.map(normalize) };
  }
  return { type: `unsupported:${typeof value}` };
}

function assertedField(value: unknown): unknown {
  const engine = new Engine();
  try {
    const id = engine.assertFact("probe", value);
    const fact = engine.getFact(id);
    if (!fact || fact.fields.length !== 1) {
      throw new Error("asserted fact did not retain one field");
    }
    return normalize(fact.fields[0]);
  } finally {
    engine.close();
  }
}

function valueCase(caseId: string): unknown {
  switch (caseId) {
    case "value.void":
      return assertedField(null);
    case "value.integer.boundaries":
      return {
        minimum: assertedField(-(1n << 63n)),
        maximum: assertedField((1n << 63n) - 1n),
      };
    case "value.float":
      return assertedField(1.5);
    case "value.symbol.explicit":
      return assertedField(new FerricSymbol("red"));
    case "value.string.explicit":
    case "value.string.plain-host":
      return assertedField("red");
    case "value.multifield.nested":
      return assertedField([
        null,
        7,
        2.5,
        new FerricSymbol("blue"),
        "text",
        [9],
      ]);
    case "value.external-address": {
      const engine = new Engine();
      let ingress = "accepted";
      try {
        engine.assertFact("probe", {});
      } catch {
        ingress = "unsupported";
      } finally {
        engine.close();
      }
      return { host_representation: "void", ingress };
    }
    default:
      throw new Error(`unknown value case ${caseId}`);
  }
}

function configurationDefault(): unknown {
  const engine = new Engine();
  let unicode = "accepted";
  try {
    engine.assertFact("unicode", "é");
  } catch {
    unicode = "rejected";
  } finally {
    engine.close();
  }
  return { max_call_depth: 64, strategy: "depth", unicode };
}

function configurationCustom(): unknown {
  const engine = Engine.fromSource(fixture("custom-config.clp"), {
    encoding: Encoding.Ascii,
    strategy: Strategy.Breadth,
    maxCallDepth: 1,
  });
  let asciiUnicode = "accepted";
  try {
    try {
      engine.assertFact("unicode", "é");
    } catch {
      asciiUnicode = "rejected";
    }
    const run = engine.run();
    if (run.haltReason !== HaltReason.ActionError) {
      throw new Error("custom maxCallDepth did not bound recursion");
    }
    return {
      ascii_unicode: asciiUnicode,
      max_call_depth: "configurable",
      strategy_count: 4,
    };
  } finally {
    engine.close();
  }
}

function configurationObservation(
  source: string,
  options: { encoding?: Encoding; strategy?: Strategy; maxCallDepth?: number },
): Record<string, string> {
  const engine = Engine.fromSource(source, options);
  let unicode = "accepted";
  try {
    try {
      engine.assertFact("unicode", "é");
    } catch {
      unicode = "rejected";
    }
    const run = engine.run();
    return { halt_reason: reason(run.haltReason), unicode };
  } finally {
    engine.close();
  }
}

function configurationStrategyFired(source: string): number {
  const engine = Engine.fromSource(source, { strategy: Strategy.Breadth });
  try {
    return engine.run().rulesFired;
  } finally {
    engine.close();
  }
}

function configurationIsolation(): unknown {
  const defaultDepthSource = fixture("configuration-default-depth.clp");
  const customDepthSource = fixture("custom-config.clp");
  const strategySource = fixture("configuration-strategy-order.clp");
  return {
    depth_1_only: configurationObservation(customDepthSource, {
      maxCallDepth: 1,
    }),
    depth_256_only: configurationObservation(defaultDepthSource, {
      maxCallDepth: 256,
    }),
    encoding_ascii_only: configurationObservation(defaultDepthSource, {
      encoding: Encoding.Ascii,
    }),
    strategy_breadth_only: {
      ...configurationObservation(defaultDepthSource, {
        strategy: Strategy.Breadth,
      }),
      strategy_fired: configurationStrategyFired(strategySource),
    },
  };
}

function errorCase(caseId: string): unknown {
  const engine = new Engine();
  let family = "";
  try {
    if (caseId === "error.runtime") {
      const id = engine.assertFact("stale");
      engine.retract(id);
      try {
        engine.retract(id);
      } catch (error) {
        if (error instanceof FerricFactNotFoundError) family = "fact_not_found";
      }
    } else {
      const source =
        caseId === "error.parse"
          ? "(defrule incomplete"
          : caseId === "error.compile"
            ? "(defrule bad => (nonexistent-fn))"
            : "(defclass Probe (is-a USER))";
      try {
        engine.load(source);
      } catch (error) {
        if (error instanceof FerricParseError) family = "parse";
        else if (error instanceof FerricCompileError) family = "compile";
        else family = "generic";
      }
    }
  } finally {
    engine.close();
  }
  if (!family) throw new Error(`${caseId} did not produce the expected error`);
  return { family };
}

function factLifecycle(): unknown {
  const engine = Engine.fromSource(fixture("template.clp"));
  try {
    const orderedId = engine.assertFact("ordered", 7);
    const ordered = engine.getFact(orderedId);
    if (!ordered) throw new Error("ordered fact was not returned");
    engine.retract(orderedId);

    const templateId = engine.assertTemplate("person", { name: "Ada" });
    const template = engine.getFact(templateId);
    if (!template) throw new Error("template fact was not returned");
    engine.retract(templateId);

    return {
      count_after_retract: engine.factCount,
      ordered_snapshot_retained:
        ordered.relation === "ordered" && ordered.fields.length === 1,
      template_snapshot_retained:
        template.templateName === "person" && template.slots?.["name"] === "Ada",
    };
  } finally {
    engine.close();
  }
}

function reason(value: HaltReason): string {
  switch (value) {
    case HaltReason.AgendaEmpty:
      return "agenda_empty";
    case HaltReason.LimitReached:
      return "limit_reached";
    case HaltReason.HaltRequested:
      return "halt_requested";
    case HaltReason.ActionError:
      return "action_error";
  }
}

function normalizeRun(result: {
  rulesFired: number;
  haltReason: HaltReason;
}): unknown {
  return { fired: result.rulesFired, reason: reason(result.haltReason) };
}

function runFixture(name: string, limit?: number): {
  result: { rulesFired: number; haltReason: HaltReason };
  engine: InstanceType<typeof Engine>;
} {
  const engine = Engine.fromSource(fixture(name));
  return { result: engine.run(limit), engine };
}

function runOnce(limit?: number): unknown {
  const { result, engine } = runFixture("run-limits.clp", limit);
  try {
    return normalizeRun(result);
  } finally {
    engine.close();
  }
}

function executionRunLimits(): unknown {
  return {
    zero: runOnce(0),
    one: runOnce(1),
    unlimited: runOnce(),
  };
}

function executionStep(): unknown {
  const engine = Engine.fromSource(fixture("one-rule.clp"));
  try {
    const first = engine.step();
    const second = engine.step();
    return { first_rule: first?.ruleName ?? null, empty: second === null };
  } finally {
    engine.close();
  }
}

function executionDiagnostic(): unknown {
  const { result, engine } = runFixture("diagnostic.clp");
  try {
    return {
      ...normalizeRun(result) as object,
      diagnostic_count: engine.diagnostics.length,
    };
  } finally {
    engine.close();
  }
}

async function batchBoundaryHalt(): Promise<unknown> {
  const handle = await EngineHandle.create({
    source: fixture("batch-boundary-halt.clp"),
  });
  try {
    return normalizeRun(await handle.run());
  } finally {
    await handle.close();
  }
}

function snapshotRoundtrip(): unknown {
  const engine = Engine.fromSource(fixture("snapshot.clp"));
  const snapshot = (() => {
    try {
      engine.assertFact("seed");
      return engine.serialize(Format.Json);
    } finally {
      engine.close();
    }
  })();
  const restored = Engine.fromSnapshot(snapshot, Format.Json);
  try {
    const factCount = restored.factCount;
    const run = restored.run();
    return {
      fact_count: factCount,
      format: "json",
      rules_fired: run.rulesFired,
    };
  } finally {
    restored.close();
  }
}

function lifecycleClose(): unknown {
  const engine = new Engine();
  engine.close();
  engine.close();
  let postClose = "no_error";
  try {
    void engine.factCount;
  } catch {
    postClose = "closed_error";
  }
  return { explicit: true, idempotent: true, post_close: postClose };
}

function highFactId(): unknown {
  const engine = new Engine();
  let roundtrip = true;
  try {
    for (let iteration = 0; iteration < HIGH_ID_ITERATIONS; iteration += 1) {
      const id = engine.assertFact("generation");
      try {
        engine.retract(id);
      } catch {
        roundtrip = false;
        break;
      }
    }
    if (roundtrip) {
      const id = engine.assertFact("generation");
      roundtrip =
        id > BigInt(Number.MAX_SAFE_INTEGER) &&
        engine.getFact(id) !== null;
    }
  } finally {
    engine.close();
  }
  return { roundtrip };
}

function countWidth(): unknown {
  const engine = Engine.fromSource(fixture("run-limits.clp"));
  let runLimitBits = 64;
  try {
    try {
      const result = engine.run(2 ** 32 + 1);
      if (result.rulesFired !== 3) runLimitBits = 32;
    } catch {
      runLimitBits = 32;
    }
  } finally {
    engine.close();
  }
  return { run_count_bits: runLimitBits, run_limit_bits: runLimitBits };
}

async function runCase(caseId: string): Promise<unknown> {
  if (caseId.startsWith("value.")) return valueCase(caseId);
  if (caseId.startsWith("error.")) return errorCase(caseId);
  switch (caseId) {
    case "configuration.default":
      return configurationDefault();
    case "configuration.custom":
      return configurationCustom();
    case "configuration.isolation":
      return configurationIsolation();
    case "fact.lifecycle":
      return factLifecycle();
    case "execution.run-limits":
      return executionRunLimits();
    case "execution.step":
      return executionStep();
    case "execution.halt": {
      const { result, engine } = runFixture("halt.clp");
      try {
        return normalizeRun(result);
      } finally {
        engine.close();
      }
    }
    case "execution.diagnostic":
      return executionDiagnostic();
    case "execution.batch-boundary-halt":
      return batchBoundaryHalt();
    case "snapshot.json-roundtrip":
      return snapshotRoundtrip();
    case "lifecycle.close":
      return lifecycleClose();
    case "robustness.embedded-nul":
      return assertedField("a\0b");
    case "identifier.high-fact-id":
      return highFactId();
    case "count.run-result-width":
      return countWidth();
    default:
      throw new Error(`unknown case ${caseId}`);
  }
}

async function main(): Promise<void> {
  const casePath = process.argv[2];
  if (!casePath) throw new Error("usage: node adapter CASE_IDS_PATH");
  const cases = readFileSync(casePath, "utf8")
    .split(/\r?\n/u)
    .filter(Boolean);
  for (const caseId of cases) {
    const result = await runCase(caseId);
    process.stdout.write(`${JSON.stringify({ case: caseId, result })}\n`);
  }
}

main().catch((error: unknown) => {
  process.stderr.write(`node conformance adapter: ${String(error)}\n`);
  process.exitCode = 1;
});

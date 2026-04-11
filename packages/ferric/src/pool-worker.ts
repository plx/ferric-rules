/**
 * Worker thread entry point for EnginePool.
 *
 * Unlike the EngineHandle worker (which manages a single engine), the pool
 * worker manages multiple named engine instances — one per EngineSpec.
 *
 * Engines are created lazily on first use for each spec name.
 *
 * Protocol:
 * - "__init" — carries a PoolWorkerInit payload in args[0]; initialises the
 *   spec registry.
 * - All other methods — args[0] is `specName: string`, remaining args are
 *   the method arguments.
 * - "__evaluate" — special method that performs reset → assert → run → collect
 *   facts and output, all in one round-trip.
 * - "__batched_run" — batched run with cooperative abort-flag cancellation.
 *
 * Cooperative cancellation:
 * - Same SharedArrayBuffer mechanism as worker.ts.
 */

/* eslint-disable @typescript-eslint/no-require-imports */
import { parentPort } from "node:worker_threads";
import { resolve } from "node:path";
import type { WorkerRequest, WorkerResponse, PoolWorkerInit } from "./wire";
import { ABORT_FLAG_INDEX, RUN_BATCH_SIZE, toWire, isWireSymbol } from "./wire";
import type { NativeEngine } from "./native";
import type { EvaluateRequest, EvaluateResult } from "./types";

if (!parentPort) {
  throw new Error("pool-worker.ts must be run as a Worker thread");
}

// ---------------------------------------------------------------------------
// Load native addon
// ---------------------------------------------------------------------------

function loadNative(): Record<string, unknown> {
  const thisDir = __dirname;
  // bundledPath: packages/ferric/dist/ -> packages/ferric/native/
  const bundledPath = resolve(thisDir, "..", "native", "index.js");
  // developmentPath: packages/ferric/dist/ -> workspace root -> crates/ferric-napi/
  const developmentPath = resolve(thisDir, "..", "..", "..", "crates", "ferric-napi", "index.js");
  try {
    return require(bundledPath) as Record<string, unknown>;
  } catch {
    return require(developmentPath) as Record<string, unknown>;
  }
}

const native = loadNative();
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const NativeEngineClass = native["Engine"] as any;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const NativeFerricSymbol = native["FerricSymbol"] as any;

/**
 * Recursively convert wire symbols in args back to native FerricSymbol.
 */
function fromWireToNative(val: unknown): unknown {
  if (val === null || val === undefined) return val;
  if (typeof val !== "object") return val;

  if (isWireSymbol(val)) {
    return new NativeFerricSymbol(val.value);
  }

  if (Array.isArray(val)) {
    return val.map(fromWireToNative);
  }

  const result: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(val)) {
    result[k] = fromWireToNative(v);
  }
  return result;
}

// ---------------------------------------------------------------------------
// Spec registry and engine cache
// ---------------------------------------------------------------------------

interface SpecEntry {
  options?: { strategy?: number; encoding?: number; maxCallDepth?: number };
  source?: string;
}

/** Registered specs by name. */
const specs = new Map<string, SpecEntry>();
/** Lazily-created engines by spec name. */
const engines = new Map<string, NativeEngine>();

function getEngine(specName: string): NativeEngine {
  const existing = engines.get(specName);
  if (existing) return existing;

  const spec = specs.get(specName);
  if (!spec) {
    throw new Error(`Unknown engine spec: "${specName}"`);
  }

  const engine = new NativeEngineClass(spec.options ?? {}) as NativeEngine;
  if (spec.source) {
    engine.load(spec.source);
    engine.reset();
  }
  engines.set(specName, engine);
  return engine;
}

// ---------------------------------------------------------------------------
// Batched run with cooperative cancellation
// ---------------------------------------------------------------------------

function batchedRun(
  engine: NativeEngine,
  limit: number | undefined | null,
  abortBuffer: Int32Array | null,
): { rulesFired: number; haltReason: number } {
  const unlimited = limit === undefined || limit === null || limit <= 0;
  let remaining = unlimited ? Infinity : limit;
  let totalFired = 0;

  while (remaining > 0) {
    if (abortBuffer !== null && Atomics.load(abortBuffer, ABORT_FLAG_INDEX) !== 0) {
      engine.halt();
      break;
    }

    const batchLimit = Math.min(remaining, RUN_BATCH_SIZE);
    const result = engine.run(batchLimit);
    totalFired += result.rulesFired;

    if (result.haltReason !== 1 /* LimitReached */) {
      return { rulesFired: totalFired, haltReason: result.haltReason };
    }

    if (!unlimited) {
      remaining -= result.rulesFired;
    }
  }

  if (abortBuffer !== null && Atomics.load(abortBuffer, ABORT_FLAG_INDEX) !== 0) {
    return { rulesFired: totalFired, haltReason: 2 /* HaltRequested */ };
  }

  return { rulesFired: totalFired, haltReason: 1 /* LimitReached */ };
}

// ---------------------------------------------------------------------------
// Evaluate: stateless one-shot reset → assert → run → collect
// ---------------------------------------------------------------------------

function handleEvaluate(
  specName: string,
  request: EvaluateRequest,
  abortBuffer: Int32Array | null,
): EvaluateResult {
  const engine = getEngine(specName);

  engine.reset();

  // Assert requested facts (converting wire symbols to native).
  for (const fact of request.facts ?? []) {
    if (fact.kind === "ordered") {
      engine.assertFact(fact.relation, ...fact.fields.map(fromWireToNative));
    } else {
      engine.assertTemplate(fact.templateName, fromWireToNative(fact.slots) as Record<string, unknown>);
    }
  }

  // Run with cooperative cancellation.
  const runResult = batchedRun(engine, request.limit, abortBuffer);

  // Collect all facts.
  const facts = engine.facts() as EvaluateResult["facts"];

  // Collect output channels. Map CLIPS channel names to friendly names.
  const output: Record<string, string> = {};
  const tOutput = engine.getOutput("t");
  if (tOutput !== null) output["stdout"] = tOutput;
  const stderrOutput = engine.getOutput("stderr");
  if (stderrOutput !== null) output["stderr"] = stderrOutput;

  // Clear output so it doesn't accumulate across calls.
  engine.clearOutput("t");
  engine.clearOutput("stderr");

  return {
    runResult: { rulesFired: runResult.rulesFired, haltReason: runResult.haltReason },
    facts,
    output,
  };
}

// ---------------------------------------------------------------------------
// General method dispatch
// ---------------------------------------------------------------------------

function handleMethod(specName: string, method: string, args: unknown[]): unknown {
  const engine = getEngine(specName);

  switch (method) {
    case "load":
      engine.load(args[0] as string);
      return undefined;
    case "assertString":
      return engine.assertString(args[0] as string);
    case "assertFact": {
      const [relation, ...fields] = args as [string, ...unknown[]];
      return engine.assertFact(relation, ...fields.map(fromWireToNative));
    }
    case "assertTemplate":
      return engine.assertTemplate(args[0] as string, fromWireToNative(args[1]) as Record<string, unknown>);
    case "retract":
      engine.retract(args[0] as number);
      return undefined;
    case "getFact":
      return engine.getFact(args[0] as number);
    case "facts":
      return engine.facts();
    case "findFacts":
      return engine.findFacts(args[0] as string);
    case "run":
      return engine.run(args[0] as number | undefined);
    case "__batched_run": {
      const limit = args[0] as number | null | undefined;
      const sab = args[1] as SharedArrayBuffer | null;
      return batchedRun(engine, limit, sab ? new Int32Array(sab) : null);
    }
    case "step":
      return engine.step();
    case "halt":
      engine.halt();
      return undefined;
    case "reset":
      engine.reset();
      return undefined;
    case "clear":
      engine.clear();
      return undefined;
    case "getOutput":
      return engine.getOutput(args[0] as string);
    case "clearOutput":
      engine.clearOutput(args[0] as string);
      return undefined;
    case "pushInput":
      engine.pushInput(args[0] as string);
      return undefined;
    case "getFactCount":
      return engine.factCount;
    case "getIsHalted":
      return engine.isHalted;
    case "getAgendaSize":
      return engine.agendaSize;
    case "getCurrentModule":
      return engine.currentModule;
    case "getFocus":
      return engine.focus;
    case "getFocusStack":
      return engine.focusStack;
    case "rules":
      return engine.rules();
    case "templates":
      return engine.templates();
    case "modules":
      return engine.modules();
    case "getGlobal":
      return engine.getGlobal(args[0] as string);
    case "getDiagnostics":
      return engine.diagnostics;
    case "clearDiagnostics":
      engine.clearDiagnostics();
      return undefined;
    case "serialize":
      return engine.serialize(args[0] as number | undefined);
    default:
      throw new Error(`Unknown method: ${method}`);
  }
}

// ---------------------------------------------------------------------------
// Message loop
// ---------------------------------------------------------------------------

parentPort.on("message", (msg: WorkerRequest) => {
  const { id, method, args } = msg;

  try {
    // Init: register specs.
    if (method === "__init") {
      const init = args[0] as PoolWorkerInit;
      for (const spec of init.specs) {
        specs.set(spec.name, {
          options: spec.options,
          source: spec.source,
        });
      }
      parentPort!.postMessage({ id, result: undefined } satisfies WorkerResponse);
      return;
    }

    // Close: shut down all engines.
    if (method === "__close") {
      for (const [, engine] of engines) {
        try { engine.close(); } catch { /* ignore */ }
      }
      engines.clear();
      specs.clear();
      parentPort!.postMessage({ id, result: undefined } satisfies WorkerResponse);
      return;
    }

    // Evaluate: one-shot stateless call.
    if (method === "__evaluate") {
      const specName = args[0] as string;
      const request = args[1] as EvaluateRequest;
      const sab = args[2] as SharedArrayBuffer | null;
      const abortBuf = sab ? new Int32Array(sab) : null;
      const result = handleEvaluate(specName, request, abortBuf);
      // Convert FerricSymbol instances to wire format for structured clone.
      parentPort!.postMessage({ id, result: toWire(result) } satisfies WorkerResponse);
      return;
    }

    // General method dispatch: args[0] = specName, rest = method args.
    const specName = args[0] as string;
    const methodArgs = args.slice(1);
    const result = handleMethod(specName, method, methodArgs);

    // Zero-copy buffer transfer for serialize().
    if (method === "serialize" && result instanceof Buffer) {
      const arrayBuffer = result.buffer.slice(
        result.byteOffset,
        result.byteOffset + result.byteLength,
      );
      const resp: WorkerResponse = { id, result: arrayBuffer };
      parentPort!.postMessage(resp, [arrayBuffer as ArrayBuffer]);
      return;
    }

    // Convert FerricSymbol instances to wire format for structured clone.
    parentPort!.postMessage({ id, result: toWire(result) } satisfies WorkerResponse);
  } catch (err: unknown) {
    const e = err instanceof Error ? err : new Error(String(err));
    const resp: WorkerResponse = {
      id,
      error: {
        name: e.name,
        message: e.message,
        code: (e as { code?: string }).code ?? "FERRIC_ERROR",
      },
    };
    parentPort!.postMessage(resp);
  }
});

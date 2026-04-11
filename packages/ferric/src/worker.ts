/**
 * Worker thread entry point for EngineHandle.
 *
 * This script runs inside a Node.js Worker thread. It:
 * 1. Receives a "__init" request carrying WorkerInit options.
 * 2. Creates a synchronous native Engine on this OS thread.
 * 3. Enters a request loop, dispatching WorkerRequest messages to the engine
 *    and posting WorkerResponse messages back.
 *
 * Cooperative cancellation for run():
 * - The main thread passes a SharedArrayBuffer (Int32Array, length 1) in the
 *   "__run_batched" method's args.
 * - Between batches of RUN_BATCH_SIZE rule firings, this worker reads
 *   Atomics.load(abortBuf, 0). If non-zero, it calls engine.halt() and returns
 *   the partial RunResult.
 * - The main thread sets the flag to 1 when its AbortSignal fires.
 */

/* eslint-disable @typescript-eslint/no-require-imports */
import { parentPort } from "node:worker_threads";
import { resolve } from "node:path";
import type { WorkerRequest, WorkerResponse, WorkerInit } from "./wire";
import { ABORT_FLAG_INDEX, RUN_BATCH_SIZE, toWire, isWireSymbol, extractFerricError } from "./wire";
import type { NativeEngine } from "./native";

if (!parentPort) {
  throw new Error("worker.ts must be run as a Worker thread");
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
const NativeEngine = native["Engine"] as any;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const NativeFerricSymbol = native["FerricSymbol"] as any;

/**
 * Recursively convert wire symbols in args back to native FerricSymbol
 * before passing to the engine.
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

  // Plain object — convert values recursively (e.g. template slots).
  const result: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(val)) {
    result[k] = fromWireToNative(v);
  }
  return result;
}

// ---------------------------------------------------------------------------
// Engine state
// ---------------------------------------------------------------------------

let engine: NativeEngine | null = null;

// ---------------------------------------------------------------------------
// Init handler
// ---------------------------------------------------------------------------

function handleInit(init: WorkerInit): void {
  if (engine !== null) {
    try { engine.close(); } catch { /* ignore */ }
  }

  const opts = init.options ?? {};

  if (init.snapshot) {
    engine = NativeEngine.fromSnapshot(
      Buffer.from(init.snapshot.data),
      init.snapshot.format,
    ) as NativeEngine;
  } else {
    engine = new NativeEngine(opts) as NativeEngine;
    if (init.source) {
      engine.load(init.source);
      engine.reset();
    }
  }
}

// ---------------------------------------------------------------------------
// Batched run with cooperative cancellation
// ---------------------------------------------------------------------------

/**
 * Run the engine in batches of RUN_BATCH_SIZE, checking the abort flag
 * between each batch. Returns { rulesFired, haltReason } as a plain object.
 */
function batchedRun(
  limit: number | undefined | null,
  abortBuffer: Int32Array | null,
): { rulesFired: number; haltReason: number } {
  if (!engine) throw new Error("Engine is not initialized");

  // N-01: undefined/null = unlimited, 0 = zero firings, positive = max firings.
  if (limit === 0) {
    return { rulesFired: 0, haltReason: 1 /* LimitReached */ };
  }

  const unlimited = limit === undefined || limit === null;
  let remaining = unlimited ? Infinity : limit;
  let totalFired = 0;

  while (remaining > 0) {
    // Check for abort before each batch.
    if (abortBuffer !== null && Atomics.load(abortBuffer, ABORT_FLAG_INDEX) !== 0) {
      engine.halt();
      break;
    }

    const batchLimit = Math.min(remaining, RUN_BATCH_SIZE);
    const result = engine.run(batchLimit);
    totalFired += result.rulesFired;

    // HaltReason: 0 = AgendaEmpty, 1 = LimitReached, 2 = HaltRequested
    if (result.haltReason !== 1 /* LimitReached */) {
      // Agenda empty or halt requested — we're done.
      return { rulesFired: totalFired, haltReason: result.haltReason };
    }

    if (!unlimited) {
      remaining -= result.rulesFired;
    }
  }

  // Check if we stopped due to abort flag.
  if (abortBuffer !== null && Atomics.load(abortBuffer, ABORT_FLAG_INDEX) !== 0) {
    return { rulesFired: totalFired, haltReason: 2 /* HaltRequested */ };
  }

  return { rulesFired: totalFired, haltReason: 1 /* LimitReached */ };
}

// ---------------------------------------------------------------------------
// Method dispatch
// ---------------------------------------------------------------------------

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function handleMethod(method: string, args: unknown[]): unknown {
  if (!engine) {
    throw new Error("Engine is not initialized. Send __init before other requests.");
  }

  switch (method) {
    // Loading
    case "load":
      engine.load(args[0] as string);
      return undefined;
    case "loadFile":
      engine.loadFile(args[0] as string);
      return undefined;

    // Fact operations
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
    case "getFactSlot":
      return engine.getFactSlot(args[0] as number, args[1] as string);

    // Execution
    case "run":
      // Simple (non-batched) run, no cancellation support.
      return engine.run(args[0] as number | undefined);
    case "__run_batched": {
      // Batched run with cooperative cancellation.
      // args[0] = limit (number | null), args[1] = SharedArrayBuffer | null
      const batchLimit = args[0] as number | null | undefined;
      const sab = args[1] as SharedArrayBuffer | null;
      const abortBuf = sab ? new Int32Array(sab) : null;
      return batchedRun(batchLimit, abortBuf);
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

    // Introspection
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

    // Focus stack
    case "setFocus":
      engine.setFocus(args[0] as string);
      return undefined;
    case "pushFocus":
      engine.pushFocus(args[0] as string);
      return undefined;

    // I/O
    case "getOutput":
      return engine.getOutput(args[0] as string);
    case "clearOutput":
      engine.clearOutput(args[0] as string);
      return undefined;
    case "pushInput":
      engine.pushInput(args[0] as string);
      return undefined;

    // Diagnostics
    case "getDiagnostics":
      return engine.diagnostics;
    case "clearDiagnostics":
      engine.clearDiagnostics();
      return undefined;

    // Serialization
    case "serialize":
      return engine.serialize(args[0] as number | undefined);
    case "saveSnapshot":
      engine.saveSnapshot(args[0] as string, args[1] as number | undefined);
      return undefined;

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
    if (method === "__init") {
      handleInit(args[0] as WorkerInit);
      const resp: WorkerResponse = { id, result: undefined };
      parentPort!.postMessage(resp);
      return;
    }

    if (method === "__close") {
      if (engine) {
        try { engine.close(); } catch { /* ignore */ }
        engine = null;
      }
      const resp: WorkerResponse = { id, result: undefined };
      parentPort!.postMessage(resp);
      return;
    }

    const result = handleMethod(method, args);

    // For serialize(), wrap the Buffer as an ArrayBuffer transfer for zero-copy.
    // Node.js Buffers have an underlying ArrayBuffer we can transfer.
    if (method === "serialize" && result instanceof Buffer) {
      const arrayBuffer = result.buffer.slice(
        result.byteOffset,
        result.byteOffset + result.byteLength,
      );
      const resp: WorkerResponse = { id, result: arrayBuffer };
      parentPort!.postMessage(resp, [arrayBuffer as ArrayBuffer]);
      return;
    }

    // Convert FerricSymbol instances in results to wire format
    // so they survive structured clone across the thread boundary.
    const resp: WorkerResponse = { id, result: toWire(result) };
    parentPort!.postMessage(resp);
  } catch (err: unknown) {
    const e = err instanceof Error ? err : new Error(String(err));
    const errorPayload = extractFerricError(
      e.name,
      e.message,
      (e as { code?: string }).code,
    );
    const resp: WorkerResponse = { id, error: errorPayload };
    parentPort!.postMessage(resp);
  }
});

/**
 * EnginePool thread-count validation tests (FR-NODE-007 / E-014).
 */
import { execFile } from "node:child_process";
import { EventEmitter } from "node:events";
import { resolve } from "node:path";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EnginePool } from "../../../helpers/ferric";

const workerThreads = require("node:worker_threads") as typeof import("node:worker_threads");

const THREAD_RANGE_MESSAGE =
  "EnginePool.create: 'threads' must be a safe integer between 1 and 64";

class MockWorker extends EventEmitter {
  static constructorCalls = 0;
  static instances: MockWorker[] = [];
  static throwOnConstruct = false;

  terminateCalls = 0;

  static reset(options: { throwOnConstruct?: boolean } = {}): void {
    MockWorker.constructorCalls = 0;
    MockWorker.instances = [];
    MockWorker.throwOnConstruct = options.throwOnConstruct ?? false;
  }

  constructor(_filename: string) {
    super();
    MockWorker.constructorCalls += 1;
    if (MockWorker.throwOnConstruct) {
      throw new Error("validation allowed a Worker constructor side effect");
    }
    MockWorker.instances.push(this);
  }

  postMessage(message: { id: number; method: string }): void {
    if (message.method === "__init") {
      queueMicrotask(() => {
        this.emit("message", { id: message.id, result: undefined });
      });
    }
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

interface CapturedCreate {
  thrown: unknown;
  settleEscapedPromise: Promise<void>;
}

/**
 * Capture the direct-call outcome while safely containing the pre-fix async
 * implementation. The constructor tripwire prevents Infinity from spawning
 * an unbounded number of fake or real workers.
 */
function captureCreate(
  threads: unknown,
  specs: Parameters<typeof EnginePool.create>[0] = [{ name: "rules" }],
): CapturedCreate {
  let thrown: unknown;
  let returned: Promise<EnginePool> | undefined;
  try {
    returned = EnginePool.create(specs, { threads: threads as number });
  } catch (error) {
    thrown = error;
  }

  const settleEscapedPromise = returned
    ? returned.then(
        async (pool) => {
          await pool.close();
        },
        () => undefined,
      )
    : Promise.resolve();
  return { thrown, settleEscapedPromise };
}

test("E-014 invalid thread counts throw before spec inspection or Worker construction", async () => {
  const invalidCases: Array<{ label: string; value: unknown }> = [
    { label: "NaN", value: Number.NaN },
    { label: "+Infinity", value: Number.POSITIVE_INFINITY },
    { label: "-Infinity", value: Number.NEGATIVE_INFINITY },
    { label: "+0", value: 0 },
    { label: "-0", value: -0 },
    { label: "negative integer", value: -1 },
    { label: "fraction", value: 1.5 },
    { label: "positive unsafe integer", value: Number.MAX_SAFE_INTEGER + 1 },
    { label: "negative unsafe integer", value: Number.MIN_SAFE_INTEGER - 1 },
    { label: "maximum plus one", value: 65 },
    { label: "explicit null", value: null },
  ];

  const OriginalWorker = workerThreads.Worker;
  workerThreads.Worker = MockWorker as unknown as typeof workerThreads.Worker;

  try {
    for (const item of invalidCases) {
      MockWorker.reset({ throwOnConstruct: true });
      let specAccesses = 0;
      const poisonedSpecs = new Proxy(
        [] as Parameters<typeof EnginePool.create>[0],
        {
          get() {
            specAccesses += 1;
            throw new Error("validation inspected engine specs");
          },
        },
      );
      const captured = captureCreate(item.value, poisonedSpecs);
      await captured.settleEscapedPromise;

      assert.ok(
        captured.thrown instanceof RangeError,
        `${item.label} did not throw RangeError from the direct create() call`,
      );
      assert.strictEqual(captured.thrown.message, THREAD_RANGE_MESSAGE, item.label);
      assert.strictEqual(
        MockWorker.constructorCalls,
        0,
        `${item.label} reached the Worker constructor`,
      );
      assert.strictEqual(specAccesses, 0, `${item.label} inspected engine specs`);
    }
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

test("E-014 EnginePool accepts omitted, undefined, minimum, and maximum thread counts", async () => {
  const validCases: Array<{
    label: string;
    options?: { threads?: number };
    expectedThreads: number;
  }> = [
    { label: "omitted", expectedThreads: 1 },
    { label: "explicit undefined", options: { threads: undefined }, expectedThreads: 1 },
    { label: "minimum", options: { threads: 1 }, expectedThreads: 1 },
    { label: "maximum", options: { threads: 64 }, expectedThreads: 64 },
  ];

  const OriginalWorker = workerThreads.Worker;
  workerThreads.Worker = MockWorker as unknown as typeof workerThreads.Worker;

  try {
    for (const item of validCases) {
      MockWorker.reset();
      const pool = item.options === undefined
        ? await EnginePool.create([{ name: "rules" }])
        : await EnginePool.create([{ name: "rules" }], item.options);

      assert.strictEqual(
        MockWorker.constructorCalls,
        item.expectedThreads,
        item.label,
      );
      await pool.close();
      assert.deepStrictEqual(
        MockWorker.instances.map((worker) => worker.terminateCalls),
        Array.from({ length: item.expectedThreads }, () => 1),
        `${item.label} did not close every accepted worker`,
      );
    }
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

function runNodeScript(script: string): Promise<{ stdout: string; stderr: string }> {
  const packageRoot = resolve(__dirname, "../../../..");
  const childEnv = { ...process.env };
  childEnv.NODE_V8_COVERAGE = "";
  delete childEnv.NODE_TEST_CONTEXT;

  return new Promise((resolveRun, rejectRun) => {
    execFile(
      process.execPath,
      ["-e", script],
      {
        cwd: packageRoot,
        env: childEnv,
        timeout: 8_000,
        killSignal: "SIGKILL",
        windowsHide: true,
      },
      (error, stdout, stderr) => {
        if (error) {
          rejectRun(
            new Error(
              `Node subprocess failed: ${error.message}\nstdout:\n${stdout}\nstderr:\n${stderr}`,
              { cause: error },
            ),
          );
          return;
        }
        resolveRun({ stdout, stderr });
      },
    );
  });
}

test("E-014 hostile thread counts cannot create workers or keep a subprocess alive", async () => {
  const script = `
const workerThreads = require("node:worker_threads");
let constructorCalls = 0;
class ConstructorTripwire {
  constructor() {
    constructorCalls += 1;
    throw new Error("invalid thread count reached Worker construction");
  }
}
workerThreads.Worker = ConstructorTripwire;
const { EnginePool } = require("./dist");
const expected = ${JSON.stringify(THREAD_RANGE_MESSAGE)};
const reports = [];
const messagePortCount = () => process.getActiveResourcesInfo()
  .filter((resource) => resource === "MessagePort").length;
const before = messagePortCount();
for (const threads of [Infinity, 65]) {
  let thrown;
  let returned;
  try {
    returned = EnginePool.create([{ name: "rules" }], { threads });
  } catch (error) {
    thrown = error;
  }
  if (returned && typeof returned.then === "function") {
    returned.catch(() => undefined);
  }
  if (!(thrown instanceof RangeError) || thrown.message !== expected) {
    throw new Error(String(threads) + " did not throw the expected synchronous RangeError");
  }
  reports.push({ name: thrown.name, message: thrown.message });
}
if (constructorCalls !== 0) {
  throw new Error("Worker constructor was called " + constructorCalls + " times");
}
const after = messagePortCount();
if (after !== before) {
  throw new Error("Worker resources changed from " + before + " to " + after);
}
console.log(JSON.stringify({ after, before, constructorCalls, reports }));
`;

  const { stdout } = await runNodeScript(script);
  const report = JSON.parse(stdout.trim()) as {
    after: number;
    before: number;
    constructorCalls: number;
    reports: Array<{ name: string; message: string }>;
  };
  assert.strictEqual(report.after, report.before);
  assert.strictEqual(report.constructorCalls, 0);
  assert.deepStrictEqual(report.reports, [
    { name: "RangeError", message: THREAD_RANGE_MESSAGE },
    { name: "RangeError", message: THREAD_RANGE_MESSAGE },
  ]);
});

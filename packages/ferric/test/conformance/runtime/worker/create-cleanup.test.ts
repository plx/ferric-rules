/**
 * Failed EngineHandle.create() ownership and cleanup tests (D-010).
 *
 * These tests deliberately restrict synchronous-send fault injection to the
 * initialization transaction. Ordinary call()/run() rollback belongs to
 * FR-NODE-008.
 */
import { execFile } from "node:child_process";
import { EventEmitter } from "node:events";
import { resolve } from "node:path";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EngineHandle } from "../../../helpers/ferric";

const workerThreads = require("node:worker_threads") as typeof import("node:worker_threads");

interface InitRequest {
  id: number;
  method: string;
}

interface MockWorkerPlan {
  constructorError?: unknown;
  post?: (
    worker: MockWorker,
    message: InitRequest,
    transferList: readonly unknown[] | undefined,
  ) => void;
  terminate?: (worker: MockWorker) => Promise<number>;
}

interface CapturedPendingEntry {
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
}

interface CapturedHandleState {
  closed: boolean;
  pending: Map<number, CapturedPendingEntry>;
  worker: unknown;
}

let activePlan: MockWorkerPlan = {};

class MockWorker extends EventEmitter {
  static constructorCalls = 0;
  static instances: MockWorker[] = [];

  readonly messages: InitRequest[] = [];
  readonly transferLists: Array<readonly unknown[] | undefined> = [];
  terminateCalls = 0;

  static reset(): void {
    MockWorker.constructorCalls = 0;
    MockWorker.instances = [];
  }

  constructor(_filename: string) {
    super();
    MockWorker.constructorCalls += 1;
    if (activePlan.constructorError !== undefined) {
      throw activePlan.constructorError;
    }
    MockWorker.instances.push(this);
  }

  postMessage(
    message: InitRequest,
    transferList?: readonly unknown[],
  ): void {
    this.messages.push(message);
    this.transferLists.push(transferList);
    activePlan.post?.(this, message, transferList);
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return activePlan.terminate?.(this) ?? Promise.resolve(0);
  }
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
} {
  let resolvePromise!: (value: T) => void;
  let rejectPromise!: (error: unknown) => void;
  const promise = new Promise<T>((resolveValue, rejectValue) => {
    resolvePromise = resolveValue;
    rejectPromise = rejectValue;
  });
  return { promise, resolve: resolvePromise, reject: rejectPromise };
}

async function within<T>(
  promise: Promise<T>,
  label: string,
  timeoutMs = 5_000,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(
      () => reject(new Error(`Timed out waiting for ${label}`)),
      timeoutMs,
    );
  });

  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

async function rejectionOf<T>(promise: Promise<T>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("Expected Promise to reject");
}

async function withMockWorker<T>(
  plan: MockWorkerPlan,
  body: () => Promise<T>,
): Promise<T> {
  const OriginalWorker = workerThreads.Worker;
  MockWorker.reset();
  activePlan = plan;
  workerThreads.Worker = MockWorker as unknown as typeof workerThreads.Worker;

  try {
    return await body();
  } finally {
    workerThreads.Worker = OriginalWorker;
    activePlan = {};
  }
}

async function withCapturedHandles<T>(
  body: (handles: CapturedHandleState[]) => Promise<T>,
): Promise<T> {
  const prototype = EngineHandle.prototype as unknown as {
    initialize?: (
      this: EngineHandle,
      ...args: unknown[]
    ) => Promise<unknown>;
  };
  const originalInitialize = prototype.initialize;
  if (typeof originalInitialize !== "function") {
    throw new Error("EngineHandle.initialize private test seam is unavailable");
  }

  const handles: CapturedHandleState[] = [];
  prototype.initialize = function (...args: unknown[]): Promise<unknown> {
    handles.push(this as unknown as CapturedHandleState);
    return Reflect.apply(originalInitialize, this, args) as Promise<unknown>;
  };

  try {
    return await body(handles);
  } finally {
    prototype.initialize = originalInitialize;
  }
}

function emitProtocolError(
  worker: MockWorker,
  request: InitRequest,
  suffix = "",
): void {
  queueMicrotask(() => {
    worker.emit("message", {
      id: request.id,
      error: {
        name: "ForcedInitError",
        message: `forced init protocol error${suffix}`,
        code: "FORCED_INIT",
      },
    });
  });
}

function assertWorkerListenersRemoved(worker: MockWorker): void {
  assert.strictEqual(worker.listenerCount("message"), 0);
  assert.strictEqual(worker.listenerCount("error"), 0);
  assert.strictEqual(worker.listenerCount("exit"), 0);
}

function assertFailedHandleClean(handle: CapturedHandleState): void {
  assert.strictEqual(handle.pending.size, 0);
  assert.strictEqual(handle.worker, null);
  assert.strictEqual(handle.closed, true);
}

function registeredInitEntry(
  handles: CapturedHandleState[],
  request: InitRequest,
): CapturedPendingEntry {
  assert.strictEqual(handles.length, 1);
  assert.strictEqual(
    handles[0].pending.size,
    1,
    "the init request must be registered before postMessage",
  );
  const entry = handles[0].pending.get(request.id);
  assert.ok(entry, "the posted init request must already have a pending entry");
  return entry;
}

function runNodeScript(script: string): Promise<{ stdout: string; stderr: string }> {
  const packageRoot = resolve(__dirname, "../../../..");
  return new Promise((resolveRun, rejectRun) => {
    execFile(
      process.execPath,
      ["-e", script],
      {
        cwd: packageRoot,
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

function realCreateFailureScript(kind: "source" | "snapshot"): string {
  const options = kind === "source"
    ? "{ source: \"(defrule broken\" }"
    : "{ snapshot: { data: Buffer.from(\"not-a-snapshot\") } }";

  return `
const { EngineHandle } = require("./dist");
const messagePortCount = () => process.getActiveResourcesInfo()
  .filter((resource) => resource === "MessagePort").length;

(async () => {
  const before = messagePortCount();
  let failure;
  try {
    await EngineHandle.create(${options});
  } catch (error) {
    failure = error;
  }

  await new Promise((resolveValue) => setImmediate(resolveValue));
  const after = messagePortCount();
  console.log(JSON.stringify({
    before,
    after,
    name: failure?.name,
    code: failure?.code,
    message: failure?.message,
  }));

  if (!failure) throw new Error("EngineHandle.create unexpectedly succeeded");
  if (after !== before) {
    throw new Error(\`MessagePort count changed from \${before} to \${after}\`);
  }
})();
`;
}

test("D-010 real initialization failures release MessagePorts and exit naturally", async (t) => {
  const cases = [
    {
      kind: "source" as const,
      name: "FerricParseError",
      code: "FERRIC_PARSE_ERROR",
    },
    {
      kind: "snapshot" as const,
      name: "FerricSerializationError",
      code: "FERRIC_SERIALIZATION_ERROR",
    },
  ];

  for (const item of cases) {
    await t.test(`D-010 ${item.kind} init failure exits without process.exit or unref`, async () => {
      const script = realCreateFailureScript(item.kind);
      assert.doesNotMatch(script, /process\.exit\s*\(/);
      assert.doesNotMatch(script, /\.unref\s*\(/);

      const { stdout } = await runNodeScript(script);
      const report = JSON.parse(stdout.trim()) as {
        before: number;
        after: number;
        name: string;
        code: string;
        message: string;
      };
      assert.strictEqual(report.after, report.before);
      assert.strictEqual(report.name, item.name);
      assert.strictEqual(report.code, item.code);
      assert.ok(report.message.length > 0);
    });
  }
});

test("D-010 source and snapshot protocol failures clean their owned worker", async (t) => {
  const cases = [
    {
      name: "source",
      options: { source: "(defrule synthetic =>)" },
      transferCount: 0,
    },
    {
      name: "snapshot",
      options: { snapshot: { data: Buffer.from([1, 2, 3]) } },
      transferCount: 1,
    },
  ];

  for (const item of cases) {
    await t.test(`D-010 ${item.name} protocol rejection cleans initialization`, async () => {
      await withMockWorker({}, async () => {
        await withCapturedHandles(async (handles) => {
          let initRejectCalls = 0;
          activePlan.post = (worker, request) => {
            const entry = registeredInitEntry(handles, request);
            const originalReject = entry.reject;
            entry.reject = (error: unknown): void => {
              initRejectCalls += 1;
              originalReject(error);
            };
            emitProtocolError(worker, request);
          };

          const error = await within(
            rejectionOf(EngineHandle.create(item.options)),
            `${item.name} protocol rejection`,
          ) as Error & { code?: string };

          assert.strictEqual(error.name, "ForcedInitError");
          assert.strictEqual(error.message, "forced init protocol error");
          assert.strictEqual(error.code, "FORCED_INIT");
          assert.strictEqual(initRejectCalls, 1);
          assert.strictEqual(handles.length, 1);
          assertFailedHandleClean(handles[0]);

          const worker = MockWorker.instances[0];
          assert.strictEqual(worker.terminateCalls, 1);
          assert.strictEqual(worker.messages.length, 1);
          assert.strictEqual(worker.messages[0].method, "__init");
          assert.strictEqual(
            worker.transferLists[0]?.length ?? 0,
            item.transferCount,
          );
          assertWorkerListenersRemoved(worker);
        });
      });
    });
  }
});

test("D-010 synchronous init sends roll back before cleanup", async (t) => {
  const cases = [
    {
      name: "non-transfer",
      options: { source: "(defrule synthetic =>)" },
      transferCount: 0,
    },
    {
      name: "transfer",
      options: { snapshot: { data: Buffer.from([4, 5, 6]) } },
      transferCount: 1,
    },
  ];

  for (const item of cases) {
    await t.test(`D-010 ${item.name} init send throw preserves identity and bookkeeping`, async () => {
      const primary = new Error(`${item.name} postMessage failed`);
      const unhandled: unknown[] = [];
      const onUnhandled = (reason: unknown): void => {
        unhandled.push(reason);
      };

      await withMockWorker(
        {},
        async () => {
          await withCapturedHandles(async (handles) => {
            activePlan.post = (_worker, request) => {
              registeredInitEntry(handles, request);
              throw primary;
            };

            process.on("unhandledRejection", onUnhandled);
            try {
              const error = await within(
                rejectionOf(EngineHandle.create(item.options)),
                `${item.name} synchronous init send failure`,
              );
              assert.strictEqual(error, primary);
              await new Promise<void>((resolveValue) => setImmediate(resolveValue));
            } finally {
              process.removeListener("unhandledRejection", onUnhandled);
            }

            assert.deepStrictEqual(unhandled, []);
            assert.strictEqual(handles.length, 1);
            assertFailedHandleClean(handles[0]);

            const worker = MockWorker.instances[0];
            assert.strictEqual(worker.terminateCalls, 1);
            assert.strictEqual(
              worker.transferLists[0]?.length ?? 0,
              item.transferCount,
            );
            assertWorkerListenersRemoved(worker);
          });
        },
      );
    });
  }
});

test("D-010 create rejection waits for failed-init termination", async () => {
  const terminateStarted = deferred<void>();
  const allowTerminate = deferred<number>();

  await withMockWorker(
    {
      post: (worker, request) => emitProtocolError(worker, request),
      terminate: async () => {
        terminateStarted.resolve();
        return allowTerminate.promise;
      },
    },
    async () => {
      await withCapturedHandles(async (handles) => {
        const creation = EngineHandle.create({ source: "(defrule synthetic =>)" });
        let settlements = 0;
        const observedSettlement = creation.then(
          () => { settlements += 1; },
          () => { settlements += 1; },
        );

        await within(terminateStarted.promise, "failed-init termination start");
        await Promise.resolve();
        assert.strictEqual(settlements, 0);

        allowTerminate.resolve(0);
        const error = await within(
          rejectionOf(creation),
          "create rejection after failed-init termination",
        ) as Error;
        await observedSettlement;

        assert.strictEqual(error.name, "ForcedInitError");
        assert.strictEqual(settlements, 1);
        assert.strictEqual(MockWorker.instances[0].terminateCalls, 1);
        assertWorkerListenersRemoved(MockWorker.instances[0]);
        assertFailedHandleClean(handles[0]);
      });
    },
  );
});

test("D-010 termination failure is attached without replacing init failure", async (t) => {
  for (const hasExistingCause of [false, true]) {
    await t.test(
      `D-010 termination cause attachment with${hasExistingCause ? "" : "out"} an existing cause`,
      async () => {
        const existingCause = new Error("existing initialization context");
        const primary = hasExistingCause
          ? new Error("worker init error", { cause: existingCause })
          : new Error("worker init error");
        const terminationError = new Error("terminate failed");

        await withMockWorker(
          {
            post: (worker) => {
              queueMicrotask(() => worker.emit("error", primary));
            },
            terminate: () => Promise.reject(terminationError),
          },
          async () => {
            await withCapturedHandles(async (handles) => {
              const error = await within(
                rejectionOf(EngineHandle.create()),
                "termination rejection cleanup",
              );
              assert.strictEqual(error, primary);

              const attached = (primary as Error & { cause?: unknown }).cause;
              if (hasExistingCause) {
                assert.ok(attached instanceof AggregateError);
                assert.deepStrictEqual(
                  [...attached.errors],
                  [existingCause, terminationError],
                );
              } else {
                assert.strictEqual(attached, terminationError);
              }

              assert.strictEqual(MockWorker.instances[0].terminateCalls, 1);
              assertWorkerListenersRemoved(MockWorker.instances[0]);
              assertFailedHandleClean(handles[0]);
            });
          },
        );
      },
    );
  }
});

test("D-010 immutable, locked-cause, and non-Error failures retain identity when cleanup cause attachment is impossible", async (t) => {
  const lockedCause = new Error("locked initialization context");
  const lockedCausePrimary = new Error("locked-cause init failure");
  Object.defineProperty(lockedCausePrimary, "cause", {
    value: lockedCause,
    configurable: false,
    writable: false,
  });
  assert.strictEqual(Object.isExtensible(lockedCausePrimary), true);

  const cases: Array<{
    name: string;
    primary: unknown;
    expectedCause?: unknown;
  }> = [
    {
      name: "frozen Error",
      primary: Object.freeze(new Error("frozen init failure")),
    },
    {
      name: "extensible Error with locked cause",
      primary: lockedCausePrimary,
      expectedCause: lockedCause,
    },
    {
      name: "non-Error",
      primary: "primitive init failure",
    },
  ];

  for (const item of cases) {
    await t.test(`D-010 ${item.name} remains primary when cleanup metadata cannot be attached`, async () => {
      const terminationError = new Error("terminate failed");
      await withMockWorker(
        {
          post: () => {
            throw item.primary;
          },
          terminate: () => Promise.reject(terminationError),
        },
        async () => {
          await withCapturedHandles(async (handles) => {
            const error = await within(
              rejectionOf(EngineHandle.create()),
              `${item.name} cleanup failure`,
            );
            assert.strictEqual(error, item.primary);
            if (item.primary instanceof Error) {
              // Frozen errors and locked cause properties cannot accept new
              // cleanup metadata, so exact identity deliberately wins.
              assert.strictEqual(
                (item.primary as Error & { cause?: unknown }).cause,
                item.expectedCause,
              );
            }

            assert.strictEqual(MockWorker.instances[0].terminateCalls, 1);
            assertWorkerListenersRemoved(MockWorker.instances[0]);
            assertFailedHandleClean(handles[0]);
          });
        },
      );
    });
  }
});

test("D-010 an already-terminal handle still unwinds factory ownership", async () => {
  const prototype = EngineHandle.prototype as unknown as {
    attachWorkerListeners?: (worker: unknown) => void;
  };
  const originalAttach = prototype.attachWorkerListeners;
  if (typeof originalAttach !== "function") {
    throw new Error("EngineHandle.attachWorkerListeners private test seam is unavailable");
  }

  let postCalls = 0;
  await withMockWorker(
    {
      post: () => {
        postCalls += 1;
      },
    },
    async () => {
      prototype.attachWorkerListeners = function (worker: unknown): void {
        Reflect.apply(originalAttach, this, [worker]);
        (this as CapturedHandleState).worker = null;
      };

      try {
        await withCapturedHandles(async (handles) => {
          const error = await within(
            rejectionOf(EngineHandle.create()),
            "already-terminal initialization cleanup",
          ) as Error;
          assert.match(error.message, /exited before initialization/);
          assert.strictEqual(postCalls, 0);
          assert.strictEqual(MockWorker.instances[0].terminateCalls, 1);
          assertWorkerListenersRemoved(MockWorker.instances[0]);
          assertFailedHandleClean(handles[0]);
        });
      } finally {
        prototype.attachWorkerListeners = originalAttach;
      }
    },
  );
});

test("D-010 failed-create cleanup rejects residual pending work exactly once", async () => {
  const prototype = EngineHandle.prototype as unknown as {
    initialize?: (
      this: EngineHandle,
      ...args: unknown[]
    ) => Promise<unknown>;
  };
  const originalInitialize = prototype.initialize;
  if (typeof originalInitialize !== "function") {
    throw new Error("EngineHandle.initialize private test seam is unavailable");
  }

  let captured: CapturedHandleState | undefined;
  let residualRejectCalls = 0;
  let residualRejection: unknown;

  await withMockWorker(
    {
      post: (worker, request) => emitProtocolError(worker, request),
    },
    async () => {
      prototype.initialize = function (...args: unknown[]): Promise<unknown> {
        captured = this as unknown as CapturedHandleState;
        captured.pending.set(999, {
          resolve: () => {},
          reject: (error: unknown) => {
            residualRejectCalls += 1;
            residualRejection = error;
          },
        });
        return Reflect.apply(originalInitialize, this, args) as Promise<unknown>;
      };

      try {
        const error = await within(
          rejectionOf(EngineHandle.create()),
          "residual pending cleanup",
        );
        assert.strictEqual(residualRejectCalls, 1);
        assert.strictEqual(residualRejection, error);
        assert.ok(captured);
        assertFailedHandleClean(captured);
        assert.strictEqual(MockWorker.instances[0].terminateCalls, 1);
        assertWorkerListenersRemoved(MockWorker.instances[0]);
      } finally {
        prototype.initialize = originalInitialize;
      }
    },
  );
});

test("D-010 listener-detach failure cannot replace the primary init error", async () => {
  const prototype = EngineHandle.prototype as unknown as {
    detachWorkerListeners?: (worker: unknown) => void;
  };
  const originalDetach = prototype.detachWorkerListeners;
  if (typeof originalDetach !== "function") {
    throw new Error("EngineHandle.detachWorkerListeners private test seam is unavailable");
  }

  const primary = new Error("worker init failed");
  const detachError = new Error("listener detach failed");
  await withMockWorker(
    {
      post: (worker) => {
        queueMicrotask(() => worker.emit("error", primary));
      },
    },
    async () => {
      prototype.detachWorkerListeners = () => {
        throw detachError;
      };

      try {
        await withCapturedHandles(async (handles) => {
          const error = await within(
            rejectionOf(EngineHandle.create()),
            "listener-detach failure cleanup",
          );
          assert.strictEqual(error, primary);
          assert.strictEqual(MockWorker.instances[0].terminateCalls, 1);
          assertFailedHandleClean(handles[0]);
          MockWorker.instances[0].removeAllListeners();
        });
      } finally {
        prototype.detachWorkerListeners = originalDetach;
      }
    },
  );
});

test("D-010 duplicate worker terminal events and late replies clean up once", async (t) => {
  const cases = [
    {
      name: "error-first",
      emit: (worker: MockWorker, request: InitRequest, primary: Error) => {
        worker.emit("error", primary);
        worker.emit("exit", 7);
        worker.emit("message", { id: request.id, result: "late success" });
        worker.emit("message", {
          id: request.id,
          error: { name: "LateError", message: "late", code: "LATE" },
        });
      },
      verify: (error: unknown, primary: Error) => assert.strictEqual(error, primary),
    },
    {
      name: "exit-first",
      emit: (worker: MockWorker, request: InitRequest, primary: Error) => {
        worker.emit("exit", 7);
        worker.emit("error", primary);
        worker.emit("message", { id: request.id, result: "late success" });
      },
      verify: (error: unknown) => {
        assert.match((error as Error).message, /unexpectedly with code 7/);
      },
    },
  ];

  for (const item of cases) {
    await t.test(`D-010 ${item.name} terminal race preserves first settlement`, async () => {
      const primary = new Error("worker emitted init error");
      await withMockWorker({}, async () => {
        await withCapturedHandles(async (handles) => {
          let initRejectCalls = 0;
          activePlan.post = (worker, request) => {
            const entry = registeredInitEntry(handles, request);
            const originalReject = entry.reject;
            entry.reject = (error: unknown): void => {
              initRejectCalls += 1;
              originalReject(error);
            };
            queueMicrotask(() => item.emit(worker, request, primary));
          };

          const creation = EngineHandle.create();
          let settlements = 0;
          const observedSettlement = creation.then(
            () => { settlements += 1; },
            () => { settlements += 1; },
          );
          const error = await within(
            rejectionOf(creation),
            `${item.name} terminal race`,
          );
          await observedSettlement;

          item.verify(error, primary);
          assert.strictEqual(initRejectCalls, 1);
          assert.strictEqual(settlements, 1);
          assert.strictEqual(MockWorker.instances[0].terminateCalls, 1);
          assertWorkerListenersRemoved(MockWorker.instances[0]);
          assertFailedHandleClean(handles[0]);
        });
      });
    });
  }
});

test("D-010 constructor and pre-spawn validation failures preserve ownership boundary", async () => {
  const constructorError = new Error("Worker constructor failed");
  await withMockWorker(
    { constructorError },
    async () => {
      const snapshot = { data: Buffer.from([1]) };
      const validationError = await rejectionOf(
        EngineHandle.create({ source: "(defrule x =>)", snapshot }),
      );
      assert.ok(validationError instanceof TypeError);
      assert.strictEqual(MockWorker.constructorCalls, 0);

      const error = await rejectionOf(EngineHandle.create());
      assert.strictEqual(error, constructorError);
      assert.strictEqual(MockWorker.constructorCalls, 1);
      assert.deepStrictEqual(MockWorker.instances, []);
    },
  );
});

test("D-010 repeated failed creates do not accumulate workers or listeners", async () => {
  const unhandled: unknown[] = [];
  const onUnhandled = (reason: unknown): void => {
    unhandled.push(reason);
  };

  await withMockWorker(
    {
      post: (worker, request) => {
        emitProtocolError(worker, request, ` ${worker.messages.length}`);
      },
    },
    async () => {
      await withCapturedHandles(async (handles) => {
        process.on("unhandledRejection", onUnhandled);
        try {
          for (let index = 0; index < 16; index += 1) {
            const error = await within(
              rejectionOf(EngineHandle.create()),
              `repeated failed create ${index}`,
            ) as Error;
            assert.strictEqual(error.name, "ForcedInitError");
          }
          await new Promise<void>((resolveValue) => setImmediate(resolveValue));
        } finally {
          process.removeListener("unhandledRejection", onUnhandled);
        }

        assert.deepStrictEqual(unhandled, []);
        assert.strictEqual(MockWorker.instances.length, 16);
        assert.strictEqual(handles.length, 16);
        for (const [index, worker] of MockWorker.instances.entries()) {
          assert.strictEqual(worker.terminateCalls, 1, `worker ${index}`);
          assertWorkerListenersRemoved(worker);
          assertFailedHandleClean(handles[index]);
        }
      });
    },
  );
});

test("D-010 successful initialization transfers worker ownership to the handle", async () => {
  await withMockWorker(
    {
      post: (worker, request) => {
        queueMicrotask(() => worker.emit("message", {
          id: request.id,
          result: undefined,
        }));
      },
    },
    async () => {
      const handle = await within(
        EngineHandle.create({ source: "(defrule synthetic =>)" }),
        "successful initialization",
      );
      const worker = MockWorker.instances[0];

      assert.strictEqual(worker.terminateCalls, 0);
      assert.strictEqual(worker.listenerCount("message"), 1);
      assert.strictEqual(worker.listenerCount("error"), 1);
      assert.strictEqual(worker.listenerCount("exit"), 1);

      await handle.close();
      assert.strictEqual(worker.terminateCalls, 1);
    },
  );
});

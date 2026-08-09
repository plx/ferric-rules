/**
 * Worker entrypoint guard tests.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { dirname } from "node:path";
import { resolve } from "node:path";
import { createRequire } from "node:module";

const requireFromHere = createRequire(__filename);
const Module = requireFromHere("node:module") as any;

function withPatchedLoad<T>(
  patch: (
    request: string,
    parent: unknown,
    isMain: boolean,
    originalLoad: (request: string, parent: unknown, isMain: boolean) => unknown,
  ) => unknown,
  fn: () => T,
): T {
  const original = Module._load;
  Module._load = function patched(
    request: string,
    parent: unknown,
    isMain: boolean,
  ) {
    return patch(request, parent, isMain, original);
  };
  try {
    return fn();
  } finally {
    Module._load = original;
  }
}

// ---------------------------------------------------------------------------
// D-001 manual worker guard: worker.ts cannot be required on the main thread
// ---------------------------------------------------------------------------
test("D-001 worker entrypoint rejects main-thread require", () => {
  const path = resolve(__dirname, "../../../dist/worker.js");
  delete requireFromHere.cache[requireFromHere.resolve(path)];

  assert.throws(
    () => requireFromHere(path),
    /worker\.ts must be run as a Worker thread/,
  );
});

// ---------------------------------------------------------------------------
// E-007 manual pool-worker guard: pool-worker.ts cannot run on main thread
// ---------------------------------------------------------------------------
test("E-007 pool-worker entrypoint rejects main-thread require", () => {
  const path = resolve(__dirname, "../../../dist/pool-worker.js");
  delete requireFromHere.cache[requireFromHere.resolve(path)];

  assert.throws(
    () => requireFromHere(path),
    /pool-worker\.ts must be run as a Worker thread/,
  );
});

// ---------------------------------------------------------------------------
// D-001 manual worker init: re-init closes the previous engine defensively
// ---------------------------------------------------------------------------
test("D-001 worker re-init ignores close failures from previous engine", () => {
  const workerPath = resolve(__dirname, "../../../dist/worker.js");
  const bundledPath = resolve(dirname(workerPath), "..", "native", "index.js");

  class FakeParentPort extends EventEmitter {
    readonly responses: unknown[] = [];
    postMessage(message: unknown): void {
      this.responses.push(message);
    }
  }

  class FakeEngine {
    static fromSnapshot(): FakeEngine {
      return new FakeEngine();
    }

    close(): void {
      throw new Error("close failed");
    }
  }

  const parentPort = new FakeParentPort();
  delete requireFromHere.cache[requireFromHere.resolve(workerPath)];

  withPatchedLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === "node:worker_threads") return { parentPort };
      if (request === bundledPath) {
        return {
          Engine: FakeEngine,
          FerricSymbol: class FerricSymbol {
            constructor(readonly value: string) {}
          },
        };
      }
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        requireFromHere(workerPath);
        parentPort.emit("message", { id: 1, method: "__init", args: [{}] });
        parentPort.emit("message", { id: 2, method: "__init", args: [{}] });

        // The second init calls close() on the previous engine, catches the
        // synthetic failure, and still acknowledges initialization.
        assert.deepStrictEqual(parentPort.responses, [
          { id: 1, result: undefined },
          { id: 2, result: undefined },
        ]);
      } finally {
        delete requireFromHere.cache[requireFromHere.resolve(workerPath)];
      }
    },
  );
});

// ---------------------------------------------------------------------------
// D-001 table-driven mocked worker protocol: table-driven branches stay stable
// ---------------------------------------------------------------------------
test("D-001 table-driven mocked worker protocol covers init/run/close branches", () => {
  const workerPath = resolve(__dirname, "../../../dist/worker.js");
  const bundledPath = resolve(dirname(workerPath), "..", "native", "index.js");

  class FakeParentPort extends EventEmitter {
    readonly responses: any[] = [];
    postMessage(message: unknown): void {
      this.responses.push(message);
    }
  }

  class FakeEngine {
    static readonly instances: FakeEngine[] = [];
    static readonly snapshots: Array<{ data: number[]; format: number | undefined }> = [];

    static fromSnapshot(data: Buffer, format?: number): FakeEngine {
      FakeEngine.snapshots.push({ data: [...data], format });
      return new FakeEngine({ restored: true });
    }

    closeThrows = false;
    halted = false;
    haltCalls = 0;
    diagnostics: string[] = [];
    loadCalls: string[] = [];
    resetCalls = 0;
    runResults: Array<{ rulesFired: number; haltReason: number }> = [];
    continueRunResults: Array<{ rulesFired: number; haltReason: number }> = [];
    runCalls: string[] = [];
    afterRun: (() => void) | undefined;

    constructor(readonly options: Record<string, unknown> = {}) {
      FakeEngine.instances.push(this);
    }

    close(): void {
      if (this.closeThrows) throw new Error("close failed");
    }

    load(source: string): void {
      this.loadCalls.push(source);
    }

    reset(): void {
      this.resetCalls += 1;
    }

    run(limit?: number): { rulesFired: number; haltReason: number } {
      this.runCalls.push(`run:${String(limit)}`);
      this.halted = false;
      this.diagnostics = [];
      const result = this.runResults.shift() ?? {
        rulesFired: typeof limit === "number" ? limit : 0,
        haltReason: 1,
      };
      const afterRun = this.afterRun;
      this.afterRun = undefined;
      afterRun?.();
      return result;
    }

    halt(): void {
      this.haltCalls += 1;
      this.halted = true;
    }

    get isHalted(): boolean {
      return this.halted;
    }

    facts(): unknown[] {
      throw "string failure";
    }

    serialize(): Buffer {
      return Buffer.from([1, 2, 3]);
    }
  }

  const continuedEngines: FakeEngine[] = [];
  function nativeContinueRun(
    engine: FakeEngine,
    limit: number,
  ): { rulesFired: number; haltReason: number } {
    continuedEngines.push(engine);
    engine.runCalls.push(`continue:${String(limit)}`);
    const result = engine.continueRunResults.shift() ?? {
      rulesFired: limit,
      haltReason: 1,
    };
    const afterRun = engine.afterRun;
    engine.afterRun = undefined;
    afterRun?.();
    return result;
  }

  const parentPort = new FakeParentPort();
  delete requireFromHere.cache[requireFromHere.resolve(workerPath)];

  withPatchedLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === "node:worker_threads") return { parentPort };
      if (request === bundledPath) {
        return {
          Engine: FakeEngine,
          __continueRun: nativeContinueRun,
          FerricSymbol: class FerricSymbol {
            constructor(readonly value: string) {}
          },
        };
      }
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        requireFromHere(workerPath);

        // Generated frames cover the less common protocol branches without
        // depending on native timing: pre-init rejection, source init, snapshot
        // init, all run modes, serialize transfer, non-Error catches, and close.
        parentPort.emit("message", { id: 1, method: "facts", args: [] });
        assert.match(parentPort.responses.pop().error.message, /not initialized/);

        parentPort.emit("message", {
          id: 2,
          method: "__init",
          args: [{ options: { maxCallDepth: 3 }, source: "(defrule ok =>)" }],
        });
        const sourceEngine = FakeEngine.instances.at(-1)!;
        assert.deepStrictEqual(sourceEngine.loadCalls, ["(defrule ok =>)"]);
        assert.strictEqual(sourceEngine.resetCalls, 1);

        parentPort.emit("message", { id: 21, method: "load", args: ["(defrule later =>)"] });
        assert.deepStrictEqual(parentPort.responses.pop(), { id: 21, result: null });
        assert.deepStrictEqual(sourceEngine.loadCalls, [
          "(defrule ok =>)",
          "(defrule later =>)",
        ]);

        parentPort.emit("message", {
          id: 3,
          method: "__init",
          args: [{ snapshot: { data: Uint8Array.from([9, 8]).buffer, format: 1 } }],
        });
        assert.deepStrictEqual(FakeEngine.snapshots, [{ data: [9, 8], format: 1 }]);
        const engine = FakeEngine.instances.at(-1)!;
        assert.strictEqual(Reflect.has(engine, "__continueRun"), false);
        assert.strictEqual(
          Object.hasOwn(FakeEngine.prototype, "__continueRun"),
          false,
        );

        engine.halted = true;
        engine.diagnostics = ["stale diagnostic"];
        parentPort.emit("message", { id: 4, method: "__run_batched", args: [0, null] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 4,
          result: { rulesFired: 0, haltReason: 1 },
        });
        assert.strictEqual(engine.halted, false);
        assert.deepStrictEqual(engine.diagnostics, []);
        assert.deepStrictEqual(engine.runCalls, ["run:0"]);

        parentPort.emit("message", { id: 5, method: "__run_batched", args: [5, null] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 5,
          result: { rulesFired: 5, haltReason: 1 },
        });

        engine.runResults.push({ rulesFired: 1, haltReason: 0 });
        parentPort.emit("message", { id: 6, method: "__run_batched", args: [null, null] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 6,
          result: { rulesFired: 1, haltReason: 0 },
        });

        engine.runCalls = [];
        engine.runResults.push(
          { rulesFired: 100, haltReason: 1 },
          // Keeps a pre-fix repro finite if the worker incorrectly calls run()
          // for the second chunk instead of the continuation entrypoint.
          { rulesFired: 1, haltReason: 0 },
        );
        engine.continueRunResults.push({ rulesFired: 1, haltReason: 0 });
        parentPort.emit("message", { id: 7, method: "__run_batched", args: [null, null] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 7,
          result: { rulesFired: 101, haltReason: 0 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100", "continue:100"]);
        assert.deepStrictEqual(continuedEngines, [engine]);
        engine.runResults = [];
        engine.continueRunResults = [];

        const sab = new SharedArrayBuffer(Int32Array.BYTES_PER_ELEMENT);
        const abortFlag = new Int32Array(sab);
        engine.runCalls = [];
        engine.runResults.push({ rulesFired: 100, haltReason: 1 });
        engine.afterRun = () => Atomics.store(abortFlag, 0, 1);
        parentPort.emit("message", { id: 8, method: "__run_batched", args: [null, sab] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 8,
          result: { rulesFired: 100, haltReason: 2 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100"]);
        assert.strictEqual(engine.haltCalls, 0);
        assert.strictEqual(engine.halted, false);

        // A later external request must start a fresh native logical run.
        Atomics.store(abortFlag, 0, 0);
        engine.runResults.push({ rulesFired: 1, haltReason: 0 });
        parentPort.emit("message", { id: 9, method: "__run_batched", args: [null, sab] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 9,
          result: { rulesFired: 1, haltReason: 0 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100", "run:100"]);

        // A completed explicit limit takes precedence over a host cancellation
        // that becomes visible as its final chunk returns.
        engine.runCalls = [];
        engine.runResults.push({ rulesFired: 100, haltReason: 1 });
        engine.afterRun = () => Atomics.store(abortFlag, 0, 1);
        parentPort.emit("message", { id: 10, method: "__run_batched", args: [100, sab] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 10,
          result: { rulesFired: 100, haltReason: 1 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100"]);
        assert.strictEqual(engine.haltCalls, 0);

        // A native terminal result wins over a host abort published as that
        // same chunk returns; neither continuation nor native halt is needed.
        Atomics.store(abortFlag, 0, 0);
        engine.runCalls = [];
        engine.runResults.push({ rulesFired: 1, haltReason: 0 });
        engine.afterRun = () => Atomics.store(abortFlag, 0, 1);
        parentPort.emit("message", {
          id: 11,
          method: "__run_batched",
          args: [null, sab],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 11,
          result: { rulesFired: 1, haltReason: 0 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100"]);
        assert.strictEqual(engine.haltCalls, 0);

        // A pre-set cancellation returns a partial result without invoking
        // either native run entrypoint or setting the rule-level halt latch.
        engine.runCalls = [];
        parentPort.emit("message", { id: 12, method: "__run_batched", args: [10, sab] });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 12,
          result: { rulesFired: 0, haltReason: 2 },
        });
        assert.deepStrictEqual(engine.runCalls, []);
        assert.strictEqual(engine.haltCalls, 0);
        assert.strictEqual(engine.halted, false);

        parentPort.emit("message", { id: 13, method: "serialize", args: [] });
        assert.ok(parentPort.responses.pop().result instanceof ArrayBuffer);

        parentPort.emit("message", { id: 14, method: "facts", args: [] });
        assert.deepStrictEqual(parentPort.responses.pop().error, {
          name: "Error",
          message: "string failure",
          code: "FERRIC_ERROR",
        });

        engine.closeThrows = true;
        parentPort.emit("message", { id: 15, method: "__close", args: [] });
        assert.deepStrictEqual(parentPort.responses.pop(), { id: 15, result: undefined });
      } finally {
        delete requireFromHere.cache[requireFromHere.resolve(workerPath)];
      }
    },
  );
});

// ---------------------------------------------------------------------------
// E-005/E-011 mocked pool worker: logical-run routing and cancellation ordering
// ---------------------------------------------------------------------------
test("E-005 E-011 mocked pool worker routes continuation and host abort", () => {
  const workerPath = resolve(__dirname, "../../../dist/pool-worker.js");
  const bundledPath = resolve(dirname(workerPath), "..", "native", "index.js");

  class FakeParentPort extends EventEmitter {
    readonly responses: any[] = [];

    postMessage(message: unknown): void {
      this.responses.push(message);
    }
  }

  class FakeEngine {
    static readonly instances: FakeEngine[] = [];

    halted = false;
    haltCalls = 0;
    loadCalls: string[] = [];
    resetCalls = 0;
    runResults: Array<{ rulesFired: number; haltReason: number }> = [];
    continueRunResults: Array<{ rulesFired: number; haltReason: number }> = [];
    runCalls: string[] = [];
    afterRun: (() => void) | undefined;

    constructor(readonly options: Record<string, unknown> = {}) {
      FakeEngine.instances.push(this);
    }

    load(source: string): void {
      this.loadCalls.push(source);
    }

    reset(): void {
      this.resetCalls += 1;
    }

    run(limit?: number): { rulesFired: number; haltReason: number } {
      this.runCalls.push(`run:${String(limit)}`);
      this.halted = false;
      const result = this.runResults.shift() ?? {
        rulesFired: typeof limit === "number" ? limit : 0,
        haltReason: 1,
      };
      const afterRun = this.afterRun;
      this.afterRun = undefined;
      afterRun?.();
      return result;
    }

    halt(): void {
      this.haltCalls += 1;
      this.halted = true;
    }

    get isHalted(): boolean {
      return this.halted;
    }
  }

  const continuedEngines: FakeEngine[] = [];
  function nativeContinueRun(
    engine: FakeEngine,
    limit: number,
  ): { rulesFired: number; haltReason: number } {
    continuedEngines.push(engine);
    engine.runCalls.push(`continue:${String(limit)}`);
    const result = engine.continueRunResults.shift() ?? {
      rulesFired: limit,
      haltReason: 1,
    };
    const afterRun = engine.afterRun;
    engine.afterRun = undefined;
    afterRun?.();
    return result;
  }

  const parentPort = new FakeParentPort();
  delete requireFromHere.cache[requireFromHere.resolve(workerPath)];

  withPatchedLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === "node:worker_threads") return { parentPort };
      if (request === bundledPath) {
        return {
          Engine: FakeEngine,
          __continueRun: nativeContinueRun,
          FerricSymbol: class FerricSymbol {
            constructor(readonly value: string) {}
          },
        };
      }
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        requireFromHere(workerPath);
        parentPort.emit("message", {
          id: 1,
          method: "__init",
          args: [{ specs: [{ name: "rules", source: "(defrule ok =>)" }] }],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 1,
          result: undefined,
        });

        // Lazily construct the registered engine so the fake can script each
        // native chunk result independently.
        parentPort.emit("message", {
          id: 2,
          method: "getIsHalted",
          args: ["rules"],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 2,
          result: false,
        });
        const engine = FakeEngine.instances.at(-1)!;
        assert.strictEqual(Reflect.has(engine, "__continueRun"), false);
        assert.strictEqual(
          Object.hasOwn(FakeEngine.prototype, "__continueRun"),
          false,
        );
        assert.deepStrictEqual(engine.loadCalls, ["(defrule ok =>)"]);
        assert.strictEqual(engine.resetCalls, 1);

        engine.runResults.push(
          { rulesFired: 100, haltReason: 1 },
          // Keeps the repro bounded if a regression calls run() twice.
          { rulesFired: 1, haltReason: 0 },
        );
        engine.continueRunResults.push({ rulesFired: 1, haltReason: 0 });
        parentPort.emit("message", {
          id: 3,
          method: "__batched_run",
          args: ["rules", null, null],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 3,
          result: { rulesFired: 101, haltReason: 0 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100", "continue:100"]);
        assert.deepStrictEqual(continuedEngines, [engine]);
        engine.runResults = [];
        engine.continueRunResults = [];

        const sab = new SharedArrayBuffer(Int32Array.BYTES_PER_ELEMENT);
        const abortFlag = new Int32Array(sab);
        engine.runCalls = [];
        engine.runResults.push({ rulesFired: 100, haltReason: 1 });
        engine.afterRun = () => Atomics.store(abortFlag, 0, 1);
        parentPort.emit("message", {
          id: 4,
          method: "__batched_run",
          args: ["rules", null, sab],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 4,
          result: { rulesFired: 100, haltReason: 2 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100"]);
        assert.strictEqual(engine.haltCalls, 0);
        assert.strictEqual(engine.halted, false);

        // The next external request is fresh even after a partial host abort.
        Atomics.store(abortFlag, 0, 0);
        engine.runResults.push({ rulesFired: 1, haltReason: 0 });
        parentPort.emit("message", {
          id: 5,
          method: "__batched_run",
          args: ["rules", null, sab],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 5,
          result: { rulesFired: 1, haltReason: 0 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100", "run:100"]);

        // Completing the caller's exact limit wins over an abort published as
        // the final chunk returns.
        engine.runCalls = [];
        engine.runResults.push({ rulesFired: 100, haltReason: 1 });
        engine.afterRun = () => Atomics.store(abortFlag, 0, 1);
        parentPort.emit("message", {
          id: 6,
          method: "__batched_run",
          args: ["rules", 100, sab],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 6,
          result: { rulesFired: 100, haltReason: 1 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100"]);
        assert.strictEqual(engine.haltCalls, 0);

        // Native terminal completion has priority over a simultaneous abort.
        Atomics.store(abortFlag, 0, 0);
        engine.runCalls = [];
        engine.runResults.push({ rulesFired: 1, haltReason: 0 });
        engine.afterRun = () => Atomics.store(abortFlag, 0, 1);
        parentPort.emit("message", {
          id: 7,
          method: "__batched_run",
          args: ["rules", null, sab],
        });
        assert.deepStrictEqual(parentPort.responses.pop(), {
          id: 7,
          result: { rulesFired: 1, haltReason: 0 },
        });
        assert.deepStrictEqual(engine.runCalls, ["run:100"]);
        assert.strictEqual(engine.haltCalls, 0);
      } finally {
        delete requireFromHere.cache[requireFromHere.resolve(workerPath)];
      }
    },
  );
});

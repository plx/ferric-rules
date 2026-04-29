import { EventEmitter } from "node:events";
import { test } from "node:test";
import * as assert from "node:assert/strict";

import { EnginePool } from "../../../helpers/ferric";

const workerThreads = require("node:worker_threads") as typeof import("node:worker_threads");

class MockWorker extends EventEmitter {
  static instances: MockWorker[] = [];
  static failOnConstructAt: number | null = null;
  static failInitIndices = new Set<number>();

  readonly index: number;
  terminateCalls = 0;

  static reset(): void {
    MockWorker.instances = [];
    MockWorker.failOnConstructAt = null;
    MockWorker.failInitIndices = new Set<number>();
  }

  constructor(_filename: string) {
    super();

    const ordinal = MockWorker.instances.length + 1;
    if (MockWorker.failOnConstructAt === ordinal) {
      throw new Error("spawn failed");
    }

    this.index = MockWorker.instances.length;
    MockWorker.instances.push(this);
  }

  postMessage(message: { id: number; method: string }): void {
    if (message.method !== "__init") {
      return;
    }

    queueMicrotask(() => {
      if (MockWorker.failInitIndices.has(this.index)) {
        this.emit("message", {
          id: message.id,
          error: { name: "Error", message: "init failed", code: "FERRIC_ERROR" },
        });
        return;
      }

      this.emit("message", { id: message.id, result: undefined });
    });
  }

  terminate(): Promise<number> {
    this.terminateCalls += 1;
    return Promise.resolve(0);
  }
}

test("EnginePool.create terminates spawned workers when init fails", async () => {
  MockWorker.reset();
  MockWorker.failInitIndices = new Set([0]);

  const OriginalWorker = workerThreads.Worker;
  workerThreads.Worker = MockWorker as unknown as typeof workerThreads.Worker;

  try {
    await assert.rejects(
      () => EnginePool.create([{ name: "test" }], { threads: 2 }),
      /init failed/,
    );

    assert.deepStrictEqual(
      MockWorker.instances.map((worker) => worker.terminateCalls),
      [1, 1],
    );
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

test("EnginePool.create terminates earlier workers when a later spawn throws", async () => {
  MockWorker.reset();
  MockWorker.failOnConstructAt = 2;

  const OriginalWorker = workerThreads.Worker;
  workerThreads.Worker = MockWorker as unknown as typeof workerThreads.Worker;

  try {
    await assert.rejects(
      () => EnginePool.create([{ name: "test" }], { threads: 2 }),
      /spawn failed/,
    );

    assert.strictEqual(MockWorker.instances.length, 1);
    assert.strictEqual(MockWorker.instances[0]?.terminateCalls, 1);
  } finally {
    workerThreads.Worker = OriginalWorker;
  }
});

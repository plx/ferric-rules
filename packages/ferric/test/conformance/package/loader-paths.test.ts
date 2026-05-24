/**
 * Native loader path tests.
 *
 * These isolate CommonJS loaders with mocked module resolution so fallback and
 * failure branches are tested without moving real build artifacts on disk.
 */
import { test } from "node:test";
import * as assert from "node:assert/strict";
import { dirname, join, resolve } from "node:path";
import { createRequire } from "node:module";

const requireFromHere = createRequire(__filename);
const Module = requireFromHere("node:module") as any;

function clearModule(path: string): void {
  delete requireFromHere.cache[requireFromHere.resolve(path)];
}

function withModuleLoad<T>(
  load: (
    request: string,
    parent: unknown,
    isMain: boolean,
    originalLoad: (request: string, parent: unknown, isMain: boolean) => unknown,
  ) => unknown,
  fn: () => T,
): T {
  const original = Module._load;
  Module._load = function patchedLoad(
    request: string,
    parent: unknown,
    isMain: boolean,
  ) {
    return load(request, parent, isMain, original);
  };
  try {
    return fn();
  } finally {
    Module._load = original;
  }
}

function fakeNativeBinding(options?: { constructThrows?: boolean; snapshotFileThrows?: boolean }) {
  class FerricSymbol {
    constructor(readonly value: string) {}
  }

  class Engine {
    static fromSource(): Engine {
      return new Engine();
    }

    static fromSnapshot(): Engine {
      return new Engine();
    }

    static fromSnapshotFile(): Engine {
      if (options?.snapshotFileThrows) {
        throw new Error("FerricSerializationError: bad snapshot file");
      }
      return new Engine();
    }

    constructor() {
      if (options?.constructThrows) {
        throw new Error("FerricRuntimeError: constructor failed");
      }
    }

    close(): void {}

    assertFact(_relation: string, fields: unknown[]): unknown[] {
      return fields;
    }

    assertTemplate(_templateName: string, slots: Record<string, unknown>): Record<string, unknown> {
      return slots;
    }
  }

  return { Engine, FerricSymbol };
}

// ---------------------------------------------------------------------------
// G-002 manual package loader: bundled native path can satisfy native.ts
// ---------------------------------------------------------------------------
test("G-002 package native loader accepts bundled native path", () => {
  const nativePath = resolve(__dirname, "../../../dist/native.js");
  const bundledPath = resolve(dirname(nativePath), "..", "native", "index.js");

  clearModule(nativePath);
  withModuleLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === bundledPath) return fakeNativeBinding();
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        const mod = requireFromHere(nativePath);
        assert.strictEqual(typeof mod.Engine, "function");
        assert.strictEqual(typeof mod.FerricSymbol, "function");
        assert.ok(new mod.Engine());
        assert.ok(mod.Engine.fromSource("ignored"));
      } finally {
        clearModule(nativePath);
      }
    },
  );
});

// ---------------------------------------------------------------------------
// G-002 manual package loader: native.ts reports both failed paths
// ---------------------------------------------------------------------------
test("G-002 package native loader reports deterministic failure", () => {
  const nativePath = resolve(__dirname, "../../../dist/native.js");
  const bundledPath = resolve(dirname(nativePath), "..", "native", "index.js");
  const developmentPath = resolve(
    dirname(nativePath),
    "..",
    "..",
    "..",
    "crates",
    "ferric-napi",
    "index.js",
  );

  clearModule(nativePath);
  withModuleLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === bundledPath || request === developmentPath) {
        throw new Error(`missing ${request}`);
      }
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        assert.throws(
          () => requireFromHere(nativePath),
          (err: any) => {
            assert.match(err.message, /Could not load native addon/);
            assert.match(err.message, /native\/index\.js/);
            assert.match(err.message, /crates\/ferric-napi\/index\.js/);
            return true;
          },
        );
      } finally {
        clearModule(nativePath);
      }
    },
  );
});

// ---------------------------------------------------------------------------
// G-002 manual package loader: constructor/static errors are converted
// ---------------------------------------------------------------------------
test("G-002 package native wrapper converts mocked constructor and static errors", () => {
  const nativePath = resolve(__dirname, "../../../dist/native.js");
  const bundledPath = resolve(dirname(nativePath), "..", "native", "index.js");

  for (const [binding, exercise, expected] of [
    [
      fakeNativeBinding({ constructThrows: true }),
      (mod: any) => new mod.Engine(),
      /constructor failed/,
    ],
    [
      fakeNativeBinding({ snapshotFileThrows: true }),
      (mod: any) => mod.Engine.fromSnapshotFile("bad"),
      /bad snapshot file/,
    ],
  ] as const) {
    clearModule(nativePath);
    withModuleLoad(
      (request, parent, isMain, originalLoad) => {
        if (request === bundledPath) return binding;
        return originalLoad(request, parent, isMain);
      },
      () => {
        try {
          const mod = requireFromHere(nativePath);
          assert.throws(() => exercise(mod), expected);
        } finally {
          clearModule(nativePath);
        }
      },
    );
  }
});

// ---------------------------------------------------------------------------
// G-002 manual napi loader: dev failure falls back to platform package
// ---------------------------------------------------------------------------
test("G-002 napi loader falls back from local node file to platform package", () => {
  const napiPath = resolve(__dirname, "../../../../../crates/ferric-napi/index.js");
  const devPath = join(dirname(napiPath), "ferric-napi.node");
  const platformPackage = `@ferric-rules/napi-${process.platform}-${process.arch === "x64" ? "x64-gnu" : process.arch}`;
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;

  clearModule(napiPath);
  fs.existsSync = (path) => path === devPath || originalExistsSync(path);
  withModuleLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === devPath) throw new Error("bad local binary");
      if (request === platformPackage || request.startsWith("@ferric-rules/napi-")) {
        return fakeNativeBinding();
      }
      return originalLoad(request, parent, isMain);
    },
    () => {
      try {
        const mod = requireFromHere(napiPath);
        const engine = new mod.Engine();
        const sym = new mod.FerricSymbol("red");
        const fields = engine.assertFact("color", sym, [sym], null, undefined, 7) as any[];
        assert.deepStrictEqual(fields[0], { __ferric_symbol: true, value: "red" });
        assert.deepStrictEqual(fields[1][0], { __ferric_symbol: true, value: "red" });
        assert.strictEqual(fields[2], null);
        assert.strictEqual(fields[3], undefined);
        assert.strictEqual(fields[4], 7);

        // Slots can be absent in defensive/mocked calls; the loader should
        // pass non-object slot values through instead of trying to enumerate.
        assert.strictEqual(engine.assertTemplate("thing", null as any), null);
      } finally {
        fs.existsSync = originalExistsSync;
        clearModule(napiPath);
      }
    },
  );
});

// ---------------------------------------------------------------------------
// G-002 property-style napi loader failures produce deterministic messages
// ---------------------------------------------------------------------------
test("G-002 property-style napi loader failure cases are explicit", () => {
  const napiPath = resolve(__dirname, "../../../../../crates/ferric-napi/index.js");
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;
  const originalPlatform = process.platform;
  const originalArch = process.arch;

  const cases = [
    {
      platform: "darwin",
      arch: "arm64",
      exists: false,
      expected: /Cannot find module|Failed to load native binding/,
    },
    {
      platform: "freebsd",
      arch: "riscv64",
      exists: false,
      expected: /No \.node file found/,
    },
    {
      platform: "freebsd",
      arch: "riscv64",
      exists: true,
      expected: /The \.node file existed but could not be loaded/,
      devReturnsNull: true,
    },
  ];

  for (const item of cases) {
    clearModule(napiPath);
    fs.existsSync = () => item.exists;
    Object.defineProperty(process, "platform", { value: item.platform });
    Object.defineProperty(process, "arch", { value: item.arch });

    try {
      withModuleLoad(
        (request, parent, isMain, originalLoad) => {
          if (item.devReturnsNull && request === join(dirname(napiPath), "ferric-napi.node")) {
            return null;
          }
          return originalLoad(request, parent, isMain);
        },
        () => {
          assert.throws(
            () => requireFromHere(napiPath),
            (err: any) => {
              assert.match(err.message, item.expected);
              return true;
            },
          );
        },
      );
    } finally {
      Object.defineProperty(process, "platform", { value: originalPlatform });
      Object.defineProperty(process, "arch", { value: originalArch });
      fs.existsSync = originalExistsSync;
      clearModule(napiPath);
    }
  }
});

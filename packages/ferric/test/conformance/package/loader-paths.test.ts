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

import {
  FerricRuntimeError,
  FerricSerializationError,
} from "../../../dist/types";

const requireFromHere = createRequire(__filename);
const Module = requireFromHere("node:module") as any;
const releaseNativeLoaderPath = resolve(__dirname, "../../../native/index.js");
const runtimeTargetPath = resolve(
  __dirname,
  "../../../native/runtime-target.js",
);

function clearModule(path: string): void {
  delete requireFromHere.cache[requireFromHere.resolve(path)];
  if (path === releaseNativeLoaderPath) {
    delete requireFromHere.cache[requireFromHere.resolve(runtimeTargetPath)];
  }
}

function withModuleLoad<T>(
  load: (
    request: string,
    parent: unknown,
    isMain: boolean,
    originalLoad: (
      request: string,
      parent: unknown,
      isMain: boolean,
    ) => unknown,
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

function fakeNativeBinding(options?: {
  constructThrows?: boolean;
  snapshotFileThrows?: boolean;
}) {
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

    assertTemplate(
      _templateName: string,
      slots: Record<string, unknown>,
    ): Record<string, unknown> {
      return slots;
    }
  }

  return {
    Engine,
    FerricSymbol,
    nativePackageVersion: () => "0.1.0",
  };
}

interface NativeTargetFixture {
  id: string;
  packageName: string;
  platform: string;
  arch: string;
  os: string[];
  cpu: string[];
  libc?: string[];
}

const declaredTargetFixtures: NativeTargetFixture[] = [
  {
    id: "darwin-arm64",
    packageName: "@ferric-rules/napi-darwin-arm64",
    platform: "darwin",
    arch: "arm64",
    os: ["darwin"],
    cpu: ["arm64"],
  },
  {
    id: "darwin-x64",
    packageName: "@ferric-rules/napi-darwin-x64",
    platform: "darwin",
    arch: "x64",
    os: ["darwin"],
    cpu: ["x64"],
  },
  {
    id: "linux-x64-gnu",
    packageName: "@ferric-rules/napi-linux-x64-gnu",
    platform: "linux",
    arch: "x64",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
  },
  {
    id: "linux-arm64-gnu",
    packageName: "@ferric-rules/napi-linux-arm64-gnu",
    platform: "linux",
    arch: "arm64",
    os: ["linux"],
    cpu: ["arm64"],
    libc: ["glibc"],
  },
  {
    id: "linux-x64-musl",
    packageName: "@ferric-rules/napi-linux-x64-musl",
    platform: "linux",
    arch: "x64",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["musl"],
  },
  {
    id: "linux-arm64-musl",
    packageName: "@ferric-rules/napi-linux-arm64-musl",
    platform: "linux",
    arch: "arm64",
    os: ["linux"],
    cpu: ["arm64"],
    libc: ["musl"],
  },
  {
    id: "win32-x64-msvc",
    packageName: "@ferric-rules/napi-win32-x64-msvc",
    platform: "win32",
    arch: "x64",
    os: ["win32"],
    cpu: ["x64"],
  },
];

function withProcessTarget<T>(platform: string, arch: string, fn: () => T): T {
  const platformDescriptor = Object.getOwnPropertyDescriptor(
    process,
    "platform",
  );
  const archDescriptor = Object.getOwnPropertyDescriptor(process, "arch");
  assert.ok(platformDescriptor);
  assert.ok(archDescriptor);
  Object.defineProperty(process, "platform", { value: platform });
  Object.defineProperty(process, "arch", { value: arch });
  try {
    return fn();
  } finally {
    Object.defineProperty(process, "platform", platformDescriptor);
    Object.defineProperty(process, "arch", archDescriptor);
  }
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
// G-002 manual package loader: native.ts preserves the actionable loader error
// ---------------------------------------------------------------------------
test("G-002 package native loader reports deterministic failure", () => {
  const nativePath = resolve(__dirname, "../../../dist/native.js");
  const bundledPath = resolve(dirname(nativePath), "..", "native", "index.js");

  clearModule(nativePath);
  withModuleLoad(
    (request, parent, isMain, originalLoad) => {
      if (request === bundledPath) {
        throw new Error("native package was omitted");
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
            assert.match(err.message, /native package was omitted/);
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

  for (const [binding, exercise, ErrorClass, strippedMessage] of [
    [
      fakeNativeBinding({ constructThrows: true }),
      (mod: any) => new mod.Engine(),
      FerricRuntimeError,
      "constructor failed",
    ],
    [
      fakeNativeBinding({ snapshotFileThrows: true }),
      (mod: any) => mod.Engine.fromSnapshotFile("bad"),
      FerricSerializationError,
      "bad snapshot file",
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
          assert.throws(
            () => exercise(mod),
            (err: unknown) => {
              // The wrapper must CONVERT the raw napi-style error into the right
              // FerricError subclass AND strip the "FerricXxxError:" prefix. A bare
              // message substring would also match the unconverted error, so we
              // pin both the class and the cleaned message to prove conversion ran.
              assert.ok(
                err instanceof ErrorClass,
                `expected ${ErrorClass.name}, got ${(err as Error)?.constructor?.name}`,
              );
              assert.strictEqual((err as Error).message, strippedMessage);
              return true;
            },
          );
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
  const napiPath = resolve(
    __dirname,
    "../../../../../crates/ferric-rules-napi/index.js",
  );
  const releaseLoaderPath = resolve(__dirname, "../../../native/index.js");
  const devPath = join(dirname(napiPath), "ferric-rules-napi.node");
  const platformPackage = "@ferric-rules/napi-darwin-arm64";
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;

  clearModule(napiPath);
  clearModule(releaseLoaderPath);
  fs.existsSync = (path) => path === devPath || originalExistsSync(path);
  withProcessTarget("darwin", "arm64", () =>
    withModuleLoad(
      (request, parent, isMain, originalLoad) => {
        if (request === devPath) throw new Error("bad local binary");
        if (request === `${platformPackage}/package.json`) {
          return { version: "0.1.0" };
        }
        if (
          request === platformPackage ||
          request.startsWith("@ferric-rules/napi-")
        ) {
          return fakeNativeBinding();
        }
        return originalLoad(request, parent, isMain);
      },
      () => {
        try {
          const mod = requireFromHere(napiPath);
          const engine = new mod.Engine();
          const sym = new mod.FerricSymbol("red");
          const fields = engine.assertFact(
            "color",
            sym,
            [sym],
            null,
            undefined,
            7,
          ) as any[];
          assert.deepStrictEqual(fields[0], {
            __ferric_symbol: true,
            value: "red",
          });
          assert.deepStrictEqual(fields[1][0], {
            __ferric_symbol: true,
            value: "red",
          });
          assert.strictEqual(fields[2], null);
          assert.strictEqual(fields[3], undefined);
          assert.strictEqual(fields[4], 7);

          // Slots can be absent in defensive/mocked calls; the loader should
          // pass non-object slot values through instead of trying to enumerate.
          assert.strictEqual(engine.assertTemplate("thing", null as any), null);
        } finally {
          fs.existsSync = originalExistsSync;
          clearModule(napiPath);
          clearModule(releaseLoaderPath);
        }
      },
    ),
  );
});

// ---------------------------------------------------------------------------
// FR-DIST-002: every declared OS/architecture/libc target is exact
// ---------------------------------------------------------------------------
test("FR-DIST-002 native loader selects exactly one compatible package", () => {
  const releaseLoaderPath = resolve(__dirname, "../../../native/index.js");
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;
  const cases = declaredTargetFixtures.map((target) => ({
    ...target,
    family: target.libc?.[0],
  }));

  fs.existsSync = () => false;
  try {
    for (const item of cases) {
      clearModule(releaseLoaderPath);
      const nativeRequests: string[] = [];
      let detectorCalls = 0;

      withProcessTarget(item.platform, item.arch, () =>
        withModuleLoad(
          (request, parent, isMain, originalLoad) => {
            if (request === "./targets.json") return declaredTargetFixtures;
            if (request === "detect-libc") {
              detectorCalls += 1;
              return {
                familySync: () => item.family,
                GLIBC: "glibc",
                MUSL: "musl",
              };
            }
            if (request.startsWith("@ferric-rules/napi-")) {
              nativeRequests.push(request);
              if (request === `${item.packageName}/package.json`) {
                return { version: "0.1.0" };
              }
              if (request === item.packageName) return fakeNativeBinding();
              throw new Error(
                `loader requested incompatible package ${request}`,
              );
            }
            return originalLoad(request, parent, isMain);
          },
          () => {
            const mod = requireFromHere(releaseLoaderPath);
            assert.strictEqual(typeof mod.Engine, "function");
          },
        ),
      );

      assert.deepStrictEqual(
        nativeRequests,
        [`${item.packageName}/package.json`, item.packageName],
        `${item.id} must not probe a sibling OS, architecture, or libc package`,
      );
      assert.strictEqual(
        detectorCalls,
        item.platform === "linux" ? 1 : 0,
        `${item.id} must ${item.platform === "linux" ? "use" : "skip"} libc detection`,
      );
    }
  } finally {
    fs.existsSync = originalExistsSync;
    clearModule(releaseLoaderPath);
  }
});

// ---------------------------------------------------------------------------
// FR-DIST-002: unknown libc fails closed instead of guessing GNU or musl
// ---------------------------------------------------------------------------
test("FR-DIST-002 native loader fails closed when Linux libc is unknown", () => {
  const releaseLoaderPath = resolve(__dirname, "../../../native/index.js");
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;
  const supported = declaredTargetFixtures
    .map((target) => target.id)
    .join(", ");
  const cases = [
    {
      label: "null",
      familySync: () => null,
      expectedCause: undefined,
    },
    {
      label: "detector exception",
      familySync: () => {
        throw new Error("detector exploded");
      },
      expectedCause: /detector exploded/,
    },
    {
      label: "unrecognized family",
      familySync: () => "bionic",
      expectedCause: /detect-libc returned unsupported family bionic/,
    },
    {
      label: "incomplete detector constants",
      familySync: () => undefined,
      omitConstants: true,
      expectedCause: /detect-libc returned unsupported family undefined/,
    },
    {
      label: "missing familySync export",
      familySync: undefined,
      omitConstants: true,
      expectedCause: /detect-libc familySync export is unavailable/,
    },
  ];

  fs.existsSync = () => true;
  try {
    for (const item of cases) {
      clearModule(releaseLoaderPath);
      const nativeRequests: string[] = [];
      withProcessTarget("linux", "x64", () =>
        withModuleLoad(
          (request, parent, isMain, originalLoad) => {
            if (request === "./targets.json") return declaredTargetFixtures;
            if (request === "detect-libc") {
              return item.omitConstants
                ? { familySync: item.familySync }
                : {
                    familySync: item.familySync,
                    GLIBC: "glibc",
                    MUSL: "musl",
                  };
            }
            if (
              request.endsWith(".node") ||
              request.startsWith("@ferric-rules/napi-")
            ) {
              nativeRequests.push(request);
            }
            return originalLoad(request, parent, isMain);
          },
          () => {
            assert.throws(
              () => requireFromHere(releaseLoaderPath),
              (error: Error & { cause?: unknown }) => {
                assert.match(
                  error.message,
                  /Unsupported native target linux-x64-unknown/,
                  item.label,
                );
                assert.match(
                  error.message,
                  new RegExp(`Supported targets: ${supported}\\.`),
                  item.label,
                );
                if (item.expectedCause) {
                  assert.match(error.message, item.expectedCause, item.label);
                  assert.ok(error.cause instanceof Error, item.label);
                } else {
                  assert.strictEqual(error.cause, undefined, item.label);
                }
                return true;
              },
            );
          },
        ),
      );
      assert.deepStrictEqual(
        nativeRequests,
        [],
        `${item.label} must fail before requiring a development or platform addon`,
      );
    }
  } finally {
    fs.existsSync = originalExistsSync;
    clearModule(releaseLoaderPath);
  }
});

// ---------------------------------------------------------------------------
// FR-DIST-002: unsupported triples identify the complete detected target
// ---------------------------------------------------------------------------
test("FR-DIST-002 unsupported native targets fail before addon loading", () => {
  const releaseLoaderPath = resolve(__dirname, "../../../native/index.js");
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;
  const supported = declaredTargetFixtures
    .map((target) => target.id)
    .join(", ");
  const cases = [
    {
      platform: "linux",
      arch: "riscv64",
      family: "glibc",
      detectedId: "linux-riscv64-gnu",
    },
    {
      platform: "win32",
      arch: "arm64",
      family: undefined,
      detectedId: "win32-arm64-msvc",
    },
    {
      platform: "freebsd",
      arch: "riscv64",
      family: undefined,
      detectedId: "freebsd-riscv64",
    },
  ];

  fs.existsSync = () => true;
  try {
    for (const item of cases) {
      clearModule(releaseLoaderPath);
      const nativeRequests: string[] = [];
      let detectorCalls = 0;
      withProcessTarget(item.platform, item.arch, () =>
        withModuleLoad(
          (request, parent, isMain, originalLoad) => {
            if (request === "./targets.json") return declaredTargetFixtures;
            if (request === "detect-libc") {
              detectorCalls += 1;
              return {
                familySync: () => item.family,
                GLIBC: "glibc",
                MUSL: "musl",
              };
            }
            if (
              request.endsWith(".node") ||
              request.startsWith("@ferric-rules/napi-")
            ) {
              nativeRequests.push(request);
            }
            return originalLoad(request, parent, isMain);
          },
          () => {
            assert.throws(
              () => requireFromHere(releaseLoaderPath),
              (error: Error) => {
                assert.match(
                  error.message,
                  new RegExp(`Unsupported native target ${item.detectedId}\\.`),
                );
                assert.match(
                  error.message,
                  new RegExp(`Supported targets: ${supported}\\.`),
                );
                return true;
              },
            );
          },
        ),
      );
      assert.strictEqual(detectorCalls, item.platform === "linux" ? 1 : 0);
      assert.deepStrictEqual(
        nativeRequests,
        [],
        `${item.detectedId} must fail before requiring an addon`,
      );
    }
  } finally {
    fs.existsSync = originalExistsSync;
    clearModule(releaseLoaderPath);
  }
});

// ---------------------------------------------------------------------------
// FR-DIST-002: corrupt/ambiguous target manifests fail before addon loading
// ---------------------------------------------------------------------------
test("FR-DIST-002 ambiguous native targets fail before addon loading", () => {
  const releaseLoaderPath = resolve(__dirname, "../../../native/index.js");
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;
  const duplicate = {
    ...declaredTargetFixtures.find((target) => target.id === "linux-x64-gnu")!,
    id: "linux-x64-gnu-duplicate",
    packageName: "@ferric-rules/napi-linux-x64-gnu-duplicate",
  };
  const ambiguousTargets = [...declaredTargetFixtures, duplicate];
  const supported = ambiguousTargets.map((target) => target.id).join(", ");
  const nativeRequests: string[] = [];

  fs.existsSync = () => true;
  clearModule(releaseLoaderPath);
  try {
    withProcessTarget("linux", "x64", () =>
      withModuleLoad(
        (request, parent, isMain, originalLoad) => {
          if (request === "./targets.json") return ambiguousTargets;
          if (request === "detect-libc") {
            return {
              familySync: () => "glibc",
              GLIBC: "glibc",
              MUSL: "musl",
            };
          }
          if (
            request.endsWith(".node") ||
            request.startsWith("@ferric-rules/napi-")
          ) {
            nativeRequests.push(request);
          }
          return originalLoad(request, parent, isMain);
        },
        () => {
          assert.throws(
            () => requireFromHere(releaseLoaderPath),
            (error: Error) => {
              assert.match(
                error.message,
                /Ambiguous native target linux-x64-gnu/,
              );
              assert.match(
                error.message,
                /matched linux-x64-gnu, linux-x64-gnu-duplicate/,
              );
              assert.match(
                error.message,
                new RegExp(`Supported targets: ${supported}\\.`),
              );
              return true;
            },
          );
        },
      ),
    );
    assert.deepStrictEqual(
      nativeRequests,
      [],
      "ambiguous detection must fail before requiring an addon",
    );
  } finally {
    fs.existsSync = originalExistsSync;
    clearModule(releaseLoaderPath);
  }
});

// ---------------------------------------------------------------------------
// G-002 table-driven napi loader: package and binary versions must match
// ---------------------------------------------------------------------------
test("G-002 napi loader rejects native package version skew", () => {
  const napiPath = resolve(
    __dirname,
    "../../../../../crates/ferric-rules-napi/index.js",
  );
  const releaseLoaderPath = resolve(__dirname, "../../../native/index.js");
  const platformPackage = "@ferric-rules/napi-darwin-arm64";
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;
  const originalPlatform = process.platform;
  const originalArch = process.arch;

  const cases = [
    {
      label: "platform package metadata",
      metadataVersion: "9.0.0",
      bindingVersion: "0.1.0",
      expected: /package metadata reports 9\.0\.0/,
    },
    {
      label: "embedded native addon",
      metadataVersion: "0.1.0",
      bindingVersion: "9.0.0",
      expected: /Native addon version mismatch/,
    },
  ];

  fs.existsSync = () => false;
  Object.defineProperty(process, "platform", { value: "darwin" });
  Object.defineProperty(process, "arch", { value: "arm64" });

  try {
    for (const item of cases) {
      clearModule(napiPath);
      clearModule(releaseLoaderPath);
      withModuleLoad(
        (request, parent, isMain, originalLoad) => {
          if (request === `${platformPackage}/package.json`) {
            return { version: item.metadataVersion };
          }
          if (request === platformPackage) {
            return {
              ...fakeNativeBinding(),
              nativePackageVersion: () => item.bindingVersion,
            };
          }
          return originalLoad(request, parent, isMain);
        },
        () => {
          assert.throws(
            () => requireFromHere(napiPath),
            (error: any) => {
              assert.match(error.message, item.expected, item.label);
              return true;
            },
          );
        },
      );
    }
  } finally {
    Object.defineProperty(process, "platform", { value: originalPlatform });
    Object.defineProperty(process, "arch", { value: originalArch });
    fs.existsSync = originalExistsSync;
    clearModule(napiPath);
    clearModule(releaseLoaderPath);
  }
});

// ---------------------------------------------------------------------------
// G-002 table-driven napi loader failures produce deterministic messages
// ---------------------------------------------------------------------------
test("G-002 table-driven napi loader failure cases are explicit", () => {
  const napiPath = resolve(
    __dirname,
    "../../../../../crates/ferric-rules-napi/index.js",
  );
  const releaseLoaderPath = resolve(__dirname, "../../../native/index.js");
  const fs = requireFromHere("node:fs") as typeof import("node:fs");
  const originalExistsSync = fs.existsSync;
  const originalPlatform = process.platform;
  const originalArch = process.arch;

  const cases = [
    {
      platform: "darwin",
      arch: "arm64",
      exists: false,
      expected: /Could not load @ferric-rules\/napi-darwin-arm64@0\.1\.0/,
    },
    {
      platform: "freebsd",
      arch: "riscv64",
      exists: false,
      expected: /Unsupported native target freebsd-riscv64/,
    },
    {
      platform: "darwin",
      arch: "arm64",
      exists: true,
      expected: /development addon .* also failed/,
      devReturnsNull: true,
    },
  ];

  for (const item of cases) {
    clearModule(napiPath);
    clearModule(releaseLoaderPath);
    fs.existsSync = () => item.exists;
    Object.defineProperty(process, "platform", { value: item.platform });
    Object.defineProperty(process, "arch", { value: item.arch });

    try {
      withModuleLoad(
        (request, parent, isMain, originalLoad) => {
          if (
            item.devReturnsNull &&
            request === join(dirname(napiPath), "ferric-rules-napi.node")
          ) {
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
      clearModule(releaseLoaderPath);
    }
  }
});

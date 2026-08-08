import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import * as assert from "node:assert/strict";
import { test } from "node:test";

import {
  CANONICAL_NODE_TARGETS,
  DETECT_LIBC_VERSION,
  collectNodeTargetValidationErrors,
  createPlatformManifest,
  targetDetectionKey,
} from "../../../../../scripts/node-package-lib.mjs";

function readJson(relativePath: string): any {
  return JSON.parse(readFileSync(resolve(__dirname, relativePath), "utf8"));
}

const mainPackage = readJson("../../../package.json");
const mainLock = readJson("../../../package-lock.json");
const declaredTargets = readJson("../../../native/targets.json");
const artifactWorkflow = readFileSync(
  resolve(
    __dirname,
    "../../../../../.github/workflows/node-package-artifacts.yml",
  ),
  "utf8",
);
const ciWorkflow = readFileSync(
  resolve(__dirname, "../../../../../.github/workflows/ci.yml"),
  "utf8",
);

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value));
}

function validConfiguration(overrides: Record<string, unknown> = {}): any {
  return {
    targets: clone(CANONICAL_NODE_TARGETS),
    version: mainPackage.version,
    dependencies: clone(mainPackage.dependencies),
    optionalDependencies: clone(mainPackage.optionalDependencies),
    lockedDependencies: clone(mainLock.packages[""].dependencies),
    lockedOptionalDependencies: clone(
      mainLock.packages[""].optionalDependencies,
    ),
    lockedPackages: clone(mainLock.packages),
    ...overrides,
  };
}

function assertHasError(errors: string[], expected: RegExp): void {
  assert.ok(
    errors.some((error) => expected.test(error)),
    `expected ${expected} in:\n${errors.join("\n")}`,
  );
}

function workflowTargetRows(): Array<Record<string, string>> {
  const matrix = artifactWorkflow.match(
    /\n\s+matrix:\n\s+include:\n([\s\S]*?)\n\s+runs-on:/,
  )?.[1];
  assert.ok(matrix, "Node artifact workflow must contain an include matrix");

  return [
    ...matrix.matchAll(/^\s+- target:\s*(\S+)\n((?:\s{12}.+\n?)*)/gm),
  ].map(([, target, body]) => {
    const row: Record<string, string> = { target };
    for (const field of ["runner", "rust_target", "libc"]) {
      const value = body.match(
        new RegExp(`^\\s+${field}:\\s*(\\S+)`, "m"),
      )?.[1];
      if (value) row[field] = value;
    }
    return row;
  });
}

test("G-001 Node target manifest is the exact canonical seven-target matrix", () => {
  assert.deepStrictEqual(declaredTargets, CANONICAL_NODE_TARGETS);
  assert.deepStrictEqual(declaredTargets.map(targetDetectionKey), [
    "darwin/arm64/none",
    "darwin/x64/none",
    "linux/x64/glibc",
    "linux/arm64/glibc",
    "linux/x64/musl",
    "linux/arm64/musl",
    "win32/x64/none",
  ]);
  assert.deepStrictEqual(
    collectNodeTargetValidationErrors(validConfiguration()),
    [],
  );
});

test("G-001 Node package and lock metadata pin every target and detect-libc", () => {
  const packageNames = CANONICAL_NODE_TARGETS.map(
    (target) => target.packageName,
  );
  assert.deepStrictEqual(
    Object.keys(mainPackage.optionalDependencies),
    packageNames,
  );
  assert.deepStrictEqual(
    Object.keys(mainLock.packages[""].optionalDependencies),
    packageNames,
  );
  assert.strictEqual(
    mainPackage.dependencies["detect-libc"],
    DETECT_LIBC_VERSION,
  );
  assert.strictEqual(
    mainLock.packages[""].dependencies["detect-libc"],
    DETECT_LIBC_VERSION,
  );
  assert.strictEqual(
    mainLock.packages["node_modules/detect-libc"].version,
    DETECT_LIBC_VERSION,
  );

  for (const packageName of packageNames) {
    assert.strictEqual(
      mainPackage.optionalDependencies[packageName],
      mainPackage.version,
    );
    assert.strictEqual(
      mainLock.packages[`node_modules/${packageName}`].optional,
      true,
    );
  }
});

test("G-001 generated platform manifests preserve every canonical target selector", () => {
  for (const target of CANONICAL_NODE_TARGETS) {
    const manifest = createPlatformManifest({
      target,
      mainPackage,
      version: mainPackage.version,
    });
    assert.deepStrictEqual(manifest, {
      name: target.packageName,
      version: mainPackage.version,
      description: `Native Ferric addon for ${target.id}`,
      main: "ferric-rules-napi.node",
      files: ["ferric-rules-napi.node"],
      os: target.os,
      cpu: target.cpu,
      ...(target.libc ? { libc: target.libc } : {}),
      engines: mainPackage.engines,
      repository: {
        type: "git",
        url: "git+https://github.com/plx/ferric-rules.git",
      },
      license: mainPackage.license,
      publishConfig: { access: "public" },
    });
  }
});

test("G-001 Node artifact workflow covers the exact native target matrix", () => {
  const contractJob = artifactWorkflow.match(
    /\n  validate-artifact-contract:\n([\s\S]*?)\n  pack-and-smoke:/,
  )?.[1];
  assert.ok(contractJob, "Node artifact workflow must validate clean checkout state");
  const installOffset = contractJob.indexOf("npm ci");
  const buildOffset = contractJob.indexOf("npm run build");
  const testOffset = contractJob.indexOf("./node_modules/.bin/tsx --test");
  assert.ok(installOffset >= 0, "artifact contract job must install dependencies");
  assert.ok(
    buildOffset > installOffset,
    "artifact contract job must build dist after installing dependencies",
  );
  assert.ok(
    testOffset > buildOffset,
    "artifact contract job must build dist before running loader tests",
  );

  const rows = workflowTargetRows();
  assert.deepStrictEqual(rows, [
    {
      target: "darwin-arm64",
      runner: "macos-15",
      rust_target: "aarch64-apple-darwin",
    },
    {
      target: "darwin-x64",
      runner: "macos-15-intel",
      rust_target: "x86_64-apple-darwin",
    },
    {
      target: "linux-x64-gnu",
      runner: "ubuntu-24.04",
      rust_target: "x86_64-unknown-linux-gnu",
    },
    {
      target: "linux-arm64-gnu",
      runner: "ubuntu-24.04-arm",
      rust_target: "aarch64-unknown-linux-gnu",
    },
    {
      target: "linux-x64-musl",
      runner: "ubuntu-24.04",
      rust_target: "x86_64-unknown-linux-musl",
      libc: "musl",
    },
    {
      target: "linux-arm64-musl",
      runner: "ubuntu-24.04-arm",
      rust_target: "aarch64-unknown-linux-musl",
      libc: "musl",
    },
    {
      target: "win32-x64-msvc",
      runner: "windows-2025",
      rust_target: "x86_64-pc-windows-msvc",
    },
  ]);
  assert.strictEqual(new Set(rows.map((row) => row.target)).size, 7);

  const muslRows = rows.filter((row) => row.libc === "musl");
  assert.deepStrictEqual(
    muslRows.map((row) => row.target),
    ["linux-x64-musl", "linux-arm64-musl"],
  );
  for (const target of ["linux-arm64-gnu", "linux-arm64-musl"]) {
    assert.strictEqual(
      rows.find((row) => row.target === target)?.runner,
      "ubuntu-24.04-arm",
    );
  }
});

test("G-001 Node CI preserves the version-locked package metadata", () => {
  const nodeBindingsJob = ciWorkflow.match(
    /\n  node-bindings:\n([\s\S]*?)\n  bindings-conformance:/,
  )?.[1];
  assert.ok(nodeBindingsJob, "CI must contain the Node bindings job");
  assert.strictEqual(
    [...nodeBindingsJob.matchAll(/^\s+npm ci$/gm)].length,
    2,
    "Node binding installs must use the immutable lockfile path",
  );
  assert.strictEqual(
    [...nodeBindingsJob.matchAll(/^\s+npm install$/gm)].length,
    0,
    "Node binding installs must not rewrite the locked native matrix",
  );
});

test("G-001 musl artifact lanes build and smoke in matching Alpine runtimes", () => {
  for (const requiredFragment of [
    "if: matrix.libc == 'musl'",
    "docker run --rm",
    '--env RUST_TARGET="${{ matrix.rust_target }}"',
    '--env TARGET_ID="${{ matrix.target }}"',
    "node:22-alpine",
    'npm run build -- --target "$RUST_TARGET"',
    '--target "$TARGET_ID"',
  ]) {
    assert.ok(
      artifactWorkflow.includes(requiredFragment),
      `Node artifact workflow must contain ${requiredFragment}`,
    );
  }
});

test("G-001 target validation rejects reordered, malformed, and ambiguous rows", () => {
  const reordered = clone(CANONICAL_NODE_TARGETS);
  [reordered[2], reordered[3]] = [reordered[3], reordered[2]];
  assertHasError(
    collectNodeTargetValidationErrors(
      validConfiguration({ targets: reordered }),
    ),
    /canonical order/,
  );

  const missingLibc = clone(CANONICAL_NODE_TARGETS);
  delete missingLibc[2].libc;
  const missingLibcErrors = collectNodeTargetValidationErrors(
    validConfiguration({ targets: missingLibc }),
  );
  assertHasError(missingLibcErrors, /linux-x64-gnu.*fields/);
  assertHasError(missingLibcErrors, /linux-x64-gnu\.libc/);

  const ambiguous = clone(CANONICAL_NODE_TARGETS);
  ambiguous[5].libc = ["glibc"];
  assertHasError(
    collectNodeTargetValidationErrors(
      validConfiguration({ targets: ambiguous }),
    ),
    /ambiguous target detection key linux\/arm64\/glibc/,
  );

  const extraField = clone(CANONICAL_NODE_TARGETS);
  extraField[0].abi = "napi8";
  assertHasError(
    collectNodeTargetValidationErrors(
      validConfiguration({ targets: extraField }),
    ),
    /darwin-arm64.*exactly these fields/,
  );
});

test("G-001 target validation rejects dependency and lock drift", () => {
  const optionalDependencies = clone(mainPackage.optionalDependencies);
  optionalDependencies["@ferric-rules/napi-linux-arm64-musl"] = "0.2.0";
  optionalDependencies["@ferric-rules/napi-freebsd-x64"] = mainPackage.version;
  const optionalErrors = collectNodeTargetValidationErrors(
    validConfiguration({ optionalDependencies }),
  );
  assertHasError(optionalErrors, /linux-arm64-musl.*exact optional dependency/);
  assertHasError(optionalErrors, /unexpected optional dependency.*freebsd/);

  const lockedPackages = clone(mainLock.packages);
  delete lockedPackages["node_modules/@ferric-rules/napi-linux-x64-musl"];
  lockedPackages["node_modules/detect-libc"].dev = true;
  const lockedErrors = collectNodeTargetValidationErrors(
    validConfiguration({
      dependencies: { "detect-libc": "^2.1.2" },
      lockedDependencies: { "detect-libc": "2.0.0" },
      lockedPackages,
    }),
  );
  assertHasError(lockedErrors, /exact runtime dependency/);
  assertHasError(lockedErrors, /version-locked/);
  assertHasError(lockedErrors, /linux-x64-musl.*present and optional/);
  assertHasError(lockedErrors, /regular runtime dependency/);
});

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
    Object.keys(mainPackage.optionalDependencies).sort(),
    [...packageNames].sort(),
  );
  assert.deepStrictEqual(
    Object.keys(mainLock.packages[""].optionalDependencies).sort(),
    [...packageNames].sort(),
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

test("G-001 npm dependency key reordering preserves the target contract", () => {
  const configuration = validConfiguration();
  configuration.optionalDependencies = Object.fromEntries(
    Object.entries(configuration.optionalDependencies).reverse(),
  );
  configuration.lockedOptionalDependencies = Object.fromEntries(
    Object.entries(configuration.lockedOptionalDependencies).sort(([a], [b]) =>
      a.localeCompare(b),
    ),
  );
  assert.deepStrictEqual(collectNodeTargetValidationErrors(configuration), []);

  delete configuration.lockedOptionalDependencies[
    "@ferric-rules/napi-linux-arm64-musl"
  ];
  assertHasError(
    collectNodeTargetValidationErrors(configuration),
    /linux-arm64-musl.*exact locked optional dependency/,
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

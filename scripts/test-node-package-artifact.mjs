#!/usr/bin/env node

import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

import {
  mainPackageDirectory,
  nativeBinaryName,
  nativeCrateDirectory,
  packPackage,
  runCommand,
  runNpmCommand,
  stagePlatformPackage,
  validateNodePackage,
} from "./node-package-lib.mjs";

const requireFromMainPackage = createRequire(
  join(mainPackageDirectory, "package.json"),
);
const {
  detectRuntimeTarget,
  formatRuntimeTarget,
  selectDeclaredTarget,
  targetMatchesRuntime,
} = requireFromMainPackage("./native/runtime-target.js");

function parseArguments(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--target") options.target = argv[++index];
    else if (argument === "--binary") options.binary = argv[++index];
    else if (argument === "--artifacts-dir") {
      options.artifactsDirectory = argv[++index];
    } else {
      throw new Error(`Unknown argument: ${argument}`);
    }
  }
  return options;
}

const options = parseArguments(process.argv.slice(2));
const { mainPackage, targets } = await validateNodePackage();
const runtime = detectRuntimeTarget();
const detectedTarget = selectDeclaredTarget(targets, runtime);
const targetId = options.target ?? detectedTarget.id;
const target = targets.find((candidate) => candidate.id === targetId);
if (!target) throw new Error(`Unknown target ${targetId}`);
if (!targetMatchesRuntime(target, runtime)) {
  throw new Error(
    `Cannot smoke-test ${targetId} on ${formatRuntimeTarget(runtime)}`,
  );
}

const workDirectory = await mkdtemp(join(tmpdir(), "ferric-node-package-"));
const artifactsDirectory = options.artifactsDirectory
  ? resolve(options.artifactsDirectory)
  : join(workDirectory, "artifacts");
const dependencyArtifactsDirectory = join(
  workDirectory,
  "dependency-artifacts",
);
const platformStage = join(workDirectory, "platform-package");
const consumerDirectory = join(workDirectory, "consumer");
const binaryPath = resolve(
  options.binary ?? join(nativeCrateDirectory, nativeBinaryName),
);

try {
  await mkdir(consumerDirectory, { recursive: true });
  await stagePlatformPackage({
    targetId,
    binaryPath,
    outputDirectory: platformStage,
  });

  const platformPack = await packPackage({
    packageDirectory: platformStage,
    artifactsDirectory,
    runScripts: false,
  });
  const detectLibcDirectory = dirname(
    requireFromMainPackage.resolve("detect-libc/package.json"),
  );
  const detectLibcPack = await packPackage({
    packageDirectory: detectLibcDirectory,
    artifactsDirectory: dependencyArtifactsDirectory,
    runScripts: false,
  });
  if (
    detectLibcPack.record.name !== "detect-libc" ||
    detectLibcPack.record.version !== mainPackage.dependencies?.["detect-libc"]
  ) {
    throw new Error(
      `Packed detect-libc ${String(detectLibcPack.record.version)}, expected ` +
        String(mainPackage.dependencies?.["detect-libc"]),
    );
  }
  const mainPack = await packPackage({
    packageDirectory: mainPackageDirectory,
    artifactsDirectory,
    runScripts: true,
  });

  const mainNativeFiles = mainPack.files
    .filter((path) => path.startsWith("native/"))
    .sort();
  const expectedMainNativeFiles = [
    "native/index.js",
    "native/runtime-target.js",
    "native/targets.json",
  ];
  if (
    JSON.stringify(mainNativeFiles) !== JSON.stringify(expectedMainNativeFiles)
  ) {
    throw new Error(
      `Main npm tarball native payload is ${mainNativeFiles.join(", ")}, ` +
        `expected ${expectedMainNativeFiles.join(", ")}`,
    );
  }
  if (mainPack.files.some((path) => path.endsWith(".node"))) {
    throw new Error("Main npm tarball contains a host-specific .node file");
  }
  const platformFiles = [...platformPack.files].sort();
  const expectedPlatformFiles = [
    "README.md",
    nativeBinaryName,
    "package.json",
  ].sort();
  if (JSON.stringify(platformFiles) !== JSON.stringify(expectedPlatformFiles)) {
    throw new Error(
      `Platform npm tarball contains ${platformFiles.join(", ")}, expected ` +
        expectedPlatformFiles.join(", "),
    );
  }

  await writeFile(
    join(consumerDirectory, "package.json"),
    '{\n  "name": "ferric-clean-consumer",\n  "private": true\n}\n',
    "utf8",
  );

  runNpmCommand(
    [
      "install",
      "--offline",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      "--package-lock=false",
      detectLibcPack.archivePath,
      platformPack.archivePath,
      mainPack.archivePath,
    ],
    {
      cwd: consumerDirectory,
      env: {
        npm_config_cache: join(workDirectory, "empty-npm-cache"),
      },
    },
  );

  const commonJsSmoke = `
    (async () => {
      const assert = require("node:assert/strict");
      const mainMetadata = require("@ferric-rules/node/package.json");
      const nativeMetadata = require(${JSON.stringify(
        `${target.packageName}/package.json`,
      )});
      const rawNative = require(${JSON.stringify(target.packageName)});
      assert.equal(nativeMetadata.version, mainMetadata.version);
      assert.equal(rawNative.nativePackageVersion(), mainMetadata.version);
      const { Engine, EngineHandle } = require("@ferric-rules/node");
      const engine = Engine.fromSource(
        "(defrule smoke => (assert (packaged-result 42)))"
      );
      const result = engine.run();
      assert.equal(result.rulesFired, 1);
      assert.equal(engine.findFacts("packaged-result").length, 1);
      engine.close();

      const handle = await EngineHandle.create({
        source: "(defrule worker-smoke => (assert (worker-result 42)))",
      });
      const workerResult = await handle.run();
      assert.equal(workerResult.rulesFired, 1);
      await handle.close();
    })().catch((error) => {
      console.error(error);
      process.exitCode = 1;
    });
  `;
  runCommand(process.execPath, ["-e", commonJsSmoke], {
    cwd: consumerDirectory,
  });

  const moduleSmoke = `
    import assert from "node:assert/strict";
    const ferric = await import("@ferric-rules/node");
    assert.equal(typeof ferric.Engine, "function");
    const engine = ferric.Engine.fromSource(
      "(defrule smoke => (assert (module-result 42)))"
    );
    const result = engine.run();
    assert.equal(result.rulesFired, 1);
    engine.close();
  `;
  runCommand(process.execPath, ["--input-type=module", "-e", moduleSmoke], {
    cwd: consumerDirectory,
  });

  console.log(
    `clean npm artifact smoke passed for ${targetId}: ` +
      `${mainPack.record.filename} + ${platformPack.record.filename}`,
  );
} finally {
  await rm(workDirectory, { recursive: true, force: true });
}

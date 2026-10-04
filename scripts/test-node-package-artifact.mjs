#!/usr/bin/env node

// Exercise the current host's packed facade and addon from an external install.
// Build the native addon first; this script builds TypeScript explicitly.
import { copyFile, mkdtemp, mkdir, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { npmInvocation } from "./npm-command.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const mainPackageDirectory = join(repositoryRoot, "packages", "ferric");
const nativeCrateDirectory = join(repositoryRoot, "crates", "ferric-rules-napi");
const nativeBinaryName = "ferric-rules-napi.node";
const requireFromMainPackage = createRequire(join(mainPackageDirectory, "package.json"));
const { detectRuntimeTarget, selectDeclaredTarget } =
  requireFromMainPackage("./native/runtime-target.js");

if (process.argv.length !== 2) {
  throw new Error("usage: node scripts/test-node-package-artifact.mjs");
}
const [mainPackage, nativePackage, targets] = await Promise.all([
  readFile(join(mainPackageDirectory, "package.json"), "utf8").then(JSON.parse),
  readFile(join(nativeCrateDirectory, "package.json"), "utf8").then(JSON.parse),
  readFile(join(mainPackageDirectory, "native", "targets.json"), "utf8").then(JSON.parse),
]);
if (nativePackage.version !== mainPackage.version) {
  throw new Error(`Native package version ${nativePackage.version} differs from facade ${mainPackage.version}`);
}
const target = selectDeclaredTarget(targets, detectRuntimeTarget());

function runCommand(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    ...options,
    env: { ...process.env, NODE_PATH: "", NODE_OPTIONS: "", ...options.env },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `Command failed (${result.status}): ${command} ${args.join(" ")}\n` +
      `${result.stdout ?? ""}${result.stderr ?? ""}`,
    );
  }
  return result;
}

function runNpmCommand(args, options = {}) {
  const { command, shell } = npmInvocation();
  return runCommand(command, args, { ...options, shell });
}

async function packPackage(packageDirectory, artifactsDirectory) {
  await mkdir(artifactsDirectory, { recursive: true });
  const result = runNpmCommand(
    ["pack", "--json", "--ignore-scripts", "--pack-destination", artifactsDirectory],
    { cwd: packageDirectory, env: { npm_config_loglevel: "silent" } },
  );
  const records = JSON.parse(result.stdout);
  if (!Array.isArray(records) || records.length !== 1 || typeof records[0].filename !== "string") {
    throw new Error("Expected one local npm pack result");
  }
  return { archivePath: join(artifactsDirectory, records[0].filename), record: records[0] };
}

const workDirectory = await mkdtemp(join(tmpdir(), "ferric-node-consumer-"));
const artifactsDirectory = join(workDirectory, "artifacts");
const dependencyArtifactsDirectory = join(workDirectory, "dependency-artifacts");
const platformStage = join(workDirectory, "platform-package");
const consumerDirectory = join(workDirectory, "consumer");

try {
  const location = relative(await realpath(repositoryRoot), await realpath(workDirectory));
  if (location === "" || (!isAbsolute(location) && location !== ".." && !location.startsWith(`..${sep}`))) {
    throw new Error("Temporary consumer must be outside the checkout; choose an external TMPDIR");
  }
  runNpmCommand(["run", "build"], { cwd: mainPackageDirectory });
  await mkdir(platformStage);
  await mkdir(consumerDirectory);
  await copyFile(
    join(nativeCrateDirectory, nativeBinaryName),
    join(platformStage, nativeBinaryName),
  );
  await writeFile(join(platformStage, "package.json"), JSON.stringify({
    name: target.packageName,
    version: mainPackage.version,
    main: nativeBinaryName,
    files: [nativeBinaryName],
    os: target.os,
    cpu: target.cpu,
    ...(target.libc ? { libc: target.libc } : {}),
    engines: mainPackage.engines,
    license: mainPackage.license,
  }, null, 2) + "\n");
  const platformPack = await packPackage(platformStage, artifactsDirectory);
  const detectLibcPack = await packPackage(
    dirname(requireFromMainPackage.resolve("detect-libc/package.json")),
    dependencyArtifactsDirectory,
  );
  if (detectLibcPack.record.version !== mainPackage.dependencies?.["detect-libc"]) {
    throw new Error(`Installed detect-libc ${detectLibcPack.record.version} does not match the facade dependency`);
  }
  // Pack local compiler/type dependencies so the consumer's type resolution
  // has no path back into checkout node_modules and needs no registry/cache.
  const typeTools = [];
  for (const name of ["typescript", "@types/node", "undici-types"]) {
    typeTools.push(await packPackage(
      dirname(requireFromMainPackage.resolve(`${name}/package.json`)),
      dependencyArtifactsDirectory,
    ));
  }
  const mainPack = await packPackage(mainPackageDirectory, artifactsDirectory);

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
      ...typeTools.map((tool) => tool.archivePath),
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

  await writeFile(join(consumerDirectory, "launch-selection.clp"), await readFile(join(repositoryRoot, "examples/embedding/launch-selection.clp")));
  await writeFile(join(consumerDirectory, "consumer.cts"), `
    import ferric = require("@ferric-rules/node");
    const engine = new ferric.Engine();
    const id: ferric.FactId = engine.assertFact("typed", 9223372036854775807n);
    engine.retract(id);
    engine[Symbol.dispose]();
    async function worker() {
      const handle = await ferric.EngineHandle.create();
      const snapshot: Buffer = await handle.serialize(ferric.Format.Cbor);
      await handle[Symbol.asyncDispose]();
      return snapshot;
    }
    void worker;
  `);
  await writeFile(join(consumerDirectory, "consumer.mts"), `
    import { Engine, EngineHandle, Format, type RunResult } from "@ferric-rules/node";
    const engine = new Engine();
    const result: RunResult = engine.run(100);
    const restored = Engine.fromSnapshot(engine.serialize(), Format.Cbor);
    engine.close(); restored.close();
    const dynamic = await import("@ferric-rules/node");
    const handle = await EngineHandle.create();
    await handle.close();
    void result; void dynamic;
  `);
  for (const resolution of ["Node16", "NodeNext"]) {
    runCommand(process.execPath, [join(consumerDirectory, "node_modules/typescript/bin/tsc"), "--noEmit", "--strict", "--target", "ES2022", "--module", resolution, "--moduleResolution", resolution, "--types", "node", "consumer.cts", "consumer.mts"], { cwd: consumerDirectory });
  }

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
      const ferric = require("@ferric-rules/node");
      const { Engine, EngineHandle, FerricParseError, Format } = ferric;
      assert.throws(() => require("@ferric-rules/node/dist/index.js"), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
      const launch = require("node:fs").readFileSync("launch-selection.clp", "utf8");
      const direct = Engine.fromSource(launch);
      const checkpoint = direct.serialize();
      const launchEngines = [direct, Engine.fromSnapshot(checkpoint, Format.Cbor), await EngineHandle.create({ source: launch }), await EngineHandle.create({ snapshot: { data: checkpoint } })];
      for (const current of launchEngines) {
        assert.deepEqual(await current.run(current instanceof EngineHandle ? { limit: 100 } : 100), { rulesFired: 1, haltReason: 0 });
        const actions = await current.findFacts("action");
        assert.equal(actions.length, 1);
        assert.deepEqual(actions[0].fields.map((value) => value.value), ["session-42", "sign-in"]);
        assert.equal(await current.getOutput("t"), "action session-42 sign-in\\n");
        assert.equal((await current.run()).rulesFired, 0);
        await current.close();
      }
      const invalid = new Engine();
      assert.throws(() => invalid.load("(defrule incomplete"), FerricParseError);
      invalid.close();
      assert.equal(Object.hasOwn(ferric, "__continueRun"), false);
      assert.equal(
        Object.hasOwn(rawNative.Engine.prototype, "__continueRun"),
        false,
      );
      assert.equal(typeof rawNative.__continueRun, "function");
      const engine = Engine.fromSource(
        "(defrule smoke => (assert (packaged-result 42)))"
      );
      assert.equal(Reflect.has(engine, "__continueRun"), false);
      for (
        let prototype = Object.getPrototypeOf(engine);
        prototype !== null;
        prototype = Object.getPrototypeOf(prototype)
      ) {
        assert.equal(Object.hasOwn(prototype, "__continueRun"), false);
      }
      const result = engine.run();
      assert.equal(result.rulesFired, 1);
      assert.equal(engine.findFacts("packaged-result").length, 1);
      engine.close();

      const handle = await EngineHandle.create({
        source: \`
          (deffacts start (position 0))
          (defrule halt-at-boundary
            (declare (salience 100))
            (position 100)
            =>
            (halt))
          (defrule advance
            ?current <- (position ?n&:(< ?n 100))
            =>
            (retract ?current)
            (assert (position (+ ?n 1))))
        \`,
      });
      const workerResult = await handle.run();
      assert.deepEqual(workerResult, { rulesFired: 101, haltReason: 2 });
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
    import { Engine, EngineHandle } from "@ferric-rules/node";
    const ferric = await import("@ferric-rules/node");
    assert.equal(ferric.Engine, Engine);
    assert.equal(ferric.EngineHandle, EngineHandle);
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
    `clean npm host consumer passed for ${target.id}: ` +
      `${mainPack.record.filename} + ${platformPack.record.filename}`,
  );
} finally {
  await rm(workDirectory, { recursive: true, force: true });
}

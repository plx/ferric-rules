import {
  copyFile,
  mkdir,
  readFile,
  readdir,
  stat,
  writeFile,
} from "node:fs/promises";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { npmInvocation } from "./npm-command.mjs";
import { findNonLfTextFiles } from "./node-package-text.mjs";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));

export const repositoryRoot = resolve(scriptDirectory, "..");
export const mainPackageDirectory = join(repositoryRoot, "packages", "ferric");
export const nativeCrateDirectory = join(
  repositoryRoot,
  "crates",
  "ferric-rules-napi",
);
export const nativeBinaryName = "ferric-rules-napi.node";
export const DETECT_LIBC_VERSION = "2.1.2";

export const CANONICAL_NODE_TARGETS = Object.freeze([
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
]);

const targetsPath = join(mainPackageDirectory, "native", "targets.json");

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

async function writeJson(path, value) {
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

export async function loadTargets() {
  return readJson(targetsPath);
}

function sameSequence(actual, expected) {
  return (
    actual.length === expected.length &&
    actual.every((value, index) => value === expected[index])
  );
}

function formatValue(value) {
  return JSON.stringify(value) ?? String(value);
}

export function targetDetectionKey(target) {
  const libc =
    Array.isArray(target.libc) && target.libc.length === 1
      ? target.libc[0]
      : "none";
  return `${String(target.platform)}/${String(target.arch)}/${String(libc)}`;
}

function validateVersionMap({ errors, label, actual, expectedNames, version }) {
  if (actual === null || typeof actual !== "object" || Array.isArray(actual)) {
    errors.push(`${label} must be an object`);
    return;
  }

  const actualNames = Object.keys(actual);
  if (!sameSequence(actualNames, expectedNames)) {
    errors.push(
      `${label} must follow canonical target order: ${expectedNames.join(", ")}`,
    );
  }

  for (const packageName of expectedNames) {
    if (actual[packageName] !== version) {
      errors.push(`${packageName} must be an exact ${label} at ${version}`);
    }
  }
  for (const packageName of actualNames) {
    if (!expectedNames.includes(packageName)) {
      errors.push(`unexpected ${label} ${packageName}`);
    }
  }
}

export function collectNodeTargetValidationErrors({
  targets,
  version,
  dependencies = {},
  optionalDependencies = {},
  lockedDependencies = {},
  lockedOptionalDependencies = {},
  lockedPackages = {},
}) {
  const errors = [];
  const expectedIds = CANONICAL_NODE_TARGETS.map((target) => target.id);
  const expectedPackageNames = CANONICAL_NODE_TARGETS.map(
    (target) => target.packageName,
  );
  const canonicalById = new Map(
    CANONICAL_NODE_TARGETS.map((target) => [target.id, target]),
  );

  if (!Array.isArray(targets)) {
    return ["native/targets.json must contain an array"];
  }

  if (targets.length !== CANONICAL_NODE_TARGETS.length) {
    errors.push(
      `native/targets.json must declare exactly ${CANONICAL_NODE_TARGETS.length} targets, found ${targets.length}`,
    );
  }

  const actualIds = targets.map((target) => target?.id);
  if (!sameSequence(actualIds, expectedIds)) {
    errors.push(
      `native targets must follow canonical order: ${expectedIds.join(", ")}`,
    );
  }

  const ids = new Set();
  const packageNames = new Set();
  const detectionKeys = new Set();
  for (const target of targets) {
    if (
      target === null ||
      typeof target !== "object" ||
      Array.isArray(target)
    ) {
      errors.push(
        `native target rows must be objects, found ${formatValue(target)}`,
      );
      continue;
    }

    const id = String(target.id);
    const packageName = String(target.packageName);
    const detectionKey = targetDetectionKey(target);
    if (ids.has(id)) errors.push(`duplicate target id ${id}`);
    if (packageNames.has(packageName)) {
      errors.push(`duplicate native package ${packageName}`);
    }
    if (detectionKeys.has(detectionKey)) {
      errors.push(`ambiguous target detection key ${detectionKey}`);
    }
    ids.add(id);
    packageNames.add(packageName);
    detectionKeys.add(detectionKey);

    const canonical = canonicalById.get(id);
    if (!canonical) {
      errors.push(`unexpected native target id ${id}`);
      continue;
    }

    const actualKeys = Object.keys(target).sort();
    const expectedKeys = Object.keys(canonical).sort();
    if (!sameSequence(actualKeys, expectedKeys)) {
      errors.push(
        `${id} must contain exactly these fields: ${expectedKeys.join(", ")}`,
      );
    }
    for (const [field, expected] of Object.entries(canonical)) {
      if (formatValue(target[field]) !== formatValue(expected)) {
        errors.push(
          `${id}.${field} must be ${formatValue(expected)}, found ${formatValue(target[field])}`,
        );
      }
    }
  }

  for (const id of expectedIds) {
    if (!ids.has(id)) errors.push(`missing native target ${id}`);
  }

  validateVersionMap({
    errors,
    label: "optional dependency",
    actual: optionalDependencies,
    expectedNames: expectedPackageNames,
    version,
  });
  validateVersionMap({
    errors,
    label: "locked optional dependency",
    actual: lockedOptionalDependencies,
    expectedNames: expectedPackageNames,
    version,
  });

  if (dependencies["detect-libc"] !== DETECT_LIBC_VERSION) {
    errors.push(
      `detect-libc must be an exact runtime dependency at ${DETECT_LIBC_VERSION}`,
    );
  }
  if (lockedDependencies["detect-libc"] !== DETECT_LIBC_VERSION) {
    errors.push(
      `detect-libc must be version-locked at ${DETECT_LIBC_VERSION} in package-lock.json`,
    );
  }

  const expectedLockedPackagePaths = new Set(
    expectedPackageNames.map((name) => `node_modules/${name}`),
  );
  const lockedNativePackagePaths = Object.keys(lockedPackages).filter((path) =>
    path.startsWith("node_modules/@ferric-rules/napi-"),
  );
  for (const path of expectedLockedPackagePaths) {
    if (lockedPackages[path]?.optional !== true) {
      errors.push(`${path} must be present and optional in package-lock.json`);
    }
  }
  for (const path of lockedNativePackagePaths) {
    if (!expectedLockedPackagePaths.has(path)) {
      errors.push(`unexpected locked native package ${path}`);
    }
  }

  const lockedDetectLibc = lockedPackages["node_modules/detect-libc"];
  if (lockedDetectLibc?.version !== DETECT_LIBC_VERSION) {
    errors.push(
      `node_modules/detect-libc must be locked at ${DETECT_LIBC_VERSION}`,
    );
  }
  if (lockedDetectLibc?.dev === true || lockedDetectLibc?.optional === true) {
    errors.push("detect-libc must be a regular runtime dependency");
  }

  return errors;
}

export function npmArchiveName(packageName, version) {
  return `${packageName.replace(/^@/, "").replaceAll("/", "-")}-${version}.tgz`;
}

function workspaceVersion(cargoManifest) {
  const workspaceSection = cargoManifest.match(
    /\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/,
  );
  const version = workspaceSection?.[1].match(
    /^\s*version\s*=\s*"([^"]+)"\s*$/m,
  );
  return version?.[1];
}

export async function validateNodePackage() {
  const [
    mainPackage,
    mainLock,
    nativePackage,
    cargoManifest,
    targets,
    nativeEntries,
    mainPackageText,
    nativeLoaderText,
    runtimeTargetText,
    targetsText,
  ] = await Promise.all([
    readJson(join(mainPackageDirectory, "package.json")),
    readJson(join(mainPackageDirectory, "package-lock.json")),
    readJson(join(nativeCrateDirectory, "package.json")),
    readFile(join(repositoryRoot, "Cargo.toml"), "utf8"),
    loadTargets(),
    readdir(join(mainPackageDirectory, "native")),
    readFile(join(mainPackageDirectory, "package.json"), "utf8"),
    readFile(join(mainPackageDirectory, "native", "index.js"), "utf8"),
    readFile(join(mainPackageDirectory, "native", "runtime-target.js"), "utf8"),
    readFile(targetsPath, "utf8"),
  ]);

  const errors = [];
  const version = mainPackage.version;
  const cargoVersion = workspaceVersion(cargoManifest);
  const dependencies = mainPackage.dependencies ?? {};
  const optionalDependencies = mainPackage.optionalDependencies ?? {};
  const lockedDependencies = mainLock.packages?.[""]?.dependencies ?? {};
  const lockedOptionalDependencies =
    mainLock.packages?.[""]?.optionalDependencies ?? {};

  if (nativePackage.version !== version) {
    errors.push(
      `crates/ferric-rules-napi/package.json is ${nativePackage.version}, expected ${version}`,
    );
  }
  if (cargoVersion !== version) {
    errors.push(
      `Cargo workspace is ${String(cargoVersion)}, expected ${version}`,
    );
  }
  if (nativePackage.private !== true) {
    errors.push(
      "crates/ferric-rules-napi/package.json must stay private; release payloads use generated platform packages",
    );
  }
  if (
    !mainPackage.files?.some((entry) => entry.replace(/\/$/, "") === "native")
  ) {
    errors.push("packages/ferric/package.json must include native/ in files");
  }

  const nonLfTextFiles = findNonLfTextFiles([
    {
      path: "packages/ferric/package.json",
      contents: mainPackageText,
    },
    {
      path: "packages/ferric/native/index.js",
      contents: nativeLoaderText,
    },
    {
      path: "packages/ferric/native/runtime-target.js",
      contents: runtimeTargetText,
    },
    {
      path: "packages/ferric/native/targets.json",
      contents: targetsText,
    },
  ]);
  if (nonLfTextFiles.length !== 0) {
    errors.push(
      `release package text files must use LF line endings: ${nonLfTextFiles.join(", ")}`,
    );
  }

  const looseNativeBinaries = nativeEntries.filter((entry) =>
    entry.endsWith(".node"),
  );
  if (looseNativeBinaries.length !== 0) {
    errors.push(
      `the main package must not bundle host-specific binaries: ${looseNativeBinaries.join(", ")}`,
    );
  }

  errors.push(
    ...collectNodeTargetValidationErrors({
      targets,
      version,
      dependencies,
      optionalDependencies,
      lockedDependencies,
      lockedOptionalDependencies,
      lockedPackages: mainLock.packages ?? {},
    }),
  );

  if (errors.length !== 0) {
    throw new Error(
      `Node release package validation failed:\n${errors
        .map((error) => `- ${error}`)
        .join("\n")}`,
    );
  }

  return { mainPackage, targets, version };
}

export function createPlatformManifest({ target, mainPackage, version }) {
  return {
    name: target.packageName,
    version,
    description: `Native Ferric addon for ${target.id}`,
    main: nativeBinaryName,
    files: [nativeBinaryName],
    os: target.os,
    cpu: target.cpu,
    ...(target.libc ? { libc: target.libc } : {}),
    engines: mainPackage.engines,
    repository: {
      type: "git",
      url: "git+https://github.com/plx/ferric-rules.git",
    },
    license: mainPackage.license,
    publishConfig: {
      access: "public",
    },
  };
}

export async function stagePlatformPackage({
  targetId,
  binaryPath,
  outputDirectory,
}) {
  const { mainPackage, targets, version } = await validateNodePackage();
  const target = targets.find((candidate) => candidate.id === targetId);
  if (!target) {
    throw new Error(
      `Unknown native target ${targetId}. Expected one of: ${targets
        .map((candidate) => candidate.id)
        .join(", ")}`,
    );
  }

  const binary = await stat(binaryPath);
  if (!binary.isFile()) {
    throw new Error(`Native addon is not a file: ${binaryPath}`);
  }

  await mkdir(outputDirectory, { recursive: true });
  const existing = await readdir(outputDirectory);
  if (existing.length !== 0) {
    throw new Error(
      `Refusing to stage a native package into non-empty directory ${outputDirectory}`,
    );
  }

  const platformManifest = createPlatformManifest({
    target,
    mainPackage,
    version,
  });

  await Promise.all([
    copyFile(binaryPath, join(outputDirectory, nativeBinaryName)),
    writeJson(join(outputDirectory, "package.json"), platformManifest),
    writeFile(
      join(outputDirectory, "README.md"),
      `# ${target.packageName}\n\n` +
        `Native ${target.id} payload for ` +
        `[\`@ferric-rules/node\`](https://www.npmjs.com/package/@ferric-rules/node) ` +
        `version ${version}. Install the main package instead of depending on ` +
        `this package directly.\n`,
      "utf8",
    ),
  ]);

  return { target, platformManifest };
}

export function runCommand(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    ...options,
    env: {
      ...process.env,
      ...options.env,
    },
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

export function runNpmCommand(args, options = {}) {
  const { command, shell } = npmInvocation();
  return runCommand(command, args, {
    ...options,
    shell,
  });
}

function parsePackOutput(stdout) {
  const trimmed = stdout.trim();
  try {
    return JSON.parse(trimmed);
  } catch {
    const start = trimmed.lastIndexOf("\n[");
    if (start === -1)
      throw new Error(`npm pack returned invalid JSON:\n${stdout}`);
    return JSON.parse(trimmed.slice(start + 1));
  }
}

export async function packPackage({
  packageDirectory,
  artifactsDirectory,
  runScripts,
}) {
  await mkdir(artifactsDirectory, { recursive: true });
  const args = ["pack", "--json", "--pack-destination", artifactsDirectory];
  if (!runScripts) args.push("--ignore-scripts");

  const result = runNpmCommand(args, {
    cwd: packageDirectory,
    env: { npm_config_loglevel: "silent" },
  });
  const records = parsePackOutput(result.stdout);
  if (!Array.isArray(records) || records.length !== 1) {
    throw new Error(`Expected one npm pack record, received ${records.length}`);
  }
  const record = records[0];
  return {
    archivePath: join(artifactsDirectory, record.filename),
    files: record.files.map((file) => file.path),
    record,
  };
}

export async function sha256(path) {
  const hash = createHash("sha256");
  hash.update(await readFile(path));
  return hash.digest("hex");
}

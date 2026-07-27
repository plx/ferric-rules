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
  "ferric-napi",
);
export const nativeBinaryName = "ferric-napi.node";

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
    readFile(targetsPath, "utf8"),
  ]);

  const errors = [];
  const version = mainPackage.version;
  const cargoVersion = workspaceVersion(cargoManifest);
  const optionalDependencies = mainPackage.optionalDependencies ?? {};
  const lockedOptionalDependencies =
    mainLock.packages?.[""]?.optionalDependencies ?? {};

  if (nativePackage.version !== version) {
    errors.push(
      `crates/ferric-napi/package.json is ${nativePackage.version}, expected ${version}`,
    );
  }
  if (cargoVersion !== version) {
    errors.push(
      `Cargo workspace is ${String(cargoVersion)}, expected ${version}`,
    );
  }
  if (nativePackage.private !== true) {
    errors.push(
      "crates/ferric-napi/package.json must stay private; release payloads use generated platform packages",
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

  const ids = new Set();
  const packageNames = new Set();
  const detectionKeys = new Set();
  for (const target of targets) {
    const detectionKey = `${target.platform}-${target.arch}`;
    if (ids.has(target.id)) errors.push(`duplicate target id ${target.id}`);
    if (packageNames.has(target.packageName)) {
      errors.push(`duplicate native package ${target.packageName}`);
    }
    if (detectionKeys.has(detectionKey)) {
      errors.push(`ambiguous target detection key ${detectionKey}`);
    }
    ids.add(target.id);
    packageNames.add(target.packageName);
    detectionKeys.add(detectionKey);

    if (optionalDependencies[target.packageName] !== version) {
      errors.push(
        `${target.packageName} must be an exact optional dependency at ${version}`,
      );
    }
    if (lockedOptionalDependencies[target.packageName] !== version) {
      errors.push(
        `${target.packageName} must be version-locked in package-lock.json`,
      );
    }
  }

  for (const packageName of Object.keys(optionalDependencies)) {
    if (!packageNames.has(packageName)) {
      errors.push(`unexpected optional dependency ${packageName}`);
    }
  }
  for (const packageName of Object.keys(lockedOptionalDependencies)) {
    if (!packageNames.has(packageName)) {
      errors.push(`unexpected locked optional dependency ${packageName}`);
    }
  }

  if (errors.length !== 0) {
    throw new Error(
      `Node release package validation failed:\n${errors
        .map((error) => `- ${error}`)
        .join("\n")}`,
    );
  }

  return { mainPackage, targets, version };
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

  const platformManifest = {
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

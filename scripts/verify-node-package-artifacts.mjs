#!/usr/bin/env node

import { readdir } from "node:fs/promises";
import { join, resolve } from "node:path";

import {
  loadTargets,
  npmArchiveName,
  sha256,
  validateNodePackage,
} from "./node-package-lib.mjs";

async function findTarballs(directory) {
  const results = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) results.push(...(await findTarballs(path)));
    else if (entry.name.endsWith(".tgz")) results.push(path);
  }
  return results;
}

const artifactRoot = resolve(process.argv[2] ?? "");
if (!process.argv[2]) {
  throw new Error(
    "usage: verify-node-package-artifacts.mjs <artifact-directory>",
  );
}

const { mainPackage, version } = await validateNodePackage();
const targets = await loadTargets();
const tarballs = await findTarballs(artifactRoot);
const byName = new Map();
for (const path of tarballs) {
  const name = path.split(/[\\/]/).at(-1);
  const paths = byName.get(name) ?? [];
  paths.push(path);
  byName.set(name, paths);
}

const mainArchive = npmArchiveName(mainPackage.name, version);
const expectedArchives = new Set([
  mainArchive,
  ...targets.map((target) => npmArchiveName(target.packageName, version)),
]);
for (const archive of byName.keys()) {
  if (!expectedArchives.has(archive)) {
    throw new Error(`Unexpected Node release artifact ${archive}`);
  }
}

const mainArchives = byName.get(mainArchive) ?? [];
if (mainArchives.length !== targets.length) {
  throw new Error(
    `Expected ${targets.length} independently packed ${mainArchive} files, ` +
      `found ${mainArchives.length}`,
  );
}

const mainHashes = new Set();
for (const path of mainArchives) mainHashes.add(await sha256(path));
if (mainHashes.size !== 1) {
  throw new Error(
    `Main npm tarball is not deterministic across targets: ${[
      ...mainHashes,
    ].join(", ")}`,
  );
}

for (const target of targets) {
  const archive = npmArchiveName(target.packageName, version);
  const matches = byName.get(archive) ?? [];
  if (matches.length !== 1) {
    throw new Error(`Expected one ${archive}, found ${matches.length}`);
  }
}

console.log(
  `verified deterministic Node artifact set: ${targets.length} native ` +
    `packages and ${mainArchive} sha256=${[...mainHashes][0]}`,
);

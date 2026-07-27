# Node package release artifacts

`@ferric-rules/node` uses platform-specific optional dependencies so a normal
npm install receives a native addon without compiling Rust or retaining a
source checkout.

## Artifact contract

The main tarball contains the compiled JavaScript/declarations plus
`native/index.js` and `native/targets.json`. It never contains a host-specific
`.node` file. Its optional dependencies are exact-version pins to the native
packages below:

| Target ID | Native package | Runtime selector |
| --- | --- | --- |
| `darwin-arm64` | `@ferric-rules/napi-darwin-arm64` | macOS arm64 |
| `darwin-x64` | `@ferric-rules/napi-darwin-x64` | macOS x64 |
| `linux-x64-gnu` | `@ferric-rules/napi-linux-x64-gnu` | Linux x64 with glibc |
| `win32-x64-msvc` | `@ferric-rules/napi-win32-x64-msvc` | Windows x64 |

Each native package contains only its package metadata, README, and
`ferric-napi.node`. `native/targets.json` is the source of truth used by both
the loader and release assembly. FR-DIST-002 owns any expansion or refinement
of this matrix.

The Rust addon exports `nativePackageVersion()`. Loading succeeds only when
the main package version, selected native package version, and embedded addon
version are identical. A missing package, unsupported target, load failure, or
version mismatch fails before an engine is exposed and identifies the expected
package in the error.

## Local release dry run

Run:

```sh
just node-package-smoke
```

This builds the release-profile N-API addon, builds the TypeScript package,
packs the exact main and current-platform native tarballs, installs both
tarballs offline into a temporary project outside the repository, then uses
CommonJS and dynamic `import()` to create, run, and close engines. The smoke
also checks that the binary reports the same version as both package manifests.

The `Node Package Artifacts` workflow repeats this operation natively on every
declared target, uploads the exact tarballs, checks target coverage, and
requires the independently packed main tarball to be byte-identical across the
matrix. It stages artifacts only; publishing to npm remains subject to the
production-readiness release checkpoint.

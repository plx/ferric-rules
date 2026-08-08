# Node package release artifacts

`@ferric-rules/node` uses platform-specific optional dependencies so a normal
npm install receives a native addon without compiling Rust or retaining a
source checkout.

## Artifact contract

The main tarball contains the compiled JavaScript/declarations plus
`native/index.js`, `native/runtime-target.js`, and `native/targets.json`. It
never contains a host-specific `.node` file. Its regular `detect-libc` runtime
dependency is exactly pinned so Linux selection uses a tested detector, and its
optional dependencies are exact-version pins to the native packages below:

| Target ID | Native package | Runtime selector |
| --- | --- | --- |
| `darwin-arm64` | `@ferric-rules/napi-darwin-arm64` | macOS arm64 |
| `darwin-x64` | `@ferric-rules/napi-darwin-x64` | macOS x64 |
| `linux-x64-gnu` | `@ferric-rules/napi-linux-x64-gnu` | Linux x64 with glibc |
| `linux-arm64-gnu` | `@ferric-rules/napi-linux-arm64-gnu` | Linux arm64 with glibc |
| `linux-x64-musl` | `@ferric-rules/napi-linux-x64-musl` | Linux x64 with musl |
| `linux-arm64-musl` | `@ferric-rules/napi-linux-arm64-musl` | Linux arm64 with musl |
| `win32-x64-msvc` | `@ferric-rules/napi-win32-x64-msvc` | Windows x64 |

Each native package contains only its package metadata, README, and
`ferric-rules-napi.node`. `native/targets.json` is the source of truth used by both
the loader and release assembly. Future matrix changes must preserve exact
version alignment, unambiguous OS/architecture/libc detection, and an executable
clean-install smoke for every declared target.

The Rust addon exports `nativePackageVersion()`. Loading succeeds only when
the main package version, selected native package version, and embedded addon
version are identical. Linux selection includes the runtime C library, so a
glibc process cannot select a musl package or vice versa. A missing package,
unsupported target, inconclusive Linux C-library detection, load failure, or
version mismatch fails before an engine is exposed and identifies the detected
target, the expected package when one is declared, and the supported target
alternatives for unsupported combinations.

## Local release dry run

Run:

```sh
just node-package-smoke
```

This builds the release-profile N-API addon, builds the TypeScript package,
detects the current OS, architecture, and Linux C library, and packs the exact
main and current-platform native tarballs. It also packs the installed,
version-locked `detect-libc` dependency into temporary test storage so the
consumer install uses no registry or pre-existing npm cache. It installs all
three tarballs offline into a temporary project outside the repository, then
uses CommonJS and dynamic `import()` to create, run, and close engines. The smoke
also checks that the binary reports the same version as both package manifests.

The `Node Package Artifacts` workflow repeats this operation on every declared
target. macOS, Windows, and Linux glibc jobs run on matching hosted
architectures. Linux musl jobs build and execute inside `node:22-alpine` on a
matching-architecture Linux host; they do not use CPU emulation. The workflow
uploads the exact tarballs, checks target coverage, and requires the
independently packed main tarball to be byte-identical across the matrix. It
does not upload the temporary dependency tarball. It stages release artifacts
only; publishing to npm remains subject to the production-readiness release
checkpoint.

# Python package release contract

This document defines the release surface for the `ferric` Python distribution
and import package. It is the human-readable companion to the versioned
machine contract in
[`wheel-targets.json`](../crates/ferric-rules-python/wheel-targets.json).
Changes to either document must keep package metadata, CI, validators, and
consumer smoke tests in agreement.

This contract is limited to Python distribution artifacts. The broader
cross-binding minimum-runtime, sanitizer, stress, and clean-consumer CI program
remains tracked in
[#124](https://github.com/plx/ferric-rules/issues/124); this matrix does not
claim to complete that work.

## Package identity and support boundary

- Distribution name: `ferric`.
- Import name: `ferric`.
- Version source: `[workspace.package].version` in the root `Cargo.toml`;
  `pyproject.toml` declares its version dynamically so Maturin reads that
  source rather than maintaining a second literal.
- Python: GIL-enabled CPython 3.9, 3.10, 3.11, 3.12, and 3.13.
- Packaging range: `Requires-Python: >=3.9,<3.14`.
- ABI: PyO3 `abi3-py39`, producing `cp39-abi3` wheels.

Python 3.14 is not admitted merely because the stable ABI may be loadable on a
newer interpreter. PyPy, GraalPy, free-threaded CPython, CPython
subinterpreters, macOS universal2, Windows Arm64, and unlisted platforms and
architectures are unsupported. Their absence is intentional, not missing CI
coverage. Supported main-interpreter shutdown includes Rust-only, exactly-once
cleanup of `Engine` objects regardless of the Python thread that releases the
final reference; it does not imply extension loading in a subinterpreter.

On those supported GIL-enabled interpreters, the runtime releases the GIL only
for the explicitly documented `Engine` load, run, snapshot, and file-operation
cohort and while `close()` (including context-manager exit) waits for and
destroys native state. The native engine remains on its creator OS thread, and
ordinary calls remain creator-thread-affine. This behavior neither admits a
free-threaded build nor expands the interpreter, ABI, or platform matrix above;
the complete ordering and lifecycle semantics live in the package
[threading, GIL, and lifecycle contract](../crates/ferric-rules-python/README.md#threading-gil-and-lifecycle-contract).

## ABI decision

The project uses `abi3-py39` instead of per-minor CPython wheels. The baseline
matches the package's minimum Python version and reduces the release set from
35 platform/minor wheels to seven platform wheels. Every one of those wheels is
still installed and exercised separately on each supported Python minor.

The stable ABI restricts PyO3 to CPython's limited API and may forgo
version-specific boundary optimizations. No performance improvement is claimed
from this choice. Free-threaded CPython has a distinct ABI and is not covered by
`abi3`.

Cargo's workspace release profile strips symbols by default. The Python package
overrides that setting with `strip = "none"`, and Maturin also has
`strip = false`, because link-time stripping of an abi3 extension produced
malformed Mach-O string-table alignment with an Apple linker. Native import and
dependency inspection remain mandatory on both macOS architectures.

Rust's dynamically loaded musl target normally requests `libgcc_s.so.1`. The
release workflow pins each architecture's `rust-musl-cross` image index by
digest, then replaces that toolchain's single dynamic linker argument with its
static `libgcc_eh` archive. It also links the same pinned musl libc through its
architecture-canonical runtime name (`libc.musl-*.so.1`) instead of the sysroot
file's bare `libc.so` name. This keeps each musllinux wheel self-contained apart
from musl itself, makes the runtime dependency portable and auditable, and
preserves the exact one-native-module archive contract. The wrapper fails if
Rust stops emitting exactly one `-lgcc_s` or one `-lc` argument, so toolchain
drift cannot silently change this linkage policy.

The repaired musllinux wheel is audited inside a digest-pinned,
matching-architecture Alpine container. Running `auditwheel show` directly on
the glibc host can mistake musl's `libc.so` linker script for an ELF object.
The container receives a read-only wheel and a pinned, host-prepared
`auditwheel` environment, runs without network access, and writes only to a
temporary filesystem. The workflow requires `auditwheel` to report the exact
contracted `musllinux_1_2` architecture tag; its exit status alone is not
accepted because the tool can successfully report a weaker generic Linux tag.

Maturin's package configuration sets `locked = true`. This makes every Cargo
invocation reject dependency drift even though Maturin's standalone `sdist`
command has no `--locked` CLI option. Because Maturin relocates and prunes the
workspace manifest when it creates an sdist, its raw archive is only an
intermediate: the release tooling normalizes that relocated workspace's
`Cargo.lock` offline, proves it with `cargo metadata --locked --offline`, and
then deterministically repacks the candidate before any build or smoke test.

## Wheel matrix

The release set contains exactly seven wheels:

| Contract ID | Native runner | Rust target | Required platform tag or tags | Compatibility floor |
| --- | --- | --- | --- | --- |
| `manylinux2014-x86_64` | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` | `manylinux_2_17_x86_64`, `manylinux2014_x86_64` | glibc 2.17 |
| `manylinux2014-aarch64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `manylinux_2_17_aarch64`, `manylinux2014_aarch64` | glibc 2.17 |
| `musllinux1_2-x86_64` | `ubuntu-24.04` | `x86_64-unknown-linux-musl` | `musllinux_1_2_x86_64` | musl 1.2 |
| `musllinux1_2-aarch64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-musl` | `musllinux_1_2_aarch64` | musl 1.2 |
| `macos-x86_64` | `macos-15-intel` | `x86_64-apple-darwin` | `macosx_10_12_x86_64` | macOS 10.12 |
| `macos-arm64` | `macos-15` | `aarch64-apple-darwin` | `macosx_11_0_arm64` | macOS 11.0 |
| `windows-x86_64` | `windows-2025` | `x86_64-pc-windows-msvc` | `win_amd64` | 64-bit Windows |

Every filename and internal `WHEEL` record must combine the declared platform
tag set with `cp39-abi3`. The two manylinux spellings are equivalent tags on a
single wheel, not separate artifacts.

## Source distribution policy

Each verified bundle also contains exactly one `.tar.gz` source distribution.
The sdist must include the Python crate, its required Rust workspace crates, a
lockfile normalized for the relocated workspace, the package README, and both
license texts. The raw Maturin output is never the release candidate. A job
safe-extracts it, updates only the relocated lockfile without network access,
checks the result with locked and offline Cargo metadata, and deterministically
repacks it. A clean job then builds a wheel from those exact final bytes and
runs the same package smoke before the sdist can enter the bundle.

An sdist consumer needs:

- CPython `>=3.9,<3.14`;
- Rust 1.75 or newer;
- Maturin `>=1.0,<2.0`;
- the target's native compiler, linker, and platform development tools; and
- network access or pre-populated Python and Cargo caches for dependency
  resolution.

Supported users should receive a wheel and therefore should not need any of
these source-build tools. The sdist is not a substitute for a missing declared
wheel.

## Artifact verification

The Python artifact workflow must fail closed through this sequence:

1. Build each wheel with locked Cargo and Maturin inputs on its declared native
   runner. Package-level `locked = true` must remain active for the sdist and
   PEP 517 paths. Apply the declared manylinux, musllinux, or macOS
   compatibility floor.
2. Repair or audit the wheel with the platform-native equivalent of
   `auditwheel`, `delocate`, or `delvewheel`. Only the final inspected wheel may
   be uploaded as a workflow artifact.
3. Verify the archive layout, distribution metadata, license files, ABI and
   platform tags, native dependencies, and `RECORD` hashes. Record its SHA-256
   digest in the release manifest.
4. Download that exact artifact into clean consumer jobs for CPython 3.9 through
   3.13. Install with `pip --no-index --no-deps --only-binary=:all:` from a
   directory outside the checkout, then import, load and run rules,
   serialize/restore, and close all engines.
5. Normalize and deterministically repack Maturin's raw workspace sdist as
   described above. Re-extract the exact final archive, require locked and
   offline Cargo metadata, build its wheel through PEP 517, and run the package
   smoke. Aggregate exactly seven verified wheels and that one verified sdist;
   reject missing, duplicate, or extra files.
6. Run a non-mutating registry check against the downloaded aggregate, for
   example:

   ```sh
   uv publish --dry-run --trusted-publishing never --no-attestations \
     <each-explicitly-verified-distribution>
   ```

The publish dry run must use explicit verified paths. A permissive glob, an
artifact built again in the promotion job, a network-backed `pip` fallback, or
smoke testing only before artifact upload does not satisfy the contract.

## Publication boundary

Artifact building, repair, inspection, clean installation, attestation, and a
non-mutating registry dry run are permitted staging steps. Stable publication
to PyPI is an irreversible action and is not authorized by this contract or by
the existence of a successful workflow run.

No stable Python distribution may be published until the independent audit in
[#223](https://github.com/plx/ferric-rules/issues/223) has approved the exact
candidate and the maintainer has authorized the exact artifacts under the
[staged-artifact and publication policy](audits/production-readiness-remediation-goal.md#staged-artifacts-and-irreversible-publication).
The eventual promotion job must download the already verified bytes, recheck
their manifest, and must not rebuild them.

## Updating the contract

Treat `wheel-targets.json` as a versioned interface. A change to a Python minor,
ABI baseline, target, runner, Rust triple, compatibility floor, exclusion, or
sdist rule requires an intentional schema/contract update and matching
validation. Do not infer support from whatever a build host happens to provide.

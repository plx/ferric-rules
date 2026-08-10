# Rust package release

The Rust registry namespace is `ferric-rules`. The unqualified `ferric` and
`ferric-core` names are owned by unrelated crates on crates.io, so every Cargo
package in this repository uses the collision-free `ferric-rules-*` prefix.
Rust converts package hyphens to import underscores:

| Cargo package | Rust import | Published |
| --- | --- | --- |
| `ferric-rules` | `ferric_rules` | yes, public facade |
| `ferric-rules-core` | `ferric_rules_core` | yes |
| `ferric-rules-parser` | `ferric_rules_parser` | yes |
| `ferric-rules-runtime` | `ferric_rules_runtime` | yes |
| `ferric-rules-pinned` | `ferric_rules_pinned` | yes |
| `ferric-rules-cli` | binary remains `ferric` | yes |
| `ferric-rules-ffi` | `ferric_rules_ffi` | yes |
| `ferric-rules-ffi-macros` | internal proc-macro dependency | yes |
| `ferric-rules-napi` | `ferric_rules_napi` | no; shipped through npm |
| `ferric-rules-python` | extension remains `ferric` | no; shipped through PyPI |
| `ferric-rules-bench-gen` | n/a | no; repository tool |

All internal path dependencies also carry the exact synchronized registry
version. Cargo removes their `path` keys when producing an archive, leaving a
complete registry dependency graph. The workspace Cargo configuration patches
unpublished internal dependencies back to the local sources so first-release
package and dry-run checks work before crates.io contains them; Cargo
configuration is not included in the package archives.

## Native facade and CLI target contract

The distributed Rust artifacts covered by the native matrix are the
`ferric-rules` facade source package and the `ferric-rules-cli` source package.
The machine-readable source of truth is
[`release-targets.json`](../crates/ferric-rules-cli/release-targets.json). It
limits the package set to `ferric-rules` and `ferric-rules-cli`, names `ferric`
as the evidence binary, requires an all-feature install, and labels that binary
`ci-evidence-only`. Each target records its family, native environment, C
library, and `native` conformance. The declaration contains exactly these
rows:

| Target ID | Rust target | Native evidence environment | Declared libc |
| --- | --- | --- | --- |
| `linux-x86_64-gnu` | `x86_64-unknown-linux-gnu` | Ubuntu 24.04 x86-64 | `glibc` |
| `linux-aarch64-gnu` | `aarch64-unknown-linux-gnu` | Ubuntu 24.04 AArch64 | `glibc` |
| `linux-x86_64-musl` | `x86_64-unknown-linux-musl` | matching-architecture, digest-pinned Alpine container on Ubuntu 24.04 x86-64 | `musl` 1.2.x |
| `linux-aarch64-musl` | `aarch64-unknown-linux-musl` | matching-architecture, digest-pinned Alpine container on Ubuntu 24.04 AArch64 | `musl` 1.2.x |
| `macos-x86_64` | `x86_64-apple-darwin` | macOS 15 Intel | `none` |
| `macos-aarch64` | `aarch64-apple-darwin` | macOS 15 Apple silicon | `none` |
| `windows-x86_64-msvc` | `x86_64-pc-windows-msvc` | Windows 2025 x86-64 | `msvc` |

Each row is build-and-execute evidence on the declared OS and architecture.
The musl rows execute inside a matching-architecture musl userspace without
CPU emulation. Cross-compilation alone is non-conformance and cannot satisfy a
row. Unlisted targets are not part of the supported Rust/CLI release matrix and
must not be advertised as such.

The Ubuntu 24.04 glibc environment is the continuously tested source-build
environment, not a claim that an uploaded binary has a particular historical
glibc compatibility floor. Likewise, this matrix does not declare a macOS
deployment minimum. The project currently distributes Cargo source packages,
not downloadable per-target CLI binaries; binaries retained by CI are test
evidence only.

For pushes to `main`, pull requests targeting `main`, and manual dispatches,
the path-unfiltered `Rust Native Artifacts` workflow checks out the immutable
pull-request head (or push commit) directly. On every row it:

1. uses Rust 1.93.0 and verifies that the compiler host and observed runtime
   match the declaration;
2. runs release-profile, all-feature facade and CLI tests and builds the CLI;
3. packages the facade and CLI source crates;
4. installs the all-feature CLI from its source path into a temporary prefix
   outside the worktree; and
5. executes the installed binary from outside the worktree, covering version
   metadata, Unicode paths and output, CRLF input, snapshot creation/loading,
   REPL EOF, documented process exit codes, and native dynamic dependencies.

Every row retains the two `.crate` files, the installed CLI binary, dynamic
dependency output, and a receipt containing the candidate commit/tree,
toolchain and runtime identities, commands, and artifact SHA-256 digests. A
fail-closed aggregate job requires all seven rows, compares each receipt with
the target declaration and direct candidate, rechecks every retained hash, and
retains one candidate-SHA-named verified evidence bundle. That aggregate
exposes the stable `Rust Native Artifacts` check context; changes to the formal
required-status ruleset remain outside this lane.

This native portability lane is intentionally narrower than the clean-room
install contract in [FR-RELEASE-008 (#153)](https://github.com/plx/ferric-rules/issues/153).
Here, `cargo install --path` reads the candidate source tree while placing and
executing the binary outside it. Issue #153 owns installation from extracted
`.crate` files, empty-cache and offline registry-like reconstruction, package
omission and version-skew cases, uninstall behavior, and the complete
clean-consumer CLI suite. Binding-specific C, Go, Node, and Python matrices
remain with [FR-DIST-008 (#124)](https://github.com/plx/ferric-rules/issues/124),
and workspace-wide all-feature, hardening, audit, scaling, Miri, sanitizer, and
fuzz status policy remains with
[FR-RELEASE-004 (#150)](https://github.com/plx/ferric-rules/issues/150).

## Local artifact verification

Run:

```sh
just verify-rust-packages
cargo package -p ferric-rules --list --locked
cargo package -p ferric-rules --locked
cargo publish -p ferric-rules --dry-run --locked
```

The verifier compares the facade's `cargo package --list` output with a
reviewed allowlist, archives the dependency crates in release order, extracts
them outside the source workspace, vendors third-party dependencies, and runs
the facade's all-features test suite with an empty Cargo home in offline mode.
The isolated verifier models a registry consumer before the first versions
exist on crates.io, without weakening the published manifests.

## Publication order

Publish a version in dependency tiers. Wait for each tier to appear in the
crates.io index before publishing the next:

```sh
# Tier 1: independent packages
cargo publish -p ferric-rules-parser --locked
cargo publish -p ferric-rules-core --locked
cargo publish -p ferric-rules-ffi-macros --locked

# Tier 2: depends on parser and core
cargo publish -p ferric-rules-runtime --locked

# Tier 3: depends on runtime
cargo publish -p ferric-rules --locked
cargo publish -p ferric-rules-pinned --locked
cargo publish -p ferric-rules-cli --locked

# Tier 4: also depends on pinned
cargo publish -p ferric-rules-ffi --locked
```

For subsequent synchronized releases, run the same sequence with `--dry-run`
appended after the previous version is indexed. During the initial release,
the local Cargo patches make the facade's required dry run possible; run later
package dry runs after their preceding tier is indexed. Actual publishing
still follows the tiers because the patches are local-only and registry
consumers cannot resolve a later tier until its prerequisites are indexed.

After publishing, confirm that a fresh consumer can resolve only the facade:

```sh
tmpdir="$(mktemp -d)"
cd "$tmpdir"
cargo init --lib --name ferric-rules-consumer
cargo add ferric-rules@0.1.0
cargo test --locked
```

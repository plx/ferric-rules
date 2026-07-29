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

# Dependency checks

Run `just dependency-policy` to scan the locked Rust, npm, and Python dependencies
and verify Rust licenses and notices. The same command runs in CI on changes and
weekly against new advisories. Scanner errors and findings fail the job; reports
are retained in `dependency-policy-evidence/` and uploaded even on failure.
`just dependency-policy-test` exercises real scanner rejection of a vulnerable
npm runtime dependency, a Python version hidden behind a non-host marker, and
malformed Cargo/Python configuration.

The scanners own report parsing and advisory matching. There is no separate
exception evaluator, workspace graph hash, SBOM reconciliation service, or
calendar-based waiver renewal. The retired `dependency-policy.json` and Python
policy engine are preserved in Git history; their expiry dates no longer control
CI. This is a deliberate replacement of the former release-program contract,
not a claim that its original exit criteria were completed.

## Covered surfaces

| Surface | Check |
| --- | --- |
| Cargo workspace, all features and targets, including development/build dependencies | `cargo deny --locked --all-features check advisories bans licenses sources`, configured in [`deny.toml`](../deny.toml) |
| `packages/ferric`, `crates/ferric-rules-napi`, `documentation`, `site` | `npm audit --package-lock-only --audit-level=info`, including dev, optional, and peer dependencies |
| `crates/ferric-rules-python`, `tools/ferric-tools` | `uv export --locked --all-groups --all-extras --no-emit-project --format pylock.toml`, then `pip-audit --locked --strict` |
| Rust third-party license texts | Existing `cargo-about` configuration and `just license-notices-check` |

The native pylock reader audits every exported name/version, including versions
selected only on another Python/OS marker. It does not install or execute these
packages. Build tools locked in dependency groups (including maturin) are covered;
unlocked PEP 517 isolated build environments are not represented as locked scans.
The Python wheel's existing artifact-owned SBOM checks remain independent. The Go
module and optional integrations retain their existing build/lint/lifecycle checks;
this command does not audit their third-party dependencies. Add new Rust, npm,
or Python dependency surfaces to the short loop
in [`scripts/dependency-check.sh`](../scripts/dependency-check.sh) when introducing
them; no new policy schema is needed.

CI uses Rust 1.93.0, cargo-deny 0.20.2, cargo-about 0.9.0, Node 22.18.0,
uv 0.11.16, and pip-audit 2.10.1. These tool versions are installation choices,
not a separate graph-authentication protocol. Install Cargo tools with
`cargo install --locked cargo-deny --version 0.20.2` and
`cargo install --locked cargo-about --version 0.9.0 --features cli` (or upstream
release binaries); the script runs pip-audit through `uvx`.

## Reviewed exceptions

Checked against live advisories and current call sites on September 6, 2026.
Rust exceptions use cargo-deny's advisory-specific `ignore` entries, with
`unused-ignored-advisory = "deny"` so an upgrade removes obsolete entries.
The sole Python exception is passed only for the binding's lockfile. There are
no npm exceptions, severity cutoffs, or package-wide suppressions. New security
findings require fixing or an applicability decision in the same reviewed change;
the entries below do not accept future advisories for these dependencies.

| Advisory and affected surface | Applicability, mitigation, and reconsideration condition |
| --- | --- |
| [RUSTSEC-2025-0020](https://rustsec.org/advisories/RUSTSEC-2025-0020.html), PyO3 0.23.5 in the Python extension | The vulnerable `PyString::from_object` / `from_object_bound` decoder is not called. `src/value.rs` downcasts strings then extracts UTF-8, and creates strings with `PyString::new`; those use different PyO3 paths. Reconsider before adding encoding/decoding APIs or upgrading PyO3; remove when using >=0.24.1. |
| [RUSTSEC-2026-0177](https://rustsec.org/advisories/RUSTSEC-2026-0177.html), PyO3 0.23.5 in the Python extension | The vulnerable `PyCFunction::new_closure` / `new_closure_bound` constructors are not called. Generated `#[pymethods]` and the `wrap_pyfunction!` instance-count function use C method definitions, not closure construction. Reconsider before introducing Python callbacks or upgrading PyO3; remove when using >=0.29.0. GIL presence alone is **not** the justification. |
| [RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html), bincode 1.3.3 in optional snapshots | Maintenance notice, without a reported vulnerability. Kept for existing pre-1.0 snapshot consumers while the versioned persistence contract is implemented. Reconsider with that implementation or any reported serializer vulnerability; maintenance status is not evidence that arbitrary serialized engine state is safe. |
| [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html), paste 1.0.15 through rmp 0.8.14 / rmp-serde 1.3.0 | Maintenance notice for a build-time macro, without a reported vulnerability. Reconsider when updating MessagePack dependencies or if a concrete macro defect affects generated code. |
| [GHSA-6w46-j5rx-g56g](https://github.com/advisories/GHSA-6w46-j5rx-g56g), pytest 8.4.2 in Python 3.9 development tests | The patched pytest 9 requires Python >=3.10. Keep the declared Python 3.9 binding support: the suite's `conftest.py` creates an unpredictable private parent using `TemporaryDirectory` and configures pytest's `basetemp` inside it, avoiding the shared `/tmp/pytest-of-USER` path. A regression checks ownership and mode 0700 on POSIX. pytest is absent from wheels. Explicit `--basetemp` overrides are the caller's responsibility. Remove when a fixed Python-3.9-compatible pytest exists or Python 3.9 support is deliberately retired. |

Reproduce the PyO3 applicability inspection with
`rg 'from_object|new_closure|PyString|wrap_pyfunction' crates/ferric-rules-python/src`
and inspect the matching constructors/extractors in the locked PyO3 source.
Changes to those call sites must reassess the exception; unrelated Cargo graph
changes do not create new renewal work.

## September 6 replacement evidence

The baseline scan at `a38de6a852cce3f503467cd000ba7b182c4b5b30` reproduced
all 19 previous exception records. Compatible npm lock updates patched esbuild,
fast-uri, js-yaml, nanoid, and postcss. Targeted uv updates patched click,
Pygments, and pytest on Python >=3.10. Disabling postcard's unused default
`heapless-cas` feature removed atomic-polyfill (RUSTSEC-2023-0089) and its
transitive graph; Ferric uses postcard's `alloc` APIs. No snapshot format changed.
The remaining five exceptions are the independently assessed entries above.

Rust license allowlists, crate-specific MPL permission for the cbindgen build
tool, and generated [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md) remain.
New npm/Python license enforcement and release-wide SBOM products are deferred;
advisory scanning for their actual graphs remains active. Raw before/after
scanner logs are local execution evidence, while CI retains the native reports
for each checked revision.

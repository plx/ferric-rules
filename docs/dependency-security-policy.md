# Dependency checks

Run `just dependency-policy` to scan the locked Rust, npm, and Python dependencies
and verify Rust licenses and notices. The same command runs in CI on pull
requests, pushes to `main`, and weekly against new advisories. Scanner errors and
findings fail the job; scanner output appears in the job log and, locally, in
`dependency-policy-evidence/`.

The scanners own report parsing and advisory matching. There is no separate
exception evaluator, workspace graph hash, SBOM reconciliation service, or
calendar-based waiver renewal. The earlier bespoke `dependency-policy.json` and
Python policy engine remain available in the repository history (see
[history](history.md)).

## Covered surfaces

| Surface | Check |
| --- | --- |
| Cargo workspace, all features and targets, including development/build dependencies | `cargo deny --locked --all-features check advisories bans licenses sources`, configured in [`deny.toml`](../deny.toml) |
| `packages/ferric`, `crates/ferric-rules-napi`, `site` | `npm audit --package-lock-only --audit-level=info`, including dev, optional, and peer dependencies |
| `crates/ferric-rules-python`, `tools/ferric-tools` | `uv export --locked --all-groups --all-extras --no-emit-project --format pylock.toml`, then `pip-audit --locked --strict` |
| Rust third-party license texts | Existing `cargo-about` configuration and `just license-notices-check` |

The native pylock reader audits every exported name/version, including versions
selected only on another Python/OS marker. It does not install or execute these
packages. Build tools locked in dependency groups (including maturin) are covered;
unlocked PEP 517 isolated build environments are not represented as locked scans.
The Go module and optional integrations retain their existing build/lint/lifecycle checks;
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

The pytest exception below was reviewed on September 6, 2026 and is passed
only for the binding's lockfile. There are no Rust or npm advisory exceptions,
severity cutoffs, or package-wide suppressions. Cargo-deny's
`unused-ignored-advisory = "deny"` remains enabled. New security findings require
fixing or an applicability decision in the same reviewed change; this exception
does not accept future advisories for the dependency.

| Advisory and affected surface | Applicability, mitigation, and reconsideration condition |
| --- | --- |
| [GHSA-6w46-j5rx-g56g](https://github.com/advisories/GHSA-6w46-j5rx-g56g), pytest 8.4.2 in Python 3.9 development tests | The patched pytest 9 requires Python >=3.10. Keep the declared Python 3.9 binding support: the suite's `conftest.py` creates an unpredictable private parent using `TemporaryDirectory` and configures pytest's `basetemp` inside it, avoiding the shared `/tmp/pytest-of-USER` path. A regression checks ownership and mode 0700 on POSIX. pytest is absent from wheels. Explicit `--basetemp` overrides are the caller's responsibility. Remove when a fixed Python-3.9-compatible pytest exists or Python 3.9 support is deliberately retired. |

## September 6 replacement evidence

The baseline scan at `a38de6a852cce3f503467cd000ba7b182c4b5b30` reproduced
all 19 previous exception records. Compatible npm lock updates patched esbuild,
fast-uri, js-yaml, nanoid, and postcss. Targeted uv updates patched click,
Pygments, and pytest on Python >=3.10. Removing the experimental bincode,
MessagePack and Postcard snapshot codecs later removed bincode
(RUSTSEC-2025-0141), paste/rmp (RUSTSEC-2024-0436) and postcard from the
dependency graph. Upgrading PyO3 to 0.29.3 removed the remaining Rust advisory
exceptions for RUSTSEC-2025-0020 and RUSTSEC-2026-0177; the pytest exception
above remains.

Rust license allowlists, crate-specific MPL permission for the cbindgen build
tool, and generated [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md) remain.
New npm/Python license enforcement and release-wide SBOM products are deferred;
advisory scanning for their actual graphs remains active. Raw before/after
scanner logs are local execution evidence.

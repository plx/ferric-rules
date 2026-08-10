# Dependency security policy

This document is the human-readable contract for the repository's dependency
advisory, license, and software bill of materials (SBOM) gate. The versioned
machine source of truth is [`dependency-policy.json`](../dependency-policy.json),
and Rust license, source, ban, and advisory rules that are native to
`cargo-deny` live in [`deny.toml`](../deny.toml). A change to the covered
projects, finding normalization, exception fields, tool versions, or SBOM
requirements must update the machine policy, its deterministic fixtures, and
this document together.

The policy is deliberately fail-closed. Severity, development-only status, or
an assertion that vulnerable code is unreachable provides context for review;
none of them silently suppresses a finding.

## Covered dependency graphs

Version 1 covers every dependency group in these seven committed lockfile
surfaces:

| Project ID | Ecosystem | Manifest | Lockfile | Component owner |
| --- | --- | --- | --- | --- |
| `rust-workspace` | Cargo | `Cargo.toml` | `Cargo.lock` | `release-engineering` |
| `node-package` | npm | `packages/ferric/package.json` | `packages/ferric/package-lock.json` | `typescript-bindings` |
| `node-addon` | npm | `crates/ferric-rules-napi/package.json` | `crates/ferric-rules-napi/package-lock.json` | `typescript-bindings` |
| `documentation` | npm | `documentation/package.json` | `documentation/package-lock.json` | `documentation` |
| `site` | npm | `site/package.json` | `site/package-lock.json` | `documentation` |
| `python-package` | PyPI/uv | `crates/ferric-rules-python/pyproject.toml` | `crates/ferric-rules-python/uv.lock` | `python-bindings` |
| `python-tools` | PyPI/uv | `tools/ferric-tools/pyproject.toml` | `tools/ferric-tools/uv.lock` | `tooling` |

The Cargo inventory is the complete `Cargo.lock` union, including workspace,
runtime, build, development, optional, and target-only packages across every
feature and target. It is not the host-only graph produced by one build. The
npm inventories include production, development, optional, and peer
dependencies. The uv inventories include all groups and extras and every
locked registry name/version variant selected by an environment marker, not
only the packages applicable to the CI host.
Documentation and site dependencies are covered because their code executes in
development and CI, even though those static outputs are not release packages.
The evaluator also performs a lexical repository census: the committed tree
must contain exactly the one Cargo lock, the root Cargo manifest plus its
safely expanded workspace-member manifests, four npm manifests and locks, and
two Python manifests and uv locks listed above. A standalone `Cargo.toml` or a
new `package.json`, `package-lock.json`, `pyproject.toml`, `uv.lock`, or
`Cargo.lock` is rejected until the policy version and its declared surfaces
are reviewed. A new root-workspace Cargo member remains part of the existing
all-feature/all-target Cargo surface, but changes the reviewed workspace
manifest aggregate and therefore requires an explicit evaluator-contract
update before the gate passes. An
`npm-shrinkwrap.json` is always rejected because npm would give it precedence
over the reviewed package lock.

Go dependencies are not part of this version of FR-RELEASE-006. Adding them
requires an explicit policy version change, scanner and SBOM contracts, and
deterministic fixtures. The SBOM embedded in a Python wheel is also an
artifact-owned contract: this gate does not replace or strengthen the wheel
validator merely by generating a repository dependency inventory.

## Blocking findings

The gate normalizes and blocks all of the following unless one exact, active
exception applies:

- known vulnerabilities at `critical`, `high`, `moderate`, `low`, `info`, or
  unknown severity;
- RustSec `unmaintained`, `unsound`, and `notice` advisories;
- yanked Cargo packages;
- unknown or disallowed licenses;
- disallowed package sources; and
- dependencies explicitly banned by repository policy.

In version 1, license, source, and ban enforcement is Cargo-only through
`cargo-deny`; `THIRD_PARTY_NOTICES.md` is also a Cargo-owned artifact. The npm
and PyPI/uv projects receive advisory scanning plus exact lockfile/SBOM parity,
but this version does not promise cross-ecosystem license, source, or ban
enforcement. Adding that enforcement requires a policy-version change and
ecosystem-specific fixtures.

An unavailable advisory service, stale or missing required database, scanner
failure, unknown report schema, malformed report, incomplete dependency graph,
or failed SBOM/license verification is an operational failure and blocks the
gate. It must not be translated into an empty report or successful audit.

Finding identifiers are canonicalized deterministically:

- Cargo findings use the `RUSTSEC-*` identifier when one exists.
- npm findings use the `GHSA-*` identifier from the advisory URL, falling back
  to npm's source identifier only when no GHSA exists.
- Python findings prefer a `GHSA-*` alias, then the `PYSEC-*` identifier, then
  a `CVE-*` alias.

Reports from overlapping Rust scanners are coalesced only when their complete
normalized identities agree. A finding's identity is its ecosystem, project,
lockfile, kind, canonical identifier, package name, and package version.

## Reviewed exceptions

Exceptions are records in `dependency-policy.json`; scanner-native ignore
lists are not an alternate exception mechanism. Every record must include:

- a repository-unique exception ID;
- ecosystem, project ID, and exact lockfile path;
- for Cargo records only, an exact lowercase `cargo_graph_sha256` binding the
  reviewed exception to the authenticated Cargo graph context;
- finding kind and canonical identifier;
- exact package name and version;
- scanner severity, including the explicit `unknown` value when applicable;
- dependency scopes (`runtime`, `build`, `development`, or `optional`) and an
  explicit development-only boolean;
- every affected shipped or CI surface;
- reachability as `reachable`, `not_reachable`, `unknown`, or
  `not_applicable`;
- concrete reachability or applicability evidence;
- one component owner from `rust-runtime`, `release-engineering`,
  `python-bindings`, `typescript-bindings`, `documentation`, or `tooling`;
- a `https://github.com/plx/ferric-rules/issues/<number>` tracking issue;
- risk rationale and a remediation plan; and
- `issued_on` and `expires_on` UTC dates.

An exception matches only the complete normalized identity. Package-wide,
ecosystem-wide, version-range, severity-threshold, and wildcard exceptions are
invalid. Duplicate or ambiguous records fail. An exception that matches no
current finding also fails so that obsolete records, misspelled identifiers,
and remediated findings are removed rather than becoming a future bypass.
For npm and PyPI findings, dependency scopes and the development-only flag are
derived from the authenticated project manifest and lock graph and must match
the reviewed exception exactly. npm's `devOptional` lock flag represents
overlapping development and non-development optional reachability, so it
contributes development, optional, runtime, and (when configured) build scope
and is not development-only. For a project whose reviewed default scopes
include `build`, declaring any `prebuild`, `build`, or `postbuild` lifecycle
conservatively seeds every direct `devDependency` into build scope and
propagates that scope through exact locked dependency, optional-dependency, and
peer-dependency edges. This is based on authenticated configuration and Node's
nearest-install-path resolution, not shell-token or package-name inference.
Scopes are unioned across every installed path for the same npm name and
version, and any build-reachable path is not development-only. The uv
classifier starts at the sole editable root, propagates runtime, every
development group, and optional roots through exact locked dependency edges,
unions transitive scopes, and rejects ambiguous or unreachable registry
identities. On reviewed build surfaces it also authenticates
each `[build-system].requires` entry against the sole same-name, exact canonical
name/specifier/absent-marker row in the uv root's `metadata.requires-dev`.
The explicit PEP 517 binding v1 accepts only marker-free build requirements
with a nonempty comma-conjoined specifier made from `==`, `!=`, `<`, `<=`, `>`,
or `>=` comparisons against dot-separated nonnegative numeric releases; each
segment is `0` or has no leading zero. Trailing zero release segments compare
equally. Epochs, pre/post/dev/local
versions, wildcards, compatible and arbitrary equality, requirement markers,
and other constructs are unsupported and fail closed rather than receiving
partial PEP 440 interpretation. Every same-name edge in the authenticated root
development group must also be marker-free, resolve unambiguously to the exact
`https://pypi.org/simple` registry source, and have a locked version satisfying
the complete specifier. All passing variants seed build scope, which then
propagates transitively.

The build-system table is shape-validated on non-build surfaces too, but it
does not invent build reachability outside the project's reviewed default
scopes. In v1, `node-package` and `python-tools` are reviewed non-build
surfaces: their lifecycle or isolated-backend declarations are manifest-hash
bound (and structurally checked where applicable) but do not seed build scope.
This inventory remains lockfile-scoped and does not claim to inventory every
tool that an unconstrained build-isolation environment could install.

Each Cargo exception has a required `cargo_graph_sha256`; npm and PyPI
exceptions may not carry that field. The evaluator derives the digest as
SHA-256 over its canonical JSON encoding (sorted keys, two-space indentation,
UTF-8, and one trailing newline). The object has schema
`ferric.cargo-exception-graph-context`, version 1, and exactly these context
values: `project_id`, `lockfile`, `lockfile_sha256`,
`workspace_manifests_sha256`, `cargo_config_sha256`, `deny_config_sha256`,
`dependency_groups`, `targets`, and `features`. The current scope values are
the reviewed `all`/`all`/`all` contract. Repository derivation reads the safe
lexical `Cargo.lock`, validates and hashes the sole `.cargo/config.toml` and
`deny.toml`, and uses the pinned sorted aggregate of every safely expanded
workspace `Cargo.toml`.

The authenticated observed digest is attached to every normalized Cargo
finding in the final report. It must exactly equal the exception's digest
before reviewed scopes, the development-only flag, or affected surfaces are
copied. Context-free evaluation therefore blocks the finding and leaves the
exception unused. Any lock graph, workspace manifest or membership, Cargo
patch configuration, cargo-deny graph configuration, project identity, path,
or reviewed scope drift invalidates the old exception and requires explicit
review of a replacement digest.

Dates use strict `YYYY-MM-DD` UTC calendar dates. `expires_on` is the first
invalid day: an exception is active only when
`issued_on <= current_utc_date < expires_on`. It is expired at 00:00 UTC on the
listed expiry date. Future issue dates, invalid dates, and non-increasing date
ranges fail. The schema does not impose an arbitrary maximum duration, but an
initial or renewed expiry must be bounded and justified in review.

The component owner is an accountability category, not an invented person.
Approval is the reviewed change that lands the exception on the default branch;
an `approved: true` field cannot substitute for review. Current remediation is
tracked through the existing release, runtime, TypeScript/Node, and Python
issues, including [#151](https://github.com/plx/ferric-rules/issues/151),
[#215](https://github.com/plx/ferric-rules/issues/215),
[#219](https://github.com/plx/ferric-rules/issues/219), and
[#220](https://github.com/plx/ferric-rules/issues/220). Renewal is a new policy
diff and receives the same review as an initial exception.

## Pinned scanners and local commands

CI asserts these exact tool versions before it trusts their output:

| Tool | Version |
| --- | --- |
| Rust toolchain | `1.93.0` |
| `cargo-audit` | `0.22.2` |
| `cargo-deny` | `0.20.2` |
| `cargo-cyclonedx` | `0.5.9` |
| `cargo-about` | `0.9.0` |
| `uv` | `0.11.16` |
| `pip-audit` | `2.10.1` |
| Node.js | `22.18.0` |
| npm | `11.12.1` |
| `@cyclonedx/cyclonedx-npm` | `4.2.1` |

Run the complete policy through the repository entry point rather than treating
a raw scanner exit code as the final verdict:

```sh
just dependency-policy
```

The orchestrator retains the raw reports, then applies the reviewed exceptions
and verifies the generated evidence. Its scanner inputs are equivalent to:

```sh
cargo-audit audit --file Cargo.lock --format json
cargo-deny --locked --all-features --format json \
  check advisories bans licenses sources

# Run once in each declared npm project, without omitting any group.
npm audit --package-lock-only --json --audit-level=info \
  --registry=https://registry.npmjs.org/ \
  --include=dev --include=optional --include=peer

# Export each complete uv graph. The orchestrator derives one or more
# marker-free, fully pinned and hashed REQS audit batches from it.
uv --no-config --no-cache export \
  --locked --all-groups --all-extras --no-emit-project \
  --format requirements.txt
uvx --isolated --no-env-file --no-config --no-cache \
  --default-index https://pypi.org/simple \
  --from pip-audit==2.10.1 pip-audit \
  --strict --no-deps --require-hashes --disable-pip \
  --vulnerability-service pypi --format json \
  -r "$MARKER_FREE_REQUIREMENTS_BATCH"

./scripts/license-notices.sh check
```

The corresponding SBOM commands are pinned just as strictly:

```sh
SOURCE_DATE_EPOCH="$CANDIDATE_COMMIT_TIME" \
  cargo-cyclonedx cyclonedx --format json --spec-version 1.5 --all-features \
  --target all --describe all-cargo-targets --all

# Run once in each npm project. The orchestrator substitutes absolute output
# and manifest paths for the two placeholders.
cyclonedx-npm --package-lock-only --spec-version 1.5 \
  --output-format JSON --output-reproducible --flatten-components \
  --output-file "<output>" "<manifest>"

# Run once in each uv project and capture stdout.
uv --no-config --no-cache --preview-features sbom-export export --locked --all-groups \
  --all-extras --no-emit-project --format cyclonedx1.5
```

The Cargo plugin executables are invoked directly so repository or ambient
Cargo aliases cannot replace them. npm runs with empty isolated user/global
configuration and cache paths, and uv/pip-audit run with inherited `UV_*` and
`PIP_AUDIT_*` settings cleared. CI acquires the pinned Rust and npm tools from
a runner-temporary working directory into isolated install roots before the
candidate repository is scanned.

The sole repository Cargo configuration is lexical root `.cargo/config.toml`.
Its closed `[patch.crates-io]` table and SHA-256 are bound by the evaluator and
scan manifest, and its five path entries must resolve to the reviewed in-repo
core, FFI-macros, parser, pinned, and runtime crate manifests. Legacy, nested,
symlinked, additional, or path-escaping Cargo configurations fail.

`cargo-deny` 0.20.2 does not accept the word `all` as a target triple. The
empty `[graph].targets` list in `deny.toml` is its supported all-target graph;
the scan manifest records `target_scope: all`. Supplying a host triple instead
would narrow the graph and violate this policy.

Likewise, `pip-audit` evaluates requirement markers for its current Python
environment. A single host-filtered run is insufficient. The orchestrator
extracts every registry package name/version represented in each full
`uv.lock`, audits marker-free pinned and hashed batches, and proves that the
union of audited identities equals the locked inventory. A clean report that
omits a Windows-only or older-interpreter variant fails coverage.

The network-backed advisory result is intentionally time-sensitive. A tool
upgrade, report-schema change, or advisory-database change must be reviewed as
new evidence; copying a historical clean report into a later candidate does
not satisfy the gate. `uv`'s CycloneDX export is an upstream preview feature,
and its 0.11.16 output contains a random serial number, a wall-clock timestamp,
and no artifact checksums. The orchestrator therefore removes the two volatile
optional fields, canonicalizes the JSON, and enriches an absent PyPI checksum
set from the exact registry artifacts in `uv.lock`. A checksum emitted by the
tool that conflicts with the lockfile fails rather than being overwritten.
Both the preview feature and the accepted output shape are pinned and tested.

For deterministic fixture evaluation, the standard-library evaluator accepts
an explicit date and already captured reports:

```sh
python3 scripts/dependency-policy.py validate \
  --policy dependency-policy.json --today YYYY-MM-DD

python3 scripts/dependency-policy.py evaluate \
  --policy dependency-policy.json \
  --reports-dir <raw-report-directory> \
  --sbom-dir <sbom-directory> \
  --candidate-sha <40-hex-commit> \
  --today YYYY-MM-DD \
  --output-dir <evidence-directory>
```

Production CI derives the date with `date -u +%F`; it must not pass a
policy-controlled or historical date to keep an exception alive.

## SBOM and evidence contract

The gate generates a reproducible CycloneDX 1.5 JSON inventory for each of the
seven logical projects. It verifies that:

1. every component in the complete locked inventory is present with the exact
   name and version, including Cargo development, build, optional, and
   target-only entries;
2. checksums agree whenever the lockfile supplies one;
3. no component is absent from the corresponding lockfile;
4. component identities are unambiguous for Cargo and PyPI, while npm
   preserves the path-derived multiplicity of repeated name/version installs
   with a nonempty, document-unique `bom-ref` for each occurrence; and
5. the evidence manifest binds the SBOM SHA-256, lockfile SHA-256, and candidate
   commit.

An npm `bom-ref` is an opaque CycloneDX document identifier; version 1 does not
invent or require a particular path encoding inside it. Lockfile install paths
distinguish expected occurrences, and the verifier checks occurrence counts,
checksums, and globally unique references. Nested npm components are traversed
defensively even though the locked generator is invoked with
`--flatten-components`. Scoped npm identities are reconstructed from the
generator's separate CycloneDX `group` and `name` fields. npm integrity hashes
emitted on `distribution` external references are compared exactly with the
lockfile; conflicting top-level and distribution hashes fail closed. The seven
native optional entries in the published Node package lock retain explicit
`0.1.0` versions so npm 11 and the pinned generator see the same complete
inventory. For PyPI, `metadata.component` is the document subject, not a
registry dependency, so it is excluded from uv lockfile parity.

The generated SBOMs are CI evidence, not checked-in substitutes for the
lockfiles. The existing Rust `THIRD_PARTY_NOTICES.md` remains checked in and
must exactly match the locked all-feature Cargo graph through
`./scripts/license-notices.sh check`. The evaluator closes the `about.toml`
schema, pins the notice template and generator script bytes, and binds their
SHA-256 values plus the Cargo lock SHA-256 into both license evidence and the
scan manifest. The sole permanent Cargo license carve-out is MPL-2.0 for exact
`cbindgen@0.28.0`; its exact workspace declaration, lock version, cargo-deny
rule, and cargo-about table must remain aligned.

`cargo-cyclonedx` 0.5.9 intentionally excludes development dependencies even
when it is run for every workspace member and Cargo target. With
`--describe all-cargo-targets`, each generated document also names its Rust
target in `metadata.component`; that target name is evidence about the
document subject, not a Cargo package identity. The Rust SBOM manifest
therefore labels every raw tool member as `cargo-cyclonedx` and retains those
documents unchanged apart from canonical normalization, while ignoring their
target subject during lock parity. It also names exactly one deterministic
`cargo-lock-union` CycloneDX member derived from all `Cargo.lock` package
name/version/checksum records. The evaluator recomputes that member byte-for-
byte, requires at least one tool member, rejects any tool dependency absent
from the lock, and verifies the combined inventory against the complete lock.
This explicit supplement preserves development and target-only coverage
without claiming that the pinned plugin emitted dependencies it filters.

The normalized JSON report uses schema `ferric.dependency-policy-report`
version 1 and records the candidate, UTC evaluation date, exact tool versions,
input manifest and lockfile hashes, every normalized finding and exception disposition,
verified SBOM hashes, license-notice result, operational errors, and final
verdict. Each input record contains `project_id`, `ecosystem`, `manifest`,
`manifest_sha256`, `lockfile`, and lockfile `sha256`. Each error is a closed
object with `code`, `message`, and nullable `project_id`; an empty error list is required for a passing
verdict. A Markdown summary is derived from the JSON report for humans; it is
not a second source of policy truth.

The evidence layout is fixed so aggregate verification does not need
ecosystem-specific path discovery:

```text
dependency-policy-evidence/
  raw/
    scan-manifest.json
    tool-versions.json
    license-notices.json
    rust-workspace.cargo-audit.json
    cargo-deny.json
    node-package.npm-audit.json
    node-addon.npm-audit.json
    documentation.npm-audit.json
    site.npm-audit.json
    python-package.pip-audit.json
    python-tools.pip-audit.json
  sboms/
    rust-workspace.sbom-manifest.json
    rust-workspace/...                 # kind: cargo-cyclonedx
    rust-workspace/cargo-lock-union.cdx.json
    node-package.cdx.json
    node-addon.cdx.json
    documentation.cdx.json
    site.cdx.json
    python-package.cdx.json
    python-tools.cdx.json
  dependency-policy-report.json
  dependency-policy-report.md
  dependency-policy.json
  deny.toml
```

The scan manifest enumerates the exact raw files, argv arrays, working
directories, scanner exit classifications, file digests, manifest and lockfile digests,
tool versions, candidate, evaluation date, commit-derived
`SOURCE_DATE_EPOCH`, and all-target Cargo scope. It also binds the evaluated
policy, `deny.toml`, exact root Cargo configuration hash, and reviewed
workspace Cargo-manifest aggregate, plus the license configuration, template,
generator, and Cargo lock hashes. Missing,
extra, unreferenced, symlinked, or
non-regular raw/SBOM entries fail verification. Cargo may emit several crate
SBOM files, but the Rust manifest names and hashes every member and presents
their verified union as the single `rust-workspace` inventory.

## CI contract and ownership boundaries

The path-unfiltered `Dependency Policy` workflow runs for pushes, pull
requests, and manual dispatches against the immutable event candidate. Its
single stable `Dependency Policy` check fails if any scanner, policy, expiry,
license, or SBOM condition fails. It always uploads
`dependency-policy-evidence-${{ env.CANDIDATE_SHA }}` for 30 days, so the
artifact name and its contents both identify the checked-out pull-request head
rather than GitHub's synthetic merge SHA. A completed run includes raw and
normalized reports, the evaluated policy and deny configuration, tool
versions, candidate and lockfile hashes, SBOMs, and license evidence. The
workflow initializes an explicit failing report before installing tools, so an
earlier operational failure still produces an honest partial artifact rather
than a misleading clean or missing result; a missing bundle is itself a
failure.

The job has a 45-minute timeout. Tool acquisition occurs outside the candidate
checkout with fresh Cargo and npm install roots, canonical registries, and
isolated configuration before exact versions are asserted and the aggregate
gate begins.

FR-RELEASE-006 owns this policy, evaluator, evidence, and stable check context.
It does not silently upgrade PyO3, npm, or other binding-owned dependencies.
Those remediations remain with the relevant component owners. The bincode
format and migration decision remains with
[FR-RELEASE-005 (#151)](https://github.com/plx/ferric-rules/issues/151), and
formal required-status/ruleset wiring remains with
[FR-RELEASE-004 (#150)](https://github.com/plx/ferric-rules/issues/150).

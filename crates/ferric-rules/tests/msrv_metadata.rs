use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const EXPECTED_MSRV: &str = "1.75";
const EXPECTED_VERSION: &str = "0.1.0";
const EXPECTED_LICENSE: &str = "MIT OR Apache-2.0";
const EXPECTED_REPOSITORY: &str = "https://github.com/plx/ferric-rules";
const PUBLISHABLE_PACKAGES: [&str; 8] = [
    "ferric-rules",
    "ferric-rules-cli",
    "ferric-rules-core",
    "ferric-rules-ffi",
    "ferric-rules-ffi-macros",
    "ferric-rules-parser",
    "ferric-rules-pinned",
    "ferric-rules-runtime",
];
const PRIVATE_PACKAGES: [&str; 3] = [
    "ferric-rules-bench-gen",
    "ferric-rules-napi",
    "ferric-rules-python",
];

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    dependencies: Vec<Dependency>,
    id: String,
    license: Option<String>,
    manifest_path: PathBuf,
    name: String,
    publish: Option<Vec<String>>,
    readme: Option<PathBuf>,
    repository: Option<String>,
    rust_version: Option<String>,
    version: String,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    req: String,
}

impl Package {
    fn is_publishable(&self) -> bool {
        self.publish
            .as_ref()
            .map_or(true, |registries| !registries.is_empty())
    }
}

fn workspace_metadata() -> Metadata {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(workspace_root)
        .output()
        .expect("cargo metadata should run");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("cargo metadata should emit valid JSON")
}

#[test]
fn publishable_workspace_members_share_the_declared_msrv() {
    assert_eq!(env!("CARGO_PKG_RUST_VERSION"), EXPECTED_MSRV);

    let metadata = workspace_metadata();
    let workspace_members: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let publishable: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|package| workspace_members.contains(package.id.as_str()))
        .filter(|package| package.is_publishable())
        .collect();

    assert!(
        !publishable.is_empty(),
        "workspace should contain publishable packages"
    );

    let mismatches: Vec<String> = publishable
        .iter()
        .filter(|package| package.rust_version.as_deref() != Some(EXPECTED_MSRV))
        .map(|package| {
            format!(
                "{} declares {:?}, expected {EXPECTED_MSRV}",
                package.name, package.rust_version
            )
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "publishable package MSRV mismatch:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn ferric_rules_packages_share_the_registry_contract() {
    let metadata = workspace_metadata();
    let workspace_members: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let packages: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|package| workspace_members.contains(package.id.as_str()))
        .filter(|package| package.name.starts_with("ferric-rules"))
        .collect();

    let actual_names: BTreeSet<&str> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    let is_repository_workspace = packages.len() > 1;
    if is_repository_workspace {
        let expected_names: BTreeSet<&str> = PUBLISHABLE_PACKAGES
            .iter()
            .chain(PRIVATE_PACKAGES.iter())
            .copied()
            .collect();
        assert_eq!(actual_names, expected_names, "unexpected Rust package set");

        let actual_publishable: BTreeSet<&str> = packages
            .iter()
            .filter(|package| package.is_publishable())
            .map(|package| package.name.as_str())
            .collect();
        let expected_publishable: BTreeSet<&str> = PUBLISHABLE_PACKAGES.iter().copied().collect();
        assert_eq!(
            actual_publishable, expected_publishable,
            "publishable Rust package set changed"
        );
    } else {
        let expected_names = BTreeSet::from(["ferric-rules"]);
        assert_eq!(
            actual_names, expected_names,
            "the packaged facade workspace should contain only the facade"
        );
    }

    for package in packages {
        if is_repository_workspace {
            let manifest_parent = package
                .manifest_path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str());
            assert_eq!(
                manifest_parent,
                Some(package.name.as_str()),
                "{} directory must match its Cargo package name",
                package.name
            );
        }
        assert_eq!(
            package.version, EXPECTED_VERSION,
            "{} version is not synchronized",
            package.name
        );

        if package.is_publishable() {
            assert_eq!(
                package.license.as_deref(),
                Some(EXPECTED_LICENSE),
                "{} license metadata differs",
                package.name
            );
            assert_eq!(
                package.repository.as_deref(),
                Some(EXPECTED_REPOSITORY),
                "{} repository metadata differs",
                package.name
            );
            assert!(
                package.readme.is_some(),
                "{} must declare a README",
                package.name
            );
        }

        for dependency in package
            .dependencies
            .iter()
            .filter(|dependency| dependency.name.starts_with("ferric-rules"))
        {
            assert_eq!(
                dependency.req,
                format!("={EXPECTED_VERSION}"),
                "{} -> {} must use the synchronized exact registry version",
                package.name,
                dependency.name
            );
        }
    }
}

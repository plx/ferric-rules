use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

const EXPECTED_MSRV: &str = "1.75";

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    publish: Option<Vec<String>>,
    rust_version: Option<String>,
}

impl Package {
    fn is_publishable(&self) -> bool {
        self.publish
            .as_ref()
            .map_or(true, |registries| !registries.is_empty())
    }
}

#[test]
fn publishable_workspace_members_share_the_declared_msrv() {
    assert_eq!(env!("CARGO_PKG_RUST_VERSION"), EXPECTED_MSRV);

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

    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).expect("cargo metadata should emit valid JSON");
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

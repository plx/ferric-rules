use std::path::Path;

fn normalize_line_endings(contents: &str) -> String {
    contents.replace("\r\n", "\n").replace('\r', "\n")
}

fn workflow_job<'a>(workflow: &'a str, id: &str) -> &'a str {
    let marker = format!("\n  {id}:\n");
    assert_eq!(
        workflow.matches(&marker).count(),
        1,
        "CI workflow must define exactly one dedicated `{id}` job"
    );
    let start = workflow
        .find(&marker)
        .unwrap_or_else(|| panic!("CI workflow must define a dedicated `{id}` job"));
    let tail = &workflow[start + marker.len()..];
    let end = tail
        .match_indices('\n')
        .find_map(|(newline, _)| {
            let next_line = &tail[newline + 1..];
            let remainder = next_line.strip_prefix("  ")?;
            (!remainder.starts_with(char::is_whitespace)).then_some(newline)
        })
        .unwrap_or(tail.len());
    &tail[..end]
}

fn just_recipe<'a>(justfile: &'a str, name: &str) -> &'a str {
    let marker = format!("\n{name}:\n");
    let start = justfile
        .find(&marker)
        .unwrap_or_else(|| panic!("justfile must define the `{name}` recipe"));
    let tail = &justfile[start + marker.len()..];
    let mut end = 0;
    for line in tail.split_inclusive('\n') {
        if !line.trim().is_empty() && !line.starts_with(char::is_whitespace) {
            break;
        }
        end += line.len();
    }
    tail[..end].trim_end()
}

#[test]
fn required_tracing_job_runs_the_repository_gate_unconditionally() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workflow = normalize_line_endings(
        &std::fs::read_to_string(workspace_root.join(".github/workflows/ci.yml"))
            .expect("CI workflow should be readable"),
    );
    let job = workflow_job(&workflow, "tracing");

    for required in [
        "    name: Tracing Feature\n",
        "    runs-on: ubuntu-latest\n",
        "    timeout-minutes: 30\n",
        "      - uses: actions/checkout@v4\n",
        "      - uses: taiki-e/install-action@just\n",
        "      - uses: dtolnay/rust-toolchain@stable\n",
        "          toolchain: ${{ env.RUST_TOOLCHAIN }}\n",
        "          components: clippy\n",
        "      - run: just check-tracing\n",
    ] {
        assert!(
            job.contains(required),
            "tracing CI job is missing `{}`",
            required.trim()
        );
    }

    for forbidden in [
        "\n    needs:",
        "\n    strategy:",
        "\n      matrix:",
        "continue-on-error:",
    ] {
        assert!(
            !job.contains(forbidden),
            "tracing CI job must be unconditional; found `{}`",
            forbidden.trim()
        );
    }
    assert!(
        job.lines().all(|line| {
            let line = line.trim_start();
            !line.starts_with("if:") && !line.starts_with("- if:")
        }),
        "tracing CI job must not condition either the job or one of its steps"
    );
    for path_filter in ["\n    paths:", "\n    paths-ignore:"] {
        assert!(
            !workflow.contains(path_filter),
            "CI workflow path filters must not skip the required tracing job"
        );
    }
}

#[test]
fn tracing_gate_covers_the_locked_full_workspace_configuration() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let justfile = normalize_line_endings(
        &std::fs::read_to_string(workspace_root.join("justfile"))
            .expect("justfile should be readable"),
    );
    let expected_recipe = r"    cargo check --workspace --features tracing --locked
    cargo clippy --workspace --all-targets --features tracing --locked -- -D warnings
    cargo test --workspace --features tracing --locked";
    let recipe = just_recipe(&justfile, "check-tracing");

    assert_eq!(
        recipe, expected_recipe,
        "check-tracing must contain exactly the locked full-workspace check, clippy, and test commands"
    );
    assert!(
        !recipe.contains("--exclude"),
        "check-tracing must not exclude any workspace package"
    );
}

#[test]
fn contract_parsers_accept_windows_line_endings() {
    let workflow = normalize_line_endings(
        "name: CI\r\n\r\njobs:\r\n  tracing:\r\n    name: Tracing Feature\r\n  next:\r\n    name: Next\r\n",
    );
    assert_eq!(
        workflow_job(&workflow, "tracing"),
        "    name: Tracing Feature"
    );

    let justfile = normalize_line_endings(
        "set shell := [\"bash\", \"-uc\"]\r\n\r\ncheck-tracing:\r\n    cargo check --workspace --features tracing --locked\r\n\r\nnext:\r\n    true\r\n",
    );
    assert_eq!(
        just_recipe(&justfile, "check-tracing"),
        "    cargo check --workspace --features tracing --locked"
    );
}

//! Verifies that fenced code blocks in `docs/users-guide.md` are still present
//! in the corresponding example project files.
//!
//! How it works
//! ------------
//!
//! Each fenced code block in the guide may be tagged with an HTML comment
//! immediately preceding it:
//!
//! ````text
//! <!-- example: 01-minimal-embedding/src/main.rs -->
//! ```rust
//! fn main() { /* ... */ }
//! ```
//! ````
//!
//! The marker is interpreted as a path relative to `examples/users-guide/`.
//! The block's content is normalized by trimming each line and dropping blank
//! lines; the same normalized form must appear as a substring in the named file.
//!
//! Untagged blocks are ignored, so explanatory snippets without a backing
//! example don't break the build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const GUIDE_PATH: &str = "docs/users-guide.md";
const EXAMPLES_ROOT: &str = "examples/users-guide";

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at tools/users-guide-sync/; pop two levels.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| PathBuf::from(manifest_dir), Path::to_path_buf)
}

#[derive(Debug)]
struct Block {
    /// Absolute path to the example file the block must appear in.
    target: PathBuf,
    /// Display path (relative to examples root) for diagnostics.
    target_display: String,
    /// Fence info string (e.g. "rust", "clips", "toml"). Used in messages.
    info: String,
    /// Normalized block content the file must contain.
    content: String,
    /// Line in the guide where the block opened.
    guide_line: usize,
}

struct PendingBlock {
    info: String,
    marker: String,
    body: String,
    open_line: usize,
}

fn parse_blocks(guide_src: &str, examples_root: &Path) -> anyhow::Result<Vec<Block>> {
    let mut blocks = Vec::new();
    let mut pending_marker: Option<(String, usize)> = None;
    let mut in_block: Option<PendingBlock> = None;

    for (idx, line) in guide_src.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim_start();

        if in_block.is_some() {
            let is_fence_close =
                trimmed.starts_with("```") && trimmed.trim_end_matches('`').is_empty();
            if is_fence_close {
                let pb = in_block.take().expect("checked above");
                blocks.push(Block {
                    target: examples_root.join(&pb.marker),
                    target_display: pb.marker,
                    info: pb.info,
                    content: normalize(&pb.body),
                    guide_line: pb.open_line,
                });
            } else {
                let pb = in_block.as_mut().expect("checked above");
                pb.body.push_str(line);
                pb.body.push('\n');
            }
            continue;
        }

        // Track marker comments. Format:
        //   <!-- example: <relative path> -->
        if let Some(rest) = trimmed.strip_prefix("<!-- example:") {
            if let Some(path_part) = rest.strip_suffix("-->") {
                pending_marker = Some((path_part.trim().to_string(), line_no));
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("```") {
            if let Some((marker, _marker_line)) = pending_marker.take() {
                in_block = Some(PendingBlock {
                    info: rest.trim().to_string(),
                    marker,
                    body: String::new(),
                    open_line: line_no,
                });
            }
            continue;
        }

        // Lose the marker if there's intervening prose.
        if !trimmed.is_empty() && pending_marker.is_some() {
            pending_marker = None;
        }
    }

    if let Some(pb) = in_block {
        anyhow::bail!(
            "unterminated code block in {GUIDE_PATH} at line {} (marker {})",
            pb.open_line,
            pb.marker
        );
    }

    Ok(blocks)
}

/// Normalize a code block for comparison.
///
/// The goal is for a snippet to match its example regardless of indent or
/// extra blank lines. We do not want every cosmetic difference (e.g. wrapping
/// the snippet inside a `fn main()` in the example) to fail the check.
///
/// Steps:
/// 1. Trim each line on both sides.
/// 2. Drop blank lines outright.
/// 3. Re-join with single newlines.
fn normalize(input: &str) -> String {
    let mut out = String::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

fn check_block(block: &Block) -> anyhow::Result<()> {
    if !block.target.is_file() {
        anyhow::bail!(
            "{GUIDE_PATH}:{} — block points at {} but the file does not exist",
            block.guide_line,
            block.target_display
        );
    }
    let body = fs::read_to_string(&block.target)?;
    let normalized_body = normalize(&body);

    // Substring match on normalized text. Drop the very last newline of the
    // block so the file's body doesn't need to end exactly with the snippet.
    let needle = block.content.trim_end_matches('\n');
    if !normalized_body.contains(needle) {
        let preview = block.content.lines().take(4).collect::<Vec<_>>().join("\n");
        anyhow::bail!(
            "{GUIDE_PATH}:{} — `{}` block does not appear in {} (normalized).\n\
             First lines of the block:\n{}\n\
             Tip: keep the example and the guide in lockstep, or remove the\n\
             `<!-- example: ... -->` marker from this block.",
            block.guide_line,
            block.info,
            block.target_display,
            preview,
        );
    }
    Ok(())
}

fn run_check() -> anyhow::Result<()> {
    let root = workspace_root();
    let guide_path = root.join(GUIDE_PATH);
    let examples_root = root.join(EXAMPLES_ROOT);

    let guide_src = fs::read_to_string(&guide_path)
        .map_err(|e| anyhow::anyhow!("could not read guide at {}: {e}", guide_path.display()))?;

    let blocks = parse_blocks(&guide_src, &examples_root)?;
    if blocks.is_empty() {
        anyhow::bail!(
            "no annotated code blocks found in {GUIDE_PATH}. Did the markers go missing?"
        );
    }

    let mut errors = Vec::new();
    for block in &blocks {
        if let Err(e) = check_block(block) {
            errors.push(format!("{e}"));
        }
    }

    if !errors.is_empty() {
        for err in &errors {
            eprintln!("{err}\n");
        }
        anyhow::bail!(
            "{} of {} annotated block(s) failed verification",
            errors.len(),
            blocks.len()
        );
    }

    println!(
        "users-guide-sync: {} annotated block(s) match their example file(s).",
        blocks.len()
    );
    Ok(())
}

fn print_usage() {
    eprintln!("Usage: users-guide-sync <check>");
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    match cmd.as_deref() {
        Some("check") => match run_check() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_usage();
            ExitCode::FAILURE
        }
        None => {
            print_usage();
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_per_line_whitespace_and_blank_lines() {
        let input = "    fn main() {\n\n        let x = 1;\n    }\n";
        let got = normalize(input);
        assert_eq!(got, "fn main() {\nlet x = 1;\n}\n");
    }

    #[test]
    fn normalize_handles_indentation_at_use_site() {
        // Guide-side block with 0 indent, example with 4-space indent
        // inside a function body — both should normalize identically.
        let guide = "if x {\n    y;\n}\n";
        let example = "fn main() {\n    if x {\n        y;\n    }\n}\n";
        let n_guide = normalize(guide);
        let n_example = normalize(example);
        assert!(n_example.contains(n_guide.trim_end_matches('\n')));
    }

    #[test]
    fn parse_finds_marked_block() {
        let src = "blah\n\
             <!-- example: 01-minimal-embedding/src/main.rs -->\n\
             ```rust\n\
             fn main() {}\n\
             ```\n";
        let here = PathBuf::from("/tmp/examples-root");
        let blocks = parse_blocks(src, &here).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].info, "rust");
        assert_eq!(blocks[0].target_display, "01-minimal-embedding/src/main.rs");
        assert_eq!(blocks[0].content, "fn main() {}\n");
    }

    #[test]
    fn parse_skips_unmarked_block() {
        let src = "```rust\nfn main() {}\n```\n";
        let here = PathBuf::from("/tmp/examples-root");
        let blocks = parse_blocks(src, &here).unwrap();
        assert!(blocks.is_empty());
    }

    #[test]
    fn marker_drops_when_followed_by_prose_before_fence() {
        let src = "<!-- example: foo/bar.rs -->\n\
             some words intervene\n\
             ```rust\nfn x() {}\n```\n";
        let here = PathBuf::from("/tmp/examples-root");
        let blocks = parse_blocks(src, &here).unwrap();
        assert!(blocks.is_empty(), "marker should not bind across prose");
    }
}

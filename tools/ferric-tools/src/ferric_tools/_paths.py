"""Repository path detection and standard directory helpers."""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path


@lru_cache(maxsize=1)
def repo_root() -> Path:
    """Walk up from this file to find the repository root (contains Cargo.toml)."""
    here = Path(__file__).resolve().parent
    for ancestor in [here, *here.parents]:
        if (ancestor / "Cargo.toml").exists():
            return ancestor
    # Fallback: tools/ferric-tools/src/ferric_tools -> 4 levels up
    return here.parent.parent.parent.parent


def examples_dir() -> Path:
    """Return the path to tests/examples/."""
    return repo_root() / "tests" / "examples"


def compat_manifest_path() -> Path:
    """Return the default compat-manifest.json path."""
    return examples_dir() / "compat-manifest.json"


def target_dir() -> Path:
    """Return the path to the target/ directory."""
    return repo_root() / "target"


def ferric_bin(release: bool = True) -> Path:
    """Return the path to the ferric CLI binary."""
    profile = "release" if release else "debug"
    return target_dir() / profile / "ferric"


def harness_script() -> Path:
    """Return the path to the clips-reference.sh script."""
    return repo_root() / "scripts" / "clips-reference.sh"

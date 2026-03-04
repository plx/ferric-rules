"""Tests for ferric_tools._paths.

Verifies that repo_root() locates the repository root by walking up the
directory tree until it finds a Cargo.toml.  The other helpers are thin
path-construction wrappers whose correctness follows from repo_root(), so
we do not duplicate that coverage here.
"""

from __future__ import annotations

from ferric_tools._paths import repo_root


def test_repo_root_contains_cargo_toml():
    # The resolved root must be a directory that contains a Cargo.toml.
    # This is the only contract repo_root() makes: it is the Rust workspace root.
    root = repo_root()

    assert root.is_dir(), f"repo_root() must return an existing directory, got: {root}"
    assert (root / "Cargo.toml").exists(), (
        f"repo_root() must point to a directory containing Cargo.toml, got: {root}"
    )


def test_repo_root_returns_path_object():
    # Callers depend on repo_root() returning a pathlib.Path so they can use
    # the / operator to build sub-paths.
    from pathlib import Path

    assert isinstance(repo_root(), Path)


def test_repo_root_is_stable():
    # repo_root() is decorated with @lru_cache; repeated calls must return
    # the identical object (not just an equal one) to confirm caching works.
    first = repo_root()
    second = repo_root()

    assert first is second, "repo_root() must return the cached result on repeated calls"

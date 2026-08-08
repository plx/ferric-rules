#!/usr/bin/env python3
"""Normalize, PEP 517-build, and smoke the exact final Ferric sdist."""

from __future__ import annotations

import argparse
import os
import shutil
import sys
import tempfile
from pathlib import Path

from python_package_lib import (
    CONTRACT_PATH,
    PackageValidationError,
    contract_target,
    execute_wheel_smoke,
    load_contract,
    normalize_sdist,
    package_version,
    path_without_commands,
    run_checked,
    source_built_wheel_platform_tags,
    validate_wheel,
    write_json,
)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--sdist", type=Path, required=True, help="raw Maturin sdist intermediate"
    )
    parser.add_argument(
        "--output", type=Path, required=True, help="normalized final sdist output"
    )
    parser.add_argument(
        "--target", required=True, help="native wheel target ID for the build smoke"
    )
    parser.add_argument(
        "--receipt", type=Path, required=True, help="JSON receipt output"
    )
    parser.add_argument("--cargo", default="cargo", help="Cargo executable")
    parser.add_argument(
        "--uv", default="uv", help="uv executable used for the PEP 517 build"
    )
    parser.add_argument("--contract", type=Path, default=CONTRACT_PATH)
    return parser.parse_args()


def require_executable(command: str, description: str) -> str:
    resolved = shutil.which(command)
    if resolved is None:
        raise PackageValidationError(f"{description} is not available: {command}")
    return resolved


def main() -> None:
    arguments = parse_arguments()
    contract = load_contract(arguments.contract)
    version = package_version(contract)
    target = contract_target(contract, arguments.target)
    source_platform_tags = source_built_wheel_platform_tags(target)
    source_compatibility = "linux" if target["runtime"]["os"] == "linux" else "pypi"
    cargo = require_executable(arguments.cargo, "Cargo")
    uv = require_executable(arguments.uv, "uv")

    final_sdist = normalize_sdist(
        arguments.sdist,
        arguments.output,
        contract,
        version,
        cargo=cargo,
    )

    with tempfile.TemporaryDirectory(prefix="ferric-python-sdist-build-") as temporary:
        work_root = Path(temporary)
        wheels = work_root / "wheels"
        environment = dict(os.environ)
        environment.update(
            {
                "CARGO_BUILD_TARGET": target["rust_target"],
                "CARGO_NET_OFFLINE": "true",
                "CARGO_TARGET_DIR": str((work_root / "cargo-target").resolve()),
                "MATURIN_COMPATIBILITY": source_compatibility,
                "MATURIN_STRIP": "false",
                "PYO3_PYTHON": sys.executable,
                "UV_NO_PROGRESS": "1",
                "UV_PYTHON_DOWNLOADS": "never",
            }
        )
        deployment_target = target["compatibility"].get("deployment_target")
        if deployment_target is not None:
            environment["MACOSX_DEPLOYMENT_TARGET"] = deployment_target
        run_checked(
            [
                uv,
                "build",
                "--wheel",
                "--force-pep517",
                "--no-build-isolation",
                "--no-sources",
                "--no-config",
                "--no-create-gitignore",
                "--offline",
                "--python",
                sys.executable,
                "--out-dir",
                str(wheels),
                str(final_sdist.path.resolve()),
            ],
            cwd=work_root,
            environment=environment,
            context="PEP 517 wheel build from exact normalized sdist",
        )
        built_wheels = sorted(wheels.glob("*.whl"))
        if len(built_wheels) != 1:
            raise PackageValidationError(
                f"exact sdist build produced {len(built_wheels)} wheels, expected one"
            )
        validate_wheel(
            built_wheels[0],
            contract,
            version,
            expected_target_id=arguments.target,
            expected_platform_tags=source_platform_tags,
        )
        rust_free_path = path_without_commands(
            environment.get("PATH"), ("cargo", "rustc")
        )
        wheel_smoke = execute_wheel_smoke(
            built_wheels[0],
            arguments.target,
            contract,
            version,
            rust_free_path=rust_free_path,
            expected_platform_tags=source_platform_tags,
            receipt_kind="source-built-wheel",
        )

    receipt = {
        "schema_version": 1,
        "kind": "sdist-smoke",
        "status": "passed",
        "distribution": contract["distribution"]["name"],
        "version": version,
        "target_id": arguments.target,
        "sdist": {
            "filename": final_sdist.filename,
            "sha256": final_sdist.sha256,
            "safe_archive_paths": True,
            "cargo_lock_locked_offline": True,
            "deterministic_repack": True,
            "pep517_built_from_exact_archive": True,
        },
        "wheel_smoke": wheel_smoke,
    }
    write_json(arguments.receipt, receipt)
    print(
        f"normalized and tested {final_sdist.filename} sha256={final_sdist.sha256}; "
        f"built {wheel_smoke['wheel']['filename']}"
    )


if __name__ == "__main__":
    main()

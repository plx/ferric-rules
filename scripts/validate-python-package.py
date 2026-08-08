#!/usr/bin/env python3
"""Validate Ferric's Python package metadata and binary-release contract."""

from __future__ import annotations

import argparse
from pathlib import Path

from python_package_lib import (
    CONTRACT_PATH,
    load_contract,
    validate_repository_contract,
    validate_wheel,
)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--contract",
        type=Path,
        default=CONTRACT_PATH,
        help="wheel target contract (default: package wheel-targets.json)",
    )
    parser.add_argument("--wheel", type=Path, help="exact built wheel to validate")
    parser.add_argument("--target", help="expected wheel target ID")
    arguments = parser.parse_args()
    if (arguments.wheel is None) != (arguments.target is None):
        parser.error("--wheel and --target must be provided together")
    return arguments


def main() -> None:
    arguments = parse_arguments()
    contract = load_contract(arguments.contract)
    version = validate_repository_contract(contract)
    if arguments.wheel is not None:
        inspection = validate_wheel(
            arguments.wheel,
            contract,
            version,
            expected_target_id=arguments.target,
        )
        print(
            f"validated wheel {arguments.wheel.name}: "
            f"{inspection.target_id}, sha256={inspection.sha256}"
        )
    print(
        f"validated {contract['distribution']['name']}@{version}: "
        f"cp39-abi3, {len(contract['wheels'])} wheels, one tested sdist"
    )


if __name__ == "__main__":
    main()

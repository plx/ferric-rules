#!/usr/bin/env python3
"""Validate Ferric's Python package metadata and binary-release contract."""

from __future__ import annotations

import argparse
from pathlib import Path

from python_package_lib import (
    CONTRACT_PATH,
    load_contract,
    validate_repository_contract,
)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--contract",
        type=Path,
        default=CONTRACT_PATH,
        help="wheel target contract (default: package wheel-targets.json)",
    )
    return parser.parse_args()


def main() -> None:
    arguments = parse_arguments()
    contract = load_contract(arguments.contract)
    version = validate_repository_contract(contract)
    print(
        f"validated {contract['distribution']['name']}@{version}: "
        f"cp39-abi3, {len(contract['wheels'])} wheels, one tested sdist"
    )


if __name__ == "__main__":
    main()

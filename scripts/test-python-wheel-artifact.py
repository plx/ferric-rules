#!/usr/bin/env python3
"""Install and smoke one exact local Ferric wheel in a clean environment."""

from __future__ import annotations

import argparse
from pathlib import Path

from python_package_lib import (
    CONTRACT_PATH,
    execute_wheel_smoke,
    load_contract,
    package_version,
    write_json,
)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--wheel", type=Path, required=True, help="exact local wheel to install"
    )
    parser.add_argument(
        "--target", required=True, help="wheel target ID from wheel-targets.json"
    )
    parser.add_argument(
        "--receipt", type=Path, required=True, help="JSON receipt output"
    )
    parser.add_argument(
        "--expect-python-rejection",
        action="store_true",
        help="require Python 3.14 installation to fail because of Requires-Python",
    )
    parser.add_argument("--contract", type=Path, default=CONTRACT_PATH)
    return parser.parse_args()


def main() -> None:
    arguments = parse_arguments()
    contract = load_contract(arguments.contract)
    version = package_version(contract)
    receipt = execute_wheel_smoke(
        arguments.wheel,
        arguments.target,
        contract,
        version,
        expect_python_rejection=arguments.expect_python_rejection,
    )
    write_json(arguments.receipt, receipt)
    print(
        f"{receipt['kind']} passed for {arguments.target} on "
        f"CPython {receipt['python']['minor']}: {arguments.wheel.name}"
    )


if __name__ == "__main__":
    main()

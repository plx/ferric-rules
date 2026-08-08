#!/usr/bin/env python3
"""Verify the complete Ferric Python release set and write its SHA256 manifest."""

from __future__ import annotations

import argparse
from pathlib import Path

from python_package_lib import (
    CONTRACT_PATH,
    load_contract,
    package_version,
    verify_artifact_set,
    write_json,
)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts-dir", type=Path, required=True)
    parser.add_argument("--receipts-dir", type=Path, required=True)
    parser.add_argument("--manifest-out", type=Path, required=True)
    parser.add_argument("--contract", type=Path, default=CONTRACT_PATH)
    return parser.parse_args()


def main() -> None:
    arguments = parse_arguments()
    contract = load_contract(arguments.contract)
    version = package_version(contract)
    manifest = verify_artifact_set(
        arguments.artifacts_dir,
        arguments.receipts_dir,
        contract,
        version,
    )
    write_json(arguments.manifest_out, manifest)
    print(
        f"verified {len(manifest['artifacts'])} Python package artifacts and "
        f"{manifest['receipt_coverage']['wheel_smokes']} exact-wheel smokes; "
        f"manifest: {arguments.manifest_out}"
    )


if __name__ == "__main__":
    main()

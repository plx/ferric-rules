#!/usr/bin/env python3
"""Check or explicitly refresh the canonical C header from Cargo's OUT_DIR."""

import argparse
import difflib
import json
from pathlib import Path
import subprocess
import sys


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "mode", choices=("check", "generate"), nargs="?", default="check"
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    manifest = root / "crates/ferric-rules-ffi/Cargo.toml"
    package = subprocess.check_output(
        ["cargo", "pkgid", "--manifest-path", str(manifest)], text=True, cwd=root
    ).strip()
    result = subprocess.run(
        [
            "cargo",
            "check",
            "--locked",
            "--manifest-path",
            str(manifest),
            "--all-features",
            "--message-format=json",
        ],
        cwd=root,
        stdout=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode:
        # Rust diagnostics are inside the JSON stream in this mode.
        for line in result.stdout.splitlines():
            event = json.loads(line)
            if event.get("reason") == "compiler-message":
                print(event["message"].get("rendered", ""), file=sys.stderr, end="")
        return result.returncode
    outputs = [
        Path(event["out_dir"]) / "ferric.h"
        for line in result.stdout.splitlines()
        if (event := json.loads(line)).get("reason") == "build-script-executed"
        and event.get("package_id") == package
    ]
    if len(outputs) != 1:
        raise RuntimeError(f"expected one FFI build output, found {len(outputs)}")
    generated = outputs[0].read_bytes()
    canonical = root / "crates/ferric-rules-ffi/ferric.h"
    if args.mode == "generate":
        canonical.write_bytes(generated)
        (root / "bindings/go/internal/ffi/lib/ferric.h").write_bytes(generated)
        return 0
    expected = canonical.read_bytes()
    if expected == generated:
        return 0
    sys.stderr.writelines(
        difflib.unified_diff(
            expected.decode().splitlines(keepends=True),
            generated.decode().splitlines(keepends=True),
            fromfile=str(canonical),
            tofile="Cargo OUT_DIR/ferric.h",
        )
    )
    print("C header is stale; run just generate-ffi-header", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())

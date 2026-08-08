#!/usr/bin/env python3
"""Link musl extension modules with GCC's unwind runtime statically."""

from __future__ import annotations

import os
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path
from tempfile import TemporaryDirectory


REAL_LINKER_ENV = "CARGO_FERRIC_MUSL_LINKER"
LINKER_CONFIGS = {
    "/usr/local/musl/bin/aarch64-unknown-linux-musl-gcc": (
        Path("/usr/local/musl/aarch64-unknown-linux-musl/lib/libc.so"),
        "libc.musl-aarch64.so.1",
    ),
    "/usr/local/musl/bin/x86_64-unknown-linux-musl-gcc": (
        Path("/usr/local/musl/x86_64-unknown-linux-musl/lib/libc.so"),
        "libc.musl-x86_64.so.1",
    ),
}


def rewrite_linker_arguments(
    arguments: Sequence[str], *, alias_dir: Path, canonical_libc: str
) -> list[str]:
    """Make the GCC unwind runtime static and the musl libc name canonical."""

    rewritten: list[str] = []
    gcc_runtime_replacements = 0
    libc_replacements = 0
    for argument in arguments:
        if argument == "-lgcc_s":
            rewritten.extend(("-Wl,-Bstatic", "-lgcc_eh", "-Wl,-Bdynamic"))
            gcc_runtime_replacements += 1
        elif argument == "-lc":
            rewritten.extend((f"-L{alias_dir}", f"-l:{canonical_libc}"))
            libc_replacements += 1
        else:
            rewritten.append(argument)
    if gcc_runtime_replacements != 1 or libc_replacements != 1:
        raise ValueError(
            "expected exactly one dynamic -lgcc_s and one -lc linker argument, "
            f"found {gcc_runtime_replacements} and {libc_replacements}"
        )
    return rewritten


def main() -> None:
    real_linker = os.environ.get(REAL_LINKER_ENV)
    if real_linker not in LINKER_CONFIGS:
        allowed = ", ".join(sorted(LINKER_CONFIGS))
        raise SystemExit(
            f"{REAL_LINKER_ENV} must name one pinned musl linker ({allowed})"
        )
    real_libc, canonical_libc = LINKER_CONFIGS[real_linker]
    if not real_libc.is_file():
        raise SystemExit(f"pinned musl libc does not exist: {real_libc}")

    with TemporaryDirectory(prefix="ferric-musl-libc-", dir="/tmp") as temporary:
        alias_dir = Path(temporary)
        (alias_dir / canonical_libc).symlink_to(real_libc)
        try:
            arguments = rewrite_linker_arguments(
                sys.argv[1:], alias_dir=alias_dir, canonical_libc=canonical_libc
            )
        except ValueError as error:
            raise SystemExit(str(error)) from error
        completed = subprocess.run([real_linker, *arguments], check=False)
        raise SystemExit(completed.returncode)


if __name__ == "__main__":
    main()

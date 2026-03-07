"""JSON manifest load/save utilities."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path


def load_manifest(path: str | Path) -> dict:
    """Load a JSON manifest from *path*."""
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def save_manifest(path: str | Path, data: dict) -> None:
    """Write *data* as pretty-printed JSON to *path*."""
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")


def utc_now_iso() -> str:
    """Return the current UTC time as an ISO-8601 string."""
    return datetime.now(UTC).isoformat()

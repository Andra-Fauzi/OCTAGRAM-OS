#!/usr/bin/env python3
"""Validate the selected Rust Unix PAL C API inventory against OCTAGRAM."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tools/rust_std_pal_api_inventory.json"


def main() -> int:
    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    runtime = (ROOT / "user/runtime.c").read_text(encoding="utf-8")
    checked = 0
    for entry in data["entries"]:
        symbol = entry["symbol"]
        header_path = ROOT / entry["header"]
        header = header_path.read_text(encoding="utf-8")
        declaration = re.compile(r"\b" + re.escape(symbol) + r"\s*\(")
        definition = re.compile(
            r"(?m)^[\w\s\*]+\b" + re.escape(symbol)
            + r"\s*\([^;{}]*\)\s*\{"
        )
        if not declaration.search(header):
            raise SystemExit(f"RUST-STD-PAL-FAIL: {symbol} missing declaration in {entry['header']}")
        if not definition.search(runtime):
            raise SystemExit(f"RUST-STD-PAL-FAIL: {symbol} missing runtime definition")
        checked += 1

    partial = sum(item["status"] == "partial" for item in data["entries"])
    unsupported = sum(item["status"] == "unsupported" for item in data["entries"])
    print(
        f"RUST-STD-PAL-INVENTORY-PASS: {checked} declarations/definitions; "
        f"{partial} partial, {unsupported} unsupported"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

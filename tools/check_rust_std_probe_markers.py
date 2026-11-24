#!/usr/bin/env python3
"""Keep the Rust std guest probe and its QEMU marker gate in sync."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "user/rust/std-probe/src/main.rs"
HARNESS = ROOT / "tools/qemu_syscall_test.py"

SOURCE_MARKER = re.compile(r'marker\("(RUST-STD-[A-Z0-9-]+-PASS)"\)')
DIRECT_OUTPUT_MARKER = re.compile(r'(RUST-STD-[A-Z0-9-]+-PASS)\\n')
HARNESS_MARKER = re.compile(r'\("(RUST-STD-[A-Z0-9-]+-PASS)"\s*,')


def main() -> int:
    source_text = SOURCE.read_text(encoding="utf-8")
    harness_text = HARNESS.read_text(encoding="utf-8")

    source_markers = set(SOURCE_MARKER.findall(source_text))
    source_markers.update(DIRECT_OUTPUT_MARKER.findall(source_text))
    harness_markers = set(HARNESS_MARKER.findall(harness_text))

    missing = sorted(source_markers - harness_markers)
    if missing:
        print("RUST-STD-PROBE-MARKERS-FAIL: QEMU harness does not wait for:")
        for marker in missing:
            print(f"  - {marker}")
        return 1

    print(
        "RUST-STD-PROBE-MARKERS-PASS: "
        f"{len(source_markers)} source markers are covered by the QEMU harness"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

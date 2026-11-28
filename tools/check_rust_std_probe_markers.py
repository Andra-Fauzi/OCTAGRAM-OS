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

    source_positions = [
        (match.start(), match.group(1))
        for match in SOURCE_MARKER.finditer(source_text)
    ] + [
        (match.start(), match.group(1))
        for match in DIRECT_OUTPUT_MARKER.finditer(source_text)
    ]
    source_order = [marker for _, marker in sorted(source_positions)]
    source_markers = set(source_order)
    harness_order = HARNESS_MARKER.findall(harness_text)
    harness_markers = set(harness_order)

    missing = sorted(source_markers - harness_markers)
    if missing:
        print("RUST-STD-PROBE-MARKERS-FAIL: QEMU harness does not wait for:")
        for marker in missing:
            print(f"  - {marker}")
        return 1

    if source_order != harness_order:
        mismatch = next(
            (index for index, pair in enumerate(zip(source_order, harness_order))
             if pair[0] != pair[1]),
            min(len(source_order), len(harness_order)),
        )
        expected = source_order[mismatch] if mismatch < len(source_order) else "<end>"
        actual = harness_order[mismatch] if mismatch < len(harness_order) else "<end>"
        print(
            "RUST-STD-PROBE-MARKERS-ORDER-FAIL: "
            f"first mismatch at index {mismatch}: source={expected}, harness={actual}"
        )
        return 1

    print(
        "RUST-STD-PROBE-MARKERS-PASS: "
        f"{len(source_markers)} source markers are covered in execution order"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

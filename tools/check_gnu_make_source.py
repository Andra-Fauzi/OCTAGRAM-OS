#!/usr/bin/env python3
"""Validate the pinned GNU Make source checkout used by the OCTAGRAM port."""
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "user" / "third_party" / "gnu-make"
EXPECTED = "d66a65ad5a0e31b287f53930b0f09e31801f1613"
REQUIRED = (
    "COPYING",
    "configure.ac",
    "src/main.c",
    "src/makeint.h",
    "src/read.c",
    "src/variable.c",
    "src/job.c",
    "src/remake.c",
    "src/function.c",
)

def fail(message: str) -> None:
    print(f"GNU-MAKE-SOURCE-CHECK: FAIL: {message}", file=sys.stderr)
    raise SystemExit(1)

if not SOURCE.is_dir():
    fail(f"{SOURCE.relative_to(ROOT)} is missing; initialize submodules recursively")

try:
    actual = subprocess.check_output(
        ["git", "-C", str(SOURCE), "rev-parse", "HEAD"],
        text=True,
        stderr=subprocess.STDOUT,
    ).strip()
except (OSError, subprocess.CalledProcessError) as error:
    fail(f"cannot inspect upstream submodule revision: {error}")

if actual != EXPECTED:
    fail(f"expected upstream commit {EXPECTED}, got {actual}")

missing = [name for name in REQUIRED if not (SOURCE / name).is_file()]
if missing:
    fail("upstream source is incomplete: " + ", ".join(missing))

license_text = (SOURCE / "COPYING").read_text(encoding="utf-8", errors="replace")
if "GNU GENERAL PUBLIC LICENSE" not in license_text or "Version 3" not in license_text:
    fail("unexpected upstream license text; review before redistributing")

print("GNU-MAKE-SOURCE-CHECK: PASS")
print(f"GNU Make 4.4.1 source pinned at {actual}")

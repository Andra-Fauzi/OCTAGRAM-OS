#!/usr/bin/env python3
"""Validate the OCTAGRAM Rust host target specification with rustc."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TOOLCHAIN = os.environ.get("OCTAGRAM_RUST_TOOLCHAIN", "nightly")
TARGET = ROOT / "user/rust/targets/x86_64-unknown-octagram.json"


def main() -> int:
    command = [
        "rustc",
        f"+{TOOLCHAIN}",
        "-Z",
        "unstable-options",
        "--print",
        "cfg",
        "--target",
        str(TARGET),
    ]
    try:
        result = subprocess.run(
            command,
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        print("ERROR: rustc executable not found", file=sys.stderr)
        return 2
    except subprocess.CalledProcessError as error:
        print(error.stderr, file=sys.stderr)
        return error.returncode

    cfg = set(result.stdout.splitlines())
    required = {
        'target_arch="x86_64"',
        'target_os="octagram"',
        'target_pointer_width="64"',
        'target_endian="little"',
        'target_env=""',
        'unix',
    }
    missing = sorted(required - cfg)
    if missing:
        print("ERROR: target cfg is missing: " + ", ".join(missing), file=sys.stderr)
        return 1

    print("OCTAGRAM-RUST-TARGET-PASS")
    print("Validated custom x86_64 host target identity with rustc.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

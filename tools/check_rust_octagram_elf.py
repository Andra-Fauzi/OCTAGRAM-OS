#!/usr/bin/env python3
"""Validate the experimental OCTAGRAM Rust guest ELF before embedding it."""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
from pathlib import Path


DEFAULT_ELF = Path("user/rust/build/rust_hello_octagram.elf")


def output(*args: str) -> str:
    completed = subprocess.run(args, check=True, capture_output=True, text=True)
    return completed.stdout


def fail(message: str) -> None:
    raise SystemExit(f"RUST-OCTAGRAM-ELF-FAIL: {message}")


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_ELF
    if not path.is_file():
        fail(f"ELF does not exist: {path}")

    header = output("readelf", "-h", str(path))
    for required in ("ELF64", "EXEC (Executable file)", "Advanced Micro Devices X86-64"):
        if required not in header:
            fail(f"ELF header is missing expected field: {required}")

    program_headers = output("readelf", "-l", str(path))
    if "LOAD" not in program_headers:
        fail("ELF has no PT_LOAD segment")
    if "INTERP" in program_headers or "Requesting program interpreter" in program_headers:
        fail("freestanding guest unexpectedly requests a dynamic interpreter")

    undefined = output("nm", "-u", str(path)).strip()
    if undefined:
        fail("ELF contains undefined symbols:\\n" + undefined)

    symbols = output("nm", "-n", str(path))
    if re.search(r"\\b[Tt]\\s+_start$", symbols, re.MULTILINE) is None:
        fail("ELF does not export the _start entry symbol")

    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    print(f"RUST-OCTAGRAM-ELF-PASS: {path} ({path.stat().st_size} bytes, sha256={digest})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

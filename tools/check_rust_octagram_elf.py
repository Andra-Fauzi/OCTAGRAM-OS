#!/usr/bin/env python3
"""Validate the experimental OCTAGRAM Rust guest ELF before embedding it."""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_ELF = Path("user/rust/build/rust_hello_octagram.elf")


class ValidationError(Exception):
    """Raised when the guest ELF violates the freestanding ABI contract."""


def output(*args: str) -> str:
    try:
        completed = subprocess.run(args, check=True, capture_output=True, text=True)
    except FileNotFoundError as error:
        raise ValidationError(f"required tool is missing: {args[0]}") from error
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or str(error)).strip()
        raise ValidationError(f"{args[0]} failed: {detail}") from error
    return completed.stdout


def validate_elf(path: Path) -> tuple[int, str]:
    if not path.is_file():
        raise ValidationError(f"ELF does not exist: {path}")

    header = output("readelf", "-h", str(path))
    for required in ("ELF64", "EXEC (Executable file)", "Advanced Micro Devices X86-64"):
        if required not in header:
            raise ValidationError(f"ELF header is missing expected field: {required}")

    entry_match = re.search(r"(?m)^\s*Entry point address:\s*(0x[0-9a-fA-F]+)\s*$", header)
    if entry_match is None:
        raise ValidationError("ELF header does not contain a readable entry point")
    entry_address = int(entry_match.group(1), 16)

    program_headers = output("readelf", "-l", str(path))
    if not re.search(r"(?m)^\s*LOAD\s", program_headers):
        raise ValidationError("ELF has no PT_LOAD segment")
    if re.search(r"(?m)^\s*INTERP\s", program_headers):
        raise ValidationError("freestanding guest unexpectedly requests a dynamic interpreter")

    undefined = output("nm", "-u", str(path)).strip()
    if undefined:
        raise ValidationError("ELF contains undefined symbols:\n" + undefined)

    symbols = output("nm", "-n", "--defined-only", str(path))
    start_match = re.search(
        r"(?m)^\s*([0-9a-fA-F]+)\s+[Tt]\s+_start\s*$",
        symbols,
    )
    if start_match is None:
        raise ValidationError("ELF does not export the _start entry symbol")
    start_address = int(start_match.group(1), 16)
    if start_address != entry_address:
        raise ValidationError(
            f"ELF entry point {entry_address:#x} does not match _start {start_address:#x}"
        )

    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return path.stat().st_size, digest


def main(argv: list[str] | None = None) -> int:
    arguments = sys.argv[1:] if argv is None else argv
    if len(arguments) > 1:
        print("usage: check_rust_octagram_elf.py [ELF]", file=sys.stderr)
        return 2
    path = Path(arguments[0]) if arguments else DEFAULT_ELF
    try:
        size, digest = validate_elf(path)
    except ValidationError as error:
        print(f"RUST-OCTAGRAM-ELF-FAIL: {error}", file=sys.stderr)
        return 1

    print(f"RUST-OCTAGRAM-ELF-PASS: {path} ({size} bytes, sha256={digest})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

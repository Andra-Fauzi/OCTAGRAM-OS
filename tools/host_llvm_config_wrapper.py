#!/usr/bin/env python3
"""Host-only llvm-config adapter used by the OCTAGRAM Rust bootstrap.

LLVM's host llvm-config may emit shared-library filenames (for example
`/usr/lib/x86_64-linux-gnu/libz3.so`) in `--system-libs`. Rust's
rustc_llvm build script interprets those as bare library names and turns them
into the invalid linker option `-llibz3.so`. Normalize such entries to the
portable linker form `-lz3`. Do not change the OCTAGRAM target llvm-config.
"""

from __future__ import annotations

import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

SHARED_LIBRARY_RE = re.compile(r"lib(?P<name>[A-Za-z0-9_+.-]+)\.so(?:\.\d+)*")


def normalize_shared_library_token(token: str) -> str:
    """Convert .so filenames/paths in llvm-config system-libs to -lNAME."""
    candidate = token
    if candidate.startswith("-l"):
        candidate = candidate[2:]
    if candidate.startswith("/"):
        candidate = Path(candidate).name
    match = SHARED_LIBRARY_RE.fullmatch(candidate)
    if not match:
        return token
    return "-l" + match.group("name")


def normalize_system_libraries(output: str) -> str:
    normalized_lines = []
    for line in output.splitlines():
        tokens = shlex.split(line)
        normalized_lines.append(shlex.join(normalize_shared_library_token(t) for t in tokens))
    suffix = "\n" if output.endswith("\n") else ""
    return "\n".join(normalized_lines) + suffix


def self_test() -> int:
    cases = {
        "-llibz3.so": "-lz3",
        "/usr/lib/x86_64-linux-gnu/libz3.so": "-lz3",
        "/opt/llvm/lib/libPolly.so.22": "-lPolly",
        "-lPolly": "-lPolly",
        "-lz": "-lz",
    }
    for source, expected in cases.items():
        actual = normalize_shared_library_token(source)
        if actual != expected:
            print(f"FAIL: {source!r} -> {actual!r}, expected {expected!r}", file=sys.stderr)
            return 1
    actual = normalize_system_libraries("-lrt /usr/lib/libz3.so -llibxml2.so\n")
    if actual != "-lrt -lz3 -lxml2\n":
        print(f"FAIL: system-libs normalization returned {actual!r}", file=sys.stderr)
        return 1
    print("OCTAGRAM-HOST-LLVM-CONFIG-WRAPPER-PASS")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()

    real_config = os.environ.get("OCTAGRAM_REAL_LLVM_CONFIG")
    if not real_config:
        print("host-llvm-config-wrapper: OCTAGRAM_REAL_LLVM_CONFIG is not set", file=sys.stderr)
        return 2

    try:
        result = subprocess.run(
            [real_config, *argv],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except OSError as exc:
        print(f"host-llvm-config-wrapper: cannot execute {real_config}: {exc}", file=sys.stderr)
        return 2

    if result.stderr:
        sys.stderr.write(result.stderr)
    if result.returncode != 0:
        return result.returncode

    output = result.stdout
    if "--system-libs" in argv:
        output = normalize_system_libraries(output)
    sys.stdout.write(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

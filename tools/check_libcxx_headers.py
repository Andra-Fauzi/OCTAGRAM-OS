#!/usr/bin/env python3
"""Check which vendored libc++ headers compile against OCTAGRAM's C library.

libc++ is being brought up header by header: each standard header pulls in more
of the C library (C99 math, wide stdio, ...), so the set that compiles is a
progress ratchet rather than something that passes all at once. EXPECTED_PASS
lists the headers that must compile today; a regression there fails the run.
Anything else is reported as "not yet supported" and does not fail.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
USER = REPO / "user"
LIBCXX = USER / "third_party" / "libcxx" / "include"

EXPECTED_PASS = {
    "cstddef",
    "cstdint",
    "cstring",
    "limits",
    "new",
    "type_traits",
}

# Headers worth tracking; the rest of the standard library is not attempted yet.
TRACKED = [
    "algorithm",
    "array",
    "atomic",
    "charconv",
    "chrono",
    "cmath",
    "cstdio",
    "cstdlib",
    "ctime",
    "cwchar",
    "functional",
    "memory",
    "mutex",
    "optional",
    "random",
    "string",
    "string_view",
    "thread",
    "tuple",
    "utility",
    "variant",
    "vector",
]


def compiles(header: str) -> tuple[bool, str]:
    with tempfile.NamedTemporaryFile("w", suffix=".cpp", delete=False) as source:
        source.write(f"#include <{header}>\n")
        path = source.name
    command = [
        "g++",
        "-std=c++17",
        "-fsyntax-only",
        "-nostdinc++",
        "-isystem",
        str(LIBCXX),
        "-isystem",
        str(USER / "include"),
        "-ffreestanding",
        "-fno-exceptions",
        "-fno-rtti",
        path,
    ]
    result = subprocess.run(command, capture_output=True, text=True)
    pathlib.Path(path).unlink()
    if result.returncode == 0:
        return True, ""
    for line in result.stderr.splitlines():
        if "error:" in line:
            return False, line.strip()
    return False, "compilation failed"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--all", action="store_true", help="also check untracked standard headers")
    arguments = parser.parse_args()

    headers = sorted(EXPECTED_PASS)
    passed = set()
    failures = []
    for header in headers:
        ok, message = compiles(header)
        if ok:
            passed.add(header)
        else:
            failures.append((header, message))

    if failures:
        for header, message in failures:
            print(f"REGRESSED <{header}>: {message}")
        print(f"libc++ header check: FAIL ({len(failures)} of {len(headers)} required headers)")
        return 1

    if arguments.all:
        for header in TRACKED:
            ok, message = compiles(header)
            if ok:
                passed.add(header)
            else:
                first_error = message.split("error:")[-1].strip()[:100]
                print(f"pending   <{header}>: {first_error}")
        pending = len(TRACKED) - len(set(TRACKED) & passed)
        print(f"libc++ header check: PASS ({len(passed)} headers compile, {pending} tracked headers pending)")
    else:
        print(f"libc++ header check: PASS ({len(passed)} headers compile as C++17)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

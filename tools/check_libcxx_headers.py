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
    "algorithm",
    "any",
    "array",
    "atomic",
    "barrier",
    "bit",
    "bitset",
    "cassert",
    "ccomplex",
    "cctype",
    "cerrno",
    "cfenv",
    "cfloat",
    "charconv",
    "chrono",
    "cinttypes",
    "climits",
    "clocale",
    "cmath",
    "codecvt",
    "compare",
    "complex",
    "concepts",
    "condition_variable",
    "csetjmp",
    "csignal",
    "cstdarg",
    "cstddef",
    "cstdint",
    "cstdio",
    "cstdlib",
    "cstring",
    "ctgmath",
    "ctime",
    "cuchar",
    "cwchar",
    "cwctype",
    "deque",
    "exception",
    "execution",
    "expected",
    "filesystem",
    "format",
    "forward_list",
    "fstream",
    "functional",
    "future",
    "initializer_list",
    "iomanip",
    "ios",
    "iosfwd",
    "iostream",
    "istream",
    "iterator",
    "latch",
    "limits",
    "list",
    "locale",
    "map",
    "mdspan",
    "memory",
    "memory_resource",
    "mutex",
    "new",
    "numbers",
    "numeric",
    "optional",
    "ostream",
    "queue",
    "random",
    "ranges",
    "ratio",
    "regex",
    "scoped_allocator",
    "semaphore",
    "set",
    "shared_mutex",
    "source_location",
    "span",
    "sstream",
    "stack",
    "stdexcept",
    "stop_token",
    "streambuf",
    "string",
    "string_view",
    "strstream",
    "syncstream",
    "system_error",
    "thread",
    "tuple",
    "type_traits",
    "typeindex",
    "typeinfo",
    "unordered_map",
    "unordered_set",
    "utility",
    "valarray",
    "variant",
    "vector",
    "version",
}

# Headers worth tracking; the rest of the standard library is not attempted yet.
# Headers that are not expected to work yet: C++23 additions missing from
# libc++ 18, and deprecated aliases.
TRACKED = [
    "cstdalign",
    "ciso646",
    "flat_map",
    "flat_set",
]


def compiles(header: str) -> tuple[bool, str]:
    with tempfile.NamedTemporaryFile("w", suffix=".cpp", delete=False) as source:
        source.write(f"#include <{header}>\n")
        path = source.name
    command = [
        "g++",
        "-std=c++17",
        "-fsyntax-only",
        # Match the real build: the guest must never see a host C header such
        # as glibc's <bits/types/mbstate_t.h>, which libc++ would happily pick
        # up through __has_include and then disagree with us about mbstate_t.
        "-nostdinc",
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

#!/usr/bin/env python3
"""Verify that every public OCTAGRAM userspace header compiles as C and C++.

libc++/LLVM translation units include the C headers from C++, so a header that
silently only works in C (missing `extern "C"`, C++ keyword parameter names,
`restrict`, implicit void* conversions) breaks the port at link time with
mangled symbols. This checker compiles a tiny translation unit per header with
both front ends and reports the failing headers.

Usage: python3 tools/check_cpp_headers.py [header ...]
Exit status is non-zero when any header fails.
"""
from __future__ import annotations

import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INCLUDE = os.path.join(ROOT, "user", "include")

C_FLAGS = ["-std=c11", "-Wall", "-Wextra", "-Werror", "-ffreestanding", "-fno-builtin"]
CXX_FLAGS = [
    "-std=c++17",
    "-Wall",
    "-Wextra",
    "-Werror",
    "-ffreestanding",
    "-fno-builtin",
    "-fno-exceptions",
    "-fno-rtti",
    "-nostdinc++",
]


def header_names():
    names = []
    for base, _directories, files in os.walk(INCLUDE):
        for name in sorted(files):
            if not name.endswith(".h"):
                continue
            relative = os.path.relpath(os.path.join(base, name), INCLUDE)
            names.append(relative.replace(os.sep, "/"))
    return sorted(names)


def compile_header(header: str, compiler: str, flags: list[str], workdir: str) -> str | None:
    source = os.path.join(workdir, "tu.c" if compiler == "gcc" else "tu.cpp")
    with open(source, "w", encoding="utf-8") as handle:
        handle.write(f'#include "{header}"\nint main(void) {{ return 0; }}\n')
    command = [compiler, *flags, "-I", INCLUDE, "-fsyntax-only", source]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode == 0:
        return None
    details = (result.stderr or result.stdout).strip().splitlines()
    return "\n".join(details[:6])


def main() -> int:
    requested = sys.argv[1:]
    headers = requested or header_names()
    failures = 0
    with tempfile.TemporaryDirectory() as workdir:
        for header in headers:
            for compiler, flags in (("gcc", C_FLAGS), ("g++", CXX_FLAGS)):
                error = compile_header(header, compiler, flags, workdir)
                if error is not None:
                    failures += 1
                    print(f"FAIL {header} [{compiler}]")
                    print("\n".join(f"  {line}" for line in error.splitlines()))
    if failures != 0:
        print(f"cpp header check: FAIL ({failures} header/compiler combinations)")
        return 1
    print(f"cpp header check: PASS ({len(headers)} headers as C and C++)")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Stage the OCTAGRAM userspace sysroot for native toolchain builds.

The sysroot is what a cross build of libLLVM (M4.3.3) and then of rustc
(M4.3.4) consumes: public headers, libc++ headers, startup objects, libc.a,
libc++.a, and the linker script. It is assembled from the same artefacts the
guest programs are built from, so a sysroot build cannot drift from the image.

Layout produced:

    <output>/usr/include/            OCTAGRAM public C headers (user/include)
    <output>/usr/include/c++/v1/     vendored libc++ headers
    <output>/usr/include/c++/src/    libc++ src/ shim headers (__has_include_next)
    <output>/usr/lib/crt0.o          _start, TLS, .init_array/.fini_array
    <output>/usr/lib/libc.a          runtime.c + posix_extra + regex + math + ...
    <output>/usr/lib/libc++.a        vendored libc++ 18 objects
    <output>/usr/lib/libcxxrt.a      cxx_runtime + libcxx_runtime objects
    <output>/usr/lib/octagram.ld     user/linker.ld
    <output>/usr/lib/libtcc1.a       TinyCC support objects, when present
    <output>/sysroot.json            staged file manifest

Run it through the Makefile so the archives exist first:

    make -C user sysroot
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]

REQUIRED_ARTEFACTS = {
    "libc.a": "libc archive (runtime, posix_extra, regex, math, compiler_rt)",
    "libc++.a": "vendored libc++ archive",
    "libcxxrt.a": "C++ runtime objects (cxx_runtime, libcxx_runtime)",
    "crt0.o": "startup object",
}

OPTIONAL_ARTEFACTS = ["libtcc1.a"]

# Unix library names that CMake/LLVM and rustc ask the linker for. Their
# symbols all live in libc.a, so the sysroot exposes them as identical copies.
SYSTEM_LIBRARY_ALIASES = ("libm.a", "librt.a", "libdl.a", "libpthread.a")

# A valid, member-less `ar` archive: the global header and nothing else.
EMPTY_AR_ARCHIVE = b"!<arch>\n"

# libc++ ships headers without a suffix (string, vector, __config, __memory/...), so
# the trees are copied whole instead of being filtered by extension.
PRESERVED_SUFFIXES = None


def copy_tree(source: Path, destination: Path, suffixes: tuple[str, ...] | None) -> int:
    """Copy source into destination, optionally filtered by suffix."""
    copied = 0
    for path in sorted(source.rglob("*")):
        if not path.is_file():
            continue
        if suffixes is not None and path.suffix not in suffixes:
            continue
        relative = path.relative_to(source)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        copied += 1
    return copied


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--build-dir",
        type=Path,
        default=REPO_ROOT / "user" / "build",
        help="directory holding the userspace build artefacts",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="sysroot directory (default: <build-dir>/sysroot-octagram)",
    )
    arguments = parser.parse_args()

    build_dir = arguments.build_dir.resolve()
    output = (arguments.output or build_dir / "sysroot-octagram").resolve()
    user_dir = REPO_ROOT / "user"

    missing = [name for name, _ in REQUIRED_ARTEFACTS.items() if not (build_dir / name).is_file()]
    if missing:
        print("stage_octagram_sysroot: missing build artefacts:", file=sys.stderr)
        for name in missing:
            print(f"  {name}: {REQUIRED_ARTEFACTS[name]}", file=sys.stderr)
        print("stage_octagram_sysroot: run 'make -C user all' first", file=sys.stderr)
        return 1

    for required_header in ("include/stdio.h", "include/strings.h", "linker.ld", "third_party/libcxx/include/__config"):
        if not (user_dir / required_header).exists():
            print(
                f"stage_octagram_sysroot: missing source path user/{required_header}",
                file=sys.stderr,
            )
            return 1

    # Rebuild from scratch so a stale header or archive can never survive a
    # libc/libc++ change.
    if output.exists():
        shutil.rmtree(output)
    include_dir = output / "usr" / "include"
    lib_dir = output / "usr" / "lib"
    include_dir.mkdir(parents=True)
    lib_dir.mkdir(parents=True)

    staged: dict[str, dict[str, object]] = {}

    def record(path: Path, description: str) -> None:
        relative = str(path.relative_to(output))
        staged[relative] = {
            "description": description,
            "size": path.stat().st_size,
            "sha256": sha256(path),
        }

    header_count = copy_tree(user_dir / "include", include_dir, None)
    libcxx_headers = copy_tree(
        user_dir / "third_party" / "libcxx" / "include",
        include_dir / "c++" / "v1",
        PRESERVED_SUFFIXES,
    )
    libcxx_src = copy_tree(
        user_dir / "third_party" / "libcxx" / "src",
        include_dir / "c++" / "src",
        PRESERVED_SUFFIXES,
    )
    for name, description in REQUIRED_ARTEFACTS.items():
        shutil.copy2(build_dir / name, lib_dir / name)
        record(lib_dir / name, description)

    for name in OPTIONAL_ARTEFACTS:
        if (build_dir / name).is_file():
            shutil.copy2(build_dir / name, lib_dir / name)
            record(lib_dir / name, "optional support archive")

    # CMake and rustc link with the conventional Unix system library names.
    # Every one of those symbols lives in libc.a, which the compiler driver
    # always links, so the sysroot only needs the names to exist: otherwise
    # `-lm`/`-lrt`/`-ldl` fall through to the build host's static glibc archives
    # and the link fails with "errno: TLS reference in /usr/lib/.../libm.a
    # mismatches non-TLS reference". They are deliberately empty archives, not
    # copies of libc.a, so no member is pulled in twice through two names.
    for alias in SYSTEM_LIBRARY_ALIASES:
        empty_archive = lib_dir / alias
        empty_archive.write_bytes(EMPTY_AR_ARCHIVE)
        record(empty_archive, "empty archive; symbols come from libc.a")

    shutil.copy2(user_dir / "linker.ld", lib_dir / "octagram.ld")
    record(lib_dir / "octagram.ld", "static ET_EXEC linker script")

    manifest = {
        "layout": "x86_64-unknown-octagram",
        "source": "make -C user sysroot",
        "headers": {
            "c": header_count,
            "libcxx": libcxx_headers,
            "libcxx_src": libcxx_src,
        },
        "files": staged,
    }
    (output / "sysroot.json").write_text(json.dumps(manifest, indent=2) + "\n")

    print(f"staged OCTAGRAM sysroot at {output}")
    print(
        f"  {header_count} C headers, {libcxx_headers} libc++ headers, "
        f"{len(staged)} library/object files"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

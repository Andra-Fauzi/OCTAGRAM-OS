#!/usr/bin/env python3
"""Teach the locked openssl-src crate to build OpenSSL for OCTAGRAM.

OpenSSL's Configure target for the OCTAGRAM x86_64 ABI uses the Linux
x86_64 profile, while the actual compiler, archiver and headers come from
OCTAGRAM's cross toolchain. The patch also adds an OpenSSL-local strings.h
shim for its unconditional Unix include; it does not change the OCTAGRAM
sysroot, use host Linux OpenSSL libraries, or invalidate the LLVM input stamp.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

OPENSSL_SRC_VERSION = "300.6.1+3.6.3"
OLD_TARGET_ENTRY = '            "x86_64-unknown-linux-gnu" => "linux-x86_64",'

# OpenSSL's e_os.h unconditionally includes <strings.h> on Unix. OCTAGRAM's
# toolchain sysroot currently exposes strncasecmp through string.h but does not
# ship the separate POSIX strings.h header. Keep this shim inside the vendored
# OpenSSL source tree so the sysroot and LLVM build inputs remain untouched.
OPENSSL_STRINGS_SHIM = """#ifndef OCTAGRAM_OPENSSL_STRINGS_H
#define OCTAGRAM_OPENSSL_STRINGS_H

#include <string.h>

/* OpenSSL does not call the host strcasecmp symbol directly; map the standard
 * spelling to OCTAGRAM libc's existing strncasecmp implementation. */
static inline int strcasecmp(const char *left, const char *right)
{
    return strncasecmp(left, right, (size_t)-1);
}

#endif
"""
NEW_TARGET_ENTRY = (
    '            "x86_64-unknown-octagram" => "linux-x86_64",\n'
    + OLD_TARGET_ENTRY
)


def patch_source(source: str) -> str:
    octagram_entry = '"x86_64-unknown-octagram" => "linux-x86_64",'
    existing_octagram_entries = [
        line for line in source.splitlines() if '"x86_64-unknown-octagram" =>' in line
    ]
    if existing_octagram_entries:
        if existing_octagram_entries == ["            " + octagram_entry] and source.count(OLD_TARGET_ENTRY) == 1:
            return source
        raise ValueError(
            "openssl-src already has a different or duplicate OCTAGRAM mapping; "
            "refusing to overwrite it"
        )

    if source.count(OLD_TARGET_ENTRY) != 1:
        raise ValueError(
            "openssl-src target table does not match the pinned 300.6.1+3.6.3 source"
        )

    patched = source.replace(OLD_TARGET_ENTRY, NEW_TARGET_ENTRY, 1)
    if patched.count('"x86_64-unknown-octagram" => "linux-x86_64"') != 1:
        raise ValueError("failed to install the OCTAGRAM OpenSSL Configure mapping")
    if patched.count(OLD_TARGET_ENTRY) != 1:
        raise ValueError("the original Linux x86_64 Configure mapping was not preserved")
    return patched


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    source_file = package / "src" / "lib.rs"
    if not manifest.is_file() or not source_file.is_file():
        raise SystemExit(f"incomplete openssl-src source tree: {package}")

    metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
    version = metadata.get("package", {}).get("version")
    if version != OPENSSL_SRC_VERSION:
        raise SystemExit(
            f"unexpected openssl-src version in {manifest}: {version!r}; "
            f"expected {OPENSSL_SRC_VERSION}"
        )

    original = source_file.read_text(encoding="utf-8")
    patched = patch_source(original)
    changed = patched != original
    if changed:
        source_file.write_text(patched, encoding="utf-8")

    # openssl-src copies this directory to OUT_DIR before invoking Configure,
    # so the compatibility header remains local to its vendored OpenSSL build
    # and never leaks into the OCTAGRAM sysroot or the LLVM build fingerprint.
    openssl_include = package / "openssl" / "include"
    if not openssl_include.is_dir():
        raise SystemExit(
            f"vendored OpenSSL include directory is missing: {openssl_include}"
        )
    strings_header = openssl_include / "strings.h"
    if strings_header.exists():
        existing_header = strings_header.read_text(encoding="utf-8")
        if existing_header != OPENSSL_STRINGS_SHIM:
            raise SystemExit(
                f"unexpected pre-existing OpenSSL strings.h shim: {strings_header}; "
                "refusing to overwrite unreviewed source"
            )
    else:
        strings_header.write_text(OPENSSL_STRINGS_SHIM, encoding="utf-8")
        changed = True

    # Cargo re-extracts a package if its marker is missing or empty. Preserve
    # these local target adaptations across the build-script invocation.
    (package / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")
    return changed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--registry-root",
        type=Path,
        help="Cargo registry src directory (defaults to CARGO_HOME/registry/src)",
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        sample = (
            '            "x86_64-unknown-freebsd" => "BSD-x86_64",\n'
            + OLD_TARGET_ENTRY
            + '\n            "x86_64-unknown-linux-musl" => "linux-x86_64",\n'
        )
        patched = patch_source(sample)
        if patch_source(patched) != patched:
            raise SystemExit("openssl-src patch is not idempotent")
        if patched.count('"x86_64-unknown-octagram" => "linux-x86_64"') != 1:
            raise SystemExit("openssl-src self-test did not add the OCTAGRAM mapping")
        if "strncasecmp(left, right, (size_t)-1)" not in OPENSSL_STRINGS_SHIM:
            raise SystemExit("OpenSSL strings.h shim is missing the OCTAGRAM strncasecmp adapter")
        print("RUST-OPENSSL-SRC-OCTAGRAM-PATCH-SELF-TEST-PASS")
        return 0

    if args.registry_root is not None:
        registry_root = args.registry_root.expanduser().resolve()
    else:
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        registry_root = cargo_home / "registry" / "src"

    packages = sorted(registry_root.glob(f"*/openssl-src-{OPENSSL_SRC_VERSION}"))
    if not packages:
        raise SystemExit(
            f"openssl-src {OPENSSL_SRC_VERSION} was not found below {registry_root}; "
            "fetch the exact Cargo.lock version before applying the OCTAGRAM patch"
        )

    changed = sum(int(patch_package(package)) for package in packages)
    print(
        "RUST-OPENSSL-SRC-OCTAGRAM-PATCH-PASS: "
        f"checked {len(packages)} source tree(s), patched {changed}; "
        "x86_64 OCTAGRAM uses the cross-compiler, OpenSSL's linux-x86_64 Configure profile, and a build-local strings.h adapter"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as exc:
        print(f"patch_openssl_src_octagram: error: {exc}", file=sys.stderr)
        raise SystemExit(1)

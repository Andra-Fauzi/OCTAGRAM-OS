#!/usr/bin/env python3
"""Teach the locked openssl-src crate to build OpenSSL for OCTAGRAM.

OpenSSL's own Configure target for the OCTAGRAM x86_64 ABI is currently the
Linux x86_64 profile, while the actual compiler, archiver and headers come
from OCTAGRAM's cross toolchain. This is a Configure-platform mapping only;
it does not use the host Linux compiler or host OpenSSL libraries.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

OPENSSL_SRC_VERSION = "300.6.1+3.6.3"
OLD_TARGET_ENTRY = '            "x86_64-unknown-linux-gnu" => "linux-x86_64",'
NEW_TARGET_ENTRY = (
    '            "x86_64-unknown-octagram" => "linux-x86_64",\n'
    + OLD_TARGET_ENTRY
)


def patch_source(source: str) -> str:
    if source.count(OLD_TARGET_ENTRY) != 1:
        if source.count(NEW_TARGET_ENTRY) == 1:
            return source
        raise ValueError(
            "openssl-src target table does not match the pinned 300.6.1+3.6.3 source"
        )

    if '"x86_64-unknown-octagram" => "linux-x86_64"' in source:
        raise ValueError(
            "openssl-src already has a different OCTAGRAM mapping; refusing to overwrite it"
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
    if patched == original:
        return False

    source_file.write_text(patched, encoding="utf-8")

    # Cargo re-extracts a package if its marker is missing or empty. Preserve
    # this local target mapping across the build-script invocation.
    (package / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")
    return True


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
        "x86_64 OCTAGRAM uses the cross-compiler with OpenSSL's linux-x86_64 Configure profile"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as exc:
        print(f"patch_openssl_src_octagram: error: {exc}", file=sys.stderr)
        raise SystemExit(1)

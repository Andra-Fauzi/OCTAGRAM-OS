#!/usr/bin/env python3
"""Patch getrandom 0.3.3 to use OCTAGRAM's libc getrandom() implementation.

OCTAGRAM already exports getrandom(2) from libc. This patch selects the
upstream libc-backed getrandom implementation for target_os = "octagram";
it does not use a Linux syscall number or a CPU-specific random instruction.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

GETRANDOM_VERSION = "0.3.3"
MANIFEST_BLOCK = """[target.'cfg(target_os = "octagram")'.dependencies]
libc = { version = "0.2.154", default-features = false }

"""
MANIFEST_ANCHOR = "[lints.rust.unexpected_cfgs]\n"
BACKEND_BLOCK = """    } else if #[cfg(target_os = "octagram")] {
        mod getrandom;
        pub use getrandom::*;
"""
BACKEND_ANCHOR = """    } else if #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "dragonfly",
"""
BACKEND_MARKER = 'target_os = "octagram")] {\n        mod getrandom;'


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    backend = package / "src/backends.rs"
    libc_backend = package / "src/backends/getrandom.rs"
    if not manifest.is_file() or not backend.is_file() or not libc_backend.is_file():
        raise SystemExit(f"incomplete getrandom {GETRANDOM_VERSION} source tree: {package}")

    manifest_source = manifest.read_text(encoding="utf-8")
    parsed = tomllib.loads(manifest_source)
    version = parsed.get("package", {}).get("version")
    if version != GETRANDOM_VERSION:
        raise SystemExit(
            f"unexpected getrandom version in {manifest}: {version!r}; "
            f"expected {GETRANDOM_VERSION}"
        )

    changed = False
    if MANIFEST_BLOCK not in manifest_source:
        if manifest_source.count(MANIFEST_ANCHOR) != 1:
            raise SystemExit(
                f"expected one exact Cargo.toml insertion anchor in {manifest}; "
                "refusing an unverified dependency edit"
            )
        manifest_source = manifest_source.replace(
            MANIFEST_ANCHOR, MANIFEST_BLOCK + MANIFEST_ANCHOR, 1
        )
        manifest.write_text(manifest_source, encoding="utf-8")
        changed = True

    parsed = tomllib.loads(manifest.read_text(encoding="utf-8"))
    target_dependencies = (
        parsed.get("target", {})
        .get('cfg(target_os = "octagram")', {})
        .get("dependencies", {})
    )
    libc_dependency = target_dependencies.get("libc")
    if not isinstance(libc_dependency, dict) or libc_dependency.get("version") != "0.2.154":
        raise SystemExit(
            f"{manifest} does not declare the expected OCTAGRAM libc dependency"
        )

    backend_source = backend.read_text(encoding="utf-8")
    if BACKEND_BLOCK not in backend_source:
        if backend_source.count(BACKEND_ANCHOR) != 1:
            raise SystemExit(
                f"expected one exact backend-selection anchor in {backend}; "
                "refusing an unverified source edit"
            )
        backend_source = backend_source.replace(
            BACKEND_ANCHOR, BACKEND_BLOCK + BACKEND_ANCHOR, 1
        )
        backend.write_text(backend_source, encoding="utf-8")
        changed = True

    final_backend = backend.read_text(encoding="utf-8")
    if BACKEND_BLOCK not in final_backend or BACKEND_MARKER not in final_backend:
        raise SystemExit(f"OCTAGRAM getrandom backend selection did not verify in {backend}")
    return changed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--registry-root",
        type=Path,
        help="Cargo registry src directory (defaults to CARGO_HOME/registry/src)",
    )
    args = parser.parse_args()

    if args.registry_root is not None:
        registry_root = args.registry_root.expanduser().resolve()
    else:
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        registry_root = cargo_home / "registry" / "src"

    packages = sorted(registry_root.glob(f"*/getrandom-{GETRANDOM_VERSION}"))
    if not packages:
        raise SystemExit(
            f"getrandom {GETRANDOM_VERSION} was not found below {registry_root}; "
            "run cargo fetch before applying the OCTAGRAM backend"
        )

    changed_count = 0
    for package in packages:
        changed_count += int(patch_package(package))
    print(
        "RUST-GETRANDOM-OCTAGRAM-BACKEND-PASS: "
        f"patched {len(packages)} getrandom source tree(s); "
        "OCTAGRAM uses libc::getrandom"
    )
    if changed_count == 0:
        print("getrandom OCTAGRAM backend was already patched")
    return 0


if __name__ == "__main__":
    sys.exit(main())

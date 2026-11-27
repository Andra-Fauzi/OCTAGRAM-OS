#!/usr/bin/env python3
"""Patch errno 0.3.14 for OCTAGRAM's Unix errno ABI.

The runtime exports __errno_location(), not errno_location().  Upstream
errno's Unix backend needs that link-name mapping on target_os=octagram.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

ERRNO_VERSION = "0.3.14"

OLD_LINK_ATTR = """#[cfg_attr(
    any(
        target_os = "linux",
        target_os = "hurd",
        target_os = "redox",
        target_os = "dragonfly",
        target_os = "emscripten",
    ),
    link_name = "__errno_location"
)]"""
NEW_LINK_ATTR = """#[cfg_attr(
    any(
        target_os = "linux",
        target_os = "octagram",
        target_os = "hurd",
        target_os = "redox",
        target_os = "dragonfly",
        target_os = "emscripten",
    ),
    link_name = "__errno_location"
)]"""


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    unix_source = package / "src" / "unix.rs"
    if not manifest.is_file() or not unix_source.is_file():
        raise SystemExit(f"incomplete errno {ERRNO_VERSION} source tree: {package}")

    metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
    version = metadata.get("package", {}).get("version")
    if version != ERRNO_VERSION:
        raise SystemExit(
            f"unexpected errno version in {manifest}: {version!r}; "
            f"expected {ERRNO_VERSION}"
        )

    source = unix_source.read_text(encoding="utf-8")
    if NEW_LINK_ATTR in source:
        return False
    count = source.count(OLD_LINK_ATTR)
    if count != 1:
        raise SystemExit(
            f"expected one errno_location link-name block in {unix_source}, "
            f"found {count}; refusing an unreviewed source layout"
        )
    if "fn errno_location() -> *mut c_int;" not in source:
        raise SystemExit(f"errno_location declaration missing in {unix_source}")

    unix_source.write_text(source.replace(OLD_LINK_ATTR, NEW_LINK_ATTR, 1), encoding="utf-8")
    # Cargo may re-extract a registry package if this marker is missing.
    (package / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")
    return True


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

    packages = sorted(registry_root.glob(f"*/errno-{ERRNO_VERSION}"))
    if not packages:
        raise SystemExit(
            f"errno {ERRNO_VERSION} was not found below {registry_root}; "
            "run cargo fetch before applying the OCTAGRAM patch"
        )

    changed = sum(int(patch_package(package)) for package in packages)
    print(
        f"RUST-ERRNO-OCTAGRAM-BACKEND-PASS: checked {len(packages)} source tree(s); "
        f"patched {changed}; __errno_location symbol mapping verified"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

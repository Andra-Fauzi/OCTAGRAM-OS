#!/usr/bin/env python3
"""Select tempfile/filetime's POSIX utimensat backend on OCTAGRAM."""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

FILETIME_VERSIONS = ("0.2.27", "0.2.29")
UTIMENSAT_BACKEND = 'target_os = "octagram",'


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    unix_mod = package / "src" / "unix" / "mod.rs"
    if not manifest.is_file() or not unix_mod.is_file():
        raise SystemExit(f"incomplete filetime source tree: {package}")

    metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
    version = metadata.get("package", {}).get("version")
    if version not in FILETIME_VERSIONS:
        raise SystemExit(
            f"unexpected filetime version in {manifest}: {version!r}; "
            f"expected one of {', '.join(FILETIME_VERSIONS)}"
        )

    source = unix_mod.read_text(encoding="utf-8")
    # Both locked versions have an explicit utimensat platform group ending
    # with Haiku. OCTAGRAM implements the same utimensat entry point and the
    # libc module supplies timespec, AT_FDCWD, AT_SYMLINK_NOFOLLOW, UTIME_OMIT.
    marker = 'target_os = "haiku"'
    if UTIMENSAT_BACKEND in source:
        if "mod utimensat;" not in source:
            raise SystemExit(f"OCTAGRAM cfg found outside the utimensat backend in {unix_mod}")
        return False

    expected = 'target_os = "haiku"))] {\n        mod utimensat;'
    if source.count(expected) != 1:
        raise SystemExit(
            f"expected one filetime utimensat backend selector in {unix_mod}; "
            "refusing an unreviewed source layout"
        )
    replacement = (
        'target_os = "haiku",\n'
        '                        target_os = "octagram",))] {\n'
        '        mod utimensat;'
    )
    source = source.replace(expected, replacement, 1)
    if source.count(UTIMENSAT_BACKEND) != 1 or "mod utimensat;" not in source:
        raise SystemExit(f"failed to add OCTAGRAM utimensat backend in {unix_mod}")

    unix_mod.write_text(source, encoding="utf-8")
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

    packages = []
    for version in FILETIME_VERSIONS:
        packages.extend(sorted(registry_root.glob(f"*/filetime-{version}")))
    if not packages:
        raise SystemExit(
            f"filetime {', '.join(FILETIME_VERSIONS)} was not found below {registry_root}; "
            "run cargo fetch before applying the OCTAGRAM patch"
        )

    changed = sum(int(patch_package(package)) for package in packages)
    versions = sorted({p.name.removeprefix("filetime-") for p in packages})
    print(
        "RUST-FILETIME-OCTAGRAM-BACKEND-PASS: checked "
        f"{len(packages)} source tree(s) ({', '.join(versions)}); "
        "OCTAGRAM selects utimensat"
    )
    if changed == 0:
        print("filetime OCTAGRAM patches were already present")
    return 0


if __name__ == "__main__":
    sys.exit(main())

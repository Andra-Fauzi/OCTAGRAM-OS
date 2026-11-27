#!/usr/bin/env python3
"""Patch locked tempfile versions to use std filesystem calls on OCTAGRAM.

Rust's workspace and Cargo's submodule have separate lockfiles:
tempfile 3.23.0 and 3.27.0. Both use rustix on ordinary Unix targets, but
OCTAGRAM can perform these operations through its POSIX std filesystem PAL.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

TEMPFILE_RUSTIX_VERSIONS = {
    "3.23.0": "1.0.0",
    "3.27.0": "1.1.4",
}

OLD_MANIFEST_HEADER = (
    "[target.'cfg(any(unix, target_os = \"wasi\"))'.dependencies.rustix]"
)
NEW_MANIFEST_HEADER = (
    "[target.'cfg(all(any(unix, target_os = \"wasi\"), "
    "not(target_os = \"octagram\")))'.dependencies.rustix]"
)

OLD_IMPORTS_323 = """#[cfg(not(target_os = "redox"))]
use {
    rustix::fs::{rename, unlink},
    std::fs::hard_link,
};
"""
NEW_IMPORTS_323 = """#[cfg(not(target_os = "redox"))]
use std::fs::hard_link;

#[cfg(all(not(target_os = "redox"), not(target_os = "octagram")))]
use rustix::fs::{rename, unlink};

// OCTAGRAM std::fs already dispatches to the POSIX rename/unlink ABI.
#[cfg(target_os = "octagram")]
fn rename(old_path: &Path, new_path: &Path) -> io::Result<()> {
    fs::rename(old_path, new_path)
}

#[cfg(target_os = "octagram")]
fn unlink(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}
"""

OLD_IMPORTS_327 = """use {
    rustix::fs::{rename, unlink},
    std::fs::hard_link,
};
"""
NEW_IMPORTS_327 = """use std::fs::hard_link;

#[cfg(not(target_os = "octagram"))]
use rustix::fs::{rename, unlink};

// OCTAGRAM std::fs already dispatches to the POSIX rename/unlink ABI.
#[cfg(target_os = "octagram")]
fn rename(old_path: &Path, new_path: &Path) -> io::Result<()> {
    fs::rename(old_path, new_path)
}

#[cfg(target_os = "octagram")]
fn unlink(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}
"""

REOPEN_327_OLD = """pub fn reopen(file: &File, path: &Path) -> io::Result<File> {
    let new_file = OpenOptions::new().read(true).write(true).open(path)?;
    let old_meta = rustix::fs::fstat(file)?;
    let new_meta = rustix::fs::fstat(&new_file)?;
    if old_meta.st_dev != new_meta.st_dev || old_meta.st_ino != new_meta.st_ino {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "original tempfile has been replaced",
        ));
    }
    Ok(new_file)
}
"""

REOPEN_327_NEW = """pub fn reopen(file: &File, path: &Path) -> io::Result<File> {
    use std::os::unix::fs::MetadataExt;

    let new_file = OpenOptions::new().read(true).write(true).open(path)?;
    let old_meta = file.metadata()?;
    let new_meta = new_file.metadata()?;
    if old_meta.dev() != new_meta.dev() || old_meta.ino() != new_meta.ino() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "original tempfile has been replaced",
        ));
    }
    Ok(new_file)
}
"""


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    unix_source = package / "src" / "file" / "imp" / "unix.rs"
    if not manifest.is_file() or not unix_source.is_file():
        raise SystemExit(f"incomplete tempfile source tree: {package}")

    metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
    version = metadata.get("package", {}).get("version")
    expected_rustix = TEMPFILE_RUSTIX_VERSIONS.get(version)
    if expected_rustix is None:
        raise SystemExit(
            f"unexpected tempfile version in {manifest}: {version!r}; "
            f"expected one of {', '.join(TEMPFILE_RUSTIX_VERSIONS)}"
        )

    manifest_text = manifest.read_text(encoding="utf-8")
    if NEW_MANIFEST_HEADER in manifest_text:
        manifest_new = manifest_text
        dependency_start = manifest_new.index(NEW_MANIFEST_HEADER)
        dependency_block = manifest_new[dependency_start:]
    else:
        if manifest_text.count(OLD_MANIFEST_HEADER) != 1:
            raise SystemExit(
                f"expected one normalized Unix/WASI rustix dependency table "
                f"in {manifest}; refusing an unreviewed source layout"
            )
        dependency_start = manifest_text.index(OLD_MANIFEST_HEADER)
        dependency_block = manifest_text[dependency_start:]
        if expected_rustix not in dependency_block or 'features = ["fs"]' not in dependency_block:
            raise SystemExit(
                f"unexpected rustix dependency version/features in {manifest}; "
                f"expected rustix {expected_rustix} with fs"
            )
        manifest_new = manifest_text.replace(
            OLD_MANIFEST_HEADER, NEW_MANIFEST_HEADER, 1
        )

    if expected_rustix not in dependency_block or 'features = ["fs"]' not in dependency_block:
        raise SystemExit(
            f"patched rustix dependency in {manifest} does not match expected "
            f"version {expected_rustix} and fs feature"
        )

    source_text = unix_source.read_text(encoding="utf-8")
    if version == "3.23.0":
        old_imports, new_imports = OLD_IMPORTS_323, NEW_IMPORTS_323
    else:
        old_imports, new_imports = OLD_IMPORTS_327, NEW_IMPORTS_327

    if new_imports in source_text:
        source_new = source_text
    else:
        if source_text.count(old_imports) != 1:
            raise SystemExit(
                f"expected one known rustix import block in {unix_source}; "
                "refusing an unreviewed source layout"
            )
        if "use std::path::Path;" not in source_text:
            raise SystemExit(f"Path import missing in {unix_source}")
        source_new = source_text.replace(old_imports, new_imports, 1)

    if version == "3.27.0":
        if REOPEN_327_NEW not in source_new:
            if source_new.count(REOPEN_327_OLD) != 1:
                raise SystemExit(
                    f"expected one rustix fstat-based reopen implementation in {unix_source}"
                )
            source_new = source_new.replace(REOPEN_327_OLD, REOPEN_327_NEW, 1)

    changed = manifest_new != manifest_text or source_new != source_text
    if changed:
        manifest.write_text(manifest_new, encoding="utf-8")
        unix_source.write_text(source_new, encoding="utf-8")
        (package / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")
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

    packages = []
    for version in TEMPFILE_RUSTIX_VERSIONS:
        packages.extend(sorted(registry_root.glob(f"*/tempfile-{version}")))
    if not packages:
        raise SystemExit(
            "tempfile 3.23.0/3.27.0 was not found below "
            f"{registry_root}; run cargo fetch before applying the OCTAGRAM patch"
        )

    changed = sum(int(patch_package(package)) for package in packages)
    found_versions = sorted({p.name.removeprefix("tempfile-") for p in packages})
    print(
        "RUST-TEMPFILE-OCTAGRAM-BACKEND-PASS: checked "
        f"{len(packages)} source tree(s) ({', '.join(found_versions)}); "
        "rustix excluded for OCTAGRAM"
    )
    if changed == 0:
        print("tempfile OCTAGRAM patches were already present")
    return 0


if __name__ == "__main__":
    sys.exit(main())

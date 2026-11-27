#!/usr/bin/env python3
"""Remove tempfile's rustix dependency on OCTAGRAM using equivalent std APIs.

For OCTAGRAM, tempfile only uses rustix for rename/unlink in its Unix backend.
The std filesystem APIs already provide the required POSIX operations, so this
avoids compiling rustix's much broader, non-portable libc API surface.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

TEMPFILE_VERSION = "3.23.0"

OLD_MANIFEST_HEADER = "[target.'cfg(any(unix, target_os = \"wasi\"))'.dependencies]"
NEW_MANIFEST_HEADER = (
    "[target.'cfg(all(any(unix, target_os = \"wasi\"), "
    "not(target_os = \"octagram\")))'.dependencies]"
)
RUSTIX_DEPENDENCY = 'rustix = { version = "1.0.0", features = ["fs"] }'

OLD_IMPORTS = """#[cfg(not(target_os = "redox"))]
use {
    rustix::fs::{rename, unlink},
    std::fs::hard_link,
};
"""
NEW_IMPORTS = """#[cfg(not(target_os = "redox"))]
use std::fs::hard_link;

#[cfg(all(not(target_os = "redox"), not(target_os = "octagram")))]
use rustix::fs::{rename, unlink};

// OCTAGRAM libc/runtime already backs std::fs::rename/remove_file. Keep
// tempfile's no-clobber path (hard_link + unlink) and overwrite path semantics
// without pulling rustix's generic Linux/BSD syscall surface into this target.
#[cfg(target_os = "octagram")]
fn rename(old_path: &Path, new_path: &Path) -> io::Result<()> {
    fs::rename(old_path, new_path)
}

#[cfg(target_os = "octagram")]
fn unlink(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}
"""


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    unix_source = package / "src" / "file" / "imp" / "unix.rs"
    if not manifest.is_file() or not unix_source.is_file():
        raise SystemExit(f"incomplete tempfile {TEMPFILE_VERSION} source tree: {package}")

    metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
    version = metadata.get("package", {}).get("version")
    if version != TEMPFILE_VERSION:
        raise SystemExit(
            f"unexpected tempfile version in {manifest}: {version!r}; "
            f"expected {TEMPFILE_VERSION}"
        )

    manifest_text = manifest.read_text(encoding="utf-8")
    if NEW_MANIFEST_HEADER in manifest_text:
        if RUSTIX_DEPENDENCY not in manifest_text:
            raise SystemExit(f"tempfile rustix dependency entry missing in {manifest}")
        manifest_new = manifest_text
    else:
        if manifest_text.count(OLD_MANIFEST_HEADER) != 1:
            raise SystemExit(
                f"expected one Unix/WASI dependency table in {manifest}; "
                "refusing an unreviewed source layout"
            )
        if manifest_text.count(RUSTIX_DEPENDENCY) != 1:
            raise SystemExit(
                f"expected one rustix fs dependency in {manifest}; "
                "refusing an unreviewed source layout"
            )
        manifest_new = manifest_text.replace(
            OLD_MANIFEST_HEADER, NEW_MANIFEST_HEADER, 1
        )

    source_text = unix_source.read_text(encoding="utf-8")
    if NEW_IMPORTS in source_text:
        source_new = source_text
    else:
        if source_text.count(OLD_IMPORTS) != 1:
            raise SystemExit(
                f"expected one rustix import block in {unix_source}; "
                "refusing an unreviewed source layout"
            )
        if "use std::path::Path;" not in source_text:
            raise SystemExit(f"Path import missing in {unix_source}")
        source_new = source_text.replace(OLD_IMPORTS, NEW_IMPORTS, 1)

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

    packages = sorted(registry_root.glob(f"*/tempfile-{TEMPFILE_VERSION}"))
    if not packages:
        raise SystemExit(
            f"tempfile {TEMPFILE_VERSION} was not found below {registry_root}; "
            "run cargo fetch before applying the OCTAGRAM patch"
        )

    changed = sum(int(patch_package(package)) for package in packages)
    print(
        f"RUST-TEMPFILE-OCTAGRAM-BACKEND-PASS: checked {len(packages)} source tree(s); "
        f"patched {changed}; rustix excluded for OCTAGRAM"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

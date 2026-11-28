#!/usr/bin/env python3
"""Make nix 0.30.1 an empty crate for the OCTAGRAM host build.

nix reaches the OCTAGRAM dependency graph only through ctrlc's Unix backend,
and that backend is replaced for OCTAGRAM (see patch_ctrlc_octagram.py)
because the guest libc has sigaction() but no per-thread signal APIs. The
crate still has to build, and its Linux/BDS cfg tables do not know
target_os = "octagram", so this patch compiles it as an empty crate there.
Host builds keep the real nix.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

VERSION = "0.30.1"
CANONICAL = '#![cfg(all(unix, not(target_os = "octagram")))]'
REPLACEMENTS = (
    ('#![cfg(unix)]', CANONICAL),
    ('#![cfg(all(unix, not(target_os = "octagram")))]', CANONICAL),
)


def find_packages(root: Path, name: str) -> list[Path]:
    """Locate a crate source under either a registry index dir or its parent."""
    if not root.is_dir():
        raise SystemExit(f"Cargo registry source root is missing: {root}")
    matches = [path for path in root.iterdir() if path.is_dir() and path.name == name]
    if matches:
        return sorted(matches)
    for index in root.iterdir():
        if index.is_dir():
            matches.extend(
                path for path in index.iterdir() if path.is_dir() and path.name == name
            )
    return sorted(matches)


def patch_lib(path: Path) -> bool:
    source = path.read_text(encoding="utf-8")
    if CANONICAL in source:
        return False
    if source.count("#![cfg(unix)]") != 1:
        raise SystemExit(f"unexpected nix {VERSION} crate cfg layout: {path}")
    path.write_text(source.replace(*REPLACEMENTS[0], 1), encoding="utf-8")
    return True


def verify(package: Path) -> None:
    lib = (package / "src/lib.rs").read_text(encoding="utf-8")
    if CANONICAL not in lib:
        raise SystemExit(f"nix crate cfg is not gated for OCTAGRAM: {package}")
    if "#![cfg(unix)]" in lib:
        raise SystemExit(f"nix crate cfg still selects OCTAGRAM: {package}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry-root", required=True, type=Path)
    args = parser.parse_args()

    packages = find_packages(args.registry_root, f"nix-{VERSION}")
    if not packages:
        raise SystemExit(f"nix {VERSION} is missing from Cargo's registry cache")

    patched = 0
    for package in packages:
        manifest = package / "Cargo.toml"
        lib = package / "src/lib.rs"
        if not manifest.is_file() or not lib.is_file():
            raise SystemExit(f"incomplete nix {VERSION} registry source: {package}")
        if not re.search(
            rf'(?m)^version = "{re.escape(VERSION)}"$',
            manifest.read_text(encoding="utf-8"),
        ):
            raise SystemExit(f"unexpected nix package version in {manifest}")

        # Cargo treats a missing or empty `.cargo-ok` as an interrupted
        # extraction and re-extracts the crate, discarding these edits.
        cargo_ok = package / ".cargo-ok"
        if not cargo_ok.is_file() or not cargo_ok.read_text(encoding="utf-8").strip():
            cargo_ok.write_text('{"v":1}', encoding="utf-8")

        patched += int(patch_lib(lib))
        verify(package)

    print(
        "RUST-NIX-OCTAGRAM-STUB-PASS: "
        f"checked {len(packages)} nix {VERSION} source tree(s); patched {patched}; "
        "OCTAGRAM compiles an empty nix, host builds keep the real crate"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

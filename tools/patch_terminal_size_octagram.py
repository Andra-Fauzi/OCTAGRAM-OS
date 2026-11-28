#!/usr/bin/env python3
"""Disable terminal_size's rustix termios path for OCTAGRAM."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

VERSION = "0.4.4"
OLD_DEP = '[target."cfg(unix)".dependencies.rustix]'
NEW_DEP = "[target.'cfg(all(unix, not(target_os = \"octagram\")))'.dependencies.rustix]"
OLD_RUSTIX_CFG = '#[cfg(not(any(target_os = "horizon", target_os = "vita")))]'
NEW_RUSTIX_CFG = '#[cfg(not(any(target_os = "horizon", target_os = "vita", target_os = "octagram")))]'
OLD_FALLBACK_CFG = '#[cfg(any(target_os = "horizon", target_os = "vita"))]'
NEW_FALLBACK_CFG = '#[cfg(any(target_os = "horizon", target_os = "vita", target_os = "octagram"))]'


def replace_once(source: str, old: str, new: str, path: Path) -> tuple[str, bool]:
    count = source.count(old)
    if count == 1:
        return source.replace(old, new, 1), True
    if count == 0 and new in source:
        return source, False
    raise SystemExit(f"unexpected terminal_size {VERSION} source layout in {path}: {old!r} count={count}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry-root", required=True, type=Path)
    args = parser.parse_args()

    packages = sorted(args.registry_root.glob(f"*/terminal_size-{VERSION}"))
    if not packages:
        raise SystemExit(f"terminal_size {VERSION} is missing from Cargo's registry cache")

    patched = 0
    for package in packages:
        manifest = package / "Cargo.toml"
        unix_source = package / "src/unix.rs"
        marker = package / ".cargo-ok"
        if not manifest.is_file() or not unix_source.is_file() or not marker.is_file():
            raise SystemExit(f"incomplete terminal_size {VERSION} registry source: {package}")
        if not re.search(
            rf'(?m)^version = "{re.escape(VERSION)}"$',
            manifest.read_text(encoding="utf-8"),
        ):
            raise SystemExit(f"unexpected terminal_size package version in {manifest}")

        manifest_text = manifest.read_text(encoding="utf-8")
        manifest_text, changed_manifest = replace_once(manifest_text, OLD_DEP, NEW_DEP, manifest)
        unix_text = unix_source.read_text(encoding="utf-8")
        unix_text, changed_a = replace_once(unix_text, OLD_RUSTIX_CFG, NEW_RUSTIX_CFG, unix_source)
        unix_text, changed_b = replace_once(unix_text, OLD_FALLBACK_CFG, NEW_FALLBACK_CFG, unix_source)

        if changed_manifest:
            manifest.write_text(manifest_text, encoding="utf-8")
        if changed_a or changed_b:
            unix_source.write_text(unix_text, encoding="utf-8")
        patched += int(changed_manifest or changed_a or changed_b)

    print(
        "RUST-TERMINAL-SIZE-OCTAGRAM-BACKEND-PASS: "
        f"checked {len(packages)} terminal_size {VERSION} source tree(s); "
        f"patched {patched}; terminal size safely reports unavailable on OCTAGRAM"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

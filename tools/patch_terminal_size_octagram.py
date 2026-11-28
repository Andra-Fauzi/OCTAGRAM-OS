#!/usr/bin/env python3
"""Use terminal_size's no-size fallback on OCTAGRAM (no rustix termios)."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

VERSION = "0.4.4"
OLD_DEP = '[target."cfg(unix)".dependencies.rustix]'
NEW_DEP = "[target.'cfg(all(unix, not(target_os = \"octagram\")))'.dependencies.rustix]"
FUNCTION = re.compile(
    r"(?ms)^pub fn terminal_size_of<Fd: AsFd>\(fd: Fd\) -> Option<\(Width, Height\)> \{\n.*?^\}\n"
)
FALLBACK = """#[cfg(target_os = "octagram")]
pub fn terminal_size_of<Fd: AsFd>(_fd: Fd) -> Option<(Width, Height)> {
    None
}
"""


def patch_manifest(path: Path) -> bool:
    source = path.read_text(encoding="utf-8")
    count = source.count(OLD_DEP)
    if count == 1:
        source = source.replace(OLD_DEP, NEW_DEP, 1)
        path.write_text(source, encoding="utf-8")
        return True
    if count == 0 and NEW_DEP in source:
        return False
    raise SystemExit(f"unexpected terminal_size {VERSION} Cargo.toml layout: {path}")


def patch_unix(path: Path) -> bool:
    source = path.read_text(encoding="utf-8")
    cfg_prefix = '#[cfg(not(target_os = "octagram"))]\npub fn terminal_size_of<Fd: AsFd>(fd: Fd)'
    if cfg_prefix in source and FALLBACK in source:
        return False

    # 0.4.4 has an unguarded rustix implementation. Some later patch levels
    # already carry Horizon/Vita fallbacks; extend those if encountered.
    old_horizon = '#[cfg(not(any(target_os = "horizon", target_os = "vita")))]'
    old_fallback = '#[cfg(any(target_os = "horizon", target_os = "vita"))]'
    if old_horizon in source and old_fallback in source:
        source = source.replace(
            old_horizon,
            '#[cfg(not(any(target_os = "horizon", target_os = "vita", target_os = "octagram")))]',
            1,
        )
        source = source.replace(
            old_fallback,
            '#[cfg(any(target_os = "horizon", target_os = "vita", target_os = "octagram"))]',
            1,
        )
        path.write_text(source, encoding="utf-8")
        return True

    matches = list(FUNCTION.finditer(source))
    if len(matches) != 1:
        raise SystemExit(
            f"expected one terminal_size_of rustix implementation in {path}, found {len(matches)}"
        )
    match = matches[0]
    body = match.group(0)
    if "rustix::termios::{isatty, tcgetwinsize}" not in body:
        raise SystemExit(f"terminal_size_of no longer uses the expected rustix termios API: {path}")

    guarded = '#[cfg(not(target_os = "octagram"))]\n' + body
    updated = source[: match.start()] + guarded + FALLBACK + source[match.end() :]
    path.write_text(updated, encoding="utf-8")
    return True


def verify(package: Path) -> None:
    manifest = (package / "Cargo.toml").read_text(encoding="utf-8")
    source = (package / "src/unix.rs").read_text(encoding="utf-8")
    if OLD_DEP in manifest or NEW_DEP not in manifest:
        raise SystemExit(f"terminal_size rustix dependency is not excluded for OCTAGRAM: {package}")
    if '#[cfg(not(target_os = "octagram"))]' not in source:
        raise SystemExit(f"terminal_size rustix implementation is not cfg-gated: {package}")
    if FALLBACK not in source:
        raise SystemExit(f"terminal_size OCTAGRAM fallback is missing: {package}")


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

        changed_manifest = patch_manifest(manifest)
        changed_source = patch_unix(unix_source)
        verify(package)
        patched += int(changed_manifest or changed_source)

    print(
        "RUST-TERMINAL-SIZE-OCTAGRAM-BACKEND-PASS: "
        f"checked {len(packages)} terminal_size {VERSION} source tree(s); "
        f"patched {patched}; terminal size safely reports unavailable on OCTAGRAM"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

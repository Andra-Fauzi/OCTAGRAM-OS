#!/usr/bin/env python3
"""Patch socket2's Unix msghdr iovlen type for the OCTAGRAM ABI."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

VERSIONS = ("0.6.4", "0.6.5")
IOVLEN_CFG = re.compile(
    r"(?ms)^#\[cfg\(any\(\n"
    r"(?P<body>(?:(?!^#\[cfg).)*?)"
    r"^\)\)\]\ntype IovLen = usize;"
)
ANDROID_CFG = re.compile(r'(?m)^(?P<indent>[ \t]*)target_os = "android",[ \t]*$')


def patch_source(path: Path) -> bool:
    source = path.read_text(encoding="utf-8")
    matches = list(IOVLEN_CFG.finditer(source))
    if len(matches) != 1:
        raise SystemExit(
            f"expected one socket2 usize IovLen cfg block in {path}, found {len(matches)}"
        )

    match = matches[0]
    body = match.group("body")
    if 'target_os = "octagram"' in body:
        return False

    updated_body, count = ANDROID_CFG.subn(
        lambda found: (
            f'{found.group("indent")}target_os = "android",\n'
            f'{found.group("indent")}target_os = "octagram",'
        ),
        body,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"unsupported socket2 IovLen cfg layout in {path}")

    updated = source[: match.start("body")] + updated_body + source[match.end("body") :]
    path.write_text(updated, encoding="utf-8")
    return True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry-root", required=True, type=Path)
    args = parser.parse_args()

    checked = 0
    patched = 0
    for version in VERSIONS:
        matches = sorted(args.registry_root.glob(f"*/socket2-{version}"))
        if not matches:
            raise SystemExit(f"socket2 {version} is missing from Cargo's registry cache")
        for package in matches:
            manifest = package / "Cargo.toml"
            source = package / "src/sys/unix.rs"
            if not manifest.is_file() or not source.is_file():
                raise SystemExit(f"incomplete socket2 {version} registry source: {package}")
            if not re.search(
                rf'(?m)^version = "{re.escape(version)}"$',
                manifest.read_text(encoding="utf-8"),
            ):
                raise SystemExit(f"unexpected socket2 package version in {manifest}")
            patched += int(patch_source(source))
            checked += 1
            marker = package / ".cargo-ok"
            if not marker.is_file():
                raise SystemExit(f"Cargo registry extraction marker is missing: {marker}")

    print(
        "RUST-SOCKET2-OCTAGRAM-BACKEND-PASS: "
        f"checked {checked} source tree(s) (0.6.4, 0.6.5); "
        f"patched {patched}; OCTAGRAM uses size_t msghdr.msg_iovlen"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

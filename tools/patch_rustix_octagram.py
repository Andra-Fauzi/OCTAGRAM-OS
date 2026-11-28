#!/usr/bin/env python3
"""Apply the OCTAGRAM cfg compatibility patch to rustix 1.1.6.

The cfg adaptation is based on the rustix 1.1.6 Horizon fallback work in
KorinOS (Stéphane Wegener and contributors, commit c619001dd700d5f5e8560d8a4869c60a4a4924e8),
with the custom target changed to OCTAGRAM.
"""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = "1.1.6"
PATCH_FILE = ROOT / "patches" / "rustix-1.1.6-octagram.patch"
MARKER = ".octagram-patched"
VERIFY_FILES = (
    "src/backend/libc/io/errno.rs",
    "src/backend/libc/fs/types.rs",
    "src/backend/libc/fs/syscalls.rs",
    "src/backend/libc/io/syscalls.rs",
    "src/ioctl/mod.rs",
)


def verify(package: Path) -> None:
    for relative in VERIFY_FILES:
        path = package / relative
        if not path.is_file():
            raise SystemExit(f"rustix patch verification file is missing: {path}")
        source = path.read_text(encoding="utf-8")
        if 'target_os = "octagram"' not in source:
            raise SystemExit(f"rustix OCTAGRAM cfg was not installed in {path}")
        if 'target_os = "korin"' in source:
            raise SystemExit(f"unconverted Korin target cfg remains in {path}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry-root", required=True, type=Path)
    args = parser.parse_args()

    if not PATCH_FILE.is_file():
        raise SystemExit(f"rustix OCTAGRAM patch file is missing: {PATCH_FILE}")
    patch_text = PATCH_FILE.read_text(encoding="utf-8")
    if 'target_os = "korin"' in patch_text or 'target_os = "octagram"' not in patch_text:
        raise SystemExit("rustix patch must contain OCTAGRAM cfgs and no Korin cfgs")

    packages = sorted(args.registry_root.glob(f"*/rustix-{VERSION}"))
    if not packages:
        raise SystemExit(f"rustix {VERSION} is missing from Cargo's registry cache")

    patched = 0
    for package in packages:
        manifest = package / "Cargo.toml"
        if not manifest.is_file() or not re.search(
            rf'(?m)^version = "{re.escape(VERSION)}"$',
            manifest.read_text(encoding="utf-8"),
        ):
            raise SystemExit(f"unexpected rustix package version in {manifest}")

        marker = package / MARKER
        if marker.is_file():
            verify(package)
            continue

        result = subprocess.run(
            ["patch", "-p1", "--batch", "--forward", "--fuzz=0"],
            cwd=package,
            input=patch_text,
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            raise SystemExit(
                f"failed to apply rustix OCTAGRAM patch to {package}\n"
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )

        verify(package)
        marker.write_text("rustix 1.1.6 OCTAGRAM cfg patch applied\n", encoding="utf-8")
        patched += 1

    print(
        "RUST-RUSTIX-OCTAGRAM-BACKEND-PASS: "
        f"checked {len(packages)} rustix {VERSION} source tree(s); patched {patched}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

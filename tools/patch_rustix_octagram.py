#!/usr/bin/env python3
"""Apply the OCTAGRAM cfg compatibility patch to the locked rustix releases.

The patch was authored against rustix 1.1.6 and its unchanged patch contexts
were checked against 1.1.5 as well. Rust's compiler and Cargo sub-workspaces
can lock different compatible rustix releases, so both must be patched.
"""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSIONS = ("1.1.5", "1.1.6")
PATCH_FILE = ROOT / "patches" / "rustix-1.1.6-octagram.patch"
MARKER = ".octagram-patched"
VERIFY_FILES = (
    "src/backend/libc/io/errno.rs",
    "src/backend/libc/fs/types.rs",
    "src/backend/libc/fs/syscalls.rs",
    "src/backend/libc/io/syscalls.rs",
    "src/ioctl/mod.rs",
)


def verify_manifest(package: Path, version: str) -> None:
    manifest = package / "Cargo.toml"
    if not manifest.is_file() or not re.search(
        rf'(?m)^version = "{re.escape(version)}"

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry-root", required=True, type=Path)
    args = parser.parse_args()

    if not PATCH_FILE.is_file():
        raise SystemExit(f"rustix OCTAGRAM patch file is missing: {PATCH_FILE}")
    patch_text = PATCH_FILE.read_text(encoding="utf-8")
    if 'target_os = "korin"' in patch_text or 'target_os = "octagram"' not in patch_text:
        raise SystemExit("rustix patch must contain OCTAGRAM cfgs and no Korin cfgs")

    checked = 0
    patched = 0
    for version in VERSIONS:
        packages = sorted(args.registry_root.glob(f"*/rustix-{version}"))
        if not packages:
            raise SystemExit(
                f"rustix {version} is missing from Cargo's registry cache; "
                "fetch every locked rustix version before applying the OCTAGRAM patch"
            )

        for package in packages:
            verify_manifest(package, version)
            marker = package / MARKER
            if marker.is_file():
                ensure_octagram_cfg(package)
                verify(package, version)
                checked += 1
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

            ensure_octagram_cfg(package)
            verify(package, version)
            marker.write_text(
                f"rustix {version} OCTAGRAM cfg patch applied\n",
                encoding="utf-8",
            )
            checked += 1
            patched += 1

    print(
        "RUST-RUSTIX-OCTAGRAM-BACKEND-PASS: "
        f"checked {checked} rustix source tree(s) across versions "
        f"{', '.join(VERSIONS)}; patched {patched}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
,
        manifest.read_text(encoding="utf-8"),
    ):
        raise SystemExit(f"unexpected rustix package version in {manifest}")


def ensure_octagram_cfg(package: Path) -> None:
    # rustix 1.1.5 and 1.1.6 have small source-context differences in errno.rs.
    # The intended compatibility change is identical: OCTAGRAM shares the
    # Horizon libc errno implementation. Normalize any remaining Horizon cfg
    # in the exact files covered by the upstream patch.
    for relative in VERIFY_FILES:
        path = package / relative
        if not path.is_file():
            raise SystemExit(f"rustix patch verification file is missing: {path}")
        source = path.read_text(encoding="utf-8")
        normalized = re.sub(
            r'(?<!any\\()target_os = "horizon"',
            'any(target_os = "horizon", target_os = "octagram")',
            source,
        )
        if normalized != source:
            path.write_text(normalized, encoding="utf-8")


def verify(package: Path, version: str) -> None:
    verify_manifest(package, version)
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

    checked = 0
    patched = 0
    for version in VERSIONS:
        packages = sorted(args.registry_root.glob(f"*/rustix-{version}"))
        if not packages:
            raise SystemExit(
                f"rustix {version} is missing from Cargo's registry cache; "
                "fetch every locked rustix version before applying the OCTAGRAM patch"
            )

        for package in packages:
            verify(package, version)
            marker = package / MARKER
            if marker.is_file():
                checked += 1
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

            verify(package, version)
            marker.write_text(
                f"rustix {version} OCTAGRAM cfg patch applied\n",
                encoding="utf-8",
            )
            checked += 1
            patched += 1

    print(
        "RUST-RUSTIX-OCTAGRAM-BACKEND-PASS: "
        f"checked {checked} rustix source tree(s) across versions "
        f"{', '.join(VERSIONS)}; patched {patched}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

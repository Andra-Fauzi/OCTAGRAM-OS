#!/usr/bin/env python3
"""Apply the OCTAGRAM cfg compatibility patch to locked rustix releases.

Rustix 1.1.5 has two source-context differences in Horizon cfg lists. The
maintained patch applies all other changes; those two narrowly-validated
rejects are normalized explicitly before the backend is verified.
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
    "src/backend/libc/thread/syscalls.rs",
    "src/thread/clock.rs",
    "src/ioctl/mod.rs",
)
EXPECTED_1_1_5_REJECTS = {
    "src/backend/libc/thread/syscalls.rs",
    "src/thread/clock.rs",
}


def verify_manifest(package: Path, version: str) -> None:
    manifest = package / "Cargo.toml"
    if not manifest.is_file() or not re.search(
        rf'(?m)^version = "{re.escape(version)}"$',
        manifest.read_text(encoding="utf-8"),
    ):
        raise SystemExit(f"unexpected rustix package version in {manifest}")


def ensure_octagram_cfg(package: Path) -> None:
    # OCTAGRAM deliberately shares Horizon's libc implementation for these
    # rustix interfaces. Normalize only the exact source files that must carry
    # the OCTAGRAM target cfg, including the two 1.1.5 context-drift files.
    for relative in VERIFY_FILES:
        path = package / relative
        if not path.is_file():
            raise SystemExit(f"rustix patch verification file is missing: {path}")
        source = path.read_text(encoding="utf-8")
        normalized = re.sub(
            r'(?<!any\()target_os = "horizon"',
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


def validate_expected_rejects(package: Path, version: str) -> None:
    rejects = sorted(package.rglob("*.rej"))
    if not rejects:
        return

    relative_rejects = {str(path.relative_to(package)) for path in rejects}
    if version != "1.1.5" or relative_rejects != EXPECTED_1_1_5_REJECTS:
        raise SystemExit(
            f"unexpected rustix patch rejects for {version}: "
            f"{sorted(relative_rejects)}"
        )

    expected_removed = 'target_os = "horizon",'
    expected_added = 'any(target_os = "horizon", target_os = "octagram"),'
    for reject in rejects:
        changed = [
            line[1:].strip()
            for line in reject.read_text(encoding="utf-8").splitlines()
            if line.startswith(("+", "-")) and not line.startswith(("+++", "---"))
        ]
        if (
            not changed
            or len(changed) % 2 != 0
            or any(changed[index] != expected_removed for index in range(0, len(changed), 2))
            or any(changed[index] != expected_added for index in range(1, len(changed), 2))
        ):
            raise SystemExit(
                f"rustix reject contains changes beyond the expected Horizon cfg "
                f"normalization: {reject}"
            )

    for reject in rejects:
        reject.unlink()


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
            if not marker.is_file():
                result = subprocess.run(
                    ["patch", "-p1", "--batch", "--forward", "--fuzz=0"],
                    cwd=package,
                    input=patch_text,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                if result.returncode != 0 and not result.stdout:
                    raise SystemExit(
                        f"failed to apply rustix OCTAGRAM patch to {package}\n"
                        f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
                    )

                # For 1.1.5, only the known Horizon cfg hunks differ. Rejects
                # are accepted only when their files and every changed line
                # match that narrow, audited compatibility adjustment.
                validate_expected_rejects(package, version)
                marker.write_text(
                    f"rustix {version} OCTAGRAM cfg patch applied\n",
                    encoding="utf-8",
                )
                patched += 1

            # Repairs the two known 1.1.5 cfg-context differences, then
            # validates all required backend files before compilation.
            ensure_octagram_cfg(package)
            verify(package, version)
            checked += 1

    print(
        "RUST-RUSTIX-OCTAGRAM-BACKEND-PASS: "
        f"checked {checked} rustix source tree(s) across versions "
        f"{', '.join(VERSIONS)}; patched {patched}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

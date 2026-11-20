#!/usr/bin/env python3
"""Tulis manifest artefak build OCTAGRAM-OS.

Manifest mencatat setiap artefak yang dihasilkan build (kernel, ISO, userspace,
target Rust, dan file boot Limine) beserta ukuran dan SHA-256, plus metadata
build (commit git, triple target, versi toolchain). Tujuannya:

* build clean dapat diperiksa ulang: `--check` gagal bila ada artefak yang
  hilang atau berubah tanpa manifest diperbarui;
* status build host dapat dilaporkan tanpa menyentuh artefak.

Pemakaian:
    python3 tools/build_manifest.py            # tulis build/manifest.json
    python3 tools/build_manifest.py --check    # verifikasi terhadap disk
    python3 tools/build_manifest.py --stdout   # cetak ke stdout
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT = ROOT / "build" / "manifest.json"

KERNEL_CANDIDATES = (
    "kernel/kernel",
    "kernel/target/x86_64-unknown-none/release/octagram-kernel",
)
ISO_CANDIDATES = ("OCTAGRAM-x86_64.iso", "OCTAGRAM-aarch64.iso")
BOOT_FILES = (
    "limine/limine-bios.sys",
    "limine/limine-bios-cd.bin",
    "limine/limine-uefi-cd.bin",
    "limine.conf",
)
USERLAND_GLOBS = ("user/build/*.elf",)
RUST_TARGET_GLOBS = ("user/rust/build/*.elf",)


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(command: list[str]) -> str | None:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if result.returncode != 0:
        return None
    return result.stdout.strip().splitlines()[0] if result.stdout.strip() else None


def git_metadata() -> dict[str, object]:
    commit = run(["git", "rev-parse", "HEAD"])
    branch = run(["git", "rev-parse", "--abbrev-ref", "HEAD"])
    status = run(["git", "status", "--porcelain"])
    return {
        "commit": commit,
        "branch": branch,
        "dirty": None if status is None else bool(status),
    }


def toolchain_metadata() -> dict[str, str | None]:
    return {
        "rustc": run(["rustc", "+nightly", "--version"]),
        "cargo": run(["cargo", "+nightly", "--version"]),
        "cc": run(["cc", "--version"]),
        "ld_lld": run(["ld.lld", "--version"]),
        "nasm": run(["nasm", "-v"]),
        "xorriso": run(["xorriso", "-version"]),
        "python": platform.python_version(),
    }


def first_existing(candidates: tuple[str, ...]) -> Path | None:
    for candidate in candidates:
        path = ROOT / candidate
        if path.is_file():
            return path
    return None


def collect() -> dict[str, object]:
    artifacts: list[dict[str, object]] = []

    def add(path: Path, kind: str) -> None:
        artifacts.append(
            {
                "path": str(path.relative_to(ROOT)),
                "kind": kind,
                "size": path.stat().st_size,
                "sha256": sha256_of(path),
            }
        )

    kernel = first_existing(KERNEL_CANDIDATES)
    if kernel is not None:
        add(kernel, "kernel")

    iso = first_existing(ISO_CANDIDATES)
    if iso is not None:
        add(iso, "boot-image")

    for pattern, kind in (
        *((pattern, "userland") for pattern in USERLAND_GLOBS),
        *((pattern, "rust-target") for pattern in RUST_TARGET_GLOBS),
    ):
        for path in sorted(ROOT.glob(pattern)):
            add(path, kind)

    for name in BOOT_FILES:
        path = ROOT / name
        if path.is_file():
            add(path, "boot")

    artifacts.sort(key=lambda entry: (entry["kind"], entry["path"]))
    return {
        "generated_utc": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "git": git_metadata(),
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
        },
        "targets": {
            "kernel": "x86_64-unknown-none",
            "userland": "x86_64-octagram (freestanding ELF)",
            "rust_custom_target": "x86_64-unknown-octagram",
        },
        "toolchain": toolchain_metadata(),
        "artifacts": artifacts,
        "totals": {
            "artifacts": len(artifacts),
            "bytes": sum(int(entry["size"]) for entry in artifacts),
            "by_kind": {
                kind: sum(1 for entry in artifacts if entry["kind"] == kind)
                for kind in sorted({str(entry["kind"]) for entry in artifacts})
            },
        },
    }


def check(manifest: dict[str, object]) -> int:
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        print("MANIFEST-FAIL: manifest tidak memuat artefak")
        return 1

    failures = 0
    listed = set()
    for entry in artifacts:
        relative = str(entry["path"])
        listed.add(relative)
        path = ROOT / relative
        if not path.is_file():
            print(f"MANIFEST-FAIL: artefak hilang: {relative}")
            failures += 1
            continue
        size = path.stat().st_size
        if size != entry["size"]:
            print(
                f"MANIFEST-FAIL: ukuran berubah: {relative} "
                f"({entry['size']} -> {size})"
            )
            failures += 1
            continue
        if sha256_of(path) != entry["sha256"]:
            print(f"MANIFEST-FAIL: sha256 berubah: {relative}")
            failures += 1

    for pattern in USERLAND_GLOBS + RUST_TARGET_GLOBS:
        for path in ROOT.glob(pattern):
            relative = str(path.relative_to(ROOT))
            if relative not in listed:
                print(f"MANIFEST-FAIL: artefak tidak tercatat: {relative}")
                failures += 1

    if failures:
        print(f"MANIFEST-FAIL: {failures} masalah")
        return 1
    print(f"MANIFEST-PASS: {len(artifacts)} artefak cocok dengan manifest")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        default=str(DEFAULT_OUTPUT),
        help="lokasi manifest (default: build/manifest.json)",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verifikasi artefak di disk terhadap manifest yang ada",
    )
    parser.add_argument(
        "--stdout",
        action="store_true",
        help="cetak manifest ke stdout, jangan tulis file",
    )
    args = parser.parse_args(argv)

    output = Path(args.output)
    if not output.is_absolute():
        output = ROOT / output

    if args.check:
        if not output.is_file():
            print(f"MANIFEST-FAIL: manifest belum ada: {output.relative_to(ROOT)}")
            print("Jalankan `make manifest` lebih dulu.")
            return 1
        return check(json.loads(output.read_text(encoding="utf-8")))

    manifest = collect()
    text = json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    if args.stdout:
        sys.stdout.write(text)
        return 0

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(text, encoding="utf-8")
    totals = manifest["totals"]
    print(
        f"MANIFEST-WRITE: {output.relative_to(ROOT)} "
        f"({totals['artifacts']} artefak, {totals['bytes']} byte)"
    )
    for kind, count in totals["by_kind"].items():
        print(f"  {kind}: {count}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

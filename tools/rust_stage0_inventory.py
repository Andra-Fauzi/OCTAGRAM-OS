#!/usr/bin/env python3
"""Inventory the Linux Stage 0 toolchain needed to bootstrap an OCTAGRAM host toolchain.

This is deliberately an inventory, not a native-bootstrap test: the installed
rustc/Cargo still execute on the build machine. The generated report records
that distinction explicitly so CI cannot accidentally label cross-build tools
as native OCTAGRAM executables.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Sequence

TOOLCHAIN = os.environ.get("OCTAGRAM_RUST_TOOLCHAIN", "nightly")
ROOT = Path(__file__).resolve().parents[1]
TARGET_SPEC = ROOT / "user/rust/targets/x86_64-unknown-octagram.json"
KERNEL_TARGET = "x86_64-unknown-none"
OCTAGRAM_HOST_TARGET = "x86_64-unknown-octagram"

REQUIRED_TOOLS = ("rustc", "cargo", "rustup", "ld.lld", "as", "readelf", "nm", "make", "python3")


def fail(message: str) -> None:
    raise SystemExit(f"RUST-STAGE0-INVENTORY-FAIL: {message}")


def run(argv: Sequence[str]) -> str:
    try:
        result = subprocess.run(
            list(argv),
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except OSError as error:
        fail(f"could not execute {' '.join(argv)}: {error}")
    if result.returncode != 0:
        fail(
            f"{' '.join(argv)} exited {result.returncode}: "
            f"{(result.stderr or result.stdout).strip()}"
        )
    return result.stdout.strip()


def first_line(command: str) -> str:
    output = run((command, "--version"))
    return output.splitlines()[0] if output else "version unavailable"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="optional JSON report path (relative paths are relative to the current directory)",
    )
    args = parser.parse_args()

    missing = [name for name in REQUIRED_TOOLS if shutil.which(name) is None]
    if missing:
        fail("missing required Stage 0 tools: " + ", ".join(missing))

    try:
        target_spec = json.loads(TARGET_SPEC.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read OCTAGRAM target specification: {error}")

    expected = {
        "arch": "x86_64",
        "os": "octagram",
        "llvm-target": KERNEL_TARGET,
        "target-pointer-width": 64,
    }
    mismatches = [
        f"{key}={target_spec.get(key)!r} (expected {value!r})"
        for key, value in expected.items()
        if target_spec.get(key) != value
    ]
    if mismatches:
        fail("custom target specification mismatch: " + "; ".join(mismatches))

    rustc_verbose = run(("rustc", f"+{TOOLCHAIN}", "-vV"))
    host_line = next(
        (line.partition(":")[2].strip() for line in rustc_verbose.splitlines() if line.startswith("host:")),
        None,
    )
    if not host_line:
        fail(f"rustc +{TOOLCHAIN} -vV did not report a host triple")
    if not host_line.startswith("x86_64-"):
        fail(f"Stage 0 must run on an x86_64 build machine; rustc reports {host_line}")

    rustc_version = run(("rustc", f"+{TOOLCHAIN}", "--version", "--verbose"))
    cargo_version = run(("cargo", f"+{TOOLCHAIN}", "--version", "--verbose"))
    sysroot = run(("rustc", f"+{TOOLCHAIN}", "--print", "sysroot"))
    installed_components = run(("rustup", "component", "list", "--installed", "--toolchain", TOOLCHAIN))
    if not any(line == "rust-src" or line.startswith("rust-src-") for line in installed_components.splitlines()):
        fail(f"{TOOLCHAIN} rust-src component is required for -Z build-std")
    if not any(line == "rustfmt" or line.startswith("rustfmt-") for line in installed_components.splitlines()):
        fail(f"{TOOLCHAIN} rustfmt component is required by the pinned toolchain setup")

    builtin_targets = set(run(("rustc", f"+{TOOLCHAIN}", "--print", "target-list")).splitlines())
    if KERNEL_TARGET not in builtin_targets:
        fail(f"Stage 0 rustc does not list the required kernel target {KERNEL_TARGET}")

    tool_versions = {
        name: first_line(name)
        for name in ("ld.lld", "as", "readelf", "nm", "make", "python3")
    }
    report = {
        "schema_version": 1,
        "kind": "rust-stage0-host-inventory",
        "build_machine": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "rustc_host_triple": host_line,
        },
        "stage0": {
            "rustc": rustc_version.splitlines()[0],
            "cargo": cargo_version.splitlines()[0],
            "sysroot": sysroot,
            "installed_components": sorted(
                line.split("-x86_64-", 1)[0] for line in installed_components.splitlines()
            ),
            "native_octagram_executable": False,
        },
        "intended_toolchain": {
            "rustc_cargo_host": OCTAGRAM_HOST_TARGET,
            "kernel_target": KERNEL_TARGET,
            "custom_target_spec": str(TARGET_SPEC.relative_to(ROOT)),
            "custom_target_spec_validated": True,
        },
        "tool_versions": tool_versions,
        "status": {
            "stage0_linux_toolchain": "available",
            "rustc_octagram": "not-built",
            "cargo_octagram": "not-built",
            "llvm_backend_octagram": "not-built",
            "full_native_bootstrap": False,
        },
    }

    if args.output is not None:
        output_path = args.output
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print("RUST-STAGE0-INVENTORY-PASS")
    print(f"build_machine={platform.system()} {platform.machine()} ({host_line})")
    print(f"stage0_rustc={report['stage0']['rustc']}")
    print(f"stage0_cargo={report['stage0']['cargo']}")
    print(f"sysroot={sysroot}")
    print(f"intended_rustc_cargo_host={OCTAGRAM_HOST_TARGET}")
    print(f"kernel_target={KERNEL_TARGET}")
    print("rustc_octagram=not-built")
    print("cargo_octagram=not-built")
    print("native_bootstrap=false")
    if args.output is not None:
        print(f"report={args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

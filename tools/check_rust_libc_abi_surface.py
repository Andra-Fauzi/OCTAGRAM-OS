#!/usr/bin/env python3
"""Check selected Rust/libc ABI facts against OCTAGRAM's public headers."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tools/rust_libc_abi_surface.json"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"RUST-LIBC-ABI-FAIL: {message}")


def main() -> int:
    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    require(data.get("schema_version") == 1, "unsupported schema")
    require(data["target"]["triple"] == "x86_64-unknown-octagram", "wrong target triple")
    require(data["target"]["pointer_bits"] == 64, "expected 64-bit target")

    types = read("user/include/sys/types.h")
    expected_types = {
        "off_t": "int64_t", "ssize_t": "int64_t", "pid_t": "int32_t",
        "mode_t": "uint32_t", "time_t": "int64_t", "uid_t": "uint32_t",
        "gid_t": "uint32_t", "dev_t": "uint64_t", "ino_t": "uint64_t",
    }
    for name, c_type in expected_types.items():
        require(re.search(rf"typedef\s+{c_type}\s+{name}\s*;", types) is not None,
                f"{name} no longer matches {c_type}")

    octagram = read("user/include/octagram.h")
    require(re.search(r"struct\s+oct_stat\s*\{\s*uint64_t\s+size;\s*uint32_t\s+mode;\s*uint32_t\s+modified_seconds;", octagram, re.S) is not None,
            "oct_stat kernel ABI changed; update manifest and PAL mapping")

    dirent = read("user/include/dirent.h")
    require(re.search(r"struct\s+dirent\s*\{\s*uint64_t\s+d_ino;\s*off_t\s+d_off;\s*uint16_t\s+d_reclen;\s*unsigned\s+char\s+d_type;\s*char\s+d_name\[256\];", dirent, re.S) is not None,
            "dirent layout changed; update manifest and tests")

    timespec = read("user/include/time.h")
    require("time_t tv_sec;" in timespec and "long tv_nsec;" in timespec,
            "timespec fields changed")

    runtime = read("user/runtime.c")
    for field in ("result->st_size = (off_t)metadata.size;",
                  "result->st_mode = (mode_t)metadata.mode;",
                  "result->st_mtime = (time_t)metadata.modified_seconds;"):
        require(field in runtime, f"stat translation no longer contains: {field}")

    pthread = read("user/runtime.c")
    require(re.search(r"int pthread_detach\(pthread_t thread\).*?return ENOTSUP;", pthread, re.S) is not None,
            "pthread_detach behavior changed; update capability notes")

    gaps = data.get("known_gaps", [])
    require(any("not glibc or musl" in item for item in gaps),
            "must preserve explicit libc ABI boundary")
    require(any("does not prove Rust std" in item for item in gaps),
            "must not overclaim Rust std compatibility")

    print("RUST-LIBC-ABI-PASS: target types, oct_stat, dirent, timespec, stat translation, pthread limitation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

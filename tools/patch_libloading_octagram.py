#!/usr/bin/env python3
"""Give libloading's Unix dynamic-loader constant table OCTAGRAM values.

libloading 0.9.0 decides RTLD_LAZY/RTLD_NOW/RTLD_GLOBAL/RTLD_LOCAL per
target_os and stops with `compile_error!` for any triple it does not know.
OCTAGRAM's dlfcn.h uses the ELF/Linux numbering, so the compiler's proc-macro
loader (rustc_metadata) cannot be built until those arms exist.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

VERSION = "0.9.0"
# Values from user/include/dlfcn.h.
CONSTANTS = (
    ("RTLD_LAZY", "1"),
    ("RTLD_NOW", "2"),
    ("RTLD_GLOBAL", "0x100"),
    ("RTLD_LOCAL", "0"),
)


def patch_constants(path: Path) -> bool:
    source = path.read_text(encoding="utf-8")
    changed = False
    for name, value in CONSTANTS:
        message = f'"Target has no known `{name}` value.'
        old = "        } else {\n            compile_error!(\n                " + message
        new = (
            '        } else if #[cfg(target_os = "octagram")] {\n'
            f"            pub(super) const {name}: c_int = {value};\n"
            "        } else {\n"
            "            compile_error!(\n"
            "                " + message
        )
        count = source.count(old)
        if count == 0 and new in source:
            continue
        if count != 1:
            raise SystemExit(f"expected one {name} fallback in {path}, found {count}")
        source = source.replace(old, new, 1)
        changed = True
    path.write_text(source, encoding="utf-8")
    return changed


def verify(package: Path) -> None:
    source = (package / "src/os/unix/consts.rs").read_text(encoding="utf-8")
    for name, value in CONSTANTS:
        branch = (
            '#[cfg(target_os = "octagram")] {\n'
            f"            pub(super) const {name}: c_int = {value};"
        )
        if branch not in source:
            raise SystemExit(f"libloading {name} has no OCTAGRAM value: {package}")
        message = f'"Target has no known `{name}` value.'
        if source.index(branch) > source.index(message):
            raise SystemExit(
                f"libloading {name} OCTAGRAM branch sits after the compile_error: {package}"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry-root", required=True, type=Path)
    args = parser.parse_args()

    packages = sorted(args.registry_root.glob(f"*/libloading-{VERSION}"))
    if not packages:
        raise SystemExit(f"libloading {VERSION} is missing from Cargo's registry cache")

    patched = 0
    for package in packages:
        manifest = package / "Cargo.toml"
        constants = package / "src/os/unix/consts.rs"
        marker = package / ".cargo-ok"
        if not manifest.is_file() or not constants.is_file() or not marker.is_file():
            raise SystemExit(f"incomplete libloading {VERSION} registry source: {package}")
        if not re.search(
            rf'(?m)^version = "{re.escape(VERSION)}"$',
            manifest.read_text(encoding="utf-8"),
        ):
            raise SystemExit(f"unexpected libloading package version in {manifest}")

        patched += int(patch_constants(constants))
        verify(package)

    print(
        "RUST-LIBLOADING-OCTAGRAM-RTLD-PASS: "
        f"checked {len(packages)} libloading {VERSION} source tree(s); "
        f"patched {patched}; OCTAGRAM uses the dlfcn.h RTLD values"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

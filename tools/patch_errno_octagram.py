#!/usr/bin/env python3
"""Patch errno 0.3.14 for OCTAGRAM's Unix errno ABI.

The runtime exports __errno_location(), not errno_location(). Upstream errno
needs an OCTAGRAM cfg on the link-name attribute. The patch locates the
attribute semantically instead of depending on its indentation.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import tomllib
from pathlib import Path

ERRNO_VERSION = "0.3.14"

CFG_ATTR_RE = re.compile(
    r"(?ms)^(?P<indent>[ \\t]*)#\\[cfg_attr\\(.*?^[ \\t]*\\)\\]"
)
LINUX_TARGET_RE = re.compile(r'(?m)^(?P<indent>[ \\t]*)target_os = "linux",$')


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    unix_source = package / "src" / "unix.rs"
    if not manifest.is_file() or not unix_source.is_file():
        raise SystemExit(f"incomplete errno {ERRNO_VERSION} source tree: {package}")

    metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
    version = metadata.get("package", {}).get("version")
    if version != ERRNO_VERSION:
        raise SystemExit(
            f"unexpected errno version in {manifest}: {version!r}; "
            f"expected {ERRNO_VERSION}"
        )

    source = unix_source.read_text(encoding="utf-8")
    attributes = [
        match for match in CFG_ATTR_RE.finditer(source)
        if 'link_name = "__errno_location"' in match.group(0)
    ]
    if len(attributes) != 1:
        raise SystemExit(
            f"expected one cfg_attr for __errno_location in {unix_source}, "
            f"found {len(attributes)}; refusing an unreviewed source layout"
        )

    attribute = attributes[0]
    block = attribute.group(0)
    if 'target_os = "octagram"' in block:
        if "fn errno_location() -> *mut c_int;" not in source:
            raise SystemExit(f"errno_location declaration missing in {unix_source}")
        return False

    linux_lines = list(LINUX_TARGET_RE.finditer(block))
    if len(linux_lines) != 1:
        raise SystemExit(
            f"expected one Linux target entry in __errno_location cfg_attr in "
            f"{unix_source}, found {len(linux_lines)}"
        )
    if "fn errno_location() -> *mut c_int;" not in source:
        raise SystemExit(f"errno_location declaration missing in {unix_source}")

    line = linux_lines[0]
    indent = line.group("indent")
    replacement = (
        f'{indent}target_os = "linux",\\n'
        f'{indent}target_os = "octagram",'
    )
    patched_block = block[:line.start()] + replacement + block[line.end():]
    source = source[:attribute.start()] + patched_block + source[attribute.end():]
    unix_source.write_text(source, encoding="utf-8")

    # Cargo may re-extract a registry package if this marker is missing.
    (package / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--registry-root",
        type=Path,
        help="Cargo registry src directory (defaults to CARGO_HOME/registry/src)",
    )
    args = parser.parse_args()

    if args.registry_root is not None:
        registry_root = args.registry_root.expanduser().resolve()
    else:
        cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
        registry_root = cargo_home / "registry" / "src"

    packages = sorted(registry_root.glob(f"*/errno-{ERRNO_VERSION}"))
    if not packages:
        raise SystemExit(
            f"errno {ERRNO_VERSION} was not found below {registry_root}; "
            "run cargo fetch before applying the OCTAGRAM patch"
        )

    changed = sum(int(patch_package(package)) for package in packages)
    print(
        f"RUST-ERRNO-OCTAGRAM-BACKEND-PASS: checked {len(packages)} source tree(s); "
        f"patched {changed}; __errno_location symbol mapping verified"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

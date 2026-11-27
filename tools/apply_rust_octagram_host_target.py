#!/usr/bin/env python3
"""Register `x86_64-unknown-octagram` as a built-in rustc target.

Bootstrap cannot produce a compiler for a triple that exists only as a JSON
spec: the host triple must be known to `rustc_target` itself. This script
applies four checked edits to a pinned rust-lang/rust checkout:

1. copies patches/rust/octagram/x86_64_unknown_octagram.rs to
   compiler/rustc_target/src/spec/targets/;
2. adds `Octagram = "octagram"` to the `Os` enum in
   compiler/rustc_target/src/spec/mod.rs;
3. registers the target in the `supported_targets!` list;
4. suppresses only the expected unsupported-`dylib` warning for this exact
   static-only target in compiler/rustc_interface/src/passes.rs.

Every source edit is anchored on an exact block and is idempotent, so
re-running it after a partial failure is safe. The checkout revision is
verified because anchors are only valid for the pinned upstream tree.

Usage:
    python3 tools/apply_rust_octagram_host_target.py PATH_TO_RUST_CHECKOUT
                                          [--allow-unpinned]
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

EXPECTED_COMMIT = "342d74463af7dd3018ef6aa04dd5500c2deb5bd1"
REPO_ROOT = Path(__file__).resolve().parents[1]
TARGET_SOURCE = REPO_ROOT / "patches/rust/octagram/x86_64_unknown_octagram.rs"

SPEC_ROOT = Path("compiler/rustc_target/src/spec")
TARGET_DESTINATION = SPEC_ROOT / "targets/x86_64_unknown_octagram.rs"
SPEC_MOD = SPEC_ROOT / "mod.rs"

OS_ANCHOR = '        NuttX = "nuttx",\n'
OS_LINE = '        Octagram = "octagram",\n'
REGISTRATION_ANCHOR = '    ("x86_64-unknown-none", x86_64_unknown_none),\n'
REGISTRATION_LINE = '    ("x86_64-unknown-octagram", x86_64_unknown_octagram),\n'


def replace_once(path: Path, anchor: str, addition: str) -> bool:
    """Insert `addition` after `anchor`; return False when already present."""
    source = path.read_text(encoding="utf-8")
    if addition in source:
        return False
    count = source.count(anchor)
    if count != 1:
        raise SystemExit(
            f"expected one exact anchor in {path}: {anchor!r} (found {count}); "
            "the checkout has moved away from the pinned revision"
        )
    path.write_text(source.replace(anchor, anchor + addition, 1), encoding="utf-8")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("checkout", type=Path, help="path to a rust-lang/rust checkout")
    parser.add_argument(
        "--allow-unpinned",
        action="store_true",
        help="skip the pinned-commit check (anchors may not match)",
    )
    arguments = parser.parse_args()

    root = arguments.checkout.resolve()
    if not (root / SPEC_MOD).is_file():
        raise SystemExit(f"{root} is not a rust-lang/rust checkout (missing {SPEC_MOD})")

    if not TARGET_SOURCE.is_file():
        raise SystemExit(f"missing target definition source: {TARGET_SOURCE}")

    if not arguments.allow_unpinned:
        try:
            actual = subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=root, text=True, stderr=subprocess.DEVNULL
            ).strip()
        except (OSError, subprocess.CalledProcessError):
            raise SystemExit(
                f"{root} is not a git checkout, so the pinned revision cannot be "
                f"verified; re-clone rust-lang/rust at {EXPECTED_COMMIT}, or pass "
                "--allow-unpinned after checking the anchors yourself"
            ) from None
        if actual != EXPECTED_COMMIT:
            raise SystemExit(
                f"wrong upstream Rust revision: {actual} (expected {EXPECTED_COMMIT}); "
                "pass --allow-unpinned only after re-checking the anchors"
            )

    destination = root / TARGET_DESTINATION
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(TARGET_SOURCE, destination)
    print(f"installed {TARGET_DESTINATION}")

    changed = False
    changed |= replace_once(root / SPEC_MOD, OS_ANCHOR, OS_LINE)
    changed |= replace_once(root / SPEC_MOD, REGISTRATION_ANCHOR, REGISTRATION_LINE)

    # This target intentionally has no dynamic loader or shared-object ABI.
    # rustc normally warns when Cargo's std build script requests a dylib that
    # this static-only target cannot emit. Suppress only this expected warning
    # for the exact OCTAGRAM triple and Dylib crate type; keep every other
    # unsupported-output warning visible.
    passes = root / "compiler/rustc_interface/src/passes.rs"
    passes_source = passes.read_text(encoding="utf-8")
    old_diagnostic = """            session.dcx().emit_warn(diagnostics::UnsupportedCrateTypeForTarget {
                crate_type: *crate_type,
                target_triple: &session.opts.target_triple,
            });"""
    new_diagnostic = """            let expected_octagram_static_dylib =
                session.opts.target_triple == "x86_64-unknown-octagram"
                    && *crate_type == CrateType::Dylib;
            if !expected_octagram_static_dylib {
                session.dcx().emit_warn(diagnostics::UnsupportedCrateTypeForTarget {
                    crate_type: *crate_type,
                    target_triple: &session.opts.target_triple,
                });
            }"""
    if new_diagnostic not in passes_source:
        count = passes_source.count(old_diagnostic)
        if count != 1:
            raise SystemExit(
                "expected one UnsupportedCrateTypeForTarget diagnostic in "
                f"{passes}, found {count}; refusing an unsafe compiler patch"
            )
        passes.write_text(passes_source.replace(old_diagnostic, new_diagnostic, 1), encoding="utf-8")
    print("RUST-OCTAGRAM-STATIC-DYLIB-PASS: only the expected static-target dylib warning is filtered")

    print(
        "registered x86_64-unknown-octagram in compiler/rustc_target/src/spec/mod.rs"
        if changed
        else "x86_64-unknown-octagram was already registered"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

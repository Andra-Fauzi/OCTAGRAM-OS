#!/usr/bin/env python3
"""Check that both OCTAGRAM target specs describe the same ABI.

`x86_64-unknown-octagram` exists twice on purpose:

* `user/rust/targets/x86_64-unknown-octagram.json` is the JSON spec used for
  cross builds from Linux (`-Z build-std`, `-Z json-target-spec`).
* `patches/rust/octagram/x86_64_unknown_octagram.rs` is the builtin rustc
  definition needed to bootstrap a compiler that runs on OCTAGRAM itself.

The two files are maintained by hand, so this script derives the ABI values
from the JSON spec and asserts the builtin definition uses the matching
`TargetOptions`. It needs only Python: no Rust checkout, no rustc, no build.

    python3 tools/check_octagram_rust_target_sync.py

Exit status 0 prints OCTAGRAM-RUST-TARGET-SYNC-PASS.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
JSON_SPEC = ROOT / "user/rust/targets/x86_64-unknown-octagram.json"
BUILTIN_SPEC = ROOT / "patches/rust/octagram/x86_64_unknown_octagram.rs"


def expected_fragments(spec: dict) -> list[tuple[str, str]]:
    """Return (description, literal that must appear in the builtin spec)."""
    fragments: list[tuple[str, str]] = [
        ("llvm-target", f'llvm_target: "{spec["llvm-target"]}".into()'),
        ("data-layout", f'"{spec["data-layout"]}"'),
        ("os", 'os: Os::Octagram'),
        (
            "target-family",
            'families: cvs!["unix"]' if "unix" in spec.get("target-family", []) else "",
        ),
        ("pointer width", f'pointer_width: {spec["target-pointer-width"]}'),
        ("atomic width", f'max_atomic_width: Some({spec["max-atomic-width"]})'),
        ("features", f'features: "{spec["features"]}".into()'),
        (
            "dynamic-linking",
            f'dynamic_linking: {str(spec["dynamic-linking"]).lower()}',
        ),
        (
            "position-independent-executables",
            "position_independent_executables: "
            f'{str(spec["position-independent-executables"]).lower()}',
        ),
        (
            "crt-static-default",
            f'crt_static_default: {str(spec["crt-static-default"]).lower()}',
        ),
        (
            "has-thread-local",
            f'has_thread_local: {str(spec["has-thread-local"]).lower()}',
        ),
        (
            "disable-redzone",
            f'disable_redzone: {str(spec["disable-redzone"]).lower()}',
        ),
        (
            "relocation-model",
            f"relocation_model: RelocModel::{spec['relocation-model'].capitalize()}",
        ),
        (
            "code-model",
            f"code_model: Some(CodeModel::{spec['code-model'].capitalize()})",
        ),
        (
            "panic-strategy",
            f"panic_strategy: PanicStrategy::{spec['panic-strategy'].capitalize()}",
        ),
        (
            "linker-flavor",
            "linker_flavor: LinkerFlavor::Gnu(Cc::Yes, Lld::No)",
        ),
    ]
    return [(name, fragment) for name, fragment in fragments if fragment]


def main() -> int:
    for path in (JSON_SPEC, BUILTIN_SPEC):
        if not path.is_file():
            print(f"ERROR: missing target spec {path}", file=sys.stderr)
            return 2

    spec = json.loads(JSON_SPEC.read_text(encoding="utf-8"))
    builtin = BUILTIN_SPEC.read_text(encoding="utf-8")

    problems = [
        f"{name}: expected {fragment!r}"
        for name, fragment in expected_fragments(spec)
        if fragment not in builtin
    ]

    if problems:
        print("ERROR: builtin target and JSON target spec disagree", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1

    print("OCTAGRAM-RUST-TARGET-SYNC-PASS")
    print(
        "Builtin x86_64-unknown-octagram target matches "
        "user/rust/targets/x86_64-unknown-octagram.json."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

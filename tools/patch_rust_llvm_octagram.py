#!/usr/bin/env python3
"""Apply OCTAGRAM-only fixes to the pinned Rust LLVM build script.

The changes are intentionally confined to Rust's temporary bootstrap checkout:
* OCTAGRAM uses its sysroot libc++, never the runner's GNU libstdc++.
* Cross-link search paths come from the target llvm-config --libdir, not
  host paths accidentally captured in LLVM_LDFLAGS during CMake configure.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

SOURCE = Path("compiler/rustc_llvm/build.rs")
OLD_CXX = '    let stdcppname = if target.contains("openbsd")'
NEW_CXX = '    let stdcppname = if target.contains("octagram") || target.contains("openbsd")'

OLD_LD_BLOCK = '''    // LLVM ldflags
    //
    // If we're a cross-compile of LLVM then unfortunately we can't trust these
    // ldflags (largely where all the LLVM libs are located). Currently just
    // hack around this by replacing the host triple with the target and pray
    // that those -L directories are the same!
    let mut cmd = Command::new(&llvm_config);
    cmd.arg(llvm_link_arg).arg("--ldflags");
    for lib in &quoted_split(cmd) {
        if is_crossed {
            if let Some(stripped) = lib.strip_prefix("-LIBPATH:") {
                println!("cargo:rustc-link-search=native={}", stripped.replace(&host, &target));
            } else if let Some(stripped) = lib.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={}", stripped.replace(&host, &target));
            }
        } else if let Some(stripped) = lib.strip_prefix("-LIBPATH:") {
            println!("cargo:rustc-link-search=native={stripped}");
        } else if let Some(stripped) = lib.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={stripped}");
        } else if let Some(stripped) = lib.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={stripped}");
        }
    }
'''

NEW_LD_BLOCK = '''    // LLVM ldflags
    //
    // CMake's LLVM_LDFLAGS may contain paths from the build runner even when
    // the installed LLVM is for OCTAGRAM. Never add those host paths to a
    // freestanding target link; ask the target llvm-config for its actual
    // installation directory instead.
    if target.contains("octagram") {
        let libdir = output(Command::new(&llvm_config).arg("--libdir"));
        let libdir = libdir.trim();
        if libdir.is_empty() {
            panic!("OCTAGRAM llvm-config returned an empty --libdir");
        }
        println!("cargo:rustc-link-search=native={libdir}");
    } else {
        let mut cmd = Command::new(&llvm_config);
        cmd.arg(llvm_link_arg).arg("--ldflags");
        for lib in &quoted_split(cmd) {
            if is_crossed {
                if let Some(stripped) = lib.strip_prefix("-LIBPATH:") {
                    println!("cargo:rustc-link-search=native={}", stripped.replace(&host, &target));
                } else if let Some(stripped) = lib.strip_prefix("-L") {
                    println!("cargo:rustc-link-search=native={}", stripped.replace(&host, &target));
                }
            } else if let Some(stripped) = lib.strip_prefix("-LIBPATH:") {
                println!("cargo:rustc-link-search=native={stripped}");
            } else if let Some(stripped) = lib.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={stripped}");
            } else if let Some(stripped) = lib.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={stripped}");
            }
        }
    }
'''


def patch_source(source: str) -> str:
    if OLD_CXX in source:
        if source.count(OLD_CXX) != 1:
            raise ValueError("expected exactly one Rust LLVM C++ runtime selector")
        source = source.replace(OLD_CXX, NEW_CXX, 1)
    elif NEW_CXX not in source:
        raise ValueError("Rust LLVM C++ runtime selector does not match the pinned source")

    if OLD_LD_BLOCK in source:
        if source.count(OLD_LD_BLOCK) != 1:
            raise ValueError("expected exactly one Rust LLVM linker-flags block")
        source = source.replace(OLD_LD_BLOCK, NEW_LD_BLOCK, 1)
    elif NEW_LD_BLOCK not in source:
        raise ValueError("Rust LLVM linker-flags block does not match the pinned source")
    return source


def self_test() -> int:
    sample = OLD_CXX + "\n" + OLD_LD_BLOCK
    patched = patch_source(sample)
    if NEW_CXX not in patched or NEW_LD_BLOCK not in patched:
        raise AssertionError("patch did not install both OCTAGRAM fixes")
    if patch_source(patched) != patched:
        raise AssertionError("patch is not idempotent")
    if 'println!("cargo:rustc-link-lib={stdcppname}");' not in patched:
        raise AssertionError("C++ runtime selection path was unexpectedly removed")
    print("RUST-OCTAGRAM-LLVM-LINK-PATCH-SELF-TEST-PASS")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rust_checkout", nargs="?", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if args.rust_checkout is None:
        parser.error("rust_checkout is required unless --self-test is used")

    path = args.rust_checkout / SOURCE
    if not path.is_file():
        raise SystemExit(f"patch_rust_llvm_octagram: missing {path}")
    original = path.read_text()
    patched = patch_source(original)
    if patched != original:
        path.write_text(patched)
        print(f"patched {path}")
    else:
        print(f"already patched {path}")
    print("RUST-OCTAGRAM-LLVM-LINK-PASS")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, AssertionError) as exc:
        print(f"patch_rust_llvm_octagram: error: {exc}", file=sys.stderr)
        raise SystemExit(1)

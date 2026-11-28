#!/usr/bin/env python3
"""Apply OCTAGRAM-only fixes to the pinned Rust LLVM build script.

The changes are intentionally confined to Rust's temporary bootstrap checkout:
* OCTAGRAM uses its sysroot libc++, never the runner's GNU libstdc++.
* For a crossed build, bootstrap exposes the build-machine llvm-config as
  LLVM_CONFIG. OCTAGRAM must use OCTAGRAM_TARGET_LLVM_CONFIG so it does not
  bundle the runner's libLLVM archives and GNU C++ ABI into rustc_llvm.
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

OLD_LLVM_CONFIG = '''    let llvm_config =
        PathBuf::from(tracked_env_var_os("LLVM_CONFIG").expect("LLVM_CONFIG was not set"));
'''

NEW_LLVM_CONFIG = '''    // OCTAGRAM: for a crossed build bootstrap passes the build machine's
    // llvm-config in LLVM_CONFIG. An OCTAGRAM target must link the OCTAGRAM
    // libLLVM, so it reads the target llvm-config from its own variable.
    let llvm_config = if env::var("TARGET").map_or(false, |t| t.contains("octagram")) {
        PathBuf::from(tracked_env_var_os("OCTAGRAM_TARGET_LLVM_CONFIG").expect(
            "OCTAGRAM_TARGET_LLVM_CONFIG was not set (bootstrap_rustc_octagram.sh exports it)",
        ))
    } else {
        PathBuf::from(tracked_env_var_os("LLVM_CONFIG").expect("LLVM_CONFIG was not set"))
    };
'''

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


CXXRT_MARKER = '    // libc++abi and libunwind have to be specified explicitly on AIX.'
CXXRT_INSERTION = '''    // OCTAGRAM ships libc++ and its C++ ABI runtime as separate archives.
    // Match the libraries selected by user/toolchain/bin/octagram-cc.
    if target.contains("octagram") {
        println!("cargo:rustc-link-lib=cxxrt");
    }

'''



def patch_source(source: str) -> str:
    if OLD_CXX in source:
        if source.count(OLD_CXX) != 1:
            raise ValueError("expected exactly one Rust LLVM C++ runtime selector")
        source = source.replace(OLD_CXX, NEW_CXX, 1)
    elif NEW_CXX not in source:
        raise ValueError("Rust LLVM C++ runtime selector does not match the pinned source")

    if OLD_LLVM_CONFIG in source:
        if source.count(OLD_LLVM_CONFIG) != 1:
            raise ValueError("expected exactly one Rust LLVM llvm-config lookup")
        source = source.replace(OLD_LLVM_CONFIG, NEW_LLVM_CONFIG, 1)
    elif NEW_LLVM_CONFIG not in source:
        raise ValueError("Rust LLVM llvm-config lookup does not match the pinned source")

    if OLD_LD_BLOCK in source:
        if source.count(OLD_LD_BLOCK) != 1:
            raise ValueError("expected exactly one Rust LLVM linker-flags block")
        source = source.replace(OLD_LD_BLOCK, NEW_LD_BLOCK, 1)
    elif NEW_LD_BLOCK not in source:
        raise ValueError("Rust LLVM linker-flags block does not match the pinned source")

    if CXXRT_INSERTION not in source:
        if source.count(CXXRT_MARKER) != 1:
            raise ValueError("Rust LLVM C++ runtime insertion point does not match the pinned source")
        source = source.replace(CXXRT_MARKER, CXXRT_INSERTION + CXXRT_MARKER, 1)
    return source


def self_test() -> int:
    sample = OLD_CXX + "\n" + OLD_LLVM_CONFIG + OLD_LD_BLOCK + CXXRT_MARKER
    patched = patch_source(sample)
    if (
        NEW_CXX not in patched
        or NEW_LLVM_CONFIG not in patched
        or NEW_LD_BLOCK not in patched
        or CXXRT_INSERTION not in patched
    ):
        raise AssertionError("patch did not install all OCTAGRAM link fixes")
    if patch_source(patched) != patched:
        raise AssertionError("patch is not idempotent")
    if OLD_CXX in patched or OLD_LLVM_CONFIG in patched or OLD_LD_BLOCK in patched:
        raise AssertionError("old Rust host-link behaviour remained after patch")
    if "OCTAGRAM_TARGET_LLVM_CONFIG" not in patched:
        raise AssertionError("target llvm-config selection missing after patch")
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

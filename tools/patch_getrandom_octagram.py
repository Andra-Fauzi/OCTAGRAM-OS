#!/usr/bin/env python3
"""Patch getrandom 0.3.3 with OCTAGRAM's native syscall-121 backend.

The backend follows OCTAGRAM's int 0x80 syscall ABI directly. It does not
depend on upstream libc errno helpers, Linux syscall numbers, or RDRAND.
"""

from __future__ import annotations

import argparse
import os
import sys
import tomllib
from pathlib import Path

GETRANDOM_VERSION = "0.3.3"

# Remove this exact dependency table added by the earlier libc-backed patch.
OLD_MANIFEST_BLOCK = """[target.'cfg(target_os = "octagram")'.dependencies]
libc = { version = "0.2.154", default-features = false }

"""

BACKEND_ANCHOR = """    } else if #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "dragonfly",
"""
OLD_BACKEND_BLOCK = """    } else if #[cfg(target_os = "octagram")] {
        mod getrandom;
        pub use getrandom::*;
"""
BACKEND_BLOCK = """    } else if #[cfg(target_os = "octagram")] {
        mod octagram;
        pub use octagram::*;
"""
BACKEND_MARKER = 'target_os = "octagram")] {\n        mod octagram;'

OCTAGRAM_BACKEND = r'''//! Native OCTAGRAM getrandom backend.
//!
//! OCTAGRAM syscall 121 accepts (buffer, length, flags) in RDI, RSI, RDX,
//! enters through int 0x80, and returns either a byte count or -errno.
//! The kernel currently caps one call at 256 bytes.

use crate::Error;
pub use crate::util::{inner_u32, inner_u64};
use core::arch::asm;
use core::mem::MaybeUninit;

const SYS_GETRANDOM: isize = 121;
const MAX_GETRANDOM_BYTES: usize = 256;
const EINTR: isize = -4;

#[inline]
unsafe fn syscall_getrandom(buffer: *mut u8, length: usize) -> isize {
    let result: isize;
    // SAFETY: the caller provides a writable buffer of length bytes. The
    // OCTAGRAM syscall ABI preserves the non-result argument registers.
    unsafe {
        asm!(
            "int 0x80",
            inlateout("rax") SYS_GETRANDOM => result,
            in("rdi") buffer,
            in("rsi") length,
            in("rdx") 0usize,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    result
}

#[inline]
pub fn fill_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    let mut offset = 0usize;

    while offset < dest.len() {
        let request_len = core::cmp::min(
            dest.len() - offset,
            MAX_GETRANDOM_BYTES,
        );
        // SAFETY: offset is below dest.len(), and request_len is bounded by
        // the number of remaining writable bytes.
        let buffer = unsafe { dest.as_mut_ptr().add(offset).cast::<u8>() };
        // SAFETY: buffer points to request_len writable, uninitialized bytes.
        let result = unsafe { syscall_getrandom(buffer, request_len) };

        if result > 0 {
            let written = usize::try_from(result).map_err(|_| Error::UNEXPECTED)?;
            if written > request_len {
                return Err(Error::UNEXPECTED);
            }
            offset += written;
        } else if result == EINTR {
            continue;
        } else if result < 0 {
            let error_code = i32::try_from(result).map_err(|_| Error::UNEXPECTED)?;
            return Err(Error::from_neg_error_code(error_code));
        } else {
            return Err(Error::UNEXPECTED);
        }
    }

    Ok(())
}
'''


def patch_package(package: Path) -> bool:
    manifest = package / "Cargo.toml"
    backend = package / "src/backends.rs"
    backend_dir = package / "src/backends"
    octagram_backend = backend_dir / "octagram.rs"
    if not manifest.is_file() or not backend.is_file() or not backend_dir.is_dir():
        raise SystemExit(f"incomplete getrandom {GETRANDOM_VERSION} source tree: {package}")

    manifest_source = manifest.read_text(encoding="utf-8")
    parsed = tomllib.loads(manifest_source)
    version = parsed.get("package", {}).get("version")
    if version != GETRANDOM_VERSION:
        raise SystemExit(
            f"unexpected getrandom version in {manifest}: {version!r}; "
            f"expected {GETRANDOM_VERSION}"
        )

    changed = False
    if OLD_MANIFEST_BLOCK in manifest_source:
        if manifest_source.count(OLD_MANIFEST_BLOCK) != 1:
            raise SystemExit(f"duplicate stale OCTAGRAM libc dependency in {manifest}")
        manifest_source = manifest_source.replace(OLD_MANIFEST_BLOCK, "", 1)
        manifest.write_text(manifest_source, encoding="utf-8")
        changed = True

    parsed = tomllib.loads(manifest.read_text(encoding="utf-8"))
    target_table = parsed.get("target", {}).get('cfg(target_os = "octagram")', {})
    if target_table:
        raise SystemExit(
            f"unexpected target_os=octagram Cargo dependencies remain in {manifest}; "
            "refusing to keep an unverified libc dependency"
        )

    backend_source = backend.read_text(encoding="utf-8")
    if OLD_BACKEND_BLOCK in backend_source:
        if backend_source.count(OLD_BACKEND_BLOCK) != 1:
            raise SystemExit(f"duplicate old OCTAGRAM getrandom backend in {backend}")
        backend_source = backend_source.replace(OLD_BACKEND_BLOCK, BACKEND_BLOCK, 1)
        backend.write_text(backend_source, encoding="utf-8")
        changed = True
    elif BACKEND_BLOCK not in backend_source:
        if 'else if #[cfg(target_os = "octagram")]' in backend_source:
            raise SystemExit(
                f"unrecognized existing OCTAGRAM backend in {backend}; refusing to overwrite it"
            )
        if backend_source.count(BACKEND_ANCHOR) != 1:
            raise SystemExit(
                f"expected one exact backend-selection anchor in {backend}; "
                "the getrandom source has changed"
            )
        backend_source = backend_source.replace(
            BACKEND_ANCHOR, BACKEND_BLOCK + BACKEND_ANCHOR, 1
        )
        backend.write_text(backend_source, encoding="utf-8")
        changed = True

    if octagram_backend.exists():
        existing = octagram_backend.read_text(encoding="utf-8")
        if existing != OCTAGRAM_BACKEND:
            raise SystemExit(
                f"unexpected pre-existing OCTAGRAM backend file {octagram_backend}; "
                "refusing to overwrite unreviewed source"
            )
    else:
        octagram_backend.write_text(OCTAGRAM_BACKEND, encoding="utf-8")
        changed = True

    final_backend = backend.read_text(encoding="utf-8")
    if BACKEND_BLOCK not in final_backend or BACKEND_MARKER not in final_backend:
        raise SystemExit(f"OCTAGRAM getrandom backend selection did not verify in {backend}")
    if OLD_BACKEND_BLOCK in final_backend:
        raise SystemExit(f"stale libc-backed OCTAGRAM module selection remains in {backend}")
    return changed


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

    packages = sorted(registry_root.glob(f"*/getrandom-{GETRANDOM_VERSION}"))
    if not packages:
        raise SystemExit(
            f"getrandom {GETRANDOM_VERSION} was not found below {registry_root}; "
            "run cargo fetch before applying the OCTAGRAM backend"
        )

    changed_count = 0
    for package in packages:
        changed_count += int(patch_package(package))
    print(
        "RUST-GETRANDOM-OCTAGRAM-BACKEND-PASS: "
        f"patched {len(packages)} source tree(s); OCTAGRAM uses syscall 121"
    )
    if changed_count == 0:
        print("getrandom OCTAGRAM backend was already patched")
    return 0


if __name__ == "__main__":
    sys.exit(main())

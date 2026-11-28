#!/usr/bin/env python3
"""Teach ctrlc 3.5.1 to use the OCTAGRAM libc instead of nix.

`ctrlc` is a dependency of `rustc_driver_impl`, so it is compiled for the
x86_64-unknown-octagram host. Its Unix backend is built on nix 0.30, which
does not know that target_os and fails to compile for it. The OCTAGRAM guest
libc already exports sigaction() and the standard signal numbers, so this
patch installs a small backend that uses them directly and keeps nix out of
the OCTAGRAM dependency graph.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

VERSION = "3.5.1"
MANIFEST_OLD = '[target."cfg(unix)".dependencies.nix]'
MANIFEST_NEW = "[target.'cfg(all(unix, not(target_os = \"octagram\")))'.dependencies.nix]"
PLATFORM_OLD = """#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use self::unix::*;
"""
PLATFORM_NEW = """#[cfg(all(unix, not(target_os = "octagram")))]
mod unix;

#[cfg(all(unix, target_os = "octagram"))]
mod octagram;

#[cfg(windows)]
mod windows;

#[cfg(all(unix, not(target_os = "octagram")))]
pub use self::unix::*;

#[cfg(all(unix, target_os = "octagram"))]
pub use self::octagram::*;
"""
OCTAGRAM_BACKEND = r'''// OCTAGRAM signal backend.
//
// The guest libc exports sigaction() and the standard signal numbers, so this
// backend needs neither nix nor a semaphore: the async-signal-safe handler
// publishes an atomic flag and the waiting thread polls it.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::Error as CtrlcError;

const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

/// user/include/signal.h
const SA_RESTART: i32 = 0x1000_0000;

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// user/include/signal.h `struct sigaction`; the handler union is first.
#[repr(C)]
#[derive(Clone, Copy)]
struct SigAction {
    sa_sigaction: usize,
    sa_mask: u64,
    sa_flags: i32,
    sa_restorer: Option<unsafe extern "C" fn()>,
}

extern "C" {
    fn sigaction(
        signal: i32,
        action: *const SigAction,
        old_action: *mut SigAction,
    ) -> i32;
}

extern "C" fn os_handler(_: i32) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Platform specific error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A handler was already installed and `overwrite` was false.
    EEXIST,
    /// The guest libc reported an error number.
    System(i32),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EEXIST => formatter.write_str("a signal handler is already installed"),
            Error::System(number) => write!(formatter, "signal error {number}"),
        }
    }
}

impl std::error::Error for Error {}

/// Platform specific signal type.
#[derive(Debug, Clone, Copy)]
pub enum Signal {
    /// `SIGINT`
    SigInt,
    /// `SIGTERM`
    SigTerm,
    /// `SIGHUP`
    SigHup,
}

unsafe fn install(signal: i32, overwrite: bool) -> Result<SigAction, Error> {
    let action = SigAction {
        sa_sigaction: os_handler as usize,
        sa_mask: 0,
        sa_flags: SA_RESTART,
        sa_restorer: None,
    };
    let mut old: SigAction = std::mem::zeroed();
    if sigaction(signal, &action, &mut old) != 0 {
        let number = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(0);
        return Err(Error::System(number));
    }
    // SIG_DFL is 0; anything else (SIG_IGN or a handler) is already installed.
    if !overwrite && old.sa_sigaction != 0 {
        sigaction(signal, &old, std::ptr::null_mut());
        return Err(Error::EEXIST);
    }
    Ok(old)
}

unsafe fn restore(signal: i32, action: &SigAction) {
    sigaction(signal, action, std::ptr::null_mut());
}

/// Register the SIGINT (and, with `termination`, SIGTERM/SIGHUP) handler.
///
/// # Safety
///
/// Must be called before [`block_ctrl_c`] and at most once per process.
pub unsafe fn init_os_handler(overwrite: bool) -> Result<(), Error> {
    let sigint_old = install(SIGINT, overwrite)?;

    #[cfg(feature = "termination")]
    {
        let sigterm_old = match install(SIGTERM, overwrite) {
            Ok(old) => old,
            Err(error) => {
                restore(SIGINT, &sigint_old);
                return Err(error);
            }
        };
        if let Err(error) = install(SIGHUP, overwrite) {
            restore(SIGINT, &sigint_old);
            restore(SIGTERM, &sigterm_old);
            return Err(error);
        }
    }
    #[cfg(not(feature = "termination"))]
    let _ = &sigint_old;

    Ok(())
}

/// Block until the handler has run once.
///
/// # Safety
///
/// Must be called after [`init_os_handler`].
pub unsafe fn block_ctrl_c() -> Result<(), CtrlcError> {
    // OCTAGRAM has no sem_t in its libc surface; the handler only needs to
    // publish an event, so poll the flag it sets.
    while !INTERRUPTED.swap(false, Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok(())
}
'''


def find_packages(root: Path, name: str) -> list[Path]:
    """Locate a crate source under either a registry index dir or its parent."""
    if not root.is_dir():
        raise SystemExit(f"Cargo registry source root is missing: {root}")
    matches = [path for path in root.iterdir() if path.is_dir() and path.name == name]
    if matches:
        return sorted(matches)
    for index in root.iterdir():
        if index.is_dir():
            matches.extend(
                path for path in index.iterdir() if path.is_dir() and path.name == name
            )
    return sorted(matches)


def patch_manifest(path: Path) -> bool:
    source = path.read_text(encoding="utf-8")
    count = source.count(MANIFEST_OLD)
    if count == 1:
        path.write_text(source.replace(MANIFEST_OLD, MANIFEST_NEW, 1), encoding="utf-8")
        return True
    if count == 0 and MANIFEST_NEW in source:
        return False
    raise SystemExit(f"unexpected ctrlc {VERSION} nix dependency layout: {path}")


def patch_platform(path: Path) -> bool:
    source = path.read_text(encoding="utf-8")
    if PLATFORM_OLD in source:
        path.write_text(source.replace(PLATFORM_OLD, PLATFORM_NEW, 1), encoding="utf-8")
        return True
    if PLATFORM_NEW in source:
        return False
    raise SystemExit(f"unexpected ctrlc {VERSION} platform module selection: {path}")


def verify(package: Path) -> None:
    manifest = (package / "Cargo.toml").read_text(encoding="utf-8")
    platform = (package / "src/platform/mod.rs").read_text(encoding="utf-8")
    backend = package / "src/platform/octagram.rs"
    if MANIFEST_OLD in manifest or MANIFEST_NEW not in manifest:
        raise SystemExit(f"ctrlc nix dependency is not excluded for OCTAGRAM: {package}")
    if PLATFORM_OLD in platform or PLATFORM_NEW not in platform:
        raise SystemExit(f"ctrlc platform selection is not patched for OCTAGRAM: {package}")
    if not backend.is_file():
        raise SystemExit(f"ctrlc OCTAGRAM backend is missing: {backend}")
    source = backend.read_text(encoding="utf-8")
    for needle in ("fn sigaction", "init_os_handler", "block_ctrl_c", "SA_RESTART"):
        if needle not in source:
            raise SystemExit(f"ctrlc OCTAGRAM backend is missing {needle}: {backend}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--registry-root", required=True, type=Path)
    args = parser.parse_args()

    packages = find_packages(args.registry_root, f"ctrlc-{VERSION}")
    if not packages:
        raise SystemExit(f"ctrlc {VERSION} is missing from Cargo's registry cache")

    patched = 0
    for package in packages:
        manifest = package / "Cargo.toml"
        platform = package / "src/platform/mod.rs"
        if not manifest.is_file() or not platform.is_file():
            raise SystemExit(f"incomplete ctrlc {VERSION} registry source: {package}")
        # Cargo treats a missing or empty `.cargo-ok` as an interrupted
        # extraction and re-extracts the crate, which would discard these
        # edits. Keep the marker in the format Cargo itself writes.
        marker = package / ".cargo-ok"
        if not marker.is_file() or not marker.read_text(encoding="utf-8").strip():
            marker.write_text('{"v":1}', encoding="utf-8")
        if not re.search(
            rf'(?m)^version = "{re.escape(VERSION)}"$',
            manifest.read_text(encoding="utf-8"),
        ):
            raise SystemExit(f"unexpected ctrlc package version in {manifest}")

        changed = patch_manifest(manifest)
        changed = patch_platform(platform) or changed
        backend = package / "src/platform/octagram.rs"
        if not backend.is_file():
            backend.write_text(OCTAGRAM_BACKEND, encoding="utf-8")
            changed = True
        verify(package)
        patched += int(changed)

    print(
        "RUST-CTRLC-OCTAGRAM-BACKEND-PASS: "
        f"checked {len(packages)} ctrlc {VERSION} source tree(s); "
        f"patched {patched}; OCTAGRAM uses the guest sigaction() directly"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

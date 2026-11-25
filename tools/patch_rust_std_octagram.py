#!/usr/bin/env python3
"""Patch the installed rust-src component with the OCTAGRAM std PAL probe glue."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"expected one exact Rust std source anchor in {path}, found {count}")
    path.write_text(source.replace(old, new, 1), encoding="utf-8")


def backport_thread_spawn_handle_fix(std: Path) -> None:
    """Backport rust-lang/rust#156277 for std versions predating its merge."""
    current = std / "thread/current.rs"
    source = current.read_text(encoding="utf-8")
    start = source.find("/// Tries to set the thread handle for the current thread.")
    if start < 0:
        start = source.find("/// Sets the thread handle for the current thread.")
    end = source.find("/// Gets the unique identifier", start)
    if start >= 0 and end > start:
        current_impl = """/// Sets the thread handle for the current thread.
///
/// This is only called from super::lifecycle::ThreadInit::init. If
/// thread::current() ran earlier on this thread, replace the temporary handle
/// with the one from thread::spawn.
pub(super) fn set_current(thread: Thread) {
    let current = CURRENT.get();
    if current > DESTROYED {
        // SAFETY: current points to a valid Thread created by init_current.
        unsafe {
            CURRENT.set(DESTROYED);
            drop(Thread::from_raw(current));
        }
    } else if current == BUSY {
        rtabort!(
            "set_current() was called while init_current() is in progress, \
            which indicates a bug in the Rust threading implementation"
        );
    }

    id::set(thread.id());
    crate::sys::thread_local::guard::enable();
    CURRENT.set(thread.into_raw().cast_mut());
}

/// Gets the unique identifier"""
        source = source[:start] + current_impl + source[end + len("/// Gets the unique identifier"):]
        current.write_text(source, encoding="utf-8")
    elif "pub(super) fn set_current(thread: Thread) {" in source and "set_current() was called while init_current()" in source:
        pass
    else:
        raise SystemExit("unsupported rust std thread/current.rs layout; refusing unsafe patch")

    lifecycle = std / "thread/lifecycle.rs"
    source = lifecycle.read_text(encoding="utf-8")
    source = source.replace(
        "use super::current::{recover_preinitialized_for_spawn, set_current};",
        "use super::current::set_current;",
        1,
    )
    old_init_patterns = [
        """        if let Err(thread) = set_current(self.handle.clone()) {
            if !recover_preinitialized_for_spawn(thread) {
                // The current thread should not have set yet. Use an abort to save binary size (see #123356).
                rtabort!("current thread handle already set during thread spawn");
            }
        }
""",
        """        if let Err(_thread) = set_current(self.handle.clone()) {
            // The current thread should not have set yet. Use an abort to save binary size (see #123356).
            rtabort!("current thread handle already set during thread spawn");
        }
""",
    ]
    for old_init in old_init_patterns:
        if old_init in source:
            source = source.replace(old_init, "        set_current(self.handle.clone());\n", 1)
            break
    if "set_current(self.handle.clone());" not in source:
        raise SystemExit("unsupported rust std ThreadInit::init layout; refusing unsafe patch")
    lifecycle.write_text(source, encoding="utf-8")

    print("RUST-STD-THREAD-SPAWN-FIX-PASS: preinitialized thread handle is replaced")


def enable_std_file_locks(std: Path) -> None:
    """Route Rust std `File` locking through OCTAGRAM's flock syscall.

    Cargo and rustc serialize access to shared build directories with advisory
    file locks, so the upstream allowlist must name the OCTAGRAM target in
    addition to its existing Unix targets.
    """
    fs_unix = std / "sys/fs/unix.rs"
    prefix = (
        '                target_os = "redox",\n'
        '                target_vendor = "apple",\n'
        '            ) => {\n'
    )
    patched_prefix = prefix.replace(
        '                target_os = "redox",\n',
        '                target_os = "redox",\n                target_os = "octagram",\n',
        1,
    )
    probes = [
        "                cvt(unsafe { libc::flock(self.as_raw_fd(), libc::LOCK_EX) })?;",
        "                cvt(unsafe { libc::flock(self.as_raw_fd(), libc::LOCK_SH) })?;",
        "                let result =\n"
        "                    cvt(unsafe { libc::flock(self.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) });",
        "                let result =\n"
        "                    cvt(unsafe { libc::flock(self.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) });",
        "                cvt(unsafe { libc::flock(self.as_raw_fd(), libc::LOCK_UN) })?;",
    ]
    for probe in probes:
        replace_once(fs_unix, prefix + probe, patched_prefix + probe)
    print("RUST-STD-FILE-LOCK-PASS: std File locking dispatches to OCTAGRAM flock")


OCTAGRAM_LINUX = 'any(target_os = "linux", target_os = "octagram")'

OCTAGRAM_ARM_FILES = {
    "os/unix/net/ancillary.rs",
    "os/unix/net/ucred.rs",
    "sys/args/unix.rs",
    "sys/io/kernel_copy/mod.rs",
    "sys/fs/unix.rs",
    "sys/paths/unix.rs",
    "sys/random/mod.rs",
    "sys/thread/unix.rs",
}


def widen_linux_cfg_gates(std: Path) -> None:
    """Treat OCTAGRAM like Linux in every std cfg gate.

    std picks its Unix PAL pieces with hundreds of `cfg(target_os = "linux")`
    gates: getrandom, futex mutexes, pipes, process spawning, `flock` and so
    on. OCTAGRAM implements the Linux userspace ABI, so each of those gates
    must select OCTAGRAM too. Nested `any` predicates are valid cfg syntax.
    Files that already mention the widened predicate are skipped, so running
    this again is a no-op.
    """
    widened = 0
    for path in sorted(std.rglob("*.rs")):
        relative = path.relative_to(std)
        if relative.parts[:2] == ("os", "linux"):
            continue
        # These files get an OCTAGRAM arm of their own below (or above, for the
        # file lock allowlist); widening them too would define the same item
        # twice.
        if relative.as_posix() in OCTAGRAM_ARM_FILES:
            continue
        source = path.read_text(encoding="utf-8")
        if OCTAGRAM_LINUX in source:
            continue
        updated = source.replace('target_os = "linux"', OCTAGRAM_LINUX)
        if updated != source:
            path.write_text(updated, encoding="utf-8")
            widened += 1
    print(f"RUST-STD-OCTAGRAM-CFG-PASS: {widened} std files now select Linux for OCTAGRAM")


def main() -> int:
    sysroot = Path(subprocess.check_output(
        ["rustc", "+nightly", "--print", "sysroot"], text=True
    ).strip())
    library = sysroot / "lib/rustlib/src/rust/library"
    std = library / "std/src"
    if not (std / "os/unix/mod.rs").is_file():
        raise SystemExit(f"rust-src component not found under {library}")

    backport_thread_spawn_handle_fix(std)
    enable_std_file_locks(std)

    os_mod = std / "os/mod.rs"
    replace_once(
        os_mod,
        "pub mod raw;\n",
        "pub mod raw;\n\n#[cfg(target_os = \"octagram\")]\npub(crate) mod octagram;\n",
    )

    unix_mod = std / "os/unix/mod.rs"
    replace_once(
        unix_mod,
        "    #[cfg(target_os = \"vxworks\")]\n"
        "    pub use crate::os::vxworks::*;\n",
        "    #[cfg(target_os = \"vxworks\")]\n"
        "    pub use crate::os::vxworks::*;\n"
        "    #[cfg(target_os = \"octagram\")]\n"
        "    pub(crate) use crate::os::octagram::*;\n",
    )
    thread_unix = std / "sys/thread/unix.rs"
    thread_anchor = (
        "impl Drop for Thread {\n"
        "    fn drop(&mut self) {\n"
        "        let ret = unsafe { libc::pthread_detach(self.id) };\n"
        "        debug_assert_eq!(ret, 0);\n"
        "    }\n"
        "}\n"
    )
    replace_once(
        thread_unix,
        thread_anchor,
        thread_anchor
        + "\n#[cfg(target_os = \"octagram\")]\n"
        + "pub fn set_name(_name: &CStr) {\n"
        + "    // Thread naming is not exposed by the current OCTAGRAM ABI.\n"
        + "}\n",
    )

    args_unix = std / "sys/args/unix.rs"
    replace_once(
        args_unix,
        '    target_os = "nuttx",\n))]\nmod imp {',
        '    target_os = "nuttx",\n    target_os = "octagram",\n))]\nmod imp {',
    )

    random_mod = std / "sys/random/mod.rs"
    replace_once(
        random_mod,
        '    any(target_os = "horizon", target_os = "cygwin", target_os = "redox") => {',
        '    target_os = "octagram" => {\n'
        '        mod getrandom;\n'
        '        pub use getrandom::fill_bytes;\n'
        '    }\n'
        '    any(target_os = "horizon", target_os = "cygwin", target_os = "redox") => {',
    )

    paths_unix = std / "sys/paths/unix.rs"
    current_exe_anchor = "#[cfg(target_os = \"aix\")]\npub fn current_exe() -> io::Result<PathBuf> {"
    replace_once(
        paths_unix,
        current_exe_anchor,
        "#[cfg(target_os = \"octagram\")]\n"
        + "pub fn current_exe() -> io::Result<PathBuf> {\n"
        + "    Err(io::Error::new(\n"
        + "        io::ErrorKind::Unsupported,\n"
        + "        \"current_exe is not supported by the current OCTAGRAM process ABI\",\n"
        + "    ))\n"
        + "}\n\n"
        + current_exe_anchor,
    )

    # OCTAGRAM exposes clock_nanosleep on CLOCK_MONOTONIC and the cached
    # metadata wrapper only forwards Read/Write, so both Linux-only items are
    # usable as they are.
    replace_once(
        std / "sys/thread/unix.rs",
        '    target_os = "vxworks",\n    target_os = "wasi",\n))]\npub fn sleep_until',
        '    target_os = "vxworks",\n    target_os = "wasi",\n'
        '    target_os = "octagram",\n))]\npub fn sleep_until',
    )
    replace_once(
        std / "sys/fs/unix.rs",
        '#[cfg(any(target_os = "linux", target_os = "android"))]\n'
        'pub(in crate::sys) use cfm::CachedFileMetadata;',
        '#[cfg(any(target_os = "linux", target_os = "android", target_os = "octagram"))]\n'
        'pub(in crate::sys) use cfm::CachedFileMetadata;',
    )

    target = std / "os/octagram"
    target.mkdir(parents=True, exist_ok=True)
    for name in ("mod.rs", "raw.rs", "fs.rs"):
        destination = target / name
        if destination.exists():
            raise SystemExit(f"refusing to overwrite existing Rust std target file: {destination}")
        source = ROOT / "patches/rust-std/octagram" / name
        destination.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")

    widen_linux_cfg_gates(std)
    print("RUST-STD-OCTAGRAM-PATCH-PASS: Unix target glue installed in rust-src")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

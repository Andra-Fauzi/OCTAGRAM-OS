#!/usr/bin/env python3
"""Patch the pinned Rust checkout std PAL with OCTAGRAM Unix target glue."""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOOLCHAIN = os.environ.get("OCTAGRAM_RUST_TOOLCHAIN", "nightly")


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text(encoding="utf-8")
    # The Rust bootstrap build directory is checkpointed and this script runs
    # again on resume. Accept an already-applied exact edit instead of
    # failing because its upstream anchor has been consumed.
    if new in source:
        return
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
    # These files need target-specific decisions rather than blanket Linux cfg
    # widening. They contain optimizations or syscalls that are not part of the
    # current OCTAGRAM ABI, or are only consumed by the Linux kernel_copy path.
    "io/stdio.rs",
    "os/unix/mod.rs",
    "sys/args/unix.rs",
    "sys/fs/mod.rs",
    "sys/fs/unix.rs",
    "sys/io/kernel_copy/mod.rs",
    "sys/pal/unix/weak/mod.rs",
    "sys/paths/unix.rs",
    # sys/process/mod.rs re-exports PidFd only for Linux; OCTAGRAM currently
    # does not implement pidfd_open/pidfd_send_signal/pidfd_getfd.
    "sys/process/mod.rs",
    "sys/process/unix/mod.rs",
    "sys/process/unix/unix.rs",
    # The shared process implementation contains Linux-only pidfd state and
    # methods. OCTAGRAM currently uses the non-Linux stub implementation.
    "sys/process/unix/common.rs",
    # Keep Linux networking extension API declarations target-specific:
    # OCTAGRAM needs linux_ext::addr internally, but not ancillary/TCP traits.
    "os/net/linux_ext/mod.rs",
    "sys/random/mod.rs",
    "sys/thread/unix.rs",
}


def restore_linux_only_cfg_gates(std: Path) -> None:
    """Undo the old broad Linux widening in files now explicitly Linux-only.

    Rust bootstrap checkpoints may contain a checkout patched by an earlier
    version of this script. Restore only the exact nested predicate this
    patcher generated, so the updated exclusions also work on cache resume.
    """
    restored = 0
    for relative in ("os/net/linux_ext/mod.rs", "sys/process/unix/common.rs"):
        path = std / relative
        source = path.read_text(encoding="utf-8")
        updated = source.replace(OCTAGRAM_LINUX, 'target_os = "linux"')
        if updated != source:
            restored += source.count(OCTAGRAM_LINUX)
            path.write_text(updated, encoding="utf-8")
    print(
        "RUST-STD-OCTAGRAM-LINUX-ONLY-CFG-PASS: "
        f"restored {restored} Linux-only cfg predicate(s)"
    )


def gate_linux_extension_modules(std: Path) -> None:
    """Compile ancillary-data and TCP extension modules only on their ABIs."""
    path = std / "os/net/linux_ext/mod.rs"
    source = path.read_text(encoding="utf-8")
    cfg = '#[cfg(any(target_os = "linux", target_os = "android", target_os = "cygwin"))]\n'
    modules = (
        (
            '#[unstable(feature = "unix_socket_ancillary_data", issue = "76915")]\n'
            "pub(crate) mod socket;",
        ),
        (
            '#[stable(feature = "tcp_quickack", since = "1.89.0")]\n'
            "pub(crate) mod tcp;",
        ),
    )
    for (anchor,) in modules:
        gated = cfg + anchor
        if gated in source:
            continue
        if source.count(anchor) != 1:
            raise SystemExit(f"expected one Linux extension module declaration in {path}: {anchor!r}")
        source = source.replace(anchor, gated, 1)
    path.write_text(source, encoding="utf-8")
    print(
        "RUST-STD-OCTAGRAM-LINUX-EXTENSIONS-PASS: "
        "ancillary-data and TCP extension modules remain Linux/Android/Cygwin-only"
    )


def allow_octagram_internal_socket_addr_ext(std: Path) -> None:
    """Retain the internal abstract-socket helper without a false dead-code error."""
    path = std / "os/net/linux_ext/addr.rs"
    source = path.read_text(encoding="utf-8")
    anchor = (
        '#[stable(feature = "unix_socket_abstract", since = "1.70.0")]\n'
        "pub impl(in crate::os) trait SocketAddrExt {"
    )
    patched = (
        '#[stable(feature = "unix_socket_abstract", since = "1.70.0")]\n'
        '#[cfg_attr(target_os = "octagram", allow(dead_code))]\n'
        "pub impl(in crate::os) trait SocketAddrExt {"
    )
    if patched not in source:
        if source.count(anchor) != 1:
            raise SystemExit(f"expected one SocketAddrExt trait declaration in {path}")
        source = source.replace(anchor, patched, 1)
        path.write_text(source, encoding="utf-8")
    print(
        "RUST-STD-OCTAGRAM-ABSTRACT-SOCKET-EXT-PASS: "
        "internal SocketAddrExt helper is retained without an unused-code warning"
    )


def preserve_linux_socket_option_cfgs(std: Path) -> None:
    """Keep Linux-only socket option helpers out of OCTAGRAM std.

    The shared Unix socket implementation is still selected for OCTAGRAM,
    but it must not compile Linux-only TCP_QUICKACK, TCP_DEFER_ACCEPT or
    SO_PASSCRED methods that have no OCTAGRAM callers or ABI contract.
    """
    path = std / "sys/net/connection/socket/unix.rs"
    source = path.read_text(encoding="utf-8")
    shared_widened = (
        '#[cfg(any(target_os = "android", any(target_os = "linux", '
        'target_os = "octagram"), target_os = "cygwin"))]'
    )
    shared_original = (
        '#[cfg(any(target_os = "android", target_os = "linux", '
        'target_os = "cygwin"))]'
    )
    linux_widened = '#[cfg(any(target_os = "linux", target_os = "octagram"))]'
    linux_original = '#[cfg(target_os = "linux")]'
    edits = (
        ("set_quickack", shared_widened, shared_original),
        ("quickack", shared_widened, shared_original),
        ("set_deferaccept", linux_widened, linux_original),
        ("deferaccept", linux_widened, linux_original),
        ("set_passcred", shared_widened, shared_original),
        ("passcred", shared_widened, shared_original),
    )
    for method, widened_cfg, original_cfg in edits:
        widened = f"{widened_cfg}\n    pub fn {method}"
        original = f"{original_cfg}\n    pub fn {method}"
        if original in source:
            continue
        if source.count(widened) != 1:
            raise SystemExit(
                f"expected one widened or original Linux cfg for Socket::{method} in {path}"
            )
        source = source.replace(widened, original, 1)
    path.write_text(source, encoding="utf-8")
    print(
        "RUST-STD-OCTAGRAM-LINUX-SOCKET-OPTIONS-PASS: "
        "Linux-only socket option methods remain Linux-only"
    )


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
    std_override = os.environ.get("OCTAGRAM_RUST_STD_ROOT")
    if std_override:
        # x.py build-std compiles the pinned rust-lang/rust checkout, not the
        # rust-src copy installed in the host nightly sysroot.
        std = Path(std_override).resolve()
        library = std.parents[1]
    else:
        sysroot = Path(subprocess.check_output(
            ["rustc", f"+{TOOLCHAIN}", "--print", "sysroot"], text=True
        ).strip())
        library = sysroot / "lib/rustlib/src/rust/library"
        std = library / "std/src"
    if not (std / "os/unix/mod.rs").is_file():
        raise SystemExit(f"Rust std source directory not found: {std}")

    backport_thread_spawn_handle_fix(std)
    enable_std_file_locks(std)
    # OCTAGRAM libc implements accept() but deliberately does not expose accept4().
    # Preserve CLOEXEC semantics using accept() followed by fcntl, while leaving
    # Linux's accept4 fast path intact for Linux targets.
    socket_unix = std / "sys/net/connection/socket/unix.rs"
    old_accept_select = """        cfg_select! {
            any(
                target_os = "android",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "illumos",
                target_os = "linux",
                target_os = "hurd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "cygwin",
            ) => unsafe {
                let fd =
                    cvt_r(|| libc::accept4(self.as_raw_fd(), storage, len, libc::SOCK_CLOEXEC))?;
                Ok(Socket(FileDesc::from_raw_fd(fd)))
            },
            _ => unsafe {
                let fd = cvt_r(|| libc::accept(self.as_raw_fd(), storage, len))?;
                let fd = FileDesc::from_raw_fd(fd);
                fd.set_cloexec()?;
                Ok(Socket(fd))
            },
        }"""
    octagram_accept_select = """        cfg_select! {
            // OCTAGRAM has accept() but no accept4() ABI wrapper.
            target_os = "octagram" => unsafe {
                let fd = cvt_r(|| libc::accept(self.as_raw_fd(), storage, len))?;
                let fd = FileDesc::from_raw_fd(fd);
                fd.set_cloexec()?;
                Ok(Socket(fd))
            },
            any(
                target_os = "android",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "illumos",
                target_os = "linux",
                target_os = "hurd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "cygwin",
            ) => unsafe {
                let fd =
                    cvt_r(|| libc::accept4(self.as_raw_fd(), storage, len, libc::SOCK_CLOEXEC))?;
                Ok(Socket(FileDesc::from_raw_fd(fd)))
            },
            _ => unsafe {
                let fd = cvt_r(|| libc::accept(self.as_raw_fd(), storage, len))?;
                let fd = FileDesc::from_raw_fd(fd);
                fd.set_cloexec()?;
                Ok(Socket(fd))
            },
        }"""
    # A resumed build has already run widen_linux_cfg_gates(), which turns
    # the Linux arm inside this inserted branch into any(linux, octagram).
    # Recognize both exact forms so a checkpoint can safely run this patcher
    # again instead of failing on an already-patched socket source.
    socket_source = socket_unix.read_text(encoding="utf-8")
    widened_accept_select = octagram_accept_select.replace(
        'target_os = "linux",',
        'any(target_os = "linux", target_os = "octagram"),',
    )
    if octagram_accept_select in socket_source or widened_accept_select in socket_source:
        pass
    else:
        replace_once(socket_unix, old_accept_select, octagram_accept_select)
    print("RUST-STD-OCTAGRAM-ACCEPT-FALLBACK-PASS: accept + fcntl replaces unsupported accept4")

    # The upstream Linux stack-overflow handler queries pthread stack bounds.
    # OCTAGRAM does not yet expose pthread_getattr_np()/pthread_attr_getstack(),
    # so select the upstream no-op handler rather than linking to absent APIs or
    # pretending that userspace knows the current stack bounds.
    stack_overflow_mod = std / "sys/pal/unix/stack_overflow/mod.rs"
    replace_once(
        stack_overflow_mod,
        "cfg_select! {\n    // This is intentionally not enabled on iOS/tvOS/watchOS/visionOS, as it uses\n",
        "cfg_select! {\n    target_os = \"octagram\" => {\n        mod handler_none;\n        use handler_none as imp;\n    }\n    // This is intentionally not enabled on iOS/tvOS/watchOS/visionOS, as it uses\n",
    )
    print("RUST-STD-OCTAGRAM-STACK-OVERFLOW-NONE-PASS: avoids unavailable pthread stack introspection")

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

    # OCTAGRAM exposes clock_nanosleep on CLOCK_MONOTONIC. Keep the generic
    # Unix filesystem copy path: CachedFileMetadata is only consumed by the
    # Linux kernel_copy optimization, which is not enabled for this target.
    replace_once(
        std / "sys/thread/unix.rs",
        '    target_os = "vxworks",\n    target_os = "wasi",\n))]\npub fn sleep_until',
        '    target_os = "vxworks",\n    target_os = "wasi",\n'
        '    target_os = "octagram",\n))]\npub fn sleep_until',
    )

    # std/build.rs keeps the list of targets whose std is not "restricted"; without
    # this entry every user of std needs `#![feature(restricted_std)]`.
    replace_once(
        std.parent / "build.rs",
        '        || target_os == "vexos"\n',
        '        || target_os == "vexos"\n        || target_os == "octagram"\n',
    )

    target = std / "os/octagram"
    target.mkdir(parents=True, exist_ok=True)
    for name in ("mod.rs", "raw.rs", "fs.rs"):
        destination = target / name
        source = ROOT / "patches/rust-std/octagram" / name
        expected = source.read_text(encoding="utf-8")
        if destination.exists():
            if destination.read_text(encoding="utf-8") != expected:
                raise SystemExit(f"refusing to overwrite mismatched Rust std target file: {destination}")
        else:
            destination.write_text(expected, encoding="utf-8")

    # Restore old checkpoint edits before the global Linux cfg pass, then
    # keep target-specific socket options Linux-only after the shared PAL gates
    # have been widened.
    restore_linux_only_cfg_gates(std)
    gate_linux_extension_modules(std)
    allow_octagram_internal_socket_addr_ext(std)
    widen_linux_cfg_gates(std)
    preserve_linux_socket_option_cfgs(std)

    # Keep the Linux-specific public namespace Linux-only. The shared private
    # os::net::linux_ext module above it is still widened for OCTAGRAM because
    # generic Unix socket/address code uses it, but os::linux itself imports
    # Linux-only process extensions such as PidFd. Do not expose/compile those
    # modules for OCTAGRAM.
    os_mod = std / "os/mod.rs"
    os_source = os_mod.read_text(encoding="utf-8")
    lines = os_source.splitlines(keepends=True)
    changed = 0
    already_linux_only = 0
    for index in range(len(lines) - 1):
        if lines[index + 1].strip() != "pub mod linux;":
            continue
        condition = lines[index].strip()
        if condition == '#[cfg(any(target_os = "linux", target_os = "octagram"))]':
            indent = lines[index][:len(lines[index]) - len(lines[index].lstrip())]
            newline = "\r\n" if lines[index].endswith("\r\n") else "\n" if lines[index].endswith("\n") else ""
            lines[index] = f'{indent}#[cfg(target_os = "linux")]{newline}'
            changed += 1
        elif condition == '#[cfg(target_os = "linux")]':
            already_linux_only += 1
    if changed > 1 or (changed == 0 and already_linux_only != 1):
        raise SystemExit(
            "expected exactly one public os::linux module to restore or verify in "
            f"{os_mod} (changed={changed}, already_linux_only={already_linux_only})"
        )
    if changed:
        os_mod.write_text("".join(lines), encoding="utf-8")
    print("RUST-STD-OCTAGRAM-LINUX-OS-MODULE-PASS: os::linux remains Linux-only")

    print("RUST-STD-OCTAGRAM-PATCH-PASS: Unix target glue installed in rust-src")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

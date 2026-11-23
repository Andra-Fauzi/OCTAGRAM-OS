#!/usr/bin/env python3
"""Patch the installed rust-src component with the OCTAGRAM std PAL probe glue."""

from __future__ import annotations

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
    old_recovery = """/// If `thread::current()` ran before [`super::lifecycle::ThreadInit::init`] on a
/// newly spawned thread (e.g. during `DLL_THREAD_ATTACH` on Windows), replace
/// the temporary handle created by [`init_current`] with the one from `thread::spawn`.
pub(super) fn recover_preinitialized_for_spawn(thread: Thread) -> bool {
    let current = CURRENT.get();
    if current <= DESTROYED || current == NONE || current == BUSY {
        return false;
    }

    // SAFETY: `current` points to a valid `Thread` created by `init_current`.
    let old = unsafe { Thread::from_raw(current) };
    if old.name().is_some() {
        return false;
    }

    drop(old);
    CURRENT.set(NONE);
    id::set(thread.id());
    set_current(thread).is_ok()
}

/// Tries to set the thread handle for the current thread. Fails if a handle was
/// already set or if the thread ID of `thread` would change an already-set ID.
pub(super) fn set_current(thread: Thread) -> Result<(), Thread> {
    if CURRENT.get() != NONE {
        return Err(thread);
    }

    match id::get() {
        Some(id) if id == thread.id() => {}
        None => id::set(thread.id()),
        _ => return Err(thread),
    }

    // Make sure that `crate::rt::thread_cleanup` will be run, which will
    // call `drop_current`.
    crate::sys::thread_local::guard::enable();
    CURRENT.set(thread.into_raw().cast_mut());
    Ok(())
}
"""
    new_current = """/// Sets the thread handle for the current thread.
///
/// This is only called from [`super::lifecycle::ThreadInit::init`]. If
/// `thread::current()` ran earlier on this thread (e.g. during `DLL_THREAD_ATTACH`
/// on Windows), replace the temporary handle from [`init_current`] with the one from
/// `thread::spawn`.
pub(super) fn set_current(thread: Thread) {
    let current = CURRENT.get();
    if current > DESTROYED {
        // SAFETY: `current` points to a valid `Thread` created by `init_current`.
        unsafe {
            CURRENT.set(DESTROYED);
            drop(Thread::from_raw(current));
        }
    } else if current == BUSY {
        rtabort!(
            "set_current() was called while init_current() is in progress, \\
            which indicates a bug in the Rust threading implementation"
        );
    }

    id::set(thread.id());
    crate::sys::thread_local::guard::enable();
    CURRENT.set(thread.into_raw().cast_mut());
}
"""
    if old_recovery in source:
        current.write_text(source.replace(old_recovery, new_current, 1), encoding="utf-8")
    elif "pub(super) fn set_current(thread: Thread) {" in source and "set_current() was called while init_current()" in source:
        pass
    else:
        raise SystemExit("unsupported rust std thread/current.rs layout; refusing unsafe patch")

    lifecycle = std / "thread/lifecycle.rs"
    source = lifecycle.read_text(encoding="utf-8")
    old_import = "use super::current::{recover_preinitialized_for_spawn, set_current};"
    new_import = "use super::current::set_current;"
    if old_import in source:
        source = source.replace(old_import, new_import, 1)
        old_init = """        if let Err(thread) = set_current(self.handle.clone()) {
            if !recover_preinitialized_for_spawn(thread) {
                // The current thread should not have set yet. Use an abort to save binary size (see #123356).
                rtabort!("current thread handle already set during thread spawn");
            }
        }
"""
        new_init = """        set_current(self.handle.clone());
"""
        if source.count(old_init) != 1:
            raise SystemExit("unsupported rust std ThreadInit::init layout; refusing unsafe patch")
        source = source.replace(old_init, new_init, 1)
        lifecycle.write_text(source, encoding="utf-8")
    elif new_import in source and "set_current(self.handle.clone());" in source:
        pass
    else:
        raise SystemExit("unsupported rust std thread/lifecycle.rs layout; refusing unsafe patch")

    print("RUST-STD-THREAD-SPAWN-FIX-PASS: preinitialized thread handle is replaced")


def main() -> int:
    sysroot = Path(subprocess.check_output(
        ["rustc", "+nightly", "--print", "sysroot"], text=True
    ).strip())
    library = sysroot / "lib/rustlib/src/rust/library"
    std = library / "std/src"
    if not (std / "os/unix/mod.rs").is_file():
        raise SystemExit(f"rust-src component not found under {library}")

    backport_thread_spawn_handle_fix(std)

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

    target = std / "os/octagram"
    target.mkdir(parents=True, exist_ok=True)
    for name in ("mod.rs", "raw.rs", "fs.rs"):
        destination = target / name
        if destination.exists():
            raise SystemExit(f"refusing to overwrite existing Rust std target file: {destination}")
        source = ROOT / "patches/rust-std/octagram" / name
        destination.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")

    print("RUST-STD-OCTAGRAM-PATCH-PASS: Unix target glue installed in rust-src")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

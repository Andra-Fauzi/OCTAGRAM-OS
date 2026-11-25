#!/usr/bin/env python3
"""Apply the pinned OCTAGRAM libc integration using exact, checked source edits."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

EXPECTED_COMMIT = "ccc2ff4aaab962f7fde7b8edda7379c045b1ca12"
REPO_ROOT = Path(__file__).resolve().parents[1]
MODULE_SOURCE = REPO_ROOT / "patches/rust-libc/octagram/mod.rs"


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text(encoding="utf-8")
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"expected one exact anchor in {path}, found {count}")
    path.write_text(source.replace(old, new, 1), encoding="utf-8")


def main() -> int:
    arguments = [argument for argument in sys.argv[1:] if argument != "--allow-unpinned"]
    allow_unpinned = len(arguments) != len(sys.argv[1:])
    if len(arguments) != 1:
        raise SystemExit(
            "usage: apply_rust_libc_octagram.py UPSTREAM_LIBC_CHECKOUT [--allow-unpinned]"
        )
    root = Path(arguments[0]).resolve()
    # A vendored copy (for example the extracted crates.io source used by
    # `-Z build-std`) has no git history, so the pin cannot be verified there.
    # The anchors below still fail loudly if the sources moved.
    if not allow_unpinned:
        try:
            actual = subprocess.check_output(
                ["git", "rev-parse", "HEAD"], cwd=root, text=True, stderr=subprocess.DEVNULL
            ).strip()
        except (OSError, subprocess.CalledProcessError):
            raise SystemExit(
                f"{root} is not a git checkout; pass --allow-unpinned after checking "
                f"that it is libc {EXPECTED_COMMIT}"
            ) from None
        if actual != EXPECTED_COMMIT:
            raise SystemExit(f"wrong upstream libc revision: {actual}")

    replace_once(
        root / "build.rs",
        '            "helenos",\n',
        '            "helenos", "octagram",\n',
    )

    unix = root / "src/unix/mod.rs"
    replace_once(
        unix,
        '    } else if #[cfg(target_os = "haiku")] {\n'
        '        mod haiku;\n'
        '        pub use self::haiku::*;\n'
        '    } else if #[cfg(target_os = "redox")] {',
        '    } else if #[cfg(target_os = "haiku")] {\n'
        '        mod haiku;\n'
        '        pub use self::haiku::*;\n'
        '    } else if #[cfg(target_os = "octagram")] {\n'
        '        pub(crate) mod octagram;\n'
        '        pub use self::octagram::*;\n'
        '    } else if #[cfg(target_os = "redox")] {',
    )
    replace_once(unix, "    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_init$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_init(\n        lock: *mut crate::pthread_rwlock_t,\n        attr: *const crate::pthread_rwlockattr_t,\n    ) -> c_int;\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_destroy$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_destroy(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_rdlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_rdlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_tryrdlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_tryrdlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_wrlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_wrlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_trywrlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_trywrlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_unlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_unlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    pub fn pthread_rwlockattr_init(attr: *mut crate::pthread_rwlockattr_t) -> c_int;\n    pub fn pthread_rwlockattr_destroy(attr: *mut crate::pthread_rwlockattr_t) -> c_int;", "    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_init$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_init(\n        lock: *mut crate::pthread_rwlock_t,\n        attr: *const crate::pthread_rwlockattr_t,\n    ) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_destroy$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_destroy(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_rdlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_rdlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_tryrdlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_tryrdlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_wrlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_wrlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_trywrlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_trywrlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg_attr(\n        all(target_os = \"macos\", target_arch = \"x86\"),\n        link_name = \"pthread_rwlock_unlock$UNIX2003\"\n    )]\n    pub fn pthread_rwlock_unlock(lock: *mut crate::pthread_rwlock_t) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    pub fn pthread_rwlockattr_init(attr: *mut crate::pthread_rwlockattr_t) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    pub fn pthread_rwlockattr_destroy(attr: *mut crate::pthread_rwlockattr_t) -> c_int;")
    replace_once(unix, "    #[cfg(not(all(target_os = \"linux\", target_env = \"gnu\")))] // defined in new/glibc\n    #[cfg_attr(gnu_file_offset_bits64, link_name = \"statvfs64\")]\n    pub fn statvfs(path: *const c_char, buf: *mut crate::statvfs) -> c_int;\n    #[cfg(not(all(target_os = \"linux\", target_env = \"gnu\")))] // defined in new/glibc\n    #[cfg_attr(gnu_file_offset_bits64, link_name = \"fstatvfs64\")]\n    pub fn fstatvfs(fd: c_int, buf: *mut crate::statvfs) -> c_int;", "    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg(not(all(target_os = \"linux\", target_env = \"gnu\")))] // defined in new/glibc\n    #[cfg_attr(gnu_file_offset_bits64, link_name = \"statvfs64\")]\n    pub fn statvfs(path: *const c_char, buf: *mut crate::statvfs) -> c_int;\n    #[cfg(not(target_os = \"octagram\"))]\n    #[cfg(not(all(target_os = \"linux\", target_env = \"gnu\")))] // defined in new/glibc\n    #[cfg_attr(gnu_file_offset_bits64, link_name = \"fstatvfs64\")]\n    pub fn fstatvfs(fd: c_int, buf: *mut crate::statvfs) -> c_int;")
    replace_once(
        unix,
        '    if #[cfg(not(target_os = "aix"))] {\n'
        '        extern "C" {\n'
        '            pub fn dladdr(addr: *const c_void, info: *mut Dl_info) -> c_int;',
        '    if #[cfg(not(any(target_os = "aix", target_os = "octagram")))] {\n'
        '        extern "C" {\n'
        '            pub fn dladdr(addr: *const c_void, info: *mut Dl_info) -> c_int;',
    )

    # libc 0.2.x carries two module trees: the legacy `src/unix` one and the
    # newer `src/new` one that std actually resolves through. The OCTAGRAM ABI
    # module lives in the legacy tree, so make it visible from `src/new` too
    # instead of maintaining two copies of the same definitions.
    new_mod = root / "src/new/mod.rs"
    replace_once(
        new_mod,
        "mod common;\n",
        "mod common;\n"
        "\n"
        "#[cfg(target_os = \"octagram\")]\n"
        "use common::posix::unistd;\n"
        "#[cfg(target_os = \"octagram\")]\n"
        "pub use crate::unix::octagram::{\n"
        "    dirent64, fstat64, fstatat64, ftruncate64, lseek64, lstat64, open64, openat64,\n"
        "    pread64, pwrite64, readdir64, sendfile, sigaltstack, stat64, stack_t, syscall,\n"
        "    MAP_FAILED, MSG_NOSIGNAL, SA_ONSTACK, SA_SIGINFO, SS_DISABLE, SYS_futex, off64_t,\n"
        "    cmsghdr, CMSG_ALIGN, CMSG_DATA, CMSG_FIRSTHDR, CMSG_LEN, CMSG_NXTHDR, CMSG_SPACE,\n"
        "    SCM_RIGHTS,\n"
        "    FUTEX_WAIT, FUTEX_WAKE, FUTEX_REQUEUE, FUTEX_CMP_REQUEUE, FUTEX_WAKE_OP,\n"
        "    FUTEX_WAIT_BITSET, FUTEX_WAKE_BITSET, FUTEX_PRIVATE_FLAG, FUTEX_CLOCK_REALTIME,\n"
        "    FUTEX_WAIT_PRIVATE, FUTEX_WAKE_PRIVATE, FUTEX_REQUEUE_PRIVATE,\n"
        "    FUTEX_CMP_REQUEUE_PRIVATE, FUTEX_WAKE_OP_PRIVATE, FUTEX_WAIT_BITSET_PRIVATE,\n"
        "    FUTEX_WAKE_BITSET_PRIVATE,\n"
        "};\n",
    )
    # Cargo re-extracts a registry crate whenever this marker is missing, which
    # silently discards the edits above; keep the marker so the patched sources
    # are the ones cargo compiles.
    (root / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")

    target_module = root / "src/unix/octagram/mod.rs"
    target_module.parent.mkdir(parents=True, exist_ok=True)
    if target_module.exists():
        raise SystemExit(f"target module already exists: {target_module}")
    target_module.write_text(MODULE_SOURCE.read_text(encoding="utf-8"), encoding="utf-8")

    print("RUST-LIBC-OCTAGRAM-PATCH-PASS: exact source anchors and target module installed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

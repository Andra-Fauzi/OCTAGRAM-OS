#!/usr/bin/env python3
"""Apply the pinned OCTAGRAM libc integration using exact, checked source edits."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

EXPECTED_COMMIT = "ccc2ff4aaab962f7fde7b8edda7379c045b1ca12"
REPO_ROOT = Path(__file__).resolve().parents[1]
MODULE_SOURCE = REPO_ROOT / "patches/rust-libc/octagram/mod.rs"


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text(encoding="utf-8")
    # Bootstrap can be resumed with the same Cargo registry cache. Treat an
    # already-applied exact replacement as success instead of failing on the
    # now-consumed upstream anchor.
    if new in source:
        return
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
    target_module = root / "src/unix/octagram/mod.rs"
    expected_module = MODULE_SOURCE.read_text(encoding="utf-8")
    if target_module.exists() and target_module.read_text(encoding="utf-8") != expected_module:
        raise SystemExit(f"existing OCTAGRAM libc module does not match the pinned patch: {target_module}")
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

    # libc 0.2.191 added "helenos" to this check-cfg list; 0.2.186
    # predates that entry and ends the same list with "qurt".
    build_rs = root / "build.rs"
    build_source = build_rs.read_text(encoding="utf-8")
    if '"octagram"' not in build_source:
        if build_source.count('"helenos",') == 1:
            build_source = build_source.replace('"helenos",', '"helenos", "octagram",', 1)
        elif build_source.count('"qurt",') == 1:
            build_source = build_source.replace('"qurt",', '"qurt", "octagram",', 1)
        else:
            raise SystemExit(f"could not identify libc target_os check-cfg list in {build_rs}")
        build_rs.write_text(build_source, encoding="utf-8")


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
    statvfs_old = """    #[cfg(not(all(target_os = "linux", target_env = "gnu")))] // defined in new/glibc
    #[cfg_attr(gnu_file_offset_bits64, link_name = "statvfs64")]
    pub fn statvfs(path: *const c_char, buf: *mut crate::statvfs) -> c_int;
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))] // defined in new/glibc
    #[cfg_attr(gnu_file_offset_bits64, link_name = "fstatvfs64")]
    pub fn fstatvfs(fd: c_int, buf: *mut crate::statvfs) -> c_int;"""
    statvfs_new = """    #[cfg(not(target_os = "octagram"))]
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))] // defined in new/glibc
    #[cfg_attr(gnu_file_offset_bits64, link_name = "statvfs64")]
    pub fn statvfs(path: *const c_char, buf: *mut crate::statvfs) -> c_int;
    #[cfg(not(target_os = "octagram"))]
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))] // defined in new/glibc
    #[cfg_attr(gnu_file_offset_bits64, link_name = "fstatvfs64")]
    pub fn fstatvfs(fd: c_int, buf: *mut crate::statvfs) -> c_int;"""
    statvfs_older = """    #[cfg_attr(gnu_file_offset_bits64, link_name = "statvfs64")]
    pub fn statvfs(path: *const c_char, buf: *mut crate::statvfs) -> c_int;
    #[cfg_attr(gnu_file_offset_bits64, link_name = "fstatvfs64")]
    pub fn fstatvfs(fd: c_int, buf: *mut crate::statvfs) -> c_int;"""
    statvfs_older_new = """    #[cfg(not(target_os = "octagram"))]
    #[cfg_attr(gnu_file_offset_bits64, link_name = "statvfs64")]
    pub fn statvfs(path: *const c_char, buf: *mut crate::statvfs) -> c_int;
    #[cfg(not(target_os = "octagram"))]
    #[cfg_attr(gnu_file_offset_bits64, link_name = "fstatvfs64")]
    pub fn fstatvfs(fd: c_int, buf: *mut crate::statvfs) -> c_int;"""
    unix_source = unix.read_text(encoding="utf-8")
    if statvfs_new not in unix_source and statvfs_older_new not in unix_source:
        if unix_source.count(statvfs_old) == 1:
            unix_source = unix_source.replace(statvfs_old, statvfs_new, 1)
        elif unix_source.count(statvfs_older) == 1:
            unix_source = unix_source.replace(statvfs_older, statvfs_older_new, 1)
        else:
            raise SystemExit(f"could not find supported statvfs declarations in {unix}")
        unix.write_text(unix_source, encoding="utf-8")

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
    # newer `src/new` one that errno and std resolve through. The OCTAGRAM
    # ABI lives in the legacy tree. Re-export only the missing strerror_r API
    # from the new tree; all other target symbols already arrive via src/unix.
    new_mod = root / "src/new/mod.rs"
    new_source = new_mod.read_text(encoding="utf-8")

    # Older iterations inserted a broad OCTAGRAM re-export list here. Remove
    # only that exact syntactic block. Never search for the next generic `};`,
    # which could be the end of an unrelated cfg_if! and delete libc modules.
    legacy_export_re = re.compile(
        r'(?ms)^#\[cfg\(target_os = "octagram"\)\]\n'
        r'pub use crate::unix::octagram::\{\n.*?^\};\n?'
    )
    new_source, legacy_export_count = legacy_export_re.subn("", new_source)
    if legacy_export_count > 1:
        raise SystemExit(f"duplicate legacy OCTAGRAM re-export blocks in {new_mod}")
    if "pub use crate::unix::octagram::{" in new_source:
        raise SystemExit(f"unrecognized legacy OCTAGRAM re-export block in {new_mod}")

    unistd_import = (
        '#[cfg(target_os = "octagram")]\n'
        'use common::posix::unistd;\n'
    )
    if new_source.count(unistd_import) > 1:
        raise SystemExit(f"duplicate OCTAGRAM unistd imports in {new_mod}")
    if unistd_import not in new_source:
        if "mod common;\n" not in new_source:
            raise SystemExit(f"missing common module anchor in {new_mod}")
        new_source = new_source.replace(
            "mod common;\n", "mod common;\n\n" + unistd_import, 1
        )

    strerror_export = (
        '#[cfg(target_os = "octagram")]\n'
        'pub use crate::unix::octagram::strerror_r;\n'
    )
    if new_source.count(strerror_export) > 1:
        raise SystemExit(f"duplicate OCTAGRAM strerror_r exports in {new_mod}")
    if strerror_export not in new_source:
        new_source = new_source.replace(
            unistd_import, unistd_import + strerror_export, 1
        )
    if unistd_import not in new_source or strerror_export not in new_source:
        raise SystemExit(f"failed to install OCTAGRAM libc exports in {new_mod}")
    new_mod.write_text(new_source, encoding="utf-8")
    # Cargo re-extracts a registry crate whenever this marker is missing, which
    # silently discards the edits above; keep the marker so the patched sources
    # are the ones cargo compiles.
    (root / ".cargo-ok").write_text('{"v":1}', encoding="utf-8")

    target_module.parent.mkdir(parents=True, exist_ok=True)
    if not target_module.exists():
        target_module.write_text(expected_module, encoding="utf-8")

    print("RUST-LIBC-OCTAGRAM-PATCH-PASS: exact source anchors and target module installed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

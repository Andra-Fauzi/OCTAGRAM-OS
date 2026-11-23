#!/usr/bin/env python3
"""Check selected Rust/libc ABI facts against OCTAGRAM's public headers."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tools/rust_libc_abi_surface.json"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"RUST-LIBC-ABI-FAIL: {message}")


def main() -> int:
    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    require(data.get("schema_version") == 1, "unsupported schema")
    require(data["target"]["triple"] == "x86_64-unknown-octagram", "wrong target triple")
    require(data["target"]["pointer_bits"] == 64, "expected 64-bit target")

    require(data["upstream_libc"]["repository"] == "rust-lang/libc", "unexpected upstream libc repository")
    require(data["upstream_libc"]["base_commit"] == "ccc2ff4aaab962f7fde7b8edda7379c045b1ca12",
            "upstream libc pin changed; audit the applicator before updating")
    for relative in (data["upstream_libc"]["applicator"], data["upstream_libc"]["target_module"]):
        require((ROOT / relative).is_file(), f"missing upstream libc integration input: {relative}")

    types = read("user/include/sys/types.h")
    expected_types = {
        "off_t": "int64_t", "ssize_t": "int64_t", "pid_t": "int32_t",
        "mode_t": "uint32_t", "time_t": "int64_t", "uid_t": "uint32_t",
        "gid_t": "uint32_t", "dev_t": "uint64_t", "ino_t": "uint64_t",
    }
    for name, c_type in expected_types.items():
        require(re.search(rf"typedef\s+{c_type}\s+{name}\s*;", types) is not None,
                f"{name} no longer matches {c_type}")

    octagram = read("user/include/octagram.h")
    require(re.search(r"struct\s+oct_stat\s*\{\s*uint64_t\s+size;\s*uint32_t\s+mode;\s*uint32_t\s+modified_seconds;", octagram, re.S) is not None,
            "oct_stat kernel ABI changed; update manifest and PAL mapping")

    dirent = read("user/include/dirent.h")
    require(re.search(r"struct\s+dirent\s*\{\s*uint64_t\s+d_ino;\s*off_t\s+d_off;\s*uint16_t\s+d_reclen;\s*unsigned\s+char\s+d_type;\s*char\s+d_name\[256\];", dirent, re.S) is not None,
            "dirent layout changed; update manifest and tests")

    unistd = read("user/include/unistd.h")
    require("int link(const char *oldpath, const char *newpath);" in unistd
            and "int linkat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath, int flags);" in unistd,
            "POSIX link/linkat declarations missing from public unistd ABI")

    runtime = read("user/runtime.c")
    require(re.search(r"int link\\(const char \\*oldpath, const char \\*newpath\\).*?return linkat\\(OCT_AT_FDCWD, oldpath, OCT_AT_FDCWD, newpath, 0\\);", runtime, re.S) is not None,
            "link must delegate to linkat using AT_FDCWD")
    require(re.search(r"int linkat\\(int olddirfd, const char \\*oldpath, int newdirfd,.*?oct_linkat\\(olddirfd, oldpath, newdirfd,.*?newpath, flags\\)", runtime, re.S) is not None,
            "linkat must call the kernel linkat wrapper")

    require("DIR *fdopendir(int fd);" in dirent,
            "fdopendir declaration missing from public dirent ABI")

    timespec = read("user/include/time.h")
    require("time_t tv_sec;" in timespec and "long tv_nsec;" in timespec,
            "timespec fields changed")

    stat_header = read("user/include/sys/stat.h")
    require(re.search(
        r"struct\s+stat\s*\{\s*off_t\s+st_size;\s*mode_t\s+st_mode;\s*dev_t\s+st_dev;\s*ino_t\s+st_ino;\s*nlink_t\s+st_nlink;\s*uid_t\s+st_uid;\s*gid_t\s+st_gid;\s*time_t\s+st_atime;\s*time_t\s+st_mtime;\s*time_t\s+st_ctime;\s*blksize_t\s+st_blksize;\s*blkcnt_t\s+st_blocks;",
        stat_header, re.S) is not None,
        "public struct stat field order changed")

    socket_header = read("user/include/sys/socket.h")
    require("typedef uint32_t socklen_t;" in socket_header
            and "typedef uint16_t sa_family_t;" in socket_header
            and re.search(r"struct\s+sockaddr\s*\{\s*sa_family_t\s+sa_family;\s*char\s+sa_data\[14\];", socket_header, re.S) is not None,
            "sockaddr ABI changed")

    inet_header = read("user/include/netinet/in.h")
    require("#define IPV6_ADD_MEMBERSHIP 20" in inet_header
            and "#define IPV6_DROP_MEMBERSHIP 21" in inet_header,
            "IPv6 multicast membership ABI constants changed")
    netdb = read("user/include/netdb.h")
    require(re.search(r"struct\s+addrinfo\s*\{\s*int\s+ai_flags;\s*int\s+ai_family;\s*int\s+ai_socktype;\s*int\s+ai_protocol;\s*socklen_t\s+ai_addrlen;\s*struct\s+sockaddr\s*\*ai_addr;\s*char\s*\*ai_canonname;\s*struct\s+addrinfo\s*\*ai_next;", netdb, re.S) is not None,
            "addrinfo field order changed")

    require("#define MSG_PEEK 0x02" in socket_header
            and "#define SO_LINGER 13" in socket_header,
            "socket peek/linger constants changed")

    pthread_header = read("user/include/pthread.h")
    require("typedef uint64_t pthread_t;" in pthread_header
            and "volatile unsigned int state;" in pthread_header
            and "volatile unsigned int flags;" in pthread_header
            and "uintptr_t opaque;" in pthread_header,
            "pthread object ABI changed")

    termios_header = read("user/include/termios.h")
    require("typedef unsigned int tcflag_t;" in termios_header
            and "typedef unsigned char cc_t;" in termios_header
            and "typedef unsigned int speed_t;" in termios_header
            and "cc_t c_cc[NCCS];" in termios_header and "#define NCCS 32" in termios_header,
            "termios ABI changed")

    semaphore_header = read("user/include/sys/semaphore.h")
    require(re.search(r"typedef\s+struct\s*\{\s*volatile\s+unsigned\s+int\s+count;", semaphore_header, re.S) is not None,
            "sem_t ABI changed")

    select_header = read("user/include/sys/select.h")
    require("uint64_t bits[FD_SETSIZE / 64];" in select_header and "#define FD_SETSIZE 1024" in select_header,
            "fd_set ABI changed")

    signal_header = read("user/include/signal.h")
    require("sigset_t sa_mask;" in signal_header
            and "int sa_flags;" in signal_header
            and "void (*sa_restorer)(void);" in signal_header,
            "sigaction ABI changed")

    module = read("patches/rust-libc/octagram/mod.rs")
    for assertion in (
        "core::mem::size_of::<stat>()",
        "core::mem::size_of::<dirent>()",
        "core::mem::size_of::<pthread_mutex_t>()",
        "core::mem::size_of::<termios>()",
        "core::mem::size_of::<sigaction>()",
        "core::mem::size_of::<fd_set>()",
    ):
        require(assertion in module, f"missing compile-time ABI assertion: {assertion}")

    runtime = read("user/runtime.c")
    for field in ("result->st_size = (off_t)metadata.size;",
                  "result->st_mode = (mode_t)metadata.mode;",
                  "result->st_mtime = (time_t)metadata.modified_seconds;"):
        require(field in runtime, f"stat translation no longer contains: {field}")

    pthread = read("user/runtime.c")
    require(re.search(r"int pthread_detach\(pthread_t thread\).*?return ENOTSUP;", pthread, re.S) is not None,
            "pthread_detach behavior changed; update capability notes")

    require(re.search(
        r"DIR \*fdopendir\(int fd\).*?fstat\(fd, &metadata\).*?S_ISDIR\(metadata\.st_mode\).*?directory->fd = fd;",
        runtime, re.S) is not None,
        "fdopendir must validate a directory and transfer descriptor ownership on success")

    gaps = data.get("known_gaps", [])
    require(any("not glibc or musl" in item for item in gaps),
            "must preserve explicit libc ABI boundary")
    require(any("does not prove Rust std" in item for item in gaps),
            "must not overclaim Rust std compatibility")

    print("RUST-LIBC-ABI-PASS: scalar types, kernel/public records, pthread, socket, time, terminal and signal layouts")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

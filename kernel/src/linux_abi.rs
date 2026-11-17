//! Linux x86_64 system-call compatibility layer.
//!
//! Debian/Ubuntu ELF binaries enter the kernel with the `syscall` instruction,
//! not OCTAGRAM's `int 0x80`, and they pass Linux syscall numbers and Linux
//! data layouts.  This module translates the numbers and the handful of
//! structures whose layout differs (stat, sigaction, dirent, rlimit, ...).
//!
//! The layer is intentionally a translation layer, not a second kernel: file,
//! memory, process, signal, futex, socket, epoll and timer operations are
//! routed to the existing OCTAGRAM implementations.  Unsupported calls return
//! `-ENOSYS`, which is what glibc falls back on for optional interfaces such
//! as `rseq` and `clone3`.

use crate::arch::x86_64::interrupt::InterruptFrame;
use crate::arch::x86_64::thread_pointer;
use crate::memory::paging;

// Linux syscall numbers (x86_64).
const LX_READ: u64 = 0;
const LX_WRITE: u64 = 1;
const LX_OPEN: u64 = 2;
const LX_CLOSE: u64 = 3;

// Linux kernel termios ioctls. The user buffer is the 36-byte kernel layout,
// not the 60-byte glibc layout the OCTAGRAM ABI uses.
const LX_TCGETS: u64 = 0x5401;
const LX_TCSETS: u64 = 0x5402;
const LX_TCSETSW: u64 = 0x5403;
const LX_TCSETSF: u64 = 0x5404;
const LX_STAT: u64 = 4;
const LX_FSTAT: u64 = 5;
const LX_LSTAT: u64 = 6;
const LX_POLL: u64 = 7;
const LX_LSEEK: u64 = 8;
const LX_MMAP: u64 = 9;
const LX_MPROTECT: u64 = 10;
const LX_MUNMAP: u64 = 11;
const LX_BRK: u64 = 12;
const LX_RT_SIGACTION: u64 = 13;
const LX_RT_SIGPROCMASK: u64 = 14;
const LX_IOCTL: u64 = 16;
const LX_PREAD64: u64 = 17;
const LX_PWRITE64: u64 = 18;
const LX_READV: u64 = 19;
const LX_WRITEV: u64 = 20;
const LX_ACCESS: u64 = 21;
const LX_PIPE: u64 = 22;
const LX_SELECT: u64 = 23;
const LX_SCHED_YIELD: u64 = 24;
const LX_MREMAP: u64 = 25;
const LX_MSYNC: u64 = 26;
const LX_MINCORE: u64 = 27;
const LX_MADVISE: u64 = 28;
const LX_DUP: u64 = 32;
const LX_DUP2: u64 = 33;
const LX_NANOSLEEP: u64 = 35;
const LX_GETPID: u64 = 39;
const LX_SOCKET: u64 = 41;
const LX_CONNECT: u64 = 42;
const LX_ACCEPT: u64 = 43;
const LX_SENDTO: u64 = 44;
const LX_RECVFROM: u64 = 45;
const LX_SHUTDOWN: u64 = 48;
const LX_BIND: u64 = 49;
const LX_LISTEN: u64 = 50;
const LX_GETSOCKNAME: u64 = 51;
const LX_GETPEERNAME: u64 = 52;
const LX_SOCKETPAIR: u64 = 53;
const LX_SETSOCKOPT: u64 = 54;
const LX_GETSOCKOPT: u64 = 55;
const LX_CLONE: u64 = 56;
const LX_FORK: u64 = 57;
const LX_EXECVE: u64 = 59;
const LX_EXIT: u64 = 60;
const LX_WAIT4: u64 = 61;
const LX_KILL: u64 = 62;
const LX_UNAME: u64 = 63;
const LX_FCNTL: u64 = 72;
const LX_FSYNC: u64 = 74;
const LX_FDATASYNC: u64 = 75;
const LX_TRUNCATE: u64 = 76;
const LX_FTRUNCATE: u64 = 77;
const LX_GETCWD: u64 = 79;
const LX_CHDIR: u64 = 80;
const LX_RENAME: u64 = 82;
const LX_MKDIR: u64 = 83;
const LX_RMDIR: u64 = 84;
const LX_UNLINK: u64 = 87;
const LX_SYMLINK: u64 = 88;
const LX_READLINK: u64 = 89;
const LX_CHMOD: u64 = 90;
const LX_FCHMOD: u64 = 91;
const LX_UMASK: u64 = 95;
const LX_GETTIMEOFDAY: u64 = 96;
const LX_GETRLIMIT: u64 = 97;
const LX_GETRUSAGE: u64 = 98;
const LX_SYSINFO: u64 = 99;
const LX_TIMES: u64 = 100;
const LX_GETUID: u64 = 102;
const LX_GETGID: u64 = 104;
const LX_GETEUID: u64 = 107;
const LX_GETEGID: u64 = 108;
const LX_GETPPID: u64 = 110;
const LX_SETSID: u64 = 112;
const LX_GETPGID: u64 = 121;
const LX_CAPGET: u64 = 125;
const LX_SIGALTSTACK: u64 = 131;
const LX_PERSONALITY: u64 = 135;
const LX_MLOCK: u64 = 149;
const LX_MUNLOCK: u64 = 150;
const LX_MLOCKALL: u64 = 151;
const LX_MUNLOCKALL: u64 = 152;
const LX_PRCTL: u64 = 157;
const LX_ARCH_PRCTL: u64 = 158;
const LX_SETRLIMIT: u64 = 160;
const LX_GETTID: u64 = 186;
const LX_READAHEAD: u64 = 187;
const LX_TIME: u64 = 201;
const LX_FUTEX: u64 = 202;
const LX_SCHED_GETAFFINITY: u64 = 204;
const LX_GETDENTS64: u64 = 217;
const LX_SET_TID_ADDRESS: u64 = 218;
const LX_FADVISE64: u64 = 221;
const LX_CLOCK_GETTIME: u64 = 228;
const LX_CLOCK_GETRES: u64 = 229;
const LX_CLOCK_NANOSLEEP: u64 = 230;
const LX_EXIT_GROUP: u64 = 231;
const LX_EPOLL_WAIT: u64 = 232;
const LX_EPOLL_CTL: u64 = 233;
const LX_TGKILL: u64 = 234;
const LX_OPENAT: u64 = 257;
const LX_MKDIRAT: u64 = 258;
const LX_NEWFSTATAT: u64 = 262;
const LX_UNLINKAT: u64 = 263;
const LX_RENAMEAT: u64 = 264;
const LX_SYMLINKAT: u64 = 266;
const LX_READLINKAT: u64 = 267;
const LX_FACCESSAT: u64 = 269;
const LX_PPOLL: u64 = 271;
const LX_SET_ROBUST_LIST: u64 = 273;
const LX_EPOLL_PWAIT: u64 = 281;
const LX_TIMERFD_CREATE: u64 = 283;
const LX_EVENTFD: u64 = 284;
const LX_TIMERFD_SETTIME: u64 = 286;
const LX_TIMERFD_GETTIME: u64 = 287;
const LX_EVENTFD2: u64 = 290;
const LX_EPOLL_CREATE1: u64 = 291;
const LX_DUP3: u64 = 292;
const LX_PIPE2: u64 = 293;
const LX_PRLIMIT64: u64 = 302;
const LX_GETCPU: u64 = 309;
const LX_GETRANDOM: u64 = 318;
const LX_MEMBARRIER: u64 = 324;
const LX_STATX: u64 = 332;
const LX_RSEQ: u64 = 334;
const LX_CLONE3: u64 = 435;

// OCTAGRAM syscall numbers used by the translation table.
const OCT_EXIT: u64 = 0;
const OCT_WRITE: u64 = 1;
const OCT_READ: u64 = 2;
const OCT_OPEN: u64 = 3;
const OCT_CLOSE: u64 = 4;
const OCT_LSEEK: u64 = 5;
const OCT_MKDIR: u64 = 7;
const OCT_UNLINK: u64 = 8;
const OCT_FTRUNCATE: u64 = 9;
const OCT_GETPID: u64 = 11;
const OCT_CHDIR: u64 = 13;
const OCT_GETCWD: u64 = 14;
const OCT_MMAP: u64 = 16;
const OCT_MUNMAP: u64 = 17;
const OCT_WAITPID: u64 = 20;
const OCT_PIPE: u64 = 21;
const OCT_DUP2: u64 = 22;
const OCT_CHMOD: u64 = 25;
const OCT_ACCESS: u64 = 30;
const OCT_RENAME: u64 = 31;
const OCT_UMASK: u64 = 32;
const OCT_DUP: u64 = 33;
const OCT_FCNTL: u64 = 34;
const OCT_RMDIR: u64 = 35;
const OCT_SYMLINK: u64 = 36;
const OCT_READLINK: u64 = 37;
const OCT_FCHMOD: u64 = 39;
const OCT_OPENAT: u64 = 40;
const OCT_MKDIRAT: u64 = 42;
const OCT_UNLINKAT: u64 = 43;
const OCT_RENAMEAT: u64 = 44;
const OCT_READLINKAT: u64 = 45;
const OCT_SYMLINKAT: u64 = 46;
const OCT_EXECVE: u64 = 47;
const OCT_FORK: u64 = 48;
const OCT_CLOCK_GETTIME: u64 = 50;
const OCT_MSYNC: u64 = 51;
const OCT_MPROTECT: u64 = 52;
const OCT_MINCORE: u64 = 53;
const OCT_MLOCK: u64 = 54;
const OCT_MUNLOCK: u64 = 55;
const OCT_MLOCKALL: u64 = 56;
const OCT_MUNLOCKALL: u64 = 57;
const OCT_POSIX_MADVISE: u64 = 58;
const OCT_SET_THREAD_POINTER: u64 = 59;
const OCT_SOCKET: u64 = 71;
const OCT_BIND: u64 = 72;
const OCT_LISTEN: u64 = 73;
const OCT_ACCEPT: u64 = 74;
const OCT_CONNECT: u64 = 75;
const OCT_SEND: u64 = 76;
const OCT_RECV: u64 = 77;
const OCT_SHUTDOWN: u64 = 78;
const OCT_GETSOCKNAME: u64 = 79;
const OCT_GETPEERNAME: u64 = 80;
const OCT_IOCTL: u64 = 81;
const OCT_GETUID: u64 = 82;
const OCT_GETGID: u64 = 83;
const OCT_GETEUID: u64 = 84;
const OCT_GETEGID: u64 = 85;
const OCT_UNAME: u64 = 86;
const OCT_GETPPID: u64 = 87;
const OCT_BRK: u64 = 88;
const OCT_GETTIMEOFDAY: u64 = 89;
const OCT_NANOSLEEP: u64 = 90;
const OCT_KILL: u64 = 91;
const OCT_SIGPROCMASK: u64 = 93;
const OCT_FUTEX: u64 = 94;
const OCT_SETSOCKOPT: u64 = 95;
const OCT_GETSOCKOPT: u64 = 96;
const OCT_POLL: u64 = 97;
const OCT_PIPE2: u64 = 98;
const OCT_DUP3: u64 = 99;
const OCT_EVENTFD: u64 = 100;
const OCT_TIMERFD_CREATE: u64 = 101;
const OCT_PREAD: u64 = 102;
const OCT_PWRITE: u64 = 103;
const OCT_WRITEV: u64 = 104;
const OCT_READV: u64 = 105;
const OCT_TIMES: u64 = 112;
const OCT_SELECT: u64 = 117;
const OCT_CLOCK_GETRES: u64 = 118;
const OCT_FSYNC: u64 = 119;
const OCT_FDATASYNC: u64 = 120;
const OCT_GETRANDOM: u64 = 121;
const OCT_EPOLL_CREATE1: u64 = 122;
const OCT_EPOLL_CTL: u64 = 123;
const OCT_EPOLL_WAIT: u64 = 124;
const OCT_TIMERFD_SETTIME: u64 = 125;
const OCT_TIMERFD_GETTIME: u64 = 126;
const OCT_SOCKETPAIR: u64 = 127;
const OCT_CLOCK_NANOSLEEP: u64 = 128;

// Linux errno values (shared with the OCTAGRAM ABI).
const EPERM: i32 = 1;
const ENOENT: i32 = 2;
const EINTR: i32 = 4;
const EBADF: i32 = 9;
const EAGAIN: i32 = 11;
const ENOMEM: i32 = 12;
const EACCES: i32 = 13;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ENOSYS: i32 = 38;

const AT_FDCWD: u64 = (-100i64) as u64;
const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
const AT_EMPTY_PATH: u64 = 0x1000;
const ARCH_SET_FS: u64 = 0x1002;
const ARCH_GET_FS: u64 = 0x1003;

const RLIMIT_NOFILE: u64 = 7;
const RLIMIT_NPROC: u64 = 6;
const RLIMIT_STACK: u64 = 3;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserTimespec {
    seconds: i64,
    nanoseconds: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxStat {
    dev: u64,
    ino: u64,
    nlink: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    pad0: u32,
    rdev: u64,
    size: i64,
    blksize: i64,
    blocks: i64,
    atime: i64,
    atime_nsec: u64,
    mtime: i64,
    mtime_nsec: u64,
    ctime: i64,
    ctime_nsec: u64,
    unused: [i64; 3],
}

const _: () = assert!(core::mem::size_of::<LinuxStat>() == 144);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxStatxTimestamp {
    seconds: i64,
    nanoseconds: u32,
    reserved: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxStatx {
    mask: u32,
    blksize: u32,
    attributes: u64,
    nlink: u32,
    uid: u32,
    gid: u32,
    mode: u16,
    spare0: u16,
    ino: u64,
    size: u64,
    blocks: u64,
    attributes_mask: u64,
    atime: LinuxStatxTimestamp,
    btime: LinuxStatxTimestamp,
    ctime: LinuxStatxTimestamp,
    mtime: LinuxStatxTimestamp,
    rdev_major: u32,
    rdev_minor: u32,
    dev_major: u32,
    dev_minor: u32,
    mnt_id: u64,
    dio_mem_align: u32,
    dio_offset_align: u32,
    spare: [u64; 12],
}

const _: () = assert!(core::mem::size_of::<LinuxStatx>() == 256);

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxRlimit {
    current: u64,
    maximum: u64,
}

/// Translate one Linux syscall and return the frame to resume.
pub fn dispatch(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let number = frame.rax;
    let (arg1, arg2, arg3, arg4, arg5, arg6) = (
        frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9,
    );

    // Calls that may block or replace the saved frame go through the shared
    // OCTAGRAM handler so pipe waits, signals and exec/fork keep working.
    match number {
        LX_EXIT | LX_EXIT_GROUP => return internal(frame, OCT_EXIT),
        LX_FORK => return internal(frame, OCT_FORK),
        LX_EXECVE => return internal(frame, OCT_EXECVE),
        LX_WAIT4 => return internal(frame, OCT_WAITPID),
        LX_FUTEX => return internal(frame, OCT_FUTEX),
        LX_POLL => return internal(frame, OCT_POLL),
        LX_SELECT => return internal(frame, OCT_SELECT),
        LX_CLOCK_NANOSLEEP => return internal(frame, OCT_CLOCK_NANOSLEEP),
        LX_EPOLL_PWAIT => return internal(frame, OCT_EPOLL_WAIT),
        LX_PPOLL => {
            frame.rdx = match ppoll_timeout(arg3) {
                Ok(timeout) => timeout,
                Err(errno) => {
                    frame.rax = negative(errno);
                    return frame as *mut InterruptFrame;
                }
            };
            return internal(frame, OCT_POLL);
        }
        LX_SENDTO => {
            if arg5 == 0 {
                return finish(frame, crate::elf::linux_internal_call(
                    OCT_SEND, arg1, arg2, arg3, arg4, 0, 0,
                ));
            }
            return finish(frame, Err(ENOSYS));
        }
        LX_IOCTL => return finish(frame, linux_ioctl(arg1, arg2, arg3)),
        LX_MMAP => {
            // Linux-only advisory flags that do not change the mapping itself.
            const MAP_NORESERVE: u64 = 0x4000;
            const MAP_POPULATE: u64 = 0x8000;
            const MAP_STACK: u64 = 0x2_0000;
            const MAP_FIXED_NOREPLACE: u64 = 0x10_0000;
            let flags = arg4
                & !(MAP_NORESERVE | MAP_POPULATE | MAP_STACK | MAP_FIXED_NOREPLACE);
            return finish(frame, translate(LX_MMAP, arg1, arg2, arg3, flags, arg5, arg6));
        }
        LX_RECVFROM => {
            if arg5 == 0 {
                return finish(frame, crate::elf::linux_internal_call(
                    OCT_RECV, arg1, arg2, arg3, arg4, 0, 0,
                ));
            }
            return finish(frame, Err(ENOSYS));
        }
        _ => {}
    }

    finish(frame, translate(number, arg1, arg2, arg3, arg4, arg5, arg6))
}

fn internal(frame: &mut InterruptFrame, number: u64) -> *mut InterruptFrame {
    frame.rax = number;
    crate::elf::linux_internal_frame(frame)
}

fn finish(frame: &mut InterruptFrame, result: Result<u64, i32>) -> *mut InterruptFrame {
    frame.rax = match result {
        Ok(value) => value,
        Err(errno) => negative(errno),
    };
    frame as *mut InterruptFrame
}

fn negative(errno: i32) -> u64 {
    (-(errno as i64)) as u64
}

fn call(
    number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> Result<u64, i32> {
    crate::elf::linux_internal_call(number, arg1, arg2, arg3, arg4, arg5, arg6)
}

fn translate(
    number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> Result<u64, i32> {
    match number {
        LX_OPEN => call(OCT_OPEN, arg1, linux_open_flags(arg2), arg3, 0, 0, 0),
        LX_OPENAT => call(
            OCT_OPENAT,
            arg1,
            arg2,
            linux_open_flags(arg3),
            arg4,
            0,
            0,
        ),
        LX_ARCH_PRCTL => arch_prctl(arg1, arg2),
        LX_SET_TID_ADDRESS => Ok(crate::elf::linux_current_pid()),
        LX_SET_ROBUST_LIST => Ok(0),
        LX_RSEQ => Err(ENOSYS),
        LX_PRCTL => Ok(0),
        LX_GETRLIMIT => write_rlimit(arg1, arg2),
        LX_SETRLIMIT => Ok(0),
        LX_PRLIMIT64 => write_prlimit(arg1, arg2, arg3),
        LX_SCHED_GETAFFINITY => sched_getaffinity(arg1, arg2, arg3),
        LX_SYSINFO => sysinfo(arg1),
        LX_GETCPU => getcpu(arg1, arg2),
        LX_MEMBARRIER => Ok(0),
        LX_PERSONALITY => Ok(0),
        LX_GETPGID | LX_SETSID => Ok(crate::elf::linux_current_pid()),
        LX_GETTID => Ok(crate::elf::linux_current_pid()),
        LX_TIME => linux_time(arg1),
        LX_SCHED_YIELD => Ok(0),
        LX_KILL => call(OCT_KILL, arg1, arg2, 0, 0, 0, 0),
        LX_TGKILL => call(OCT_KILL, arg1, arg3, 0, 0, 0, 0),
        LX_RT_SIGPROCMASK => call(OCT_SIGPROCMASK, arg1, arg2, arg3, 0, 0, 0),
        LX_RT_SIGACTION => crate::elf::linux_sigaction(arg1, arg2, arg3),
        LX_SIGALTSTACK => Err(ENOSYS),
        LX_STAT => stat_path(arg1, arg2, true),
        LX_LSTAT => stat_path(arg1, arg2, false),
        LX_FSTAT => stat_fd(arg1, arg2),
        LX_NEWFSTATAT => newfstatat(arg1, arg2, arg3, arg4),
        LX_STATX => statx(arg1, arg2, arg3, arg5),
        LX_GETDENTS64 => crate::elf::linux_getdents64(arg1, arg2, arg3),
        LX_GETRUSAGE => zero_fill(arg2, 144),
        LX_CAPGET => Err(ENOSYS),
        LX_FACCESSAT => faccessat(arg1, arg2, arg3),
        LX_TRUNCATE => truncate(arg1, arg2),
        LX_EVENTFD => call(OCT_EVENTFD, arg1, 0, 0, 0, 0, 0),
        LX_EVENTFD2 => call(OCT_EVENTFD, arg1, arg2, 0, 0, 0, 0),
        LX_MREMAP | LX_CLONE | LX_CLONE3 => Err(ENOSYS),
        LX_READAHEAD | LX_FADVISE64 => Ok(0),
        _ => match internal_number(number) {
            Some(internal) => call(internal, arg1, arg2, arg3, arg4, arg5, arg6),
            None => Err(ENOSYS),
        },
    }
}

fn internal_number(number: u64) -> Option<u64> {
    Some(match number {
        LX_READ => OCT_READ,
        LX_WRITE => OCT_WRITE,
        LX_CLOSE => OCT_CLOSE,
        LX_LSEEK => OCT_LSEEK,
        LX_MMAP => OCT_MMAP,
        LX_MPROTECT => OCT_MPROTECT,
        LX_MUNMAP => OCT_MUNMAP,
        LX_BRK => OCT_BRK,
        LX_IOCTL => OCT_IOCTL,
        LX_PREAD64 => OCT_PREAD,
        LX_PWRITE64 => OCT_PWRITE,
        LX_READV => OCT_READV,
        LX_WRITEV => OCT_WRITEV,
        LX_ACCESS => OCT_ACCESS,
        LX_PIPE => OCT_PIPE,
        LX_DUP => OCT_DUP,
        LX_DUP2 => OCT_DUP2,
        LX_NANOSLEEP => OCT_NANOSLEEP,
        LX_GETPID => OCT_GETPID,
        LX_SOCKET => OCT_SOCKET,
        LX_CONNECT => OCT_CONNECT,
        LX_ACCEPT => OCT_ACCEPT,
        LX_SHUTDOWN => OCT_SHUTDOWN,
        LX_BIND => OCT_BIND,
        LX_LISTEN => OCT_LISTEN,
        LX_GETSOCKNAME => OCT_GETSOCKNAME,
        LX_GETPEERNAME => OCT_GETPEERNAME,
        LX_SOCKETPAIR => OCT_SOCKETPAIR,
        LX_SETSOCKOPT => OCT_SETSOCKOPT,
        LX_GETSOCKOPT => OCT_GETSOCKOPT,
        LX_UNAME => OCT_UNAME,
        LX_FCNTL => OCT_FCNTL,
        LX_FSYNC => OCT_FSYNC,
        LX_FDATASYNC => OCT_FDATASYNC,
        LX_FTRUNCATE => OCT_FTRUNCATE,
        LX_GETCWD => OCT_GETCWD,
        LX_CHDIR => OCT_CHDIR,
        LX_RENAME => OCT_RENAME,
        LX_MKDIR => OCT_MKDIR,
        LX_RMDIR => OCT_RMDIR,
        LX_UNLINK => OCT_UNLINK,
        LX_SYMLINK => OCT_SYMLINK,
        LX_READLINK => OCT_READLINK,
        LX_CHMOD => OCT_CHMOD,
        LX_FCHMOD => OCT_FCHMOD,
        LX_UMASK => OCT_UMASK,
        LX_GETTIMEOFDAY => OCT_GETTIMEOFDAY,
        LX_TIMES => OCT_TIMES,
        LX_GETUID => OCT_GETUID,
        LX_GETGID => OCT_GETGID,
        LX_GETEUID => OCT_GETEUID,
        LX_GETEGID => OCT_GETEGID,
        LX_GETPPID => OCT_GETPPID,
        LX_MLOCK => OCT_MLOCK,
        LX_MUNLOCK => OCT_MUNLOCK,
        LX_MLOCKALL => OCT_MLOCKALL,
        LX_MUNLOCKALL => OCT_MUNLOCKALL,
        LX_MSYNC => OCT_MSYNC,
        LX_MINCORE => OCT_MINCORE,
        LX_MADVISE => OCT_POSIX_MADVISE,
        LX_CLOCK_GETTIME => OCT_CLOCK_GETTIME,
        LX_CLOCK_GETRES => OCT_CLOCK_GETRES,
        LX_EPOLL_WAIT => OCT_EPOLL_WAIT,
        LX_EPOLL_CTL => OCT_EPOLL_CTL,
        LX_EPOLL_CREATE1 => OCT_EPOLL_CREATE1,
        LX_MKDIRAT => OCT_MKDIRAT,
        LX_UNLINKAT => OCT_UNLINKAT,
        LX_RENAMEAT => OCT_RENAMEAT,
        LX_SYMLINKAT => OCT_SYMLINKAT,
        LX_READLINKAT => OCT_READLINKAT,
        LX_DUP3 => OCT_DUP3,
        LX_PIPE2 => OCT_PIPE2,
        LX_TIMERFD_CREATE => OCT_TIMERFD_CREATE,
        LX_TIMERFD_SETTIME => OCT_TIMERFD_SETTIME,
        LX_TIMERFD_GETTIME => OCT_TIMERFD_GETTIME,
        LX_GETRANDOM => OCT_GETRANDOM,
        _ => return None,
    })
}

/// Map Linux open(2) flags onto the OCTAGRAM flag bits.
fn linux_open_flags(flags: u64) -> u64 {
    let mut result = flags & 0x3; // O_RDONLY / O_WRONLY / O_RDWR
    if flags & 0x40 != 0 {
        result |= 1 << 8; // O_CREAT
    }
    if flags & 0x200 != 0 {
        result |= 1 << 9; // O_TRUNC
    }
    if flags & 0x400 != 0 {
        result |= 1 << 10; // O_APPEND
    }
    if flags & 0x800 != 0 {
        result |= 1 << 11; // O_NONBLOCK
    }
    result
}

fn arch_prctl(code: u64, value: u64) -> Result<u64, i32> {
    match code {
        ARCH_SET_FS => {
            if value != 0 && !thread_pointer::is_valid_user_thread_pointer(value) {
                return Err(EPERM);
            }
            // Use the OCTAGRAM set-FS syscall: it updates both the MSR and the
            // saved per-process thread pointer, which the syscall wrapper uses
            // to restore FS on the way back to user mode.
            call(OCT_SET_THREAD_POINTER, value, 0, 0, 0, 0, 0)
        }
        ARCH_GET_FS => {
            paging::validate_user_range(value, 8, true).map_err(|_| EFAULT)?;
            unsafe { (value as *mut u64).write_unaligned(thread_pointer::read()) };
            Ok(0)
        }
        _ => Err(ENOSYS),
    }
}

fn now_seconds() -> i64 {
    crate::drivers::rtc::unix_seconds()
        .map(|seconds| seconds as i64)
        .unwrap_or(0)
}

fn stat_path(path_address: u64, stat_address: u64, follow: bool) -> Result<u64, i32> {
    let (mode, size) = crate::elf::linux_path_status(path_address, follow)?;
    write_stat(stat_address, mode, size)
}

fn stat_fd(fd: u64, stat_address: u64) -> Result<u64, i32> {
    let (mode, size) = crate::elf::linux_fd_status(fd)?;
    write_stat(stat_address, mode, size)
}

/// Translate Linux `ioctl`. Terminal ioctls are converted to the kernel
/// `struct termios` layout before touching user memory; everything else is
/// forwarded to the shared OCTAGRAM ioctl handler.
fn linux_ioctl(fd: u64, request: u64, arg: u64) -> Result<u64, i32> {
    let is_console = crate::elf::linux_fd_status(fd)
        .map(|(mode, _)| mode & 0o170000 == 0o020000)
        .unwrap_or(false);
    if is_console {
        match request {
            LX_TCGETS => {
                let bytes = crate::fs::linux_termios_bytes(&crate::fs::console_termios());
                paging::validate_user_range(arg, crate::fs::LINUX_TERMIOS_SIZE, true)
                    .map_err(|_| EFAULT)?;
                unsafe {
                    core::ptr::copy_nonoverlapping(bytes.as_ptr(), arg as *mut u8, bytes.len())
                };
                return Ok(0);
            }
            LX_TCSETS | LX_TCSETSW | LX_TCSETSF => {
                paging::validate_user_range(arg, crate::fs::LINUX_TERMIOS_SIZE, false)
                    .map_err(|_| EFAULT)?;
                let mut bytes = [0u8; crate::fs::LINUX_TERMIOS_SIZE];
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        arg as *const u8,
                        bytes.as_mut_ptr(),
                        bytes.len(),
                    )
                };
                crate::fs::set_console_termios(crate::fs::termios_from_linux_bytes(&bytes));
                if request == LX_TCSETSF {
                    crate::fs::discard_pending_input();
                }
                return Ok(0);
            }
            _ => {}
        }
    }
    call(OCT_IOCTL, fd, request, arg, 0, 0, 0)
}

fn write_stat(address: u64, mode: u32, size: u64) -> Result<u64, i32> {
    paging::validate_user_range(address, core::mem::size_of::<LinuxStat>(), true)
        .map_err(|_| EFAULT)?;
    let now = now_seconds();
    let stat = LinuxStat {
        dev: 1,
        ino: 1,
        nlink: 1,
        mode,
        uid: 0,
        gid: 0,
        pad0: 0,
        rdev: 0,
        size: size as i64,
        blksize: 4096,
        blocks: ((size + 511) / 512) as i64,
        atime: now,
        atime_nsec: 0,
        mtime: now,
        mtime_nsec: 0,
        ctime: now,
        ctime_nsec: 0,
        unused: [0; 3],
    };
    unsafe { (address as *mut LinuxStat).write_unaligned(stat) };
    Ok(0)
}

fn newfstatat(dirfd: u64, path_address: u64, stat_address: u64, flags: u64) -> Result<u64, i32> {
    paging::validate_user_range(path_address, 1, false).map_err(|_| EFAULT)?;
    let empty = unsafe { *(path_address as *const u8) == 0 };
    if empty && flags & AT_EMPTY_PATH != 0 {
        return stat_fd(dirfd, stat_address);
    }
    let (mode, size) =
        crate::elf::linux_at_path_status(dirfd, path_address, flags & AT_SYMLINK_NOFOLLOW == 0)?;
    write_stat(stat_address, mode, size)
}

fn statx(dirfd: u64, path_address: u64, flags: u64, statx_address: u64) -> Result<u64, i32> {
    paging::validate_user_range(path_address, 1, false).map_err(|_| EFAULT)?;
    let empty = unsafe { *(path_address as *const u8) == 0 };
    let (mode, size) = if empty && flags & AT_EMPTY_PATH != 0 {
        crate::elf::linux_fd_status(dirfd)?
    } else {
        crate::elf::linux_at_path_status(dirfd, path_address, flags & AT_SYMLINK_NOFOLLOW == 0)?
    };
    paging::validate_user_range(statx_address, core::mem::size_of::<LinuxStatx>(), true)
        .map_err(|_| EFAULT)?;
    let now = now_seconds();
    let timestamp = LinuxStatxTimestamp {
        seconds: now,
        nanoseconds: 0,
        reserved: 0,
    };
    let stat = LinuxStatx {
        mask: 0x7ff,
        blksize: 4096,
        attributes: 0,
        nlink: 1,
        uid: 0,
        gid: 0,
        mode: mode as u16,
        spare0: 0,
        ino: 1,
        size,
        blocks: (size + 511) / 512,
        attributes_mask: 0,
        atime: timestamp,
        btime: LinuxStatxTimestamp::default(),
        ctime: timestamp,
        mtime: timestamp,
        rdev_major: 0,
        rdev_minor: 0,
        dev_major: 1,
        dev_minor: 1,
        mnt_id: 1,
        dio_mem_align: 0,
        dio_offset_align: 0,
        spare: [0; 12],
    };
    unsafe { (statx_address as *mut LinuxStatx).write_unaligned(stat) };
    Ok(0)
}

fn faccessat(dirfd: u64, path_address: u64, mode: u64) -> Result<u64, i32> {
    if dirfd as u32 as i32 != AT_FDCWD as i32 {
        return Err(ENOSYS);
    }
    call(OCT_ACCESS, path_address, mode, 0, 0, 0, 0)
}

fn truncate(path_address: u64, length: u64) -> Result<u64, i32> {
    let fd = call(OCT_OPEN, path_address, 1, 0, 0, 0, 0)?;
    let result = call(OCT_FTRUNCATE, fd, length, 0, 0, 0, 0);
    let closed = call(OCT_CLOSE, fd, 0, 0, 0, 0, 0);
    result?;
    closed?;
    Ok(0)
}

fn write_rlimit(resource: u64, address: u64) -> Result<u64, i32> {
    paging::validate_user_range(address, core::mem::size_of::<LinuxRlimit>(), true)
        .map_err(|_| EFAULT)?;
    let limit = match resource {
        RLIMIT_NOFILE => LinuxRlimit {
            current: 16,
            maximum: 16,
        },
        RLIMIT_NPROC => LinuxRlimit {
            current: 16,
            maximum: 16,
        },
        RLIMIT_STACK => LinuxRlimit {
            current: 8 * 1024 * 1024,
            maximum: 8 * 1024 * 1024,
        },
        _ => LinuxRlimit {
            current: u64::MAX,
            maximum: u64::MAX,
        },
    };
    unsafe { (address as *mut LinuxRlimit).write_unaligned(limit) };
    Ok(0)
}

fn write_prlimit(_pid: u64, resource: u64, old_address: u64) -> Result<u64, i32> {
    if old_address == 0 {
        return Ok(0);
    }
    write_rlimit(resource, old_address)
}

fn sched_getaffinity(_pid: u64, size: u64, mask: u64) -> Result<u64, i32> {
    if mask == 0 || size == 0 {
        return Err(EINVAL);
    }
    let length = size.min(8) as usize;
    paging::validate_user_range(mask, length, true).map_err(|_| EFAULT)?;
    let mut bytes = [0u8; 8];
    bytes[0] = 1;
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), mask as *mut u8, length);
    }
    Ok(8)
}

fn sysinfo(address: u64) -> Result<u64, i32> {
    const SYSINFO_SIZE: usize = 112;
    paging::validate_user_range(address, SYSINFO_SIZE, true).map_err(|_| EFAULT)?;
    let mut buffer = [0u8; SYSINFO_SIZE];
    let uptime = (crate::arch::x86_64::monotonic_milliseconds() / 1000) as u64;
    buffer[0..8].copy_from_slice(&uptime.to_le_bytes());
    // totalram/freeram are not exposed by the frame allocator; zero is valid.
    buffer[104..108].copy_from_slice(&1u32.to_le_bytes()); // mem_unit
    unsafe {
        core::ptr::copy_nonoverlapping(buffer.as_ptr(), address as *mut u8, SYSINFO_SIZE);
    }
    Ok(0)
}

fn getcpu(cpu_address: u64, node_address: u64) -> Result<u64, i32> {
    for address in [cpu_address, node_address] {
        if address == 0 {
            continue;
        }
        paging::validate_user_range(address, 4, true).map_err(|_| EFAULT)?;
        unsafe { (address as *mut u32).write_unaligned(0) };
    }
    Ok(0)
}

fn linux_time(address: u64) -> Result<u64, i32> {
    let now = now_seconds();
    if address != 0 {
        paging::validate_user_range(address, 8, true).map_err(|_| EFAULT)?;
        unsafe { (address as *mut i64).write_unaligned(now) };
    }
    Ok(now as u64)
}

fn zero_fill(address: u64, length: usize) -> Result<u64, i32> {
    if address == 0 {
        return Ok(0);
    }
    paging::validate_user_range(address, length, true).map_err(|_| EFAULT)?;
    let zeros = [0u8; 144];
    let count = length.min(zeros.len());
    unsafe {
        core::ptr::copy_nonoverlapping(zeros.as_ptr(), address as *mut u8, count);
    }
    Ok(0)
}

fn ppoll_timeout(address: u64) -> Result<u64, i32> {
    if address == 0 {
        return Ok((-1i64) as u64);
    }
    paging::validate_user_range(address, core::mem::size_of::<UserTimespec>(), false)
        .map_err(|_| EFAULT)?;
    let timeout = unsafe { (address as *const UserTimespec).read_unaligned() };
    if timeout.seconds < 0 || timeout.nanoseconds < 0 {
        return Ok((-1i64) as u64);
    }
    Ok((timeout.seconds as u64) * 1000 + (timeout.nanoseconds as u64) / 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::{LinuxStat, LinuxStatx, linux_open_flags};

    #[test]
    fn linux_struct_layouts_match_x86_64() {
        assert_eq!(core::mem::size_of::<LinuxStat>(), 144);
        assert_eq!(core::mem::size_of::<LinuxStatx>(), 256);
        assert_eq!(core::mem::offset_of!(LinuxStat, mode), 24);
        assert_eq!(core::mem::offset_of!(LinuxStat, size), 48);
        assert_eq!(core::mem::offset_of!(LinuxStatx, size), 40);
    }

    #[test]
    fn open_flags_convert_to_octagram_bits() {
        // O_RDWR | O_CREAT | O_TRUNC | O_APPEND | O_NONBLOCK
        let converted = linux_open_flags(0x2 | 0x40 | 0x200 | 0x400 | 0x800);
        assert_eq!(converted & 0x3, 0x2);
        assert_eq!(converted & (1 << 8), 1 << 8);
        assert_eq!(converted & (1 << 9), 1 << 9);
        assert_eq!(converted & (1 << 10), 1 << 10);
        assert_eq!(converted & (1 << 11), 1 << 11);
    }
}

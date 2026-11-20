//! Loader for static ELF64 x86_64 programs and the minimal OCTAGRAM syscall ABI.

use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::arch::x86_64::{
    fpu::{self, State as FpuState},
    gdt,
    interrupt::{self, InterruptFrame, InterruptStackFrame},
    thread_pointer,
};
use crate::fs::{self, DirectoryEntry, FsError, NodeType};
use crate::memory::{
    allocate_frame, free_frame, paging,
    paging::{PAGE_SIZE, PagingError},
    physical_to_virtual,
};

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const MAX_PROGRAM_HEADERS: usize = 128;
const STACK_PAGES: u64 = 8;
/// Jumlah halaman entropi untuk menggeser puncak stack user per eksekusi.
const STACK_RANDOM_PAGES: u64 = 64;
const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_ENVIRONMENT_VARIABLES: usize = 64;
const MAX_ENVIRONMENT_BYTES: usize = 4096;
const STACK_TOP: u64 = paging::USER_ADDRESS_MAX;
const MMAP_START: u64 = 0x1_0000_0000;
const MMAP_END: u64 = STACK_TOP - (STACK_PAGES + STACK_RANDOM_PAGES + 1) * PAGE_SIZE;
const MAX_USER_PATH: usize = 256;
const MAX_SYSCALL_IO: usize = 4096;
const MAX_OPEN_FILES: usize = 16;
const MAX_SCHEDULED_PROCESSES: usize = 64;
const PIPE_CAPACITY: usize = 4096;
const WAIT_ANY_PID: u64 = u64::MAX;
const WAIT_NOHANG: u64 = 1;
const PROCESS_KERNEL_STACK_SIZE: usize = 64 * 1024;
const ELF_ET_EXEC: u16 = 2;
const ELF_ET_DYN: u16 = 3;
const PIE_LOAD_BIAS: u64 = 0x4000_0000;
const PIE_LOAD_BIAS_MAX: u64 = 0x7000_0000;
const PIE_LOAD_ALIGN: u64 = 2 * 1024 * 1024;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const SYSCALL_EXIT: u64 = 0;
const SYSCALL_WRITE: u64 = 1;
const SYSCALL_READ: u64 = 2;
const SYSCALL_OPEN: u64 = 3;
const SYSCALL_CLOSE: u64 = 4;
const SYSCALL_LSEEK: u64 = 5;
const SYSCALL_STAT: u64 = 6;
const SYSCALL_MKDIR: u64 = 7;
const SYSCALL_UNLINK: u64 = 8;
const SYSCALL_FTRUNCATE: u64 = 9;
const SYSCALL_GETPID: u64 = 11;
const SYSCALL_ISATTY: u64 = 12;
const SYSCALL_CHDIR: u64 = 13;
const SYSCALL_GETCWD: u64 = 14;
const SYSCALL_MMAP: u64 = 16;
const SYSCALL_MUNMAP: u64 = 17;
const SYSCALL_WAITPID: u64 = 20;
const SYSCALL_PIPE: u64 = 21;
const SYSCALL_DUP2: u64 = 22;
const SYSCALL_CHMOD: u64 = 25;
const SYSCALL_OPENDIR: u64 = 27;
const SYSCALL_READDIR: u64 = 28;
const SYSCALL_FSTAT: u64 = 29;
const SYSCALL_ACCESS: u64 = 30;
const SYSCALL_RENAME: u64 = 31;
const SYSCALL_UMASK: u64 = 32;
const SYSCALL_DUP: u64 = 33;
const SYSCALL_FCNTL: u64 = 34;
const SYSCALL_RMDIR: u64 = 35;
const SYSCALL_SYMLINK: u64 = 36;
const SYSCALL_READLINK: u64 = 37;
const SYSCALL_LSTAT: u64 = 38;
const SYSCALL_FCHMOD: u64 = 39;
const SYSCALL_OPENAT: u64 = 40;
const SYSCALL_FSTATAT: u64 = 41;
const SYSCALL_MKDIRAT: u64 = 42;
const SYSCALL_UNLINKAT: u64 = 43;
const SYSCALL_RENAMEAT: u64 = 44;
const SYSCALL_READLINKAT: u64 = 45;
const SYSCALL_SYMLINKAT: u64 = 46;
const SYSCALL_EXECVE: u64 = 47;
const SYSCALL_FORK: u64 = 48;
const SYSCALL_CLOCK_GETTIME: u64 = 50;
const SYSCALL_MSYNC: u64 = 51;
const SYSCALL_MPROTECT: u64 = 52;
const SYSCALL_MINCORE: u64 = 53;
const SYSCALL_MLOCK: u64 = 54;
const SYSCALL_MUNLOCK: u64 = 55;
const SYSCALL_MLOCKALL: u64 = 56;
const SYSCALL_MUNLOCKALL: u64 = 57;
const SYSCALL_POSIX_MADVISE: u64 = 58;
const SYSCALL_SET_THREAD_POINTER: u64 = 59;
const SYSCALL_GET_THREAD_POINTER: u64 = 60;
const SYSCALL_THREAD_CREATE: u64 = 61;
const SYSCALL_THREAD_EXIT: u64 = 62;
const SYSCALL_THREAD_JOIN: u64 = 63;
const SYSCALL_THREAD_SELF: u64 = 64;
const SYSCALL_KERNEL_YIELD: u64 = 65;
const SYSCALL_UDP_SOCKET: u64 = 66;
const SYSCALL_UDP_BIND: u64 = 67;
const SYSCALL_UDP_SENDTO: u64 = 68;
const SYSCALL_UDP_RECVFROM: u64 = 69;
const SYSCALL_NETWORK_DNS: u64 = 70;
const SYSCALL_SOCKET: u64 = 71;
const SYSCALL_BIND: u64 = 72;
const SYSCALL_LISTEN: u64 = 73;
const SYSCALL_ACCEPT: u64 = 74;
const SYSCALL_CONNECT: u64 = 75;
const SYSCALL_SEND: u64 = 76;
const SYSCALL_RECV: u64 = 77;
const SYSCALL_SHUTDOWN: u64 = 78;
const SYSCALL_GETSOCKNAME: u64 = 79;
const SYSCALL_GETPEERNAME: u64 = 80;
const SYSCALL_IOCTL: u64 = 81;
const SYSCALL_GETUID: u64 = 82;
const SYSCALL_GETGID: u64 = 83;
const SYSCALL_GETEUID: u64 = 84;
const SYSCALL_GETEGID: u64 = 85;
const SYSCALL_UNAME: u64 = 86;
const SYSCALL_GETPPID: u64 = 87;
const SYSCALL_BRK: u64 = 88;
const SYSCALL_GETTIMEOFDAY: u64 = 89;
const SYSCALL_NANOSLEEP: u64 = 90;
const SYSCALL_KILL: u64 = 91;
const SYSCALL_SIGACTION: u64 = 92;
const SYSCALL_SIGALTSTACK: u64 = 133;
const SYSCALL_SIGPROCMASK: u64 = 93;
const SYSCALL_FUTEX: u64 = 94;
const SYSCALL_SETSOCKOPT: u64 = 95;
const SYSCALL_GETSOCKOPT: u64 = 96;
const SYSCALL_POLL: u64 = 97;
const SYSCALL_PIPE2: u64 = 98;
const SYSCALL_DUP3: u64 = 99;
const SYSCALL_EVENTFD: u64 = 100;
const SYSCALL_TIMERFD_CREATE: u64 = 101;
const SYSCALL_PREAD: u64 = 102;
const SYSCALL_PWRITE: u64 = 103;
const SYSCALL_WRITEV: u64 = 104;
const SYSCALL_READV: u64 = 105;
const SYSCALL_SENDMSG: u64 = 106;
const SYSCALL_RECVMSG: u64 = 107;
const SYSCALL_SYSCONF: u64 = 108;
const SYSCALL_GETRLIMIT: u64 = 109;
const SYSCALL_SETRLIMIT: u64 = 110;
const SYSCALL_GETRUSAGE: u64 = 111;
const SYSCALL_TIMES: u64 = 112;
const SYSCALL_SIGRETURN: u64 = 113;
const SYSCALL_SIGPENDING: u64 = 114;
const SYSCALL_SIGSUSPEND: u64 = 115;
const SYSCALL_SIGNAL: u64 = 116;
const SYSCALL_SELECT: u64 = 117;
const SYSCALL_CLOCK_GETRES: u64 = 118;
const SYSCALL_FSYNC: u64 = 119;
const SYSCALL_FDATASYNC: u64 = 120;
const SYSCALL_GETRANDOM: u64 = 121;
const SYSCALL_EPOLL_CREATE1: u64 = 122;
const SYSCALL_EPOLL_CTL: u64 = 123;
const SYSCALL_EPOLL_WAIT: u64 = 124;
const SYSCALL_TIMERFD_SETTIME: u64 = 125;
const SYSCALL_TIMERFD_GETTIME: u64 = 126;
const SYSCALL_SOCKETPAIR: u64 = 127;
const SYSCALL_CLOCK_NANOSLEEP: u64 = 128;
const SYSCALL_GETPGRP: u64 = 129;
const SYSCALL_SETPGRP: u64 = 130;
const SYSCALL_GETSID: u64 = 131;
const SYSCALL_SETSID: u64 = 132;
const SYSCALL_SETPRIORITY: u64 = 134;
const SYSCALL_GETPRIORITY: u64 = 135;
const SYSCALL_SCHED_SETSCHEDULER: u64 = 136;
const SYSCALL_SCHED_GETSCHEDULER: u64 = 137;
const SYSCALL_SCHED_GETPARAM: u64 = 138;
const SYSCALL_SCHED_SETPARAM: u64 = 139;
const SYSCALL_SETUID: u64 = 140;
const SYSCALL_SETEUID: u64 = 141;
const SYSCALL_SETGID: u64 = 142;
const SYSCALL_SETEGID: u64 = 143;
const SYSCALL_CAPGET: u64 = 144;
const SYSCALL_CAPSET: u64 = 145;
const SYSCALL_SET_ROBUST_LIST: u64 = 146;
const SYSCALL_GET_ROBUST_LIST: u64 = 147;
const PRIO_PROCESS: u64 = 0;
const PRIO_PGRP: u64 = 1;
const PRIO_USER: u64 = 2;

/// Bit kemampuan proses. Saat ini hanya metadata: belum diperiksa di jalur akses.
pub(crate) const CAP_VFS_WRITE: u64 = 1 << 0;
pub(crate) const CAP_NETWORK: u64 = 1 << 1;
pub(crate) const CAP_SYS_ADMIN: u64 = 1 << 2;
pub(crate) const CAP_SCHED_NICE: u64 = 1 << 3;
pub(crate) const CAP_ALL: u64 = CAP_VFS_WRITE | CAP_NETWORK | CAP_SYS_ADMIN | CAP_SCHED_NICE;

/// Proses uid 0 mendapat seluruh kemampuan; selain itu tidak ada.
fn capabilities_for_uid(uid: u32) -> u64 {
    if uid == 0 { CAP_ALL } else { 0 }
}

/// Perubahan kredensial: root bebas, non-root hanya ke id nyata/efektifnya.
fn credential_change_allowed(caller_euid: u32, current: [u32; 2], target: u32) -> bool {
    caller_euid == 0 || target == current[0] || target == current[1]
}
/// Kebijakan penjadwalan yang didukung: `SCHED_OTHER`, `SCHED_FIFO`, dan
/// `SCHED_RR`.
const SCHED_OTHER: u64 = 0;
const SCHED_FIFO: u64 = 1;
const SCHED_RR: u64 = 2;
/// Rentang `sched_priority` untuk `SCHED_FIFO`/`SCHED_RR`.
const RT_PRIORITY_MIN: i32 = 1;
const RT_PRIORITY_MAX: i32 = 99;
/// Kuota `SCHED_RR` dalam tick timer sebelum task RR dirotasi.
const RT_RR_QUANTUM_TICKS: u32 = 4;
const AT_FDCWD: i64 = -100;
const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
const AT_REMOVEDIR: u64 = 0x200;

const PROT_READ: u64 = 1;
const PROT_WRITE: u64 = 2;
const PROT_EXEC: u64 = 4;
const MAP_SHARED: u64 = 1;
const MAP_PRIVATE: u64 = 2;
const MAP_FIXED: u64 = 0x10;
const MAP_ANONYMOUS: u64 = 0x20;
const MS_ASYNC: u64 = 1;
const MS_INVALIDATE: u64 = 2;
const MS_SYNC: u64 = 4;

const FBIOGET_VSCREENINFO: u64 = 0x4600;
const FBIOPUT_VSCREENINFO: u64 = 0x4601;
const FBIOGET_FSCREENINFO: u64 = 0x4602;
const FBIOBLANK: u64 = 0x4611;
const TIOCGWINSZ: u64 = 0x5413;
const TCGETS: u64 = 0x5401;
const TCSETS: u64 = 0x5402;
const TCSETSW: u64 = 0x5403;
const TCSETSF: u64 = 0x5404;


#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbBitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbVarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: FbBitfield,
    green: FbBitfield,
    blue: FbBitfield,
    transp: FbBitfield,
    nonstd: u32,
    activate: u32,
    height: u32,
    width: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FbFixScreeninfo {
    id: [u8; 16],
    smem_start: u64,
    smem_len: u32,
    fb_type: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    mmio_start: u64,
    mmio_len: u32,
    accel: u32,
    capabilities: u16,
    reserved: [u16; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[repr(C)]
struct Uname {
    sysname:  [u8; 65],
    nodename: [u8; 65],
    release:  [u8; 65],
    version:  [u8; 65],
    machine:  [u8; 65],
    domainname: [u8; 65],
}

impl Uname {
    fn new() -> Self {
        let mut u = Self { sysname: [0;65], nodename: [0;65], release: [0;65], version: [0;65], machine: [0;65], domainname: [0;65] };
        copy_cstr(&mut u.sysname, b"OCTAGRAM-OS");
        copy_cstr(&mut u.nodename, b"octagram");
        copy_cstr(&mut u.release, b"1.0.0");
        copy_cstr(&mut u.version, b"#1 POSIX");
        copy_cstr(&mut u.machine, b"x86_64");
        u
    }
}

fn copy_cstr(buf: &mut [u8], src: &[u8]) {
    let len = src.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&src[..len]);
}

#[repr(C)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

const RLIM_INFINITY: u64 = u64::MAX;

#[repr(C)]
struct Rlimit {
    rlim_cur: u64,
    rlim_max: u64,
}

#[repr(C)]
#[derive(Default)]
struct Rusage {
    ru_utime:  [i64; 2],
    ru_stime:  [i64; 2],
    ru_rest:   [i64; 14],
}

#[repr(C)]
#[derive(Default)]
struct Tms {
    tms_utime:  i64,
    tms_stime:  i64,
    tms_cutime: i64,
    tms_cstime: i64,
}

// IOVec for readv/writev
#[repr(C)]
struct IoVec {
    base: u64,
    len:  u64,
}

const O_RDONLY: u64 = 0;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_ACCESS_MASK: u64 = 3;
const O_CREAT: u64 = 1 << 8;
const O_TRUNC: u64 = 1 << 9;
const O_APPEND: u64 = 1 << 10;
const O_NONBLOCK: u64 = 1 << 11;
const O_CLOEXEC: u64 = 1 << 19;
const FD_CLOEXEC: u32 = 1;
const F_DUPFD: u64 = 0;
const F_GETFD: u64 = 1;
const F_SETFD: u64 = 2;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;

const EINTR: i32 = 4;
const EPERM: i32 = 1;
const E2BIG: i32 = 7;
const EIO: i32 = 5;
const ENOEXEC: i32 = 8;
const EBADF: i32 = 9;
const ECHILD: i32 = 10;
const EAGAIN: i32 = 11;
const ENOMEM: i32 = 12;
const EFAULT: i32 = 14;
const ENOENT: i32 = 2;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EISDIR: i32 = 21;
const EINVAL: i32 = 22;
const ESPIPE: i32 = 29;
const ERANGE: i32 = 34;
const EACCES: i32 = 13;
const EBUSY: i32 = 16;
const ENODEV: i32 = 19;
const EMFILE: i32 = 24;
const ENOTTY: i32 = 25;
const ENOSPC: i32 = 28;
const EROFS: i32 = 30;
const ENAMETOOLONG: i32 = 36;
const ENOSYS: i32 = 38;
const ENOTEMPTY: i32 = 39;
const ELOOP: i32 = 40;
const EILSEQ: i32 = 84;
const EPIPE: i32 = 32;
const EDEADLK: i32 = 35;
const ESRCH: i32 = 3;
const EOVERFLOW: i32 = 75;
const ENOTSOCK: i32 = 88;
const ENOTSUP: i32 = 95;
const EMSGSIZE: i32 = 90;
const EADDRINUSE: i32 = 98;
const ENETDOWN: i32 = 100;
const EHOSTUNREACH: i32 = 113;
const EAFNOSUPPORT: i32 = 97;
const EPROTONOSUPPORT: i32 = 93;
const EPROTOTYPE: i32 = 91;
const EADDRNOTAVAIL: i32 = 99;
const ECONNRESET: i32 = 104;
const ENOTCONN: i32 = 107;
const EISCONN: i32 = 106;
const ETIMEDOUT: i32 = 110;
const ECONNREFUSED: i32 = 111;
const ENOPROTOOPT: i32 = 92;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    InvalidHeader,
    UnsupportedFormat,
    DynamicLinkingUnsupported,
    OutOfBounds,
    InvalidSegment,
    WritableExecutableSegment,
    InvalidEntryPoint,
    AddressRange,
    OverlappingSegments,
    TooLarge,
    OutOfMemory,
    ProcessAlreadyRunning,
    InvalidArguments,
    ArgumentsTooLarge,
    SchedulerInvariant,
    PermissionDenied,
    FileSystem(FsError),
    Paging(PagingError),
}

fn syscall_fstat(fd: u64, stat_address: u64) -> Result<u64, i32> {
    paging::validate_user_range(stat_address, core::mem::size_of::<UserStat>(), true)
        .map_err(|_| EFAULT)?;
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let path = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        match process.descriptors.get(fd).and_then(Option::as_ref) {
            Some(UserDescriptor::File(file)) => file.lock().path.clone(),
            Some(UserDescriptor::Directory(directory)) => directory.lock().path.clone(),
            Some(
                UserDescriptor::Pipe(_)
                | UserDescriptor::SocketPair(_)
                | UserDescriptor::Epoll(_)
                | UserDescriptor::EventFd(_)
                | UserDescriptor::TimerFd(_)
                | UserDescriptor::UdpSocket(_)
                | UserDescriptor::TcpSocket(_),
            ) => {
                return Err(ENOTSUP);
            }
            Some(
                UserDescriptor::ConsoleInput
                | UserDescriptor::ConsoleOutput
                | UserDescriptor::ConsoleError,
            ) => {
                return Err(ENOTSUP);
            }
            None => return Err(EBADF),
        }
    };
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    let stat = UserStat {
        size: metadata.len as u64,
        mode: stat_mode(&path, metadata.node_type, metadata.permissions),
        modified_seconds: metadata
            .modified
            .and_then(|seconds| u32::try_from(seconds).ok())
            .unwrap_or(0),
    };
    unsafe {
        core::ptr::write_unaligned(stat_address as *mut UserStat, stat);
    }
    Ok(0)
}

fn syscall_access(path_address: u64, mode: u64) -> Result<u64, i32> {
    const ACCESS_MASK: u64 = 0b111;
    if mode & !ACCESS_MASK != 0 {
        return Err(EINVAL);
    }
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    let requested = (mode as u16) << 6;
    if metadata.permissions & requested != requested {
        return Err(EACCES);
    }
    Ok(0)
}

fn syscall_rename(source_address: u64, target_address: u64) -> Result<u64, i32> {
    let source = resolve_process_path(&copy_user_path(source_address)?)?;
    let target = resolve_process_path(&copy_user_path(target_address)?)?;
    syscall_rename_paths(source, target)
}

fn syscall_renameat(
    source_dirfd: u64,
    source_address: u64,
    target_dirfd: u64,
    target_address: u64,
) -> Result<u64, i32> {
    let source = resolve_at_path(source_dirfd, &copy_user_path(source_address)?)?;
    let target = resolve_at_path(target_dirfd, &copy_user_path(target_address)?)?;
    syscall_rename_paths(source, target)
}

fn syscall_rename_paths(source: String, target: String) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    fs::with_vfs(|vfs| vfs.rename(&source, &target))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    rewrite_shared_mapping_paths(&source, &target);

    let mut scheduler = USER_SCHEDULER.lock();
    let mut process = USER_PROCESS.lock();
    rewrite_descriptor_paths(&mut process.descriptors, &source, &target);
    rewrite_process_path(&mut process.cwd, &source, &target);
    for scheduled in &mut scheduler.processes {
        rewrite_descriptor_paths(&mut scheduled.state.descriptors, &source, &target);
        rewrite_process_path(&mut scheduled.state.cwd, &source, &target);
    }
    Ok(0)
}

fn rewrite_process_path(path: &mut String, source: &str, target: &str) {
    let source_prefix = alloc::format!("{source}/");
    if path == source {
        *path = String::from(target);
    } else if let Some(suffix) = path.strip_prefix(&source_prefix) {
        *path = alloc::format!("{target}/{suffix}");
    }
}

fn rewrite_descriptor_paths(
    descriptors: &mut [Option<UserDescriptor>],
    source: &str,
    target: &str,
) {
    let source_prefix = alloc::format!("{source}/");
    for descriptor in descriptors.iter_mut().filter_map(Option::as_mut) {
        match descriptor {
            UserDescriptor::File(file) => {
                let mut file = file.lock();
                if file.path == source {
                    file.path = String::from(target);
                } else if let Some(suffix) = file.path.strip_prefix(&source_prefix) {
                    file.path = alloc::format!("{target}/{suffix}");
                }
            }
            UserDescriptor::Directory(directory) => {
                let mut directory = directory.lock();
                if directory.path == source {
                    directory.path = String::from(target);
                } else if let Some(suffix) = directory.path.strip_prefix(&source_prefix) {
                    directory.path = alloc::format!("{target}/{suffix}");
                }
            }
            _ => {}
        }
    }
}

fn syscall_getpgrp() -> Result<u64, i32> {
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    Ok(process.process_group)
}

fn syscall_setpgrp(pid: u64, process_group: u64) -> Result<u64, i32> {
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let target_pid = if pid == 0 { process.pid } else { pid };
    let target_group = if process_group == 0 {
        target_pid
    } else {
        process_group
    };
    if target_pid != process.pid {
        return Err(EPERM);
    }
    if process.session != target_group && target_group != process.pid {
        return Err(EPERM);
    }
    process.process_group = target_group;
    Ok(0)
}

fn syscall_getsid(pid: u64) -> Result<u64, i32> {
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let target_pid = if pid == 0 { process.pid } else { pid };
    let scheduler = USER_SCHEDULER.lock();
    match scheduler
        .processes
        .iter()
        .find(|process| process.pid == target_pid)
    {
        Some(process) => Ok(process.state.session),
        None => Err(ESRCH),
    }
}

fn syscall_setsid() -> Result<u64, i32> {
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    if process.process_group == process.pid {
        return Err(EPERM);
    }
    process.process_group = process.pid;
    process.session = process.pid;
    Ok(process.pid)
}

/// Nilai `who` efektif untuk `setpriority`/`getpriority`; `None` berarti
/// `which` tidak dikenal. `who == 0` berarti pemanggil sendiri.
fn effective_priority_target(
    which: u64,
    who: u64,
    caller_pid: u64,
    caller_group: u64,
    caller_uid: u32,
) -> Option<u64> {
    match which {
        PRIO_PROCESS => Some(if who == 0 { caller_pid } else { who }),
        PRIO_PGRP => Some(if who == 0 { caller_group } else { who }),
        PRIO_USER => Some(if who == 0 { caller_uid as u64 } else { who }),
        _ => None,
    }
}

/// Apakah satu task cocok dengan target `which`/`who` yang sudah efektif.
fn priority_target_matches(
    which: u64,
    target: u64,
    pid: u64,
    process_group: u64,
    euid: u32,
) -> bool {
    match which {
        PRIO_PROCESS => pid == target,
        PRIO_PGRP => process_group == target,
        PRIO_USER => euid as u64 == target,
        _ => false,
    }
}

/// Izin mengubah prioritas task lain: butuh `CAP_SCHED_NICE`, atau euid sama
/// dan hanya menurunkan prioritas (nice membesar).
fn priority_change_allowed(
    caller_euid: u32,
    caller_capabilities: u64,
    target_euid: u32,
    old_priority: i32,
    new_priority: i32,
) -> bool {
    if caller_capabilities & CAP_SCHED_NICE != 0 {
        return true;
    }
    caller_euid == target_euid && new_priority >= old_priority
}

fn syscall_setpriority(which: u64, who: u64, priority: i64) -> Result<u64, i32> {
    let mut scheduler = USER_SCHEDULER.lock();
    if !scheduler.active || scheduler.processes.is_empty() {
        return Err(EINTR);
    }
    let current = scheduler.current;
    let caller = scheduler.processes.get(current).ok_or(EINTR)?;
    let (caller_pid, caller_uid, caller_capabilities) =
        (caller.pid, caller.euid, caller.capabilities);
    // Grup proses pemanggil ada di state aktif, bukan salinan scheduler.
    let caller_group = {
        let active = USER_PROCESS.lock();
        if active.active {
            active.process_group
        } else {
            caller.state.process_group
        }
    };
    let target = effective_priority_target(which, who, caller_pid, caller_group, caller_uid)
        .ok_or(EINVAL)?;
    let priority = clamp_priority(priority.clamp(PRIORITY_MIN as i64, PRIORITY_MAX as i64) as i32);
    let mut found = false;
    for process in &mut scheduler.processes {
        if process.exit_status.is_some()
            || !priority_target_matches(
                which,
                target,
                process.pid,
                if process.pid == caller_pid {
                    caller_group
                } else {
                    process.state.process_group
                },
                process.euid,
            )
        {
            continue;
        }
        if !priority_change_allowed(
            caller_uid,
            caller_capabilities,
            process.euid,
            process.priority,
            priority,
        ) {
            return Err(EPERM);
        }
        process.priority = priority;
        found = true;
    }
    if found { Ok(0) } else { Err(ESRCH) }
}

fn syscall_getpriority(which: u64, who: u64) -> Result<u64, i32> {
    let scheduler = USER_SCHEDULER.lock();
    if !scheduler.active || scheduler.processes.is_empty() {
        return Err(EINTR);
    }
    let current = scheduler.current;
    let caller = scheduler.processes.get(current).ok_or(EINTR)?;
    let caller_group = {
        let active = USER_PROCESS.lock();
        if active.active {
            active.process_group
        } else {
            caller.state.process_group
        }
    };
    let target = effective_priority_target(which, who, caller.pid, caller_group, caller.euid)
        .ok_or(EINVAL)?;
    let mut best: Option<i32> = None;
    for process in &scheduler.processes {
        if process.exit_status.is_none()
            && priority_target_matches(
                which,
                target,
                process.pid,
                if process.pid == caller.pid {
                    caller_group
                } else {
                    process.state.process_group
                },
                process.euid,
            )
        {
            best = Some(best.map_or(process.priority, |priority| priority.min(process.priority)));
        }
    }
    // Nilai nice dibias +20 agar hasil syscall tidak terlihat seperti error.
    best.map(|priority| (priority - PRIORITY_MIN) as u64)
        .ok_or(ESRCH)
}

/// Apakah bit kemampuan yang diminta ada di bitmask proses.
fn capability_present(capabilities: u64, capability: u64) -> Result<(), i32> {
    if capabilities & capability != 0 {
        Ok(())
    } else {
        Err(EPERM)
    }
}

/// Wajibkan kemampuan pada proses pemanggil syscall.
fn require_capability(capability: u64) -> Result<(), i32> {
    capability_present(current_capabilities()?, capability)
}

/// Bitmask kemampuan proses yang sedang berjalan.
fn current_capabilities() -> Result<u64, i32> {
    let scheduler = USER_SCHEDULER.lock();
    scheduler
        .processes
        .get(scheduler.current)
        .map(|process| process.capabilities)
        .ok_or(EINTR)
}

/// User ID efektif proses yang sedang berjalan.
fn current_euid() -> Result<u32, i32> {
    let scheduler = USER_SCHEDULER.lock();
    scheduler
        .processes
        .get(scheduler.current)
        .map(|process| process.euid)
        .ok_or(EINTR)
}

/// Pemeriksaan izin akses perangkat (/dev/fb0) dan console (/dev/ttyX) berdasarkan kredensial.
/// Root dan proses dengan `CAP_SYS_ADMIN` diizinkan penuh; non-root dibatasi hanya pada
/// terminal miliknya sendiri dan dilarang mengakses framebuffer mentah atau konsol terminal lain.
pub(crate) fn device_access_allowed(
    path: &str,
    current_terminal: usize,
    euid: u32,
    capabilities: u64,
) -> bool {
    if euid == 0 || capabilities & CAP_SYS_ADMIN != 0 {
        return true;
    }
    if fs::is_fb_path(path) {
        return false;
    }
    if let Some(target_term) = fs::tty_path_terminal(path) {
        if target_term != current_terminal {
            return false;
        }
    }
    true
}

fn read_user_u64(address: u64) -> Result<u64, i32> {
    paging::validate_user_range(address, core::mem::size_of::<u64>(), false)
        .map_err(|_| EFAULT)?;
    Ok(unsafe { (address as *const u64).read_unaligned() })
}

fn current_credential(select: impl Fn(&ScheduledProcess) -> u32) -> Result<u64, i32> {
    let scheduler = USER_SCHEDULER.lock();
    let process = scheduler.processes.get(scheduler.current).ok_or(EINTR)?;
    Ok(select(process) as u64)
}

#[derive(Clone, Copy)]
enum CredentialField {
    Uid,
    Euid,
    Gid,
    Egid,
}

/// Ubah kredensial seluruh thread dengan pid yang sama.
fn set_credential(field: CredentialField, value: u64) -> Result<u64, i32> {
    let value = u32::try_from(value).map_err(|_| EINVAL)?;
    let mut scheduler = USER_SCHEDULER.lock();
    if !scheduler.active {
        return Err(EINTR);
    }
    let current = scheduler.current;
    let (pid, caller_euid, current_ids) = {
        let process = scheduler.processes.get(current).ok_or(EINTR)?;
        let ids = match field {
            CredentialField::Uid | CredentialField::Euid => [process.uid, process.euid],
            CredentialField::Gid | CredentialField::Egid => [process.gid, process.egid],
        };
        (process.pid, process.euid, ids)
    };
    if !credential_change_allowed(caller_euid, current_ids, value) {
        return Err(EPERM);
    }
    for process in &mut scheduler.processes {
        if process.pid != pid {
            continue;
        }
        match field {
            CredentialField::Uid => {
                process.uid = value;
                process.euid = value;
                process.capabilities = capabilities_for_uid(value);
            }
            CredentialField::Euid => {
                process.euid = value;
                process.capabilities = capabilities_for_uid(value);
            }
            CredentialField::Gid => {
                process.gid = value;
                process.egid = value;
            }
            CredentialField::Egid => process.egid = value,
        }
    }
    Ok(0)
}

fn syscall_capget(pid: u64, mask_address: u64) -> Result<u64, i32> {
    let scheduler = USER_SCHEDULER.lock();
    if !scheduler.active {
        return Err(EINTR);
    }
    let target = sched_target_pid(pid, &scheduler)?;
    let capabilities = scheduler
        .processes
        .iter()
        .find(|process| process.pid == target && process.exit_status.is_none())
        .map(|process| process.capabilities)
        .ok_or(ESRCH)?;
    drop(scheduler);
    write_user_u64(mask_address, capabilities)?;
    Ok(0)
}

fn syscall_capset(pid: u64, mask_address: u64) -> Result<u64, i32> {
    let mask = read_user_u64(mask_address)?;
    if mask & !CAP_ALL != 0 {
        return Err(EINVAL);
    }
    let mut scheduler = USER_SCHEDULER.lock();
    if !scheduler.active {
        return Err(EINTR);
    }
    let caller_capabilities = scheduler
        .processes
        .get(scheduler.current)
        .map(|process| process.capabilities)
        .ok_or(EINTR)?;
    if caller_capabilities & CAP_SYS_ADMIN == 0 {
        return Err(EPERM);
    }
    let target = sched_target_pid(pid, &scheduler)?;
    let mut found = false;
    for process in &mut scheduler.processes {
        if process.pid == target && process.exit_status.is_none() {
            process.capabilities = mask;
            found = true;
        }
    }
    if found { Ok(0) } else { Err(ESRCH) }
}

fn read_user_i32(address: u64) -> Result<i32, i32> {
    paging::validate_user_range(address, core::mem::size_of::<i32>(), false)
        .map_err(|_| EFAULT)?;
    Ok(unsafe { (address as *const i32).read_unaligned() })
}

fn write_user_i32(address: u64, value: i32) -> Result<(), i32> {
    paging::validate_user_range(address, core::mem::size_of::<i32>(), true)
        .map_err(|_| EFAULT)?;
    unsafe { (address as *mut i32).write_unaligned(value) };
    Ok(())
}

fn scheduler_policy_supported(policy: u64) -> bool {
    policy == SCHED_OTHER || policy == SCHED_FIFO || policy == SCHED_RR
}

/// `SCHED_OTHER` hanya menerima `sched_priority` 0 (prioritas diatur nice),
/// sedangkan `SCHED_FIFO`/`SCHED_RR` menerima 1..99.
fn sched_priority_for(policy: u64, priority: i32) -> Result<i32, i32> {
    match policy {
        SCHED_OTHER => {
            if priority != 0 {
                return Err(EINVAL);
            }
            Ok(0)
        }
        SCHED_FIFO | SCHED_RR => {
            if !(RT_PRIORITY_MIN..=RT_PRIORITY_MAX).contains(&priority) {
                return Err(EINVAL);
            }
            Ok(priority)
        }
        _ => Err(EINVAL),
    }
}

/// Kebijakan pengganti task lain, atau kebijakan RT untuk diri sendiri, butuh
/// `CAP_SCHED_NICE`; non-root hanya boleh menurunkan dirinya ke `SCHED_OTHER`.
fn sched_policy_change_allowed(
    policy: u64,
    caller_euid: u32,
    caller_capabilities: u64,
    target_euid: u32,
) -> bool {
    if caller_capabilities & CAP_SCHED_NICE != 0 {
        return true;
    }
    caller_euid == target_euid && policy == SCHED_OTHER
}

fn sched_target_pid(pid: u64, scheduler: &UserScheduler) -> Result<u64, i32> {
    let caller = scheduler
        .processes
        .get(scheduler.current)
        .ok_or(EINTR)?;
    Ok(if pid == 0 { caller.pid } else { pid })
}

/// Kredensial pemanggil dan euid target untuk pemeriksaan izin `sched_*`.
fn sched_caller_and_target_euid(
    scheduler: &UserScheduler,
    target: u64,
) -> Result<(u32, u64, u32), i32> {
    let caller = scheduler
        .processes
        .get(scheduler.current)
        .ok_or(EINTR)?;
    let (caller_euid, caller_capabilities) = (caller.euid, caller.capabilities);
    let target_euid = scheduler
        .processes
        .iter()
        .find(|process| process.pid == target && process.exit_status.is_none())
        .map(|process| process.euid)
        .ok_or(ESRCH)?;
    Ok((caller_euid, caller_capabilities, target_euid))
}

fn syscall_sched_setscheduler(pid: u64, policy: u64, param_address: u64) -> Result<u64, i32> {
    let priority = read_user_i32(param_address)?;
    let rt_priority = sched_priority_for(policy, priority)?;
    let mut scheduler = USER_SCHEDULER.lock();
    if !scheduler.active {
        return Err(EINTR);
    }
    let target = sched_target_pid(pid, &scheduler)?;
    let (caller_euid, caller_capabilities, target_euid) =
        sched_caller_and_target_euid(&scheduler, target)?;
    if !sched_policy_change_allowed(policy, caller_euid, caller_capabilities, target_euid) {
        return Err(EPERM);
    }
    let mut found = false;
    for process in &mut scheduler.processes {
        if process.pid == target && process.exit_status.is_none() {
            process.policy = policy as u32;
            process.rt_priority = rt_priority;
            process.rt_slice = RT_RR_QUANTUM_TICKS;
            found = true;
        }
    }
    if found { Ok(0) } else { Err(ESRCH) }
}

fn syscall_sched_getscheduler(pid: u64) -> Result<u64, i32> {
    let scheduler = USER_SCHEDULER.lock();
    if !scheduler.active {
        return Err(EINTR);
    }
    let target = sched_target_pid(pid, &scheduler)?;
    scheduler
        .processes
        .iter()
        .find(|process| process.pid == target && process.exit_status.is_none())
        .map(|process| process.policy as u64)
        .ok_or(ESRCH)
}

fn syscall_sched_setparam(pid: u64, param_address: u64) -> Result<u64, i32> {
    let priority = read_user_i32(param_address)?;
    let mut scheduler = USER_SCHEDULER.lock();
    if !scheduler.active {
        return Err(EINTR);
    }
    let target = sched_target_pid(pid, &scheduler)?;
    let policy = scheduler
        .processes
        .iter()
        .find(|process| process.pid == target && process.exit_status.is_none())
        .map(|process| process.policy)
        .ok_or(ESRCH)?;
    let rt_priority = sched_priority_for(policy as u64, priority)?;
    let (caller_euid, caller_capabilities, target_euid) =
        sched_caller_and_target_euid(&scheduler, target)?;
    if !sched_policy_change_allowed(
        policy as u64,
        caller_euid,
        caller_capabilities,
        target_euid,
    ) {
        return Err(EPERM);
    }
    for process in &mut scheduler.processes {
        if process.pid == target && process.exit_status.is_none() {
            process.rt_priority = rt_priority;
            process.rt_slice = RT_RR_QUANTUM_TICKS;
        }
    }
    Ok(0)
}

fn syscall_sched_getparam(pid: u64, param_address: u64) -> Result<u64, i32> {
    let scheduler = USER_SCHEDULER.lock();
    if !scheduler.active {
        return Err(EINTR);
    }
    let target = sched_target_pid(pid, &scheduler)?;
    let rt_priority = scheduler
        .processes
        .iter()
        .find(|process| process.pid == target && process.exit_status.is_none())
        .map(|process| process.rt_priority)
        .ok_or(ESRCH)?;
    drop(scheduler);
    write_user_i32(param_address, rt_priority)?;
    Ok(0)
}

fn syscall_umask(mask: u64) -> Result<u64, i32> {
    let mask = (mask & 0o777) as u16;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let previous = process.file_creation_mask;
    process.file_creation_mask = mask;
    Ok(previous as u64)
}

impl From<FsError> for ElfError {
    fn from(error: FsError) -> Self {
        Self::FileSystem(error)
    }
}

impl From<PagingError> for ElfError {
    fn from(error: PagingError) -> Self {
        Self::Paging(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoadSegment {
    file_offset: usize,
    virtual_address: u64,
    file_size: usize,
    memory_size: u64,
    flags: u32,
}

struct ParsedElf {
    elf_type: u16,
    entry: u64,
    program_headers: u64,
    program_header_size: u64,
    program_header_count: u64,
    segments: Vec<LoadSegment>,
    interpreter: Option<String>,
}

pub struct LoadedElf {
    entry: u64,
    user_stack: u64,
    environment: Vec<String>,
    memory: Arc<UserMemory>,
}

struct UserMemory {
    mmap_cursor: Mutex<u64>,
    mapped_pages: Mutex<Vec<u64>>,
    mmap_pages: Mutex<Vec<MappingPage>>,
    lock_future_mappings: AtomicBool,
    address_space: paging::AddressSpace,
}

struct OpenFile {
    path: String,
    offset: usize,
    readable: bool,
    writable: bool,
    append: bool,
}

#[derive(Clone)]
struct MappingPage {
    address: u64,
    shared_backing: Option<Arc<SharedMmapPage>>,
    locked: bool,
    can_write: bool,
    anonymous: bool,
    is_device: bool,
}

struct SharedMmapPage {
    physical: u64,
    path: Mutex<Option<String>>,
    file_offset: u64,
    file_length: AtomicU64,
}

impl Drop for SharedMmapPage {
    fn drop(&mut self) {
        free_frame(self.physical as usize);
    }
}

struct DirectoryStream {
    path: String,
    entries: Vec<DirectoryEntry>,
    next_entry: usize,
}

#[repr(C)]
struct UserDirent {
    inode: u64,
    offset: i64,
    record_length: u16,
    file_type: u8,
    name: [u8; 256],
}

struct Pipe {
    id: u64,
    buffer: Vec<u8>,
    head: usize,
    length: usize,
    readers: usize,
    writers: usize,
}

impl Pipe {
    fn write(&mut self, input: &[u8]) -> Result<usize, i32> {
        if self.readers == 0 {
            return Err(EPIPE);
        }
        let available = PIPE_CAPACITY - self.length;
        if available == 0 {
            return Err(EAGAIN);
        }
        let count = input.len().min(available);
        for byte in &input[..count] {
            let tail = (self.head + self.length) % PIPE_CAPACITY;
            self.buffer[tail] = *byte;
            self.length += 1;
        }
        Ok(count)
    }

    fn read(&mut self, output: &mut [u8]) -> Result<usize, i32> {
        if self.length == 0 {
            return if self.writers == 0 {
                Ok(0)
            } else {
                Err(EAGAIN)
            };
        }
        let count = output.len().min(self.length);
        for byte in &mut output[..count] {
            *byte = self.buffer[self.head];
            self.head = (self.head + 1) % PIPE_CAPACITY;
            self.length -= 1;
        }
        Ok(count)
    }
}

struct PipeEnd {
    pipe: Arc<Mutex<Pipe>>,
    readable: bool,
    writable: bool,
}

/// Dua ujung pipe yang digabung menjadi satu endpoint dua arah (socketpair).
#[derive(Clone)]
struct DuplexEnd {
    read_end: PipeEnd,
    write_end: PipeEnd,
}

#[derive(Clone, Copy)]
struct EpollEntry {
    fd: usize,
    events: u32,
    data: u64,
    last_ready: u32,
}

struct EpollInstance {
    entries: Vec<EpollEntry>,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct UserEpollEvent {
    events: u32,
    data: u64,
}

struct EventFdState {
    counter: u64,
    semaphore: bool,
}

struct TimerFdState {
    clock: u64,
    interval_ms: u64,
    next_expiry_ms: Option<u64>,
}

#[repr(C)]
struct UserItimerspec {
    interval: UserTimespec,
    value: UserTimespec,
}

impl Clone for PipeEnd {
    fn clone(&self) -> Self {
        let mut pipe = self.pipe.lock();
        if self.readable {
            pipe.readers += 1;
        }
        if self.writable {
            pipe.writers += 1;
        }
        drop(pipe);
        Self {
            pipe: Arc::clone(&self.pipe),
            readable: self.readable,
            writable: self.writable,
        }
    }
}

impl Drop for PipeEnd {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        if self.readable {
            pipe.readers -= 1;
        }
        if self.writable {
            pipe.writers -= 1;
        }
    }
}

#[derive(Clone)]
enum UserDescriptor {
    ConsoleInput,
    ConsoleOutput,
    ConsoleError,
    File(Arc<Mutex<OpenFile>>),
    Directory(Arc<Mutex<DirectoryStream>>),
    Pipe(PipeEnd),
    SocketPair(DuplexEnd),
    Epoll(Arc<Mutex<EpollInstance>>),
    EventFd(Arc<Mutex<EventFdState>>),
    TimerFd(Arc<Mutex<TimerFdState>>),
    UdpSocket(Arc<Mutex<crate::network::UdpSocket>>),
    TcpSocket(Arc<Mutex<crate::network::TcpSocket>>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProcessWait {
    Child(u64),
    Thread(u64),
    PipeReadable(u64),
    PipeWritable(u64),
    Futex { address: u64, bitset: u32 },
    PollWait,
    Sleep,
    /// Menunggu input console pada terminal tertentu.
    ConsoleInput(usize),
}

struct UserProcess {
    active: bool,
    pid: u64,
    process_group: u64,
    session: u64,
    terminal: usize,
    descriptors: Vec<Option<UserDescriptor>>,
    descriptor_flags: Vec<u32>,
    cwd: String,
    environment: Vec<String>,
    file_creation_mask: u16,
    fpu_state: FpuState,
    thread_pointer: u64,
    /// Hasil FUTEX_WAIT yang disimpan saat proses dibangunkan.
    wait_result: Option<i32>,
    pending_signals: u64,
    blocked_signals: u64,
    signal_handlers: Vec<SignalAction>,
    interrupted: bool,
    interrupt_signal: u64,
    restart_number: Option<u64>,
    poll_deadline: Option<u64>,
    /// Stack alternatif untuk handler ber-`SA_ONSTACK` (0 = tidak ada).
    alternate_stack: u64,
    alternate_stack_size: u64,
    alternate_stack_active: bool,
}

impl UserProcess {
    const fn new() -> Self {
        Self {
            active: false,
            pid: 0,
            process_group: 0,
            session: 0,
            terminal: 0,
            descriptors: Vec::new(),
            descriptor_flags: Vec::new(),
            cwd: String::new(),
            environment: Vec::new(),
            file_creation_mask: 0o022,
            fpu_state: FpuState::new(),
            thread_pointer: 0,
            wait_result: None,
            pending_signals: 0,
            blocked_signals: 0,
            signal_handlers: Vec::new(),
            interrupted: false,
            interrupt_signal: 0,
            restart_number: None,
            poll_deadline: None,
            alternate_stack: 0,
            alternate_stack_size: 0,
            alternate_stack_active: false,
        }
    }
}

struct ProcessKernelStack(Vec<u8>);

impl ProcessKernelStack {
    fn new() -> Result<Self, ElfError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(PROCESS_KERNEL_STACK_SIZE)
            .map_err(|_| ElfError::OutOfMemory)?;
        bytes.resize(PROCESS_KERNEL_STACK_SIZE, 0);
        Ok(Self(bytes))
    }

    fn top(&self) -> u64 {
        ((self.0.as_ptr() as usize + self.0.len()) & !0xF) as u64
    }
}

struct ScheduledProcess {
    image: LoadedElf,
    state: UserProcess,
    pid: u64,
    _kernel_stack: Option<ProcessKernelStack>,
    kernel_stack_top: u64,
    frame_pointer: usize,
    exit_status: Option<u64>,
    parent_pid: Option<u64>,
    is_thread: bool,
    tid: u64,
    thread_mapping: Option<(u64, u64)>,
    waiting_for: Option<ProcessWait>,
    wait_deadline: Option<u64>,
    resume_context: Option<UserResumeContext>,
    wait_status_pointer: u64,
    /// Prioritas nice POSIX (-20..19); makin kecil, makin sering dijadwalkan.
    priority: i32,
    /// Kebijakan penjadwalan: `SCHED_OTHER`, `SCHED_FIFO`, atau `SCHED_RR`.
    policy: u32,
    /// `sched_priority` untuk kebijakan RT (0 untuk `SCHED_OTHER`).
    rt_priority: i32,
    /// Sisa kuota `SCHED_RR` dalam tick timer sebelum rotasi.
    rt_slice: u32,
    /// `set_robust_list`: alamat `robust_list_head` milik task ini (0 = tidak ada).
    robust_list: u64,
    robust_list_length: u64,
    /// Kredensial proses (diwarisi anak/thread, dibagi semua thread satu pid).
    uid: u32,
    euid: u32,
    gid: u32,
    egid: u32,
    capabilities: u64,
    /// Berapa kali proses runnable ini dilewati scheduler (aging anti-starvasi).
    age: u32,
    /// Akuntansi waktu CPU dan waktu tunggu task ini.
    runtime: TaskRuntime,
    reaped: bool,
}

#[derive(Clone, Copy)]
struct UserResumeContext {
    accumulator: u64,
    stack_frame: InterruptStackFrame,
}

struct UserScheduler {
    processes: Vec<ScheduledProcess>,
    retired_kernel_stacks: Vec<ProcessKernelStack>,
    current: usize,
    active: bool,
    next_pid: u64,
    next_tid: u64,
    /// Waktu nyata tick timer terakhir; 0 berarti belum pernah ada tick.
    last_tick_ms: u64,
    /// Waktu nyata saat akuntansi mulai, untuk menghitung persentase CPU.
    runtime_started_ms: u64,
}

impl UserScheduler {
    const fn new() -> Self {
        Self {
            processes: Vec::new(),
            retired_kernel_stacks: Vec::new(),
            current: 0,
            active: false,
            next_pid: 1,
            next_tid: 1,
            last_tick_ms: 0,
            runtime_started_ms: 0,
        }
    }
}

struct RestoreUserAddressSpace(u64);

impl Drop for RestoreUserAddressSpace {
    fn drop(&mut self) {
        let scheduler = USER_SCHEDULER.lock();
        if let Some(process) = scheduler
            .processes
            .iter()
            .find(|process| process.pid == self.0)
        {
            unsafe { process.image.memory.address_space.activate() };
        } else {
            panic!("process disappeared before syscall address-space restore");
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserStat {
    size: u64,
    mode: u32,
    modified_seconds: u32,
}

#[repr(C)]
struct UserTimespec {
    seconds: i64,
    nanoseconds: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserSockAddrIn {
    family: u16,
    port: u16,
    address: u32,
    zero: [u8; 8],
}

static USER_PROCESS: Mutex<UserProcess> = Mutex::new(UserProcess::new());
static USER_SCHEDULER: Mutex<UserScheduler> = Mutex::new(UserScheduler::new());
static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);
static SHARED_MMAP_PAGES: Mutex<Vec<Weak<SharedMmapPage>>> = Mutex::new(Vec::new());

impl LoadedElf {
    /// Run one trusted program until it exits or faults, then release its pages.
    pub fn run(self) -> Result<u64, ElfError> {
        self.run_with_cwd("/")
    }

    /// Run one trusted program with a process-local current working directory.
    pub fn run_with_cwd(self, cwd: &str) -> Result<u64, ElfError> {
        self.run_with_cwd_on_terminal(cwd, crate::terminal::active_terminal())
    }

    pub fn run_with_cwd_on_terminal(self, cwd: &str, terminal: usize) -> Result<u64, ElfError> {
        let mut images = Vec::new();
        if images.try_reserve_exact(1).is_err() {
            self.unload()?;
            return Err(ElfError::OutOfMemory);
        }
        images.push(self);
        let statuses = run_loaded_processes(images, cwd, terminal)?;
        statuses
            .first()
            .copied()
            .ok_or(ElfError::SchedulerInvariant)
    }

    /// Release the mappings without running the program.
    pub fn unload(self) -> Result<(), ElfError> {
        let Ok(memory) = Arc::try_unwrap(self.memory) else {
            return Ok(());
        };
        unsafe { memory.address_space.activate() };
        let mut mapped_pages = memory.mapped_pages.into_inner();
        let mut mmap_pages = memory.mmap_pages.into_inner();
        let page_cleanup = release_pages(&mut mapped_pages);
        let mapping_flush = flush_shared_mapping_pages(&mmap_pages);
        let mmap_cleanup = release_mmap_pages(&mut mmap_pages);
        let table_cleanup = memory.address_space.destroy();
        page_cleanup?;
        mapping_flush.map_err(ElfError::FileSystem)?;
        mmap_cleanup?;
        table_cleanup?;
        Ok(())
    }

    fn fork_clone(&self, user_pages: &[u64], shared_pages: &[u64]) -> Result<Self, ElfError> {
        let mapped_pages = try_clone_pages(&self.memory.mapped_pages.lock())?;
        let mmap_pages = try_clone_mapping_pages(&self.memory.mmap_pages.lock())?;
        let mmap_cursor = *self.memory.mmap_cursor.lock();
        // `mlockall` tidak diwariskan ke anak `fork`.
        Ok(Self {
            entry: self.entry,
            user_stack: self.user_stack,
            environment: try_clone_strings(&self.environment)?,
            memory: Arc::new(UserMemory {
                mmap_cursor: Mutex::new(mmap_cursor),
                mapped_pages: Mutex::new(mapped_pages),
                mmap_pages: Mutex::new(mmap_pages),
                lock_future_mappings: AtomicBool::new(false),
                address_space: self
                    .memory
                    .address_space
                    .fork_clone(user_pages, shared_pages)?,
            }),
        })
    }

    fn share_for_thread(&self) -> Result<Self, ElfError> {
        Ok(Self {
            entry: self.entry,
            user_stack: self.user_stack,
            environment: try_clone_strings(&self.environment)?,
            memory: Arc::clone(&self.memory),
        })
    }
}

pub fn load(bytes: &[u8]) -> Result<LoadedElf, ElfError> {
    load_with_args(bytes, &[])
}

pub fn load_with_args(bytes: &[u8], arguments: &[String]) -> Result<LoadedElf, ElfError> {
    load_with_args_and_environment(bytes, arguments, &[])
}

pub fn load_with_args_and_environment(
    bytes: &[u8],
    arguments: &[String],
    environment: &[String],
) -> Result<LoadedElf, ElfError> {
    let process_environment = try_clone_strings(environment)?;
    let mut executable = parse(bytes)?;
    if executable.elf_type == ELF_ET_DYN {
        if executable.interpreter.is_none() {
            return Err(ElfError::DynamicLinkingUnsupported);
        }
        apply_load_bias(&mut executable, randomized_pie_load_bias())?;
    }
    let interpreter = if let Some(path) = executable.interpreter.as_deref() {
        let interpreter_bytes = read_executable_file(path)?;
        let parsed = parse(&interpreter_bytes)?;
        if parsed.elf_type != ELF_ET_EXEC || parsed.interpreter.is_some() {
            return Err(ElfError::DynamicLinkingUnsupported);
        }
        Some((interpreter_bytes, parsed))
    } else {
        None
    };
    if executable.interpreter.is_some() && executable.program_headers == 0 {
        return Err(ElfError::InvalidHeader);
    }

    let (stack_top, stack_start, guard_start) = stack_layout(next_random_u64())?;

    let mut total_pages = STACK_PAGES;
    let mmap_entropy = (next_random_u64() % 128) * PAGE_SIZE;
    let mut mmap_cursor = MMAP_START
        .checked_add(mmap_entropy)
        .filter(|address| *address < MMAP_END)
        .unwrap_or(MMAP_START);
    let mut all_segments = Vec::new();
    all_segments
        .try_reserve_exact(
            executable
                .segments
                .len()
                .checked_add(
                    interpreter
                        .as_ref()
                        .map_or(0, |(_, parsed)| parsed.segments.len()),
                )
                .ok_or(ElfError::TooLarge)?,
        )
        .map_err(|_| ElfError::OutOfMemory)?;
    all_segments.extend_from_slice(&executable.segments);
    if let Some((_, parsed)) = &interpreter {
        all_segments.extend_from_slice(&parsed.segments);
    }
    for (index, segment) in all_segments.iter().enumerate() {
        let start = page_start(segment.virtual_address);
        let end = page_end(segment.virtual_address, segment.memory_size)?;
        mmap_cursor = mmap_cursor.max(end);
        if start < stack_top && guard_start < end {
            return Err(ElfError::AddressRange);
        }
        total_pages = total_pages
            .checked_add((end - start) / PAGE_SIZE)
            .ok_or(ElfError::TooLarge)?;
        for other in all_segments.iter().take(index) {
            let other_start = page_start(other.virtual_address);
            let other_end = page_end(other.virtual_address, other.memory_size)?;
            if start < other_end && other_start < end {
                return Err(ElfError::OverlappingSegments);
            }
        }
    }
    if !executable.segments.iter().any(|segment| {
        segment.flags & PF_X != 0
            && executable.entry >= segment.virtual_address
            && executable.entry < segment.virtual_address + segment.memory_size
    }) {
        return Err(ElfError::InvalidEntryPoint);
    }
    let entry = if let Some((_, parsed)) = &interpreter {
        if !parsed.segments.iter().any(|segment| {
            segment.flags & PF_X != 0
                && parsed.entry >= segment.virtual_address
                && parsed.entry < segment.virtual_address + segment.memory_size
        }) {
            return Err(ElfError::InvalidEntryPoint);
        }
        parsed.entry
    } else {
        executable.entry
    };

    unsafe { paging::enable_nxe()? };

    let address_space = paging::AddressSpace::new()?;
    unsafe { address_space.activate() };

    let mut mapped_pages = Vec::new();
    if mapped_pages
        .try_reserve_exact(usize::try_from(total_pages).map_err(|_| ElfError::TooLarge)?)
        .is_err()
    {
        address_space.destroy()?;
        return Err(ElfError::OutOfMemory);
    }

    let load_result = (|| {
        map_elf_segments(&executable.segments, bytes, &mut mapped_pages)?;
        if let Some((interpreter_bytes, parsed)) = &interpreter {
            map_elf_segments(&parsed.segments, interpreter_bytes, &mut mapped_pages)?;
        }

        for page_index in 0..STACK_PAGES {
            map_zeroed_page(
                stack_start + page_index * PAGE_SIZE,
                paging::flags::USER | paging::flags::WRITABLE | paging::flags::NO_EXECUTE,
                &mut mapped_pages,
            )?;
        }

        let interpreter_base = interpreter
            .as_ref()
            .and_then(|(_, parsed)| {
                parsed
                    .segments
                    .iter()
                    .map(|segment| page_start(segment.virtual_address))
                    .min()
            })
            .unwrap_or(0);
        let auxv = [
            (3, executable.program_headers),
            (4, executable.program_header_size),
            (5, executable.program_header_count),
            (6, PAGE_SIZE),
            (7, interpreter_base),
            (9, executable.entry),
            (0, 0),
        ];
        let (user_stack, initial_stack) = build_initial_stack(
            stack_start,
            STACK_PAGES * PAGE_SIZE,
            arguments,
            environment,
            &auxv,
        )?;
        copy_to_user_pages(user_stack, &initial_stack)?;
        Ok::<u64, ElfError>(user_stack)
    })();

    match load_result {
        Ok(user_stack) => {
            unsafe { address_space.activate_kernel() };
            Ok(LoadedElf {
                entry,
                user_stack,
                environment: process_environment,
                memory: Arc::new(UserMemory {
                    mmap_cursor: Mutex::new(mmap_cursor),
                    mapped_pages: Mutex::new(mapped_pages),
                    mmap_pages: Mutex::new(Vec::new()),
                    lock_future_mappings: AtomicBool::new(false),
                    address_space,
                }),
            })
        }
        Err(error) => {
            let page_cleanup = release_pages(&mut mapped_pages);
            let table_cleanup = address_space.destroy();
            page_cleanup?;
            table_cleanup?;
            Err(error)
        }
    }
}

fn map_elf_segments(
    segments: &[LoadSegment],
    bytes: &[u8],
    mapped_pages: &mut Vec<u64>,
) -> Result<(), ElfError> {
    for segment in segments {
        let start = page_start(segment.virtual_address);
        let end = page_end(segment.virtual_address, segment.memory_size)?;
        let mut page = start;
        while page < end {
            let mut flags = paging::flags::USER;
            if segment.flags & PF_W != 0 {
                flags |= paging::flags::WRITABLE;
            }
            if segment.flags & PF_X == 0 {
                flags |= paging::flags::NO_EXECUTE;
            }
            map_zeroed_page(page, flags, mapped_pages)?;
            page += PAGE_SIZE;
        }
        if segment.file_size != 0 {
            let file_end = segment
                .file_offset
                .checked_add(segment.file_size)
                .ok_or(ElfError::OutOfBounds)?;
            let source = bytes
                .get(segment.file_offset..file_end)
                .ok_or(ElfError::OutOfBounds)?;
            copy_to_user_pages(segment.virtual_address, source)?;
        }
    }
    Ok(())
}

fn apply_load_bias(parsed: &mut ParsedElf, load_bias: u64) -> Result<(), ElfError> {
    parsed.entry = parsed
        .entry
        .checked_add(load_bias)
        .ok_or(ElfError::AddressRange)?;
    if parsed.program_headers != 0 {
        parsed.program_headers = parsed
            .program_headers
            .checked_add(load_bias)
            .ok_or(ElfError::AddressRange)?;
    }
    for segment in &mut parsed.segments {
        segment.virtual_address = segment
            .virtual_address
            .checked_add(load_bias)
            .ok_or(ElfError::AddressRange)?;
        let end = segment
            .virtual_address
            .checked_add(segment.memory_size)
            .ok_or(ElfError::AddressRange)?;
        if segment.virtual_address < paging::USER_ADDRESS_MIN || end > paging::USER_ADDRESS_MAX {
            return Err(ElfError::AddressRange);
        }
    }
    Ok(())
}

/// Load and synchronously run an ELF file from the mounted VFS.
pub fn run_file(path: &str) -> Result<u64, ElfError> {
    run_file_from(path, "/")
}

/// Load and synchronously run an ELF file, inheriting the caller's working directory.
pub fn run_file_from(path: &str, cwd: &str) -> Result<u64, ElfError> {
    run_file_from_with_args(path, cwd, &[])
}

/// Load and run an ELF with the supplied `argv` vector and working directory.
pub fn run_file_from_with_args(
    path: &str,
    cwd: &str,
    arguments: &[String],
) -> Result<u64, ElfError> {
    run_file_from_with_args_on_terminal(path, cwd, arguments, crate::terminal::active_terminal())
}

pub fn run_file_from_with_args_on_terminal(
    path: &str,
    cwd: &str,
    arguments: &[String],
    terminal: usize,
) -> Result<u64, ElfError> {
    run_file_from_with_args_and_environment_on_terminal(path, cwd, arguments, &[], terminal)
}

pub fn run_file_from_with_args_and_environment_on_terminal(
    path: &str,
    cwd: &str,
    arguments: &[String],
    environment: &[String],
    terminal: usize,
) -> Result<u64, ElfError> {
    let bytes = read_executable_file(path)?;
    load_with_args_and_environment(&bytes, arguments, environment)?
        .run_with_cwd_on_terminal(cwd, terminal)
}

/// Run several ELF files concurrently under the user-process scheduler.
pub fn run_files_parallel_from(paths: &[String], cwd: &str) -> Result<Vec<u64>, ElfError> {
    run_files_parallel_from_on_terminal(paths, cwd, crate::terminal::active_terminal())
}

pub fn run_files_parallel_from_on_terminal(
    paths: &[String],
    cwd: &str,
    terminal: usize,
) -> Result<Vec<u64>, ElfError> {
    run_files_parallel_from_with_environment_on_terminal(paths, cwd, &[], terminal)
}

pub fn run_files_parallel_from_with_environment_on_terminal(
    paths: &[String],
    cwd: &str,
    environment: &[String],
    terminal: usize,
) -> Result<Vec<u64>, ElfError> {
    if paths.len() < 2 {
        return Err(ElfError::InvalidArguments);
    }

    let mut images = Vec::new();
    images
        .try_reserve_exact(paths.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for path in paths {
        let bytes = match read_executable_file(path) {
            Ok(bytes) => bytes,
            Err(error) => return fail_after_unload(images, error),
        };
        match load_with_args_and_environment(&bytes, core::slice::from_ref(path), environment) {
            Ok(image) => images.push(image),
            Err(error) => return fail_after_unload(images, error),
        }
    }

    run_loaded_processes(images, cwd, terminal)
}

fn read_executable_file(path: &str) -> Result<Vec<u8>, ElfError> {
    fs::with_vfs(|vfs| {
        let metadata = vfs.metadata(path)?;
        if metadata.permissions & 0o111 == 0 {
            return Err(ElfError::PermissionDenied);
        }
        if metadata.len == 0 {
            return Err(ElfError::InvalidHeader);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(metadata.len)
            .map_err(|_| ElfError::OutOfMemory)?;
        bytes.resize(metadata.len, 0);
        let read = vfs.read(path, 0, &mut bytes)?;
        if read != bytes.len() {
            return Err(ElfError::OutOfBounds);
        }
        Ok::<_, ElfError>(bytes)
    })?
}

fn run_loaded_processes(
    images: Vec<LoadedElf>,
    cwd: &str,
    terminal: usize,
) -> Result<Vec<u64>, ElfError> {
    if images.is_empty() {
        return Err(ElfError::InvalidArguments);
    }
    if images.len() > MAX_SCHEDULED_PROCESSES {
        return fail_after_unload(images, ElfError::InvalidArguments);
    }
    let scheduler_busy = {
        let scheduler = USER_SCHEDULER.lock();
        scheduler.active || !scheduler.processes.is_empty()
    };
    if scheduler_busy || USER_PROCESS.lock().active {
        return fail_after_unload(images, ElfError::ProcessAlreadyRunning);
    }

    let mut states = Vec::new();
    if states.try_reserve_exact(images.len()).is_err() {
        return fail_after_unload(images, ElfError::OutOfMemory);
    }
    for (index, image) in images.iter().enumerate() {
        let pid = index as u64 + 1;
        let process_environment = match try_clone_strings(&image.environment) {
            Ok(environment) => environment,
            Err(error) => return fail_after_unload(images, error),
        };
        match new_user_process(cwd, pid, terminal, process_environment) {
            Ok(state) => states.push(state),
            Err(error) => return fail_after_unload(images, error),
        }
    }

    let mut stacks = Vec::new();
    if stacks.try_reserve_exact(images.len()).is_err() {
        return fail_after_unload(images, ElfError::OutOfMemory);
    }
    stacks.push(None);
    for _ in 1..images.len() {
        match ProcessKernelStack::new() {
            Ok(stack) => stacks.push(Some(stack)),
            Err(error) => return fail_after_unload(images, error),
        }
    }

    let mut processes = Vec::new();
    if processes.try_reserve_exact(images.len()).is_err() {
        return fail_after_unload(images, ElfError::OutOfMemory);
    }
    let next_pid = images.len() as u64 + 1;
    for (index, ((image, state), kernel_stack)) in
        images.into_iter().zip(states).zip(stacks).enumerate()
    {
        let kernel_stack_top = kernel_stack
            .as_ref()
            .map_or_else(gdt::user_kernel_stack_top, ProcessKernelStack::top);
        let frame_pointer = kernel_stack
            .as_ref()
            .map_or(0, |stack| initialize_user_frame(&image, stack.top()));
        processes.push(ScheduledProcess {
            image,
            state,
            pid: index as u64 + 1,
            _kernel_stack: kernel_stack,
            kernel_stack_top,
            frame_pointer,
            exit_status: None,
            parent_pid: None,
            is_thread: false,
            tid: index as u64 + 1,
            thread_mapping: None,
            waiting_for: None,
            wait_deadline: None,
            resume_context: None,
            wait_status_pointer: 0,
            priority: 0,
            policy: SCHED_OTHER as u32,
            rt_priority: 0,
            rt_slice: RT_RR_QUANTUM_TICKS,
            robust_list: 0,
            robust_list_length: 0,
            uid: 0,
            euid: 0,
            gid: 0,
            egid: 0,
            capabilities: capabilities_for_uid(0),
            age: 0,
            runtime: TaskRuntime::default(),
            reaped: false,
        });
    }

    {
        let mut scheduler = USER_SCHEDULER.lock();
        scheduler.processes = processes;
        scheduler.current = 0;
        scheduler.active = true;
        scheduler.next_pid = next_pid;
        scheduler.next_tid = next_pid;
        let mut active_process = USER_PROCESS.lock();
        core::mem::swap(&mut *active_process, &mut scheduler.processes[0].state);
        let first = &scheduler.processes[0].image;
        unsafe {
            first.memory.address_space.activate();
        }
    }

    let (entry, user_stack) = {
        let scheduler = USER_SCHEDULER.lock();
        let first = &scheduler.processes[0].image;
        (first.entry, first.user_stack)
    };
    let mut first_entry = true;
    loop {
        if first_entry {
            thread_pointer::write(0);
            restore_current_user_fpu();
            let _last_status = unsafe { gdt::enter_user_mode(entry, user_stack) };
            first_entry = false;
        } else {
            if let Some(first) = USER_SCHEDULER.lock().processes.first() {
                unsafe { first.image.memory.address_space.activate_kernel() };
            }
            unsafe { crate::drivers::poll_deferred() };
            crate::network::poll_tcp();
            wake_console_waiters(crate::fs::pump_console_input());
            let only_current_process_runnable = {
                let scheduler = USER_SCHEDULER.lock();
                is_process_runnable(&scheduler.processes[scheduler.current])
                    && scheduler
                        .processes
                        .iter()
                        .filter(|process| is_process_runnable(process))
                        .count()
                        == 1
            };
            if only_current_process_runnable {
                let before_idle = crate::arch::x86_64::monotonic_milliseconds();
                unsafe {
                    core::arch::asm!("sti; hlt; cli");
                }
                let idle_gap = crate::arch::x86_64::monotonic_milliseconds()
                    .saturating_sub(before_idle);
                if idle_gap > IDLE_STALL_WATCHDOG_MS {
                    crate::println!(
                        "[sched] watchdog: idle wakeup delayed {idle_gap} ms (interrupt macet?)"
                    );
                }
                unsafe { crate::drivers::poll_deferred() };
                crate::network::poll_tcp();
                wake_console_waiters(crate::fs::pump_console_input());
            }

            let next = {
                let mut scheduler = USER_SCHEDULER.lock();
                if scheduler
                    .processes
                    .iter()
                    .all(|process| process.exit_status.is_some())
                {
                    break;
                }
                schedule_next(&mut scheduler).map(|next| {
                    let frame_pointer = scheduler.processes[next].frame_pointer;
                    // Capture the next task's TLS base before swapping its
                    // state into USER_PROCESS. After the swap, scheduler.processes[next].state
                    // contains the previously active task, not the task being resumed.
                    let thread_pointer_value = scheduler.processes[next].state.thread_pointer;
                    if let Some(context) = scheduler.processes[next].resume_context.take() {
                        let frame = unsafe { &mut *(frame_pointer as *mut InterruptFrame) };
                        frame.rax = context.accumulator;
                        frame.stack_frame = context.stack_frame;
                    }
                    let mut active_process = USER_PROCESS.lock();
                    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
                    scheduler.current = next;
                    let process = &scheduler.processes[next];
                    unsafe {
                        process.image.memory.address_space.activate();
                        gdt::set_ring0_stack_top(process.kernel_stack_top);
                    }
                    (frame_pointer, thread_pointer_value)
                })
            };

            let Some((frame_pointer, thread_pointer_value)) = next else {
                unsafe {
                    core::arch::asm!("sti; hlt");
                }
                // Semua proses tidur: layani input agar penunggu console bangun.
                unsafe { crate::drivers::poll_deferred() };
                wake_console_waiters(crate::fs::pump_console_input());
                continue;
            };
            thread_pointer::write(thread_pointer_value);
            restore_current_user_fpu();
            let _last_status = unsafe { gdt::resume_user_frame(frame_pointer as *mut u8) };
        }
    }

    thread_pointer::write(0);
    restore_current_user_fpu();

    let mut scheduler = USER_SCHEDULER.lock();
    if let Some(first) = scheduler.processes.first() {
        unsafe {
            first.image.memory.address_space.activate_kernel();
        }
    }
    let all_exited = scheduler
        .processes
        .iter()
        .all(|process| process.exit_status.is_some());
    scheduler.active = false;
    let processes = core::mem::take(&mut scheduler.processes);
    drop(scheduler);

    let mut statuses = Vec::new();
    let status_reserve_failed = statuses.try_reserve_exact(processes.len()).is_err();
    let mut missing_status = false;
    let mut cleanup_error = None;
    for process in processes {
        if let Some(status) = process.exit_status {
            if !status_reserve_failed {
                statuses.push(status);
            }
        } else {
            missing_status = true;
        }
        if let Err(error) = process.image.unload()
            && cleanup_error.is_none()
        {
            cleanup_error = Some(error);
        }
    }
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    if status_reserve_failed {
        return Err(ElfError::OutOfMemory);
    }
    if !all_exited || missing_status {
        return Err(ElfError::SchedulerInvariant);
    }
    Ok(statuses)
}

fn fail_after_unload<T>(images: Vec<LoadedElf>, error: ElfError) -> Result<T, ElfError> {
    unload_images(images)?;
    Err(error)
}

fn unload_images(images: Vec<LoadedElf>) -> Result<(), ElfError> {
    let mut first_error = None;
    for image in images {
        if let Err(error) = image.unload()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn initialize_user_frame(image: &LoadedElf, stack_top: u64) -> usize {
    let frame_pointer = (stack_top as usize - core::mem::size_of::<InterruptFrame>()) & !0xF;
    let frame = InterruptFrame {
        r15: 0,
        r14: 0,
        r13: 0,
        r12: 0,
        r11: 0,
        r10: 0,
        r9: 0,
        r8: 0,
        rbp: 0,
        rdi: 0,
        rsi: 0,
        rdx: 0,
        rcx: 0,
        rbx: 0,
        rax: 0,
        vector: u64::from(interrupt::TIMER_VECTOR),
        error_code: 0,
        stack_frame: InterruptStackFrame {
            instruction_pointer: image.entry,
            code_segment: u64::from(gdt::USER_CODE_SELECTOR),
            cpu_flags: 0x202,
            stack_pointer: image.user_stack,
            stack_segment: u64::from(gdt::USER_DATA_SELECTOR),
        },
    };
    unsafe {
        (frame_pointer as *mut InterruptFrame).write(frame);
    }
    frame_pointer
}

fn initialize_fork_frame(frame: &InterruptFrame, stack_top: u64) -> usize {
    let frame_pointer = (stack_top as usize - core::mem::size_of::<InterruptFrame>()) & !0xF;
    unsafe {
        core::ptr::copy_nonoverlapping(
            frame as *const InterruptFrame,
            frame_pointer as *mut InterruptFrame,
            1,
        );
        (*(frame_pointer as *mut InterruptFrame)).rax = 0;
    }
    frame_pointer
}

/// Menghitung tata letak stack user: puncak stack digeser 0..STACK_RANDOM_PAGES
/// halaman ke bawah, diikuti halaman guard tepat di bawah stack.
fn stack_layout(random: u64) -> Result<(u64, u64, u64), ElfError> {
    let stack_top = STACK_TOP.saturating_sub((random % STACK_RANDOM_PAGES) * PAGE_SIZE);
    let stack_start = stack_top
        .checked_sub(STACK_PAGES * PAGE_SIZE)
        .ok_or(ElfError::AddressRange)?;
    let guard_start = stack_start
        .checked_sub(PAGE_SIZE)
        .ok_or(ElfError::AddressRange)?;
    Ok((stack_top, stack_start, guard_start))
}

fn build_initial_stack(
    stack_start: u64,
    stack_size: u64,
    arguments: &[String],
    environment: &[String],
    auxiliary_vector: &[(u64, u64)],
) -> Result<(u64, Vec<u8>), ElfError> {
    if arguments.len() > MAX_ARGUMENTS || environment.len() > MAX_ENVIRONMENT_VARIABLES {
        return Err(ElfError::ArgumentsTooLarge);
    }
    let argument_bytes = arguments.iter().try_fold(0usize, |total, argument| {
        if argument.as_bytes().contains(&0) {
            return Err(ElfError::InvalidArguments);
        }
        let argument_size = argument
            .len()
            .checked_add(1)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        total
            .checked_add(argument_size)
            .ok_or(ElfError::ArgumentsTooLarge)
    })?;
    if argument_bytes > MAX_ARGUMENT_BYTES {
        return Err(ElfError::ArgumentsTooLarge);
    }
    let environment_bytes = environment.iter().try_fold(0usize, |total, variable| {
        let Some((name, _)) = variable.split_once('=') else {
            return Err(ElfError::InvalidArguments);
        };
        if !valid_environment_key(name) || variable.as_bytes().contains(&0) {
            return Err(ElfError::InvalidArguments);
        }
        total
            .checked_add(
                variable
                    .len()
                    .checked_add(1)
                    .ok_or(ElfError::ArgumentsTooLarge)?,
            )
            .ok_or(ElfError::ArgumentsTooLarge)
    })?;
    if environment_bytes > MAX_ENVIRONMENT_BYTES {
        return Err(ElfError::ArgumentsTooLarge);
    }
    let stack_size = usize::try_from(stack_size).map_err(|_| ElfError::ArgumentsTooLarge)?;
    let stack_size_u64 = u64::try_from(stack_size).map_err(|_| ElfError::ArgumentsTooLarge)?;
    let stack_end = stack_start
        .checked_add(stack_size_u64)
        .ok_or(ElfError::AddressRange)?;
    let mut stack = Vec::new();
    stack
        .try_reserve_exact(stack_size)
        .map_err(|_| ElfError::OutOfMemory)?;
    stack.resize(stack_size, 0);

    let mut cursor = stack_size;
    let mut pointers = Vec::new();
    pointers
        .try_reserve_exact(arguments.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for argument in arguments.iter().rev() {
        let size = argument
            .len()
            .checked_add(1)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        cursor = cursor
            .checked_sub(size)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        stack[cursor..cursor + argument.len()].copy_from_slice(argument.as_bytes());
        pointers.push(
            stack_start
                .checked_add(cursor as u64)
                .ok_or(ElfError::AddressRange)?,
        );
    }
    pointers.reverse();

    let mut environment_pointers = Vec::new();
    environment_pointers
        .try_reserve_exact(environment.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for variable in environment.iter().rev() {
        let size = variable
            .len()
            .checked_add(1)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        cursor = cursor
            .checked_sub(size)
            .ok_or(ElfError::ArgumentsTooLarge)?;
        stack[cursor..cursor + variable.len()].copy_from_slice(variable.as_bytes());
        environment_pointers.push(
            stack_start
                .checked_add(cursor as u64)
                .ok_or(ElfError::AddressRange)?,
        );
    }
    environment_pointers.reverse();

    let table_words = arguments
        .len()
        .checked_add(environment.len())
        .ok_or(ElfError::ArgumentsTooLarge)?
        .checked_add(3)
        .and_then(|words| words.checked_add(auxiliary_vector.len().checked_mul(2)?))
        .ok_or(ElfError::ArgumentsTooLarge)?;
    let table_bytes = table_words
        .checked_mul(core::mem::size_of::<u64>())
        .ok_or(ElfError::ArgumentsTooLarge)?;
    cursor = cursor
        .checked_sub(table_bytes)
        .ok_or(ElfError::ArgumentsTooLarge)?
        & !0xF;
    let stack_pointer = stack_start
        .checked_add(cursor as u64)
        .ok_or(ElfError::AddressRange)?;
    if stack_pointer >= stack_end {
        return Err(ElfError::ArgumentsTooLarge);
    }

    let write_word = |stack: &mut [u8], offset: usize, value: u64| {
        stack[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    };
    write_word(&mut stack, cursor, arguments.len() as u64);
    for (index, pointer) in pointers.iter().enumerate() {
        write_word(&mut stack, cursor + 8 * (index + 1), *pointer);
    }
    let environment_start = arguments.len() + 2;
    for (index, pointer) in environment_pointers.iter().enumerate() {
        write_word(
            &mut stack,
            cursor + 8 * (environment_start + index),
            *pointer,
        );
    }
    write_word(
        &mut stack,
        cursor + 8 * (arguments.len() + environment.len() + 2),
        0,
    );
    let auxiliary_start = arguments.len() + environment.len() + 3;
    for (index, (tag, value)) in auxiliary_vector.iter().enumerate() {
        write_word(&mut stack, cursor + 8 * (auxiliary_start + index * 2), *tag);
        write_word(
            &mut stack,
            cursor + 8 * (auxiliary_start + index * 2 + 1),
            *value,
        );
    }
    let mut initial_stack = Vec::new();
    initial_stack
        .try_reserve_exact(stack_size - cursor)
        .map_err(|_| ElfError::OutOfMemory)?;
    initial_stack.extend_from_slice(&stack[cursor..]);
    Ok((stack_pointer, initial_stack))
}

pub fn handle_syscall(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    USER_SCHEDULER.lock().retired_kernel_stacks.clear();
    save_current_thread_pointer();
    save_current_user_fpu();
    let mut next_frame = handle_syscall_inner(frame);
    if next_frame == frame as *mut InterruptFrame
        && frame.stack_frame.code_segment & 3 == 3
    {
        if let Some(replacement) = deliver_pending_signal(frame) {
            next_frame = replacement;
        }
    }
    // A syscall may hand control back to the kernel scheduler (for example
    // kernel_yield or process exit) by rewriting the saved CS to ring 0.
    // USER_PROCESS has already been swapped out in that case, so restoring
    // its TLS/FPU state here would install stale state while the kernel
    // trampoline is running. The scheduler restores the selected user task
    // immediately before resuming its saved user frame.
    let returns_to_user = unsafe {
        (*next_frame).stack_frame.code_segment & 3 == 3
    };
    if returns_to_user {
        restore_current_thread_pointer();
        restore_current_user_fpu();
    }
    next_frame
}

fn handle_syscall_inner(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    match frame.rax {
        SYSCALL_EXIT => terminate_user_inner(frame, frame.rdi),
        SYSCALL_KERNEL_YIELD => yield_user_to_kernel(frame),
        SYSCALL_WAITPID => syscall_waitpid(frame),
        SYSCALL_EXECVE => syscall_execve(frame),
        SYSCALL_FORK => syscall_fork(frame),
        SYSCALL_THREAD_CREATE => syscall_thread_create(frame),
        SYSCALL_THREAD_EXIT => syscall_thread_exit(frame),
        SYSCALL_THREAD_JOIN => syscall_thread_join(frame),
        SYSCALL_FUTEX => syscall_futex(frame),
        SYSCALL_SIGRETURN => syscall_sigreturn(frame),
        SYSCALL_POLL => syscall_poll_frame(frame),
        SYSCALL_SELECT => syscall_select_frame(frame),
        number => {
            let result = dispatch_syscall(
                number, frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9,
            );
            if result == Err(EAGAIN) {
                if take_interrupted_syscall(number) {
                    frame.rax = (-(EINTR as i64)) as u64;
                    return frame_pointer;
                }
                let pipe_wait = match number {
                    SYSCALL_READ => pipe_wait_target(frame.rdi, true),
                    SYSCALL_WRITE => pipe_wait_target(frame.rdi, false),
                    _ => None,
                };
                if let Some((_, wait_reason)) = pipe_wait {
                    return block_current_on_wait(frame, wait_reason);
                }
                if number == SYSCALL_READ
                    && let Some(terminal) = console_wait_target(frame.rdi)
                {
                    return block_current_on_wait(frame, ProcessWait::ConsoleInput(terminal));
                }
            }
            frame.rax = match result {
                Ok(value) => value,
                Err(errno) => (-(errno as i64)) as u64,
            };
            frame_pointer
        }
    }
}

fn yield_user_to_kernel(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    if !scheduler.active || current >= scheduler.processes.len() {
        frame.rax = (-(EINTR as i64)) as u64;
        return frame_pointer;
    }

    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    let mut active_process = USER_PROCESS.lock();
    if !active_process.active {
        frame.rax = (-(EINTR as i64)) as u64;
        return frame_pointer;
    }
    scheduler.processes[current].resume_context = Some(UserResumeContext {
        accumulator: 0,
        stack_frame: frame.stack_frame,
    });
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    frame.rax = 0;
    frame.stack_frame.instruction_pointer = gdt::user_exit_address();
    frame.stack_frame.code_segment = gdt::KERNEL_CODE_SELECTOR as u64;
    frame.stack_frame.stack_pointer = gdt::user_kernel_stack_top();
    frame.stack_frame.stack_segment = gdt::KERNEL_DATA_SELECTOR as u64;
    gdt::record_user_exit(0);
    frame_pointer
}

pub fn handle_timer(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let from_user = frame.stack_frame.code_segment & 3 == 3;
    if from_user {
        save_current_thread_pointer();
        save_current_user_fpu();
    }
    let next_frame = handle_timer_inner(frame);
    if from_user {
        restore_current_thread_pointer();
        restore_current_user_fpu();
    }
    next_frame
}

fn handle_timer_inner(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let from_user = frame.stack_frame.code_segment & 3 == 3;

    let mut scheduler = USER_SCHEDULER.lock();
    expire_futex_timeouts(&mut scheduler);
    if !from_user {
        return frame_pointer;
    }
    if !scheduler.active || scheduler.processes.is_empty() {
        return frame_pointer;
    }

    // Akuntansi waktu nyata: satu slot per tick timer.
    let now = crate::arch::x86_64::monotonic_milliseconds();
    let elapsed = if scheduler.last_tick_ms == 0 {
        0
    } else {
        now.saturating_sub(scheduler.last_tick_ms).min(1000)
    };
    scheduler.last_tick_ms = now;
    if scheduler.runtime_started_ms == 0 {
        scheduler.runtime_started_ms = now;
    }
    account_runtime(&mut scheduler, elapsed);

    let current = scheduler.current;
    if current >= scheduler.processes.len() {
        return frame_pointer;
    }
    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    // Kuota SCHED_RR habis: rotasi ke task RR lain dengan prioritas sama.
    let force_rotate = {
        let process = &mut scheduler.processes[current];
        if process.policy != SCHED_RR as u32 || !is_process_runnable(process) {
            false
        } else if process.rt_slice > 1 {
            process.rt_slice -= 1;
            false
        } else {
            true
        }
    };
    let Some(next) = schedule_next_rotating(&mut scheduler, force_rotate) else {
        return frame_pointer;
    };
    if next == current {
        return frame_pointer;
    }

    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.memory.address_space.activate();
    }
    next_frame
}

fn syscall_set_thread_pointer(value: u64) -> Result<u64, i32> {
    if !thread_pointer::is_valid_user_thread_pointer(value) {
        return Err(EINVAL);
    }

    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    process.thread_pointer = value;
    thread_pointer::write(value);
    Ok(0)
}

fn syscall_get_thread_pointer() -> Result<u64, i32> {
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    Ok(process.thread_pointer)
}

fn syscall_ioctl(fd: u64, request: u64, arg: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let terminal = process.terminal;
    let descriptor = process
        .descriptors
        .get(fd)
        .and_then(Option::as_ref)
        .ok_or(EBADF)?;

    match descriptor {
        UserDescriptor::File(file) => {
            let path = file.lock().path.clone();
            drop(process);
            if fs::is_fb_path(&path) {
                if !device_access_allowed(&path, terminal, current_euid()?, current_capabilities()?) {
                    return Err(EPERM);
                }
                handle_fb_ioctl(request, arg)
            } else if fs::is_tty_path(&path) {
                if !device_access_allowed(&path, terminal, current_euid()?, current_capabilities()?) {
                    return Err(EPERM);
                }
                handle_tty_ioctl(request, arg)
            } else {
                Err(ENOTTY)
            }
        }
        UserDescriptor::ConsoleInput
        | UserDescriptor::ConsoleOutput
        | UserDescriptor::ConsoleError => {
            drop(process);
            handle_tty_ioctl(request, arg)
        }
        _ => Err(ENOTTY),
    }
}

fn handle_fb_ioctl(request: u64, arg: u64) -> Result<u64, i32> {
    let fb = crate::graphics::Framebuffer::get().ok_or(ENODEV)?;
    match request {
        FBIOGET_VSCREENINFO => {
            paging::validate_user_range(arg, core::mem::size_of::<FbVarScreeninfo>(), true)
                .map_err(|_| EFAULT)?;
            let mut var = FbVarScreeninfo::default();
            var.xres = fb.width() as u32;
            var.yres = fb.height() as u32;
            var.xres_virtual = fb.width() as u32;
            var.yres_virtual = fb.height() as u32;
            var.bits_per_pixel = 32;
            var.red = FbBitfield { offset: 16, length: 8, msb_right: 0 };
            var.green = FbBitfield { offset: 8, length: 8, msb_right: 0 };
            var.blue = FbBitfield { offset: 0, length: 8, msb_right: 0 };
            var.transp = FbBitfield { offset: 24, length: 8, msb_right: 0 };
            var.width = fb.width() as u32;
            var.height = fb.height() as u32;
            unsafe { (arg as *mut FbVarScreeninfo).write_unaligned(var) };
            Ok(0)
        }
        FBIOPUT_VSCREENINFO => {
            paging::validate_user_range(arg, core::mem::size_of::<FbVarScreeninfo>(), false)
                .map_err(|_| EFAULT)?;
            let var = unsafe { (arg as *const FbVarScreeninfo).read_unaligned() };
            if var.xres == fb.width() as u32 && var.yres == fb.height() as u32 {
                Ok(0)
            } else {
                Err(EINVAL)
            }
        }
        FBIOGET_FSCREENINFO => {
            paging::validate_user_range(arg, core::mem::size_of::<FbFixScreeninfo>(), true)
                .map_err(|_| EFAULT)?;
            let smem_start = crate::memory::virtual_to_physical(fb.addr() as usize).unwrap_or(0) as u64;
            let mut fix = FbFixScreeninfo {
                id: [0; 16],
                smem_start,
                smem_len: (fb.pitch() * fb.height()) as u32,
                fb_type: 0,
                type_aux: 0,
                visual: 2,
                xpanstep: 0,
                ypanstep: 0,
                ywrapstep: 0,
                line_length: fb.pitch() as u32,
                mmio_start: 0,
                mmio_len: 0,
                accel: 0,
                capabilities: 0,
                reserved: [0; 2],
            };
            let name = b"octagram-fb\0";
            fix.id[..name.len()].copy_from_slice(name);
            unsafe { (arg as *mut FbFixScreeninfo).write_unaligned(fix) };
            Ok(0)
        }
        FBIOBLANK => {
            fb.clear();
            Ok(0)
        }
        _ => Err(EINVAL),
    }
}

fn handle_tty_ioctl(request: u64, arg: u64) -> Result<u64, i32> {
    match request {
        TCGETS => {
            paging::validate_user_range(arg, core::mem::size_of::<fs::Termios>(), true)
                .map_err(|_| EFAULT)?;
            let termios = fs::console_termios();
            unsafe { (arg as *mut fs::Termios).write_unaligned(termios) };
            Ok(0)
        }
        TCSETS | TCSETSW | TCSETSF => {
            paging::validate_user_range(arg, core::mem::size_of::<fs::Termios>(), false)
                .map_err(|_| EFAULT)?;
            let termios = unsafe { (arg as *const fs::Termios).read_unaligned() };
            fs::set_console_termios(termios);
            if request == TCSETSF {
                fs::discard_pending_input();
            }
            Ok(0)
        }
        TIOCGWINSZ => {
            paging::validate_user_range(arg, core::mem::size_of::<Winsize>(), true)
                .map_err(|_| EFAULT)?;
            let fb = crate::graphics::Framebuffer::get();
            let (cols, rows) = if let Some(fb) = fb.as_ref() {
                ((fb.width() / 8) as u16, (fb.height() / 16) as u16)
            } else {
                (80, 25)
            };
            let ws = Winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: fb.as_ref().map_or(640, |f| f.width() as u16),
                ws_ypixel: fb.as_ref().map_or(400, |f| f.height() as u16),
            };
            unsafe { (arg as *mut Winsize).write_unaligned(ws) };
            Ok(0)
        }
        _ => Err(EINVAL),
    }
}

fn dispatch_syscall(
    number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> Result<u64, i32> {
    match number {
        SYSCALL_WRITE => syscall_write(arg1, arg2, arg3),
        SYSCALL_READ => syscall_read(arg1, arg2, arg3),
        SYSCALL_OPEN => syscall_open(arg1, arg2, arg3),
        SYSCALL_CLOSE => syscall_close(arg1),
        SYSCALL_LSEEK => syscall_lseek(arg1, arg2 as i64, arg3),
        SYSCALL_STAT => syscall_stat(arg1, arg2),
        SYSCALL_MKDIR => syscall_mkdir(arg1, arg2),
        SYSCALL_UNLINK => syscall_unlink(arg1),
        SYSCALL_FTRUNCATE => syscall_ftruncate(arg1, arg2),
        SYSCALL_CHDIR => syscall_chdir(arg1),
        SYSCALL_GETCWD => syscall_getcwd(arg1, arg2),
        SYSCALL_IOCTL => syscall_ioctl(arg1, arg2, arg3),
        SYSCALL_CHMOD => syscall_chmod(arg1, arg2),
        SYSCALL_GETPID => {
            let process = USER_PROCESS.lock();
            if process.active {
                Ok(process.pid)
            } else {
                Err(EINTR)
            }
        }
        SYSCALL_MMAP => syscall_mmap(arg1, arg2, arg3, arg4, arg5, arg6),
        SYSCALL_MUNMAP => syscall_munmap(arg1, arg2),
        SYSCALL_PIPE => syscall_pipe(arg1),
        SYSCALL_DUP2 => syscall_dup2(arg1, arg2),
        SYSCALL_OPENDIR => syscall_opendir(arg1),
        SYSCALL_READDIR => syscall_readdir(arg1, arg2),
        SYSCALL_FSTAT => syscall_fstat(arg1, arg2),
        SYSCALL_ACCESS => syscall_access(arg1, arg2),
        SYSCALL_RENAME => syscall_rename(arg1, arg2),
        SYSCALL_UMASK => syscall_umask(arg1),
        SYSCALL_DUP => syscall_dup(arg1),
        SYSCALL_FCNTL => syscall_fcntl(arg1, arg2, arg3),
        SYSCALL_RMDIR => syscall_rmdir(arg1),
        SYSCALL_SYMLINK => syscall_symlink(arg1, arg2),
        SYSCALL_READLINK => syscall_readlink(arg1, arg2, arg3),
        SYSCALL_LSTAT => syscall_lstat(arg1, arg2),
        SYSCALL_FCHMOD => syscall_fchmod(arg1, arg2),
        SYSCALL_OPENAT => syscall_openat(arg1, arg2, arg3, arg4),
        SYSCALL_FSTATAT => syscall_fstatat(arg1, arg2, arg3, arg4),
        SYSCALL_MKDIRAT => syscall_mkdirat(arg1, arg2, arg3),
        SYSCALL_UNLINKAT => syscall_unlinkat(arg1, arg2, arg3),
        SYSCALL_RENAMEAT => syscall_renameat(arg1, arg2, arg3, arg4),
        SYSCALL_READLINKAT => syscall_readlinkat(arg1, arg2, arg3, arg4),
        SYSCALL_SYMLINKAT => syscall_symlinkat(arg1, arg2, arg3),
        SYSCALL_CLOCK_GETTIME => syscall_clock_gettime(arg1, arg2),
        SYSCALL_MSYNC => syscall_msync(arg1, arg2, arg3),
        SYSCALL_MPROTECT => syscall_mprotect(arg1, arg2, arg3),
        SYSCALL_MINCORE => syscall_mincore(arg1, arg2, arg3),
        SYSCALL_MLOCK => syscall_mlock(arg1, arg2),
        SYSCALL_MUNLOCK => syscall_munlock(arg1, arg2),
        SYSCALL_MLOCKALL => syscall_mlockall(arg1),
        SYSCALL_MUNLOCKALL => syscall_munlockall(),
        SYSCALL_POSIX_MADVISE => syscall_posix_madvise(arg1, arg2, arg3),
        SYSCALL_SET_THREAD_POINTER => syscall_set_thread_pointer(arg1),
        SYSCALL_GET_THREAD_POINTER => syscall_get_thread_pointer(),
        SYSCALL_UDP_SOCKET => syscall_udp_socket(),
        SYSCALL_UDP_BIND => syscall_udp_bind(arg1, arg2),
        SYSCALL_UDP_SENDTO => syscall_udp_sendto(arg1, arg2, arg3, arg4, arg5),
        SYSCALL_UDP_RECVFROM => syscall_udp_recvfrom(arg1, arg2, arg3, arg4, arg5),
        SYSCALL_NETWORK_DNS => syscall_network_dns(arg1),
        SYSCALL_SOCKET => syscall_socket(arg1, arg2, arg3),
        SYSCALL_BIND => syscall_bind(arg1, arg2, arg3),
        SYSCALL_LISTEN => syscall_listen(arg1, arg2),
        SYSCALL_ACCEPT => syscall_accept(arg1, arg2, arg3),
        SYSCALL_CONNECT => syscall_connect(arg1, arg2, arg3),
        SYSCALL_SEND => syscall_send(arg1, arg2, arg3, arg4),
        SYSCALL_RECV => syscall_recv(arg1, arg2, arg3, arg4),
        SYSCALL_SHUTDOWN => syscall_shutdown(arg1, arg2),
        SYSCALL_GETSOCKNAME => syscall_socket_name(arg1, arg2, arg3, false),
        SYSCALL_GETPEERNAME => syscall_socket_name(arg1, arg2, arg3, true),
        SYSCALL_THREAD_SELF => {
            let scheduler = USER_SCHEDULER.lock();
            scheduler
                .processes
                .get(scheduler.current)
                .filter(|_| scheduler.active)
                .map(|process| process.tid)
                .ok_or(EINTR)
        }
        SYSCALL_ISATTY => {
            let fd = usize::try_from(arg1).map_err(|_| EBADF)?;
            let process = USER_PROCESS.lock();
            if !process.active {
                return Err(EINTR);
            }
            match process.descriptors.get(fd).and_then(Option::as_ref) {
                Some(
                    UserDescriptor::ConsoleInput
                    | UserDescriptor::ConsoleOutput
                    | UserDescriptor::ConsoleError,
                ) => Ok(1),
                Some(UserDescriptor::File(file)) if fs::is_tty_path(&file.lock().path) => Ok(1),
                Some(UserDescriptor::File(_)) => Err(ENOTTY),
                Some(UserDescriptor::Directory(_)) => Err(ENOTTY),
                Some(
                    UserDescriptor::Pipe(_)
                    | UserDescriptor::SocketPair(_)
                    | UserDescriptor::Epoll(_)
                    | UserDescriptor::EventFd(_)
                    | UserDescriptor::TimerFd(_)
                    | UserDescriptor::UdpSocket(_)
                    | UserDescriptor::TcpSocket(_),
                ) => Err(ENOTTY),
                None => Err(EBADF),
            }
        }
        // --- syscall untuk kompatibilitas runtime (Python, Java, dsb) ---
        SYSCALL_GETUID => current_credential(|process| process.uid),
        SYSCALL_GETEUID => current_credential(|process| process.euid),
        SYSCALL_GETGID => current_credential(|process| process.gid),
        SYSCALL_GETEGID => current_credential(|process| process.egid),
        SYSCALL_GETPPID => Ok(1),
        SYSCALL_UNAME => syscall_uname(arg1),
        SYSCALL_BRK => syscall_brk(arg1),
        SYSCALL_GETTIMEOFDAY => syscall_gettimeofday(arg1),
        SYSCALL_NANOSLEEP => syscall_nanosleep(arg1),
        SYSCALL_KILL => syscall_kill(arg1, arg2),
        SYSCALL_SIGACTION => syscall_sigaction(arg1, arg2, arg3),
        SYSCALL_SIGALTSTACK => syscall_sigaltstack(arg1, arg2),
        SYSCALL_SIGPROCMASK => syscall_sigprocmask(arg1, arg2, arg3),
        SYSCALL_SIGPENDING => syscall_sigpending(arg1),
        SYSCALL_SIGSUSPEND => syscall_sigsuspend(arg1),
        SYSCALL_SIGNAL => syscall_signal(arg1, arg2),
        SYSCALL_SETSOCKOPT => syscall_setsockopt(arg1, arg2, arg3, arg4, arg5),
        SYSCALL_GETSOCKOPT => syscall_getsockopt(arg1, arg2, arg3, arg4, arg5),
        SYSCALL_POLL => syscall_poll(arg1, arg2, arg3),
        SYSCALL_SELECT => syscall_select(arg1, arg2, arg3, arg4, arg5),
        SYSCALL_CLOCK_GETRES => syscall_clock_getres(arg1, arg2),
        SYSCALL_FSYNC | SYSCALL_FDATASYNC => syscall_fsync(arg1),
        SYSCALL_GETRANDOM => syscall_getrandom(arg1, arg2, arg3),
        SYSCALL_PIPE2 => syscall_pipe2(arg1, arg2),
        SYSCALL_DUP3 => syscall_dup3(arg1, arg2, arg3),
        SYSCALL_EVENTFD => syscall_eventfd(arg1, arg2),
        SYSCALL_TIMERFD_CREATE => syscall_timerfd_create(arg1, arg2),
        SYSCALL_TIMERFD_SETTIME => syscall_timerfd_settime(arg1, arg2, arg3, arg4),
        SYSCALL_TIMERFD_GETTIME => syscall_timerfd_gettime(arg1, arg2),
        SYSCALL_EPOLL_CREATE1 => syscall_epoll_create1(arg1),
        SYSCALL_EPOLL_CTL => syscall_epoll_ctl(arg1, arg2, arg3, arg4),
        SYSCALL_EPOLL_WAIT => syscall_epoll_wait(arg1, arg2, arg3, arg4),
        SYSCALL_SOCKETPAIR => syscall_socketpair(arg1, arg2, arg3, arg4),
        SYSCALL_CLOCK_NANOSLEEP => syscall_clock_nanosleep(arg1, arg2, arg3, arg4),
        SYSCALL_GETPGRP => syscall_getpgrp(),
        SYSCALL_SETPGRP => syscall_setpgrp(arg1, arg2),
        SYSCALL_GETSID => syscall_getsid(arg1),
        SYSCALL_SETSID => syscall_setsid(),
        SYSCALL_SETPRIORITY => syscall_setpriority(arg1, arg2, arg3 as i64),
        SYSCALL_GETPRIORITY => syscall_getpriority(arg1, arg2),
        SYSCALL_SCHED_SETSCHEDULER => syscall_sched_setscheduler(arg1, arg2, arg3),
        SYSCALL_SCHED_GETSCHEDULER => syscall_sched_getscheduler(arg1),
        SYSCALL_SCHED_SETPARAM => syscall_sched_setparam(arg1, arg2),
        SYSCALL_SCHED_GETPARAM => syscall_sched_getparam(arg1, arg2),
        SYSCALL_SETUID => set_credential(CredentialField::Uid, arg1),
        SYSCALL_SETEUID => set_credential(CredentialField::Euid, arg1),
        SYSCALL_SETGID => set_credential(CredentialField::Gid, arg1),
        SYSCALL_SETEGID => set_credential(CredentialField::Egid, arg1),
        SYSCALL_CAPGET => syscall_capget(arg1, arg2),
        SYSCALL_CAPSET => syscall_capset(arg1, arg2),
        SYSCALL_SET_ROBUST_LIST => syscall_set_robust_list(arg1, arg2),
        SYSCALL_GET_ROBUST_LIST => syscall_get_robust_list(arg1, arg2, arg3),
        SYSCALL_PREAD => syscall_pread(arg1, arg2, arg3, arg4),
        SYSCALL_PWRITE => syscall_pwrite(arg1, arg2, arg3, arg4),
        SYSCALL_WRITEV => syscall_writev(arg1, arg2, arg3),
        SYSCALL_READV => syscall_readv(arg1, arg2, arg3),
        SYSCALL_SENDMSG | SYSCALL_RECVMSG => Err(ENOSYS),
        SYSCALL_SYSCONF => syscall_sysconf(arg1),
        SYSCALL_GETRLIMIT | SYSCALL_SETRLIMIT => syscall_rlimit(number, arg1, arg2),
        SYSCALL_GETRUSAGE => syscall_getrusage(arg1, arg2),
        SYSCALL_TIMES => syscall_times(arg1),
        _ => Err(ENOSYS),
    }
}

fn current_user_memory() -> Result<Arc<UserMemory>, i32> {
    let scheduler = USER_SCHEDULER.lock();
    scheduler
        .processes
        .get(scheduler.current)
        .filter(|_| scheduler.active)
        .map(|process| Arc::clone(&process.image.memory))
        .ok_or(EINTR)
}

fn syscall_execve(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    match execve_current(frame.rdi, frame.rsi, frame.rdx) {
        Ok(next_frame) => next_frame,
        Err(errno) => {
            frame.rax = (-(errno as i64)) as u64;
            frame_pointer
        }
    }
}

fn syscall_fork(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    match fork_current(frame) {
        Ok(pid) => {
            frame.rax = pid;
            frame_pointer
        }
        Err(errno) => {
            frame.rax = (-(errno as i64)) as u64;
            frame_pointer
        }
    }
}

fn syscall_thread_create(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let entry = frame.rdi;
    let argument = frame.rsi;
    let stack_top = frame.rdx;
    let thread_pointer = frame.r10;
    let mapping_base = frame.r8;
    let mapping_length = frame.r9;
    if paging::validate_user_executable(entry).is_err()
        || !thread_pointer::is_valid_user_thread_pointer(thread_pointer)
        || stack_top < 8
        || paging::validate_user_range(stack_top - 8, 8, true).is_err()
        || mapping_base % PAGE_SIZE != 0
        || mapping_length == 0
        || mapping_length % PAGE_SIZE != 0
        || validate_mapping_range(mapping_base, mapping_length).is_err()
        || usize::try_from(mapping_length)
            .ok()
            .is_none_or(|length| paging::validate_user_range(mapping_base, length, true).is_err())
        || stack_top <= mapping_base
        || stack_top > mapping_base.saturating_add(mapping_length)
        || (thread_pointer != 0
            && (thread_pointer < mapping_base
                || thread_pointer > mapping_base.saturating_add(mapping_length)))
    {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    if !scheduler.active || current >= scheduler.processes.len() {
        frame.rax = (-(EINTR as i64)) as u64;
        return frame_pointer;
    }
    if scheduler.processes.len() >= MAX_SCHEDULED_PROCESSES {
        frame.rax = (-(ENOMEM as i64)) as u64;
        return frame_pointer;
    }
    if scheduler.processes.try_reserve(1).is_err() {
        frame.rax = (-(ENOMEM as i64)) as u64;
        return frame_pointer;
    }

    let pid = {
        let process = USER_PROCESS.lock();
        if !process.active {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
        process.pid
    };
    let memory = Arc::clone(&scheduler.processes[current].image.memory);
    {
        let mappings = memory.mmap_pages.lock();
        let end = mapping_base + mapping_length;
        let mut page = mapping_base;
        while page < end {
            if !mappings
                .iter()
                .any(|mapping| mapping.address == page && mapping.anonymous)
            {
                frame.rax = (-(EINVAL as i64)) as u64;
                return frame_pointer;
            }
            page += PAGE_SIZE;
        }
    }
    let tid = scheduler.next_tid;
    let Some(next_tid) = tid.checked_add(1) else {
        frame.rax = (-(ENOMEM as i64)) as u64;
        return frame_pointer;
    };
    let image = match scheduler.processes[current].image.share_for_thread() {
        Ok(image) => image,
        Err(error) => {
            frame.rax = (-(errno_from_elf(error) as i64)) as u64;
            return frame_pointer;
        }
    };
    let mut state = match fork_clone_process_state(&USER_PROCESS.lock(), pid) {
        Ok(state) => state,
        Err(error) => {
            frame.rax = (-(error as i64)) as u64;
            return frame_pointer;
        }
    };
    state.fpu_state = FpuState::new();
    state.thread_pointer = thread_pointer;
    let kernel_stack = match ProcessKernelStack::new() {
        Ok(stack) => stack,
        Err(error) => {
            frame.rax = (-(errno_from_elf(error) as i64)) as u64;
            return frame_pointer;
        }
    };
    let kernel_stack_top = kernel_stack.top();
    let thread_frame = match initialize_thread_frame(entry, argument, stack_top, kernel_stack_top) {
        Ok(frame) => frame,
        Err(error) => {
            frame.rax = (-(error as i64)) as u64;
            return frame_pointer;
        }
    };

    let inherited_priority = scheduler.processes[current].priority;
    let inherited_policy = scheduler.processes[current].policy;
    let inherited_rt_priority = scheduler.processes[current].rt_priority;
    let inherited_credentials = {
        let process = &scheduler.processes[current];
        (
            process.uid,
            process.euid,
            process.gid,
            process.egid,
            process.capabilities,
        )
    };
    scheduler.next_tid = next_tid;
    scheduler.processes.push(ScheduledProcess {
        image,
        state,
        pid,
        _kernel_stack: Some(kernel_stack),
        kernel_stack_top,
        frame_pointer: thread_frame,
        exit_status: None,
        parent_pid: None,
        is_thread: true,
        tid,
        thread_mapping: Some((mapping_base, mapping_length)),
        waiting_for: None,
        wait_deadline: None,
        resume_context: None,
        wait_status_pointer: 0,
        priority: inherited_priority,
        policy: inherited_policy,
        rt_priority: inherited_rt_priority,
        rt_slice: RT_RR_QUANTUM_TICKS,
        robust_list: 0,
        robust_list_length: 0,
        uid: inherited_credentials.0,
        euid: inherited_credentials.1,
        gid: inherited_credentials.2,
        egid: inherited_credentials.3,
        capabilities: inherited_credentials.4,
        age: 0,
        runtime: TaskRuntime::default(),
        reaped: false,
    });
    frame.rax = tid;
    frame_pointer
}

fn initialize_thread_frame(
    entry: u64,
    argument: u64,
    stack_top: u64,
    kernel_stack_top: u64,
) -> Result<usize, i32> {
    let user_stack = stack_top.checked_sub(8).ok_or(EFAULT)?;
    paging::validate_user_range(user_stack, 8, true).map_err(|_| EFAULT)?;
    unsafe { (user_stack as *mut u64).write_unaligned(0) };
    let frame_pointer = (kernel_stack_top as usize - core::mem::size_of::<InterruptFrame>()) & !0xF;
    let frame = InterruptFrame {
        r15: 0,
        r14: 0,
        r13: 0,
        r12: 0,
        r11: 0,
        r10: 0,
        r9: 0,
        r8: 0,
        rbp: 0,
        rdi: argument,
        rsi: 0,
        rdx: 0,
        rcx: 0,
        rbx: 0,
        rax: 0,
        vector: u64::from(interrupt::TIMER_VECTOR),
        error_code: 0,
        stack_frame: InterruptStackFrame {
            instruction_pointer: entry,
            code_segment: u64::from(gdt::USER_CODE_SELECTOR),
            cpu_flags: 0x202,
            stack_pointer: user_stack,
            stack_segment: u64::from(gdt::USER_DATA_SELECTOR),
        },
    };
    unsafe { (frame_pointer as *mut InterruptFrame).write(frame) };
    Ok(frame_pointer)
}

fn syscall_thread_join(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let target_tid = frame.rdi;
    let result_pointer = frame.rsi;
    if target_tid == 0
        || (result_pointer != 0 && paging::validate_user_range(result_pointer, 8, true).is_err())
    {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    let pid = {
        let process = USER_PROCESS.lock();
        if !scheduler.active || !process.active || current >= scheduler.processes.len() {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
        process.pid
    };
    if scheduler.processes[current].tid == target_tid {
        frame.rax = (-(EDEADLK as i64)) as u64;
        return frame_pointer;
    }
    let Some(target) = scheduler.processes.iter().position(|process| {
        process.is_thread && process.pid == pid && process.tid == target_tid && !process.reaped
    }) else {
        frame.rax = (-(ESRCH as i64)) as u64;
        return frame_pointer;
    };
    if let Some(status) = scheduler.processes[target].exit_status {
        if result_pointer != 0
            && let Err(error) = write_user_u64(result_pointer, status)
        {
            frame.rax = (-(error as i64)) as u64;
            return frame_pointer;
        }
        scheduler.processes[target].reaped = true;
        frame.rax = 0;
        return frame_pointer;
    }

    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    scheduler.processes[current].waiting_for = Some(ProcessWait::Thread(target_tid));
    scheduler.processes[current].wait_status_pointer = result_pointer;
    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    let Some(next) = schedule_next(&mut scheduler) else {
        core::mem::swap(
            &mut *active_process,
            &mut scheduler.processes[current].state,
        );
        scheduler.processes[current].waiting_for = None;
        scheduler.processes[current].wait_status_pointer = 0;
        frame.rax = (-(EDEADLK as i64)) as u64;
        return frame_pointer;
    };
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.memory.address_space.activate();
    }
    next_frame
}

fn syscall_thread_exit(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    if !scheduler.active
        || current >= scheduler.processes.len()
        || !scheduler.processes[current].is_thread
    {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    }

    let tid = scheduler.processes[current].tid;
    let memory = Arc::clone(&scheduler.processes[current].image.memory);
    if let Some((base, length)) = scheduler.processes[current].thread_mapping.take()
        && let Err(error) = unmap_mapping_range(&mut memory.mmap_pages.lock(), base, length)
    {
        crate::println!("thread {tid}: failed to release thread mapping: errno={error}");
    }
    scheduler.processes[current].exit_status = Some(frame.rdi);
    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    scheduler.processes[current].state.active = false;

    for waiter in &mut scheduler.processes {
        if waiter.waiting_for != Some(ProcessWait::Thread(tid)) {
            continue;
        }
        let result_pointer = waiter.wait_status_pointer;
        waiter.waiting_for = None;
        waiter.wait_status_pointer = 0;
        let waiter_frame = waiter.frame_pointer as *mut InterruptFrame;
        unsafe {
            (*waiter_frame).rax = if result_pointer == 0 {
                0
            } else {
                match write_user_u64(result_pointer, frame.rdi) {
                    Ok(()) => 0,
                    Err(error) => (-(error as i64)) as u64,
                }
            };
        }
        scheduler.processes[current].reaped = true;
        break;
    }

    if let Some(next) = schedule_next(&mut scheduler) {
        core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
        scheduler.current = next;
        let next_process = &scheduler.processes[next];
        let next_frame = next_process.frame_pointer as *mut InterruptFrame;
        unsafe {
            gdt::set_ring0_stack_top(next_process.kernel_stack_top);
            next_process.image.memory.address_space.activate();
        }
        return next_frame;
    }

    scheduler.active = scheduler
        .processes
        .iter()
        .any(|process| process.exit_status.is_none());
    drop(active_process);
    drop(scheduler);
    gdt::record_user_exit(frame.rdi);
    frame.stack_frame.instruction_pointer = gdt::user_exit_address();
    frame.stack_frame.code_segment = gdt::KERNEL_CODE_SELECTOR as u64;
    frame.stack_frame.stack_pointer = gdt::user_kernel_stack_top();
    frame.stack_frame.stack_segment = gdt::KERNEL_DATA_SELECTOR as u64;
    frame_pointer
}

fn fork_current(frame: &InterruptFrame) -> Result<u64, i32> {
    let parent_pid = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        process.pid
    };
    let _restore_address_space = RestoreUserAddressSpace(parent_pid);
    let (child_pid, next_pid) = {
        let mut scheduler = USER_SCHEDULER.lock();
        if !scheduler.active
            || scheduler
                .processes
                .get(scheduler.current)
                .is_none_or(|process| process.pid != parent_pid)
        {
            return Err(EINTR);
        }
        reap_completed_processes(&mut scheduler)?;
        if scheduler.processes.len() >= MAX_SCHEDULED_PROCESSES {
            return Err(ENOMEM);
        }
        scheduler.processes.try_reserve(1).map_err(|_| ENOMEM)?;
        let pid = scheduler.next_pid;
        let next_pid = pid.checked_add(1).ok_or(ENOMEM)?;
        (pid, next_pid)
    };

    let mut child_state = {
        let process = USER_PROCESS.lock();
        fork_clone_process_state(&process, child_pid)?
    };
    let child_image = {
        let scheduler = USER_SCHEDULER.lock();
        let parent = scheduler
            .processes
            .iter()
            .find(|process| process.pid == parent_pid)
            .ok_or(EINTR)?;
        let mut user_pages = Vec::new();
        let mapped_pages = parent.image.memory.mapped_pages.lock();
        let mmap_pages = parent.image.memory.mmap_pages.lock();
        let page_count = mapped_pages
            .len()
            .checked_add(mmap_pages.len())
            .ok_or(ENOMEM)?;
        user_pages
            .try_reserve_exact(page_count)
            .map_err(|_| ENOMEM)?;
        user_pages.extend_from_slice(&mapped_pages);
        user_pages.extend(mmap_pages.iter().map(|page| page.address));
        let shared_pages: Vec<u64> = mmap_pages
            .iter()
            .filter(|page| page.shared_backing.is_some() || page.is_device)
            .map(|page| page.address)
            .collect();
        drop(mmap_pages);
        drop(mapped_pages);
        parent
            .image
            .fork_clone(&user_pages, &shared_pages)
            .map_err(errno_from_elf)?
    };
    let kernel_stack = match ProcessKernelStack::new() {
        Ok(stack) => stack,
        Err(error) => {
            return Err(cleanup_failed_fork(
                child_image,
                child_state,
                errno_from_elf(error),
            ));
        }
    };
    let kernel_stack_top = kernel_stack.top();
    let child_frame = initialize_fork_frame(frame, kernel_stack_top);
    child_state.pid = child_pid;

    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    if !scheduler.active
        || scheduler
            .processes
            .get(current)
            .is_none_or(|process| process.pid != parent_pid)
    {
        drop(scheduler);
        return Err(cleanup_failed_fork(child_image, child_state, EINTR));
    }
    {
        let process = USER_PROCESS.lock();
        if !process.active || process.pid != parent_pid {
            drop(process);
            drop(scheduler);
            return Err(cleanup_failed_fork(child_image, child_state, EINTR));
        }
    }
    let inherited_priority = scheduler.processes[current].priority;
    let inherited_policy = scheduler.processes[current].policy;
    let inherited_rt_priority = scheduler.processes[current].rt_priority;
    let inherited_credentials = {
        let process = &scheduler.processes[current];
        (
            process.uid,
            process.euid,
            process.gid,
            process.egid,
            process.capabilities,
        )
    };
    scheduler.next_pid = next_pid;
    scheduler.processes.push(ScheduledProcess {
        image: child_image,
        state: child_state,
        pid: child_pid,
        _kernel_stack: Some(kernel_stack),
        kernel_stack_top,
        frame_pointer: child_frame,
        exit_status: None,
        parent_pid: Some(parent_pid),
        is_thread: false,
        tid: child_pid,
        thread_mapping: None,
        waiting_for: None,
        wait_deadline: None,
        resume_context: None,
        wait_status_pointer: 0,
        priority: inherited_priority,
        policy: inherited_policy,
        rt_priority: inherited_rt_priority,
        rt_slice: RT_RR_QUANTUM_TICKS,
        robust_list: 0,
        robust_list_length: 0,
        uid: inherited_credentials.0,
        euid: inherited_credentials.1,
        gid: inherited_credentials.2,
        egid: inherited_credentials.3,
        capabilities: inherited_credentials.4,
        age: 0,
        runtime: TaskRuntime::default(),
        reaped: false,
    });
    Ok(child_pid)
}

fn execve_current(
    path_address: u64,
    arguments_address: u64,
    environment_address: u64,
) -> Result<*mut InterruptFrame, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    let arguments = copy_user_argument_vector(arguments_address)?;
    let environment = copy_user_environment(environment_address)
        .map_err(|error| if error == ENAMETOOLONG { E2BIG } else { error })?;
    let (cwd, terminal, pid, creation_mask, process_group, session,
         descriptors, descriptor_flags) = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        if process.descriptors.len() != process.descriptor_flags.len() {
            return Err(EIO);
        }
        let mut descriptors = Vec::new();
        descriptors
            .try_reserve_exact(process.descriptors.len())
            .map_err(|_| ENOMEM)?;
        descriptors.extend(
            process
                .descriptors
                .iter()
                .zip(&process.descriptor_flags)
                .map(|(descriptor, flags)| {
                    if flags & FD_CLOEXEC != 0 {
                        None
                    } else {
                        descriptor.clone()
                    }
                }),
        );
        let mut descriptor_flags = Vec::new();
        descriptor_flags
            .try_reserve_exact(descriptors.len())
            .map_err(|_| ENOMEM)?;
        descriptor_flags.resize(descriptors.len(), 0);
        (
            process.cwd.clone(),
            process.terminal,
            process.pid,
            process.file_creation_mask,
            process.process_group,
            process.session,
            descriptors,
            descriptor_flags,
        )
    };

    let _restore_address_space = RestoreUserAddressSpace(pid);
    if USER_SCHEDULER
        .lock()
        .processes
        .iter()
        .any(|process| process.pid == pid && process.is_thread && process.exit_status.is_none())
    {
        return Err(EBUSY);
    }
    let bytes = read_executable_file(&path).map_err(errno_from_elf)?;
    {
        let scheduler = USER_SCHEDULER.lock();
        if !scheduler.active
            || scheduler
                .processes
                .get(scheduler.current)
                .is_none_or(|process| process.pid != pid)
        {
            return Err(EINTR);
        }
        unsafe {
            scheduler.processes[scheduler.current]
                .image
                .memory
                .address_space
                .activate_kernel();
        }
    }
    let image =
        load_with_args_and_environment(&bytes, &arguments, &environment).map_err(errno_from_elf)?;
    let mut state = match new_user_process(&cwd, pid, terminal, environment) {
        Ok(state) => state,
        Err(error) => {
            return Err(cleanup_failed_exec(image, errno_from_elf(error)));
        }
    };
    state.process_group = process_group;
    state.session = session;
    state.descriptors = descriptors;
    state.descriptor_flags = descriptor_flags;
    state.file_creation_mask = creation_mask;

    let kernel_stack = match ProcessKernelStack::new() {
        Ok(stack) => stack,
        Err(error) => {
            return Err(cleanup_failed_exec(image, errno_from_elf(error)));
        }
    };
    let kernel_stack_top = kernel_stack.top();
    let frame_pointer = initialize_user_frame(&image, kernel_stack_top);
    let old_image = {
        let mut scheduler = USER_SCHEDULER.lock();
        let current = scheduler.current;
        if !scheduler.active
            || scheduler
                .processes
                .get(current)
                .is_none_or(|process| process.pid != pid)
        {
            drop(scheduler);
            return Err(cleanup_failed_exec(image, EINTR));
        }
        if scheduler.processes[current]._kernel_stack.is_some()
            && scheduler.retired_kernel_stacks.try_reserve(1).is_err()
        {
            drop(scheduler);
            return Err(cleanup_failed_exec(image, ENOMEM));
        }

        let mut active_process = USER_PROCESS.lock();
        if !active_process.active || active_process.pid != pid {
            drop(active_process);
            drop(scheduler);
            return Err(cleanup_failed_exec(image, EINTR));
        }

        let (old_image, old_kernel_stack) = {
            let scheduled = &mut scheduler.processes[current];
            let old_image = core::mem::replace(&mut scheduled.image, image);
            let old_kernel_stack =
                core::mem::replace(&mut scheduled._kernel_stack, Some(kernel_stack));
            scheduled.state = UserProcess::new();
            scheduled.kernel_stack_top = kernel_stack_top;
            scheduled.frame_pointer = frame_pointer;
            scheduled.waiting_for = None;
            scheduled.wait_status_pointer = 0;
            (old_image, old_kernel_stack)
        };
        if let Some(old_kernel_stack) = old_kernel_stack {
            scheduler.retired_kernel_stacks.push(old_kernel_stack);
        }
        *active_process = state;
        old_image
    };

    unsafe { gdt::set_ring0_stack_top(kernel_stack_top) };
    if let Err(error) = old_image.unload() {
        crate::println!("execve: failed to release old process image: {error:?}");
    }
    Ok(frame_pointer as *mut InterruptFrame)
}

fn reap_completed_processes(scheduler: &mut UserScheduler) -> Result<(), i32> {
    let mut index = 0;
    while index < scheduler.processes.len() {
        let process = &scheduler.processes[index];
        if index == scheduler.current || !process.reaped || process.exit_status.is_none() {
            index += 1;
            continue;
        }

        let process = scheduler.processes.remove(index);
        if index < scheduler.current {
            scheduler.current -= 1;
        }
        process.image.unload().map_err(|_| EIO)?;
    }
    Ok(())
}

fn errno_from_elf(error: ElfError) -> i32 {
    match error {
        ElfError::FileSystem(error) => errno_from_fs(error),
        ElfError::OutOfMemory | ElfError::Paging(PagingError::OutOfFrames) => ENOMEM,
        ElfError::PermissionDenied => EACCES,
        ElfError::InvalidArguments => EINVAL,
        ElfError::ArgumentsTooLarge => E2BIG,
        ElfError::ProcessAlreadyRunning => EINTR,
        _ => ENOEXEC,
    }
}

fn syscall_waitpid(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let wait_target = frame.rdi;
    let status_pointer = frame.rsi;
    let options = frame.rdx;
    if wait_target == 0 || options & !WAIT_NOHANG != 0 {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    }
    if status_pointer != 0
        && paging::validate_user_range(status_pointer, core::mem::size_of::<u64>(), true).is_err()
    {
        frame.rax = (-(EFAULT as i64)) as u64;
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    let parent_pid = {
        let process = USER_PROCESS.lock();
        if !scheduler.active || !process.active || current >= scheduler.processes.len() {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
        process.pid
    };
    let Some(child) = scheduler.processes.iter().position(|process| {
        !process.is_thread
            && process.parent_pid == Some(parent_pid)
            && !process.reaped
            && wait_target_matches(wait_target, process.pid)
    }) else {
        frame.rax = (-(ECHILD as i64)) as u64;
        return frame_pointer;
    };
    let child_pid = scheduler.processes[child].pid;

    if let Some(status) = scheduler.processes[child].exit_status {
        if status_pointer != 0
            && let Err(error) = write_user_u64(status_pointer, status)
        {
            frame.rax = (-(error as i64)) as u64;
            return frame_pointer;
        }
        scheduler.processes[child].reaped = true;
        frame.rax = child_pid;
        return frame_pointer;
    }

    if options & WAIT_NOHANG != 0 {
        frame.rax = 0;
        return frame_pointer;
    }

    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    scheduler.processes[current].waiting_for = Some(ProcessWait::Child(wait_target));
    scheduler.processes[current].wait_status_pointer = status_pointer;
    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    let Some(next) = schedule_next(&mut scheduler) else {
        core::mem::swap(
            &mut *active_process,
            &mut scheduler.processes[current].state,
        );
        scheduler.processes[current].waiting_for = None;
        scheduler.processes[current].wait_status_pointer = 0;
        frame.rax = (-(ECHILD as i64)) as u64;
        return frame_pointer;
    };
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.memory.address_space.activate();
    }
    next_frame
}

fn wait_target_matches(target: u64, child_pid: u64) -> bool {
    target == WAIT_ANY_PID || target == child_pid
}

fn write_user_u64(address: u64, value: u64) -> Result<(), i32> {
    paging::validate_user_range(address, core::mem::size_of::<u64>(), true).map_err(|_| EFAULT)?;
    unsafe { (address as *mut u64).write_unaligned(value) };
    Ok(())
}

fn pipe_wait_target(fd: u64, reading: bool) -> Option<(u64, ProcessWait)> {
    let fd = usize::try_from(fd).ok()?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return None;
    }
    if process
        .descriptor_flags
        .get(fd)
        .is_some_and(|flags| flags & O_NONBLOCK as u32 != 0)
    {
        return None;
    }
    let Some(UserDescriptor::Pipe(endpoint)) = process.descriptors.get(fd).and_then(Option::as_ref)
    else {
        return None;
    };
    let pipe_id = endpoint.pipe.lock().id;
    let wait = match (reading, endpoint.readable, endpoint.writable) {
        (true, true, _) => ProcessWait::PipeReadable(pipe_id),
        (false, _, true) => ProcessWait::PipeWritable(pipe_id),
        _ => return None,
    };
    Some((pipe_id, wait))
}

fn console_wait_target(fd: u64) -> Option<usize> {
    let fd = usize::try_from(fd).ok()?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return None;
    }
    match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::ConsoleInput) => Some(process.terminal),
        // `/dev/stdin`, `/dev/tty*`, dan `/dev/console` juga menunggu input
        // console, bukan menahan CPU di jalur VFS.
        Some(UserDescriptor::File(file)) => fs::tty_path_terminal(&file.lock().path),
        _ => None,
    }
}

fn block_current_on_wait(frame: &mut InterruptFrame, wait: ProcessWait) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let Some(retry_address) = frame.stack_frame.instruction_pointer.checked_sub(2) else {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    };
    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    {
        let process = USER_PROCESS.lock();
        if !scheduler.active || !process.active || current >= scheduler.processes.len() {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
    }

    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    scheduler.processes[current].waiting_for = Some(wait);
    // Wait ini tidak punya batas waktu; batalkan deadline lama agar tidak
    // terbangun sebagai "timeout" palsu.
    scheduler.processes[current].wait_deadline = None;
    // Ulangi instruksi int 0x80 setelah event yang ditunggu siap.
    frame.stack_frame.instruction_pointer = retry_address;
    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    let Some(next) = schedule_next(&mut scheduler) else {
        core::mem::swap(
            &mut *active_process,
            &mut scheduler.processes[current].state,
        );
        scheduler.processes[current].waiting_for = None;
        frame.stack_frame.instruction_pointer = retry_address + 2;
        if let ProcessWait::ConsoleInput(terminal) = wait {
            // Tidak ada proses lain: baca console secara blocking di kernel.
            drop(active_process);
            drop(scheduler);
            let address = frame.rsi;
            let length = frame.rdx as usize;
            let result = paging::validate_user_range(address, length, true)
                .map_err(|_| EFAULT)
                .and_then(|()| {
                    let buffer = unsafe {
                        core::slice::from_raw_parts_mut(address as *mut u8, length)
                    };
                    fs::read_tty(terminal, buffer)
                        .map(|count| count as u64)
                        .map_err(errno_from_fs)
                });
            frame.rax = match result {
                Ok(value) => value,
                Err(errno) => (-(errno as i64)) as u64,
            };
            return frame_pointer;
        }
        frame.rax = (-(EDEADLK as i64)) as u64;
        return frame_pointer;
    };
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.memory.address_space.activate();
    }
    next_frame
}

fn expire_futex_timeouts(scheduler: &mut UserScheduler) {
    let now = crate::arch::x86_64::monotonic_milliseconds();
    for process in &mut scheduler.processes {
        let Some(deadline) = process.wait_deadline else {
            continue;
        };
        if now < deadline {
            continue;
        }
        match process.waiting_for {
            Some(ProcessWait::Futex { .. }) => {
                process.waiting_for = None;
                process.state.wait_result = Some(ETIMEDOUT);
            }
            Some(ProcessWait::PollWait) => process.waiting_for = None,
            Some(ProcessWait::Sleep) => process.waiting_for = None,
            _ => {}
        }
        process.wait_deadline = None;
    }
}

const FUTEX_WAIT: u64 = 0;
const FUTEX_WAKE: u64 = 1;
const FUTEX_REQUEUE: u64 = 3;
const FUTEX_WAIT_BITSET: u64 = 9;
const FUTEX_WAKE_BITSET: u64 = 10;
const FUTEX_BITSET_MATCH_ANY: u32 = u32::MAX;
const FUTEX_PRIVATE_FLAG: u64 = 128;
const FUTEX_CLOCK_REALTIME: u64 = 256;
const FUTEX_OPERATION_MASK: u64 = !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);
const FUTEX_WAITERS: u32 = 0x8000_0000;
const FUTEX_OWNER_DIED: u32 = 0x4000_0000;
const FUTEX_TID_MASK: u32 = 0x3FFF_FFFF;
/// `struct robust_list_head`: `list.next`, `futex_offset` (i64), `list_op_pending`.
const ROBUST_LIST_HEAD_LENGTH: u64 = 24;
const MAX_ROBUST_LIST_ENTRIES: usize = 64;

fn futex_deadline(timeout: u64) -> Result<Option<u64>, i32> {
    if timeout == 0 {
        return Ok(None);
    }
    paging::validate_user_range(timeout, core::mem::size_of::<UserTimespec>(), false)
        .map_err(|_| EFAULT)?;
    let ts = unsafe { (timeout as *const UserTimespec).read_unaligned() };
    if ts.seconds < 0 || ts.nanoseconds < 0 || ts.nanoseconds >= 1_000_000_000 {
        return Err(EINVAL);
    }
    let milliseconds = (ts.seconds as u64)
        .saturating_mul(1000)
        .saturating_add((ts.nanoseconds / 1_000_000) as u64);
    if milliseconds == 0 {
        return Ok(Some(0));
    }
    Ok(Some(
        crate::arch::x86_64::monotonic_milliseconds().saturating_add(milliseconds),
    ))
}

/// Deadline absolut (WAIT_BITSET): `timeout` berisi `struct timespec` absolut.
fn futex_absolute_deadline(timeout: u64, realtime: bool) -> Result<Option<u64>, i32> {
    if timeout == 0 {
        return Ok(None);
    }
    paging::validate_user_range(timeout, core::mem::size_of::<UserTimespec>(), false)
        .map_err(|_| EFAULT)?;
    let ts = unsafe { (timeout as *const UserTimespec).read_unaligned() };
    if ts.seconds < 0 || ts.nanoseconds < 0 || ts.nanoseconds >= 1_000_000_000 {
        return Err(EINVAL);
    }
    let target = (ts.seconds as u64)
        .saturating_mul(1000)
        .saturating_add((ts.nanoseconds / 1_000_000) as u64);
    let now = if realtime {
        (crate::drivers::rtc::unix_seconds().map_err(|_| EIO)? as u64).saturating_mul(1000)
    } else {
        crate::arch::x86_64::monotonic_milliseconds()
    };
    if target <= now {
        return Ok(Some(0));
    }
    Ok(Some(
        crate::arch::x86_64::monotonic_milliseconds()
            .saturating_add(target.saturating_sub(now)),
    ))
}

/// Bit yang diminta pembangunkan harus beririsan dengan bit tunggu.
fn futex_bitset_matches(waiting: u32, wake: u32) -> bool {
    waiting & wake != 0
}

fn block_current_on_futex(
    frame: &mut InterruptFrame,
    address: u64,
    bitset: u32,
    deadline: Option<u64>,
) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let Some(retry_address) = frame.stack_frame.instruction_pointer.checked_sub(2) else {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    };
    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    {
        let process = USER_PROCESS.lock();
        if !scheduler.active || !process.active || current >= scheduler.processes.len() {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
    }
    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    scheduler.processes[current].waiting_for = Some(ProcessWait::Futex { address, bitset });
    scheduler.processes[current].wait_deadline = deadline;
    frame.stack_frame.instruction_pointer = retry_address;
    let mut active_process = USER_PROCESS.lock();
    if let Some(next) = schedule_next(&mut scheduler) {
        core::mem::swap(
            &mut *active_process,
            &mut scheduler.processes[current].state,
        );
        core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
        scheduler.current = next;
        let next_process = &scheduler.processes[next];
        let next_frame = next_process.frame_pointer as *mut InterruptFrame;
        unsafe {
            gdt::set_ring0_stack_top(next_process.kernel_stack_top);
            next_process.image.memory.address_space.activate();
        }
        return next_frame;
    }

    let Some(deadline) = deadline else {
        scheduler.processes[current].waiting_for = None;
        scheduler.processes[current].wait_deadline = None;
        frame.stack_frame.instruction_pointer = retry_address + 2;
        frame.rax = (-(EDEADLK as i64)) as u64;
        return frame_pointer;
    };
    // Tidak ada thread siap. State proses sengaja tidak di-swap keluar, jadi
    // USER_PROCESS selalu memegang state yang hidup selama tidur. Bangun tiap
    // 1 ms untuk memeriksa deadline dan menjadwalkan thread yang menjadi siap.
    drop(active_process);
    drop(scheduler);
    loop {
        sleep_milliseconds(1);
        let mut scheduler = USER_SCHEDULER.lock();
        let now = crate::arch::x86_64::monotonic_milliseconds();
        if now >= deadline || scheduler.processes[current].waiting_for.is_none() {
            let timed_out = now >= deadline;
            scheduler.processes[current].waiting_for = None;
            scheduler.processes[current].wait_deadline = None;
            drop(scheduler);
            if timed_out {
                frame.stack_frame.instruction_pointer = retry_address + 2;
                frame.rax = (-(ETIMEDOUT as i64)) as u64;
            } else {
                frame.stack_frame.instruction_pointer = retry_address;
            }
            return frame_pointer;
        }
        let Some(next) = schedule_next(&mut scheduler) else {
            continue;
        };
        let mut active = USER_PROCESS.lock();
        core::mem::swap(
            &mut *active,
            &mut scheduler.processes[current].state,
        );
        core::mem::swap(&mut *active, &mut scheduler.processes[next].state);
        scheduler.current = next;
        let next_process = &scheduler.processes[next];
        let next_frame = next_process.frame_pointer as *mut InterruptFrame;
        unsafe {
            gdt::set_ring0_stack_top(next_process.kernel_stack_top);
            next_process.image.memory.address_space.activate();
        }
        return next_frame;
    }
}

fn block_current_on_poll(frame: &mut InterruptFrame, deadline: u64) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let Some(retry_address) = frame.stack_frame.instruction_pointer.checked_sub(2) else {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    };
    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    {
        let process = USER_PROCESS.lock();
        if !scheduler.active || !process.active || current >= scheduler.processes.len() {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
    }
    scheduler.processes[current].frame_pointer = frame_pointer as usize;
    scheduler.processes[current].waiting_for = Some(ProcessWait::PollWait);
    scheduler.processes[current].wait_deadline = Some(deadline);
    frame.stack_frame.instruction_pointer = retry_address;
    let mut active_process = USER_PROCESS.lock();
    core::mem::swap(
        &mut *active_process,
        &mut scheduler.processes[current].state,
    );
    let Some(next) = schedule_next(&mut scheduler) else {
        core::mem::swap(
            &mut *active_process,
            &mut scheduler.processes[current].state,
        );
        scheduler.processes[current].waiting_for = None;
        scheduler.processes[current].wait_deadline = None;
        // Tidak ada proses lain: tidur sebentar lalu ulangi scan.
        drop(active_process);
        drop(scheduler);
        sleep_milliseconds(1);
        frame.stack_frame.instruction_pointer = retry_address;
        return frame_pointer;
    };
    core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
    scheduler.current = next;
    let next_process = &scheduler.processes[next];
    let next_frame = next_process.frame_pointer as *mut InterruptFrame;
    unsafe {
        gdt::set_ring0_stack_top(next_process.kernel_stack_top);
        next_process.image.memory.address_space.activate();
    }
    next_frame
}

/// Bangunkan waiter futex pada `address` yang bit tunggunya beririsan.
fn wake_futex_waiters(pid: u64, address: u64, wake_bits: u32, limit: u64) -> u64 {
    let mut scheduler = USER_SCHEDULER.lock();
    wake_futex_waiters_in_scheduler(&mut scheduler, pid, address, wake_bits, limit)
}

fn wake_futex_waiters_in_scheduler(
    scheduler: &mut UserScheduler,
    pid: u64,
    address: u64,
    wake_bits: u32,
    limit: u64,
) -> u64 {
    let mut woken = 0u64;
    for waiting in &mut scheduler.processes {
        if woken >= limit {
            break;
        }
        if waiting.pid != pid {
            continue;
        }
        let Some(ProcessWait::Futex { address: waiting_address, bitset }) = waiting.waiting_for
        else {
            continue;
        };
        if waiting_address == address && futex_bitset_matches(bitset, wake_bits) {
            waiting.waiting_for = None;
            waiting.wait_deadline = None;
            waiting.state.wait_result = Some(0);
            woken += 1;
        }
    }
    woken
}

/// Kata futex milik `tid` setelah pemiliknya keluar: TID dilepas dan
/// `FUTEX_OWNER_DIED` diset, sementara bit `FUTEX_WAITERS` dipertahankan.
/// `None` berarti futex itu bukan milik thread yang keluar.
fn robust_word_after_exit(word: u32, tid: u32) -> Option<u32> {
    if tid == 0 || word & FUTEX_TID_MASK != tid & FUTEX_TID_MASK {
        return None;
    }
    Some((word & FUTEX_WAITERS) | FUTEX_OWNER_DIED)
}

/// Telusuri `robust_list` task yang sedang keluar dan tandai futex miliknya,
/// lalu bangunkan waiter-nya. Dipanggil dengan USER_SCHEDULER terkunci dan
/// address space task yang keluar masih aktif.
fn release_robust_list(scheduler: &mut UserScheduler, index: usize) {
    let (head, length, tid, pid) = {
        let process = &scheduler.processes[index];
        (
            process.robust_list,
            process.robust_list_length,
            process.tid as u32,
            process.pid,
        )
    };
    if head == 0 || length != ROBUST_LIST_HEAD_LENGTH {
        return;
    }
    if paging::validate_user_range(head, ROBUST_LIST_HEAD_LENGTH as usize, false).is_err() {
        return;
    }
    let next_head = unsafe { (head as *const u64).read_unaligned() };
    let futex_offset = unsafe { ((head + 8) as *const i64).read_unaligned() };
    let pending = unsafe { ((head + 16) as *const u64).read_unaligned() };
    let mut entry = next_head;
    for _ in 0..MAX_ROBUST_LIST_ENTRIES {
        if entry == 0 || entry == head {
            return;
        }
        if paging::validate_user_range(entry, core::mem::size_of::<u64>(), false).is_err() {
            return;
        }
        let next_entry = unsafe { (entry as *const u64).read_unaligned() };
        if entry != pending {
            let address = (entry as i64)
                .checked_add(futex_offset)
                .and_then(|address| u64::try_from(address).ok());
            if let Some(address) = address
                && paging::validate_user_range(address, core::mem::size_of::<u32>(), true).is_ok()
            {
                let word = unsafe { (address as *const u32).read_unaligned() };
                if let Some(new_word) = robust_word_after_exit(word, tid) {
                    unsafe { (address as *mut u32).write_unaligned(new_word) };
                    wake_futex_waiters_in_scheduler(
                        scheduler,
                        pid,
                        address,
                        FUTEX_BITSET_MATCH_ANY,
                        u64::MAX,
                    );
                }
            }
        }
        entry = next_entry;
    }
}

fn syscall_set_robust_list(head: u64, length: u64) -> Result<u64, i32> {
    if length != ROBUST_LIST_HEAD_LENGTH {
        return Err(EINVAL);
    }
    if head != 0 {
        paging::validate_user_range(head, ROBUST_LIST_HEAD_LENGTH as usize, true)
            .map_err(|_| EFAULT)?;
    }
    let mut scheduler = USER_SCHEDULER.lock();
    let current = scheduler.current;
    let process = scheduler.processes.get_mut(current).ok_or(EINTR)?;
    process.robust_list = head;
    process.robust_list_length = length;
    Ok(0)
}

fn syscall_get_robust_list(
    pid: u64,
    head_address: u64,
    length_address: u64,
) -> Result<u64, i32> {
    let scheduler = USER_SCHEDULER.lock();
    let pid = if pid == 0 {
        scheduler
            .processes
            .get(scheduler.current)
            .map(|process| process.tid)
            .ok_or(EINTR)?
    } else {
        pid
    };
    let Some(process) = scheduler.processes.iter().find(|process| process.tid == pid) else {
        return Err(ESRCH);
    };
    let (head, length) = (process.robust_list, process.robust_list_length);
    drop(scheduler);
    write_user_u64(head_address, head)?;
    write_user_u64(length_address, length)?;
    Ok(0)
}

/// Bangunkan `wake_limit` waiter lalu pindahkan sisanya ke `target`.
fn requeue_futex_waiters(
    pid: u64,
    address: u64,
    target: u64,
    wake_limit: u64,
    requeue_limit: u64,
) -> u64 {
    let mut scheduler = USER_SCHEDULER.lock();
    let mut woken = 0u64;
    let mut requeued = 0u64;
    for waiting in &mut scheduler.processes {
        if waiting.pid != pid {
            continue;
        }
        let Some(ProcessWait::Futex { address: waiting_address, bitset }) = waiting.waiting_for
        else {
            continue;
        };
        if waiting_address != address {
            continue;
        }
        if woken < wake_limit {
            waiting.waiting_for = None;
            waiting.wait_deadline = None;
            waiting.state.wait_result = Some(0);
            woken += 1;
        } else if requeued < requeue_limit {
            waiting.waiting_for = Some(ProcessWait::Futex { address: target, bitset });
            requeued += 1;
        } else {
            break;
        }
    }
    woken
}

fn syscall_futex(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    if take_interrupted_syscall(SYSCALL_FUTEX) {
        frame.rax = (-(EINTR as i64)) as u64;
        return frame_pointer;
    }
    let address = frame.rdi;
    let operation = frame.rsi & FUTEX_OPERATION_MASK;
    let value = frame.rdx as u32;
    if address == 0 || address % 4 != 0 {
        frame.rax = (-(EINVAL as i64)) as u64;
        return frame_pointer;
    }
    if paging::validate_user_range(address, core::mem::size_of::<u32>(), true).is_err() {
        frame.rax = (-(EFAULT as i64)) as u64;
        return frame_pointer;
    }

    let pid = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            frame.rax = (-(EINTR as i64)) as u64;
            return frame_pointer;
        }
        if let Some(result) = process.wait_result.take() {
            frame.rax = if result == 0 {
                0
            } else {
                (-(result as i64)) as u64
            };
            return frame_pointer;
        }
        process.pid
    };

    match operation {
        FUTEX_WAKE => {
            frame.rax = wake_futex_waiters(pid, address, FUTEX_BITSET_MATCH_ANY, value as u64);
            frame_pointer
        }
        FUTEX_WAKE_BITSET => {
            let wake_bits = frame.r9 as u32;
            if wake_bits == 0 {
                frame.rax = (-(EINVAL as i64)) as u64;
                return frame_pointer;
            }
            frame.rax = wake_futex_waiters(pid, address, wake_bits, value as u64);
            frame_pointer
        }
        FUTEX_REQUEUE => {
            let target = frame.r8;
            if target == 0 || target % 4 != 0 {
                frame.rax = (-(EINVAL as i64)) as u64;
                return frame_pointer;
            }
            if paging::validate_user_range(target, core::mem::size_of::<u32>(), true).is_err() {
                frame.rax = (-(EFAULT as i64)) as u64;
                return frame_pointer;
            }
            let requeue_limit = frame.r10 as u32 as u64;
            frame.rax = requeue_futex_waiters(
                pid,
                address,
                target,
                value as u64,
                requeue_limit,
            );
            frame_pointer
        }
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            let current = unsafe { (address as *const u32).read_unaligned() };
            if current != value {
                frame.rax = (-(EAGAIN as i64)) as u64;
                return frame_pointer;
            }
            let bitset = if operation == FUTEX_WAIT_BITSET {
                frame.r9 as u32
            } else {
                FUTEX_BITSET_MATCH_ANY
            };
            if bitset == 0 {
                frame.rax = (-(EINVAL as i64)) as u64;
                return frame_pointer;
            }
            let deadline = if operation == FUTEX_WAIT_BITSET {
                futex_absolute_deadline(frame.r10, frame.rsi & FUTEX_CLOCK_REALTIME != 0)
            } else {
                futex_deadline(frame.r10)
            };
            let deadline = match deadline {
                Ok(deadline) => deadline,
                Err(errno) => {
                    frame.rax = (-(errno as i64)) as u64;
                    return frame_pointer;
                }
            };
            if deadline == Some(0) {
                frame.rax = (-(ETIMEDOUT as i64)) as u64;
                return frame_pointer;
            }
            block_current_on_futex(frame, address, bitset, deadline)
        }
        _ => {
            frame.rax = (-(ENOSYS as i64)) as u64;
            frame_pointer
        }
    }
}

/// Bangunkan proses yang menunggu input pada terminal yang kini punya data,
/// termasuk `poll`/`select` yang memantau descriptor console.
fn wake_console_waiters(readable_terminals: u32) {
    if readable_terminals == 0 {
        return;
    }
    let mut scheduler = USER_SCHEDULER.lock();
    for process in &mut scheduler.processes {
        let console_ready = matches!(
            process.waiting_for,
            Some(ProcessWait::ConsoleInput(terminal))
                if terminal < u32::BITS as usize && readable_terminals & (1 << terminal) != 0
        );
        if console_ready || is_poll_wait(process.waiting_for) {
            process.waiting_for = None;
            process.wait_deadline = None;
        }
    }
}

/// Penunggu `poll`/`select` tidak menyimpan fd yang dipantau, jadi kernel cukup
/// membangunkan semuanya dan membiarkan scan ulang.
fn is_poll_wait(waiting: Option<ProcessWait>) -> bool {
    matches!(waiting, Some(ProcessWait::PollWait))
}

fn wake_poll_waiters() {
    let mut scheduler = USER_SCHEDULER.lock();
    wake_poll_waiters_in_scheduler(&mut scheduler);
}

fn wake_poll_waiters_in_scheduler(scheduler: &mut UserScheduler) {
    for process in &mut scheduler.processes {
        if is_poll_wait(process.waiting_for) {
            process.waiting_for = None;
            process.wait_deadline = None;
        }
    }
}

fn wake_pipe_waiters(pipe_id: u64, readers_ready: bool, writers_ready: bool) {
    let mut scheduler = USER_SCHEDULER.lock();
    wake_pipe_waiters_in_scheduler(&mut scheduler, pipe_id, readers_ready, writers_ready);
}

fn wake_pipe_waiters_in_scheduler(
    scheduler: &mut UserScheduler,
    pipe_id: u64,
    readers_ready: bool,
    writers_ready: bool,
) {
    for process in &mut scheduler.processes {
        let ready = match process.waiting_for {
            Some(ProcessWait::PipeReadable(waiting_id)) => readers_ready && waiting_id == pipe_id,
            Some(ProcessWait::PipeWritable(waiting_id)) => writers_ready && waiting_id == pipe_id,
            _ => false,
        };
        // Perubahan isi pipe bisa membuat `poll`/`select` siap.
        if ready || is_poll_wait(process.waiting_for) {
            process.waiting_for = None;
            process.wait_deadline = None;
        }
    }
}

fn new_user_process(
    cwd: &str,
    pid: u64,
    terminal: usize,
    environment: Vec<String>,
) -> Result<UserProcess, ElfError> {
    let cwd = fs::normalize_path(cwd)?;
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&cwd))??;
    if metadata.node_type != NodeType::Directory {
        return Err(ElfError::FileSystem(FsError::NotDirectory));
    }

    let mut descriptors = Vec::new();
    descriptors
        .try_reserve_exact(3)
        .map_err(|_| ElfError::OutOfMemory)?;
    descriptors.extend([
        Some(UserDescriptor::ConsoleInput),
        Some(UserDescriptor::ConsoleOutput),
        Some(UserDescriptor::ConsoleError),
    ]);
    let mut descriptor_flags = Vec::new();
    descriptor_flags
        .try_reserve_exact(3)
        .map_err(|_| ElfError::OutOfMemory)?;
    descriptor_flags.resize(3, 0);
    let mut process_cwd = String::new();
    process_cwd
        .try_reserve(cwd.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    process_cwd.push_str(&cwd);
    Ok(UserProcess {
        active: true,
        pid,
        process_group: pid,
        session: pid,
        terminal,
        descriptors,
        descriptor_flags,
        cwd: process_cwd,
        environment,
        file_creation_mask: 0o022,
        fpu_state: FpuState::new(),
        thread_pointer: 0,
        wait_result: None,
        pending_signals: 0,
        blocked_signals: 0,
        signal_handlers: Vec::new(),
        interrupted: false,
        interrupt_signal: 0,
        restart_number: None,
        poll_deadline: None,
        alternate_stack: 0,
        alternate_stack_size: 0,
        alternate_stack_active: false,
    })
}

fn fork_clone_process_state(parent: &UserProcess, pid: u64) -> Result<UserProcess, i32> {
    if !parent.active {
        return Err(EINTR);
    }
    if parent.descriptors.len() != parent.descriptor_flags.len() {
        return Err(EIO);
    }
    let mut descriptors = Vec::new();
    descriptors
        .try_reserve_exact(parent.descriptors.len())
        .map_err(|_| ENOMEM)?;
    descriptors.extend(parent.descriptors.iter().cloned());
    let mut descriptor_flags = Vec::new();
    descriptor_flags
        .try_reserve_exact(parent.descriptor_flags.len())
        .map_err(|_| ENOMEM)?;
    descriptor_flags.extend_from_slice(&parent.descriptor_flags);
    let mut cwd = String::new();
    cwd.try_reserve(parent.cwd.len()).map_err(|_| ENOMEM)?;
    cwd.push_str(&parent.cwd);
    Ok(UserProcess {
        active: true,
        pid,
        process_group: parent.process_group,
        session: parent.session,
        terminal: parent.terminal,
        descriptors,
        descriptor_flags,
        cwd,
        environment: try_clone_strings(&parent.environment).map_err(|_| ENOMEM)?,
        file_creation_mask: parent.file_creation_mask,
        fpu_state: parent.fpu_state,
        thread_pointer: parent.thread_pointer,
        wait_result: None,
        pending_signals: 0,
        blocked_signals: parent.blocked_signals,
        signal_handlers: parent.signal_handlers.clone(),
        interrupted: false,
        interrupt_signal: 0,
        restart_number: None,
        poll_deadline: None,
        alternate_stack: parent.alternate_stack,
        alternate_stack_size: parent.alternate_stack_size,
        alternate_stack_active: false,
    })
}

/// Snapshot the active scheduled task's TLS identity for fatal user-fault
/// diagnostics. This is deliberately called only after a fault, not in the
/// scheduler hot path.
pub fn current_user_tls_debug() -> (u64, u64, bool, u64, u64) {
    let scheduler = USER_SCHEDULER.lock();
    let process = USER_PROCESS.lock();
    let Some(task) = scheduler.processes.get(scheduler.current) else {
        return (0, 0, false, process.thread_pointer, thread_pointer::read());
    };
    (
        task.pid,
        task.tid,
        task.is_thread,
        process.thread_pointer,
        thread_pointer::read(),
    )
}

fn save_current_user_fpu() {
    let mut process = USER_PROCESS.lock();
    if process.active {
        unsafe { fpu::save(&mut process.fpu_state) };
    }
}

fn restore_current_user_fpu() {
    let process = USER_PROCESS.lock();
    unsafe {
        if process.active {
            fpu::restore(&process.fpu_state);
        } else {
            fpu::restore(&FpuState::new());
        }
    }
}

fn save_current_thread_pointer() {
    let mut process = USER_PROCESS.lock();
    if process.active {
        process.thread_pointer = thread_pointer::read();
    }
}

fn restore_current_thread_pointer() {
    let process = USER_PROCESS.lock();
    thread_pointer::write(if process.active {
        process.thread_pointer
    } else {
        0
    });
}

fn try_clone_strings(values: &[String]) -> Result<Vec<String>, ElfError> {
    let mut cloned = Vec::new();
    cloned
        .try_reserve_exact(values.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for value in values {
        let mut copy = String::new();
        copy.try_reserve(value.len())
            .map_err(|_| ElfError::OutOfMemory)?;
        copy.push_str(value);
        cloned.push(copy);
    }
    Ok(cloned)
}

fn try_clone_pages(pages: &[u64]) -> Result<Vec<u64>, ElfError> {
    let mut cloned = Vec::new();
    cloned
        .try_reserve_exact(pages.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    cloned.extend_from_slice(pages);
    Ok(cloned)
}

/// Salin metadata mmap untuk anak `fork`. Kunci memori tidak diwariskan (POSIX:
/// `mlock`/`mlockall` hanya berlaku pada proses yang memintanya), jadi setiap
/// salinan `locked` dimatikan.
fn try_clone_mapping_pages(pages: &[MappingPage]) -> Result<Vec<MappingPage>, ElfError> {
    let mut cloned = Vec::new();
    cloned
        .try_reserve_exact(pages.len())
        .map_err(|_| ElfError::OutOfMemory)?;
    for page in pages {
        let mut page = page.clone();
        page.locked = false;
        cloned.push(page);
    }
    Ok(cloned)
}

fn cleanup_failed_fork(image: LoadedElf, state: UserProcess, original_error: i32) -> i32 {
    drop(state);
    match image.unload() {
        Ok(()) => original_error,
        Err(_) => EIO,
    }
}

fn cleanup_failed_exec(image: LoadedElf, original_error: i32) -> i32 {
    match image.unload() {
        Ok(()) => original_error,
        Err(_) => EIO,
    }
}

pub(crate) fn valid_environment_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn create_pipe() -> Result<(PipeEnd, PipeEnd), i32> {
    let id = NEXT_PIPE_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| ENOMEM)?;
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(PIPE_CAPACITY)
        .map_err(|_| ENOMEM)?;
    buffer.resize(PIPE_CAPACITY, 0);
    let pipe = Arc::new(Mutex::new(Pipe {
        id,
        buffer,
        head: 0,
        length: 0,
        readers: 1,
        writers: 1,
    }));
    Ok((
        PipeEnd {
            pipe: Arc::clone(&pipe),
            readable: true,
            writable: false,
        },
        PipeEnd {
            pipe,
            readable: false,
            writable: true,
        },
    ))
}

fn reserve_descriptor_slot(descriptors: &mut Vec<Option<UserDescriptor>>) -> Result<usize, i32> {
    if let Some(fd) = descriptors
        .iter()
        .enumerate()
        .find_map(|(index, descriptor)| descriptor.is_none().then_some(index))
    {
        return Ok(fd);
    }
    if descriptors.len() >= MAX_OPEN_FILES {
        return Err(EMFILE);
    }
    descriptors.try_reserve(1).map_err(|_| ENOMEM)?;
    descriptors.push(None);
    Ok(descriptors.len() - 1)
}

fn install_pipe_descriptors(
    descriptors: &mut Vec<Option<UserDescriptor>>,
    descriptor_flags: &mut Vec<u32>,
    read_end: PipeEnd,
    write_end: PipeEnd,
) -> Result<(usize, usize), i32> {
    let read_fd = reserve_descriptor_slot(descriptors)?;
    descriptor_flags.resize(descriptors.len(), 0);
    descriptors[read_fd] = Some(UserDescriptor::Pipe(read_end));
    let write_fd = match reserve_descriptor_slot(descriptors) {
        Ok(fd) => fd,
        Err(error) => {
            descriptors[read_fd] = None;
            while descriptors.last().is_some_and(Option::is_none) {
                descriptors.pop();
            }
            return Err(error);
        }
    };
    descriptor_flags.resize(descriptors.len(), 0);
    descriptors[write_fd] = Some(UserDescriptor::Pipe(write_end));
    Ok((read_fd, write_fd))
}

fn syscall_write(fd: u64, address: u64, length: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if length == 0 {
        return Ok(0);
    }
    paging::validate_user_range(address, length, false).map_err(|_| EFAULT)?;
    let bytes = unsafe { core::slice::from_raw_parts(address as *const u8, length) };
    syscall_write_bytes(fd, bytes)
}

fn syscall_write_bytes(fd: usize, bytes: &[u8]) -> Result<u64, i32> {
    if bytes.len() > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if bytes.is_empty() {
        return Ok(0);
    }
    // Diambil sebelum mengunci `USER_PROCESS` agar urutan kunci tetap konsisten;
    // hanya penulisan ke file biasa yang menuntut `CAP_VFS_WRITE`.
    let caller_capabilities = current_capabilities()?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let terminal = process.terminal;
    let descriptor = process
        .descriptors
        .get_mut(fd)
        .and_then(Option::as_mut)
        .ok_or(EBADF)?;
    let mut pipe_id = None;
    let mut poll_event = false;
    let result = match descriptor {
        UserDescriptor::ConsoleOutput | UserDescriptor::ConsoleError => {
            let text = core::str::from_utf8(bytes).map_err(|_| EILSEQ)?;
            crate::terminal::terminal_print_to(terminal, text);
            // Mirror console writes to COM1 so headless boots can observe output.
            crate::drivers::serial::write_str(text);
            Ok(bytes.len() as u64)
        }
        UserDescriptor::File(file) => {
            capability_present(caller_capabilities, CAP_VFS_WRITE)?;
            let mut file = file.lock();
            if !file.writable {
                return Err(EBADF);
            }
            if !device_access_allowed(&file.path, terminal, current_euid()?, caller_capabilities) {
                return Err(EPERM);
            }
            if fs::is_tty_path(&file.path) {
                return fs::with_vfs(|vfs| vfs.write(&file.path, 0, bytes))
                    .map_err(errno_from_fs)?
                    .map(|count| count as u64)
                    .map_err(errno_from_fs);
            }
            let offset = if file.append {
                fs::with_vfs(|vfs| vfs.metadata(&file.path).map(|metadata| metadata.len))
                    .map_err(errno_from_fs)?
                    .map_err(errno_from_fs)?
            } else {
                file.offset
            };
            let count = fs::with_vfs(|vfs| vfs.write(&file.path, offset, bytes))
                .map_err(errno_from_fs)?
                .map_err(errno_from_fs)?;
            if !fs::is_fb_path(&file.path) {
                update_shared_mapping_pages(&file.path, offset, &bytes[..count]);
            }
            file.offset = offset.checked_add(count).ok_or(EINVAL)?;
            Ok(count as u64)
        }
        UserDescriptor::Pipe(endpoint) => {
            if !endpoint.writable {
                return Err(EBADF);
            }
            let mut pipe = endpoint.pipe.lock();
            let count = pipe.write(bytes)?;
            pipe_id = Some(pipe.id);
            Ok(count as u64)
        }
        UserDescriptor::SocketPair(endpoint) => {
            if !endpoint.write_end.writable {
                return Err(EBADF);
            }
            let mut pipe = endpoint.write_end.pipe.lock();
            let count = pipe.write(bytes)?;
            pipe_id = Some(pipe.id);
            Ok(count as u64)
        }
        UserDescriptor::EventFd(state) => {
            if bytes.len() != core::mem::size_of::<u64>() {
                return Err(EINVAL);
            }
            let mut event = state.lock();
            let value = u64::from_ne_bytes(bytes.try_into().map_err(|_| EINVAL)?);
            if event.semaphore {
                event.counter = event.counter.saturating_add(value);
            } else {
                event.counter = event
                    .counter
                    .checked_add(value)
                    .filter(|counter| *counter < u64::MAX)
                    .ok_or(EAGAIN)?;
            }
            poll_event = true;
            Ok(bytes.len() as u64)
        }
        UserDescriptor::TcpSocket(socket) => crate::network::send_tcp_socket(socket, bytes)
            .map(|count| count as u64)
            .map_err(errno_from_network),
        UserDescriptor::ConsoleInput
        | UserDescriptor::Directory(_)
        | UserDescriptor::UdpSocket(_)
        | UserDescriptor::Epoll(_)
        | UserDescriptor::TimerFd(_) => Err(EBADF),
    };
    drop(process);
    if let Some(pipe_id) = pipe_id {
        wake_pipe_waiters(pipe_id, true, false);
    }
    if poll_event {
        wake_poll_waiters();
    }
    result
}

#[cfg(test)]
fn format_printf(
    format: &str,
    mut argument: impl FnMut(usize) -> Result<u64, i32>,
    mut copy_string: impl FnMut(u64, usize) -> Result<String, i32>,
) -> Result<Vec<u8>, i32> {
    let input = format.as_bytes();
    let mut output = Vec::new();
    output.try_reserve(input.len()).map_err(|_| ENOMEM)?;
    let mut cursor = 0;
    let mut argument_index = 0;

    while cursor < input.len() {
        if input[cursor] != b'%' {
            append_printf_bytes(&mut output, &input[cursor..cursor + 1])?;
            cursor += 1;
            continue;
        }
        cursor += 1;
        if input.get(cursor) == Some(&b'%') {
            append_printf_bytes(&mut output, b"%")?;
            cursor += 1;
            continue;
        }

        let mut left = false;
        let mut plus = false;
        let mut space = false;
        let mut alternate = false;
        let mut zero = false;
        loop {
            match input.get(cursor) {
                Some(b'-') => left = true,
                Some(b'+') => plus = true,
                Some(b' ') => space = true,
                Some(b'#') => alternate = true,
                Some(b'0') => zero = true,
                _ => break,
            }
            cursor += 1;
        }

        let width = if input.get(cursor) == Some(&b'*') {
            cursor += 1;
            let width = argument(argument_index)? as i32;
            argument_index += 1;
            if width < 0 {
                left = true;
                width.unsigned_abs() as usize
            } else {
                width as usize
            }
        } else {
            parse_printf_number(input, &mut cursor)?.unwrap_or(0)
        };
        if width > MAX_SYSCALL_IO {
            return Err(EOVERFLOW);
        }

        let precision = if input.get(cursor) == Some(&b'.') {
            cursor += 1;
            if input.get(cursor) == Some(&b'*') {
                cursor += 1;
                let precision = argument(argument_index)? as i32;
                argument_index += 1;
                if precision < 0 {
                    None
                } else {
                    let precision = precision as usize;
                    if precision > MAX_SYSCALL_IO {
                        return Err(EOVERFLOW);
                    }
                    Some(precision)
                }
            } else {
                Some(parse_printf_number(input, &mut cursor)?.unwrap_or(0))
            }
        } else {
            None
        };

        let mut length_modifier = 0;
        if input.get(cursor) == Some(&b'h') {
            cursor += 1;
            length_modifier = 1;
            if input.get(cursor) == Some(&b'h') {
                cursor += 1;
                length_modifier = 2;
            }
        } else if input.get(cursor) == Some(&b'l') {
            cursor += 1;
            length_modifier = 3;
            if input.get(cursor) == Some(&b'l') {
                cursor += 1;
                length_modifier = 4;
            }
        } else if matches!(input.get(cursor), Some(b'z' | b't' | b'j')) {
            cursor += 1;
            length_modifier = 3;
        }

        let conversion = *input.get(cursor).ok_or(EINVAL)?;
        cursor += 1;
        match conversion {
            b's' => {
                let text = copy_string(argument(argument_index)?, MAX_SYSCALL_IO)?;
                argument_index += 1;
                let bytes = text.as_bytes();
                let count = precision.map_or(bytes.len(), |limit| bytes.len().min(limit));
                let padding = width.saturating_sub(count);
                if !left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
                append_printf_bytes(&mut output, &bytes[..count])?;
                if left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
            }
            b'c' => {
                let character = argument(argument_index)? as u8;
                argument_index += 1;
                let padding = width.saturating_sub(1);
                if !left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
                append_printf_bytes(&mut output, &[character])?;
                if left {
                    append_printf_repeat(&mut output, b' ', padding)?;
                }
            }
            b'd' | b'i' => {
                let raw = argument(argument_index)?;
                argument_index += 1;
                let value = match length_modifier {
                    1 => raw as i16 as i64,
                    2 => raw as i8 as i64,
                    3 | 4 => raw as i64,
                    _ => raw as u32 as i32 as i64,
                };
                append_printf_integer(
                    &mut output,
                    value.unsigned_abs(),
                    value < 0,
                    10,
                    false,
                    width,
                    precision,
                    left,
                    zero,
                    plus,
                    space,
                    false,
                    false,
                )?;
            }
            b'u' | b'o' | b'x' | b'X' | b'p' => {
                let raw = argument(argument_index)?;
                argument_index += 1;
                let value = if length_modifier >= 3 || conversion == b'p' {
                    raw
                } else {
                    match length_modifier {
                        1 => raw as u16 as u64,
                        2 => raw as u8 as u64,
                        _ => raw as u32 as u64,
                    }
                };
                let base = match conversion {
                    b'o' => 8,
                    b'x' | b'X' | b'p' => 16,
                    _ => 10,
                };
                append_printf_integer(
                    &mut output,
                    value,
                    false,
                    base,
                    conversion == b'X',
                    width,
                    precision,
                    left,
                    zero,
                    false,
                    false,
                    alternate || conversion == b'p',
                    conversion == b'p',
                )?;
            }
            _ => return Err(EINVAL),
        }
    }
    Ok(output)
}

#[cfg(test)]
fn parse_printf_number(input: &[u8], cursor: &mut usize) -> Result<Option<usize>, i32> {
    let start = *cursor;
    let mut value = 0usize;
    while let Some(byte) = input.get(*cursor).copied().filter(u8::is_ascii_digit) {
        let digit = byte - b'0';
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(digit as usize))
            .filter(|value| *value <= MAX_SYSCALL_IO)
            .ok_or(EOVERFLOW)?;
        *cursor += 1;
    }
    Ok((*cursor > start).then_some(value))
}

#[cfg(test)]
fn append_printf_integer(
    output: &mut Vec<u8>,
    mut value: u64,
    negative: bool,
    base: u64,
    uppercase: bool,
    width: usize,
    precision: Option<usize>,
    left: bool,
    zero: bool,
    plus: bool,
    space: bool,
    alternate: bool,
    pointer: bool,
) -> Result<(), i32> {
    let alphabet = if uppercase {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut digits = [0; 64];
    let mut digit_count = 0;
    if value != 0 || precision != Some(0) {
        loop {
            digits[digit_count] = alphabet[(value % base) as usize];
            digit_count += 1;
            value /= base;
            if value == 0 {
                break;
            }
        }
    }

    let mut leading_zeroes = precision.unwrap_or(0).saturating_sub(digit_count);
    if base == 8
        && alternate
        && (digit_count == 0 || digits[digit_count - 1] != b'0')
        && leading_zeroes == 0
    {
        leading_zeroes = 1;
    }
    let sign = if negative {
        Some(b'-')
    } else if plus {
        Some(b'+')
    } else if space {
        Some(b' ')
    } else {
        None
    };
    let prefix = if pointer
        || (alternate && base == 16 && digit_count != 0 && !(digit_count == 1 && digits[0] == b'0'))
    {
        Some(if uppercase { b"0X" } else { b"0x" }.as_slice())
    } else {
        None
    };
    let prefix_length = prefix.map_or(0, <[u8]>::len);
    let total = digit_count
        .checked_add(leading_zeroes)
        .and_then(|length| length.checked_add(prefix_length))
        .and_then(|length| length.checked_add(usize::from(sign.is_some())))
        .ok_or(EOVERFLOW)?;
    let mut padding = width.saturating_sub(total);
    if !left && (!zero || precision.is_some()) {
        append_printf_repeat(output, b' ', padding)?;
    }
    if let Some(sign) = sign {
        append_printf_bytes(output, &[sign])?;
    }
    if let Some(prefix) = prefix {
        append_printf_bytes(output, prefix)?;
    }
    if !left && zero && precision.is_none() {
        leading_zeroes = leading_zeroes.checked_add(padding).ok_or(EOVERFLOW)?;
        padding = 0;
    }
    append_printf_repeat(output, b'0', leading_zeroes)?;
    while digit_count != 0 {
        digit_count -= 1;
        append_printf_bytes(output, &digits[digit_count..digit_count + 1])?;
    }
    if left {
        append_printf_repeat(output, b' ', padding)?;
    }
    Ok(())
}

#[cfg(test)]
fn append_printf_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), i32> {
    let new_length = output
        .len()
        .checked_add(bytes.len())
        .filter(|length| *length <= MAX_SYSCALL_IO)
        .ok_or(EOVERFLOW)?;
    output.try_reserve(bytes.len()).map_err(|_| ENOMEM)?;
    output.extend_from_slice(bytes);
    debug_assert_eq!(output.len(), new_length);
    Ok(())
}

#[cfg(test)]
fn append_printf_repeat(output: &mut Vec<u8>, byte: u8, count: usize) -> Result<(), i32> {
    let new_length = output
        .len()
        .checked_add(count)
        .filter(|length| *length <= MAX_SYSCALL_IO)
        .ok_or(EOVERFLOW)?;
    output.try_reserve(count).map_err(|_| ENOMEM)?;
    output.resize(new_length, byte);
    Ok(())
}

fn syscall_read(fd: u64, address: u64, length: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if length == 0 {
        return Ok(0);
    }
    paging::validate_user_range(address, length, true).map_err(|_| EFAULT)?;
    let output = unsafe { core::slice::from_raw_parts_mut(address as *mut u8, length) };
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let terminal = process.terminal;
    let descriptor = process
        .descriptors
        .get_mut(fd)
        .and_then(Option::as_mut)
        .ok_or(EBADF)?;
    let mut pipe_id = None;
    let result = match descriptor {
        UserDescriptor::ConsoleInput => match fs::poll_tty(terminal, output) {
            Ok(Some(count)) => Ok(count as u64),
            Ok(None) => Err(EAGAIN),
            Err(error) => Err(errno_from_fs(error)),
        },
        UserDescriptor::File(file) => {
            let mut file = file.lock();
            if !file.readable {
                return Err(EBADF);
            }
            // Terminal yang dibuka lewat path (`/dev/stdin`, `/dev/tty`) dibaca
            // tanpa menunggu di kernel; `EAGAIN` memicu `ProcessWait::ConsoleInput`.
            if let Some(tty_terminal) = fs::tty_path_terminal(&file.path) {
                return match fs::poll_tty(tty_terminal, output) {
                    Ok(Some(count)) => Ok(count as u64),
                    Ok(None) => Err(EAGAIN),
                    Err(error) => Err(errno_from_fs(error)),
                };
            }
            let count = fs::with_vfs(|vfs| vfs.read(&file.path, file.offset, output))
                .map_err(errno_from_fs)?
                .map_err(errno_from_fs)?;
            if !fs::is_tty_path(&file.path) {
                if !fs::is_fb_path(&file.path) {
                    overlay_shared_mapping_pages(&file.path, file.offset, &mut output[..count]);
                }
                file.offset = file.offset.checked_add(count).ok_or(EINVAL)?;
            }
            Ok(count as u64)
        }
        UserDescriptor::Pipe(endpoint) => {
            if !endpoint.readable {
                return Err(EBADF);
            }
            let mut pipe = endpoint.pipe.lock();
            let count = pipe.read(output)?;
            pipe_id = Some(pipe.id);
            Ok(count as u64)
        }
        UserDescriptor::UdpSocket(socket) => crate::network::receive_udp_socket(socket, output)
            .map(|(count, _, _)| count as u64)
            .map_err(|_| EAGAIN),
        UserDescriptor::SocketPair(endpoint) => {
            if !endpoint.read_end.readable {
                return Err(EBADF);
            }
            let mut pipe = endpoint.read_end.pipe.lock();
            let count = pipe.read(output)?;
            pipe_id = Some(pipe.id);
            Ok(count as u64)
        }
        UserDescriptor::EventFd(state) => {
            if output.len() < core::mem::size_of::<u64>() {
                return Err(EINVAL);
            }
            let mut event = state.lock();
            if event.counter == 0 {
                return Err(EAGAIN);
            }
            let value = if event.semaphore {
                event.counter -= 1;
                1
            } else {
                let counter = event.counter;
                event.counter = 0;
                counter
            };
            output[..8].copy_from_slice(&value.to_ne_bytes());
            Ok(8)
        }
        UserDescriptor::TimerFd(state) => {
            if output.len() < core::mem::size_of::<u64>() {
                return Err(EINVAL);
            }
            let now = crate::arch::x86_64::monotonic_milliseconds();
            let mut timer = state.lock();
            let expirations = timerfd_take_expirations(&mut timer, now);
            if expirations == 0 {
                return Err(EAGAIN);
            }
            output[..8].copy_from_slice(&expirations.to_ne_bytes());
            Ok(8)
        }
        UserDescriptor::TcpSocket(socket) => {
            crate::network::receive_tcp_socket(socket, output, false)
                .map(|count| count as u64)
                .map_err(errno_from_network)
        }
        UserDescriptor::ConsoleOutput | UserDescriptor::ConsoleError => Err(EBADF),
        UserDescriptor::Directory(_) => Err(EBADF),
        UserDescriptor::Epoll(_) => Err(EBADF),
    };
    drop(process);
    if let Some(pipe_id) = pipe_id {
        wake_pipe_waiters(pipe_id, false, true);
    }
    result
}

fn syscall_udp_socket() -> Result<u64, i32> {
    let socket = crate::network::create_udp_socket();
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let fd = reserve_descriptor_slot(&mut process.descriptors)?;
    let descriptor_count = process.descriptors.len();
    process.descriptor_flags.resize(descriptor_count, 0);
    process.descriptors[fd] = Some(UserDescriptor::UdpSocket(socket));
    Ok(fd as u64)
}

fn udp_socket_for_fd(fd: u64) -> Result<Arc<Mutex<crate::network::UdpSocket>>, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::UdpSocket(socket)) => Ok(Arc::clone(socket)),
        Some(_) => Err(ENOTSOCK),
        None => Err(EBADF),
    }
}

fn syscall_udp_bind(fd: u64, port: u64) -> Result<u64, i32> {
    let port = u16::try_from(port).map_err(|_| EINVAL)?;
    let socket = udp_socket_for_fd(fd)?;
    crate::network::bind_udp_socket(&socket, port)
        .map(|bound_port| bound_port as u64)
        .map_err(errno_from_network)
}

fn syscall_udp_sendto(
    fd: u64,
    destination: u64,
    port: u64,
    address: u64,
    length: u64,
) -> Result<u64, i32> {
    let destination = u32::try_from(destination)
        .map_err(|_| EINVAL)?
        .to_be_bytes();
    let port = u16::try_from(port).map_err(|_| EINVAL)?;
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    let payload = if length == 0 {
        &[]
    } else {
        paging::validate_user_range(address, length, false).map_err(|_| EFAULT)?;
        unsafe { core::slice::from_raw_parts(address as *const u8, length) }
    };
    let socket = udp_socket_for_fd(fd)?;
    crate::network::send_udp_socket(&socket, destination, port, payload)
        .map(|()| length as u64)
        .map_err(errno_from_network)
}

fn syscall_udp_recvfrom(
    fd: u64,
    address: u64,
    length: u64,
    source_ip_address: u64,
    source_port_address: u64,
) -> Result<u64, i32> {
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if length != 0 {
        paging::validate_user_range(address, length, true).map_err(|_| EFAULT)?;
    }
    if source_ip_address != 0 {
        paging::validate_user_range(source_ip_address, 4, true).map_err(|_| EFAULT)?;
    }
    if source_port_address != 0 {
        paging::validate_user_range(source_port_address, 2, true).map_err(|_| EFAULT)?;
    }
    let socket = udp_socket_for_fd(fd)?;
    let output = if length == 0 {
        &mut []
    } else {
        unsafe { core::slice::from_raw_parts_mut(address as *mut u8, length) }
    };
    let (_copied, received, source_ip, source_port) =
        crate::network::receive_udp_socket_length(&socket, output).map_err(errno_from_network)?;
    if source_ip_address != 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                source_ip.as_ptr(),
                source_ip_address as *mut u8,
                source_ip.len(),
            );
        }
    }
    if source_port_address != 0 {
        unsafe { (source_port_address as *mut u16).write_unaligned(source_port) };
    }
    Ok(received as u64)
}

fn syscall_network_dns(address: u64) -> Result<u64, i32> {
    paging::validate_user_range(address, 4, true).map_err(|_| EFAULT)?;
    let config = crate::network::config();
    if !config.configured || config.dns == [0; 4] {
        return Err(ENETDOWN);
    }
    unsafe {
        core::ptr::copy_nonoverlapping(config.dns.as_ptr(), address as *mut u8, 4);
    }
    Ok(0)
}

fn tcp_socket_for_fd(fd: u64) -> Result<Arc<Mutex<crate::network::TcpSocket>>, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::TcpSocket(socket)) => Ok(Arc::clone(socket)),
        Some(_) => Err(ENOTSOCK),
        None => Err(EBADF),
    }
}

fn syscall_socket(domain: u64, socket_type: u64, protocol: u64) -> Result<u64, i32> {
    require_capability(CAP_NETWORK)?;
    if domain != 2 {
        return Err(EAFNOSUPPORT);
    }
    if socket_type != 1 || (protocol != 0 && protocol != 6) {
        return Err(ENOTSUP);
    }
    let socket = crate::network::create_tcp_socket();
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let fd = reserve_descriptor_slot(&mut process.descriptors)?;
    let descriptor_count = process.descriptors.len();
    process.descriptor_flags.resize(descriptor_count, 0);
    process.descriptors[fd] = Some(UserDescriptor::TcpSocket(socket));
    Ok(fd as u64)
}

fn copy_user_sockaddr(address: u64, length: u64) -> Result<([u8; 4], u16), i32> {
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length < core::mem::size_of::<UserSockAddrIn>() {
        return Err(EINVAL);
    }
    paging::validate_user_range(address, core::mem::size_of::<UserSockAddrIn>(), false)
        .map_err(|_| EFAULT)?;
    let address = unsafe { (address as *const UserSockAddrIn).read_unaligned() };
    if address.family != 2 {
        return Err(EAFNOSUPPORT);
    }
    Ok((address.address.to_ne_bytes(), u16::from_be(address.port)))
}

fn validate_sockaddr_output(address: u64, length_address: u64) -> Result<(), i32> {
    if address == 0 {
        return Ok(());
    }
    if length_address == 0 {
        return Err(EFAULT);
    }
    paging::validate_user_range(length_address, core::mem::size_of::<u32>(), true)
        .map_err(|_| EFAULT)?;
    let capacity = unsafe { (length_address as *const u32).read_unaligned() as usize };
    let copy_length = capacity.min(core::mem::size_of::<UserSockAddrIn>());
    if copy_length != 0 {
        paging::validate_user_range(address, copy_length, true).map_err(|_| EFAULT)?;
    }
    Ok(())
}

fn write_user_sockaddr(
    address: u64,
    length_address: u64,
    peer_address: [u8; 4],
    peer_port: u16,
) -> Result<(), i32> {
    if address == 0 {
        return Ok(());
    }
    validate_sockaddr_output(address, length_address)?;
    let capacity = unsafe { (length_address as *const u32).read_unaligned() as usize };
    let socket_address = UserSockAddrIn {
        family: 2,
        port: peer_port.to_be(),
        address: u32::from_ne_bytes(peer_address),
        zero: [0; 8],
    };
    let copy_length = capacity.min(core::mem::size_of::<UserSockAddrIn>());
    if copy_length != 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                &socket_address as *const UserSockAddrIn as *const u8,
                address as *mut u8,
                copy_length,
            );
        }
    }
    unsafe {
        (length_address as *mut u32).write_unaligned(core::mem::size_of::<UserSockAddrIn>() as u32);
    }
    Ok(())
}

fn syscall_bind(fd: u64, address: u64, length: u64) -> Result<u64, i32> {
    let (local_address, local_port) = copy_user_sockaddr(address, length)?;
    crate::network::bind_tcp_socket(&tcp_socket_for_fd(fd)?, local_address, local_port)
        .map(|_| 0)
        .map_err(errno_from_network)
}

fn syscall_listen(fd: u64, backlog: u64) -> Result<u64, i32> {
    if backlog == 0 || backlog > i32::MAX as u64 {
        return Err(EINVAL);
    }
    crate::network::listen_tcp_socket(&tcp_socket_for_fd(fd)?, backlog as usize)
        .map(|()| 0)
        .map_err(errno_from_network)
}

fn syscall_accept(fd: u64, address: u64, length_address: u64) -> Result<u64, i32> {
    validate_sockaddr_output(address, length_address)?;
    let listener = tcp_socket_for_fd(fd)?;
    let accepted_fd = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let fd = reserve_descriptor_slot(&mut process.descriptors)?;
        let descriptor_count = process.descriptors.len();
        process.descriptor_flags.resize(descriptor_count, 0);
        fd
    };
    let (socket, peer_address, peer_port) = match crate::network::accept_tcp_socket(&listener) {
        Ok(accepted) => accepted,
        Err(error) => return Err(errno_from_network(error)),
    };
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    process.descriptors[accepted_fd] = Some(UserDescriptor::TcpSocket(socket));
    drop(process);
    if let Err(error) = write_user_sockaddr(address, length_address, peer_address, peer_port) {
        let _ = syscall_close(accepted_fd as u64);
        return Err(error);
    }
    Ok(accepted_fd as u64)
}

fn syscall_connect(fd: u64, address: u64, length: u64) -> Result<u64, i32> {
    let (remote_address, remote_port) = copy_user_sockaddr(address, length)?;
    match udp_socket_for_fd(fd) {
        Ok(socket) => {
            return crate::network::connect_udp_socket(&socket, remote_address, remote_port)
                .map(|()| 0)
                .map_err(errno_from_network);
        }
        Err(errno) if errno == EINTR => return Err(EINTR),
        Err(_) => {}
    }
    crate::network::connect_tcp_socket(&tcp_socket_for_fd(fd)?, remote_address, remote_port)
        .map(|()| 0)
        .map_err(errno_from_network)
}

fn syscall_send(fd: u64, address: u64, length: u64, flags: u64) -> Result<u64, i32> {
    if flags & !0x4040 != 0 {
        return Err(EINVAL);
    }
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    let payload = if length == 0 {
        &[]
    } else {
        paging::validate_user_range(address, length, false).map_err(|_| EFAULT)?;
        unsafe { core::slice::from_raw_parts(address as *const u8, length) }
    };
    if let Ok(socket) = udp_socket_for_fd(fd) {
        return crate::network::send_udp_socket_connected(&socket, payload)
            .map(|()| length as u64)
            .map_err(errno_from_network);
    }
    crate::network::send_tcp_socket(&tcp_socket_for_fd(fd)?, payload)
        .map(|length| length as u64)
        .map_err(errno_from_network)
}

fn syscall_recv(fd: u64, address: u64, length: u64, flags: u64) -> Result<u64, i32> {
    if flags & !0x42 != 0 {
        return Err(EINVAL);
    }
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    if length > MAX_SYSCALL_IO {
        return Err(EINVAL);
    }
    if length != 0 {
        paging::validate_user_range(address, length, true).map_err(|_| EFAULT)?;
    }
    let mut empty = [];
    let output = if length == 0 {
        &mut empty[..]
    } else {
        unsafe { core::slice::from_raw_parts_mut(address as *mut u8, length) }
    };
    if let Ok(socket) = udp_socket_for_fd(fd) {
        let (copied, _total, _source_ip, _source_port) =
            crate::network::receive_udp_socket_length(&socket, output)
                .map_err(errno_from_network)?;
        return Ok(copied as u64);
    }
    crate::network::receive_tcp_socket(&tcp_socket_for_fd(fd)?, output, flags & 0x02 != 0)
        .map(|length| length as u64)
        .map_err(errno_from_network)
}

fn syscall_shutdown(fd: u64, how: u64) -> Result<u64, i32> {
    crate::network::shutdown_tcp_socket(&tcp_socket_for_fd(fd)?, how)
        .map(|()| 0)
        .map_err(errno_from_network)
}

fn syscall_socket_name(fd: u64, address: u64, length_address: u64, peer: bool) -> Result<u64, i32> {
    validate_sockaddr_output(address, length_address)?;
    let (socket_address, port) = crate::network::tcp_socket_name(&tcp_socket_for_fd(fd)?, peer)
        .map_err(errno_from_network)?;
    write_user_sockaddr(address, length_address, socket_address, port)?;
    Ok(0)
}

fn errno_from_network(error: &'static str) -> i32 {
    match error {
        "no network interface" => ENODEV,
        "network interface has no IPv4 address" | "network interface is not ready" => ENETDOWN,
        "UDP socket would block" => EAGAIN,
        "UDP port is already in use" => EADDRINUSE,
        "UDP socket is already bound" => EINVAL,
        "UDP socket is not connected" => ENOTCONN,
        "invalid UDP peer address" => EADDRNOTAVAIL,
        "UDP payload exceeds the IPv4 MTU" | "IPv4 payload exceeds the MTU" => EMSGSIZE,
        "no route to destination" => EHOSTUNREACH,
        "TCP accept would block"
        | "TCP connection is in progress"
        | "TCP send would block"
        | "TCP receive would block"
        | "ARP request sent; retry after the neighbor responds" => EAGAIN,
        "TCP port is already in use" => EADDRINUSE,
        "local IPv4 address is not assigned to this interface" => EADDRNOTAVAIL,
        "TCP socket is not connected" => ENOTCONN,
        "TCP connection refused" => ECONNREFUSED,
        "TCP connection reset" => ECONNRESET,
        "TCP connection timed out" => ETIMEDOUT,
        "TCP write side is shut down" => EPIPE,
        "TCP socket is closed" => ENOTCONN,
        "invalid TCP shutdown mode" | "invalid TCP peer address" => EINVAL,
        "TCP socket is already connected" => EISCONN,
        "TCP socket cannot listen in its current state"
        | "TCP socket cannot connect in its current state"
        | "TCP socket is not listening" => EINVAL,
        "TCP socket is already bound" => EINVAL,
        "no ephemeral TCP ports available" => EADDRINUSE,
        "TCP payload exceeds the IPv4 MTU" => EMSGSIZE,
        "TCP socket cannot be bound in its current state" => EINVAL,
        _ => EIO,
    }
}

fn syscall_open(address: u64, flags: u64, mode: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(address)?)?;
    syscall_open_path(path, flags, mode)
}

fn syscall_openat(dirfd: u64, path_address: u64, flags: u64, mode: u64) -> Result<u64, i32> {
    let path = resolve_at_path(dirfd, &copy_user_path(path_address)?)?;
    syscall_open_path(path, flags, mode)
}

fn syscall_open_path(mut path: String, flags: u64, mode: u64) -> Result<u64, i32> {
    const ALLOWED_FLAGS: u64 = O_ACCESS_MASK | O_CREAT | O_TRUNC | O_APPEND | O_NONBLOCK;
    if flags & !ALLOWED_FLAGS != 0 || flags & O_ACCESS_MASK == 3 {
        return Err(EINVAL);
    }
    if flags & (O_WRONLY | O_RDWR | O_CREAT | O_TRUNC | O_APPEND) != 0 {
        require_capability(CAP_VFS_WRITE)?;
    }
    let mode = if flags & O_CREAT != 0 {
        let mode = u16::try_from(mode).map_err(|_| EINVAL)?;
        if mode & !0o777 != 0 {
            return Err(EINVAL);
        }
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        mode & !process.file_creation_mask
    } else {
        0o644
    };
    let writable = flags & O_ACCESS_MASK != O_RDONLY;
    let readable = flags & O_ACCESS_MASK != O_WRONLY;
    if flags & O_TRUNC != 0 && !writable || flags & O_APPEND != 0 && !writable {
        return Err(EINVAL);
    }
    let process_terminal = USER_PROCESS.lock().terminal;
    if matches!(
        path.as_str(),
        "/dev/tty" | "/dev/stdin" | "/dev/stdout" | "/dev/stderr" | "/dev/console"
    ) {
        path = alloc::format!("/dev/tty{process_terminal}");
    }
    if !device_access_allowed(&path, process_terminal, current_euid()?, current_capabilities()?) {
        return Err(EPERM);
    }
    if let Some((device_readable, device_writable)) = fs::device_access(&path)
        && (readable && !device_readable || writable && !device_writable)
    {
        return Err(EACCES);
    }
    let fd = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        if let Some(fd) = process
            .descriptors
            .iter()
            .enumerate()
            .find_map(|(index, descriptor)| descriptor.is_none().then_some(index))
        {
            fd
        } else {
            if process.descriptors.len() >= MAX_OPEN_FILES {
                return Err(EMFILE);
            }
            process.descriptors.try_reserve(1).map_err(|_| ENOMEM)?;
            process.descriptors.push(None);
            process.descriptors.len() - 1
        }
    };
    {
        let mut process = USER_PROCESS.lock();
        let descriptor_count = process.descriptors.len();
        process.descriptor_flags.resize(descriptor_count, 0);
    }
    let size = match fs::with_vfs(|vfs| {
        let mut created = false;
        let metadata = match vfs.metadata(&path) {
            Ok(metadata) => metadata,
            Err(FsError::NotFound) if flags & O_CREAT != 0 => {
                vfs.create_file(&path)?;
                created = true;
                if mode != 0o644 {
                    apply_creation_mode(|| vfs.set_permissions(&path, mode))?;
                }
                vfs.metadata(&path)?
            }
            Err(error) => return Err(error),
        };
        if metadata.node_type == NodeType::Directory {
            return Err(FsError::IsDirectory);
        }
        if !created
            && ((readable && metadata.permissions & 0o400 == 0)
                || (writable && metadata.permissions & 0o200 == 0))
        {
            return Err(FsError::PermissionDenied);
        }
        if flags & O_TRUNC != 0 {
            vfs.truncate(&path, 0)?;
            Ok(0)
        } else {
            Ok(metadata.len)
        }
    })
    .map_err(errno_from_fs)?
    .map_err(errno_from_fs)
    {
        Ok(size) => size,
        Err(errno) => {
            USER_PROCESS.lock().descriptors[fd] = None;
            return Err(errno);
        }
    };

    let mut process = USER_PROCESS.lock();
    process.descriptors[fd] = Some(UserDescriptor::File(Arc::new(Mutex::new(OpenFile {
        path,
        offset: if flags & O_APPEND != 0 { size } else { 0 },
        readable,
        writable,
        append: flags & O_APPEND != 0,
    }))));
    let descriptor_count = process.descriptors.len();
    process.descriptor_flags.resize(descriptor_count, 0);
    if flags & O_NONBLOCK != 0 {
        process.descriptor_flags[fd] |= O_NONBLOCK as u32;
    }
    Ok(fd as u64)
}

fn syscall_pipe(address: u64) -> Result<u64, i32> {
    paging::validate_user_range(address, core::mem::size_of::<[i32; 2]>(), true)
        .map_err(|_| EFAULT)?;
    let (read_end, write_end) = create_pipe()?;
    let (read_fd, write_fd) = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let UserProcess {
            descriptors,
            descriptor_flags,
            ..
        } = &mut *process;
        install_pipe_descriptors(descriptors, descriptor_flags, read_end, write_end)?
    };
    unsafe {
        (address as *mut [i32; 2]).write_unaligned([read_fd as i32, write_fd as i32]);
    }
    Ok(0)
}

fn syscall_dup2(source_fd: u64, target_fd: u64) -> Result<u64, i32> {
    let source_fd = usize::try_from(source_fd).map_err(|_| EBADF)?;
    let target_fd = usize::try_from(target_fd).map_err(|_| EBADF)?;
    if target_fd >= MAX_OPEN_FILES {
        return Err(EBADF);
    }
    if source_fd == target_fd {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        return process
            .descriptors
            .get(source_fd)
            .and_then(Option::as_ref)
            .map(|_| target_fd as u64)
            .ok_or(EBADF);
    }

    let replaced_pipe_id = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let duplicate = process
            .descriptors
            .get(source_fd)
            .and_then(Option::as_ref)
            .ok_or(EBADF)?
            .clone();
        let replaced_pipe_id = process
            .descriptors
            .get(target_fd)
            .and_then(Option::as_ref)
            .and_then(|descriptor| match descriptor {
                UserDescriptor::Pipe(endpoint) => Some(endpoint.pipe.lock().id),
                _ => None,
            });
        if target_fd >= process.descriptors.len() {
            let additional = target_fd + 1 - process.descriptors.len();
            process
                .descriptors
                .try_reserve(additional)
                .map_err(|_| ENOMEM)?;
            process.descriptors.resize_with(target_fd + 1, || None);
        }
        let descriptor_count = process.descriptors.len();
        process.descriptor_flags.resize(descriptor_count, 0);
        process.descriptor_flags[target_fd] = 0;
        process.descriptors[target_fd] = Some(duplicate);
        replaced_pipe_id
    };
    if let Some(pipe_id) = replaced_pipe_id {
        wake_pipe_waiters(pipe_id, true, true);
    }
    Ok(target_fd as u64)
}

fn syscall_dup(source_fd: u64) -> Result<u64, i32> {
    let source_fd = usize::try_from(source_fd).map_err(|_| EBADF)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let duplicate = process
        .descriptors
        .get(source_fd)
        .and_then(Option::as_ref)
        .ok_or(EBADF)?
        .clone();
    let target_fd = reserve_descriptor_slot(&mut process.descriptors)?;
    let descriptor_count = process.descriptors.len();
    process.descriptor_flags.resize(descriptor_count, 0);
    process.descriptors[target_fd] = Some(duplicate);
    process.descriptor_flags[target_fd] = 0;
    Ok(target_fd as u64)
}

fn syscall_fcntl(fd: u64, command: u64, argument: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let descriptor = process
        .descriptors
        .get(fd)
        .and_then(Option::as_ref)
        .ok_or(EBADF)?;
    match command {
        F_DUPFD => {
            let minimum = usize::try_from(argument).map_err(|_| EINVAL)?;
            if minimum >= MAX_OPEN_FILES {
                return Err(EINVAL);
            }
            let duplicate = descriptor.clone();
            let target = (minimum..MAX_OPEN_FILES)
                .find(|index| process.descriptors.get(*index).is_none_or(Option::is_none))
                .ok_or(EMFILE)?;
            if target >= process.descriptors.len() {
                let additional = target + 1 - process.descriptors.len();
                process
                    .descriptors
                    .try_reserve(additional)
                    .map_err(|_| ENOMEM)?;
                process.descriptors.resize_with(target + 1, || None);
            }
            let descriptor_count = process.descriptors.len();
            process.descriptor_flags.resize(descriptor_count, 0);
            process.descriptors[target] = Some(duplicate);
            process.descriptor_flags[target] = 0;
            Ok(target as u64)
        }
        F_GETFD => Ok(process.descriptor_flags.get(fd).copied().unwrap_or(0) as u64),
        F_SETFD => {
            let flags = u32::try_from(argument).map_err(|_| EINVAL)?;
            if flags & !FD_CLOEXEC != 0 {
                return Err(EINVAL);
            }
            if fd >= process.descriptor_flags.len() {
                let descriptor_count = process.descriptors.len();
                process.descriptor_flags.resize(descriptor_count, 0);
            }
            process.descriptor_flags[fd] = flags;
            Ok(0)
        }
        F_GETFL => {
            let status = process
                .descriptor_flags
                .get(fd)
                .copied()
                .unwrap_or(0) as u64
                & (O_APPEND | O_NONBLOCK);
            let access = match descriptor {
                UserDescriptor::File(file) => {
                    let file = file.lock();
                    if !file.readable {
                        O_WRONLY
                    } else if file.writable {
                        O_RDWR
                    } else {
                        O_RDONLY
                    }
                }
                UserDescriptor::Pipe(endpoint) => {
                    if endpoint.writable {
                        O_WRONLY
                    } else {
                        O_RDONLY
                    }
                }
                _ => O_RDONLY,
            };
            Ok(access | status)
        }
        F_SETFL => {
            if argument & !(O_APPEND | O_NONBLOCK) != 0 {
                return Err(EINVAL);
            }
            if let UserDescriptor::File(file) = descriptor {
                let mut file = file.lock();
                file.append = argument & O_APPEND != 0;
            }
            if fd >= process.descriptor_flags.len() {
                let descriptor_count = process.descriptors.len();
                process.descriptor_flags.resize(descriptor_count, 0);
            }
            let cloexec = process.descriptor_flags[fd] & FD_CLOEXEC;
            process.descriptor_flags[fd] =
                cloexec | (argument as u32 & (O_APPEND | O_NONBLOCK) as u32);
            Ok(0)
        }
        _ => Err(EINVAL),
    }
}

fn syscall_close(fd: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let descriptor = process.descriptors.get_mut(fd).ok_or(EBADF)?;
    if descriptor.is_none() {
        return Err(EBADF);
    }
    let pipe_id = descriptor.as_ref().and_then(|descriptor| match descriptor {
        UserDescriptor::Pipe(endpoint) => Some(endpoint.pipe.lock().id),
        _ => None,
    });
    let tcp_socket_to_close = descriptor.as_ref().and_then(|descriptor| match descriptor {
        UserDescriptor::TcpSocket(socket)
            if crate::network::tcp_socket_is_last_descriptor(socket) =>
        {
            Some(Arc::clone(socket))
        }
        _ => None,
    });
    *descriptor = None;
    if let Some(flags) = process.descriptor_flags.get_mut(fd) {
        *flags = 0;
    }
    drop(process);
    if let Some(pipe_id) = pipe_id {
        wake_pipe_waiters(pipe_id, true, true);
    }
    if let Some(socket) = tcp_socket_to_close {
        crate::network::close_tcp_socket(&socket);
    }
    Ok(0)
}

fn syscall_lseek(fd: u64, offset: i64, whence: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let file = match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::File(file)) => Arc::clone(file),
        Some(
            UserDescriptor::Pipe(_)
            | UserDescriptor::Directory(_)
            | UserDescriptor::UdpSocket(_)
            | UserDescriptor::TcpSocket(_),
        ) => return Err(ESPIPE),
        _ => return Err(EBADF),
    };
    let mut file = file.lock();
    if fs::is_tty_path(&file.path) {
        return Err(ESPIPE);
    }
    let base = match whence {
        0 => 0usize,
        1 => file.offset,
        2 => fs::with_vfs(|vfs| vfs.metadata(&file.path).map(|metadata| metadata.len))
            .map_err(errno_from_fs)?
            .map_err(errno_from_fs)?,
        _ => return Err(EINVAL),
    };
    let result = (base as i128) + (offset as i128);
    if result < 0 || result > usize::MAX as i128 {
        return Err(EINVAL);
    }
    file.offset = result as usize;
    Ok(file.offset as u64)
}

fn syscall_opendir(address: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(address)?)?;
    let entries = fs::with_vfs(|vfs| vfs.read_dir(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let fd = reserve_descriptor_slot(&mut process.descriptors)?;
    let descriptor_count = process.descriptors.len();
    process.descriptor_flags.resize(descriptor_count, 0);
    process.descriptors[fd] = Some(UserDescriptor::Directory(Arc::new(Mutex::new(
        DirectoryStream {
            path,
            entries,
            next_entry: 0,
        },
    ))));
    Ok(fd as u64)
}

fn syscall_readdir(fd: u64, address: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let directory = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        match process.descriptors.get(fd).and_then(Option::as_ref) {
            Some(UserDescriptor::Directory(directory)) => Arc::clone(directory),
            Some(_) => return Err(ENOTDIR),
            None => return Err(EBADF),
        }
    };

    paging::validate_user_range(address, core::mem::size_of::<UserDirent>(), true)
        .map_err(|_| EFAULT)?;
    let mut directory = directory.lock();
    let Some(entry) = directory.entries.get(directory.next_entry) else {
        return Ok(0);
    };
    let record = user_dirent(directory.next_entry, entry)?;
    directory.next_entry += 1;
    unsafe {
        core::ptr::write_unaligned(address as *mut UserDirent, record);
    }
    Ok(1)
}

fn user_dirent(index: usize, entry: &DirectoryEntry) -> Result<UserDirent, i32> {
    if entry.name.len() >= 256 {
        return Err(ENAMETOOLONG);
    }
    let offset = i64::try_from(index.checked_add(1).ok_or(EOVERFLOW)?).map_err(|_| EOVERFLOW)?;
    let file_type = match entry.metadata.node_type {
        NodeType::File => 8,
        NodeType::Directory => 4,
        NodeType::Symlink => 10,
    };
    let mut record = UserDirent {
        inode: 0,
        offset,
        record_length: u16::try_from(core::mem::size_of::<UserDirent>()).map_err(|_| EOVERFLOW)?,
        file_type,
        name: [0; 256],
    };
    record.name[..entry.name.len()].copy_from_slice(entry.name.as_bytes());
    Ok(record)
}

fn syscall_stat(path_address: u64, stat_address: u64) -> Result<u64, i32> {
    syscall_stat_inner(path_address, stat_address, true)
}

fn syscall_lstat(path_address: u64, stat_address: u64) -> Result<u64, i32> {
    syscall_stat_inner(path_address, stat_address, false)
}

fn syscall_stat_inner(
    path_address: u64,
    stat_address: u64,
    follow_final_symlink: bool,
) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    syscall_stat_path(path, stat_address, follow_final_symlink)
}

fn syscall_fstatat(
    dirfd: u64,
    path_address: u64,
    stat_address: u64,
    flags: u64,
) -> Result<u64, i32> {
    if flags & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(EINVAL);
    }
    let path = resolve_at_path(dirfd, &copy_user_path(path_address)?)?;
    syscall_stat_path(path, stat_address, flags & AT_SYMLINK_NOFOLLOW == 0)
}

fn syscall_stat_path(
    path: String,
    stat_address: u64,
    follow_final_symlink: bool,
) -> Result<u64, i32> {
    paging::validate_user_range(stat_address, core::mem::size_of::<UserStat>(), true)
        .map_err(|_| EFAULT)?;
    let metadata = fs::with_vfs(|vfs| {
        if follow_final_symlink {
            vfs.metadata(&path)
        } else {
            vfs.symlink_metadata(&path)
        }
    })
    .map_err(errno_from_fs)?
    .map_err(errno_from_fs)?;
    let stat = UserStat {
        size: metadata.len as u64,
        mode: stat_mode(&path, metadata.node_type, metadata.permissions),
        modified_seconds: metadata
            .modified
            .and_then(|seconds| u32::try_from(seconds).ok())
            .unwrap_or(0),
    };
    unsafe {
        core::ptr::write_unaligned(stat_address as *mut UserStat, stat);
    }
    Ok(0)
}

fn syscall_mkdir(path_address: u64, mode: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    syscall_mkdir_path(path, mode)
}

fn syscall_mkdirat(dirfd: u64, path_address: u64, mode: u64) -> Result<u64, i32> {
    let path = resolve_at_path(dirfd, &copy_user_path(path_address)?)?;
    syscall_mkdir_path(path, mode)
}

fn syscall_mkdir_path(path: String, mode: u64) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    let mode = u16::try_from(mode).map_err(|_| EINVAL)?;
    if mode & !0o777 != 0 {
        return Err(EINVAL);
    }
    let mode = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        mode & !process.file_creation_mask
    };
    fs::with_vfs(|vfs| {
        vfs.create_dir(&path)?;
        if mode != 0o755 {
            apply_creation_mode(|| vfs.set_permissions(&path, mode))?;
        }
        Ok(())
    })
    .map_err(errno_from_fs)?
    .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_unlink(path_address: u64) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    fs::with_vfs(|vfs| vfs.remove_file(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    forget_shared_mapping_path(&path);
    Ok(0)
}

fn syscall_rmdir(path_address: u64) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    fs::with_vfs(|vfs| vfs.remove_dir(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_unlinkat(dirfd: u64, path_address: u64, flags: u64) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    if flags & !AT_REMOVEDIR != 0 {
        return Err(EINVAL);
    }
    let path = resolve_at_path(dirfd, &copy_user_path(path_address)?)?;
    fs::with_vfs(|vfs| {
        if flags & AT_REMOVEDIR != 0 {
            vfs.remove_dir(&path)
        } else {
            vfs.remove_file(&path)
        }
    })
    .map_err(errno_from_fs)?
    .map_err(errno_from_fs)?;
    if flags & AT_REMOVEDIR == 0 {
        forget_shared_mapping_path(&path);
    }
    Ok(0)
}

fn syscall_symlink(target_address: u64, link_address: u64) -> Result<u64, i32> {
    let target = copy_user_path(target_address)?;
    let link_path = resolve_process_path(&copy_user_path(link_address)?)?;
    syscall_symlink_path(target, link_path)
}

fn syscall_symlinkat(target_address: u64, dirfd: u64, link_address: u64) -> Result<u64, i32> {
    let target = copy_user_path(target_address)?;
    let link_path = resolve_at_path(dirfd, &copy_user_path(link_address)?)?;
    syscall_symlink_path(target, link_path)
}

fn syscall_symlink_path(target: String, link_path: String) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    fs::with_vfs(|vfs| vfs.symlink(&target, &link_path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_readlink(path_address: u64, buffer_address: u64, size: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    syscall_readlink_path(path, buffer_address, size)
}

fn syscall_readlinkat(
    dirfd: u64,
    path_address: u64,
    buffer_address: u64,
    size: u64,
) -> Result<u64, i32> {
    let path = resolve_at_path(dirfd, &copy_user_path(path_address)?)?;
    syscall_readlink_path(path, buffer_address, size)
}

fn syscall_readlink_path(path: String, buffer_address: u64, size: u64) -> Result<u64, i32> {
    let size = usize::try_from(size).map_err(|_| EINVAL)?;
    let target = fs::with_vfs(|vfs| vfs.read_link(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    let bytes = target.as_bytes();
    let count = bytes.len().min(size);
    if count > 0 {
        paging::validate_user_range(buffer_address, count, true).map_err(|_| EFAULT)?;
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer_address as *mut u8, count);
        }
    }
    u64::try_from(count).map_err(|_| EOVERFLOW)
}

fn syscall_ftruncate(fd: u64, length: u64) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let file = match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::File(file)) if file.lock().writable => Arc::clone(file),
        _ => return Err(EBADF),
    };
    let mut file = file.lock();
    fs::with_vfs(|vfs| vfs.truncate(&file.path, length))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    truncate_shared_mapping_pages(&file.path, length);
    if file.offset > length {
        file.offset = length;
    }
    Ok(0)
}

fn syscall_mmap(
    address: u64,
    length: u64,
    protection: u64,
    map_flags: u64,
    descriptor: u64,
    offset: u64,
) -> Result<u64, i32> {
    let (mapped_size, page_count) = mapping_size(length)?;
    let page_flags = mapping_flags(protection)?;
    let supported_flags = MAP_SHARED | MAP_PRIVATE | MAP_FIXED | MAP_ANONYMOUS;
    let map_type = map_flags & (MAP_SHARED | MAP_PRIVATE);
    if map_flags & !supported_flags != 0
        || (map_type != MAP_SHARED && map_type != MAP_PRIVATE)
        || offset % PAGE_SIZE != 0
    {
        return Err(EINVAL);
    }

    let shared = map_flags & MAP_SHARED != 0;
    let anonymous = map_flags & MAP_ANONYMOUS != 0;
    let (file_path, file_length, can_write, is_device) = if anonymous {
        if descriptor as i64 != -1 || offset != 0 {
            return Err(EINVAL);
        }
        (None, 0, true, false)
    } else {
        let fd = usize::try_from(descriptor).map_err(|_| EBADF)?;
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let file = match process.descriptors.get(fd).and_then(Option::as_ref) {
            Some(UserDescriptor::File(file)) => Arc::clone(file),
            _ => return Err(EBADF),
        };
        let file = file.lock();
        if !file.readable || (shared && protection & PROT_WRITE != 0 && !file.writable) {
            return Err(EACCES);
        }
        let path = file.path.clone();
        let can_write = !shared || file.writable;
        let is_device = fs::is_fb_path(&path);
        let process_terminal = process.terminal;
        drop(file);
        drop(process);
        if is_device && !device_access_allowed(&path, process_terminal, current_euid()?, current_capabilities()?) {
            return Err(EPERM);
        }
        let metadata = fs::with_vfs(|vfs| vfs.metadata(&path))
            .map_err(errno_from_fs)?
            .map_err(errno_from_fs)?;
        if metadata.node_type != NodeType::File {
            return Err(ENODEV);
        }
        (Some(path), metadata.len as u64, can_write, is_device)
    };
    if !anonymous {
        offset.checked_add(mapped_size).ok_or(EINVAL)?;
    }

    let memory = current_user_memory()?;
    if !USER_PROCESS.lock().active {
        return Err(EINTR);
    }
    let mut mappings = memory.mmap_pages.lock();
    let mut mmap_cursor = memory.mmap_cursor.lock();
    let fixed = map_flags & MAP_FIXED != 0;
    let hint = if address == 0 {
        None
    } else {
        Some(address & !(PAGE_SIZE - 1))
    };
    if fixed && (address == 0 || address % PAGE_SIZE != 0) {
        return Err(EINVAL);
    }

    let start = if let Some(hint) = hint.filter(|_| fixed) {
        validate_mapping_range(hint, mapped_size)?;
        replace_fixed_mapping_range(&mut mappings, hint, mapped_size)?;
        hint
    } else if let Some(hint) = hint.filter(|hint| {
        validate_mapping_range(*hint, mapped_size).is_ok()
            && mapping_range_is_free(*hint, mapped_size)
    }) {
        hint
    } else {
        find_free_mapping_range(*mmap_cursor, mapped_size)?
    };
    let end = start.checked_add(mapped_size).ok_or(ENOMEM)?;
    mappings.try_reserve(page_count).map_err(|_| ENOMEM)?;
    let initial_page_count = mappings.len();

    for index in 0..page_count {
        let page_address = start
            .checked_add((index as u64).checked_mul(PAGE_SIZE).ok_or(ENOMEM)?)
            .ok_or(ENOMEM)?;
        let file_offset = offset
            .checked_add((index as u64).checked_mul(PAGE_SIZE).ok_or(EINVAL)?)
            .ok_or(EINVAL)?;
        let backing = if shared && !is_device {
            match shared_mapping_page(file_path.as_deref(), file_offset, file_length) {
                Ok(backing) => Some(backing),
                Err(error) => {
                    let cleanup = release_mmap_pages_from(&mut mappings, initial_page_count);
                    return Err(if cleanup.is_err() { EIO } else { error });
                }
            }
        } else {
            None
        };
        let physical = if is_device {
            let fb = crate::graphics::Framebuffer::get().ok_or(ENODEV)?;
            let fb_phys = crate::memory::virtual_to_physical(fb.addr() as usize).ok_or(EIO)? as u64;
            fb_phys.checked_add(file_offset).ok_or(EINVAL)?
        } else if let Some(backing) = &backing {
            backing.physical
        } else {
            match allocate_mapping_frame(file_path.as_deref().map(|path| (path, file_offset))) {
                Ok((physical, _)) => physical,
                Err(error) => {
                    let cleanup = release_mmap_pages_from(&mut mappings, initial_page_count);
                    return Err(if cleanup.is_err() { EIO } else { error });
                }
            }
        };
        if let Err(error) = unsafe { paging::map_page(page_address, physical, page_flags) } {
            if backing.is_none() && !is_device {
                free_frame(physical as usize);
            }
            let cleanup = release_mmap_pages_from(&mut mappings, initial_page_count);
            return Err(if cleanup.is_err() {
                EIO
            } else {
                paging_errno(error)
            });
        }
        let locked = memory.lock_future_mappings.load(Ordering::Acquire);
        mappings.push(MappingPage {
            address: page_address,
            shared_backing: backing,
            locked,
            can_write,
            anonymous,
            is_device,
        });
    }

    mappings.sort_unstable_by_key(|page| page.address);
    *mmap_cursor = (*mmap_cursor).max(end);
    Ok(start)
}

fn syscall_munmap(address: u64, length: u64) -> Result<u64, i32> {
    if address == 0 || address % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    let (mapped_size, _) = mapping_size(length)?;
    validate_mapping_range(address, mapped_size)?;
    if !USER_PROCESS.lock().active {
        return Err(EINTR);
    }
    let memory = current_user_memory()?;
    unmap_mapping_range(&mut memory.mmap_pages.lock(), address, mapped_size)?;
    Ok(0)
}

fn syscall_mprotect(address: u64, length: u64, protection: u64) -> Result<u64, i32> {
    if address % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    let (mapped_size, page_count) = mapping_size(length)?;
    validate_user_mapping_range(address, mapped_size)?;
    let flags = mapping_flags(protection)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let memory = current_user_memory()?;
    let mappings = memory.mmap_pages.lock();
    for index in 0..page_count {
        let page = address + index as u64 * PAGE_SIZE;
        if paging::translate_address(page).is_err() {
            return Err(ENOMEM);
        }
        if protection & PROT_WRITE != 0
            && mappings
                .iter()
                .find(|mapped| mapped.address == page)
                .is_some_and(|mapped| !mapped.can_write)
        {
            return Err(EACCES);
        }
    }
    drop(process);
    for index in 0..page_count {
        let page = address + index as u64 * PAGE_SIZE;
        unsafe { paging::set_page_flags(page, flags) }.map_err(paging_errno)?;
    }
    Ok(0)
}

fn syscall_msync(address: u64, length: u64, flags: u64) -> Result<u64, i32> {
    if address % PAGE_SIZE != 0
        || flags & !(MS_ASYNC | MS_SYNC | MS_INVALIDATE) != 0
        || flags & (MS_ASYNC | MS_SYNC) == 0
        || flags & (MS_ASYNC | MS_SYNC) == (MS_ASYNC | MS_SYNC)
    {
        return Err(EINVAL);
    }
    let (mapped_size, page_count) = mapping_size(length)?;
    validate_user_mapping_range(address, mapped_size)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let memory = current_user_memory()?;
    let mappings = memory.mmap_pages.lock();
    for index in 0..page_count {
        let page_address = address + index as u64 * PAGE_SIZE;
        let page = mappings
            .iter()
            .find(|page| page.address == page_address)
            .ok_or(ENOMEM)?;
        if let Some(backing) = &page.shared_backing {
            flush_shared_mapping_page(backing).map_err(errno_from_fs)?;
        }
    }
    Ok(0)
}

fn syscall_mincore(address: u64, length: u64, vector_address: u64) -> Result<u64, i32> {
    if address % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    let (mapped_size, page_count) = mapping_size(length)?;
    validate_user_mapping_range(address, mapped_size)?;
    paging::validate_user_range(vector_address, page_count, true).map_err(|_| EFAULT)?;
    for index in 0..page_count {
        if paging::translate_address(address + index as u64 * PAGE_SIZE).is_err() {
            return Err(ENOMEM);
        }
    }
    let vector = unsafe { core::slice::from_raw_parts_mut(vector_address as *mut u8, page_count) };
    vector.fill(1);
    Ok(0)
}

fn syscall_mlock(address: u64, length: u64) -> Result<u64, i32> {
    set_mapping_lock(address, length, true)
}

fn syscall_munlock(address: u64, length: u64) -> Result<u64, i32> {
    set_mapping_lock(address, length, false)
}

fn set_mapping_lock(address: u64, length: u64, locked: bool) -> Result<u64, i32> {
    if address % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    let (mapped_size, page_count) = mapping_size(length)?;
    validate_user_mapping_range(address, mapped_size)?;
    for index in 0..page_count {
        if paging::translate_address(address + index as u64 * PAGE_SIZE).is_err() {
            return Err(ENOMEM);
        }
    }
    let memory = current_user_memory()?;
    if !USER_PROCESS.lock().active {
        return Err(EINTR);
    }
    for page in &mut *memory.mmap_pages.lock() {
        if page.address >= address && page.address < address + mapped_size {
            page.locked = locked;
        }
    }
    Ok(0)
}

fn syscall_mlockall(flags: u64) -> Result<u64, i32> {
    const MCL_CURRENT: u64 = 1;
    const MCL_FUTURE: u64 = 2;
    if flags == 0 || flags & !(MCL_CURRENT | MCL_FUTURE) != 0 {
        return Err(EINVAL);
    }
    let memory = current_user_memory()?;
    if !USER_PROCESS.lock().active {
        return Err(EINTR);
    }
    let mut mappings = memory.mmap_pages.lock();
    if flags & MCL_CURRENT != 0 {
        for page in &mut *mappings {
            page.locked = true;
        }
    }
    if flags & MCL_FUTURE != 0 {
        memory.lock_future_mappings.store(true, Ordering::Release);
    }
    Ok(0)
}

fn syscall_munlockall() -> Result<u64, i32> {
    let memory = current_user_memory()?;
    if !USER_PROCESS.lock().active {
        return Err(EINTR);
    }
    for page in &mut *memory.mmap_pages.lock() {
        page.locked = false;
    }
    memory.lock_future_mappings.store(false, Ordering::Release);
    Ok(0)
}

fn syscall_posix_madvise(address: u64, length: u64, advice: u64) -> Result<u64, i32> {
    if advice > 4 || address % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    if length == 0 {
        return Ok(0);
    }
    let (mapped_size, page_count) = mapping_size(length)?;
    validate_user_mapping_range(address, mapped_size)?;
    for index in 0..page_count {
        if paging::translate_address(address + index as u64 * PAGE_SIZE).is_err() {
            return Err(ENOMEM);
        }
    }
    Ok(0)
}

fn unmap_mapping_range(pages: &mut Vec<MappingPage>, address: u64, length: u64) -> Result<(), i32> {
    let end = address.checked_add(length).ok_or(EINVAL)?;
    for page in pages
        .iter()
        .filter(|page| page.address >= address && page.address < end)
    {
        if let Some(backing) = &page.shared_backing {
            flush_shared_mapping_page(backing).map_err(errno_from_fs)?;
        }
    }
    let mut index = 0;
    while index < pages.len() {
        if pages[index].address < address || pages[index].address >= end {
            index += 1;
            continue;
        }
        let physical = unsafe { paging::unmap_page(pages[index].address) }.map_err(|_| EIO)?;
        let page = pages.remove(index);
        if page.shared_backing.is_none() && !page.is_device {
            free_frame(physical as usize);
        }
    }
    Ok(())
}

fn replace_fixed_mapping_range(
    pages: &mut Vec<MappingPage>,
    address: u64,
    length: u64,
) -> Result<(), i32> {
    let end = address.checked_add(length).ok_or(EINVAL)?;
    let mut page_address = address;
    while page_address < end {
        if paging::translate_address(page_address).is_ok()
            && !pages.iter().any(|page| page.address == page_address)
        {
            return Err(ENOMEM);
        }
        page_address = page_address.checked_add(PAGE_SIZE).ok_or(EINVAL)?;
    }
    unmap_mapping_range(pages, address, length)
}

fn mapping_range_is_free(address: u64, length: u64) -> bool {
    let end = address.saturating_add(length);
    let mut page = address;
    while page < end {
        if paging::translate_address(page).is_ok() {
            return false;
        }
        page = match page.checked_add(PAGE_SIZE) {
            Some(page) => page,
            None => return false,
        };
    }
    true
}

fn find_free_mapping_range(cursor: u64, length: u64) -> Result<u64, i32> {
    let mut address = cursor.max(MMAP_START);
    while address
        .checked_add(length)
        .is_some_and(|end| end <= MMAP_END)
    {
        if mapping_range_is_free(address, length) {
            return Ok(address);
        }
        address = address.checked_add(PAGE_SIZE).ok_or(ENOMEM)?;
    }
    address = MMAP_START;
    while address
        .checked_add(length)
        .is_some_and(|end| end <= cursor.min(MMAP_END))
    {
        if mapping_range_is_free(address, length) {
            return Ok(address);
        }
        address = address.checked_add(PAGE_SIZE).ok_or(ENOMEM)?;
    }
    Err(ENOMEM)
}

fn mapping_size(length: u64) -> Result<(u64, usize), i32> {
    if length == 0 {
        return Err(EINVAL);
    }
    let mapped_size = length.checked_add(PAGE_SIZE - 1).ok_or(EINVAL)? & !(PAGE_SIZE - 1);
    let page_count = usize::try_from(mapped_size / PAGE_SIZE).map_err(|_| ENOMEM)?;
    Ok((mapped_size, page_count))
}

fn mapping_flags(protection: u64) -> Result<u64, i32> {
    if protection & !(PROT_READ | PROT_WRITE | PROT_EXEC) != 0 {
        return Err(EINVAL);
    }
    let mut flags = 0;
    if protection != 0 {
        flags |= paging::flags::USER;
    }
    if protection & PROT_WRITE != 0 {
        flags |= paging::flags::WRITABLE;
    }
    if protection & PROT_EXEC == 0 {
        flags |= paging::flags::NO_EXECUTE;
    }
    Ok(flags)
}

fn validate_mapping_range(address: u64, length: u64) -> Result<(), i32> {
    let end = address.checked_add(length).ok_or(EINVAL)?;
    if address < paging::USER_ADDRESS_MIN || end > MMAP_END {
        return Err(EINVAL);
    }
    Ok(())
}

fn validate_user_mapping_range(address: u64, length: u64) -> Result<(), i32> {
    let end = address.checked_add(length).ok_or(EINVAL)?;
    if address < paging::USER_ADDRESS_MIN || end > paging::USER_ADDRESS_MAX {
        return Err(EINVAL);
    }
    Ok(())
}

fn release_mmap_pages(mapped_pages: &mut Vec<MappingPage>) -> Result<(), PagingError> {
    let pages = core::mem::take(mapped_pages);
    let mut first_error = None;
    for page in pages {
        match unsafe { paging::unmap_page(page.address) } {
            Ok(physical) if page.shared_backing.is_none() => free_frame(physical as usize),
            Ok(_) => {}
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn release_mmap_pages_from(
    mapped_pages: &mut Vec<MappingPage>,
    first_page: usize,
) -> Result<(), i32> {
    while mapped_pages.len() > first_page {
        let address = mapped_pages.last().ok_or(EIO)?.address;
        let physical = unsafe { paging::unmap_page(address) }.map_err(|_| EIO)?;
        let page = mapped_pages.pop().ok_or(EIO)?;
        debug_assert_eq!(page.address, address);
        if page.shared_backing.is_none() && !page.is_device {
            free_frame(physical as usize);
        }
    }
    Ok(())
}

fn allocate_mapping_frame(file: Option<(&str, u64)>) -> Result<(u64, usize), i32> {
    let physical = allocate_frame().ok_or(ENOMEM)? as u64;
    let Some(virtual_address) = physical_to_virtual(physical as usize) else {
        free_frame(physical as usize);
        return Err(ENOMEM);
    };
    unsafe {
        core::ptr::write_bytes(virtual_address as *mut u8, 0, PAGE_SIZE as usize);
    }
    if let Some((path, offset)) = file {
        let buffer = unsafe {
            core::slice::from_raw_parts_mut(virtual_address as *mut u8, PAGE_SIZE as usize)
        };
        let file_offset = match usize::try_from(offset) {
            Ok(offset) => offset,
            Err(_) => {
                free_frame(physical as usize);
                return Err(EINVAL);
            }
        };
        let read_result = fs::with_vfs(|vfs| vfs.read(path, file_offset, buffer));
        match read_result {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                free_frame(physical as usize);
                return Err(errno_from_fs(error));
            }
            Err(error) => {
                free_frame(physical as usize);
                return Err(errno_from_fs(error));
            }
        }
    }
    Ok((physical, virtual_address))
}

fn shared_mapping_page(
    path: Option<&str>,
    file_offset: u64,
    file_length: u64,
) -> Result<Arc<SharedMmapPage>, i32> {
    if let Some(path) = path {
        let mut pages = SHARED_MMAP_PAGES.lock();
        pages.retain(|page| page.strong_count() > 0);
        if let Some(page) = pages.iter().filter_map(Weak::upgrade).find(|page| {
            page.path.lock().as_deref() == Some(path) && page.file_offset == file_offset
        }) {
            page.file_length.fetch_max(file_length, Ordering::Relaxed);
            return Ok(page);
        }
    }

    let file = path.map(|path| (path, file_offset));
    let (physical, _) = allocate_mapping_frame(file)?;
    let page = Arc::new(SharedMmapPage {
        physical,
        path: Mutex::new(path.map(String::from)),
        file_offset,
        file_length: AtomicU64::new(file_length),
    });
    if path.is_some() {
        SHARED_MMAP_PAGES.lock().push(Arc::downgrade(&page));
    }
    Ok(page)
}

fn flush_shared_mapping_page(page: &SharedMmapPage) -> Result<(), FsError> {
    let Some(path) = page.path.lock().clone() else {
        return Ok(());
    };
    let file_length = fs::with_vfs(|vfs| vfs.metadata(&path).map(|metadata| metadata.len))?? as u64;
    let end = page
        .file_offset
        .saturating_add(PAGE_SIZE)
        .min(page.file_length.load(Ordering::Relaxed))
        .min(file_length);
    if end <= page.file_offset {
        return Ok(());
    }
    let Some(virtual_address) = physical_to_virtual(page.physical as usize) else {
        return Err(FsError::IoError);
    };
    let size = usize::try_from(end - page.file_offset).map_err(|_| FsError::IoError)?;
    let bytes = unsafe { core::slice::from_raw_parts(virtual_address as *const u8, size) };
    let written = fs::with_vfs(|vfs| vfs.write(&path, page.file_offset as usize, bytes))??;
    if written == size {
        Ok(())
    } else {
        Err(FsError::IoError)
    }
}

fn flush_shared_mapping_pages(pages: &[MappingPage]) -> Result<(), FsError> {
    for page in pages {
        if let Some(backing) = &page.shared_backing {
            flush_shared_mapping_page(backing)?;
        }
    }
    Ok(())
}

fn shared_mapping_pages_snapshot() -> Vec<Arc<SharedMmapPage>> {
    let mut pages = SHARED_MMAP_PAGES.lock();
    let mut snapshot = Vec::new();
    pages.retain(|page| {
        if let Some(page) = page.upgrade() {
            snapshot.push(page);
            true
        } else {
            false
        }
    });
    snapshot
}

fn overlay_shared_mapping_pages(path: &str, offset: usize, buffer: &mut [u8]) {
    let end = offset.saturating_add(buffer.len());
    for page in shared_mapping_pages_snapshot() {
        if page.path.lock().as_deref() != Some(path) {
            continue;
        }
        let page_start = page.file_offset as usize;
        let page_end = page_start.saturating_add(PAGE_SIZE as usize);
        let overlap_start = offset.max(page_start);
        let overlap_end = end.min(page_end);
        if overlap_start >= overlap_end {
            continue;
        }
        let Some(physical_address) = physical_to_virtual(page.physical as usize) else {
            continue;
        };
        let source_offset = overlap_start - page_start;
        let destination_offset = overlap_start - offset;
        let count = overlap_end - overlap_start;
        unsafe {
            core::ptr::copy_nonoverlapping(
                (physical_address as *const u8).add(source_offset),
                buffer.as_mut_ptr().add(destination_offset),
                count,
            );
        }
    }
}

fn update_shared_mapping_pages(path: &str, offset: usize, bytes: &[u8]) {
    let end = offset.saturating_add(bytes.len());
    for page in shared_mapping_pages_snapshot() {
        if page.path.lock().as_deref() != Some(path) {
            continue;
        }
        let page_start = page.file_offset as usize;
        let page_end = page_start.saturating_add(PAGE_SIZE as usize);
        let overlap_start = offset.max(page_start);
        let overlap_end = end.min(page_end);
        if overlap_start >= overlap_end {
            continue;
        }
        let Some(physical_address) = physical_to_virtual(page.physical as usize) else {
            continue;
        };
        let source_offset = overlap_start - offset;
        let destination_offset = overlap_start - page_start;
        let count = overlap_end - overlap_start;
        page.file_length.fetch_max(end as u64, Ordering::Relaxed);
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr().add(source_offset),
                (physical_address as *mut u8).add(destination_offset),
                count,
            );
        }
    }
}

fn rewrite_shared_mapping_paths(source: &str, target: &str) {
    let source_prefix = alloc::format!("{source}/");
    for page in shared_mapping_pages_snapshot() {
        let mut path = page.path.lock();
        if path.as_deref() == Some(source) {
            *path = Some(String::from(target));
        } else if let Some(suffix) = path
            .as_deref()
            .and_then(|path| path.strip_prefix(&source_prefix))
        {
            *path = Some(alloc::format!("{target}/{suffix}"));
        }
    }
}

fn forget_shared_mapping_path(path: &str) {
    for page in shared_mapping_pages_snapshot() {
        let mut mapped_path = page.path.lock();
        if mapped_path.as_deref() == Some(path) {
            *mapped_path = None;
        }
    }
}

fn truncate_shared_mapping_pages(path: &str, length: usize) {
    let length = length as u64;
    for page in shared_mapping_pages_snapshot() {
        if page.path.lock().as_deref() != Some(path) {
            continue;
        }
        page.file_length.store(length, Ordering::Relaxed);
        let page_start = page.file_offset;
        let page_end = page_start.saturating_add(PAGE_SIZE);
        if length >= page_end {
            continue;
        }
        let Some(virtual_address) = physical_to_virtual(page.physical as usize) else {
            continue;
        };
        let clear_from = length.saturating_sub(page_start).min(PAGE_SIZE) as usize;
        unsafe {
            core::ptr::write_bytes(
                (virtual_address as *mut u8).add(clear_from),
                0,
                PAGE_SIZE as usize - clear_from,
            );
        }
    }
}

fn paging_errno(error: PagingError) -> i32 {
    match error {
        PagingError::AlreadyMapped => EEXIST,
        PagingError::OutOfFrames | PagingError::HhdmUnavailable => ENOMEM,
        _ => EINVAL,
    }
}

fn syscall_chdir(path_address: u64) -> Result<u64, i32> {
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    let metadata = fs::with_vfs(|vfs| vfs.metadata(&path))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    if metadata.node_type != NodeType::Directory {
        return Err(ENOTDIR);
    }
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    process.cwd = path;
    Ok(0)
}

fn syscall_chmod(path_address: u64, mode: u64) -> Result<u64, i32> {
    require_capability(CAP_VFS_WRITE)?;
    let mode = u16::try_from(mode).map_err(|_| EINVAL)?;
    if mode & !0o777 != 0 {
        return Err(EINVAL);
    }
    let path = resolve_process_path(&copy_user_path(path_address)?)?;
    fs::with_vfs(|vfs| vfs.set_permissions(&path, mode))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_fchmod(fd: u64, mode: u64) -> Result<u64, i32> {
    let mode = u16::try_from(mode).map_err(|_| EINVAL)?;
    if mode & !0o777 != 0 {
        return Err(EINVAL);
    }
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let path = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        match process.descriptors.get(fd).and_then(Option::as_ref) {
            Some(UserDescriptor::File(file)) => file.lock().path.clone(),
            Some(UserDescriptor::Directory(directory)) => directory.lock().path.clone(),
            Some(
                UserDescriptor::Pipe(_)
                | UserDescriptor::SocketPair(_)
                | UserDescriptor::Epoll(_)
                | UserDescriptor::EventFd(_)
                | UserDescriptor::TimerFd(_)
                | UserDescriptor::UdpSocket(_)
                | UserDescriptor::TcpSocket(_),
            ) => {
                return Err(ENOTSUP);
            }
            Some(
                UserDescriptor::ConsoleInput
                | UserDescriptor::ConsoleOutput
                | UserDescriptor::ConsoleError,
            ) => return Err(ENOTSUP),
            None => return Err(EBADF),
        }
    };
    fs::with_vfs(|vfs| vfs.set_permissions(&path, mode))
        .map_err(errno_from_fs)?
        .map_err(errno_from_fs)?;
    Ok(0)
}

fn syscall_getcwd(address: u64, size: u64) -> Result<u64, i32> {
    let size = usize::try_from(size).map_err(|_| EINVAL)?;
    if size == 0 {
        return Err(EINVAL);
    }
    let path = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        process.cwd.clone()
    };
    let needed = path.len().checked_add(1).ok_or(EINVAL)?;
    if size < needed {
        return Err(ERANGE);
    }
    paging::validate_user_range(address, needed, true).map_err(|_| EFAULT)?;
    unsafe {
        core::ptr::copy_nonoverlapping(path.as_ptr(), address as *mut u8, path.len());
        (address as *mut u8).add(path.len()).write(0);
    }
    Ok(needed as u64)
}

fn syscall_clock_gettime(clock_id: u64, address: u64) -> Result<u64, i32> {
    if !matches!(clock_id, 0 | 1 | 2 | 3 | 4 | 7) {
        return Err(EINVAL);
    }
    paging::validate_user_range(address, core::mem::size_of::<UserTimespec>(), true)
        .map_err(|_| EFAULT)?;
    let time = match clock_id {
        // CLOCK_REALTIME: RTC CMOS hanya punya resolusi satu detik.
        0 => UserTimespec {
            seconds: crate::drivers::rtc::unix_seconds().map_err(|_| EIO)?,
            nanoseconds: 0,
        },
        // CLOCK_MONOTONIC, PROCESS_CPUTIME, THREAD_CPUTIME, MONOTONIC_RAW,
        // BOOTTIME: semuanya memakai uptime TSC.
        1 | 2 | 3 | 4 | 7 => {
            let milliseconds = crate::arch::x86_64::monotonic_milliseconds();
            UserTimespec {
                seconds: (milliseconds / 1000) as i64,
                nanoseconds: ((milliseconds % 1000) * 1_000_000) as i64,
            }
        }
        _ => return Err(EINVAL),
    };
    unsafe { (address as *mut UserTimespec).write_unaligned(time) };
    Ok(0)
}

fn syscall_clock_getres(clock_id: u64, address: u64) -> Result<u64, i32> {
    if address == 0 {
        return Err(EFAULT);
    }
    paging::validate_user_range(address, core::mem::size_of::<UserTimespec>(), true)
        .map_err(|_| EFAULT)?;
    let resolution = match clock_id {
        0 => UserTimespec {
            seconds: 1,
            nanoseconds: 0,
        },
        1 | 2 | 3 | 4 | 7 => UserTimespec {
            seconds: 0,
            nanoseconds: 1_000_000,
        },
        _ => return Err(EINVAL),
    };
    unsafe { (address as *mut UserTimespec).write_unaligned(resolution) };
    Ok(0)
}

const GETRANDOM_MAX: usize = 256;
static RANDOM_STATE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_random_u64() -> u64 {
    let mut state = RANDOM_STATE.load(Ordering::Relaxed);
    loop {
        let current = state;
        let mut next = if current == 0 {
            crate::arch::x86_64::read_tsc() ^ 0x9E37_79B9_7F4A_7C15
        } else {
            current
        };
        next ^= next << 13;
        next ^= next >> 7;
        next ^= next << 17;
        if next == 0 {
            next = 0x9E37_79B9_7F4A_7C15;
        }
        match RANDOM_STATE.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(actual) => state = actual,
        }
    }
}

fn randomized_pie_load_bias() -> u64 {
    let span = (PIE_LOAD_BIAS_MAX - PIE_LOAD_BIAS) / PIE_LOAD_ALIGN;
    if span == 0 {
        PIE_LOAD_BIAS
    } else {
        PIE_LOAD_BIAS
            .checked_add((next_random_u64() % span) * PIE_LOAD_ALIGN)
            .unwrap_or(PIE_LOAD_BIAS)
            & !(PAGE_SIZE - 1)
    }
}

fn syscall_getrandom(buffer: u64, length: u64, _flags: u64) -> Result<u64, i32> {
    let length = usize::try_from(length).map_err(|_| EINVAL)?;
    let count = length.min(GETRANDOM_MAX);
    if count == 0 {
        return Ok(0);
    }
    paging::validate_user_range(buffer, count, true).map_err(|_| EFAULT)?;
    let mut offset = 0usize;
    while offset < count {
        let value = next_random_u64().to_le_bytes();
        let chunk = (count - offset).min(value.len());
        unsafe {
            core::ptr::copy_nonoverlapping(
                value.as_ptr(),
                (buffer as usize + offset) as *mut u8,
                chunk,
            );
        }
        offset += chunk;
    }
    Ok(count as u64)
}

fn syscall_fsync(fd: u64) -> Result<u64, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let descriptor = process
        .descriptors
        .get(fd)
        .and_then(Option::as_ref)
        .ok_or(EBADF)?;
    match descriptor {
        // POSIX menolak fsync pada pipe, console, dan socket.
        UserDescriptor::Pipe(_)
        | UserDescriptor::SocketPair(_)
        | UserDescriptor::Epoll(_)
        | UserDescriptor::EventFd(_)
        | UserDescriptor::TimerFd(_)
        | UserDescriptor::ConsoleInput
        | UserDescriptor::ConsoleOutput
        | UserDescriptor::ConsoleError
        | UserDescriptor::UdpSocket(_)
        | UserDescriptor::TcpSocket(_) => Err(EINVAL),
        // RAM filesystem tidak punya cache tulis yang perlu di-flush.
        UserDescriptor::File(_) | UserDescriptor::Directory(_) => Ok(0),
    }
}

// --- Implementasi syscall tambahan ---

fn syscall_uname(address: u64) -> Result<u64, i32> {
    paging::validate_user_range(address, core::mem::size_of::<Uname>(), true)
        .map_err(|_| EFAULT)?;
    unsafe { (address as *mut Uname).write_unaligned(Uname::new()) };
    Ok(0)
}

// brk: return current mmap_cursor sebagai "program break"
fn syscall_brk(addr: u64) -> Result<u64, i32> {
    let memory = current_user_memory()?;
    let cursor = *memory.mmap_cursor.lock();
    if addr == 0 {
        return Ok(cursor);
    }
    // Jika addr lebih besar, coba alokasi anonymous pages
    if addr > cursor {
        let pages = ((addr - cursor + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        let mut mappings = memory.mmap_pages.lock();
        let mut cur = *memory.mmap_cursor.lock();
        mappings.try_reserve(pages).map_err(|_| ENOMEM)?;
        for _ in 0..pages {
            let physical = match allocate_mapping_frame(None) {
                Ok((p, _)) => p,
                Err(_) => return Ok(cur),
            };
            let page_flags = mapping_flags(PROT_READ | PROT_WRITE).unwrap_or_default();
            if unsafe { paging::map_page(cur, physical, page_flags) }.is_err() {
                free_frame(physical as usize);
                return Ok(cur);
            }
            mappings.push(MappingPage {
                address: cur,
                shared_backing: None,
                locked: false,
                can_write: true,
                anonymous: true,
                is_device: false,
            });
            cur += PAGE_SIZE;
        }
        *memory.mmap_cursor.lock() = cur;
        return Ok(cur);
    }
    Ok(cursor)
}

fn syscall_gettimeofday(address: u64) -> Result<u64, i32> {
    if address == 0 { return Ok(0); }
    paging::validate_user_range(address, core::mem::size_of::<Timeval>(), true)
        .map_err(|_| EFAULT)?;
    let secs = crate::drivers::rtc::unix_seconds().map_err(|_| EIO)?;
    let tv = Timeval { tv_sec: secs, tv_usec: 0 };
    unsafe { (address as *mut Timeval).write_unaligned(tv) };
    Ok(0)
}

fn syscall_nanosleep(req: u64) -> Result<u64, i32> {
    if take_interrupted_syscall(SYSCALL_NANOSLEEP) {
        return Err(EINTR);
    }
    if req == 0 {
        return Err(EFAULT);
    }
    paging::validate_user_range(req, core::mem::size_of::<UserTimespec>(), false)
        .map_err(|_| EFAULT)?;
    let ts = unsafe { (req as *const UserTimespec).read_unaligned() };
    if ts.seconds < 0 || ts.nanoseconds < 0 || ts.nanoseconds >= 1_000_000_000 {
        return Err(EINVAL);
    }
    let requested = (ts.seconds as u64)
        .saturating_mul(1000)
        .saturating_add((ts.nanoseconds / 1_000_000) as u64);
    // Tidur berjalan di kernel dan belum scheduler-aware, jadi totalnya dibatasi
    // satu menit agar satu proses tidak membekukan proses lain terlalu lama.
    let milliseconds = requested.min(60_000);
    if milliseconds > 0 {
        crate::arch::x86_64::delay_milliseconds(milliseconds as u32);
    }
    Ok(0)
}

const POLLIN: i16 = 0x0001;
const POLLOUT: i16 = 0x0004;
const POLLERR: i16 = 0x0008;
const POLLHUP: i16 = 0x0010;
const POLLNVAL: i16 = 0x0020;
const SELECT_MAX_FDS: u64 = 1024;
const SELECT_FD_SET_WORDS: usize = (SELECT_MAX_FDS as usize) / 64;
const SOL_SOCKET: u64 = 1;
const SO_TYPE: u64 = 3;
const SO_ERROR: u64 = 4;
const SO_KEEPALIVE: u64 = 9;
const SOCK_STREAM_TYPE: i32 = 1;
const SOCK_DGRAM_TYPE: i32 = 2;

// Status siap satu descriptor, dihitung dari state yang benar-benar dimiliki
// kernel: isi pipe, baris TTY yang sudah lengkap, dan socket yang masih terbuka.
#[derive(Clone, Copy)]
struct PollState {
    readable: bool,
    writable: bool,
    hangup: bool,
    error: bool,
}

impl PollState {
    const IDLE: Self = Self {
        readable: false,
        writable: false,
        hangup: false,
        error: false,
    };
    const BOTH: Self = Self {
        readable: true,
        writable: true,
        hangup: false,
        error: false,
    };
}

fn descriptor_poll_state(process: &UserProcess, index: usize, fd: i32) -> Option<PollState> {
    if fd < 0 {
        return None;
    }
    let descriptor = process.descriptors.get(index).and_then(Option::as_ref)?;
    let state = match descriptor {
        UserDescriptor::ConsoleInput => PollState {
            readable: fs::tty_readable(process.terminal),
            ..PollState::IDLE
        },
        UserDescriptor::ConsoleOutput | UserDescriptor::ConsoleError => PollState {
            writable: true,
            ..PollState::IDLE
        },
        UserDescriptor::Directory(_) => PollState::BOTH,
        UserDescriptor::Pipe(endpoint) => {
            let pipe = endpoint.pipe.lock();
            PollState {
                readable: endpoint.readable && (pipe.length > 0 || pipe.writers == 0),
                writable: endpoint.writable
                    && (pipe.length < PIPE_CAPACITY || pipe.readers == 0),
                hangup: endpoint.readable && pipe.writers == 0,
                error: endpoint.writable && pipe.readers == 0,
            }
        }
        UserDescriptor::SocketPair(endpoint) => {
            let pipe = endpoint.read_end.pipe.lock();
            PollState {
                readable: endpoint.read_end.readable
                    && (pipe.length > 0 || pipe.writers == 0),
                writable: endpoint.write_end.writable
                    && (pipe.length < PIPE_CAPACITY || pipe.readers == 0),
                hangup: endpoint.read_end.readable && pipe.writers == 0,
                error: endpoint.write_end.writable && pipe.readers == 0,
            }
        }
        UserDescriptor::Epoll(_) => PollState::IDLE,
        UserDescriptor::EventFd(state) => {
            let event = state.lock();
            PollState {
                readable: event.counter > 0,
                writable: true,
                ..PollState::IDLE
            }
        }
        UserDescriptor::TimerFd(state) => {
            let timer = state.lock();
            let now = crate::arch::x86_64::monotonic_milliseconds();
            PollState {
                readable: timer
                    .next_expiry_ms
                    .is_some_and(|deadline| now >= deadline),
                writable: true,
                ..PollState::IDLE
            }
        }
        UserDescriptor::UdpSocket(socket) => PollState {
            readable: crate::network::udp_socket_readable(socket),
            writable: true,
            ..PollState::IDLE
        },
        UserDescriptor::TcpSocket(socket) => PollState {
            readable: crate::network::tcp_socket_readable(socket),
            writable: crate::network::tcp_socket_writable(socket),
            hangup: crate::network::tcp_is_closed(socket),
            error: crate::network::tcp_socket_error(socket).is_some(),
        },
        UserDescriptor::File(file) => {
            let path = file.lock().path.clone();
            match fs::tty_path_readable(&path) {
                Some(readable) => PollState {
                    readable,
                    writable: true,
                    ..PollState::IDLE
                },
                None => PollState::BOTH,
            }
        }
    };
    Some(state)
}

fn poll_revents(fd: i32, events: i16) -> i16 {
    if fd < 0 {
        return 0;
    }
    let process = USER_PROCESS.lock();
    if !process.active {
        return POLLERR;
    }
    let Ok(index) = usize::try_from(fd) else {
        return POLLNVAL;
    };
    let Some(state) = descriptor_poll_state(&process, index, fd) else {
        return POLLNVAL;
    };
    let mut revents = 0i16;
    if events & POLLIN != 0 && state.readable {
        revents |= POLLIN;
    }
    if events & POLLOUT != 0 && state.writable {
        revents |= POLLOUT;
    }
    if state.hangup {
        revents |= POLLHUP;
    }
    if state.error {
        revents |= POLLERR;
    }
    revents
}

fn poll_user_fds(fds_addr: u64, nfds: u64) -> Result<u64, i32> {
    if nfds > SELECT_MAX_FDS {
        return Err(EINVAL);
    }
    let size = (nfds as usize)
        .checked_mul(core::mem::size_of::<PollFd>())
        .ok_or(EINVAL)?;
    paging::validate_user_range(fds_addr, size, true).map_err(|_| EFAULT)?;
    let mut ready = 0u64;
    for index in 0..nfds as usize {
        let pointer = (fds_addr as usize + index * core::mem::size_of::<PollFd>()) as *mut PollFd;
        let mut descriptor = unsafe { pointer.read_unaligned() };
        descriptor.revents = poll_revents(descriptor.fd, descriptor.events);
        if descriptor.revents != 0 {
            ready += 1;
        }
        unsafe { pointer.write_unaligned(descriptor) };
    }
    Ok(ready)
}

fn wait_budget_milliseconds(timeout_ms: u64) -> u64 {
    if timeout_ms == u64::MAX {
        60_000
    } else {
        timeout_ms.min(60_000)
    }
}

/// Sleep while leaving interrupts enabled so keyboard and timer IRQs keep
/// running; poll/select must observe input that arrives during the wait.
fn sleep_milliseconds(milliseconds: u64) {
    if milliseconds == 0 {
        return;
    }
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) flags,
            options(nomem, nostack, preserves_flags));
    }
    let interrupts_were_enabled = flags & (1 << 9) != 0;
    if !interrupts_were_enabled {
        unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
    }
    crate::arch::x86_64::delay_milliseconds(milliseconds as u32);
    if !interrupts_were_enabled {
        unsafe { core::arch::asm!("cli", options(nomem, nostack)) };
    }
}

fn poll_deadline_for(timeout_ms: u64) -> u64 {
    let now = crate::arch::x86_64::monotonic_milliseconds();
    if timeout_ms == 0 {
        now
    } else {
        now.saturating_add(wait_budget_milliseconds(timeout_ms))
    }
}

/// Satu putaran `poll`: mengembalikan `EAGAIN` bila pemanggil harus menunggu.
fn syscall_poll(fds_addr: u64, nfds: u64, timeout_ms: u64) -> Result<u64, i32> {
    let deadline = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        match process.poll_deadline {
            Some(deadline) => deadline,
            None => {
                let deadline = poll_deadline_for(timeout_ms);
                process.poll_deadline = Some(deadline);
                deadline
            }
        }
    };
    let now = crate::arch::x86_64::monotonic_milliseconds();
    let ready = if nfds == 0 {
        0
    } else {
        poll_user_fds(fds_addr, nfds)?
    };
    if ready != 0 || timeout_ms == 0 || now >= deadline {
        USER_PROCESS.lock().poll_deadline = None;
        return Ok(ready);
    }
    Err(EAGAIN)
}

fn syscall_poll_frame(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    if take_interrupted_syscall(SYSCALL_POLL) {
        USER_PROCESS.lock().poll_deadline = None;
        frame.rax = (-(EINTR as i64)) as u64;
        return frame_pointer;
    }
    match syscall_poll(frame.rdi, frame.rsi, frame.rdx) {
        Ok(value) => {
            frame.rax = value;
            frame_pointer
        }
        Err(EAGAIN) => {
            let deadline =
                crate::arch::x86_64::monotonic_milliseconds().saturating_add(1);
            block_current_on_poll(frame, deadline)
        }
        Err(errno) => {
            frame.rax = (-(errno as i64)) as u64;
            frame_pointer
        }
    }
}

fn read_fd_set(address: u64, words: usize) -> Result<[u64; SELECT_FD_SET_WORDS], i32> {
    let mut set = [0u64; SELECT_FD_SET_WORDS];
    if address == 0 || words == 0 {
        return Ok(set);
    }
    let bytes = words * core::mem::size_of::<u64>();
    paging::validate_user_range(address, bytes, false).map_err(|_| EFAULT)?;
    for index in 0..words {
        set[index] = unsafe {
            ((address as usize + index * core::mem::size_of::<u64>()) as *const u64).read_unaligned()
        };
    }
    Ok(set)
}

fn write_fd_set(address: u64, set: &[u64; SELECT_FD_SET_WORDS], words: usize) -> Result<(), i32> {
    if address == 0 || words == 0 {
        return Ok(());
    }
    let bytes = words * core::mem::size_of::<u64>();
    paging::validate_user_range(address, bytes, true).map_err(|_| EFAULT)?;
    for index in 0..words {
        unsafe {
            ((address as usize + index * core::mem::size_of::<u64>()) as *mut u64)
                .write_unaligned(set[index]);
        }
    }
    Ok(())
}

fn timeval_milliseconds(timeout: u64) -> Result<u64, i32> {
    if timeout == 0 {
        return Ok(0);
    }
    paging::validate_user_range(timeout, core::mem::size_of::<Timeval>(), false)
        .map_err(|_| EFAULT)?;
    let tv = unsafe { (timeout as *const Timeval).read_unaligned() };
    if tv.tv_sec < 0 || tv.tv_usec < 0 {
        return Err(EINVAL);
    }
    Ok((tv.tv_sec as u64)
        .saturating_mul(1000)
        .saturating_add((tv.tv_usec / 1000) as u64))
}

fn select_scan(
    nfds: u64,
    read_set: &[u64; SELECT_FD_SET_WORDS],
    write_set: &[u64; SELECT_FD_SET_WORDS],
    except_set: &[u64; SELECT_FD_SET_WORDS],
    read_out: &mut [u64; SELECT_FD_SET_WORDS],
    write_out: &mut [u64; SELECT_FD_SET_WORDS],
    except_out: &mut [u64; SELECT_FD_SET_WORDS],
) -> u64 {
    *read_out = [0; SELECT_FD_SET_WORDS];
    *write_out = [0; SELECT_FD_SET_WORDS];
    *except_out = [0; SELECT_FD_SET_WORDS];
    let mut ready = 0u64;
    for fd in 0..nfds as usize {
        let word = fd / 64;
        let bit = 1u64 << (fd % 64);
        let wants_read = read_set[word] & bit != 0;
        let wants_write = write_set[word] & bit != 0;
        let wants_except = except_set[word] & bit != 0;
        if !wants_read && !wants_write && !wants_except {
            continue;
        }
        let mut events = 0i16;
        if wants_read {
            events |= POLLIN;
        }
        if wants_write {
            events |= POLLOUT;
        }
        let revents = poll_revents(fd as i32, events);
        let mut matched = false;
        if wants_read && revents & POLLIN != 0 {
            read_out[word] |= bit;
            matched = true;
        }
        if wants_write && revents & POLLOUT != 0 {
            write_out[word] |= bit;
            matched = true;
        }
        if wants_except && revents & (POLLERR | POLLHUP | POLLNVAL) != 0 {
            except_out[word] |= bit;
            matched = true;
        }
        if matched {
            ready += 1;
        }
    }
    ready
}

fn syscall_select(
    nfds: u64,
    readfds: u64,
    writefds: u64,
    exceptfds: u64,
    timeout: u64,
) -> Result<u64, i32> {
    if nfds > SELECT_MAX_FDS {
        return Err(EINVAL);
    }
    let timeout_ms = timeval_milliseconds(timeout)?;
    let deadline = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        match process.poll_deadline {
            Some(deadline) => deadline,
            None => {
                let deadline = poll_deadline_for(timeout_ms);
                process.poll_deadline = Some(deadline);
                deadline
            }
        }
    };
    let now = crate::arch::x86_64::monotonic_milliseconds();
    if nfds == 0 {
        if now >= deadline || timeout_ms == 0 {
            USER_PROCESS.lock().poll_deadline = None;
            return Ok(0);
        }
        return Err(EAGAIN);
    }
    let words = (nfds as usize).div_ceil(64);
    let read_set = read_fd_set(readfds, words)?;
    let write_set = read_fd_set(writefds, words)?;
    let except_set = read_fd_set(exceptfds, words)?;
    let mut read_out = [0u64; SELECT_FD_SET_WORDS];
    let mut write_out = [0u64; SELECT_FD_SET_WORDS];
    let mut except_out = [0u64; SELECT_FD_SET_WORDS];
    let ready = select_scan(
        nfds,
        &read_set,
        &write_set,
        &except_set,
        &mut read_out,
        &mut write_out,
        &mut except_out,
    );
    write_fd_set(readfds, &read_out, words)?;
    write_fd_set(writefds, &write_out, words)?;
    write_fd_set(exceptfds, &except_out, words)?;
    if ready != 0 || now >= deadline || timeout_ms == 0 {
        USER_PROCESS.lock().poll_deadline = None;
        return Ok(ready);
    }
    Err(EAGAIN)
}

fn syscall_select_frame(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    if take_interrupted_syscall(SYSCALL_SELECT) {
        USER_PROCESS.lock().poll_deadline = None;
        frame.rax = (-(EINTR as i64)) as u64;
        return frame_pointer;
    }
    match syscall_select(frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8) {
        Ok(value) => {
            frame.rax = value;
            frame_pointer
        }
        Err(EAGAIN) => {
            let deadline =
                crate::arch::x86_64::monotonic_milliseconds().saturating_add(1);
            block_current_on_poll(frame, deadline)
        }
        Err(errno) => {
            frame.rax = (-(errno as i64)) as u64;
            frame_pointer
        }
    }
}


fn socket_descriptor_type(fd: u64) -> Result<i32, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::UdpSocket(_)) => Ok(SOCK_DGRAM_TYPE),
        Some(UserDescriptor::TcpSocket(_)) => Ok(SOCK_STREAM_TYPE),
        Some(_) => Err(ENOTSOCK),
        None => Err(EBADF),
    }
}

fn syscall_setsockopt(
    fd: u64,
    level: u64,
    option_name: u64,
    option_value: u64,
    option_length: u64,
) -> Result<u64, i32> {
    socket_descriptor_type(fd)?;
    if level != SOL_SOCKET {
        return Err(ENOPROTOOPT);
    }
    if option_name == SO_KEEPALIVE {
        let size = core::mem::size_of::<i32>();
        if option_length < size as u64 {
            return Err(EINVAL);
        }
        paging::validate_user_range(option_value, size, false).map_err(|_| EFAULT)?;
        let value = unsafe { (option_value as *const i32).read_unaligned() };
        let socket = match tcp_socket_for_fd(fd) {
            Ok(socket) => socket,
            Err(errno) if errno == ENOTSOCK => return Err(ENOPROTOOPT),
            Err(errno) => return Err(errno),
        };
        crate::network::set_tcp_keepalive(&socket, value != 0);
        return Ok(0);
    }
    if option_length > 0 {
        if option_value == 0 {
            return Err(EFAULT);
        }
        let length = usize::try_from(option_length).map_err(|_| EINVAL)?;
        paging::validate_user_range(option_value, length, false).map_err(|_| EFAULT)?;
    }
    // Opsi socket diterima tetapi belum mengubah perilaku jaringan.
    Ok(0)
}

fn syscall_getsockopt(
    fd: u64,
    level: u64,
    option_name: u64,
    option_value: u64,
    option_length: u64,
) -> Result<u64, i32> {
    let socket_type = socket_descriptor_type(fd)?;
    if level != SOL_SOCKET {
        return Err(ENOPROTOOPT);
    }
    if option_value == 0 || option_length == 0 {
        return Err(EFAULT);
    }
    let length_size = core::mem::size_of::<u32>();
    paging::validate_user_range(option_length, length_size, true).map_err(|_| EFAULT)?;
    let capacity = unsafe { (option_length as *const u32).read_unaligned() } as usize;
    let value = match option_name {
        SO_TYPE => socket_type,
        SO_ERROR => 0,
        SO_KEEPALIVE => match tcp_socket_for_fd(fd) {
            Ok(socket) => crate::network::tcp_keepalive_enabled(&socket) as i32,
            Err(_) => return Err(ENOPROTOOPT),
        },
        _ => return Err(ENOPROTOOPT),
    };
    let needed = core::mem::size_of::<i32>();
    if capacity < needed {
        return Err(EINVAL);
    }
    paging::validate_user_range(option_value, needed, true).map_err(|_| EFAULT)?;
    unsafe {
        (option_value as *mut i32).write_unaligned(value);
        (option_length as *mut u32).write_unaligned(needed as u32);
    }
    Ok(0)
}

fn syscall_pipe2(pipefd: u64, flags: u64) -> Result<u64, i32> {
    const ALLOWED: u64 = O_NONBLOCK | O_CLOEXEC;
    if flags & !ALLOWED != 0 {
        return Err(EINVAL);
    }
    paging::validate_user_range(pipefd, core::mem::size_of::<[i32; 2]>(), true)
        .map_err(|_| EFAULT)?;
    let (read_end, write_end) = create_pipe()?;
    let (read_fd, write_fd) = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let UserProcess {
            descriptors,
            descriptor_flags,
            ..
        } = &mut *process;
        let fds = install_pipe_descriptors(descriptors, descriptor_flags, read_end, write_end)?;
        let stored = (flags & ALLOWED) as u32;
        descriptor_flags[fds.0] |= stored;
        descriptor_flags[fds.1] |= stored;
        fds
    };
    unsafe {
        (pipefd as *mut [i32; 2]).write_unaligned([read_fd as i32, write_fd as i32]);
    }
    Ok(0)
}

fn syscall_dup3(oldfd: u64, newfd: u64, _flags: u64) -> Result<u64, i32> {
    // Gunakan dup2 yang sudah ada
    dispatch_syscall(SYSCALL_DUP2, oldfd, newfd, 0, 0, 0, 0)
}

fn syscall_pread(fd: u64, buf: u64, count: u64, offset: u64) -> Result<u64, i32> {
    // lseek ke offset, baca, lseek kembali
    let old_pos = dispatch_syscall(SYSCALL_LSEEK, fd, 0, 1 /* SEEK_CUR */, 0, 0, 0)?;
    dispatch_syscall(SYSCALL_LSEEK, fd, offset, 0 /* SEEK_SET */, 0, 0, 0)?;
    let n = dispatch_syscall(SYSCALL_READ, fd, buf, count, 0, 0, 0);
    let _ = dispatch_syscall(SYSCALL_LSEEK, fd, old_pos, 0, 0, 0, 0);
    n
}

fn syscall_pwrite(fd: u64, buf: u64, count: u64, offset: u64) -> Result<u64, i32> {
    let old_pos = dispatch_syscall(SYSCALL_LSEEK, fd, 0, 1, 0, 0, 0)?;
    dispatch_syscall(SYSCALL_LSEEK, fd, offset, 0, 0, 0, 0)?;
    let n = dispatch_syscall(SYSCALL_WRITE, fd, buf, count, 0, 0, 0);
    let _ = dispatch_syscall(SYSCALL_LSEEK, fd, old_pos, 0, 0, 0, 0);
    n
}

fn syscall_writev(fd: u64, iov_addr: u64, iovcnt: u64) -> Result<u64, i32> {
    if iovcnt > 1024 { return Err(EINVAL); }
    let iov_size = (iovcnt as usize).checked_mul(core::mem::size_of::<IoVec>()).ok_or(EINVAL)?;
    paging::validate_user_range(iov_addr, iov_size, false).map_err(|_| EFAULT)?;
    let mut total = 0u64;
    for i in 0..iovcnt as usize {
        let iov = unsafe {
            &*((iov_addr as usize + i * core::mem::size_of::<IoVec>()) as *const IoVec)
        };
        if iov.len == 0 { continue; }
        let n = dispatch_syscall(SYSCALL_WRITE, fd, iov.base, iov.len, 0, 0, 0)?;
        total = total.saturating_add(n);
    }
    Ok(total)
}

fn syscall_readv(fd: u64, iov_addr: u64, iovcnt: u64) -> Result<u64, i32> {
    if iovcnt > 1024 { return Err(EINVAL); }
    let iov_size = (iovcnt as usize).checked_mul(core::mem::size_of::<IoVec>()).ok_or(EINVAL)?;
    paging::validate_user_range(iov_addr, iov_size, false).map_err(|_| EFAULT)?;
    let mut total = 0u64;
    for i in 0..iovcnt as usize {
        let iov = unsafe {
            &*((iov_addr as usize + i * core::mem::size_of::<IoVec>()) as *const IoVec)
        };
        if iov.len == 0 { continue; }
        let n = dispatch_syscall(SYSCALL_READ, fd, iov.base, iov.len, 0, 0, 0)?;
        total = total.saturating_add(n);
        if n < iov.len { break; } // EOF
    }
    Ok(total)
}

// sysconf: nilai mengikuti penomoran _SC_* gaya glibc
fn syscall_sysconf(name: u64) -> Result<u64, i32> {
    match name {
        0 => Ok(MAX_ARGUMENT_BYTES as u64),        // _SC_ARG_MAX
        1 => Ok(MAX_SCHEDULED_PROCESSES as u64),   // _SC_CHILD_MAX
        2 => Ok(100),                              // _SC_CLK_TCK
        3 => Ok(0),                                // _SC_NGROUPS_MAX
        4 => Ok(MAX_OPEN_FILES as u64),            // _SC_OPEN_MAX
        30 => Ok(PAGE_SIZE),                       // _SC_PAGESIZE / _SC_PAGE_SIZE
        83 => Ok(8),                               // _SC_NPROCESSORS_CONF
        84 => Ok(8),                               // _SC_NPROCESSORS_ONLN
        _ => Ok(u64::MAX),                         // tidak diketahui -> -1
    }
}

fn syscall_rlimit(syscall: u64, resource: u64, addr: u64) -> Result<u64, i32> {
    if syscall == SYSCALL_SETRLIMIT { return Ok(0); }
    // GETRLIMIT
    if addr == 0 { return Err(EFAULT); }
    paging::validate_user_range(addr, core::mem::size_of::<Rlimit>(), true)
        .map_err(|_| EFAULT)?;
    let rl = Rlimit { rlim_cur: RLIM_INFINITY, rlim_max: RLIM_INFINITY };
    unsafe { (addr as *mut Rlimit).write_unaligned(rl) };
    let _ = resource;
    Ok(0)
}

fn syscall_getrusage(_who: u64, addr: u64) -> Result<u64, i32> {
    if addr == 0 { return Err(EFAULT); }
    paging::validate_user_range(addr, core::mem::size_of::<Rusage>(), true)
        .map_err(|_| EFAULT)?;
    let cpu = current_cpu_milliseconds();
    let mut usage = Rusage::default();
    usage.ru_utime = [
        i64::try_from(cpu / 1000).unwrap_or(i64::MAX),
        i64::try_from((cpu % 1000) * 1000).unwrap_or(0),
    ];
    unsafe { (addr as *mut Rusage).write_unaligned(usage) };
    Ok(0)
}

fn syscall_times(addr: u64) -> Result<u64, i32> {
    let ticks = clock_ticks_from_milliseconds(current_cpu_milliseconds());
    if addr != 0 {
        paging::validate_user_range(addr, core::mem::size_of::<Tms>(), true)
            .map_err(|_| EFAULT)?;
        let mut times = Tms::default();
        times.tms_utime = ticks;
        unsafe { (addr as *mut Tms).write_unaligned(times) };
    }
    Ok(ticks as u64)
}

fn resolve_process_path(path: &str) -> Result<String, i32> {
    let cwd = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        process.cwd.clone()
    };
    resolve_user_path(&cwd, path).map_err(errno_from_fs)
}

fn resolve_at_path(dirfd: u64, path: &str) -> Result<String, i32> {
    if path.starts_with('/') {
        return resolve_process_path(path);
    }

    let cwd = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        if dirfd as i64 == AT_FDCWD {
            process.cwd.clone()
        } else {
            let fd = usize::try_from(dirfd).map_err(|_| EBADF)?;
            match process.descriptors.get(fd).and_then(Option::as_ref) {
                Some(UserDescriptor::Directory(directory)) => directory.lock().path.clone(),
                Some(_) => return Err(ENOTDIR),
                None => return Err(EBADF),
            }
        }
    };
    resolve_user_path(&cwd, path).map_err(errno_from_fs)
}

fn resolve_user_path(cwd: &str, path: &str) -> Result<String, FsError> {
    if path.is_empty() {
        return Err(FsError::InvalidPath);
    }
    if path.starts_with('/') {
        return fs::normalize_path(path);
    }

    let capacity = cwd
        .len()
        .checked_add(path.len())
        .and_then(|length| length.checked_add(1))
        .ok_or(FsError::NoSpace)?;
    let mut joined = String::new();
    joined.try_reserve(capacity).map_err(|_| FsError::NoSpace)?;
    joined.push_str(cwd);
    if !cwd.ends_with('/') {
        joined.push('/');
    }
    joined.push_str(path);
    fs::normalize_path(&joined)
}

fn copy_user_path(address: u64) -> Result<String, i32> {
    copy_user_string(address, MAX_USER_PATH)
}

fn copy_user_argument_vector(address: u64) -> Result<Vec<String>, i32> {
    if address == 0 {
        return Err(EFAULT);
    }
    let mut arguments = Vec::new();
    arguments
        .try_reserve_exact(MAX_ARGUMENTS)
        .map_err(|_| ENOMEM)?;
    let mut total_bytes = 0usize;
    for index in 0..=MAX_ARGUMENTS {
        let pointer_address = address
            .checked_add((index * core::mem::size_of::<u64>()) as u64)
            .ok_or(EFAULT)?;
        paging::validate_user_range(pointer_address, core::mem::size_of::<u64>(), false)
            .map_err(|_| EFAULT)?;
        let argument_address = unsafe { (pointer_address as *const u64).read_unaligned() };
        if argument_address == 0 {
            return Ok(arguments);
        }
        if index == MAX_ARGUMENTS {
            return Err(E2BIG);
        }

        let argument = copy_user_string(argument_address, MAX_ARGUMENT_BYTES)
            .map_err(|error| if error == ENAMETOOLONG { E2BIG } else { error })?;
        total_bytes = total_bytes
            .checked_add(argument.len().checked_add(1).ok_or(E2BIG)?)
            .filter(|length| *length <= MAX_ARGUMENT_BYTES)
            .ok_or(E2BIG)?;
        arguments.push(argument);
    }
    Err(E2BIG)
}

fn copy_user_environment(address: u64) -> Result<Vec<String>, i32> {
    if address == 0 {
        return Err(EFAULT);
    }
    let mut environment = Vec::new();
    environment
        .try_reserve_exact(MAX_ENVIRONMENT_VARIABLES)
        .map_err(|_| ENOMEM)?;
    let mut total_bytes = 0usize;
    for index in 0..=MAX_ENVIRONMENT_VARIABLES {
        let pointer_address = address
            .checked_add((index * core::mem::size_of::<u64>()) as u64)
            .ok_or(EFAULT)?;
        paging::validate_user_range(pointer_address, core::mem::size_of::<u64>(), false)
            .map_err(|_| EFAULT)?;
        let entry_address = unsafe { (pointer_address as *const u64).read_unaligned() };
        if entry_address == 0 {
            return Ok(environment);
        }
        if index == MAX_ENVIRONMENT_VARIABLES {
            return Err(E2BIG);
        }

        let entry = copy_user_string(entry_address, MAX_ENVIRONMENT_BYTES)?;
        let (key, _) = entry.split_once('=').ok_or(EINVAL)?;
        if !valid_environment_key(key) {
            return Err(EINVAL);
        }
        total_bytes = total_bytes
            .checked_add(entry.len().checked_add(1).ok_or(E2BIG)?)
            .filter(|length| *length <= MAX_ENVIRONMENT_BYTES)
            .ok_or(E2BIG)?;
        environment.push(entry);
    }
    Err(E2BIG)
}

fn copy_user_string(address: u64, maximum_length: usize) -> Result<String, i32> {
    if address == 0 {
        return Err(EFAULT);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve(maximum_length.min(64))
        .map_err(|_| ENOMEM)?;
    for index in 0..maximum_length {
        let current = address.checked_add(index as u64).ok_or(EFAULT)?;
        paging::validate_user_range(current, 1, false).map_err(|_| EFAULT)?;
        let byte = unsafe { (current as *const u8).read() };
        if byte == 0 {
            return String::from_utf8(bytes).map_err(|_| EILSEQ);
        }
        if bytes.len() == bytes.capacity() {
            bytes.try_reserve(1).map_err(|_| ENOMEM)?;
        }
        bytes.push(byte);
    }
    Err(ENAMETOOLONG)
}

fn errno_from_fs(error: FsError) -> i32 {
    match error {
        FsError::NotFound => ENOENT,
        FsError::AlreadyExists => EEXIST,
        FsError::NotDirectory => ENOTDIR,
        FsError::IsDirectory => EISDIR,
        FsError::InvalidPath => EINVAL,
        FsError::NotSymlink => EINVAL,
        FsError::PermissionDenied => EACCES,
        FsError::InvalidData => EILSEQ,
        FsError::NoSpace => ENOSPC,
        FsError::ReadOnly => EROFS,
        FsError::DirectoryNotEmpty => ENOTEMPTY,
        FsError::Busy => EBUSY,
        FsError::SymlinkLoop => ELOOP,
        FsError::Unsupported => ENOSYS,
        _ => EIO,
    }
}

fn apply_creation_mode(
    set_permissions: impl FnOnce() -> Result<(), FsError>,
) -> Result<(), FsError> {
    match set_permissions() {
        Err(FsError::Unsupported) => Ok(()),
        result => result,
    }
}

fn stat_mode(path: &str, node_type: NodeType, permissions: u16) -> u32 {
    let node_mode = if fs::is_tty_path(path) {
        0o020000
    } else {
        match node_type {
            NodeType::File => 0o100000,
            NodeType::Directory => 0o040000,
            NodeType::Symlink => 0o120000,
        }
    };
    node_mode | u32::from(permissions & 0o777)
}

/// Rentang prioritas nice POSIX: makin kecil nilainya, makin tinggi prioritasnya.
const PRIORITY_MIN: i32 = -20;
const PRIORITY_MAX: i32 = 19;
/// Boost aging maksimum; cukup untuk mengalahkan seluruh rentang nice.
const MAX_AGING_BOOST: u32 = (PRIORITY_MAX - PRIORITY_MIN) as u32;
/// Umur tunggu yang dianggap berpotensi kelaparan (dua kali boost aging).
const STARVATION_WATCHDOG_AGE: u32 = MAX_AGING_BOOST * 2;
/// Keterlambatan bangun dari `hlt` (ms) sebelum watchdog idle melapor.
pub(crate) const IDLE_STALL_WATCHDOG_MS: u64 = 250;

/// True bila proses runnable sudah terlalu lama tidak dijadwalkan.
fn starvation_watchdog_trips(age: u32) -> bool {
    age >= STARVATION_WATCHDOG_AGE
}

/// Ambang starvasi berbasis waktu nyata (ms): task runnable yang menunggu
/// selama ini dianggap berpotensi kelaparan.
const STARVATION_WATCHDOG_MILLISECONDS: u64 = 500;
/// `CLK_TCK` yang dipakai `times()`/`getrusage()` (jam 100 Hz).
const CLOCK_TICKS_PER_SECOND: u64 = 100;

/// True bila task runnable sudah menunggu cukup lama dalam waktu nyata.
fn starvation_watchdog_trips_wall_clock(wait_milliseconds: u64) -> bool {
    wait_milliseconds >= STARVATION_WATCHDOG_MILLISECONDS
}

/// Ubah milidetik menjadi `clock_t` (`CLK_TCK` = 100).
fn clock_ticks_from_milliseconds(milliseconds: u64) -> i64 {
    let ticks = milliseconds.saturating_mul(CLOCK_TICKS_PER_SECOND) / 1000;
    i64::try_from(ticks).unwrap_or(i64::MAX)
}

/// Akuntansi waktu per task: waktu CPU saat dijadwalkan dan waktu tunggu saat
/// runnable tetapi tidak dipilih.
#[derive(Default, Clone, Copy)]
struct TaskRuntime {
    cpu_milliseconds: u64,
    wait_milliseconds: u64,
}

impl TaskRuntime {
    /// Tambahkan satu slot waktu timer ke task ini.
    fn account_tick(&mut self, elapsed_milliseconds: u64, running: bool) {
        if running {
            self.cpu_milliseconds = self
                .cpu_milliseconds
                .saturating_add(elapsed_milliseconds);
        } else {
            self.wait_milliseconds = self
                .wait_milliseconds
                .saturating_add(elapsed_milliseconds);
        }
    }

    /// Dipilih scheduler: waktu tunggu saat ini selesai.
    fn on_selected(&mut self) {
        self.wait_milliseconds = 0;
    }

    /// Persentase waktu CPU terhadap lama scheduler berjalan.
    fn cpu_percent(&self, elapsed_milliseconds: u64) -> u32 {
        if elapsed_milliseconds == 0 {
            return 0;
        }
        ((self.cpu_milliseconds.saturating_mul(100) / elapsed_milliseconds).min(100)) as u32
    }
}

fn clamp_priority(priority: i32) -> i32 {
    priority.clamp(PRIORITY_MIN, PRIORITY_MAX)
}

/// Prioritas efektif: prioritas dasar dikurangi boost aging, sehingga proses
/// yang lama tidak dijadwalkan naik prioritasnya secara bertahap.
fn effective_priority(priority: i32, age: u32) -> i32 {
    let boosted = clamp_priority(priority).saturating_sub(age.min(MAX_AGING_BOOST) as i32);
    boosted.max(PRIORITY_MIN)
}

fn is_rt_policy(policy: u32) -> bool {
    policy == SCHED_FIFO as u32 || policy == SCHED_RR as u32
}

/// Kunci urut pemilihan; makin kecil makin dipilih. Task RT selalu di atas
/// `SCHED_OTHER`, di antara RT `rt_priority` lebih besar menang (tanpa aging),
/// dan di `SCHED_OTHER` nice efektif (dengan boost aging) menang.
fn scheduling_key(policy: u32, priority: i32, rt_priority: i32, age: u32) -> (u8, i32) {
    if is_rt_policy(policy) {
        (0, -rt_priority.clamp(RT_PRIORITY_MIN, RT_PRIORITY_MAX))
    } else {
        (1, effective_priority(priority, age))
    }
}

/// Pilih proses runnable dengan kunci terbaik. `hold_current` mempertahankan
/// proses saat ini selama tidak ada kunci yang lebih baik (`SCHED_FIFO` dan
/// `SCHED_RR` yang kuotanya belum habis); tanpa itu seri diputus oleh umur
/// tunggu terlama lalu urutan round-robin mulai dari proses saat ini.
fn next_runnable<K: Ord + Copy>(
    process_count: usize,
    current: usize,
    mut is_runnable: impl FnMut(usize) -> bool,
    mut key_of: impl FnMut(usize) -> (K, u32),
    hold_current: bool,
) -> Option<usize> {
    if process_count == 0 || current >= process_count {
        return None;
    }
    let mut best: Option<(usize, K, u32)> = None;
    if hold_current && is_runnable(current) {
        let (key, age) = key_of(current);
        best = Some((current, key, age));
    }
    for offset in 1..=process_count {
        let candidate = (current + offset) % process_count;
        if hold_current && candidate == current {
            continue;
        }
        if !is_runnable(candidate) {
            continue;
        }
        let (key, age) = key_of(candidate);
        let better = match &best {
            None => true,
            Some((_, best_key, best_age)) => {
                key < *best_key || (!hold_current && key == *best_key && age > *best_age)
            }
        };
        if better {
            best = Some((candidate, key, age));
        }
    }
    best.map(|(index, _, _)| index)
}

/// Pilih proses berikutnya menurut kebijakan/prioritas dan aging, lalu perbarui
/// umur tunggu setiap proses runnable (yang terpilih atau tidak runnable
/// kembali 0).
fn schedule_next(scheduler: &mut UserScheduler) -> Option<usize> {
    schedule_next_rotating(scheduler, false)
}

/// `force_rotate` memaksa rotasi meski proses saat ini memegang kebijakan yang
/// biasanya dipertahankan pada seri (dipakai saat kuota `SCHED_RR` habis).
fn schedule_next_rotating(scheduler: &mut UserScheduler, force_rotate: bool) -> Option<usize> {
    let current = scheduler.current;
    let hold_current = scheduler
        .processes
        .get(current)
        .filter(|process| is_process_runnable(process))
        .is_some_and(|process| match process.policy {
            policy if policy == SCHED_FIFO as u32 => true,
            policy if policy == SCHED_RR as u32 => !force_rotate,
            _ => false,
        });
    let chosen = next_runnable(
        scheduler.processes.len(),
        current,
        |index| is_process_runnable(&scheduler.processes[index]),
        |index| {
            let process = &scheduler.processes[index];
            (
                scheduling_key(
                    process.policy,
                    process.priority,
                    process.rt_priority,
                    process.age,
                ),
                process.age,
            )
        },
        hold_current,
    );
    if let Some(chosen) = chosen {
        if chosen != current || force_rotate {
            scheduler.processes[chosen].rt_slice = RT_RR_QUANTUM_TICKS;
        }
        scheduler.processes[chosen].runtime.on_selected();
        for (index, process) in scheduler.processes.iter_mut().enumerate() {
            if index == chosen || !is_process_runnable(process) {
                process.age = 0;
            } else {
                process.age = process.age.saturating_add(1);
            }
        }
    }
    chosen
}

fn is_process_runnable(process: &ScheduledProcess) -> bool {
    process.exit_status.is_none() && process.waiting_for.is_none()
}

/// Bagikan waktu tick terakhir: waktu CPU untuk task yang berjalan, waktu
/// tunggu untuk task runnable lain (task yang memblokir tidak dihitung).
fn account_runtime(scheduler: &mut UserScheduler, elapsed_milliseconds: u64) {
    if elapsed_milliseconds == 0 {
        return;
    }
    let current = scheduler.current;
    for (index, process) in scheduler.processes.iter_mut().enumerate() {
        if process.exit_status.is_some() || process.waiting_for.is_some() {
            continue;
        }
        process
            .runtime
            .account_tick(elapsed_milliseconds, index == current);
    }
}

/// Snapshot scheduler untuk shell `sched` dan pengawasan watchdog.
pub(crate) struct SchedulerStatus {
    pub pid: u64,
    pub tid: u64,
    pub priority: i32,
    pub policy: u32,
    pub rt_priority: i32,
    pub age: u32,
    pub cpu_milliseconds: u64,
    pub cpu_percent: u32,
    pub wait_milliseconds: u64,
    pub waiting: bool,
    pub exited: bool,
    pub is_thread: bool,
}

/// Salin status setiap task terjadwal tanpa memblokir scheduler lama.
pub(crate) fn scheduler_status() -> Vec<SchedulerStatus> {
    let scheduler = USER_SCHEDULER.lock();
    let mut entries = Vec::new();
    if entries.try_reserve(scheduler.processes.len()).is_err() {
        return entries;
    }
    let now = crate::arch::x86_64::monotonic_milliseconds();
    let elapsed = now.saturating_sub(scheduler.runtime_started_ms);
    for process in &scheduler.processes {
        entries.push(SchedulerStatus {
            pid: process.pid,
            tid: process.tid,
            priority: process.priority,
            policy: process.policy,
            rt_priority: process.rt_priority,
            age: process.age,
            cpu_milliseconds: process.runtime.cpu_milliseconds,
            cpu_percent: process.runtime.cpu_percent(elapsed),
            wait_milliseconds: process.runtime.wait_milliseconds,
            waiting: process.waiting_for.is_some(),
            exited: process.exit_status.is_some(),
            is_thread: process.is_thread,
        });
    }
    entries
}

pub(crate) fn scheduler_watchdog_tripped(age: u32) -> bool {
    starvation_watchdog_trips(age)
}

pub(crate) fn scheduler_watchdog_tripped_wall_clock(wait_milliseconds: u64) -> bool {
    starvation_watchdog_trips_wall_clock(wait_milliseconds)
}

/// Waktu CPU task saat ini dalam milidetik (dipakai `times()`/`getrusage()`).
fn current_cpu_milliseconds() -> u64 {
    let scheduler = USER_SCHEDULER.lock();
    scheduler
        .processes
        .get(scheduler.current)
        .filter(|_| scheduler.active)
        .map(|process| process.runtime.cpu_milliseconds)
        .unwrap_or(0)
}

pub fn terminate_user(frame: &mut InterruptFrame, status: u64) -> *mut InterruptFrame {
    let next_frame = terminate_user_inner(frame, status);
    restore_current_user_fpu();
    next_frame
}

fn terminate_user_inner(frame: &mut InterruptFrame, status: u64) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    if frame.stack_frame.code_segment & 3 != 3 {
        return frame_pointer;
    }

    let mut scheduler = USER_SCHEDULER.lock();
    if scheduler.active && !scheduler.processes.is_empty() {
        let current = scheduler.current;
        if current < scheduler.processes.len() {
            // Robust futex: tandai futex milik task ini sebelum address space-nya
            // dilepas, dengan address space-nya masih aktif.
            release_robust_list(&mut scheduler, current);
            let child_pid = scheduler.processes[current].pid;
            let parent_pid = scheduler.processes[current].parent_pid;
            for (index, process) in scheduler.processes.iter_mut().enumerate() {
                if index != current && process.pid == child_pid && process.exit_status.is_none() {
                    process.exit_status = Some(status);
                    process.reaped = true;
                    process.waiting_for = None;
                    process.wait_status_pointer = 0;
                    process.state.active = false;
                }
            }
            scheduler.processes[current].exit_status = Some(status);
            let mut active_process = USER_PROCESS.lock();
            core::mem::swap(
                &mut *active_process,
                &mut scheduler.processes[current].state,
            );
            scheduler.processes[current].state.active = false;
            while let Some(descriptor) = scheduler.processes[current].state.descriptors.pop() {
                let pipe_id = descriptor.as_ref().and_then(|descriptor| match descriptor {
                    UserDescriptor::Pipe(endpoint) => Some(endpoint.pipe.lock().id),
                    _ => None,
                });
                let socket_to_close = descriptor.as_ref().and_then(|descriptor| match descriptor {
                    UserDescriptor::TcpSocket(socket)
                        if crate::network::tcp_socket_is_last_descriptor(socket) =>
                    {
                        Some(Arc::clone(socket))
                    }
                    _ => None,
                });
                drop(descriptor);
                if let Some(pipe_id) = pipe_id {
                    wake_pipe_waiters_in_scheduler(&mut scheduler, pipe_id, true, true);
                }
                if let Some(socket) = socket_to_close {
                    crate::network::close_tcp_socket(&socket);
                }
            }

            if let Some(parent) = parent_pid.and_then(|pid| {
                scheduler.processes.iter().position(|process| {
                    process.pid == pid
                        && matches!(
                            process.waiting_for,
                            Some(ProcessWait::Child(target))
                                if wait_target_matches(target, child_pid)
                        )
                })
            }) {
                let status_pointer = scheduler.processes[parent].wait_status_pointer;
                scheduler.processes[parent].waiting_for = None;
                scheduler.processes[parent].wait_status_pointer = 0;
                core::mem::swap(&mut *active_process, &mut scheduler.processes[parent].state);
                scheduler.current = parent;
                let parent_process = &scheduler.processes[parent];
                let parent_frame = parent_process.frame_pointer as *mut InterruptFrame;
                unsafe {
                    gdt::set_ring0_stack_top(parent_process.kernel_stack_top);
                    parent_process.image.memory.address_space.activate();
                    (*parent_frame).rax = if status_pointer == 0 {
                        child_pid
                    } else {
                        match write_user_u64(status_pointer, status) {
                            Ok(()) => child_pid,
                            Err(error) => (-(error as i64)) as u64,
                        }
                    };
                }
                if status_pointer == 0
                    || paging::validate_user_range(
                        status_pointer,
                        core::mem::size_of::<u64>(),
                        true,
                    )
                    .is_ok()
                {
                    scheduler.processes[current].reaped = true;
                }
                return parent_frame;
            }

            if let Some(next) = schedule_next(&mut scheduler) {
                core::mem::swap(&mut *active_process, &mut scheduler.processes[next].state);
                scheduler.current = next;
                let next_process = &scheduler.processes[next];
                let next_frame = next_process.frame_pointer as *mut InterruptFrame;
                unsafe {
                    gdt::set_ring0_stack_top(next_process.kernel_stack_top);
                    next_process.image.memory.address_space.activate();
                }
                return next_frame;
            }
            scheduler.active = scheduler
                .processes
                .iter()
                .any(|process| process.exit_status.is_none());
            drop(active_process);
            drop(scheduler);
            gdt::record_user_exit(status);
            frame.stack_frame.instruction_pointer = gdt::user_exit_address();
            frame.stack_frame.code_segment = gdt::KERNEL_CODE_SELECTOR as u64;
            frame.stack_frame.stack_pointer = gdt::user_kernel_stack_top();
            frame.stack_frame.stack_segment = gdt::KERNEL_DATA_SELECTOR as u64;
            return frame_pointer;
        }
    }
    drop(scheduler);

    gdt::record_user_exit(status);
    frame.stack_frame.instruction_pointer = gdt::user_exit_address();
    frame.stack_frame.code_segment = gdt::KERNEL_CODE_SELECTOR as u64;
    frame.stack_frame.stack_pointer = gdt::user_kernel_stack_top();
    frame.stack_frame.stack_segment = gdt::KERNEL_DATA_SELECTOR as u64;
    frame_pointer
}

fn map_zeroed_page(
    virtual_address: u64,
    flags: u64,
    mapped_pages: &mut Vec<u64>,
) -> Result<(), ElfError> {
    let physical = allocate_frame().ok_or(ElfError::OutOfMemory)? as u64;
    if let Err(error) = unsafe { paging::map_page(virtual_address, physical, flags) } {
        free_frame(physical as usize);
        return Err(ElfError::Paging(error));
    }
    mapped_pages.push(virtual_address);
    Ok(())
}

fn copy_to_user_pages(address: u64, source: &[u8]) -> Result<(), ElfError> {
    let mut copied = 0;
    while copied < source.len() {
        let current = address
            .checked_add(copied as u64)
            .ok_or(ElfError::AddressRange)?;
        let physical = paging::translate_address(current)?;
        let destination = crate::memory::physical_to_virtual(physical as usize)
            .ok_or(PagingError::HhdmUnavailable)?;
        let page_remaining = (PAGE_SIZE - current % PAGE_SIZE) as usize;
        let chunk = (source.len() - copied).min(page_remaining);
        unsafe {
            core::ptr::copy_nonoverlapping(
                source.as_ptr().add(copied),
                destination as *mut u8,
                chunk,
            );
        }
        copied += chunk;
    }
    Ok(())
}

fn release_pages(mapped_pages: &mut Vec<u64>) -> Result<(), ElfError> {
    let mut first_error = None;
    for virtual_address in mapped_pages.drain(..) {
        match unsafe { paging::unmap_page(virtual_address) } {
            Ok(physical) => free_frame(physical as usize),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), |error| Err(ElfError::Paging(error)))
}

fn parse(bytes: &[u8]) -> Result<ParsedElf, ElfError> {
    if bytes.len() < ELF_HEADER_SIZE {
        return Err(ElfError::InvalidHeader);
    }
    if bytes.get(..4) != Some(b"\x7FELF") || bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 {
        return Err(ElfError::UnsupportedFormat);
    }
    let elf_type = read_u16(bytes, 16)?;
    if (elf_type != ELF_ET_EXEC && elf_type != ELF_ET_DYN)
        || read_u16(bytes, 18)? != 62
        || read_u32(bytes, 20)? != 1
        || read_u16(bytes, 52)? != ELF_HEADER_SIZE as u16
    {
        return Err(ElfError::UnsupportedFormat);
    }

    let entry = read_u64(bytes, 24)?;
    let ph_offset = usize::try_from(read_u64(bytes, 32)?).map_err(|_| ElfError::OutOfBounds)?;
    let ph_entry_size = read_u16(bytes, 54)? as usize;
    let ph_count = read_u16(bytes, 56)? as usize;
    if ph_offset < ELF_HEADER_SIZE
        || ph_entry_size < PROGRAM_HEADER_SIZE
        || ph_count == 0
        || ph_count > MAX_PROGRAM_HEADERS
    {
        return Err(ElfError::InvalidHeader);
    }
    let ph_bytes = ph_entry_size
        .checked_mul(ph_count)
        .ok_or(ElfError::OutOfBounds)?;
    let ph_end = ph_offset
        .checked_add(ph_bytes)
        .ok_or(ElfError::OutOfBounds)?;
    if ph_end > bytes.len() {
        return Err(ElfError::OutOfBounds);
    }

    let mut segments = Vec::new();
    segments
        .try_reserve_exact(ph_count)
        .map_err(|_| ElfError::OutOfMemory)?;
    let mut interpreter = None;
    for index in 0..ph_count {
        let offset = ph_offset + index * ph_entry_size;
        match read_u32(bytes, offset)? {
            PT_DYNAMIC => {}
            PT_INTERP => {
                if interpreter.is_some() {
                    return Err(ElfError::InvalidSegment);
                }
                let path_offset = usize::try_from(read_u64(bytes, offset + 8)?)
                    .map_err(|_| ElfError::OutOfBounds)?;
                let path_size = usize::try_from(read_u64(bytes, offset + 32)?)
                    .map_err(|_| ElfError::OutOfBounds)?;
                let path_end = path_offset
                    .checked_add(path_size)
                    .ok_or(ElfError::OutOfBounds)?;
                let path_bytes = bytes
                    .get(path_offset..path_end)
                    .ok_or(ElfError::OutOfBounds)?;
                if path_size < 2
                    || path_size > MAX_USER_PATH
                    || path_bytes.last() != Some(&0)
                    || path_bytes[..path_size - 1].contains(&0)
                    || path_bytes.first() != Some(&b'/')
                {
                    return Err(ElfError::InvalidSegment);
                }
                let path = core::str::from_utf8(&path_bytes[..path_size - 1])
                    .map_err(|_| ElfError::InvalidSegment)?;
                let mut owned_path = String::new();
                owned_path
                    .try_reserve_exact(path.len())
                    .map_err(|_| ElfError::OutOfMemory)?;
                owned_path.push_str(path);
                interpreter = Some(owned_path);
            }
            PT_LOAD => {
                let flags = read_u32(bytes, offset + 4)?;
                let file_offset = usize::try_from(read_u64(bytes, offset + 8)?)
                    .map_err(|_| ElfError::OutOfBounds)?;
                let virtual_address = read_u64(bytes, offset + 16)?;
                let file_size = usize::try_from(read_u64(bytes, offset + 32)?)
                    .map_err(|_| ElfError::OutOfBounds)?;
                let memory_size = read_u64(bytes, offset + 40)?;
                let alignment = read_u64(bytes, offset + 48)?;
                if memory_size == 0 && file_size == 0 {
                    continue;
                }
                if file_size as u64 > memory_size {
                    return Err(ElfError::InvalidSegment);
                }
                if flags & (PF_W | PF_X) == (PF_W | PF_X) {
                    return Err(ElfError::WritableExecutableSegment);
                }
                if alignment > 1
                    && (!alignment.is_power_of_two()
                        || virtual_address % alignment != (file_offset as u64) % alignment)
                {
                    return Err(ElfError::InvalidSegment);
                }
                let file_end = file_offset
                    .checked_add(file_size)
                    .ok_or(ElfError::OutOfBounds)?;
                if file_end > bytes.len() {
                    return Err(ElfError::OutOfBounds);
                }
                let memory_end = virtual_address
                    .checked_add(memory_size)
                    .ok_or(ElfError::AddressRange)?;
                if (elf_type == ELF_ET_EXEC && virtual_address < paging::USER_ADDRESS_MIN)
                    || memory_end > paging::USER_ADDRESS_MAX
                {
                    return Err(ElfError::AddressRange);
                }
                segments.push(LoadSegment {
                    file_offset,
                    virtual_address,
                    file_size,
                    memory_size,
                    flags,
                });
            }
            _ => {}
        }
    }
    if segments.is_empty() {
        return Err(ElfError::InvalidSegment);
    }
    let program_header_file_end = ph_offset
        .checked_add(ph_bytes)
        .ok_or(ElfError::OutOfBounds)?;
    let program_headers = segments
        .iter()
        .find_map(|segment| {
            let segment_file_end = segment.file_offset.checked_add(segment.file_size)?;
            (segment.file_offset <= ph_offset && program_header_file_end <= segment_file_end)
                .then(|| {
                    segment
                        .virtual_address
                        .checked_add((ph_offset - segment.file_offset) as u64)
                })
                .flatten()
        })
        .unwrap_or(0);
    if !segments.iter().any(|segment| {
        segment.flags & PF_X != 0
            && entry >= segment.virtual_address
            && entry < segment.virtual_address.saturating_add(segment.memory_size)
    }) {
        return Err(ElfError::InvalidEntryPoint);
    }
    Ok(ParsedElf {
        elf_type,
        entry,
        program_headers,
        program_header_size: ph_entry_size as u64,
        program_header_count: ph_count as u64,
        segments,
        interpreter,
    })
}

fn page_start(address: u64) -> u64 {
    address & !(PAGE_SIZE - 1)
}

fn page_end(address: u64, size: u64) -> Result<u64, ElfError> {
    let end = address.checked_add(size).ok_or(ElfError::AddressRange)?;
    end.checked_add(PAGE_SIZE - 1)
        .map(|aligned| aligned & !(PAGE_SIZE - 1))
        .ok_or(ElfError::AddressRange)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ElfError> {
    let end = offset.checked_add(2).ok_or(ElfError::OutOfBounds)?;
    let value: [u8; 2] = bytes
        .get(offset..end)
        .ok_or(ElfError::OutOfBounds)?
        .try_into()
        .map_err(|_| ElfError::OutOfBounds)?;
    Ok(u16::from_le_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ElfError> {
    let end = offset.checked_add(4).ok_or(ElfError::OutOfBounds)?;
    let value: [u8; 4] = bytes
        .get(offset..end)
        .ok_or(ElfError::OutOfBounds)?
        .try_into()
        .map_err(|_| ElfError::OutOfBounds)?;
    Ok(u32::from_le_bytes(value))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ElfError> {
    let end = offset.checked_add(8).ok_or(ElfError::OutOfBounds)?;
    let value: [u8; 8] = bytes
        .get(offset..end)
        .ok_or(ElfError::OutOfBounds)?
        .try_into()
        .map_err(|_| ElfError::OutOfBounds)?;
    Ok(u64::from_le_bytes(value))
}

#[cfg(test)]
mod tests {
    use super::{
        EACCES, EAGAIN, EEXIST, EILSEQ, EINTR, EINVAL, ENOENT, ENOSYS, EPERM,
        CAP_NETWORK, CAP_VFS_WRITE, CAP_SYS_ADMIN, device_access_allowed, EPIPE, PRIO_PGRP, PRIO_PROCESS, PRIO_USER,
        ProcessWait, capability_present, is_poll_wait, robust_word_after_exit,
        FUTEX_OWNER_DIED, FUTEX_WAITERS,
        effective_priority_target, priority_change_allowed, priority_target_matches,
        SCHED_OTHER, ElfError,
        MAP_ANONYMOUS, MAP_PRIVATE, MAP_SHARED, MMAP_END, MMAP_START, NodeType, PIPE_CAPACITY,
        PAGE_SIZE, PROT_EXEC, PROT_READ, PROT_WRITE, Pipe, STACK_PAGES, STACK_RANDOM_PAGES,
        STACK_TOP, SYSCALL_MMAP, SYSCALL_MUNMAP, UserDescriptor, signal_stack_top, stack_layout,
        MappingPage, UserDirent, WAIT_ANY_PID, apply_creation_mode, apply_load_bias,
        build_initial_stack, try_clone_mapping_pages,
        create_pipe, dispatch_syscall, errno_from_fs, install_pipe_descriptors, mapping_flags,
        mapping_size, next_runnable, paging, parse, resolve_user_path, stat_mode, user_dirent,
        validate_user_mapping_range,
        CAP_ALL, capabilities_for_uid, credential_change_allowed, effective_priority,
        MAX_AGING_BOOST, RT_PRIORITY_MAX, SCHED_FIFO, SCHED_RR, STARVATION_WATCHDOG_AGE,
        STARVATION_WATCHDOG_MILLISECONDS, TaskRuntime, clock_ticks_from_milliseconds,
        sched_policy_change_allowed,
        sched_priority_for, scheduler_policy_supported, scheduling_key,
        starvation_watchdog_trips, starvation_watchdog_trips_wall_clock,
        validate_mapping_range, wait_target_matches, futex_bitset_matches,
        FBIOGET_VSCREENINFO, FbFixScreeninfo, FbVarScreeninfo, SYSCALL_IOCTL, Winsize,
    };
    use crate::fs::{self, DirectoryEntry, FsError, Metadata};

    #[test]
    fn futex_wake_matches_only_intersecting_bitsets() {
        assert!(futex_bitset_matches(u32::MAX, 0x1));
        assert!(futex_bitset_matches(0x4, 0x6));
        assert!(!futex_bitset_matches(0x1, 0x2));
        assert!(!futex_bitset_matches(0, u32::MAX));
    }

    #[test]
    fn creation_mode_is_advisory_when_filesystem_lacks_permissions() {
        assert_eq!(apply_creation_mode(|| Err(FsError::Unsupported)), Ok(()));
        assert_eq!(
            apply_creation_mode(|| Err(FsError::IoError)),
            Err(FsError::IoError)
        );
    }

    #[test]
    fn signal_frames_prefer_the_alternate_stack() {
        assert_eq!(signal_stack_top(0x7000, None), 0x7000);
        assert_eq!(signal_stack_top(0x7000, Some((0x1000, 0x2000))), 0x3000);
        assert_eq!(signal_stack_top(0, Some((0x1000, 0))), 0x1000);
    }

    #[test]
    fn stack_layout_is_randomized_and_clear_of_the_mmap_region() {
        let mut tops = Vec::new();
        for pages in 0..STACK_RANDOM_PAGES {
            let (top, start, guard) = stack_layout(pages).expect("stack layout");
            assert_eq!(top, STACK_TOP - pages * PAGE_SIZE);
            assert_eq!(top - start, STACK_PAGES * PAGE_SIZE);
            assert_eq!(start - guard, PAGE_SIZE);
            assert!(guard > MMAP_END, "stack guard {guard:#x} menimpa mmap");
            tops.push(top);
        }
        tops.sort_unstable();
        tops.dedup();
        assert_eq!(tops.len() as u64, STACK_RANDOM_PAGES);
    }

    #[test]
    fn encodes_directory_entries_in_the_user_abi_layout() {
        let entry = DirectoryEntry {
            name: String::from("sample"),
            metadata: Metadata {
                node_type: NodeType::Directory,
                len: 0,
                permissions: 0o755,
                modified: None,
            },
        };
        let record = user_dirent(2, &entry).unwrap();

        assert_eq!(core::mem::size_of::<UserDirent>(), 280);
        assert_eq!(core::mem::offset_of!(UserDirent, name), 19);
        assert_eq!(record.offset, 3);
        assert_eq!(
            record.record_length as usize,
            core::mem::size_of::<UserDirent>()
        );
        assert_eq!(record.file_type, 4);
        assert_eq!(&record.name[..7], b"sample\0");
    }

    #[test]
    fn rejects_directory_entry_names_that_do_not_fit_the_abi() {
        let entry = DirectoryEntry {
            name: String::from_utf8(alloc::vec![b'x'; 256]).unwrap(),
            metadata: Metadata {
                node_type: NodeType::File,
                len: 0,
                permissions: 0o644,
                modified: None,
            },
        };

        assert!(matches!(
            user_dirent(0, &entry),
            Err(error) if error == super::ENAMETOOLONG
        ));
    }

    fn sample_elf() -> Vec<u8> {
        let mut bytes = alloc::vec![0; 0x1001];
        bytes[..16].copy_from_slice(b"\x7FELF\x02\x01\x01\0\0\0\0\0\0\0\0\0");
        put_u16(&mut bytes, 16, 2);
        put_u16(&mut bytes, 18, 62);
        put_u32(&mut bytes, 20, 1);
        put_u64(&mut bytes, 24, 0x400000);
        put_u64(&mut bytes, 32, 64);
        put_u16(&mut bytes, 52, 64);
        put_u16(&mut bytes, 54, 56);
        put_u16(&mut bytes, 56, 1);
        put_u32(&mut bytes, 64, 1);
        put_u32(&mut bytes, 68, 5);
        put_u64(&mut bytes, 72, 0x1000);
        put_u64(&mut bytes, 80, 0x400000);
        put_u64(&mut bytes, 96, 1);
        put_u64(&mut bytes, 104, 0x1000);
        put_u64(&mut bytes, 112, 0x1000);
        bytes[0x1000] = 0xC3;
        bytes
    }

    #[test]
    fn formats_printf_string_signed_and_hexadecimal_arguments() {
        let arguments = [24, 42, 1];
        let output = super::format_printf(
            "umur %d, hex %#x, %s\n",
            |index| arguments.get(index).copied().ok_or(EINVAL),
            |address, _| {
                if address == 1 {
                    Ok(alloc::string::String::from("siap"))
                } else {
                    Err(EINVAL)
                }
            },
        )
        .unwrap();
        assert_eq!(output, b"umur 24, hex 0x2a, siap\n");
    }

    #[test]
    fn formats_printf_width_and_rejects_unsupported_conversions() {
        let output = super::format_printf("%04d", |_| Ok(7), |_, _| Err(EINVAL)).unwrap();
        assert_eq!(output, b"0007");
        assert_eq!(
            super::format_printf("%.2f", |_| Ok(0), |_, _| Err(EINVAL)),
            Err(EINVAL)
        );
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn parses_static_executable_segment() {
        let parsed = parse(&sample_elf()).unwrap();
        assert_eq!(parsed.entry, 0x400000);
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].file_size, 1);
    }

    #[test]
    fn rejects_load_segments_outside_the_user_address_space() {
        let mut below = sample_elf();
        put_u64(&mut below, 80, paging::USER_ADDRESS_MIN - PAGE_SIZE);
        assert_eq!(parse(&below).err(), Some(ElfError::AddressRange));

        let mut above = sample_elf();
        put_u64(&mut above, 80, paging::USER_ADDRESS_MAX - PAGE_SIZE);
        put_u64(&mut above, 104, 2 * PAGE_SIZE);
        assert_eq!(parse(&above).err(), Some(ElfError::AddressRange));
    }

    #[test]
    fn rejects_malformed_load_segments() {
        let mut writable_executable = sample_elf();
        put_u32(&mut writable_executable, 68, 7);
        assert_eq!(
            parse(&writable_executable).err(),
            Some(ElfError::WritableExecutableSegment)
        );

        let mut misaligned = sample_elf();
        put_u64(&mut misaligned, 112, 3);
        assert_eq!(parse(&misaligned).err(), Some(ElfError::InvalidSegment));

        let mut oversized_file = sample_elf();
        put_u64(&mut oversized_file, 96, 0x2000);
        assert_eq!(parse(&oversized_file).err(), Some(ElfError::InvalidSegment));

        let mut truncated = sample_elf();
        put_u64(&mut truncated, 72, 0x1001);
        put_u64(&mut truncated, 112, 1);
        assert_eq!(parse(&truncated).err(), Some(ElfError::OutOfBounds));
    }

    #[test]
    fn mapping_size_rounds_pages_and_rejects_bad_lengths() {
        assert_eq!(mapping_size(0), Err(EINVAL));
        assert_eq!(mapping_size(PAGE_SIZE), Ok((PAGE_SIZE, 1)));
        assert_eq!(mapping_size(PAGE_SIZE + 1), Ok((2 * PAGE_SIZE, 2)));
        assert_eq!(mapping_size(u64::MAX), Err(EINVAL));
        assert_eq!(mapping_size(u64::MAX - PAGE_SIZE + 2), Err(EINVAL));
    }

    #[test]
    fn user_range_validation_rejects_overflow_and_non_user_addresses() {
        use crate::memory::paging::{PagingError, USER_ADDRESS_MAX, USER_ADDRESS_MIN};
        assert_eq!(paging::validate_user_range(0, 0, false), Ok(()));
        assert_eq!(
            paging::validate_user_range(USER_ADDRESS_MIN - 1, 8, false),
            Err(PagingError::UserAddressOutOfRange)
        );
        assert_eq!(
            paging::validate_user_range(USER_ADDRESS_MAX, 1, false),
            Err(PagingError::UserAddressOutOfRange)
        );
        assert_eq!(
            paging::validate_user_range(u64::MAX - 3, 8, true),
            Err(PagingError::AddressOverflow)
        );
        assert_eq!(
            validate_mapping_range(paging::USER_ADDRESS_MIN - 1, 8),
            Err(EINVAL)
        );
        assert_eq!(
            validate_mapping_range(MMAP_START, MMAP_END - MMAP_START + 1),
            Err(EINVAL)
        );
        assert_eq!(validate_user_mapping_range(0, 8), Err(EINVAL));
        assert_eq!(validate_user_mapping_range(MMAP_START, u64::MAX), Err(EINVAL));
    }

    #[test]
    fn applies_a_load_bias_to_a_position_independent_executable() {
        let mut bytes = sample_elf();
        put_u16(&mut bytes, 16, 3);
        put_u64(&mut bytes, 24, 0x10);
        put_u64(&mut bytes, 80, 0);
        let mut parsed = parse(&bytes).unwrap();
        apply_load_bias(&mut parsed, super::PIE_LOAD_BIAS).unwrap();
        assert_eq!(parsed.entry, super::PIE_LOAD_BIAS + 0x10);
        assert_eq!(parsed.segments[0].virtual_address, super::PIE_LOAD_BIAS);
    }

    #[test]
    fn randomized_pie_bias_is_page_aligned_and_bounded() {
        for _ in 0..16 {
            let bias = super::randomized_pie_load_bias();
            assert!(bias >= super::PIE_LOAD_BIAS);
            assert!(bias < super::PIE_LOAD_BIAS_MAX);
            assert_eq!(bias & (super::PAGE_SIZE - 1), 0);
        }
    }

    #[test]
    fn rejects_writable_executable_segments() {
        let mut bytes = sample_elf();
        put_u32(&mut bytes, 68, 7);
        assert!(matches!(
            parse(&bytes),
            Err(ElfError::WritableExecutableSegment)
        ));
    }

    #[test]
    fn rejects_truncated_segment_data() {
        let mut bytes = sample_elf();
        put_u64(&mut bytes, 96, 2);
        assert!(matches!(parse(&bytes), Err(ElfError::OutOfBounds)));
    }

    #[test]
    fn accepts_dynamic_segment_without_an_interpreter() {
        let mut bytes = sample_elf();
        put_u16(&mut bytes, 56, 2);
        put_u32(&mut bytes, 120, 2);
        assert!(parse(&bytes).is_ok());
    }

    #[test]
    fn reads_the_dynamic_interpreter_path() {
        let mut bytes = sample_elf();
        let interpreter = b"/lib/ld-octagram.so\0";
        bytes.resize(0x1000 + interpreter.len(), 0);
        put_u16(&mut bytes, 56, 2);
        put_u32(&mut bytes, 120, 3);
        put_u64(&mut bytes, 128, 0x1000);
        put_u64(&mut bytes, 152, interpreter.len() as u64);
        put_u64(&mut bytes, 160, interpreter.len() as u64);
        bytes[0x1000..].copy_from_slice(interpreter);
        assert_eq!(
            parse(&bytes).unwrap().interpreter.as_deref(),
            Some("/lib/ld-octagram.so")
        );
    }

    #[test]
    fn maps_filesystem_errors_and_unknown_syscalls() {
        assert_eq!(errno_from_fs(FsError::NotFound), ENOENT);
        assert_eq!(errno_from_fs(FsError::AlreadyExists), EEXIST);
        assert_eq!(errno_from_fs(FsError::PermissionDenied), EACCES);
        assert_eq!(errno_from_fs(FsError::InvalidData), EILSEQ);
        assert_eq!(dispatch_syscall(u64::MAX, 0, 0, 0, 0, 0, 0), Err(ENOSYS));
    }

    #[test]
    fn retired_non_posix_syscalls_are_unavailable() {
        for number in [10, 15, 18, 19, 23, 24, 26, 49] {
            assert_eq!(dispatch_syscall(number, 0, 0, 0, 0, 0, 0), Err(ENOSYS));
        }
    }

    #[test]
    fn validates_ioctl_and_framebuffer_path_recognition() {
        assert!(fs::is_fb_path("/dev/fb0"));
        assert!(fs::is_fb_path("/dev/fb"));
        assert!(!fs::is_fb_path("/dev/tty"));
        assert!(!fs::is_tty_path("/dev/fb0"));
        assert!(fs::is_tty_path("/dev/tty"));

        assert_eq!(core::mem::size_of::<FbVarScreeninfo>(), 160);
        assert_eq!(core::mem::size_of::<FbFixScreeninfo>(), 80);
        assert_eq!(core::mem::size_of::<Winsize>(), 8);

        // When no user process is active, syscall_ioctl safely returns EINTR
        assert_eq!(dispatch_syscall(SYSCALL_IOCTL, 0, FBIOGET_VSCREENINFO, 0, 0, 0, 0), Err(EINTR));
    }

    #[test]
    fn clock_gettime_accepts_realtime_and_rejects_unknown_clocks() {
        assert_eq!(dispatch_syscall(50, u64::MAX, 1, 0, 0, 0, 0), Err(EINVAL));
        assert_eq!(dispatch_syscall(50, 0, 0, 0, 0, 0, 0), Err(super::EFAULT));
    }

    #[test]
    fn validates_anonymous_mapping_size_and_protection() {
        assert_eq!(mapping_size(1), Ok((4096, 1)));
        assert_eq!(mapping_size(4097), Ok((8192, 2)));
        assert_eq!(mapping_size(0), Err(EINVAL));
        assert_eq!(mapping_size(u64::MAX), Err(EINVAL));

        assert!(mapping_flags(PROT_READ).is_ok());
        assert!(mapping_flags(PROT_READ | PROT_WRITE).is_ok());
        assert!(mapping_flags(PROT_READ | PROT_EXEC).is_ok());
        assert!(mapping_flags(PROT_WRITE).is_ok());
        assert!(mapping_flags(PROT_READ | PROT_WRITE | PROT_EXEC).is_ok());
        assert_eq!(mapping_flags(0), Ok(paging::flags::NO_EXECUTE));
    }

    #[test]
    fn mmap_syscalls_require_an_active_user_process() {
        assert_eq!(
            dispatch_syscall(
                SYSCALL_MMAP,
                0,
                4096,
                PROT_READ,
                MAP_PRIVATE | MAP_ANONYMOUS,
                u64::MAX,
                0,
            ),
            Err(EINTR)
        );
        assert_eq!(
            dispatch_syscall(SYSCALL_MUNMAP, MMAP_START, 4096, 0, 0, 0, 0),
            Err(EINTR)
        );
    }

    #[test]
    fn mmap_rejects_invalid_mapping_types_and_offsets() {
        assert_eq!(
            dispatch_syscall(SYSCALL_MMAP, 0, 4096, PROT_READ, 0, u64::MAX, 0),
            Err(EINVAL)
        );
        assert_eq!(
            dispatch_syscall(
                SYSCALL_MMAP,
                0,
                4096,
                PROT_READ,
                MAP_SHARED | MAP_PRIVATE,
                u64::MAX,
                0,
            ),
            Err(EINVAL)
        );
        assert_eq!(
            dispatch_syscall(
                SYSCALL_MMAP,
                0,
                4096,
                PROT_READ,
                MAP_PRIVATE | MAP_ANONYMOUS,
                u64::MAX,
                1,
            ),
            Err(EINVAL)
        );
    }

    #[test]
    fn mapping_family_rejects_invalid_flags_before_accessing_pages() {
        assert_eq!(
            dispatch_syscall(51, MMAP_START, 4096, 0, 0, 0, 0),
            Err(EINVAL)
        );
        assert_eq!(
            dispatch_syscall(52, MMAP_START, 4096, 8, 0, 0, 0),
            Err(EINVAL)
        );
        assert_eq!(
            dispatch_syscall(53, MMAP_START + 1, 4096, 0, 0, 0, 0),
            Err(EINVAL)
        );
        assert_eq!(dispatch_syscall(56, 0, 0, 0, 0, 0, 0), Err(EINVAL));
        assert_eq!(
            dispatch_syscall(58, MMAP_START, 4096, 5, 0, 0, 0),
            Err(EINVAL)
        );
    }

    #[test]
    fn validates_anonymous_mapping_user_address_range() {
        assert_eq!(validate_mapping_range(0x10000, 4096), Ok(()));
        assert_eq!(validate_mapping_range(MMAP_END - 4096, 4096), Ok(()));
        assert_eq!(validate_mapping_range(0xFFFF, 4096), Err(EINVAL));
        assert_eq!(validate_mapping_range(MMAP_END - 4095, 4096), Err(EINVAL));
    }

    #[test]
    fn resolves_process_relative_paths_without_escaping_vfs_root() {
        assert_eq!(
            resolve_user_path("/home/user", "../shared/data").unwrap(),
            "/home/shared/data"
        );
        assert_eq!(
            resolve_user_path("/home/user", "/etc/config").unwrap(),
            "/etc/config"
        );
        assert_eq!(
            resolve_user_path("/", "../../outside"),
            Err(FsError::InvalidPath)
        );
        assert_eq!(resolve_user_path("/", ""), Err(FsError::InvalidPath));
    }

    #[test]
    fn reports_devfs_tty_nodes_as_character_devices() {
        assert_eq!(stat_mode("/dev/tty", NodeType::File, 0o666), 0o020666);
        assert_eq!(stat_mode("/tmp/file", NodeType::File, 0o644), 0o100644);
    }

    #[test]
    fn builds_an_aligned_initial_stack_with_argv_and_empty_envp() {
        let arguments = alloc::vec![String::from("program"), String::from("two words")];
        let (stack_pointer, stack) =
            build_initial_stack(0x7000, 4096, &arguments, &[], &[(0, 0)]).unwrap();
        let word =
            |offset: usize| u64::from_le_bytes(stack[offset..offset + 8].try_into().unwrap());
        assert_eq!(stack_pointer % 16, 0);
        assert_eq!(word(0), 2);

        let first = (word(8) - stack_pointer) as usize;
        let second = (word(16) - stack_pointer) as usize;
        assert_eq!(&stack[first..first + 8], b"program\0");
        assert_eq!(&stack[second..second + 10], b"two words\0");
        assert_eq!(word(24), 0);
        assert_eq!(word(32), 0);
        assert_eq!(word(40), 0);
        assert_eq!(word(48), 0);
    }

    #[test]
    fn builds_initial_stack_with_environment_pointers() {
        let arguments = alloc::vec![String::from("program")];
        let environment = alloc::vec![String::from("PATH=/bin:/usr/bin"), String::from("HOME=/"),];
        let (stack_pointer, stack) =
            build_initial_stack(0x7000, 4096, &arguments, &environment, &[(0, 0)]).unwrap();
        let word =
            |offset: usize| u64::from_le_bytes(stack[offset..offset + 8].try_into().unwrap());
        assert_eq!(word(0), 1);
        assert_eq!(word(16), 0);

        let path = (word(24) - stack_pointer) as usize;
        let home = (word(32) - stack_pointer) as usize;
        assert_eq!(&stack[path..path + 19], b"PATH=/bin:/usr/bin\0");
        assert_eq!(&stack[home..home + 7], b"HOME=/\0");
        assert_eq!(word(40), 0);
        assert_eq!(word(48), 0);
        assert_eq!(word(56), 0);
    }

    #[test]
    fn rejects_invalid_environment_entries() {
        let arguments = alloc::vec![String::from("program")];
        let invalid = alloc::vec![String::from("=value")];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &arguments, &invalid, &[(0, 0)]),
            Err(ElfError::InvalidArguments)
        );
    }

    #[test]
    fn scheduler_selects_runnable_processes_in_round_robin_order() {
        let runnable = [true, true, false];
        let equal = |_| (0, 0);
        assert_eq!(
            next_runnable(runnable.len(), 0, |index| runnable[index], equal, false),
            Some(1)
        );
        assert_eq!(
            next_runnable(runnable.len(), 1, |index| runnable[index], equal, false),
            Some(0)
        );
        let runnable = [true, false, true];
        assert_eq!(
            next_runnable(runnable.len(), 0, |index| runnable[index], equal, false),
            Some(2)
        );
        assert_eq!(next_runnable(1, 0, |_| true, equal, false), Some(0));
        assert_eq!(next_runnable(0, 0, |_| true, equal, false), None);
        assert_eq!(next_runnable(2, 2, |_| true, equal, false), None);
    }

    #[test]
    fn scheduler_returns_none_when_every_process_has_exited() {
        let runnable = [false, false, false];
        assert_eq!(
            next_runnable(runnable.len(), 1, |index| runnable[index], |_| (0, 0), false),
            None
        );
    }

    #[test]
    fn scheduler_prefers_higher_nice_priority() {
        let priorities = [5, -10, 0];
        let ages = [0, 0, 0];
        assert_eq!(
            next_runnable(
                3,
                2,
                |_| true,
                |index| (scheduling_key(SCHED_OTHER as u32, priorities[index], 0, ages[index]), ages[index]),
                false
            ),
            Some(1)
        );
    }

    #[test]
    fn robust_list_marks_only_the_owner_as_dead() {
        let tid = 7;
        assert_eq!(robust_word_after_exit(7, tid), Some(FUTEX_OWNER_DIED));
        assert_eq!(
            robust_word_after_exit(7 | FUTEX_WAITERS, tid),
            Some(FUTEX_WAITERS | FUTEX_OWNER_DIED)
        );
        assert_eq!(robust_word_after_exit(FUTEX_OWNER_DIED, tid), None);
        assert_eq!(robust_word_after_exit(0, tid), None);
        assert_eq!(robust_word_after_exit(8, tid), None);
        assert_eq!(robust_word_after_exit(7, 0), None);
    }

    #[test]
    fn only_poll_waiters_are_woken_by_poll_events() {
        assert!(is_poll_wait(Some(ProcessWait::PollWait)));
        assert!(!is_poll_wait(None));
        assert!(!is_poll_wait(Some(ProcessWait::Sleep)));
        assert!(!is_poll_wait(Some(ProcessWait::ConsoleInput(0))));
        assert!(!is_poll_wait(Some(ProcessWait::PipeReadable(1))));
    }

    #[test]
    fn capability_checks_require_the_requested_bit() {
        assert_eq!(capability_present(CAP_ALL, CAP_VFS_WRITE), Ok(()));
        assert_eq!(capability_present(CAP_ALL, CAP_NETWORK), Ok(()));
        assert_eq!(capability_present(CAP_VFS_WRITE, CAP_NETWORK), Err(EPERM));
        assert_eq!(capability_present(0, CAP_VFS_WRITE), Err(EPERM));
    }

    #[test]
    fn priority_targets_cover_process_group_and_user() {
        assert_eq!(effective_priority_target(PRIO_PROCESS, 0, 7, 3, 0), Some(7));
        assert_eq!(effective_priority_target(PRIO_PROCESS, 9, 7, 3, 0), Some(9));
        assert_eq!(effective_priority_target(PRIO_PGRP, 0, 7, 3, 0), Some(3));
        assert_eq!(effective_priority_target(PRIO_PGRP, 12, 7, 3, 0), Some(12));
        assert_eq!(effective_priority_target(PRIO_USER, 0, 7, 3, 1000), Some(1000));
        assert_eq!(effective_priority_target(PRIO_USER, 42, 7, 3, 0), Some(42));
        assert_eq!(effective_priority_target(99, 0, 7, 3, 0), None);

        assert!(priority_target_matches(PRIO_PROCESS, 7, 7, 3, 0));
        assert!(!priority_target_matches(PRIO_PROCESS, 7, 8, 3, 0));
        assert!(priority_target_matches(PRIO_PGRP, 3, 8, 3, 0));
        assert!(!priority_target_matches(PRIO_PGRP, 3, 8, 4, 0));
        assert!(priority_target_matches(PRIO_USER, 1000, 8, 4, 1000));
        assert!(!priority_target_matches(PRIO_USER, 1000, 8, 4, 0));
    }

    #[test]
    fn priority_changes_need_matching_euid_or_capability() {
        // Root boleh menaikkan prioritas siapa pun.
        assert!(priority_change_allowed(0, CAP_ALL, 1000, 5, -5));
        // Non-root hanya untuk euid sendiri, dan hanya menurunkan prioritas.
        assert!(priority_change_allowed(1000, 0, 1000, 5, 10));
        assert!(priority_change_allowed(1000, 0, 1000, 5, 5));
        assert!(!priority_change_allowed(1000, 0, 1000, 5, 0));
        assert!(!priority_change_allowed(1000, 0, 1001, 5, 10));
        assert!(!priority_change_allowed(1000, 0, 1001, 5, 0));
    }

    #[test]
    fn scheduler_aging_prevents_starvation() {
        assert_eq!(effective_priority(19, 0), 19);
        assert_eq!(effective_priority(19, MAX_AGING_BOOST), -20);
        assert_eq!(effective_priority(-20, MAX_AGING_BOOST), -20);
        // Proses nice 19 yang sudah lama menunggu menyamai nice -20 dan menang
        // karena umur tunggunya lebih besar.
        assert_eq!(
            next_runnable(
                2,
                0,
                |_| true,
                |index| {
                    if index == 1 {
                        (
                            scheduling_key(SCHED_OTHER as u32, 19, 0, MAX_AGING_BOOST),
                            MAX_AGING_BOOST,
                        )
                    } else {
                        (scheduling_key(SCHED_OTHER as u32, -20, 0, 0), 0)
                    }
                },
                false
            ),
            Some(1)
        );
        assert_eq!(effective_priority(19, u32::MAX), -20);
    }

    #[test]
    fn fork_does_not_inherit_memory_locks() {
        let mut pages = Vec::new();
        pages.push(MappingPage {
            address: 0x40_0000,
            shared_backing: None,
            locked: true,
            can_write: true,
            anonymous: true,
            is_device: false,
        });
        let cloned = try_clone_mapping_pages(&pages).expect("clone mapping pages");
        assert_eq!(cloned.len(), 1);
        assert!(!cloned[0].locked, "kunci memori tidak boleh diwariskan");
        assert_eq!(cloned[0].address, 0x40_0000);
        assert!(cloned[0].can_write);
        assert!(cloned[0].anonymous);
    }

    #[test]
    fn fork_shared_pages_filter_includes_shared_backings_and_device_mappings() {
        let pages = [
            MappingPage {
                address: 0x40_0000,
                shared_backing: None,
                locked: false,
                can_write: true,
                anonymous: true,
                is_device: false,
            },
            MappingPage {
                address: 0x50_0000,
                shared_backing: Some(alloc::sync::Arc::new(super::SharedMmapPage {
                    physical: 0x1000,
                    path: spin::Mutex::new(None),
                    file_offset: 0,
                    file_length: core::sync::atomic::AtomicU64::new(0),
                })),
                locked: false,
                can_write: true,
                anonymous: true,
                is_device: false,
            },
            MappingPage {
                address: 0x60_0000,
                shared_backing: None,
                locked: false,
                can_write: true,
                anonymous: false,
                is_device: true,
            },
        ];
        let shared: alloc::vec::Vec<u64> = pages
            .iter()
            .filter(|page| page.shared_backing.is_some() || page.is_device)
            .map(|page| page.address)
            .collect();
        assert_eq!(shared, alloc::vec![0x50_0000, 0x60_0000]);
    }

    #[test]
    fn credentials_and_capabilities_follow_uid_rules() {
        assert_eq!(capabilities_for_uid(0), CAP_ALL);
        assert_eq!(capabilities_for_uid(1000), 0);
        // Root boleh mengubah ke id apa pun.
        assert!(credential_change_allowed(0, [0, 0], 1000));
        // Non-root hanya boleh kembali ke id nyata/efektifnya.
        assert!(credential_change_allowed(1000, [1000, 1001], 1000));
        assert!(credential_change_allowed(1000, [1000, 1001], 1001));
        assert!(!credential_change_allowed(1000, [1000, 1001], 0));
    }

    #[test]
    fn device_and_console_access_respects_credentials_and_capabilities() {
        // Root (euid == 0) bebas mengakses framebuffer dan terminal manapun.
        assert!(device_access_allowed("/dev/fb0", 0, 0, 0));
        assert!(device_access_allowed("/dev/tty2", 0, 0, 0));

        // Proses dengan CAP_SYS_ADMIN bebas mengakses perangkat dan terminal manapun.
        assert!(device_access_allowed("/dev/fb0", 0, 1000, CAP_SYS_ADMIN));
        assert!(device_access_allowed("/dev/tty2", 0, 1000, CAP_SYS_ADMIN));

        // Non-root (euid == 1000) tanpa CAP_SYS_ADMIN boleh mengakses terminalnya sendiri.
        assert!(device_access_allowed("/dev/tty0", 0, 1000, 0));
        assert!(device_access_allowed("/dev/tty1", 1, 1000, 0));

        // Non-root tanpa CAP_SYS_ADMIN DITOLAK mengakses framebuffer dan terminal lain.
        assert!(!device_access_allowed("/dev/fb0", 0, 1000, 0));
        assert!(!device_access_allowed("/dev/fb", 0, 1000, 0));
        assert!(!device_access_allowed("/dev/tty2", 0, 1000, 0));
        assert!(!device_access_allowed("/dev/tty3", 1, 1000, 0));
    }

    #[test]
    fn starvation_watchdog_trips_after_the_aging_budget() {
        assert!(!starvation_watchdog_trips(0));
        assert!(!starvation_watchdog_trips(STARVATION_WATCHDOG_AGE - 1));
        assert!(starvation_watchdog_trips(STARVATION_WATCHDOG_AGE));
        assert!(starvation_watchdog_trips(u32::MAX));
    }

    #[test]
    fn cpu_accounting_tracks_running_and_waiting_time() {
        let mut runtime = TaskRuntime::default();
        runtime.account_tick(10, true);
        runtime.account_tick(5, false);
        assert_eq!(runtime.cpu_milliseconds, 10);
        assert_eq!(runtime.wait_milliseconds, 5);
        // Terpilih: hitungan tunggu dimulai ulang, waktu CPU tetap.
        runtime.on_selected();
        assert_eq!(runtime.wait_milliseconds, 0);
        assert_eq!(runtime.cpu_milliseconds, 10);
        assert_eq!(runtime.cpu_percent(20), 50);
        assert_eq!(runtime.cpu_percent(5), 100);
        assert_eq!(runtime.cpu_percent(0), 0);
        runtime.account_tick(u64::MAX, true);
        assert_eq!(runtime.cpu_milliseconds, u64::MAX);
    }

    #[test]
    fn wall_clock_watchdog_and_clock_ticks_follow_the_thresholds() {
        assert!(!starvation_watchdog_trips_wall_clock(
            STARVATION_WATCHDOG_MILLISECONDS - 1
        ));
        assert!(starvation_watchdog_trips_wall_clock(
            STARVATION_WATCHDOG_MILLISECONDS
        ));
        assert_eq!(clock_ticks_from_milliseconds(0), 0);
        assert_eq!(clock_ticks_from_milliseconds(10), 1);
        assert_eq!(clock_ticks_from_milliseconds(999), 99);
        assert_eq!(clock_ticks_from_milliseconds(1000), 100);
        // Perkalian dijenuhkan lebih dulu, jadi hasil maksimum adalah MAX/1000.
        assert_eq!(
            clock_ticks_from_milliseconds(u64::MAX),
            (u64::MAX / 1000) as i64
        );
    }

    #[test]
    fn scheduler_policies_accept_rt_priorities() {
        assert!(scheduler_policy_supported(SCHED_OTHER));
        assert!(scheduler_policy_supported(SCHED_FIFO));
        assert!(scheduler_policy_supported(SCHED_RR));
        assert!(!scheduler_policy_supported(3));
        assert_eq!(sched_priority_for(SCHED_OTHER, 0), Ok(0));
        assert_eq!(sched_priority_for(SCHED_OTHER, 1), Err(EINVAL));
        assert_eq!(sched_priority_for(SCHED_FIFO, 1), Ok(1));
        assert_eq!(
            sched_priority_for(SCHED_RR, RT_PRIORITY_MAX),
            Ok(RT_PRIORITY_MAX)
        );
        assert_eq!(sched_priority_for(SCHED_FIFO, 0), Err(EINVAL));
        assert_eq!(
            sched_priority_for(SCHED_RR, RT_PRIORITY_MAX + 1),
            Err(EINVAL)
        );
        assert_eq!(sched_priority_for(3, 1), Err(EINVAL));
    }

    #[test]
    fn rt_tasks_outrank_sched_other_and_higher_rt_priority_wins() {
        let other = scheduling_key(SCHED_OTHER as u32, -20, 0, MAX_AGING_BOOST);
        let rt_low = scheduling_key(SCHED_FIFO as u32, 0, 10, 0);
        let rt_high = scheduling_key(SCHED_RR as u32, 0, 50, 0);
        assert!(rt_low < other, "task RT harus di atas SCHED_OTHER");
        assert!(rt_high < rt_low, "rt_priority lebih besar menang");
        // Aging tidak menaikkan task RT melewati RT lain yang lebih tinggi.
        assert!(scheduling_key(SCHED_FIFO as u32, 0, 10, MAX_AGING_BOOST) > rt_high);
        let keys = [other, rt_low, rt_high];
        assert_eq!(
            next_runnable(3, 0, |_| true, |index| (keys[index], 0), false),
            Some(2)
        );
    }

    #[test]
    fn fifo_holds_ties_while_rr_rotates() {
        let rt = scheduling_key(SCHED_FIFO as u32, 0, 10, 0);
        let keys = [rt, rt];
        // SCHED_FIFO: pemilik CPU dipertahankan pada seri.
        assert_eq!(
            next_runnable(2, 0, |_| true, |index| (keys[index], 0), true),
            Some(0)
        );
        // SCHED_RR (kuota habis): task berprioritas sama mengambil alih.
        assert_eq!(
            next_runnable(2, 0, |_| true, |index| (keys[index], 0), false),
            Some(1)
        );
        // Prioritas RT lebih tinggi tetap mempreempt SCHED_FIFO.
        let higher = scheduling_key(SCHED_FIFO as u32, 0, 40, 0);
        let keys = [rt, higher];
        assert_eq!(
            next_runnable(2, 0, |_| true, |index| (keys[index], 0), true),
            Some(1)
        );
        // SCHED_OTHER tidak pernah mengalahkan task RT.
        let other = scheduling_key(SCHED_OTHER as u32, -20, 0, MAX_AGING_BOOST);
        let keys = [rt, other];
        assert_eq!(
            next_runnable(2, 0, |_| true, |index| (keys[index], 0), true),
            Some(0)
        );
    }

    #[test]
    fn sched_policy_changes_need_capability_or_self_demotion() {
        assert!(sched_policy_change_allowed(SCHED_FIFO, 1000, CAP_ALL, 0));
        assert!(sched_policy_change_allowed(SCHED_OTHER, 1000, 0, 1000));
        assert!(!sched_policy_change_allowed(SCHED_FIFO, 1000, 0, 1000));
        assert!(!sched_policy_change_allowed(SCHED_OTHER, 1000, 0, 1001));
    }

    #[test]
    fn wait_target_matches_specific_or_any_child() {
        assert!(wait_target_matches(7, 7));
        assert!(!wait_target_matches(7, 8));
        assert!(wait_target_matches(WAIT_ANY_PID, 7));
    }

    #[test]
    fn pipe_handles_empty_eof_capacity_and_wrapped_data() {
        let mut buffer = Vec::new();
        buffer.resize(PIPE_CAPACITY, 0);
        let mut pipe = Pipe {
            id: 1,
            buffer,
            head: 0,
            length: 0,
            readers: 1,
            writers: 1,
        };
        let mut output = [0; 5];
        assert_eq!(pipe.read(&mut output), Err(EAGAIN));

        assert_eq!(pipe.write(b"abc"), Ok(3));
        assert_eq!(pipe.read(&mut output[..2]), Ok(2));
        assert_eq!(&output[..2], b"ab");
        assert_eq!(pipe.write(b"defg"), Ok(4));
        assert_eq!(pipe.read(&mut output), Ok(5));
        assert_eq!(&output, b"cdefg");

        let full = [1; PIPE_CAPACITY];
        assert_eq!(pipe.write(&full), Ok(PIPE_CAPACITY));
        assert_eq!(pipe.write(b"x"), Err(EAGAIN));
        pipe.readers = 0;
        assert_eq!(pipe.write(b"x"), Err(EPIPE));
        pipe.writers = 0;
        let mut drain = [0; PIPE_CAPACITY];
        assert_eq!(pipe.read(&mut drain), Ok(PIPE_CAPACITY));
        assert_eq!(pipe.read(&mut drain), Ok(0));
    }

    #[test]
    fn pipe_endpoints_receive_distinct_descriptor_slots() {
        let mut descriptors = Vec::new();
        let mut descriptor_flags = Vec::new();
        descriptors.extend([
            Some(UserDescriptor::ConsoleInput),
            Some(UserDescriptor::ConsoleOutput),
            Some(UserDescriptor::ConsoleError),
        ]);
        let (read_end, write_end) = create_pipe().unwrap();

        assert_eq!(
            install_pipe_descriptors(&mut descriptors, &mut descriptor_flags, read_end, write_end,),
            Ok((3, 4))
        );
        assert_eq!(descriptor_flags, [0; 5]);
        assert!(descriptors[3].as_ref().is_some_and(|descriptor| matches!(
            descriptor,
            UserDescriptor::Pipe(endpoint) if endpoint.readable && !endpoint.writable
        )));
        assert!(descriptors[4].as_ref().is_some_and(|descriptor| matches!(
            descriptor,
            UserDescriptor::Pipe(endpoint) if !endpoint.readable && endpoint.writable
        )));
    }

    #[test]
    fn rejects_arguments_with_nul_or_exceeding_limits() {
        let nul_argument = alloc::vec![String::from("bad\0argument")];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &nul_argument, &[], &[(0, 0)]),
            Err(ElfError::InvalidArguments)
        );

        let too_many = alloc::vec![String::from("arg"); 65];
        assert_eq!(
            build_initial_stack(0x7000, 4096, &too_many, &[], &[(0, 0)]),
            Err(ElfError::ArgumentsTooLarge)
        );
    }
}

// ==== epoll / eventfd / timerfd / socketpair / clock_nanosleep ====

const EPOLLIN: u32 = 0x0001;
const EPOLLOUT: u32 = 0x0004;
const EPOLLERR: u32 = 0x0008;
const EPOLLHUP: u32 = 0x0010;
const EPOLLET: u32 = 0x8000_0000;
const EPOLLONESHOT: u32 = 0x4000_0000;
const EPOLL_CTL_ADD: u64 = 1;
const EPOLL_CTL_DEL: u64 = 2;
const EPOLL_CTL_MOD: u64 = 3;
const EFD_SEMAPHORE: u64 = 1;
const TFD_TIMER_ABSTIME: u64 = 1;
const TIMER_ABSTIME: u64 = 1;
const AF_UNIX: u64 = 1;
const SOCK_STREAM: u64 = 1;
const SOCK_TYPE_MASK: u64 = 0xF;

fn install_descriptor(descriptor: UserDescriptor) -> Result<u64, i32> {
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let fd = reserve_descriptor_slot(&mut process.descriptors)?;
    let descriptor_count = process.descriptors.len();
    process.descriptor_flags.resize(descriptor_count, 0);
    process.descriptors[fd] = Some(descriptor);
    Ok(fd as u64)
}

fn timespec_to_milliseconds(ts: &UserTimespec) -> Result<u64, i32> {
    if ts.seconds < 0 || ts.nanoseconds < 0 || ts.nanoseconds >= 1_000_000_000 {
        return Err(EINVAL);
    }
    Ok((ts.seconds as u64)
        .saturating_mul(1000)
        .saturating_add((ts.nanoseconds / 1_000_000) as u64))
}

fn milliseconds_to_timespec(milliseconds: u64) -> UserTimespec {
    UserTimespec {
        seconds: (milliseconds / 1000) as i64,
        nanoseconds: ((milliseconds % 1000) * 1_000_000) as i64,
    }
}

fn timerfd_take_expirations(timer: &mut TimerFdState, now: u64) -> u64 {
    let Some(deadline) = timer.next_expiry_ms else {
        return 0;
    };
    if now < deadline {
        return 0;
    }
    if timer.interval_ms == 0 {
        timer.next_expiry_ms = None;
        return 1;
    }
    let elapsed = now - deadline;
    let expirations = 1 + elapsed / timer.interval_ms;
    timer.next_expiry_ms = Some(
        deadline
            .saturating_add(expirations.saturating_mul(timer.interval_ms)),
    );
    expirations
}

fn syscall_eventfd(initial_value: u64, flags: u64) -> Result<u64, i32> {
    install_descriptor(UserDescriptor::EventFd(Arc::new(Mutex::new(EventFdState {
        counter: initial_value,
        semaphore: flags & EFD_SEMAPHORE != 0,
    }))))
}

fn syscall_timerfd_create(clock_id: u64, _flags: u64) -> Result<u64, i32> {
    if clock_id != 0 && clock_id != 1 && clock_id != 7 {
        return Err(EINVAL);
    }
    install_descriptor(UserDescriptor::TimerFd(Arc::new(Mutex::new(TimerFdState {
        clock: clock_id,
        interval_ms: 0,
        next_expiry_ms: None,
    }))))
}

fn timerfd_for_fd(fd: u64) -> Result<Arc<Mutex<TimerFdState>>, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::TimerFd(state)) => Ok(Arc::clone(state)),
        Some(_) => Err(EINVAL),
        None => Err(EBADF),
    }
}

fn syscall_timerfd_settime(fd: u64, flags: u64, new_value: u64, old_value: u64) -> Result<u64, i32> {
    if new_value == 0 {
        return Err(EFAULT);
    }
    paging::validate_user_range(new_value, core::mem::size_of::<UserItimerspec>(), false)
        .map_err(|_| EFAULT)?;
    if old_value != 0 {
        paging::validate_user_range(old_value, core::mem::size_of::<UserItimerspec>(), true)
            .map_err(|_| EFAULT)?;
    }
    let spec = unsafe { (new_value as *const UserItimerspec).read_unaligned() };
    let value_ms = timespec_to_milliseconds(&spec.value)?;
    let interval_ms = timespec_to_milliseconds(&spec.interval)?;
    let state = timerfd_for_fd(fd)?;
    let now = crate::arch::x86_64::monotonic_milliseconds();
    let mut timer = state.lock();
    if old_value != 0 {
        let previous = UserItimerspec {
            interval: milliseconds_to_timespec(timer.interval_ms),
            value: milliseconds_to_timespec(
                timer
                    .next_expiry_ms
                    .map_or(0, |deadline| deadline.saturating_sub(now)),
            ),
        };
        unsafe { (old_value as *mut UserItimerspec).write_unaligned(previous) };
    }
    timer.interval_ms = interval_ms;
    timer.next_expiry_ms = if value_ms == 0 {
        None
    } else if flags & TFD_TIMER_ABSTIME != 0 {
        Some(value_ms)
    } else {
        Some(now.saturating_add(value_ms))
    };
    Ok(0)
}

fn syscall_timerfd_gettime(fd: u64, current_value: u64) -> Result<u64, i32> {
    if current_value == 0 {
        return Err(EFAULT);
    }
    paging::validate_user_range(
        current_value,
        core::mem::size_of::<UserItimerspec>(),
        true,
    )
    .map_err(|_| EFAULT)?;
    let state = timerfd_for_fd(fd)?;
    let now = crate::arch::x86_64::monotonic_milliseconds();
    let timer = state.lock();
    let spec = UserItimerspec {
        interval: milliseconds_to_timespec(timer.interval_ms),
        value: milliseconds_to_timespec(
            timer
                .next_expiry_ms
                .map_or(0, |deadline| deadline.saturating_sub(now)),
        ),
    };
    unsafe { (current_value as *mut UserItimerspec).write_unaligned(spec) };
    Ok(0)
}

fn epoll_instance_for_fd(fd: u64) -> Result<Arc<Mutex<EpollInstance>>, i32> {
    let fd = usize::try_from(fd).map_err(|_| EBADF)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    match process.descriptors.get(fd).and_then(Option::as_ref) {
        Some(UserDescriptor::Epoll(instance)) => Ok(Arc::clone(instance)),
        Some(_) => Err(EINVAL),
        None => Err(EBADF),
    }
}

fn syscall_epoll_create1(_flags: u64) -> Result<u64, i32> {
    let mut entries = Vec::new();
    entries.try_reserve_exact(8).map_err(|_| ENOMEM)?;
    install_descriptor(UserDescriptor::Epoll(Arc::new(Mutex::new(EpollInstance {
        entries,
    }))))
}

fn syscall_epoll_ctl(epfd: u64, operation: u64, fd: u64, event: u64) -> Result<u64, i32> {
    let target = usize::try_from(fd).map_err(|_| EBADF)?;
    {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        match process.descriptors.get(target).and_then(Option::as_ref) {
            Some(_) => {}
            None => return Err(EBADF),
        }
        if matches!(
            process.descriptors.get(target).and_then(Option::as_ref),
            Some(UserDescriptor::Epoll(_))
        ) {
            return Err(EINVAL);
        }
        if usize::try_from(epfd).ok() == Some(target) {
            return Err(EINVAL);
        }
    }

    let (events, data) = if operation == EPOLL_CTL_DEL {
        (0u32, 0u64)
    } else {
        if event == 0 {
            return Err(EFAULT);
        }
        paging::validate_user_range(event, core::mem::size_of::<UserEpollEvent>(), false)
            .map_err(|_| EFAULT)?;
        let user_event = unsafe { (event as *const UserEpollEvent).read_unaligned() };
        (user_event.events, user_event.data)
    };

    let instance = epoll_instance_for_fd(epfd)?;
    let mut epoll = instance.lock();
    match operation {
        EPOLL_CTL_ADD => {
            if epoll.entries.iter().any(|entry| entry.fd == target) {
                return Err(EEXIST);
            }
            epoll
                .entries
                .try_reserve(1)
                .map_err(|_| ENOMEM)?;
            epoll.entries.push(EpollEntry {
                fd: target,
                events,
                data,
                last_ready: 0,
            });
        }
        EPOLL_CTL_MOD => {
            let Some(entry) = epoll.entries.iter_mut().find(|entry| entry.fd == target) else {
                return Err(ENOENT);
            };
            entry.events = events;
            entry.data = data;
            entry.last_ready = 0;
        }
        EPOLL_CTL_DEL => {
            let Some(index) = epoll.entries.iter().position(|entry| entry.fd == target) else {
                return Err(ENOENT);
            };
            epoll.entries.remove(index);
        }
        _ => return Err(EINVAL),
    }
    Ok(0)
}

fn collect_epoll_events(
    instance: &Arc<Mutex<EpollInstance>>,
    events_address: u64,
    maxevents: usize,
) -> Result<u64, i32> {
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let mut epoll = instance.lock();
    let mut written = 0usize;
    for entry in epoll.entries.iter_mut() {
        let Some(state) = descriptor_poll_state(&process, entry.fd, entry.fd as i32) else {
            continue;
        };
        let mut ready = 0u32;
        if entry.events & EPOLLIN != 0 && state.readable {
            ready |= EPOLLIN;
        }
        if entry.events & EPOLLOUT != 0 && state.writable {
            ready |= EPOLLOUT;
        }
        if state.error {
            ready |= EPOLLERR;
        }
        if state.hangup {
            ready |= EPOLLHUP;
        }
        if entry.events & EPOLLET != 0 {
            // Edge-triggered: laporkan hanya saat readiness berubah dari 0.
            let report = ready != 0 && entry.last_ready == 0;
            entry.last_ready = ready;
            if !report {
                continue;
            }
        }
        if ready == 0 {
            continue;
        }
        let user_event = UserEpollEvent {
            events: ready,
            data: entry.data,
        };
        unsafe {
            (events_address as *mut UserEpollEvent)
                .add(written)
                .write_unaligned(user_event);
        }
        if entry.events & EPOLLONESHOT != 0 {
            entry.events &= !(EPOLLIN | EPOLLOUT);
        }
        written += 1;
        if written == maxevents {
            break;
        }
    }
    Ok(written as u64)
}

fn syscall_epoll_wait(epfd: u64, events: u64, maxevents: u64, timeout_ms: u64) -> Result<u64, i32> {
    let maxevents = usize::try_from(maxevents).map_err(|_| EINVAL)?;
    if maxevents == 0 {
        return Err(EINVAL);
    }
    let bytes = maxevents
        .checked_mul(core::mem::size_of::<UserEpollEvent>())
        .ok_or(EINVAL)?;
    paging::validate_user_range(events, bytes, true).map_err(|_| EFAULT)?;
    let instance = epoll_instance_for_fd(epfd)?;
    let ready = collect_epoll_events(&instance, events, maxevents)?;
    if ready != 0 || timeout_ms == 0 {
        return Ok(ready);
    }
    let budget = wait_budget_milliseconds(timeout_ms);
    let mut waited = 0u64;
    while waited < budget {
        sleep_milliseconds(1);
        waited += 1;
        let ready = collect_epoll_events(&instance, events, maxevents)?;
        if ready != 0 {
            return Ok(ready);
        }
    }
    Ok(0)
}

fn syscall_socketpair(domain: u64, socket_type: u64, protocol: u64, addresses: u64) -> Result<u64, i32> {
    if domain != AF_UNIX {
        return Err(EAFNOSUPPORT);
    }
    if protocol != 0 {
        return Err(EPROTONOSUPPORT);
    }
    match socket_type & SOCK_TYPE_MASK {
        SOCK_STREAM | 2 => {}
        _ => return Err(EPROTOTYPE),
    }
    paging::validate_user_range(addresses, core::mem::size_of::<[i32; 2]>(), true)
        .map_err(|_| EFAULT)?;
    let (first_read, first_write) = create_pipe()?;
    let (second_read, second_write) = create_pipe()?;
    let first = install_descriptor(UserDescriptor::SocketPair(DuplexEnd {
        read_end: first_read,
        write_end: second_write,
    }))?;
    let second = match install_descriptor(UserDescriptor::SocketPair(DuplexEnd {
        read_end: second_read,
        write_end: first_write,
    })) {
        Ok(fd) => fd,
        Err(error) => {
            let mut process = USER_PROCESS.lock();
            if let Some(slot) = process.descriptors.get_mut(first as usize) {
                *slot = None;
            }
            return Err(error);
        }
    };
    unsafe {
        (addresses as *mut [i32; 2]).write_unaligned([first as i32, second as i32]);
    }
    Ok(0)
}

fn syscall_clock_nanosleep(clock_id: u64, flags: u64, request: u64, remain: u64) -> Result<u64, i32> {
    if take_interrupted_syscall(SYSCALL_CLOCK_NANOSLEEP) {
        return Err(EINTR);
    }
    if clock_id != 0 && clock_id != 1 && clock_id != 7 {
        return Err(EINVAL);
    }
    if request == 0 {
        return Err(EFAULT);
    }
    paging::validate_user_range(request, core::mem::size_of::<UserTimespec>(), false)
        .map_err(|_| EFAULT)?;
    let ts = unsafe { (request as *const UserTimespec).read_unaligned() };
    let requested = timespec_to_milliseconds(&ts)?;
    let milliseconds = if flags & TIMER_ABSTIME != 0 {
        let now = if clock_id == 0 {
            (crate::drivers::rtc::unix_seconds().map_err(|_| EIO)? as u64)
                .saturating_mul(1000)
        } else {
            crate::arch::x86_64::monotonic_milliseconds()
        };
        requested.saturating_sub(now)
    } else {
        requested
    };
    if remain != 0 {
        paging::validate_user_range(remain, core::mem::size_of::<UserTimespec>(), true)
            .map_err(|_| EFAULT)?;
        unsafe { (remain as *mut UserTimespec).write_unaligned(milliseconds_to_timespec(0)) };
    }
    sleep_milliseconds(milliseconds.min(60_000));
    Ok(0)
}

// ==== signal delivery ====

const SIGKILL_NUMBER: u64 = 9;
const SIGSTOP_NUMBER: u64 = 19;
const SIGNAL_TABLE_LENGTH: usize = 32;
const SA_RESETHAND_FLAG: u64 = 0x8000_0000;
const SA_ONSTACK_FLAG: u64 = 0x0800_0000;
const SA_RESTART_FLAG: u64 = 0x1000_0000;
const SS_ONSTACK_FLAG: i32 = 1;
const SS_DISABLE_FLAG: i32 = 2;
const MINSIGSTKSZ: u64 = 2048;

/// `stack_t` versi OCTAGRAM: sp, flags, padding, size (24 byte).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserStack {
    sp: u64,
    flags: i32,
    padding: i32,
    size: u64,
}

/// Memeriksa sinyal yang siap dikirim sebelum syscall memblokir. Bila ada,
/// syscall harus gagal dengan `EINTR`; untuk handler ber-`SA_RESTART` syscall
/// dicatat agar diulang setelah handler selesai.
fn take_interrupted_syscall(number: u64) -> bool {
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return false;
    }
    ensure_signal_table(&mut process);
    process.interrupted = false;
    process.interrupt_signal = 0;
    let pending = process.pending_signals & !process.blocked_signals;
    if pending == 0 {
        return false;
    }
    let signal = pending.trailing_zeros() as u64 + 1;
    let action = process.signal_handlers[signal as usize];
    if action.handler == 1 {
        return false;
    }
    if action.handler > 1 && action.flags & SA_RESTART_FLAG != 0 {
        process.restart_number = Some(number);
    }
    true
}

#[derive(Clone, Copy)]
struct SignalAction {
    handler: u64,
    flags: u64,
    mask: u64,
    restorer: u64,
}

const SIGNAL_ACTION_DEFAULT: SignalAction = SignalAction {
    handler: 0,
    flags: 0,
    mask: 0,
    restorer: 0,
};

/// Konteks yang ditulis kernel ke stack user sebelum memanggil handler.
/// `sigreturn` memulihkan seluruh isinya.
#[repr(C)]
#[derive(Clone, Copy)]
struct UserSignalFrame {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    rbp: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rip: u64,
    rflags: u64,
    rsp: u64,
    mask: u64,
    signum: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserSigaction {
    handler: u64,
    mask: u64,
    flags: i32,
    padding: u32,
    restorer: u64,
}

fn ensure_signal_table(process: &mut UserProcess) {
    if process.signal_handlers.len() != SIGNAL_TABLE_LENGTH {
        process.signal_handlers.clear();
        process
            .signal_handlers
            .resize(SIGNAL_TABLE_LENGTH, SIGNAL_ACTION_DEFAULT);
    }
}

fn signal_default_ignored(number: u64) -> bool {
    matches!(number, 17 | 18 | 23 | 28)
}

fn syscall_kill(pid: u64, signal: u64) -> Result<u64, i32> {
    if signal >= SIGNAL_TABLE_LENGTH as u64 {
        return Err(EINVAL);
    }
    if signal == 0 {
        return Ok(0);
    }
    let bit = 1u64 << (signal - 1);
    let current_pid = {
        let process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        process.pid
    };
    // kill ke diri sendiri tetap harus membangunkan thread saudara yang
    // sedang menunggu, jadi jalurnya sama dengan kill ke pid lain.
    let self_target = pid == 0 || pid == current_pid || pid == u64::MAX;
    let target_pid = if self_target { current_pid } else { pid };
    let mut scheduler = USER_SCHEDULER.lock();
    let current_index = scheduler.current;
    let mut woke_blocked_thread = false;
    for (index, process) in scheduler.processes.iter_mut().enumerate() {
        if process.pid != target_pid || index == current_index {
            continue;
        }
        if process.exit_status.is_some() {
            continue;
        }
        ensure_signal_table(&mut process.state);
        process.state.pending_signals |= bit;
        let deliverable = process.state.blocked_signals & bit == 0
            && process.state.signal_handlers[signal as usize].handler != 1;
        if deliverable && process.waiting_for.is_some() {
            process.waiting_for = None;
            process.wait_deadline = None;
            process.state.interrupted = true;
            process.state.interrupt_signal = signal;
            woke_blocked_thread = true;
        }
    }
    drop(scheduler);

    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    if process.pid != target_pid {
        return Err(ESRCH);
    }
    if !woke_blocked_thread {
        ensure_signal_table(&mut process);
        process.pending_signals |= bit;
    }
    Ok(0)
}

fn syscall_sigaction(number: u64, action: u64, old_action: u64) -> Result<u64, i32> {
    if number == 0 || number >= SIGNAL_TABLE_LENGTH as u64 {
        return Err(EINVAL);
    }
    if number == SIGKILL_NUMBER || number == SIGSTOP_NUMBER {
        return Err(EINVAL);
    }
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    ensure_signal_table(&mut process);
    if old_action != 0 {
        paging::validate_user_range(old_action, core::mem::size_of::<UserSigaction>(), true)
            .map_err(|_| EFAULT)?;
        let previous = process.signal_handlers[number as usize];
        let user_previous = UserSigaction {
            handler: previous.handler,
            mask: previous.mask,
            flags: previous.flags as i32,
            padding: 0,
            restorer: previous.restorer,
        };
        unsafe { (old_action as *mut UserSigaction).write_unaligned(user_previous) };
    }
    if action != 0 {
        paging::validate_user_range(action, core::mem::size_of::<UserSigaction>(), false)
            .map_err(|_| EFAULT)?;
        let user_action = unsafe { (action as *const UserSigaction).read_unaligned() };
        process.signal_handlers[number as usize] = SignalAction {
            handler: user_action.handler,
            flags: user_action.flags as u64,
            mask: user_action.mask,
            restorer: user_action.restorer,
        };
    }
    Ok(0)
}

fn syscall_sigaltstack(new_stack: u64, old_stack: u64) -> Result<u64, i32> {
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    if old_stack != 0 {
        paging::validate_user_range(old_stack, core::mem::size_of::<UserStack>(), true)
            .map_err(|_| EFAULT)?;
        let mut flags = 0;
        if process.alternate_stack_active {
            flags |= SS_ONSTACK_FLAG;
        }
        if process.alternate_stack_size == 0 {
            flags |= SS_DISABLE_FLAG;
        }
        let current = UserStack {
            sp: process.alternate_stack,
            flags,
            padding: 0,
            size: process.alternate_stack_size,
        };
        unsafe { (old_stack as *mut UserStack).write_unaligned(current) };
    }
    if new_stack != 0 {
        paging::validate_user_range(new_stack, core::mem::size_of::<UserStack>(), false)
            .map_err(|_| EFAULT)?;
        let request = unsafe { (new_stack as *const UserStack).read_unaligned() };
        if process.alternate_stack_active {
            return Err(EPERM);
        }
        let flags = request.flags as u64;
        if flags & !((SS_ONSTACK_FLAG | SS_DISABLE_FLAG) as u64) != 0 || request.padding != 0 {
            return Err(EINVAL);
        }
        if request.flags & SS_DISABLE_FLAG != 0 {
            process.alternate_stack = 0;
            process.alternate_stack_size = 0;
        } else {
            let size = usize::try_from(request.size).map_err(|_| EINVAL)?;
            if request.sp == 0 || request.size < MINSIGSTKSZ {
                return Err(EINVAL);
            }
            paging::validate_user_range(request.sp, size, true).map_err(|_| EFAULT)?;
            process.alternate_stack = request.sp;
            process.alternate_stack_size = request.size;
        }
    }
    Ok(0)
}

fn syscall_signal(number: u64, handler: u64) -> Result<u64, i32> {
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    if number == 0 || number >= SIGNAL_TABLE_LENGTH as u64 {
        return Err(EINVAL);
    }
    if number == SIGKILL_NUMBER || number == SIGSTOP_NUMBER {
        return Err(EINVAL);
    }
    ensure_signal_table(&mut process);
    let slot = &mut process.signal_handlers[number as usize];
    let previous = slot.handler;
    slot.handler = handler;
    Ok(previous)
}

fn syscall_sigprocmask(how: u64, set: u64, old_set: u64) -> Result<u64, i32> {
    if how > 2 {
        return Err(EINVAL);
    }
    let mut process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    if old_set != 0 {
        paging::validate_user_range(old_set, core::mem::size_of::<u64>(), true)
            .map_err(|_| EFAULT)?;
        unsafe { (old_set as *mut u64).write_unaligned(process.blocked_signals) };
    }
    if set != 0 {
        paging::validate_user_range(set, core::mem::size_of::<u64>(), false)
            .map_err(|_| EFAULT)?;
        let mut mask = unsafe { (set as *const u64).read_unaligned() };
        mask &= !((1u64 << (SIGKILL_NUMBER - 1)) | (1u64 << (SIGSTOP_NUMBER - 1)));
        process.blocked_signals = match how {
            0 => process.blocked_signals | mask,
            1 => process.blocked_signals & !mask,
            _ => mask,
        };
    }
    Ok(0)
}

fn syscall_sigpending(set: u64) -> Result<u64, i32> {
    paging::validate_user_range(set, core::mem::size_of::<u64>(), true).map_err(|_| EFAULT)?;
    let process = USER_PROCESS.lock();
    if !process.active {
        return Err(EINTR);
    }
    let pending = process.pending_signals & process.blocked_signals;
    unsafe { (set as *mut u64).write_unaligned(pending) };
    Ok(0)
}

fn syscall_sigsuspend(mask_address: u64) -> Result<u64, i32> {
    let mask = if mask_address == 0 {
        0
    } else {
        paging::validate_user_range(mask_address, core::mem::size_of::<u64>(), false)
            .map_err(|_| EFAULT)?;
        unsafe { (mask_address as *const u64).read_unaligned() }
    };
    let previous = {
        let mut process = USER_PROCESS.lock();
        if !process.active {
            return Err(EINTR);
        }
        let previous = process.blocked_signals;
        process.blocked_signals = mask;
        previous
    };
    let mut waited = 0u64;
    loop {
        let ready = {
            let process = USER_PROCESS.lock();
            process.pending_signals & !process.blocked_signals != 0
        };
        if ready || waited >= 60_000 {
            break;
        }
        sleep_milliseconds(1);
        waited += 1;
    }
    let mut process = USER_PROCESS.lock();
    process.blocked_signals = previous;
    Ok(0)
}

fn syscall_sigreturn(frame: &mut InterruptFrame) -> *mut InterruptFrame {
    let frame_pointer = frame as *mut InterruptFrame;
    let stack_pointer = frame.stack_frame.stack_pointer;
    if paging::validate_user_range(
        stack_pointer,
        core::mem::size_of::<UserSignalFrame>(),
        false,
    )
    .is_err()
    {
        frame.rax = (-(EFAULT as i64)) as u64;
        return frame_pointer;
    }
    let saved = unsafe { (stack_pointer as *const UserSignalFrame).read_unaligned() };
    frame.rax = saved.rax;
    frame.rbx = saved.rbx;
    frame.rcx = saved.rcx;
    frame.rdx = saved.rdx;
    frame.rsi = saved.rsi;
    frame.rdi = saved.rdi;
    frame.rbp = saved.rbp;
    frame.r8 = saved.r8;
    frame.r9 = saved.r9;
    frame.r10 = saved.r10;
    frame.r11 = saved.r11;
    frame.r12 = saved.r12;
    frame.r13 = saved.r13;
    frame.r14 = saved.r14;
    frame.r15 = saved.r15;
    frame.stack_frame.instruction_pointer = saved.rip;
    frame.stack_frame.cpu_flags = saved.rflags;
    frame.stack_frame.stack_pointer = saved.rsp;
    let restart = {
        let mut process = USER_PROCESS.lock();
        process.blocked_signals = saved.mask;
        process.alternate_stack_active = false;
        process.restart_number.take()
    };
    if let Some(number) = restart {
        // Ulangi syscall yang terinterupsi setelah handler selesai.
        frame.rax = number;
        frame.stack_frame.instruction_pointer =
            frame.stack_frame.instruction_pointer.saturating_sub(2);
    }
    frame_pointer
}

/// Puncak stack untuk frame sinyal: stack alternatif bila tersedia, jika tidak
/// stack yang sedang dipakai.
fn signal_stack_top(normal_stack_pointer: u64, alternate: Option<(u64, u64)>) -> u64 {
    alternate
        .map(|(base, size)| base.saturating_add(size))
        .unwrap_or(normal_stack_pointer)
}

/// Memilih sinyal pending yang bisa dikirim, lalu menyiapkan handler di stack
/// user. Mengembalikan frame pengganti hanya bila proses dihentikan.
fn deliver_pending_signal(frame: &mut InterruptFrame) -> Option<*mut InterruptFrame> {
    for _ in 0..SIGNAL_TABLE_LENGTH {
        let (number, action, blocked) = {
            let mut process = USER_PROCESS.lock();
            if !process.active {
                return None;
            }
            ensure_signal_table(&mut process);
            let kill_bit = 1u64 << (SIGKILL_NUMBER - 1);
            let deliverable = if process.pending_signals & kill_bit != 0 {
                kill_bit
            } else {
                process.pending_signals & !process.blocked_signals
            };
            if deliverable == 0 {
                return None;
            }
            let number = deliverable.trailing_zeros() as u64 + 1;
            let action = process.signal_handlers[number as usize];
            let blocked = process.blocked_signals;
            (number, action, blocked)
        };

        if action.handler == 1 {
            let mut process = USER_PROCESS.lock();
            process.pending_signals &= !(1u64 << (number - 1));
            continue;
        }

        if action.handler == 0 {
            if signal_default_ignored(number) {
                let mut process = USER_PROCESS.lock();
                process.pending_signals &= !(1u64 << (number - 1));
                continue;
            }
            return Some(terminate_user_inner(frame, 128 + number));
        }

        let original_stack_pointer = frame.stack_frame.stack_pointer;
        let frame_size = core::mem::size_of::<UserSignalFrame>();
        let alternate = {
            let process = USER_PROCESS.lock();
            if action.flags & SA_ONSTACK_FLAG != 0
                && process.alternate_stack_size != 0
                && !process.alternate_stack_active
            {
                Some((process.alternate_stack, process.alternate_stack_size))
            } else {
                None
            }
        };
        let stack_pointer = signal_stack_top(original_stack_pointer, alternate);
        let frame_address = stack_pointer.saturating_sub(frame_size as u64) & !0xF;
        let restorer_address = frame_address.saturating_sub(8);
        if paging::validate_user_range(restorer_address, frame_size + 8, true).is_err() {
            frame.rax = (-(EFAULT as i64)) as u64;
            return None;
        }
        let saved = UserSignalFrame {
            rax: frame.rax,
            rbx: frame.rbx,
            rcx: frame.rcx,
            rdx: frame.rdx,
            rsi: frame.rsi,
            rdi: frame.rdi,
            rbp: frame.rbp,
            r8: frame.r8,
            r9: frame.r9,
            r10: frame.r10,
            r11: frame.r11,
            r12: frame.r12,
            r13: frame.r13,
            r14: frame.r14,
            r15: frame.r15,
            rip: frame.stack_frame.instruction_pointer,
            rflags: frame.stack_frame.cpu_flags,
            rsp: original_stack_pointer,
            mask: blocked,
            signum: number,
        };
        unsafe {
            (frame_address as *mut UserSignalFrame).write_unaligned(saved);
            (restorer_address as *mut u64).write_unaligned(action.restorer);
        }
        frame.stack_frame.instruction_pointer = action.handler;
        frame.stack_frame.stack_pointer = restorer_address;
        frame.rdi = number;
        frame.rsi = frame_address;
        frame.rdx = frame_address;
        frame.rax = 0;

        let mut process = USER_PROCESS.lock();
        process.pending_signals &= !(1u64 << (number - 1));
        process.blocked_signals |= (1u64 << (number - 1)) | action.mask;
        if alternate.is_some() {
            process.alternate_stack_active = true;
        }
        if action.flags & SA_RESETHAND_FLAG != 0 {
            process.signal_handlers[number as usize].handler = 0;
        }
        return None;
    }
    None
}

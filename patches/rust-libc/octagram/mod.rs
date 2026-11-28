//! OCTAGRAM x86_64 userspace ABI.
//!
//! These definitions describe OCTAGRAM's public C ABI. They intentionally do
//! not reuse Linux or Redox structure layouts. Keep this module synchronized
//! with the OCTAGRAM public headers and ABI validator.

// libc re-exports its primitive aliases from whichever module tree is active,
// and the OCTAGRAM module is shared by both of them. Declaring them here keeps
// the module independent of that choice.
use core::ffi::{
    c_char, c_double, c_float, c_int, c_long, c_longlong, c_short, c_uchar, c_uint, c_ulong,
    c_ulonglong, c_ushort, c_void,
};

use core::option::Option;

pub type intptr_t = isize;
pub type size_t = usize;
pub type ssize_t = isize;
pub type uintptr_t = usize;


pub type wchar_t = i32;
pub type blkcnt_t = i64;
pub type blksize_t = i64;
pub type clock_t = i64;
pub type clockid_t = c_int;
pub type dev_t = u64;
pub type fsblkcnt_t = u64;
pub type fsfilcnt_t = u64;
pub type ino_t = u64;
pub type mode_t = u32;
pub type nfds_t = c_ulong;
pub type nlink_t = u32;
pub type off_t = i64;
pub type pthread_t = u64;
pub type pthread_key_t = u32;
pub type rlim_t = u64;
pub type sa_family_t = u16;
pub type sigset_t = u64;
pub type socklen_t = u32;
pub type tcflag_t = u32;
pub type cc_t = u8;
pub type speed_t = u32;
pub type suseconds_t = i64;
pub type time_t = i64;
pub type id_t = u32;
pub type pid_t = i32;
pub type uid_t = u32;
pub type gid_t = u32;

s! {
    /// Public C stat structure. This is not the 16-byte kernel oct_stat record.
    // Field order mirrors `struct stat` in user/include/sys/stat.h exactly; the
    // `st_atime`/`st_*time_nsec` names line up with the C macros that alias the
    // `st_atim`/`st_atim.tv_nsec` members.
    pub struct stat {
        pub st_size: off_t,
        pub st_mode: mode_t,
        pub st_dev: dev_t,
        pub st_ino: ino_t,
        pub st_nlink: nlink_t,
        pub st_uid: uid_t,
        pub st_gid: gid_t,
        pub st_rdev: dev_t,
        pub st_blksize: blksize_t,
        pub st_blocks: blkcnt_t,
        pub st_atime: time_t,
        pub st_atime_nsec: time_t,
        pub st_mtime: time_t,
        pub st_mtime_nsec: time_t,
        pub st_ctime: time_t,
        pub st_ctime_nsec: time_t,
    }

    pub struct statvfs {
        pub f_bsize: c_ulong,
        pub f_frsize: c_ulong,
        pub f_blocks: fsblkcnt_t,
        pub f_bfree: fsblkcnt_t,
        pub f_bavail: fsblkcnt_t,
        pub f_files: fsfilcnt_t,
        pub f_ffree: fsfilcnt_t,
        pub f_favail: fsfilcnt_t,
        pub f_fsid: c_ulong,
        pub f_flag: c_ulong,
        pub f_namemax: c_ulong,
    }

    pub struct dirent {
        pub d_ino: ino_t,
        pub d_off: off_t,
        pub d_reclen: c_ushort,
        pub d_type: c_uchar,
        pub d_name: [c_char; 256],
    }

    #[repr(align(8))]
    pub struct pthread_attr_t {
        pub opaque: usize,
        pub stack_size: size_t,
    }

    #[repr(align(8))]
    pub struct pthread_mutex_t {
        pub next: *mut c_void,
        pub state: c_uint,
        pub flags: c_uint,
    }

    #[repr(align(4))]
    pub struct pthread_cond_t {
        pub sequence: c_uint,
    }

    pub struct pthread_mutexattr_t {
        pub robustness: c_int,
    }

    pub struct pthread_condattr_t {
        pub opaque: c_int,
    }

    pub struct sockaddr {
        pub sa_family: sa_family_t,
        pub sa_data: [c_char; 14],
    }

    pub struct sockaddr_un {
        pub sun_family: sa_family_t,
        pub sun_path: [c_char; 108],
    }

    #[repr(C, align(8))]
    pub struct sockaddr_storage {
        pub ss_family: sa_family_t,
        pub __data: [c_uchar; 118],
        pub __align: u64,
    }

    pub struct in_addr {
        pub s_addr: u32,
    }

    pub struct sockaddr_in {
        pub sin_family: sa_family_t,
        pub sin_port: u16,
        pub sin_addr: in_addr,
        pub sin_zero: [c_uchar; 8],
    }

    pub struct sockaddr_in6 {
        pub sin6_family: sa_family_t,
        pub sin6_port: u16,
        pub sin6_flowinfo: u32,
        pub sin6_addr: crate::in6_addr,
        pub sin6_scope_id: u32,
    }

    pub struct ip_mreq {
        pub imr_multiaddr: in_addr,
        pub imr_interface: in_addr,
    }

    // Linux-compatible layouts used by socket2's generic Unix implementation.
    // OCTAGRAM currently does not implement source-specific multicast or the
    // Linux interface-index extension; these types preserve ABI layout only.
    pub struct ip_mreqn {
        pub imr_multiaddr: in_addr,
        pub imr_address: in_addr,
        pub imr_ifindex: c_int,
    }

    pub struct ip_mreq_source {
        pub imr_multiaddr: in_addr,
        pub imr_interface: in_addr,
        pub imr_sourceaddr: in_addr,
    }

    pub struct addrinfo {
        pub ai_flags: c_int,
        pub ai_family: c_int,
        pub ai_socktype: c_int,
        pub ai_protocol: c_int,
        pub ai_addrlen: socklen_t,
        pub ai_addr: *mut sockaddr,
        pub ai_canonname: *mut c_char,
        pub ai_next: *mut addrinfo,
    }

    pub struct passwd {
        pub pw_name: *mut c_char,
        pub pw_passwd: *mut c_char,
        pub pw_uid: uid_t,
        pub pw_gid: gid_t,
        pub pw_gecos: *mut c_char,
        pub pw_dir: *mut c_char,
        pub pw_shell: *mut c_char,
    }

    pub struct tm {
        pub tm_sec: c_int,
        pub tm_min: c_int,
        pub tm_hour: c_int,
        pub tm_mday: c_int,
        pub tm_mon: c_int,
        pub tm_year: c_int,
        pub tm_wday: c_int,
        pub tm_yday: c_int,
        pub tm_isdst: c_int,
    }

    pub struct fd_set {
        pub bits: [u64; 16],
    }

    pub struct lconv {
        pub decimal_point: *mut c_char,
        pub thousands_sep: *mut c_char,
        pub grouping: *mut c_char,
        pub int_curr_symbol: *mut c_char,
        pub currency_symbol: *mut c_char,
        pub mon_decimal_point: *mut c_char,
        pub mon_thousands_sep: *mut c_char,
        pub mon_grouping: *mut c_char,
        pub positive_sign: *mut c_char,
        pub negative_sign: *mut c_char,
        pub int_frac_digits: c_char,
        pub frac_digits: c_char,
        pub p_cs_precedes: c_char,
        pub p_sep_by_space: c_char,
        pub n_cs_precedes: c_char,
        pub n_sep_by_space: c_char,
        pub p_sign_posn: c_char,
        pub n_sign_posn: c_char,
        pub int_p_cs_precedes: c_char,
        pub int_p_sep_by_space: c_char,
        pub int_n_cs_precedes: c_char,
        pub int_n_sep_by_space: c_char,
        pub int_p_sign_posn: c_char,
        pub int_n_sign_posn: c_char,
    }

    pub struct sem_t {
        pub count: c_uint,
    }

    pub struct termios {
        pub c_iflag: tcflag_t,
        pub c_oflag: tcflag_t,
        pub c_cflag: tcflag_t,
        pub c_lflag: tcflag_t,
        pub c_line: cc_t,
        pub c_cc: [cc_t; 32],
        pub c_ispeed: speed_t,
        pub c_ospeed: speed_t,
    }

    pub struct sigaction {
        pub sa_sigaction: crate::sighandler_t,
        pub sa_mask: sigset_t,
        pub sa_flags: c_int,
        pub sa_restorer: Option<unsafe extern "C" fn()>,
    }
}

const _: [(); 120] = [(); core::mem::size_of::<stat>()];
const _: [(); 8] = [(); core::mem::align_of::<stat>()];
const _: [(); 110] = [(); core::mem::size_of::<sockaddr_un>()];
const _: [(); 2] = [(); core::mem::align_of::<sockaddr_un>()];
const _: [(); 128] = [(); core::mem::size_of::<sockaddr_storage>()];
const _: [(); 8] = [(); core::mem::align_of::<sockaddr_storage>()];
const _: [(); 4] = [(); core::mem::size_of::<in_addr>()];
const _: [(); 16] = [(); core::mem::size_of::<sockaddr_in>()];
const _: [(); 28] = [(); core::mem::size_of::<sockaddr_in6>()];
const _: [(); 8] = [(); core::mem::size_of::<ip_mreq>()];
const _: [(); 8] = [(); core::mem::align_of::<stat>()];
const _: [(); 280] = [(); core::mem::size_of::<dirent>()];
const _: [(); 8] = [(); core::mem::align_of::<dirent>()];
const _: [(); 16] = [(); core::mem::size_of::<pthread_attr_t>()];
const _: [(); 16] = [(); core::mem::size_of::<pthread_mutex_t>()];
const _: [(); 4] = [(); core::mem::size_of::<pthread_cond_t>()];
const _: [(); 4] = [(); core::mem::size_of::<sem_t>()];
const _: [(); 60] = [(); core::mem::size_of::<termios>()];
const _: [(); 32] = [(); core::mem::size_of::<sigaction>()];
const _: [(); 16] = [(); core::mem::size_of::<sockaddr>()];
const _: [(); 48] = [(); core::mem::size_of::<addrinfo>()];
const _: [(); 48] = [(); core::mem::size_of::<passwd>()];
const _: [(); 36] = [(); core::mem::size_of::<tm>()];
const _: [(); 128] = [(); core::mem::size_of::<fd_set>()];
const _: [(); 96] = [(); core::mem::size_of::<lconv>()];

pub const PTHREAD_MUTEX_INITIALIZER: pthread_mutex_t = pthread_mutex_t {
    next: core::ptr::null_mut(),
    state: 0,
    flags: 0,
};
pub const PTHREAD_COND_INITIALIZER: pthread_cond_t = pthread_cond_t { sequence: 0 };

pub const PATH_MAX: c_int = 4096;
pub const F_OK: c_int = 0;
pub const X_OK: c_int = 1;
pub const W_OK: c_int = 2;
pub const R_OK: c_int = 4;
pub const RTLD_DEFAULT: *mut c_void = core::ptr::null_mut();
pub const UTIME_NOW: c_long = 0x3fffffff;
pub const MS_ASYNC: c_int = 0x01;
pub const MS_INVALIDATE: c_int = 0x02;
pub const MS_SYNC: c_int = 0x04;
pub const MADV_NORMAL: c_int = 0;
pub const MADV_RANDOM: c_int = 1;
pub const MADV_SEQUENTIAL: c_int = 2;
pub const MADV_WILLNEED: c_int = 3;
pub const MADV_DONTNEED: c_int = 4;
pub const AT_FDCWD: c_int = -100;
pub const AT_SYMLINK_NOFOLLOW: c_int = 1 << 8;
pub const AT_REMOVEDIR: c_int = 1 << 9;
pub const AT_EACCESS: c_int = 1 << 10;
pub const AT_SYMLINK_FOLLOW: c_int = 1 << 11;

pub const O_RDONLY: c_int = 0;
pub const O_WRONLY: c_int = 1;
pub const O_RDWR: c_int = 2;
pub const O_ACCMODE: c_int = 3;
pub const O_CREAT: c_int = 1 << 8;
pub const O_TRUNC: c_int = 1 << 9;
pub const O_APPEND: c_int = 1 << 10;
pub const O_NONBLOCK: c_int = 1 << 11;
pub const O_EXCL: c_int = 1 << 12;
pub const O_CLOEXEC: c_int = 1 << 19;
pub const O_NOFOLLOW: c_int = 1 << 17;
// Reserved OCTAGRAM open flag; the runtime may reject it until O_SYNC is implemented.
pub const O_SYNC: c_int = 1 << 20;

pub const S_IFMT: mode_t = 0o170000;
pub const S_IFIFO: mode_t = 0o010000;
pub const S_IFCHR: mode_t = 0o020000;
pub const S_IFBLK: mode_t = 0o060000;
pub const S_IFDIR: mode_t = 0o040000;
pub const S_IFLNK: mode_t = 0o120000;
pub const S_IFSOCK: mode_t = 0o140000;
pub const S_IFREG: mode_t = 0o100000;
pub const S_IRUSR: mode_t = 0o400;
pub const S_IWUSR: mode_t = 0o200;
pub const S_IXUSR: mode_t = 0o100;
pub const S_IRGRP: mode_t = 0o040;
pub const S_IWGRP: mode_t = 0o020;
pub const S_IXGRP: mode_t = 0o010;
pub const S_IROTH: mode_t = 0o004;
pub const S_IWOTH: mode_t = 0o002;
pub const S_IXOTH: mode_t = 0o001;
pub const S_IRWXU: mode_t = 0o700;
pub const S_IRWXG: mode_t = 0o070;
pub const S_IRWXO: mode_t = 0o007;

pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;

pub const CLOCK_REALTIME: clockid_t = 0;
pub const CLOCK_MONOTONIC: clockid_t = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: clockid_t = 2;
pub const CLOCK_THREAD_CPUTIME_ID: clockid_t = 3;
pub const CLOCK_MONOTONIC_RAW: clockid_t = 4;
pub const CLOCK_BOOTTIME: clockid_t = 7;
pub const TIMER_ABSTIME: c_int = 1;

pub const PROT_NONE: c_int = 0;
pub const PROT_READ: c_int = 1;
pub const PROT_WRITE: c_int = 2;
pub const PROT_EXEC: c_int = 4;
pub const MAP_SHARED: c_int = 1;
pub const MAP_PRIVATE: c_int = 2;
pub const MAP_FIXED: c_int = 0x10;
pub const MAP_ANON: c_int = 0x20;
pub const MAP_ANONYMOUS: c_int = MAP_ANON;

pub const PTHREAD_MUTEX_STALLED: c_int = 0;
pub const PTHREAD_MUTEX_NORMAL: c_int = 0;
pub const PTHREAD_MUTEX_DEFAULT: c_int = PTHREAD_MUTEX_NORMAL;
pub const PTHREAD_MUTEX_ROBUST: c_int = 1;
pub const PTHREAD_CREATE_JOINABLE: c_int = 0;
pub const PTHREAD_CREATE_DETACHED: c_int = 1;
pub const PTHREAD_STACK_MIN: size_t = 16384;

pub const EPERM: c_int = 1;
pub const ENOENT: c_int = 2;
pub const ESRCH: c_int = 3;
pub const EINTR: c_int = 4;
pub const EIO: c_int = 5;
pub const ENXIO: c_int = 6;
pub const E2BIG: c_int = 7;
pub const ENOEXEC: c_int = 8;
pub const EBADF: c_int = 9;
pub const ECHILD: c_int = 10;
pub const EAGAIN: c_int = 11;
pub const EWOULDBLOCK: c_int = EAGAIN;
pub const EXDEV: c_int = 18;
pub const ETXTBSY: c_int = 26;
pub const EFBIG: c_int = 27;
pub const EMLINK: c_int = 31;
pub const EDQUOT: c_int = 122;
pub const ESTALE: c_int = 116;
pub const ENOMEM: c_int = 12;
pub const EACCES: c_int = 13;
pub const EFAULT: c_int = 14;
pub const EBUSY: c_int = 16;
pub const EEXIST: c_int = 17;
pub const ENODEV: c_int = 19;
pub const ENOTDIR: c_int = 20;
pub const EISDIR: c_int = 21;
pub const EINVAL: c_int = 22;
pub const ENFILE: c_int = 23;
pub const EMFILE: c_int = 24;
pub const ENOTTY: c_int = 25;
pub const ENOSPC: c_int = 28;
pub const ESPIPE: c_int = 29;
pub const EROFS: c_int = 30;
pub const EPIPE: c_int = 32;
pub const EDOM: c_int = 33;
pub const ERANGE: c_int = 34;
pub const EDEADLK: c_int = 35;
pub const ENAMETOOLONG: c_int = 36;
pub const ENOLCK: c_int = 37;
pub const ENOSYS: c_int = 38;
pub const ENOTEMPTY: c_int = 39;
pub const ELOOP: c_int = 40;
pub const ENOTSUP: c_int = 95;
pub const EOVERFLOW: c_int = 75;
pub const EILSEQ: c_int = 84;
pub const ENOTSOCK: c_int = 88;
pub const EDESTADDRREQ: c_int = 89;
pub const EMSGSIZE: c_int = 90;
pub const EPROTOTYPE: c_int = 91;
pub const ENOPROTOOPT: c_int = 92;
pub const EPROTONOSUPPORT: c_int = 93;
pub const ESOCKTNOSUPPORT: c_int = 94;
pub const EOPNOTSUPP: c_int = 95;
pub const EAFNOSUPPORT: c_int = 97;
pub const EADDRINUSE: c_int = 98;
pub const EADDRNOTAVAIL: c_int = 99;
pub const ENETDOWN: c_int = 100;
pub const ENETUNREACH: c_int = 101;
pub const ENETRESET: c_int = 102;
pub const ECONNABORTED: c_int = 103;
pub const ECONNRESET: c_int = 104;
pub const ENOBUFS: c_int = 105;
pub const EISCONN: c_int = 106;
pub const ENOTCONN: c_int = 107;
pub const ESHUTDOWN: c_int = 108;
pub const ETIMEDOUT: c_int = 110;
pub const ECONNREFUSED: c_int = 111;
pub const EHOSTUNREACH: c_int = 113;
// Linux-compatible extended errno values used by rustix's libc backend.
pub const EIDRM: c_int = 43;
pub const ENOMSG: c_int = 42;
pub const EBADMSG: c_int = 74;
pub const EPROTO: c_int = 71;
pub const EMULTIHOP: c_int = 72;
pub const ENODATA: c_int = 61;
pub const ENOSTR: c_int = 60;
pub const ENOSR: c_int = 63;
pub const ENOLINK: c_int = 67;
pub const ETIME: c_int = 62;
pub const ETOOMANYREFS: c_int = 109;
pub const EHOSTDOWN: c_int = 112;
pub const ECANCELED: c_int = 125;
pub const EPFNOSUPPORT: c_int = 96;
pub const EALREADY: c_int = 114;
pub const EINPROGRESS: c_int = 115;
pub const MSG_PEEK: c_int = 0x02;
pub const POLLIN: c_short = 0x0001;
pub const POLLOUT: c_short = 0x0004;
pub const POLLERR: c_short = 0x0008;
pub const POLLHUP: c_short = 0x0010;
pub const POLLNVAL: c_short = 0x0020;
pub const SO_LINGER: c_int = 13;
// Linux-compatible socket option numbers. Several are reserved by OCTAGRAM
// and may return ENOPROTOOPT until their kernel handlers are implemented.
pub const SO_SNDBUF: c_int = 7;
pub const SO_RCVBUF: c_int = 8;
pub const SO_OOBINLINE: c_int = 10;
pub const SOCK_RDM: c_int = 4;
pub const MSG_OOB: c_int = 1;
pub const AF_UNSPEC: c_int = 0;
pub const AF_UNIX: c_int = 1;
pub const AF_INET: c_int = 2;
pub const AF_INET6: c_int = 10;
pub const SOCK_STREAM: c_int = 1;
pub const SOCK_DGRAM: c_int = 2;
pub const SOCK_RAW: c_int = 3;
pub const SOL_SOCKET: c_int = 1;
pub const SO_REUSEADDR: c_int = 2;
pub const SO_TYPE: c_int = 3;
pub const SO_ERROR: c_int = 4;
pub const SO_BROADCAST: c_int = 6;
pub const SO_KEEPALIVE: c_int = 9;
pub const SO_RCVTIMEO: c_int = 20;
pub const SO_SNDTIMEO: c_int = 21;
pub const SOMAXCONN: c_int = 128;
pub const SHUT_RD: c_int = 0;
pub const SHUT_WR: c_int = 1;
pub const SHUT_RDWR: c_int = 2;
pub const IPPROTO_IP: c_int = 0;
pub const IPPROTO_TCP: c_int = 6;
pub const IPPROTO_UDP: c_int = 17;
pub const TCP_NODELAY: c_int = 1;
pub const TCP_KEEPIDLE: c_int = 4;
pub const IP_TOS: c_int = 1;
pub const IP_TTL: c_int = 2;
pub const IP_RECVTOS: c_int = 13;
pub const IP_MULTICAST_IF: c_int = 32;
pub const IP_ADD_SOURCE_MEMBERSHIP: c_int = 39;
pub const IP_DROP_SOURCE_MEMBERSHIP: c_int = 40;
pub const IP_ADD_MEMBERSHIP: c_int = 35;
pub const IP_DROP_MEMBERSHIP: c_int = 36;
pub const IP_MULTICAST_TTL: c_int = 33;
pub const IP_MULTICAST_LOOP: c_int = 34;
pub const IPV6_UNICAST_HOPS: c_int = 16;
pub const IPV6_MULTICAST_IF: c_int = 17;
pub const IPV6_MULTICAST_HOPS: c_int = 18;
pub const IPV6_RECVTCLASS: c_int = 66;
pub const IPV6_MULTICAST_LOOP: c_int = 19;
pub const IPV6_V6ONLY: c_int = 26;
pub const IPV6_ADD_MEMBERSHIP: c_int = 20;
pub const IPV6_DROP_MEMBERSHIP: c_int = 21;
pub const EXIT_SUCCESS: c_int = 0;
pub const EXIT_FAILURE: c_int = 1;
pub const EAI_SYSTEM: c_int = -11;
// Linux-compatible value. 5 is F_GETLK, and the libc fcntl wrapper treats 5
// as a struct flock pointer argument.
pub const F_DUPFD_CLOEXEC: c_int = 1030;
pub const F_GETFD: c_int = 1;
pub const F_SETFD: c_int = 2;
pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;
// POSIX record-lock commands. The guest libc maps the whole-file form onto its
// flock syscall (see user/include/fcntl.h), which is the form rustc's file
// locks and LLVM's lock files use.
pub const F_GETLK: c_int = 5;
pub const F_SETLK: c_int = 6;
pub const F_SETLKW: c_int = 7;
pub const F_RDLCK: c_int = 0;
pub const F_WRLCK: c_int = 1;
pub const F_UNLCK: c_int = 2;
pub const LOCK_SH: c_int = 1;
pub const LOCK_EX: c_int = 2;
pub const LOCK_NB: c_int = 4;
pub const LOCK_UN: c_int = 8;
pub const FIOCLEX: c_ulong = 0x5451;
pub const FIONBIO: c_ulong = 0x5421;
// Linux-compatible request number. The guest ioctl has no FIONREAD handler
// yet, so a caller such as jobserver gets ENOTTY at runtime instead of a wrong
// count; the constant still has to exist for the crate to compile.
pub const FIONREAD: c_ulong = 0x541B;
pub const O_DIRECTORY: c_int = 0o200000;
pub const SEEK_SET: c_int = 0;
pub const SEEK_CUR: c_int = 1;
pub const SEEK_END: c_int = 2;
pub const UTIME_OMIT: c_long = 0x3ffffffe;
pub const _SC_ARG_MAX: c_int = 0;
pub const _SC_HOST_NAME_MAX: c_int = 5;
pub const _SC_GETPW_R_SIZE_MAX: c_int = 6;
pub const _SC_PAGESIZE: c_int = 30;
pub const WNOHANG: c_int = 1;
pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;
pub const SIGQUIT: c_int = 3;
pub const SIGILL: c_int = 4;
pub const SIGTRAP: c_int = 5;
pub const SIGABRT: c_int = 6;
pub const SIGBUS: c_int = 7;
pub const SIGFPE: c_int = 8;
pub const SIGKILL: c_int = 9;
pub const SIGUSR1: c_int = 10;
pub const SIGSEGV: c_int = 11;
pub const SIGUSR2: c_int = 12;
pub const SIGPIPE: c_int = 13;
pub const SIGALRM: c_int = 14;
pub const SIGTERM: c_int = 15;
pub const SIGSTKFLT: c_int = 16;
pub const SIGCHLD: c_int = 17;
pub const SIGCONT: c_int = 18;
pub const SIGSTOP: c_int = 19;
pub const SIGTSTP: c_int = 20;
pub const SIGTTIN: c_int = 21;
pub const SIGTTOU: c_int = 22;
pub const SIGURG: c_int = 23;
pub const SIGXCPU: c_int = 24;
pub const SIGXFSZ: c_int = 25;
pub const SIGVTALRM: c_int = 26;
pub const SIGPROF: c_int = 27;
pub const SIGWINCH: c_int = 28;
pub const SIGIO: c_int = 29;
pub const SIGPWR: c_int = 30;
pub const SIGSYS: c_int = 31;
pub const EOWNERDEAD: c_int = 130;
pub const ENOTRECOVERABLE: c_int = 131;

// `wait4` memakai encoding POSIX/Linux: exit code di bit 8..15, nomor sinyal
// di bit 0..6. Kernel OCTAGRAM menyimpan `(code & 0xff) << 8`.
pub const fn WIFEXITED(status: c_int) -> bool { status & 0x7f == 0 }
pub const fn WEXITSTATUS(status: c_int) -> c_int { (status >> 8) & 0xff }
pub const fn WIFSIGNALED(status: c_int) -> bool {
    ((status & 0x7f) + 1) as i8 >> 1 > 0
}
pub const fn WTERMSIG(status: c_int) -> c_int { status & 0x7f }
pub const fn WCOREDUMP(status: c_int) -> bool { WIFSIGNALED(status) && status & 0x80 != 0 }
pub const fn WIFSTOPPED(status: c_int) -> bool { status & 0xff == 0x7f }
pub const fn WSTOPSIG(status: c_int) -> c_int { (status >> 8) & 0xff }
pub const fn WIFCONTINUED(status: c_int) -> bool { status & 0xffff == 0xffff }

extern "C" {
    pub fn bind(fd: c_int, address: *const sockaddr, address_length: socklen_t) -> c_int;
    pub fn recvfrom(fd: c_int, buffer: *mut c_void, length: size_t, flags: c_int,
                    address: *mut sockaddr, address_length: *mut socklen_t) -> ssize_t;
    pub fn readv(fd: c_int, vectors: *const iovec, count: c_int) -> ssize_t;
    pub fn writev(fd: c_int, vectors: *const iovec, count: c_int) -> ssize_t;
    // POSIX string and filesystem calls exported by the OCTAGRAM C runtime.
    pub fn strerror_r(error: c_int, buffer: *mut c_char, length: size_t) -> c_int;
    pub fn unlink(path: *const c_char) -> c_int;
    pub fn unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int;
    pub fn link(old_path: *const c_char, new_path: *const c_char) -> c_int;
    pub fn linkat(old_dirfd: c_int, old_path: *const c_char, new_dirfd: c_int,
                  new_path: *const c_char, flags: c_int) -> c_int;
    pub fn rename(old_path: *const c_char, new_path: *const c_char) -> c_int;
    pub fn renameat(old_dirfd: c_int, old_path: *const c_char, new_dirfd: c_int,
                    new_path: *const c_char) -> c_int;
    pub fn flock(fd: c_int, operation: c_int) -> c_int;
    pub fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    pub fn opendir(path: *const c_char) -> *mut crate::DIR;
    pub fn fdopendir(fd: c_int) -> *mut crate::DIR;
    pub fn closedir(directory: *mut crate::DIR) -> c_int;
    pub fn readdir(directory: *mut crate::DIR) -> *mut dirent;
    pub fn dirfd(directory: *mut crate::DIR) -> c_int;
    pub fn futimens(fd: c_int, times: *const crate::timespec) -> c_int;
    pub fn utimensat(dirfd: c_int, path: *const c_char, times: *const crate::timespec, flags: c_int) -> c_int;
    pub fn setgroups(size: size_t, groups: *const gid_t) -> c_int;
    pub fn getpwuid_r(uid: uid_t, pwd: *mut passwd, buffer: *mut c_char,
                      buffer_length: size_t, result: *mut *mut passwd) -> c_int;
    pub fn getrandom(buffer: *mut c_void, length: size_t, flags: c_uint) -> ssize_t;
    pub fn clock_gettime(clock_id: clockid_t, tp: *mut crate::timespec) -> c_int;
    pub fn clock_getres(clock_id: clockid_t, res: *mut crate::timespec) -> c_int;
    pub fn nanosleep(request: *const crate::timespec, remaining: *mut crate::timespec) -> c_int;
    pub fn pthread_create(
        thread: *mut pthread_t,
        attr: *const pthread_attr_t,
        start_routine: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
    ) -> c_int;
    pub fn pthread_join(thread: pthread_t, retval: *mut *mut c_void) -> c_int;
    pub fn pthread_self() -> pthread_t;
    pub fn pthread_attr_init(attributes: *mut pthread_attr_t) -> c_int;
    pub fn pthread_attr_destroy(attributes: *mut pthread_attr_t) -> c_int;
    pub fn pthread_attr_setstacksize(attributes: *mut pthread_attr_t, stack_size: size_t) -> c_int;
    pub fn pthread_detach(thread: pthread_t) -> c_int;
    pub fn pthread_mutex_init(mutex: *mut pthread_mutex_t, attr: *const pthread_mutexattr_t) -> c_int;
    pub fn pthread_mutex_destroy(mutex: *mut pthread_mutex_t) -> c_int;
    pub fn pthread_mutexattr_init(attributes: *mut pthread_mutexattr_t) -> c_int;
    pub fn pthread_mutexattr_destroy(attributes: *mut pthread_mutexattr_t) -> c_int;
    pub fn pthread_mutexattr_settype(attributes: *mut pthread_mutexattr_t, kind: c_int) -> c_int;
    pub fn pthread_mutex_lock(mutex: *mut pthread_mutex_t) -> c_int;
    pub fn pthread_mutex_trylock(mutex: *mut pthread_mutex_t) -> c_int;
    pub fn pthread_mutex_unlock(mutex: *mut pthread_mutex_t) -> c_int;
    pub fn pthread_cond_init(cond: *mut pthread_cond_t, attr: *const pthread_condattr_t) -> c_int;
    pub fn pthread_cond_destroy(cond: *mut pthread_cond_t) -> c_int;
    pub fn pthread_condattr_init(attributes: *mut pthread_condattr_t) -> c_int;
    pub fn pthread_condattr_destroy(attributes: *mut pthread_condattr_t) -> c_int;
    pub fn pthread_condattr_setclock(attributes: *mut pthread_condattr_t, clock: clockid_t) -> c_int;
    pub fn pthread_cond_wait(cond: *mut pthread_cond_t, mutex: *mut pthread_mutex_t) -> c_int;
    pub fn pthread_cond_timedwait(
        cond: *mut pthread_cond_t,
        mutex: *mut pthread_mutex_t,
        abstime: *const crate::timespec,
    ) -> c_int;
    pub fn pthread_cond_signal(cond: *mut pthread_cond_t) -> c_int;
    pub fn pthread_cond_broadcast(cond: *mut pthread_cond_t) -> c_int;
    pub fn pthread_key_create(key: *mut pthread_key_t, destructor: Option<unsafe extern "C" fn(*mut c_void)>) -> c_int;
    pub fn pthread_key_delete(key: pthread_key_t) -> c_int;
    pub fn pthread_getspecific(key: pthread_key_t) -> *mut c_void;
    pub fn pthread_setspecific(key: pthread_key_t, value: *const c_void) -> c_int;
}

// ---------------------------------------------------------------------------
// Linux-compatible extras used by Rust's Unix PAL.
//
// OCTAGRAM's `off_t` is already 64 bit and the C library exports the plain
// names, so the `*64` entry points are ABI aliases of the base functions
// instead of new kernel interfaces.

pub type off64_t = off_t;
pub type stat64 = stat;
pub type dirent64 = dirent;

pub const MAP_FAILED: *mut c_void = -1isize as *mut c_void;

pub const SA_ONSTACK: c_int = 0x0800_0000;
pub const SA_SIGINFO: c_int = 0x0000_0004;
// From user/include/signal.h. SA_RESETHAND is the high bit, which is why it is
// written through u32 before the cast.
pub const SA_NODEFER: c_int = 0x4000_0000;
pub const SA_RESETHAND: c_int = 0x8000_0000u32 as c_int;
pub const SS_DISABLE: c_int = 2;

pub const MSG_NOSIGNAL: c_int = 0x4000;

pub const SYS_futex: c_long = 202;
pub const FUTEX_WAIT: c_int = 0;
pub const FUTEX_WAKE: c_int = 1;
pub const FUTEX_REQUEUE: c_int = 3;
pub const FUTEX_CMP_REQUEUE: c_int = 4;
pub const FUTEX_WAKE_OP: c_int = 5;
pub const FUTEX_WAIT_BITSET: c_int = 9;
pub const FUTEX_WAKE_BITSET: c_int = 10;
pub const FUTEX_PRIVATE_FLAG: c_int = 128;
pub const FUTEX_CLOCK_REALTIME: c_int = 256;
pub const FUTEX_WAIT_PRIVATE: c_int = FUTEX_WAIT | FUTEX_PRIVATE_FLAG;
pub const FUTEX_WAKE_PRIVATE: c_int = FUTEX_WAKE | FUTEX_PRIVATE_FLAG;
pub const FUTEX_REQUEUE_PRIVATE: c_int = FUTEX_REQUEUE | FUTEX_PRIVATE_FLAG;
pub const FUTEX_CMP_REQUEUE_PRIVATE: c_int = FUTEX_CMP_REQUEUE | FUTEX_PRIVATE_FLAG;
pub const FUTEX_WAKE_OP_PRIVATE: c_int = FUTEX_WAKE_OP | FUTEX_PRIVATE_FLAG;
pub const FUTEX_WAIT_BITSET_PRIVATE: c_int = FUTEX_WAIT_BITSET | FUTEX_PRIVATE_FLAG;
pub const FUTEX_WAKE_BITSET_PRIVATE: c_int = FUTEX_WAKE_BITSET | FUTEX_PRIVATE_FLAG;

s! {
    pub struct stack_t {
        pub ss_sp: *mut c_void,
        pub ss_flags: c_int,
        pub ss_size: size_t,
    }

    /// `struct flock` from user/include/fcntl.h. The name shares no namespace
    /// with the `flock()` function the module also declares.
    pub struct flock {
        pub l_type: c_short,
        pub l_whence: c_short,
        pub l_start: off_t,
        pub l_len: off_t,
        pub l_pid: pid_t,
    }

    /// `Dl_info` from user/include/dlfcn.h, filled in by the guest `dladdr`.
    pub struct Dl_info {
        pub dli_fname: *const c_char,
        pub dli_fbase: *mut c_void,
        pub dli_sname: *const c_char,
        pub dli_saddr: *mut c_void,
    }
}

const _: [(); 24] = [(); core::mem::size_of::<stack_t>()];
const _: [(); 8] = [(); core::mem::align_of::<stack_t>()];
const _: [(); 32] = [(); core::mem::size_of::<flock>()];
const _: [(); 8] = [(); core::mem::align_of::<flock>()];
const _: [(); 32] = [(); core::mem::size_of::<Dl_info>()];
const _: [(); 8] = [(); core::mem::align_of::<Dl_info>()];

extern "C" {
    pub fn syscall(number: c_long, ...) -> c_long;
    pub fn mprotect(address: *mut c_void, length: size_t, protection: c_int) -> c_int;
    pub fn sigaltstack(new_stack: *const stack_t, old_stack: *mut stack_t) -> c_int;
    pub fn sendfile(out_fd: c_int, in_fd: c_int, offset: *mut off_t, count: size_t) -> ssize_t;

    // The target has 64-bit offsets but uses the POSIX (unsuffixed) symbol
    // names. Keep both Rust API spellings mapped to the same C ABI symbols.
    pub fn open(path: *const c_char, oflag: c_int, ...) -> c_int;
    pub fn openat(dirfd: c_int, path: *const c_char, oflag: c_int, ...) -> c_int;
    pub fn stat(path: *const c_char, buf: *mut stat) -> c_int;
    pub fn lstat(path: *const c_char, buf: *mut stat) -> c_int;
    pub fn fstat(fd: c_int, buf: *mut stat) -> c_int;
    pub fn fstatat(dirfd: c_int, path: *const c_char, buf: *mut stat, flag: c_int) -> c_int;
    pub fn faccessat(dirfd: c_int, path: *const c_char, mode: c_int, flags: c_int) -> c_int;
    pub fn statvfs(path: *const c_char, buffer: *mut statvfs) -> c_int;
    pub fn fstatvfs(fd: c_int, buffer: *mut statvfs) -> c_int;
    pub fn msync(address: *mut c_void, length: size_t, flags: c_int) -> c_int;
    pub fn madvise(address: *mut c_void, length: size_t, advice: c_int) -> c_int;
    pub fn lseek(fd: c_int, offset: off_t, whence: c_int) -> off_t;
    pub fn ftruncate(fd: c_int, length: off_t) -> c_int;
    pub fn pread(fd: c_int, buf: *mut c_void, count: size_t, offset: off_t) -> ssize_t;
    pub fn pwrite(fd: c_int, buf: *const c_void, count: size_t, offset: off_t) -> ssize_t;

    #[link_name = "open"]
    pub fn open64(path: *const c_char, oflag: c_int, ...) -> c_int;
    #[link_name = "openat"]
    pub fn openat64(dirfd: c_int, path: *const c_char, oflag: c_int, ...) -> c_int;
    #[link_name = "stat"]
    pub fn stat64(path: *const c_char, buf: *mut stat) -> c_int;
    #[link_name = "lstat"]
    pub fn lstat64(path: *const c_char, buf: *mut stat) -> c_int;
    #[link_name = "fstat"]
    pub fn fstat64(fd: c_int, buf: *mut stat) -> c_int;
    #[link_name = "fstatat"]
    pub fn fstatat64(dirfd: c_int, path: *const c_char, buf: *mut stat, flag: c_int) -> c_int;
    #[link_name = "lseek"]
    pub fn lseek64(fd: c_int, offset: off64_t, whence: c_int) -> off64_t;
    #[link_name = "ftruncate"]
    pub fn ftruncate64(fd: c_int, length: off64_t) -> c_int;
    #[link_name = "pread"]
    pub fn pread64(fd: c_int, buf: *mut c_void, count: size_t, offset: off64_t) -> ssize_t;
    #[link_name = "pwrite"]
    pub fn pwrite64(fd: c_int, buf: *const c_void, count: size_t, offset: off64_t) -> ssize_t;
    #[link_name = "readdir"]
    pub fn readdir64(dirp: *mut crate::DIR) -> *mut dirent64;
}

// Ancillary data. The OCTAGRAM guest rejects SCM_RIGHTS at runtime, but std and
// its users still name these symbols while building, so the ABI-matching
// definitions live here.
pub const SCM_RIGHTS: c_int = 1;
pub const SCM_CREDENTIALS: c_int = 2;

s! {
    pub struct cmsghdr {
        pub cmsg_len: size_t,
        pub cmsg_level: c_int,
        pub cmsg_type: c_int,
    }
}

const _: [(); 16] = [(); core::mem::size_of::<cmsghdr>()];

pub use crate::iovec;

s! {
    pub struct msghdr {
        pub msg_name: *mut c_void,
        pub msg_namelen: socklen_t,
        pub msg_iov: *mut iovec,
        pub msg_iovlen: size_t,
        pub msg_control: *mut c_void,
        pub msg_controllen: size_t,
        pub msg_flags: c_int,
    }
}

pub const fn CMSG_ALIGN(length: usize) -> usize {
    (length + core::mem::size_of::<usize>() - 1) & !(core::mem::size_of::<usize>() - 1)
}

pub const fn CMSG_SPACE(length: u32) -> u32 {
    (CMSG_ALIGN(core::mem::size_of::<cmsghdr>()) + CMSG_ALIGN(length as usize)) as u32
}

pub const fn CMSG_LEN(length: u32) -> u32 {
    (CMSG_ALIGN(core::mem::size_of::<cmsghdr>()) + length as usize) as u32
}

pub unsafe fn CMSG_DATA(cmsg: *const cmsghdr) -> *mut c_uchar {
    (cmsg as *mut c_uchar).add(CMSG_ALIGN(core::mem::size_of::<cmsghdr>()))
}

pub unsafe fn CMSG_FIRSTHDR(mhdr: *const msghdr) -> *mut cmsghdr {
    if (*mhdr).msg_controllen >= core::mem::size_of::<cmsghdr>() as _ {
        (*mhdr).msg_control as *mut cmsghdr
    } else {
        core::ptr::null_mut()
    }
}

pub unsafe fn CMSG_NXTHDR(mhdr: *const msghdr, cmsg: *const cmsghdr) -> *mut cmsghdr {
    let next = (cmsg as *const c_uchar).add(CMSG_ALIGN((*cmsg).cmsg_len)) as *const cmsghdr;
    let end = (*mhdr).msg_control as *const c_uchar;
    if next.is_null() || (next as *const c_uchar) >= end.add((*mhdr).msg_controllen) {
        core::ptr::null_mut()
    } else {
        next as *mut cmsghdr
    }
}

// ---------------------------------------------------------------------------
// POSIX surface the OCTAGRAM headers already provide.

pub const SOCK_CLOEXEC: c_int = 0o2000000;
pub const MSG_CTRUNC: c_int = 0x08;
pub const MSG_TRUNC: c_int = 0x20;

pub const WEXITED: c_int = 4;
pub const WUNTRACED: c_int = 2;
pub const WCONTINUED: c_int = 8;

pub const GRND_NONBLOCK: c_int = 0x0001;

pub const CPU_SETSIZE: c_int = 128;

s! {
    pub struct siginfo_t {
        pub si_signo: c_int,
        pub si_errno: c_int,
        pub si_code: c_int,
        pub si_pid: pid_t,
        pub si_uid: uid_t,
        status_code: c_int,
    }

    pub struct cpu_set_t {
        pub bits: [c_ulong; 2],
    }
}

const _: [(); 24] = [(); core::mem::size_of::<siginfo_t>()];
const _: [(); 16] = [(); core::mem::size_of::<cpu_set_t>()];

pub fn CPU_ZERO(set: &mut cpu_set_t) {
    set.bits = [0; 2];
}

pub fn CPU_SET(cpu: usize, set: &mut cpu_set_t) {
    if cpu < CPU_SETSIZE as usize {
        set.bits[cpu / 64] |= 1 << (cpu % 64);
    }
}

pub fn CPU_CLR(cpu: usize, set: &mut cpu_set_t) {
    if cpu < CPU_SETSIZE as usize {
        set.bits[cpu / 64] &= !(1 << (cpu % 64));
    }
}

pub fn CPU_ISSET(cpu: usize, set: &cpu_set_t) -> bool {
    cpu < CPU_SETSIZE as usize && set.bits[cpu / 64] & (1 << (cpu % 64)) != 0
}

extern "C" {
    pub fn sendmsg(fd: c_int, msg: *const msghdr, flags: c_int) -> ssize_t;
    pub fn recvmsg(fd: c_int, msg: *mut msghdr, flags: c_int) -> ssize_t;
}

// ---------------------------------------------------------------------------
// Linux ABI names used by std's Unix PAL.
//
// OCTAGRAM accepts the Linux syscall numbers and socket flags below. Where the
// guest has no implementation (pidfd, credential passing, TCP tuning) the value
// is still the Linux one: std only needs the name while building, and the
// corresponding call fails with ENOSYS/EINVAL at runtime instead of silently
// doing something else.

pub const SOCK_SEQPACKET: c_int = 5;
pub const SO_MARK: c_int = 36;
pub const SO_PASSCRED: c_int = 16;
pub const MSG_CMSG_CLOEXEC: c_int = 0x4000_0000;
pub const TCP_DEFER_ACCEPT: c_int = 9;
pub const TCP_QUICKACK: c_int = 12;
pub const GRND_INSECURE: c_int = 0x0004;
pub const UIO_MAXIOV: c_int = 1024;
pub const SYS_pidfd_open: c_long = 434;
pub const SYS_pidfd_send_signal: c_long = 424;

pub const CLD_EXITED: c_int = 1;
pub const CLD_KILLED: c_int = 2;
pub const CLD_DUMPED: c_int = 3;
pub const CLD_TRAPPED: c_int = 4;
pub const CLD_STOPPED: c_int = 5;
pub const CLD_CONTINUED: c_int = 6;

impl siginfo_t {
    /// The address that faulted, for the signals that carry one.
    pub unsafe fn si_addr(&self) -> *mut c_void {
        // The OCTAGRAM `siginfo_t` is the short POSIX form and does not carry
        // a fault address; report "no address" rather than reading past it.
        core::ptr::null_mut()
    }

    pub unsafe fn si_status(&self) -> c_int {
        self.status_code
    }
}

// ---------------------------------------------------------------------------
// Remaining POSIX names the OCTAGRAM headers export.

pub const SOCK_NONBLOCK: c_int = 0o4000;
pub const SIGSTKSZ: size_t = 8192;
pub const MINSIGSTKSZ: size_t = 2048;

// Linux flag values, used by std's Unix PAL. OCTAGRAM ignores MAP_STACK (the
// kernel has no separate stack mappings) and has no O_PATH.
pub const MAP_STACK: c_int = 0x20000;
pub const O_PATH: c_int = 0o10000000;
pub const MSG_EOR: c_int = 0x80;

pub const AT_MINSIGSTKSZ: c_ulong = 51;

extern "C" {
    // Cargo's Unix process helpers use these; the guest runtime exports them.
    pub fn setsid() -> pid_t;
    pub fn umask(mask: mode_t) -> mode_t;

    pub fn pipe2(fds: *mut c_int, flags: c_int) -> c_int;
    pub fn preadv(fd: c_int, iov: *const iovec, iovcnt: c_int, offset: off_t) -> ssize_t;
    pub fn pwritev(fd: c_int, iov: *const iovec, iovcnt: c_int, offset: off_t) -> ssize_t;
    pub fn getauxval(kind: c_ulong) -> c_ulong;
    // glibc-compatible introspection the guest runtime implements
    // (user/include/execinfo.h and user/include/dlfcn.h). A static binary
    // yields no symbol names, which the callers already handle.
    pub fn backtrace(buffer: *mut *mut c_void, size: c_int) -> c_int;
    pub fn dladdr(address: *const c_void, info: *mut Dl_info) -> c_int;
}

impl siginfo_t {
    /// Sending process, for the signals that carry one.
    pub unsafe fn si_pid(&self) -> pid_t {
        self.si_pid
    }
}

// ---------------------------------------------------------------------------
// Thread attributes and process-info surface.
//
// `pthread_attr_getstack`/`getguardsize` only read the attribute block std
// filled in, so they are implemented here instead of in the guest libc.
// `accept4` and `waitid` are not exported by the guest yet; the constants and
// signatures are Linux-compatible so std compiles and the call fails cleanly.

pub const P_PID: c_int = 1;
pub const P_PIDFD: c_int = 3;
pub const PIDFD_SIGNAL_PROCESS_GROUP: c_uint = 1;
pub const PIDFD_GET_INFO: c_ulong = 0xb0ff;
pub const PIDFD_INFO_EXIT: u64 = 1 << 0;

s! {
    pub struct pidfd_info {
        pub mask: u64,
        pub cgroupid: u64,
        pub pid: u32,
        pub tgid: u32,
        pub exit_code: i32,
    }
}

pub unsafe fn pthread_attr_getstack(
    attr: *const pthread_attr_t,
    stackaddr: *mut *mut c_void,
    stacksize: *mut size_t,
) -> c_int {
    if attr.is_null() || stackaddr.is_null() || stacksize.is_null() {
        return crate::EINVAL;
    }
    // The guest attribute block does not track the base address; the size is
    // what std needs to derive the guard page.
    *stackaddr = core::ptr::null_mut();
    *stacksize = (*attr).stack_size;
    0
}

pub unsafe fn pthread_attr_getguardsize(attr: *const pthread_attr_t, guardsize: *mut size_t) -> c_int {
    if attr.is_null() || guardsize.is_null() {
        return crate::EINVAL;
    }
    *guardsize = 0;
    0
}

pub unsafe fn pthread_getattr_np(thread: pthread_t, attr: *mut pthread_attr_t) -> c_int {
    let _ = thread;
    if attr.is_null() {
        return crate::EINVAL;
    }
    // The guest does not expose a running thread's stack bounds; report failure
    // so std falls back to "stack bounds unknown" instead of a wrong range.
    crate::ENOTSUP
}

pub unsafe fn accept4(
    fd: c_int,
    addr: *mut crate::sockaddr,
    len: *mut socklen_t,
    flags: c_int,
) -> c_int {
    let fd = crate::accept(fd, addr, len);
    if fd < 0 {
        return fd;
    }
    if flags & SOCK_NONBLOCK != 0 {
        let current = crate::fcntl(fd, crate::F_GETFL);
        if current < 0 || crate::fcntl(fd, crate::F_SETFL, current | crate::O_NONBLOCK) < 0 {
            crate::close(fd);
            return -1;
        }
    }
    if flags & SOCK_CLOEXEC != 0 {
        if crate::fcntl(fd, F_SETFD, crate::FD_CLOEXEC) < 0 {
            crate::close(fd);
            return -1;
        }
    }
    fd
}

extern "C" {
    pub fn clock_nanosleep(
        clock_id: clockid_t,
        flags: c_int,
        request: *const crate::timespec,
        remain: *mut crate::timespec,
    ) -> c_int;
}

// Peer credentials over SCM_CREDENTIALS/SO_PEERCRED. The guest does not fill
// them in yet, but std's Unix credential type needs the ABI shape.
pub const SO_PEERCRED: c_int = 17;

s! {
    pub struct ucred {
        pub pid: pid_t,
        pub uid: uid_t,
        pub gid: gid_t,
    }
}

const _: [(); 12] = [(); core::mem::size_of::<ucred>()];

// ---------------------------------------------------------------------------
// Entry points that the guest libc does not export yet.
//
// `__xpg_strerror_r` is the POSIX flavour of `strerror_r` that the gnu target
// env selects, `waitid` is emulated on top of `waitpid`, and `pthread_kill`
// reports `ENOSYS` because OCTAGRAM has no per-thread signal delivery.

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __xpg_strerror_r(
    errnum: c_int,
    buf: *mut c_char,
    buflen: size_t,
) -> c_int {
    let message = crate::strerror(errnum);
    if message.is_null() {
        return crate::EINVAL;
    }
    let length = crate::strlen(message);
    if length >= buflen {
        if buflen > 0 {
            core::ptr::copy_nonoverlapping(message, buf, buflen - 1);
            *buf.add(buflen - 1) = 0;
        }
        return crate::ERANGE;
    }
    core::ptr::copy_nonoverlapping(message, buf, length + 1);
    0
}

/// OCTAGRAM has no `tgkill`; `kill` only addresses a whole process, and
/// delivering the signal there would run the handler on an arbitrary thread
/// instead of the one the caller named. Report the missing facility instead:
/// callers that use this as a best-effort wakeup, such as jobserver's helper
/// thread cleanup, already ignore the result and keep working.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_kill(thread: pthread_t, signal: c_int) -> c_int {
    let _ = (thread, signal);
    ENOSYS
}

pub unsafe extern "C" fn waitid(
    idtype: c_int,
    id: id_t,
    info: *mut siginfo_t,
    options: c_int,
) -> c_int {
    if info.is_null() {
        return -1;
    }
    let pid = match idtype {
        P_PID => id as pid_t,
        _ => return -1,
    };
    let mut status: c_int = 0;
    let waited = crate::waitpid(pid, &mut status, options & crate::WNOHANG);
    if waited < 0 {
        return -1;
    }
    if waited == 0 {
        return 0;
    }
    (*info).si_signo = crate::SIGCHLD;
    (*info).si_errno = 0;
    (*info).si_pid = waited;
    (*info).si_uid = 0;
    let exited = status & 0x7f == 0;
    (*info).si_code = if exited { CLD_EXITED } else { CLD_KILLED };
    (*info).status_code = if exited { (status >> 8) & 0xff } else { status & 0x7f };
    0
}

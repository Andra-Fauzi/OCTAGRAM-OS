//! OCTAGRAM x86_64 userspace ABI.
//!
//! These definitions describe OCTAGRAM's public C ABI. They intentionally do
//! not reuse Linux or Redox structure layouts. Keep this module synchronized
//! with the OCTAGRAM public headers and ABI validator.

use crate::prelude::*;

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
    pub struct stat {
        pub st_size: off_t,
        pub st_mode: mode_t,
        pub st_dev: dev_t,
        pub st_ino: ino_t,
        pub st_nlink: nlink_t,
        pub st_uid: uid_t,
        pub st_gid: gid_t,
        pub st_atime: time_t,
        pub st_mtime: time_t,
        pub st_ctime: time_t,
        pub st_blksize: blksize_t,
        pub st_blocks: blkcnt_t,
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

const _: [(); 88] = [(); core::mem::size_of::<stat>()];
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
pub const AT_FDCWD: c_int = -100;
pub const AT_SYMLINK_NOFOLLOW: c_int = 1 << 8;
pub const AT_REMOVEDIR: c_int = 1 << 9;

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

pub const S_IFMT: mode_t = 0o170000;
pub const S_IFCHR: mode_t = 0o020000;
pub const S_IFDIR: mode_t = 0o040000;
pub const S_IFLNK: mode_t = 0o120000;
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
pub const EALREADY: c_int = 114;
pub const EINPROGRESS: c_int = 115;
pub const MSG_PEEK: c_int = 0x02;
pub const SO_LINGER: c_int = 13;
pub const EXIT_SUCCESS: c_int = 0;
pub const EXIT_FAILURE: c_int = 1;
pub const EOWNERDEAD: c_int = 130;
pub const ENOTRECOVERABLE: c_int = 131;

extern "C" {
    pub fn clock_gettime(clock_id: clockid_t, tp: *mut crate::timespec) -> c_int;
    pub fn clock_getres(clock_id: clockid_t, res: *mut crate::timespec) -> c_int;
    pub fn nanosleep(request: *const crate::timespec, remaining: *mut crate::timespec) -> c_int;
    pub fn pthread_create(
        thread: *mut pthread_t,
        attr: *const pthread_attr_t,
        start_routine: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
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

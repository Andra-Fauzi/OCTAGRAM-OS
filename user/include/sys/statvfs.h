/*
 * <sys/statvfs.h> for OCTAGRAM.
 *
 * libc++'s <filesystem> includes this header from posix_compat.h and uses
 * statvfs()/fstatvfs() only for std::filesystem::space(). OCTAGRAM has no
 * filesystem-capacity syscall yet, so the calls report ENOSYS instead of
 * inventing sizes; space() therefore reports "function not implemented"
 * rather than a plausible but false number.
 */
#ifndef _OCTAGRAM_SYS_STATVFS_H
#define _OCTAGRAM_SYS_STATVFS_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifndef _OCTAGRAM_FSBLKCNT_T
#define _OCTAGRAM_FSBLKCNT_T
typedef unsigned long fsblkcnt_t;
typedef unsigned long fsfilcnt_t;
#endif

struct statvfs {
    unsigned long f_bsize;   /* filesystem block size */
    unsigned long f_frsize;  /* fragment size */
    fsblkcnt_t f_blocks;     /* size of filesystem in f_frsize units */
    fsblkcnt_t f_bfree;      /* free blocks */
    fsblkcnt_t f_bavail;     /* free blocks for unprivileged users */
    fsfilcnt_t f_files;      /* inodes */
    fsfilcnt_t f_ffree;      /* free inodes */
    fsfilcnt_t f_favail;     /* free inodes for unprivileged users */
    unsigned long f_fsid;    /* filesystem id */
    unsigned long f_flag;    /* mount flags */
    unsigned long f_namemax; /* maximum filename length */
};

/* f_flag bits (POSIX names). */
#define ST_RDONLY 0x0001
#define ST_NOSUID 0x0002

int statvfs(const char *path, struct statvfs *buffer);
int fstatvfs(int descriptor, struct statvfs *buffer);

#ifdef __cplusplus
}
#endif

#endif /* _OCTAGRAM_SYS_STATVFS_H */

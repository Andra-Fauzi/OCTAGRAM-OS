#ifndef OCTAGRAM_SYS_STAT_H
#define OCTAGRAM_SYS_STAT_H

#include <sys/types.h>
#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif


#define S_IFMT  0170000
#define S_IFIFO 0010000
#define S_IFCHR 0020000
#define S_IFBLK 0060000
#define S_IFDIR 0040000
#define S_IFLNK 0120000
#define S_IFSOCK 0140000
#define S_IFREG 0100000

#define S_IRUSR 0400
#define S_IWUSR 0200
#define S_IXUSR 0100
#define S_IRGRP 0040
#define S_IWGRP 0020
#define S_IXGRP 0010
#define S_IROTH 0004
#define S_IWOTH 0002
#define S_IXOTH 0001
#define S_IRWXU 0700
#define S_IRWXG 0070
#define S_IRWXO 0007

struct stat {
    off_t st_size;
    mode_t st_mode;
    dev_t st_dev;
    ino_t st_ino;
    nlink_t st_nlink;
    uid_t st_uid;
    gid_t st_gid;
    /*
     * No device nodes exist yet, so libc reports 0. The field is part of the
     * public layout because Rust's std (and any `st_rdev` user) addresses it.
     */
    dev_t st_rdev;
    blksize_t st_blksize;
    blkcnt_t st_blocks;
    /* POSIX timespec members. The st_atime/st_mtime/st_ctime and
     * st_*time_nsec spellings below are the traditional glibc aliases into
     * them, so existing OCTAGRAM code keeps compiling while libc++'s
     * <filesystem> can read st_mtim/st_atim/st_ctim directly. The kernel only
     * records whole seconds of modification time, so the nsec fields stay 0. */
    struct timespec st_atim;
    struct timespec st_mtim;
    struct timespec st_ctim;
};

#define st_atime st_atim.tv_sec
#define st_mtime st_mtim.tv_sec
#define st_ctime st_ctim.tv_sec
#define st_atime_nsec st_atim.tv_nsec
#define st_mtime_nsec st_mtim.tv_nsec
#define st_ctime_nsec st_ctim.tv_nsec

#define S_ISCHR(mode) (((mode) & S_IFMT) == S_IFCHR)
#define S_ISDIR(mode) (((mode) & S_IFMT) == S_IFDIR)
#define S_ISLNK(mode) (((mode) & S_IFMT) == S_IFLNK)
#define S_ISREG(mode) (((mode) & S_IFMT) == S_IFREG)
#define S_ISBLK(mode) (((mode) & S_IFMT) == S_IFBLK)
#define S_ISFIFO(mode) (((mode) & S_IFMT) == S_IFIFO)
#define S_ISSOCK(mode) (((mode) & S_IFMT) == S_IFSOCK)

int chmod(const char *path, mode_t mode);
int mkdir(const char *path, mode_t mode);
int stat(const char *path, struct stat *result);
int lstat(const char *path, struct stat *result);
int fstat(int fd, struct stat *result);
int fstatat(int dirfd, const char *path, struct stat *result, int flags);
int fchmod(int fd, mode_t mode);
int mkdirat(int dirfd, const char *path, mode_t mode);
mode_t umask(mode_t mask);

/* utimensat/futimens are declared in <unistd.h>; OCTAGRAM has no timestamp
 * syscall, so they report ENOSYS rather than doing nothing silently. */


#ifdef __cplusplus
}
#endif

#endif

#ifndef OCTAGRAM_SYS_STAT_H
#define OCTAGRAM_SYS_STAT_H

#include <sys/types.h>

#define S_IFMT  0170000
#define S_IFCHR 0020000
#define S_IFDIR 0040000
#define S_IFLNK 0120000
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
};

#define S_ISCHR(mode) (((mode) & S_IFMT) == S_IFCHR)
#define S_ISDIR(mode) (((mode) & S_IFMT) == S_IFDIR)
#define S_ISLNK(mode) (((mode) & S_IFMT) == S_IFLNK)
#define S_ISREG(mode) (((mode) & S_IFMT) == S_IFREG)

int chmod(const char *path, mode_t mode);
int mkdir(const char *path, mode_t mode);
int stat(const char *path, struct stat *result);
int lstat(const char *path, struct stat *result);
int fstat(int fd, struct stat *result);
int fstatat(int dirfd, const char *path, struct stat *result, int flags);
int fchmod(int fd, mode_t mode);
int mkdirat(int dirfd, const char *path, mode_t mode);
mode_t umask(mode_t mask);

#endif

#ifndef OCTAGRAM_FCNTL_H
#define OCTAGRAM_FCNTL_H

#ifdef __cplusplus
extern "C" {
#endif

#include "octagram.h"

#define O_RDONLY OCT_O_RDONLY
#define O_WRONLY OCT_O_WRONLY
#define O_RDWR OCT_O_RDWR
#define O_ACCMODE 3
#define O_CREAT OCT_O_CREAT
#define O_TRUNC OCT_O_TRUNC
#define O_APPEND OCT_O_APPEND
#define O_NONBLOCK OCT_O_NONBLOCK
#define O_EXCL OCT_O_EXCL
#define O_CLOEXEC 02000000
#define O_NOFOLLOW 00400000
#define AT_FDCWD OCT_AT_FDCWD
#define AT_SYMLINK_NOFOLLOW OCT_AT_SYMLINK_NOFOLLOW
#define AT_REMOVEDIR OCT_AT_REMOVEDIR

#define F_DUPFD 0
#define F_DUPFD_CLOEXEC 5
#define O_DIRECTORY 00200000

/* Reserves space for [offset, offset+length). Returns 0 or an error number
 * (not -1), as POSIX specifies. */
int posix_fallocate(int descriptor, off_t offset, off_t length);
#define F_GETFD 1
#define F_SETFD 2
#define F_GETFL 3
#define F_SETFL 4
#define FD_CLOEXEC 1

int open(const char *path, int flags, ...);
int openat(int dirfd, const char *path, int flags, ...);
int fcntl(int fd, int command, ...);


#ifdef __cplusplus
}
#endif
#endif

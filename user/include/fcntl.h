#ifndef OCTAGRAM_FCNTL_H
#define OCTAGRAM_FCNTL_H

#include "octagram.h"

#define O_RDONLY OCT_O_RDONLY
#define O_WRONLY OCT_O_WRONLY
#define O_RDWR OCT_O_RDWR
#define O_ACCMODE 3
#define O_CREAT OCT_O_CREAT
#define O_TRUNC OCT_O_TRUNC
#define O_APPEND OCT_O_APPEND
#define O_NONBLOCK OCT_O_NONBLOCK
#define O_CLOEXEC 02000000
#define O_NOFOLLOW 00400000
#ifndef O_EXCL
#define O_EXCL 0200
#endif
#define AT_FDCWD OCT_AT_FDCWD
#define AT_SYMLINK_NOFOLLOW OCT_AT_SYMLINK_NOFOLLOW
#define AT_REMOVEDIR OCT_AT_REMOVEDIR

#define F_DUPFD 0
#define F_GETFD 1
#define F_SETFD 2
#define F_GETFL 3
#define F_SETFL 4
#define FD_CLOEXEC 1

int open(const char *path, int flags, ...);
int openat(int dirfd, const char *path, int flags, ...);
int fcntl(int fd, int command, ...);

#endif

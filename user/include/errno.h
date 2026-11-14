#ifndef OCTAGRAM_ERRNO_H
#define OCTAGRAM_ERRNO_H

#ifdef OCTAGRAM_TLS_ERRNO
extern __thread int errno;
#else
extern int errno;
#endif

#define EACCES 13
#define EAGAIN 11
#define EBADF 9
#define E2BIG 7
#define EBUSY 16
#define ECHILD 10
#define EDEADLK 35
#define EEXIST 17
#define EFAULT 14
#define EILSEQ 84
#define EINTR 4
#define EINVAL 22
#define EIO 5
#define EISDIR 21
#define ELOOP 40
#define EMFILE 24
#define ENAMETOOLONG 36
#define ENODEV 19
#define ENOENT 2
#define ENOEXEC 8
#define ENOMEM 12
#define ENOSPC 28
#define ENOSYS 38
#define ENOTDIR 20
#define ENOTEMPTY 39
#define ENOTTY 25
#define ENOTSUP 95
#define EOVERFLOW 75
#define EPIPE 32
#define ERANGE 34
#define EROFS 30
#define ESPIPE 29

#endif

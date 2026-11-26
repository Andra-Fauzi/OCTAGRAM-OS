#ifndef OCTAGRAM_SYS_TYPES_H
#define OCTAGRAM_SYS_TYPES_H

#include <stddef.h>
#include <stdint.h>

/* POSIX requires sys/types.h to provide size_t as well; portable code relies on
 * that (LLVM's bundled BSD regex uses size_t after including only this header). */
typedef int64_t off_t;
typedef int64_t ssize_t;
typedef int32_t pid_t;
typedef int32_t id_t;
typedef uint32_t mode_t;
typedef int64_t time_t;
typedef int64_t suseconds_t;
typedef int64_t clock_t;
typedef int32_t clockid_t;
typedef uint32_t uid_t;
typedef uint32_t gid_t;
typedef uint32_t useconds_t;
typedef uint64_t dev_t;
typedef uint64_t ino_t;
typedef uint32_t nlink_t;
typedef int64_t blksize_t;
typedef int64_t blkcnt_t;

#endif

#ifndef OCTAGRAM_SYS_MMAN_H
#define OCTAGRAM_SYS_MMAN_H

#include <stddef.h>
#include <sys/types.h>

#define PROT_NONE  0x0
#define PROT_READ  0x1
#define PROT_WRITE 0x2
#define PROT_EXEC  0x4

#define MAP_SHARED    0x01
#define MAP_PRIVATE   0x02
#define MAP_FIXED     0x10
#define MAP_ANONYMOUS 0x20
#define MAP_ANON MAP_ANONYMOUS

#define MS_ASYNC      0x01
#define MS_INVALIDATE 0x02
#define MS_SYNC       0x04

#define MCL_CURRENT 0x01
#define MCL_FUTURE  0x02

#define POSIX_MADV_NORMAL     0
#define POSIX_MADV_RANDOM     1
#define POSIX_MADV_SEQUENTIAL 2
#define POSIX_MADV_WILLNEED   3
#define POSIX_MADV_DONTNEED   4

#define MAP_FAILED ((void *)-1)

void *mmap(void *address, size_t length, int protection, int flags,
           int file_descriptor, off_t offset);
int munmap(void *address, size_t length);
int mprotect(void *address, size_t length, int protection);
int msync(void *address, size_t length, int flags);
int mincore(void *address, size_t length, unsigned char *vector);
int mlock(const void *address, size_t length);
int munlock(const void *address, size_t length);
int mlockall(int flags);
int munlockall(void);
int posix_madvise(void *address, size_t length, int advice);

#endif

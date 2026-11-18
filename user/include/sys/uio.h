#ifndef OCTAGRAM_SYS_UIO_H
#define OCTAGRAM_SYS_UIO_H

#include <stddef.h>
#include <sys/types.h>

struct iovec {
    void *iov_base;
    size_t iov_len;
};

ssize_t readv(int fd, const struct iovec *vectors, int vector_count);
ssize_t writev(int fd, const struct iovec *vectors, int vector_count);
ssize_t preadv(int fd, const struct iovec *vectors, int vector_count,
               off_t offset);
ssize_t pwritev(int fd, const struct iovec *vectors, int vector_count,
                off_t offset);
ssize_t preadv2(int fd, const struct iovec *vectors, int vector_count,
                off_t offset, int flags);
ssize_t pwritev2(int fd, const struct iovec *vectors, int vector_count,
                 off_t offset, int flags);

#endif

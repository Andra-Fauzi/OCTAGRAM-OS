#ifndef OCTAGRAM_SYS_SELECT_H
#define OCTAGRAM_SYS_SELECT_H

#include <stdint.h>
#include <sys/time.h>
#include <sys/types.h>

#define FD_SETSIZE 1024

typedef struct {
    uint64_t bits[FD_SETSIZE / 64];
} fd_set;

#define FD_ZERO(set) __builtin_memset((set), 0, sizeof(fd_set))
#define FD_SET(fd, set) \
    ((set)->bits[(fd) / 64] |= 1ULL << ((fd) % 64))
#define FD_CLR(fd, set) \
    ((set)->bits[(fd) / 64] &= ~(1ULL << ((fd) % 64)))
#define FD_ISSET(fd, set) \
    (((set)->bits[(fd) / 64] & (1ULL << ((fd) % 64))) != 0)

int select(int nfds, fd_set *readfds, fd_set *writefds,
           fd_set *exceptfds, struct timeval *timeout);

#endif

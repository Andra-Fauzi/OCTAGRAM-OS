#ifndef OCTAGRAM_POLL_H
#define OCTAGRAM_POLL_H

#include <stddef.h>
#include <stdint.h>

typedef unsigned long nfds_t;

struct pollfd {
    int fd;
    int16_t events;
    int16_t revents;
};

#define POLLIN 0x0001
#define POLLOUT 0x0004
#define POLLERR 0x0008
#define POLLHUP 0x0010
#define POLLNVAL 0x0020

int poll(struct pollfd *fds, nfds_t nfds, int timeout);

#endif

#ifndef OCTAGRAM_SYS_EPOLL_H
#define OCTAGRAM_SYS_EPOLL_H

#include <stdint.h>

#define EPOLLIN 0x0001u
#define EPOLLOUT 0x0004u
#define EPOLLERR 0x0008u
#define EPOLLHUP 0x0010u
#define EPOLLRDHUP 0x2000u
#define EPOLLET 0x80000000u
#define EPOLLONESHOT 0x40000000u

#define EPOLL_CTL_ADD 1
#define EPOLL_CTL_DEL 2
#define EPOLL_CTL_MOD 3

#define EPOLL_CLOEXEC 02000000

struct epoll_event {
    uint32_t events;
    uint64_t data;
} __attribute__((packed));

int epoll_create1(int flags);
int epoll_ctl(int epfd, int operation, int fd, struct epoll_event *event);
int epoll_wait(int epfd, struct epoll_event *events, int maxevents,
               int timeout);

#endif

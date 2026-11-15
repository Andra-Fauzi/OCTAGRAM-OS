#ifndef OCTAGRAM_SYS_SOCKET_H
#define OCTAGRAM_SYS_SOCKET_H

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>

typedef uint32_t socklen_t;
typedef uint16_t sa_family_t;

struct sockaddr {
    sa_family_t sa_family;
    char sa_data[14];
};

#define AF_UNSPEC 0
#define AF_INET 2

#define SOCK_STREAM 1
#define SOCK_DGRAM 2
#define SOCK_RAW 3

#define MSG_PEEK 0x02
#define MSG_DONTWAIT 0x40
#define MSG_WAITALL 0x100
#define MSG_NOSIGNAL 0x4000

#define SOL_SOCKET 1
#define SO_TYPE 3
#define SO_ERROR 4

#define SHUT_RD 0
#define SHUT_WR 1
#define SHUT_RDWR 2

int socket(int domain, int type, int protocol);
int bind(int socket_fd, const struct sockaddr *address, socklen_t address_length);
int listen(int socket_fd, int backlog);
int accept(int socket_fd, struct sockaddr *address, socklen_t *address_length);
int connect(int socket_fd, const struct sockaddr *address, socklen_t address_length);
ssize_t send(int socket_fd, const void *buffer, size_t length, int flags);
ssize_t recv(int socket_fd, void *buffer, size_t length, int flags);
int shutdown(int socket_fd, int how);
int getsockname(int socket_fd, struct sockaddr *address, socklen_t *address_length);
int getpeername(int socket_fd, struct sockaddr *address, socklen_t *address_length);

#endif

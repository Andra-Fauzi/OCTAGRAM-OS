#ifndef OCTAGRAM_SYS_SOCKET_H
#define OCTAGRAM_SYS_SOCKET_H

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
#include <sys/uio.h>

typedef uint32_t socklen_t;
typedef uint16_t sa_family_t;

struct sockaddr {
    sa_family_t sa_family;
    char sa_data[14];
};

struct sockaddr_un {
    sa_family_t sun_family;
    char sun_path[108];
};

struct sockaddr_storage {
    sa_family_t ss_family;
    unsigned char __data[118];
    uint64_t __align;
};

#define AF_UNSPEC 0
#define AF_INET 2
#define AF_INET6 10

#define AF_UNIX 1
#define AF_INET 2
#define SOCK_STREAM 1
#define SOCK_DGRAM 2
#define SOCK_RAW 3
#define SOCK_CLOEXEC 02000000
#define SOCK_NONBLOCK 00004000

#define MSG_CTRUNC 0x08
#define MSG_TRUNC 0x20
#define MSG_PEEK 0x02
#define MSG_DONTWAIT 0x40
#define MSG_WAITALL 0x100
#define MSG_NOSIGNAL 0x4000

#define SOL_SOCKET 1
#define SO_REUSEADDR 2
#define SO_TYPE 3
#define SO_ERROR 4
#define SO_KEEPALIVE 9
#define SO_BROADCAST 6
#define SO_RCVTIMEO 20
#define SO_SNDTIMEO 21
#define SOMAXCONN 128
/* Reserved ABI value; OCTAGRAM currently rejects SO_LINGER with ENOPROTOOPT. */
#define SO_LINGER 13

#define SHUT_RD 0
#define SHUT_WR 1
#define SHUT_RDWR 2

struct msghdr {
    void *msg_name;
    socklen_t msg_namelen;
    struct iovec *msg_iov;
    size_t msg_iovlen;
    void *msg_control;
    size_t msg_controllen;
    int msg_flags;
};

struct cmsghdr {
    size_t cmsg_len;
    int cmsg_level;
    int cmsg_type;
};

#define CMSG_ALIGN(length) \
    (((length) + sizeof(size_t) - 1) & ~(sizeof(size_t) - 1))
#define CMSG_DATA(header) \
    ((unsigned char *)(header) + CMSG_ALIGN(sizeof(struct cmsghdr)))
#define CMSG_SPACE(length) \
    (CMSG_ALIGN(sizeof(struct cmsghdr)) + CMSG_ALIGN(length))
#define CMSG_LEN(length) (CMSG_ALIGN(sizeof(struct cmsghdr)) + (length))
#define CMSG_FIRSTHDR(message) \
    ((message)->msg_controllen >= sizeof(struct cmsghdr) \
         ? (struct cmsghdr *)(message)->msg_control \
         : NULL)
#define CMSG_NXTHDR(message, header) \
    ((header) == NULL \
         ? CMSG_FIRSTHDR(message) \
         : (((unsigned char *)(header) + CMSG_ALIGN((header)->cmsg_len) \
             + CMSG_ALIGN(sizeof(struct cmsghdr)) \
             > (unsigned char *)(message)->msg_control \
                   + (message)->msg_controllen) \
                ? NULL \
                : (struct cmsghdr *)((unsigned char *)(header) \
                                     + CMSG_ALIGN((header)->cmsg_len))))

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
int setsockopt(int socket_fd, int level, int option_name,
               const void *option_value, socklen_t option_length);
int getsockopt(int socket_fd, int level, int option_name,
               void *option_value, socklen_t *option_length);
ssize_t sendto(int socket_fd, const void *buffer, size_t length, int flags,
               const struct sockaddr *destination, socklen_t destination_length);
ssize_t recvfrom(int socket_fd, void *buffer, size_t length, int flags,
                 struct sockaddr *source, socklen_t *source_length);
ssize_t sendmsg(int socket_fd, const struct msghdr *message, int flags);
ssize_t recvmsg(int socket_fd, struct msghdr *message, int flags);

int socketpair(int domain, int socket_type, int protocol, int sv[2]);

#endif

#ifndef OCTAGRAM_NETDB_H
#define OCTAGRAM_NETDB_H

#include <stddef.h>
#include <netinet/in.h>
#include <sys/socket.h>

struct hostent {
    char *h_name;
    char **h_aliases;
    int h_addrtype;
    int h_length;
    char **h_addr_list;
};

struct addrinfo {
    int ai_flags;
    int ai_family;
    int ai_socktype;
    int ai_protocol;
    socklen_t ai_addrlen;
    struct sockaddr *ai_addr;
    char *ai_canonname;
    struct addrinfo *ai_next;
};

#define AI_PASSIVE 1
#define AI_CANONNAME 2
#define AI_NUMERICHOST 4
#define AI_NUMERICSERV 8

#define NI_NUMERICHOST 1
#define NI_NUMERICSERV 2
#define EAI_FAMILY -1
#define EAI_MEMORY -2
#define EAI_NONAME -3
#define EAI_BADFLAGS -4
#define EAI_SERVICE -5
#define EAI_SOCKTYPE -6
#define EAI_OVERFLOW -7
#define EAI_FAIL -8
#define EAI_SYSTEM -11

#define HOST_NOT_FOUND 1
#define TRY_AGAIN 2
#define NO_RECOVERY 3
#define NO_DATA 4
#define NO_ADDRESS NO_DATA

#ifdef OCTAGRAM_TLS_ERRNO
extern __thread int h_errno;
#elif defined(OCTAGRAM_RUNTIME_ERRNO_ACCESSOR)
int *__h_errno_location(void);
#define h_errno (*__h_errno_location())
#else
extern int h_errno;
#endif

int getaddrinfo(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **result);
void freeaddrinfo(struct addrinfo *result);
const char *gai_strerror(int error);
int getnameinfo(const struct sockaddr *address, socklen_t address_length,
                char *host, socklen_t host_length,
                char *service, socklen_t service_length, int flags);
struct hostent *gethostbyname(const char *name);
struct hostent *gethostbyaddr(const void *address, socklen_t length, int family);

#endif

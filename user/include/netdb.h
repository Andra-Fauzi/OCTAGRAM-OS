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

int getaddrinfo(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **result);
void freeaddrinfo(struct addrinfo *result);
const char *gai_strerror(int error);
struct hostent *gethostbyname(const char *name);

#endif

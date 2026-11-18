#ifndef OCTAGRAM_NETINET_IN_H
#define OCTAGRAM_NETINET_IN_H

#include <stdint.h>
#include <sys/socket.h>

typedef uint16_t in_port_t;
typedef uint32_t in_addr_t;

struct in_addr {
    in_addr_t s_addr;
};

struct sockaddr_in {
    sa_family_t sin_family;
    in_port_t sin_port;
    struct in_addr sin_addr;
    unsigned char sin_zero[8];
};

#define INADDR_ANY ((in_addr_t)0)
#define INADDR_LOOPBACK ((in_addr_t)0x0100007f)
#define IPPROTO_IP 0
#define IPPROTO_TCP 6
#define IPPROTO_UDP 17
typedef uint32_t in_addr_t;

static inline uint16_t htons(uint16_t value)
{
#if __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
    return __builtin_bswap16(value);
#else
    return value;
#endif
}

static inline uint16_t ntohs(uint16_t value)
{
    return htons(value);
}

static inline uint32_t htonl(uint32_t value)
{
#if __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
    return __builtin_bswap32(value);
#else
    return value;
#endif
}

static inline uint32_t ntohl(uint32_t value)
{
    return htonl(value);
}

in_addr_t inet_addr(const char *text);
char *inet_ntoa(struct in_addr address);
int inet_pton(int family, const char *text, void *address);
const char *inet_ntop(int family, const void *address,
                      char *buffer, unsigned int length);

#endif

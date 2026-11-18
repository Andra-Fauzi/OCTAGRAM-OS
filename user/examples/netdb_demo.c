#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>

static int address_matches(const struct addrinfo *info, uint32_t host_address,
                           unsigned int port)
{
    if (info == NULL || info->ai_family != AF_INET || info->ai_addr == NULL ||
        info->ai_addrlen != sizeof(struct sockaddr_in)) {
        return 0;
    }
    const struct sockaddr_in *ipv4 = (const struct sockaddr_in *)info->ai_addr;
    return ipv4->sin_addr.s_addr == htonl(host_address)
        && ntohs(ipv4->sin_port) == port;
}

int main(void)
{
    struct addrinfo *result = NULL;

    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    if (getaddrinfo("127.0.0.1", "80", &hints, &result) != 0) {
        return 1;
    }
    if (!address_matches(result, 0x7f000001u, 80)) {
        return 2;
    }
    if (result->ai_socktype != SOCK_STREAM ||
        result->ai_protocol != IPPROTO_TCP || result->ai_canonname != NULL) {
        return 3;
    }
    freeaddrinfo(result);
    result = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_flags = AI_PASSIVE | AI_CANONNAME;
    hints.ai_socktype = SOCK_DGRAM;
    if (getaddrinfo(NULL, "8080", &hints, &result) != 0) {
        return 4;
    }
    if (!address_matches(result, 0, 8080) ||
        result->ai_protocol != IPPROTO_UDP) {
        return 5;
    }
    if (result->ai_canonname == NULL ||
        strcmp(result->ai_canonname, "0.0.0.0") != 0) {
        return 6;
    }
    freeaddrinfo(result);
    result = NULL;

    memset(&hints, 0, sizeof(hints));
    if (getaddrinfo(NULL, "9", &hints, &result) != 0 ||
        !address_matches(result, 0x7f000001u, 9)) {
        return 7;
    }
    freeaddrinfo(result);
    result = NULL;

    memset(&hints, 0, sizeof(hints));
    hints.ai_flags = AI_NUMERICHOST;
    if (getaddrinfo("example.com", "80", &hints, &result) != EAI_NONAME ||
        result != NULL) {
        return 8;
    }

    memset(&hints, 0, sizeof(hints));
    if (getaddrinfo("127.0.0.1", "http", &hints, &result) != EAI_SERVICE) {
        return 9;
    }
    hints.ai_flags = AI_NUMERICSERV;
    if (getaddrinfo("127.0.0.1", "http", &hints, &result) != EAI_NONAME) {
        return 10;
    }

    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNIX;
    if (getaddrinfo("127.0.0.1", "80", &hints, &result) != EAI_FAMILY) {
        return 11;
    }
    memset(&hints, 0, sizeof(hints));
    hints.ai_flags = 0x4000;
    if (getaddrinfo("127.0.0.1", "80", &hints, &result) != EAI_BADFLAGS) {
        return 12;
    }
    memset(&hints, 0, sizeof(hints));
    hints.ai_socktype = 99;
    if (getaddrinfo("127.0.0.1", "80", &hints, &result) != EAI_SOCKTYPE) {
        return 13;
    }

    struct sockaddr_in address;
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons(443);
    address.sin_addr.s_addr = htonl(0x0a010203u); /* 10.1.2.3 */

    char host[32];
    char service[16];
    if (getnameinfo((const struct sockaddr *)&address, sizeof(address),
                    host, sizeof(host), service, sizeof(service), 0) != 0) {
        return 14;
    }
    if (strcmp(host, "10.1.2.3") != 0 || strcmp(service, "443") != 0) {
        return 15;
    }
    char small[8];
    if (getnameinfo((const struct sockaddr *)&address, sizeof(address),
                    small, sizeof(small), NULL, 0, 0) != EAI_OVERFLOW) {
        return 16;
    }
    address.sin_family = AF_UNIX;
    if (getnameinfo((const struct sockaddr *)&address, sizeof(address),
                    host, sizeof(host), NULL, 0, 0) != EAI_FAMILY) {
        return 17;
    }
    address.sin_family = AF_INET;
    if (getnameinfo((const struct sockaddr *)&address, sizeof(address),
                    host, sizeof(host), NULL, 0, 0x8) != EAI_BADFLAGS) {
        return 18;
    }

    struct hostent *entry = gethostbyname("127.0.0.1");
    if (entry == NULL || entry->h_addrtype != AF_INET ||
        entry->h_length != 4 || strcmp(entry->h_name, "127.0.0.1") != 0) {
        return 19;
    }
    if (entry->h_addr_list == NULL || entry->h_addr_list[0] == NULL ||
        *(uint32_t *)entry->h_addr_list[0] != htonl(0x7f000001u)) {
        return 20;
    }
    if (gethostbyname("example.com") != NULL || h_errno != HOST_NOT_FOUND) {
        return 21;
    }
    uint32_t numeric = htonl(0x08080404u); /* 8.8.4.4 */
    entry = gethostbyaddr(&numeric, sizeof(numeric), AF_INET);
    if (entry == NULL || strcmp(entry->h_name, "8.8.4.4") != 0) {
        return 22;
    }
    if (gethostbyaddr(&numeric, sizeof(numeric), AF_UNIX) != NULL ||
        h_errno != HOST_NOT_FOUND) {
        return 23;
    }

    /* AF_UNSPEC: alamat numerik hanya menghasilkan keluarga yang cocok. */
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    if (getaddrinfo("127.0.0.1", "80", &hints, &result) != 0 ||
        result == NULL || result->ai_next != NULL ||
        !address_matches(result, 0x7f000001u, 80)) {
        return 24;
    }
    freeaddrinfo(result);
    result = NULL;
    if (getaddrinfo("::1", "80", &hints, &result) != 0 ||
        result == NULL || result->ai_family != AF_INET6 ||
        result->ai_addr == NULL ||
        result->ai_addrlen != sizeof(struct sockaddr_in6) ||
        result->ai_next != NULL) {
        return 25;
    }
    freeaddrinfo(result);
    result = NULL;

    printf("netdb numeric API passed\n");
    return 0;
}

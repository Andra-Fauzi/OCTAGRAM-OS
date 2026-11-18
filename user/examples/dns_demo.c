#include <netdb.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>

int main(void)
{
    static const char hostname[] = "example.com";

    struct hostent *entry = gethostbyname(hostname);
    if (entry == NULL || entry->h_addr_list == NULL
        || entry->h_addr_list[0] == NULL || entry->h_length != 4) {
        printf("dns: cannot resolve %s (h_errno=%d); pastikan DHCP/DNS aktif\n",
               hostname, h_errno);
        return 1;
    }
    uint32_t resolved;
    memcpy(&resolved, entry->h_addr_list[0], sizeof(resolved));
    if (resolved == 0 || resolved == htonl(0xffffffffu)) {
        return 2;
    }

    /* Panggilan kedua harus dilayani cache dan memberi alamat yang sama. */
    struct hostent *cached = gethostbyname(hostname);
    if (cached == NULL || cached->h_addr_list == NULL
        || cached->h_addr_list[0] == NULL
        || memcmp(cached->h_addr_list[0], &resolved, sizeof(resolved)) != 0) {
        return 3;
    }

    struct addrinfo hints;
    memset(&hints, 0, sizeof(hints));
    hints.ai_flags = AI_NUMERICHOST;
    struct addrinfo *result = NULL;
    if (getaddrinfo(hostname, "80", &hints, &result) != EAI_NONAME
        || result != NULL) {
        return 4;
    }

    memset(&hints, 0, sizeof(hints));
    hints.ai_flags = AI_CANONNAME;
    if (getaddrinfo(hostname, "80", &hints, &result) != 0 || result == NULL) {
        return 5;
    }
    const struct sockaddr_in *ipv4 =
        (const struct sockaddr_in *)result->ai_addr;
    if (ipv4->sin_addr.s_addr != resolved || ntohs(ipv4->sin_port) != 80) {
        return 6;
    }
    if (result->ai_canonname == NULL
        || strcmp(result->ai_canonname, hostname) != 0) {
        return 7;
    }
    freeaddrinfo(result);

    const unsigned char *bytes = (const unsigned char *)&resolved;
    printf("dns: %s -> %u.%u.%u.%u (cached)\n", hostname,
           (unsigned)bytes[0], (unsigned)bytes[1],
           (unsigned)bytes[2], (unsigned)bytes[3]);
    return 0;
}

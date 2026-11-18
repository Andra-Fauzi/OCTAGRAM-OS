/* Uji host parser /etc/resolv.conf di atas runtime asli. */
#include <stdint.h>

int oct_resolv_parse_nameserver(const char *text, uint8_t server[4]);

static int same(const uint8_t server[4], uint8_t a, uint8_t b, uint8_t c, uint8_t d)
{
    return server[0] == a && server[1] == b && server[2] == c && server[3] == d;
}

int main(void)
{
    uint8_t server[4] = {0, 0, 0, 0};

    if (!oct_resolv_parse_nameserver("nameserver 192.168.1.1\n", server)
        || !same(server, 192, 168, 1, 1)) {
        return 1;
    }

    /* Spasi/tab di awal, komentar, dan CRLF. */
    if (!oct_resolv_parse_nameserver(
            "; generated\r\n  nameserver\t10.0.0.53  # lan\r\n", server)
        || !same(server, 10, 0, 0, 53)) {
        return 2;
    }

    /* Baris pertama yang valid menang. */
    if (!oct_resolv_parse_nameserver(
            "nameserver 1.1.1.1\nnameserver 8.8.8.8\n", server)
        || !same(server, 1, 1, 1, 1)) {
        return 3;
    }

    /* Oktet tidak valid dilewati, bukan menghentikan pencarian. */
    if (!oct_resolv_parse_nameserver(
            "nameserver 300.1.1.1\nnameserver 9.9.9.9\n", server)
        || !same(server, 9, 9, 9, 9)) {
        return 4;
    }

    if (oct_resolv_parse_nameserver("search example.com\n", server)) {
        return 5;
    }
    if (oct_resolv_parse_nameserver("nameserver 1.2.3\n", server)) {
        return 6;
    }
    if (oct_resolv_parse_nameserver("", server)) {
        return 7;
    }
    if (oct_resolv_parse_nameserver("nameserver\n", server)) {
        return 8;
    }
    return 0;
}

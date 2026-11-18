/* Uji host IPv6: primitif alamat, getaddrinfo AF_INET6, dan parser DNS AAAA. */
#include <netinet/in.h>
#include <stdint.h>
#include <string.h>
#include <sys/socket.h>

int oct_dns_parse_address(const uint8_t *packet, size_t length,
                          uint16_t identifier, uint16_t qtype,
                          uint8_t *address, size_t address_capacity,
                          uint32_t *ttl);

static size_t build_response(uint8_t *packet, uint16_t identifier,
                             uint16_t qtype, const uint8_t *data,
                             size_t data_length, uint32_t ttl)
{
    size_t offset = 0;
    packet[offset++] = (uint8_t)(identifier >> 8);
    packet[offset++] = (uint8_t)(identifier & 0xff);
    packet[offset++] = 0x81;
    packet[offset++] = 0x80;
    packet[offset++] = 0;
    packet[offset++] = 1;
    packet[offset++] = 0;
    packet[offset++] = 1;
    for (int index = 0; index < 4; ++index) {
        packet[offset++] = 0;
    }
    packet[offset++] = 1;
    packet[offset++] = 'a';
    packet[offset++] = 0;
    packet[offset++] = (uint8_t)(qtype >> 8);
    packet[offset++] = (uint8_t)(qtype & 0xff);
    packet[offset++] = 0;
    packet[offset++] = 1;
    packet[offset++] = 0xc0;
    packet[offset++] = 0x0c;
    packet[offset++] = (uint8_t)(qtype >> 8);
    packet[offset++] = (uint8_t)(qtype & 0xff);
    packet[offset++] = 0;
    packet[offset++] = 1;
    packet[offset++] = (uint8_t)(ttl >> 24);
    packet[offset++] = (uint8_t)(ttl >> 16);
    packet[offset++] = (uint8_t)(ttl >> 8);
    packet[offset++] = (uint8_t)ttl;
    packet[offset++] = 0;
    packet[offset++] = (uint8_t)data_length;
    memcpy(packet + offset, data, data_length);
    return offset + data_length;
}

int main(void)
{
    static const struct {
        const char *text;
        const char *formatted;
    } addresses[] = {
        {"2001:db8::1", "2001:db8::1"},
        {"::1", "::1"},
        {"::", "::"},
        {"1:2:3:4:5:6:7:8", "1:2:3:4:5:6:7:8"},
        {"fe80::1:2", "fe80::1:2"},
        {"2001:0db8:0000:0000:0000:ff00:0042:8329", "2001:db8::ff00:42:8329"},
    };
    uint8_t binary[16];
    for (size_t index = 0; index < sizeof(addresses) / sizeof(addresses[0]); ++index) {
        if (inet_pton(AF_INET6, addresses[index].text, binary) != 1) {
            return (int)(1 + index);
        }
        char text[46];
        if (inet_ntop(AF_INET6, binary, text, sizeof(text)) == NULL
            || strcmp(text, addresses[index].formatted) != 0) {
            return (int)(10 + index);
        }
    }

    static const char *invalid[] = {"1::2::3", "12345::", "1:2:3:4:5:6:7:8:9", ":::", ""};
    for (size_t index = 0; index < sizeof(invalid) / sizeof(invalid[0]); ++index) {
        if (inet_pton(AF_INET6, invalid[index], binary) == 1) {
            return (int)(20 + index);
        }
    }

    uint8_t packet[96];
    const uint8_t address_bytes[16] = {
        0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
    };
    size_t length = build_response(packet, 0x1234, 28, address_bytes, 16, 300);
    uint8_t parsed[16];
    uint32_t ttl = 0;
    if (oct_dns_parse_address(packet, length, 0x1234, 28, parsed, 16, &ttl) != 0
        || ttl != 300 || memcmp(parsed, address_bytes, 16) != 0) {
        return 40;
    }
    /* Identifier atau tipe yang salah tidak boleh cocok. */
    if (oct_dns_parse_address(packet, length, 0x4321, 28, parsed, 16, &ttl) == 0
        || oct_dns_parse_address(packet, length, 0x1234, 1, parsed, 16, &ttl) == 0) {
        return 41;
    }

    const uint8_t ipv4_bytes[4] = {192, 168, 1, 1};
    length = build_response(packet, 0x5678, 1, ipv4_bytes, 4, 60);
    uint32_t ipv4 = 0;
    if (oct_dns_parse_address(packet, length, 0x5678, 1, (uint8_t *)&ipv4, 4,
                              &ttl) != 0
        || ttl != 60 || memcmp(&ipv4, ipv4_bytes, 4) != 0) {
        return 42;
    }
    return 0;
}

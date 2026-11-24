#include "octagram.h"

#include <ctype.h>
#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <limits.h>
#include <pwd.h>
#include <sched.h>
#include <stddef.h>
#include <stdlib.h>
#include <spawn.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/ioctl.h>
#include <stdarg.h>
#include <time.h>
#include <unistd.h>
#include <netinet/in.h>
#include <netdb.h>
#include <poll.h>
#include <signal.h>
#include <sys/select.h>
#include <sys/file.h>
#include <sys/time.h>
#include <sys/utsname.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <sys/random.h>
#include <sys/resource.h>
#include <sys/capability.h>
#include <sys/semaphore.h>
#include <locale.h>
#include <wchar.h>
#include <uchar.h>
#include <termios.h>
#include <sys/socket.h>
#include <sys/timerfd.h>
#include <sys/uio.h>
#include <sys/times.h>

struct octagram_FILE {
    int fd;
    unsigned int flags;
    int pushed_character;
};

struct octagram_DIR {
    int fd;
    struct dirent entry;
};

_Static_assert(offsetof(struct dirent, d_name) == 19,
               "OCTAGRAM dirent name offset must match the syscall ABI");
_Static_assert(sizeof(struct dirent) == 280,
               "OCTAGRAM dirent size must match the syscall ABI");
_Static_assert(sizeof(struct sockaddr_in) == 16,
               "OCTAGRAM sockaddr_in must match the syscall ABI");

enum {
    STREAM_READABLE = 1u << 0,
    STREAM_WRITABLE = 1u << 1,
    STREAM_EOF = 1u << 2,
    STREAM_ERROR = 1u << 3,
    STREAM_PUSHBACK = 1u << 4,
    STREAM_CLOSED = 1u << 5
};

static FILE standard_input = {0, STREAM_READABLE, 0};
static FILE standard_output = {1, STREAM_WRITABLE, 0};
static FILE standard_error = {2, STREAM_WRITABLE, 0};

FILE *stdin = &standard_input;
FILE *stdout = &standard_output;
FILE *stderr = &standard_error;

char **environ;
#if defined(OCTAGRAM_SHARED)
typedef struct OctRuntimeThreadState {
    volatile uint64_t pid;
    volatile uint64_t tid;
    int errno_value;
    int h_errno_value;
    void *key_values[64];
    uint64_t key_generations[64];
} OctRuntimeThreadState;

static OctRuntimeThreadState *oct_runtime_state_for_thread(void);
#endif

int *__errno_location(void);
int *__h_errno_location(void);
#if defined(OCTAGRAM_TLS_ERRNO) && !defined(OCTAGRAM_SHARED)
__thread int errno;
__thread int h_errno;
#elif !defined(OCTAGRAM_SHARED)
int errno;
int h_errno;
#endif

extern unsigned char __oct_tls_start[] __attribute__((weak));
extern const uintptr_t __oct_tls_filesz[] __attribute__((weak));
extern const uintptr_t __oct_tls_size[] __attribute__((weak));
extern const uintptr_t __oct_tls_align[] __attribute__((weak));

typedef struct __attribute__((aligned(16))) AllocationHeader {
    size_t size;
    struct AllocationHeader *next_free;
    /* For aligned allocations, points to the original malloc block header. */
    struct AllocationHeader *allocation_base;
} AllocationHeader;

#if defined(OCTAGRAM_TLS_ERRNO) && !defined(OCTAGRAM_SHARED)
static __thread AllocationHeader *free_list;
#else
static AllocationHeader *free_list;
#endif

static int64_t posix_result(int64_t result)
{
    if (result < 0) {
        errno = (int)-result;
        return -1;
    }
    return result;
}

/*
 * ELF64 program headers are also present in the initial auxiliary vector.
 * GNU ld builds use the __oct_tls_* symbols emitted by user/linker.ld, while
 * TinyCC's built-in linker emits the standard PT_TLS program header instead.
 */
typedef struct OctElf64ProgramHeader {
    uint32_t type;
    uint32_t flags;
    uint64_t offset;
    uint64_t virtual_address;
    uint64_t physical_address;
    uint64_t file_size;
    uint64_t memory_size;
    uint64_t alignment;
} OctElf64ProgramHeader;

enum {
    OCT_AT_NULL = 0,
    OCT_AT_PHDR = 3,
    OCT_AT_PHENT = 4,
    OCT_AT_PHNUM = 5,
    OCT_PT_TLS = 7
};

static int oct_tls_from_auxiliary_vector(uintptr_t *template_start,
                                         uintptr_t *filesz,
                                         uintptr_t *size,
                                         uintptr_t *alignment,
                                         uintptr_t *aligned_size)
{
    if (environ == NULL) {
        return 0;
    }

    /* The kernel lays out envp, its NULL terminator, then the auxiliary vector. */
    char **environment_end = environ;
    unsigned int environment_count = 0;
    while (environment_count < 65 && environment_end[environment_count] != NULL) {
        ++environment_count;
    }
    if (environment_count == 65) {
        return 0;
    }

    const uintptr_t *auxiliary = (const uintptr_t *)(environment_end
                                      + environment_count + 1);
    uintptr_t program_headers = 0;
    uintptr_t program_header_size = 0;
    uintptr_t program_header_count = 0;
    for (unsigned int index = 0; index < 128; ++index) {
        uintptr_t type = auxiliary[index * 2];
        uintptr_t value = auxiliary[index * 2 + 1];
        if (type == OCT_AT_NULL) {
            break;
        }
        switch (type) {
        case OCT_AT_PHDR:
            program_headers = value;
            break;
        case OCT_AT_PHENT:
            program_header_size = value;
            break;
        case OCT_AT_PHNUM:
            program_header_count = value;
            break;
        default:
            break;
        }
    }

    if (program_headers == 0
        || program_header_size != sizeof(OctElf64ProgramHeader)
        || program_header_count == 0 || program_header_count > 128
        || program_headers > UINTPTR_MAX
             - program_header_count * sizeof(OctElf64ProgramHeader)) {
        return 0;
    }

    for (uintptr_t index = 0; index < program_header_count; ++index) {
        const OctElf64ProgramHeader *header =
            (const OctElf64ProgramHeader *)(program_headers
                + index * sizeof(OctElf64ProgramHeader));
        if (header->type != OCT_PT_TLS) {
            continue;
        }

        uintptr_t tls_filesz = (uintptr_t)header->file_size;
        uintptr_t tls_size = (uintptr_t)header->memory_size;
        uintptr_t tls_alignment = (uintptr_t)header->alignment;
        uintptr_t tls_start = (uintptr_t)header->virtual_address;

        if (header->alignment > 4096
            || tls_filesz > tls_size
            || (tls_filesz != 0 && tls_start == 0)
            || tls_start > UINTPTR_MAX - tls_filesz) {
            return -1;
        }

        /* The TCB is pointer-aligned even when PT_TLS requests weaker alignment. */
        if (tls_alignment < sizeof(uintptr_t)) {
            tls_alignment = sizeof(uintptr_t);
        }
        if (tls_alignment == 0 || (tls_alignment & (tls_alignment - 1)) != 0
            || tls_size > SIZE_MAX - (tls_alignment - 1)) {
            return -1;
        }

        *template_start = tls_start;
        *filesz = tls_filesz;
        *size = tls_size;
        *alignment = tls_alignment;
        *aligned_size = (tls_size + tls_alignment - 1) & ~(tls_alignment - 1);
        return 1;
    }

    return 0;
}

static int oct_tls_layout(uintptr_t *template_start, uintptr_t *filesz,
                          uintptr_t *size, uintptr_t *alignment,
                          uintptr_t *aligned_size)
{
    *template_start = 0;
    *size = __oct_tls_size == NULL ? 0 : *__oct_tls_size;
    *filesz = __oct_tls_filesz == NULL ? 0 : *__oct_tls_filesz;
    *alignment = __oct_tls_align == NULL ? 0 : *__oct_tls_align;

    if (*size != 0) {
        if (__oct_tls_start == NULL || *filesz > *size
            || *alignment == 0 || *alignment > 4096
            || (*alignment & (*alignment - 1)) != 0
            || *size > SIZE_MAX - (*alignment - 1)) {
            return -1;
        }
        *template_start = (uintptr_t)__oct_tls_start;
        *aligned_size = (*size + *alignment - 1) & ~(*alignment - 1);
        return 0;
    }

    int result = oct_tls_from_auxiliary_vector(
        template_start, filesz, size, alignment, aligned_size);
    if (result <= 0) {
        *template_start = 0;
        *filesz = 0;
        *size = 0;
        *alignment = 0;
        *aligned_size = 0;
    }
    return result < 0 ? -1 : 0;
}

static void oct_tls_copy(void *destination, uintptr_t template_start,
                         uintptr_t filesz, uintptr_t size)
{
    if (filesz != 0) {
        memcpy(destination, (const void *)template_start, filesz);
    }
    if (size > filesz) {
        memset((unsigned char *)destination + filesz, 0, size - filesz);
    }
}

in_addr_t inet_addr(const char *text)
{
    uint32_t result = 0;
    if (text == NULL) {
        return (in_addr_t)-1;
    }
    for (int component = 0; component < 4; ++component) {
        unsigned int value = 0;
        int digits = 0;
        while (*text >= '0' && *text <= '9') {
            value = value * 10u + (unsigned int)(*text - '0');
            ++text;
            ++digits;
            if (value > 255u) {
                return (in_addr_t)-1;
            }
        }
        if (digits == 0 || (component < 3 && *text != '.') ||
            (component == 3 && *text != '\0')) {
            return (in_addr_t)-1;
        }
        result = (result << 8) | value;
        if (component < 3) {
            ++text;
        }
    }
    return htonl(result);
}

/* Parse alamat IPv6 (grup heksadesimal dengan satu "::"). */
static int oct_parse_ipv6(const char *text, uint8_t address[16])
{
    if (text == NULL || address == NULL) {
        return -1;
    }
    uint16_t groups[8] = {0, 0, 0, 0, 0, 0, 0, 0};
    int count = 0;
    int compressed = -1;
    const char *cursor = text;
    if (*cursor == ':') {
        if (cursor[1] != ':') {
            return -1;
        }
        compressed = 0;
        cursor += 2;
        if (*cursor == '\0') {
            memset(address, 0, 16);
            return 0;
        }
    }
    while (*cursor != '\0') {
        if (count >= 8) {
            return -1;
        }
        unsigned value = 0;
        int digits = 0;
        for (;;) {
            char character = *cursor;
            unsigned digit;
            if (character >= '0' && character <= '9') {
                digit = (unsigned)(character - '0');
            } else if (character >= 'a' && character <= 'f') {
                digit = (unsigned)(character - 'a' + 10);
            } else if (character >= 'A' && character <= 'F') {
                digit = (unsigned)(character - 'A' + 10);
            } else {
                break;
            }
            value = (value << 4) | digit;
            if (value > 0xffffu) {
                return -1;
            }
            ++digits;
            ++cursor;
            if (digits > 4) {
                return -1;
            }
        }
        if (digits == 0) {
            return -1;
        }
        groups[count++] = (uint16_t)value;
        if (*cursor == '\0') {
            break;
        }
        if (*cursor != ':') {
            return -1;
        }
        ++cursor;
        if (*cursor == ':') {
            if (compressed >= 0) {
                return -1;
            }
            compressed = count;
            ++cursor;
            if (*cursor == '\0') {
                break;
            }
        } else if (*cursor == '\0') {
            return -1;
        }
    }
    if (compressed >= 0) {
        int missing = 8 - count;
        if (missing <= 0) {
            return -1;
        }
        for (int index = count - 1; index >= compressed; --index) {
            groups[index + missing] = groups[index];
        }
        for (int index = 0; index < missing; ++index) {
            groups[compressed + index] = 0;
        }
        count = 8;
    }
    if (count != 8) {
        return -1;
    }
    for (int index = 0; index < 8; ++index) {
        address[index * 2] = (uint8_t)(groups[index] >> 8);
        address[index * 2 + 1] = (uint8_t)(groups[index] & 0xff);
    }
    return 0;
}

/* Format alamat IPv6 dengan kompresi nol terpanjang (RFC 5952). */
static int oct_format_ipv6(const uint8_t address[16], char *buffer, size_t size)
{
    uint16_t groups[8];
    for (int index = 0; index < 8; ++index) {
        groups[index] = (uint16_t)(((uint16_t)address[index * 2] << 8)
                                   | address[index * 2 + 1]);
    }
    int best_start = -1;
    int best_length = 0;
    for (int index = 0; index < 8;) {
        if (groups[index] != 0) {
            ++index;
            continue;
        }
        int start = index;
        while (index < 8 && groups[index] == 0) {
            ++index;
        }
        if (index - start > best_length) {
            best_length = index - start;
            best_start = start;
        }
    }
    if (best_length < 2) {
        best_start = -1;
    }
    size_t written = 0;
    for (int index = 0; index < 8; ++index) {
        if (best_start == index) {
            if (written + 2 >= size) {
                return -1;
            }
            buffer[written++] = ':';
            buffer[written++] = ':';
            index += best_length - 1;
            continue;
        }
        if (written != 0 && buffer[written - 1] != ':') {
            if (written + 1 >= size) {
                return -1;
            }
            buffer[written++] = ':';
        }
        int length = snprintf(buffer + written, size - written, "%x",
                              (unsigned)groups[index]);
        if (length < 0 || (size_t)length >= size - written) {
            return -1;
        }
        written += (size_t)length;
    }
    if (written >= size) {
        return -1;
    }
    buffer[written] = '\0';
    return 0;
}

int inet_pton(int family, const char *text, void *address)
{
    if (text == NULL || address == NULL) {
        return -1;
    }
    if (family == AF_INET6) {
        return oct_parse_ipv6(text, (uint8_t *)address) == 0 ? 1 : 0;
    }
    if (family != AF_INET) {
        return -1;
    }
    in_addr_t value = inet_addr(text);
    if (value == (in_addr_t)-1) {
        return 0;
    }
    memcpy(address, &value, sizeof(value));
    return 1;
}

static char *inet_format_address(in_addr_t address)
{
    static char text[16];
    uint32_t host = ntohl(address);
    snprintf(text, sizeof(text), "%u.%u.%u.%u",
             (unsigned)((host >> 24) & 0xff),
             (unsigned)((host >> 16) & 0xff),
             (unsigned)((host >> 8) & 0xff),
             (unsigned)(host & 0xff));
    return text;
}

char *inet_ntoa(struct in_addr address)
{
    return inet_format_address(address.s_addr);
}

const char *inet_ntop(int family, const void *address,
                      char *buffer, unsigned int length)
{
    if (address == NULL || buffer == NULL) {
        errno = EINVAL;
        return NULL;
    }
    if (family == AF_INET6) {
        if (length < 46
            || oct_format_ipv6((const uint8_t *)address, buffer, length) != 0) {
            errno = EINVAL;
            return NULL;
        }
        return buffer;
    }
    if (family != AF_INET || length < 16) {
        errno = EINVAL;
        return NULL;
    }
    in_addr_t value;
    memcpy(&value, address, sizeof(value));
    char *text = inet_format_address(value);
    memcpy(buffer, text, strlen(text) + 1);
    return buffer;
}

static int oct_format_ipv4(in_addr_t address, char *buffer, size_t size)
{
    uint32_t host = ntohl(address);
    int length = snprintf(buffer, size, "%u.%u.%u.%u",
                          (unsigned)((host >> 24) & 0xff),
                          (unsigned)((host >> 16) & 0xff),
                          (unsigned)((host >> 8) & 0xff),
                          (unsigned)(host & 0xff));
    if (length < 0 || (size_t)length >= size) {
        return -1;
    }
    return 0;
}

static int oct_parse_numeric_service(const char *service, unsigned short *port)
{
    if (service == NULL || service[0] == '\0') {
        *port = 0;
        return 0;
    }
    unsigned int parsed = 0;
    for (const char *cursor = service; *cursor != '\0'; ++cursor) {
        if (*cursor < '0' || *cursor > '9') {
            return -1;
        }
        parsed = parsed * 10u + (unsigned int)(*cursor - '0');
        if (parsed > 65535u) {
            return -1;
        }
    }
    *port = (unsigned short)parsed;
    return 0;
}

#define OCT_DNS_CACHE_ENTRIES 8
#define OCT_DNS_MAX_PACKET 512
#define OCT_DNS_TIMEOUT_MILLISECONDS 2000
#define OCT_DNS_DEFAULT_TTL 60
#define OCT_DNS_MAX_TTL 86400
#define OCT_DNS_MAX_CANON_NAME 128

typedef struct {
    char name[128];
    in_addr_t address;
    uint64_t expires;
    int used;
} oct_dns_cache_entry;

static oct_dns_cache_entry oct_dns_cache[OCT_DNS_CACHE_ENTRIES];
static unsigned int oct_dns_query_identifier;
static unsigned int oct_dns_cache_cursor;

static int oct_dns_name_equal(const char *left, const char *right)
{
    for (;;) {
        unsigned char a = (unsigned char)*left++;
        unsigned char b = (unsigned char)*right++;
        if (tolower(a) != tolower(b)) {
            return 0;
        }
        if (a == 0) {
            return 1;
        }
    }
}

static int oct_dns_cache_lookup(const char *name, in_addr_t *address)
{
    uint64_t now = (uint64_t)time(NULL);
    for (int index = 0; index < OCT_DNS_CACHE_ENTRIES; ++index) {
        oct_dns_cache_entry *entry = &oct_dns_cache[index];
        if (!entry->used) {
            continue;
        }
        if (entry->expires <= now) {
            entry->used = 0;
            continue;
        }
        if (oct_dns_name_equal(entry->name, name)) {
            *address = entry->address;
            return 0;
        }
    }
    return -1;
}

static void oct_dns_cache_store(const char *name, in_addr_t address,
                                uint32_t ttl)
{
    if (ttl < OCT_DNS_DEFAULT_TTL) {
        ttl = OCT_DNS_DEFAULT_TTL;
    }
    if (ttl > OCT_DNS_MAX_TTL) {
        ttl = OCT_DNS_MAX_TTL;
    }
    uint64_t now = (uint64_t)time(NULL);
    int slot = -1;
    for (int index = 0; index < OCT_DNS_CACHE_ENTRIES; ++index) {
        oct_dns_cache_entry *entry = &oct_dns_cache[index];
        if (entry->used && entry->expires > now
            && oct_dns_name_equal(entry->name, name)) {
            slot = index;
            break;
        }
        if (slot < 0 && (!entry->used || entry->expires <= now)) {
            slot = index;
        }
    }
    if (slot < 0) {
        slot = (int)(oct_dns_cache_cursor++ % OCT_DNS_CACHE_ENTRIES);
    }
    oct_dns_cache_entry *entry = &oct_dns_cache[slot];
    size_t name_length = strlen(name);
    memcpy(entry->name, name, name_length + 1);
    entry->address = address;
    entry->expires = now + ttl;
    entry->used = 1;
}

static int oct_dns_encode_name(const char *name, uint8_t *packet,
                               size_t capacity, size_t *length)
{
    size_t position = 0;
    while (name[position] != '\0') {
        size_t label_start = position;
        while (name[position] != '\0' && name[position] != '.') {
            ++position;
        }
        size_t label_length = position - label_start;
        if (label_length == 0 || label_length > 63
            || *length + 1 + label_length >= capacity) {
            return -1;
        }
        packet[*length] = (uint8_t)label_length;
        ++*length;
        memcpy(packet + *length, name + label_start, label_length);
        *length += label_length;
        if (name[position] == '.') {
            ++position;
            if (name[position] == '\0') {
                return -1;
            }
        }
    }
    if (*length + 1 >= capacity) {
        return -1;
    }
    packet[*length] = 0;
    ++*length;
    return 0;
}

static int oct_dns_skip_name(const uint8_t *packet, size_t length,
                             size_t *offset)
{
    while (*offset < length) {
        uint8_t label = packet[*offset];
        if ((label & 0xc0) == 0xc0) {
            *offset += 2;
            return *offset <= length ? 0 : -1;
        }
        if ((label & 0xc0) != 0) {
            return -1;
        }
        ++*offset;
        if (label == 0) {
            return 0;
        }
        if (*offset + label > length) {
            return -1;
        }
        *offset += label;
    }
    return -1;
}

/// Parse jawaban DNS untuk record `qtype` (1 = IPv4, 28 = IPv6) dan salin
/// alamatnya. Dipakai juga oleh uji host.
int oct_dns_parse_address(const uint8_t *packet, size_t length,
                          uint16_t identifier, uint16_t qtype,
                          uint8_t *address, size_t address_capacity,
                          uint32_t *ttl)
{
    if (address == NULL || ttl == NULL) {
        return -1;
    }
    if (length < 12) {
        return -1;
    }
    if ((((uint16_t)packet[0] << 8) | packet[1]) != identifier) {
        return -1;
    }
    uint16_t flags = (uint16_t)(((uint16_t)packet[2] << 8) | packet[3]);
    if ((flags & 0x8000) == 0 || (flags & 0x000f) != 0) {
        return -1;
    }
    uint16_t questions = (uint16_t)(((uint16_t)packet[4] << 8) | packet[5]);
    uint16_t answers = (uint16_t)(((uint16_t)packet[6] << 8) | packet[7]);
    size_t offset = 12;
    for (uint16_t question = 0; question < questions; ++question) {
        if (oct_dns_skip_name(packet, length, &offset) != 0
            || offset + 4 > length) {
            return -1;
        }
        offset += 4;
    }
    for (uint16_t answer = 0; answer < answers; ++answer) {
        if (oct_dns_skip_name(packet, length, &offset) != 0
            || offset + 10 > length) {
            return -1;
        }
        uint16_t type = (uint16_t)(((uint16_t)packet[offset] << 8)
                                   | packet[offset + 1]);
        uint16_t class_value = (uint16_t)(((uint16_t)packet[offset + 2] << 8)
                                          | packet[offset + 3]);
        uint32_t record_ttl = ((uint32_t)packet[offset + 4] << 24)
            | ((uint32_t)packet[offset + 5] << 16)
            | ((uint32_t)packet[offset + 6] << 8)
            | (uint32_t)packet[offset + 7];
        uint16_t data_length = (uint16_t)(((uint16_t)packet[offset + 8] << 8)
                                          | packet[offset + 9]);
        offset += 10;
        if (offset + data_length > length) {
            return -1;
        }
        if (type == qtype && class_value == 1 && data_length == address_capacity) {
            memcpy(address, packet + offset, address_capacity);
            *ttl = record_ttl;
            return 0;
        }
        offset += data_length;
    }
    return -1;
}

#define OCT_RESOLV_CONF_MAX 1024

/// Parse baris `nameserver <IPv4>` pertama dari isi resolv.conf. Mengembalikan
/// 1 bila alamat ditemukan, 0 bila tidak. Dipakai juga oleh uji host.
int oct_resolv_parse_nameserver(const char *text, uint8_t server[4])
{
    if (text == NULL || server == NULL) {
        return 0;
    }
    const char *cursor = text;
    while (*cursor != '\0') {
        const char key[] = "nameserver";
        const size_t key_length = sizeof(key) - 1;
        while (*cursor == ' ' || *cursor == '\t') {
            ++cursor;
        }
        size_t matched = 0;
        while (matched < key_length && cursor[matched] == key[matched]) {
            ++matched;
        }
        if (matched == key_length
            && (cursor[key_length] == ' ' || cursor[key_length] == '\t')) {
            cursor += key_length;
            while (*cursor == ' ' || *cursor == '\t') {
                ++cursor;
            }
            unsigned values[4] = {0, 0, 0, 0};
            int count = 0;
            int valid = 1;
            while (count < 4) {
                unsigned value = 0;
                int digits = 0;
                while (*cursor >= '0' && *cursor <= '9') {
                    value = value * 10u + (unsigned)(*cursor - '0');
                    ++cursor;
                    ++digits;
                    if (value > 255u) {
                        valid = 0;
                        break;
                    }
                }
                if (!valid || digits == 0) {
                    valid = 0;
                    break;
                }
                values[count++] = value;
                if (count < 4) {
                    if (*cursor != '.') {
                        valid = 0;
                        break;
                    }
                    ++cursor;
                }
            }
            char terminator = *cursor;
            if (valid && count == 4
                && (terminator == '\0' || terminator == ' ' || terminator == '\t'
                    || terminator == '\r' || terminator == '\n'
                    || terminator == '#' || terminator == ';')) {
                for (int index = 0; index < 4; ++index) {
                    server[index] = (uint8_t)values[index];
                }
                return 1;
            }
        }
        while (*cursor != '\0' && *cursor != '\n') {
            ++cursor;
        }
        if (*cursor == '\n') {
            ++cursor;
        }
    }
    return 0;
}

static uint8_t oct_resolv_server[4];
static int oct_resolv_state; /* 0 belum dibaca, 1 file dipakai, -1 fallback kernel */

static int oct_resolv_read_server(uint8_t server[4])
{
    int descriptor = open("/etc/resolv.conf", O_RDONLY);
    if (descriptor < 0) {
        return 0;
    }
    char buffer[OCT_RESOLV_CONF_MAX + 1];
    size_t total = 0;
    while (total < OCT_RESOLV_CONF_MAX) {
        ssize_t amount = read(descriptor, buffer + total,
                              (size_t)(OCT_RESOLV_CONF_MAX - total));
        if (amount < 0) {
            close(descriptor);
            return 0;
        }
        if (amount == 0) {
            break;
        }
        total += (size_t)amount;
    }
    close(descriptor);
    buffer[total] = '\0';
    return oct_resolv_parse_nameserver(buffer, server);
}

/* Server DNS efektif: /etc/resolv.conf bila ada, lalu konfigurasi kernel. */
static int oct_dns_resolver_server(uint8_t server[4])
{
    if (oct_resolv_state == 0) {
        oct_resolv_state = oct_resolv_read_server(oct_resolv_server) ? 1 : -1;
    }
    if (oct_resolv_state == 1) {
        for (int index = 0; index < 4; ++index) {
            server[index] = oct_resolv_server[index];
        }
        return 0;
    }
    return oct_dns_server(server) < 0 ? -1 : 0;
}

/* Kirim satu query DNS (QTYPE bebas) dan ambil respons mentah. */
static int oct_dns_query(const char *qname, uint16_t qtype,
                         uint16_t *identifier_out,
                         uint8_t *response, size_t *response_length)
{
    uint8_t server[4];
    if (oct_dns_resolver_server(server) < 0) {
        return -1;
    }

    uint8_t query[OCT_DNS_MAX_PACKET];
    memset(query, 0, 12);
    uint16_t identifier = (uint16_t)(++oct_dns_query_identifier & 0xffffu);
    query[0] = (uint8_t)(identifier >> 8);
    query[1] = (uint8_t)(identifier & 0xff);
    query[2] = 0x01; /* recursion desired */
    query[5] = 0x01; /* satu pertanyaan */
    size_t length = 12;
    if (oct_dns_encode_name(qname, query, sizeof(query) - 4, &length) != 0) {
        return -1;
    }
    query[length++] = (uint8_t)(qtype >> 8);
    query[length++] = (uint8_t)(qtype & 0xff);
    query[length++] = 0; /* QCLASS IN */
    query[length++] = 1;

    int socket_fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (socket_fd < 0) {
        return -1;
    }
    struct sockaddr_in destination;
    memset(&destination, 0, sizeof(destination));
    destination.sin_family = AF_INET;
    destination.sin_port = htons(53);
    destination.sin_addr.s_addr =
        htonl(((uint32_t)server[0] << 24) | ((uint32_t)server[1] << 16)
              | ((uint32_t)server[2] << 8) | (uint32_t)server[3]);

    ssize_t sent = sendto(socket_fd, query, length, 0,
                          (const struct sockaddr *)&destination,
                          sizeof(destination));
    if (sent != (ssize_t)length) {
        close(socket_fd);
        return -1;
    }

    struct pollfd descriptor;
    descriptor.fd = socket_fd;
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    if (poll(&descriptor, 1, OCT_DNS_TIMEOUT_MILLISECONDS) <= 0) {
        close(socket_fd);
        return -1;
    }

    ssize_t received = recvfrom(socket_fd, response, OCT_DNS_MAX_PACKET, 0,
                                NULL, NULL);
    close(socket_fd);
    if (received < 12) {
        return -1;
    }
    *identifier_out = identifier;
    *response_length = (size_t)received;
    return 0;
}

/* Baca nama pada record jawaban, termasuk pointer kompresi. */
static int oct_dns_read_name(const uint8_t *packet, size_t length,
                             size_t *offset, char *output, size_t capacity)
{
    size_t position = *offset;
    size_t written = 0;
    int jumped = 0;
    int jumps = 0;
    for (;;) {
        if (position >= length) {
            return -1;
        }
        uint8_t label = packet[position];
        if ((label & 0xc0) == 0xc0) {
            if (position + 1 >= length) {
                return -1;
            }
            size_t target = (size_t)(((size_t)label & 0x3fu) << 8)
                | packet[position + 1];
            if (!jumped) {
                *offset = position + 2;
                jumped = 1;
            }
            if (target >= position || ++jumps > 8) {
                return -1;
            }
            position = target;
            continue;
        }
        if ((label & 0xc0) != 0) {
            return -1;
        }
        ++position;
        if (label == 0) {
            if (!jumped) {
                *offset = position;
            }
            if (written == 0) {
                if (capacity < 1) {
                    return -1;
                }
                output[0] = '\0';
            } else {
                output[written - 1] = '\0';
            }
            return 0;
        }
        if (position + label > length || written + label + 2 > capacity) {
            return -1;
        }
        if (written != 0) {
            output[written++] = '.';
        }
        memcpy(output + written, packet + position, label);
        written += label;
        output[written++] = '.';
        position += label;
    }
}

static int oct_dns_parse_ptr(const uint8_t *packet, size_t length,
                             uint16_t identifier, char *name,
                             size_t name_capacity, uint32_t *ttl)
{
    if (length < 12) {
        return -1;
    }
    if ((((uint16_t)packet[0] << 8) | packet[1]) != identifier) {
        return -1;
    }
    uint16_t flags = (uint16_t)(((uint16_t)packet[2] << 8) | packet[3]);
    if ((flags & 0x8000) == 0 || (flags & 0x000f) != 0) {
        return -1;
    }
    uint16_t questions = (uint16_t)(((uint16_t)packet[4] << 8) | packet[5]);
    uint16_t answers = (uint16_t)(((uint16_t)packet[6] << 8) | packet[7]);
    size_t offset = 12;
    for (uint16_t question = 0; question < questions; ++question) {
        if (oct_dns_skip_name(packet, length, &offset) != 0
            || offset + 4 > length) {
            return -1;
        }
        offset += 4;
    }
    for (uint16_t answer = 0; answer < answers; ++answer) {
        if (oct_dns_skip_name(packet, length, &offset) != 0
            || offset + 10 > length) {
            return -1;
        }
        uint16_t type = (uint16_t)(((uint16_t)packet[offset] << 8)
                                   | packet[offset + 1]);
        uint16_t class_value = (uint16_t)(((uint16_t)packet[offset + 2] << 8)
                                          | packet[offset + 3]);
        uint32_t record_ttl = ((uint32_t)packet[offset + 4] << 24)
            | ((uint32_t)packet[offset + 5] << 16)
            | ((uint32_t)packet[offset + 6] << 8)
            | (uint32_t)packet[offset + 7];
        uint16_t data_length = (uint16_t)(((uint16_t)packet[offset + 8] << 8)
                                          | packet[offset + 9]);
        offset += 10;
        if (offset + data_length > length) {
            return -1;
        }
        if (type == 12 && class_value == 1) {
            size_t name_offset = offset;
            if (oct_dns_read_name(packet, length, &name_offset, name,
                                  name_capacity) != 0) {
                return -1;
            }
            *ttl = record_ttl;
            return 0;
        }
        offset += data_length;
    }
    return -1;
}

typedef struct {
    in_addr_t address;
    char name[OCT_DNS_MAX_CANON_NAME];
    uint64_t expires;
    int used;
} oct_dns_reverse_entry;

static oct_dns_reverse_entry oct_dns_reverse_cache[OCT_DNS_CACHE_ENTRIES];
static unsigned int oct_dns_reverse_cursor;

static int oct_dns_reverse_cache_lookup(in_addr_t address, char *name,
                                        size_t capacity)
{
    uint64_t now = (uint64_t)time(NULL);
    for (int index = 0; index < OCT_DNS_CACHE_ENTRIES; ++index) {
        oct_dns_reverse_entry *entry = &oct_dns_reverse_cache[index];
        if (!entry->used) {
            continue;
        }
        if (entry->expires <= now) {
            entry->used = 0;
            continue;
        }
        if (entry->address == address) {
            size_t length = strlen(entry->name);
            if (length >= capacity) {
                return -1;
            }
            memcpy(name, entry->name, length + 1);
            return 0;
        }
    }
    return -1;
}

static void oct_dns_reverse_cache_store(in_addr_t address, const char *name,
                                        uint32_t ttl)
{
    if (ttl < OCT_DNS_DEFAULT_TTL) {
        ttl = OCT_DNS_DEFAULT_TTL;
    }
    if (ttl > OCT_DNS_MAX_TTL) {
        ttl = OCT_DNS_MAX_TTL;
    }
    size_t name_length = strlen(name);
    if (name_length >= sizeof(oct_dns_reverse_cache[0].name)) {
        return;
    }
    uint64_t now = (uint64_t)time(NULL);
    int slot = -1;
    for (int index = 0; index < OCT_DNS_CACHE_ENTRIES; ++index) {
        oct_dns_reverse_entry *entry = &oct_dns_reverse_cache[index];
        if (entry->used && entry->expires > now && entry->address == address) {
            slot = index;
            break;
        }
        if (slot < 0 && (!entry->used || entry->expires <= now)) {
            slot = index;
        }
    }
    if (slot < 0) {
        slot = (int)(oct_dns_reverse_cursor++ % OCT_DNS_CACHE_ENTRIES);
    }
    oct_dns_reverse_entry *entry = &oct_dns_reverse_cache[slot];
    memcpy(entry->name, name, name_length + 1);
    entry->address = address;
    entry->expires = now + ttl;
    entry->used = 1;
}

static int oct_dns_reverse_name(in_addr_t address, char *output,
                                size_t capacity)
{
    const unsigned char *bytes = (const unsigned char *)&address;
    int length = snprintf(output, capacity, "%u.%u.%u.%u.in-addr.arpa",
                          (unsigned)bytes[3], (unsigned)bytes[2],
                          (unsigned)bytes[1], (unsigned)bytes[0]);
    return length < 0 || (size_t)length >= capacity ? -1 : 0;
}

/* Resolver PTR (reverse DNS) dengan cache TTL. */
static int oct_dns_reverse(in_addr_t address, char *name, size_t capacity)
{
    if (capacity == 0) {
        return -1;
    }
    if (oct_dns_reverse_cache_lookup(address, name, capacity) == 0) {
        return 0;
    }
    char qname[64];
    if (oct_dns_reverse_name(address, qname, sizeof(qname)) != 0) {
        return -1;
    }
    uint8_t response[OCT_DNS_MAX_PACKET];
    size_t response_length = 0;
    uint16_t identifier = 0;
    if (oct_dns_query(qname, 12, &identifier, response,
                      &response_length) != 0) {
        return -1;
    }
    char resolved[OCT_DNS_MAX_CANON_NAME];
    uint32_t ttl = 0;
    if (oct_dns_parse_ptr(response, response_length, identifier, resolved,
                          sizeof(resolved), &ttl) != 0) {
        return -1;
    }
    size_t resolved_length = strlen(resolved);
    if (resolved_length == 0 || resolved_length >= capacity) {
        return -1;
    }
    memcpy(name, resolved, resolved_length + 1);
    oct_dns_reverse_cache_store(address, resolved, ttl);
    return 0;
}

/* Resolver A-record IPv4 lewat UDP ke server DNS dari DHCP, dengan cache TTL. */
static int oct_dns_lookup(const char *name, in_addr_t *address)
{
    if (name == NULL || name[0] == '\0'
        || strlen(name) >= sizeof(oct_dns_cache[0].name)) {
        return -1;
    }
    if (oct_dns_cache_lookup(name, address) == 0) {
        return 0;
    }

    uint8_t response[OCT_DNS_MAX_PACKET];
    size_t response_length = 0;
    uint16_t identifier = 0;
    if (oct_dns_query(name, 1, &identifier, response,
                      &response_length) != 0) {
        return -1;
    }

    uint32_t ttl = 0;
    if (oct_dns_parse_address(response, response_length, identifier, 1,
                              (uint8_t *)address, sizeof(*address), &ttl) != 0) {
        return -1;
    }
    oct_dns_cache_store(name, *address, ttl);
    return 0;
}

typedef struct {
    char name[128];
    uint8_t address[16];
    uint64_t expires;
    int used;
} oct_dns_aaaa_cache_entry;

static oct_dns_aaaa_cache_entry oct_dns_aaaa_cache[OCT_DNS_CACHE_ENTRIES];
static unsigned int oct_dns_aaaa_cache_cursor;

static int oct_dns_aaaa_cache_lookup(const char *name, uint8_t address[16])
{
    uint64_t now = (uint64_t)time(NULL);
    for (int index = 0; index < OCT_DNS_CACHE_ENTRIES; ++index) {
        oct_dns_aaaa_cache_entry *entry = &oct_dns_aaaa_cache[index];
        if (!entry->used) {
            continue;
        }
        if (entry->expires <= now) {
            entry->used = 0;
            continue;
        }
        if (oct_dns_name_equal(entry->name, name)) {
            memcpy(address, entry->address, 16);
            return 0;
        }
    }
    return -1;
}

static void oct_dns_aaaa_cache_store(const char *name, const uint8_t address[16],
                                     uint32_t ttl)
{
    if (ttl < OCT_DNS_DEFAULT_TTL) {
        ttl = OCT_DNS_DEFAULT_TTL;
    }
    if (ttl > OCT_DNS_MAX_TTL) {
        ttl = OCT_DNS_MAX_TTL;
    }
    uint64_t now = (uint64_t)time(NULL);
    int slot = -1;
    for (int index = 0; index < OCT_DNS_CACHE_ENTRIES; ++index) {
        oct_dns_aaaa_cache_entry *entry = &oct_dns_aaaa_cache[index];
        if (entry->used && entry->expires > now
            && oct_dns_name_equal(entry->name, name)) {
            slot = index;
            break;
        }
        if (slot < 0 && (!entry->used || entry->expires <= now)) {
            slot = index;
        }
    }
    if (slot < 0) {
        slot = (int)(oct_dns_aaaa_cache_cursor++ % OCT_DNS_CACHE_ENTRIES);
    }
    oct_dns_aaaa_cache_entry *entry = &oct_dns_aaaa_cache[slot];
    size_t name_length = strlen(name);
    memcpy(entry->name, name, name_length + 1);
    memcpy(entry->address, address, 16);
    entry->expires = now + ttl;
    entry->used = 1;
}

/* Resolver AAAA (IPv6) dengan cache TTL terpisah dari cache A. */
static int oct_dns_lookup_aaaa(const char *name, uint8_t address[16])
{
    if (name == NULL || name[0] == '\0'
        || strlen(name) >= sizeof(oct_dns_aaaa_cache[0].name)) {
        return -1;
    }
    if (oct_dns_aaaa_cache_lookup(name, address) == 0) {
        return 0;
    }
    uint8_t response[OCT_DNS_MAX_PACKET];
    size_t response_length = 0;
    uint16_t identifier = 0;
    if (oct_dns_query(name, 28, &identifier, response,
                      &response_length) != 0) {
        return -1;
    }
    uint32_t ttl = 0;
    if (oct_dns_parse_address(response, response_length, identifier, 28,
                              address, 16, &ttl) != 0) {
        return -1;
    }
    oct_dns_aaaa_cache_store(name, address, ttl);
    return 0;
}

/* Bangun satu entri addrinfo untuk keluarga tertentu. */
static int oct_addrinfo_entry(int family, const char *node, unsigned short port,
                              int flags, int socktype, int protocol,
                              struct addrinfo **output)
{
    *output = NULL;
    char canonical[OCT_DNS_MAX_CANON_NAME];
    canonical[0] = '\0';
    if (node != NULL && strlen(node) >= sizeof(canonical)) {
        return EAI_NONAME;
    }
    struct addrinfo *info = calloc(1, sizeof(*info));
    if (info == NULL) {
        return EAI_MEMORY;
    }
    info->ai_flags = flags;
    info->ai_family = family;
    info->ai_socktype = socktype;
    info->ai_protocol = protocol;

    if (family == AF_INET) {
        in_addr_t address;
        if (node == NULL) {
            /* Tanpa AI_PASSIVE hasilkan alamat loopback untuk klien. */
            address = (flags & AI_PASSIVE) != 0 ? INADDR_ANY : INADDR_LOOPBACK;
        } else {
            if (strchr(node, ':') != NULL) {
                free(info);
                return EAI_NONAME;
            }
            address = inet_addr(node);
            if (address == (in_addr_t)-1) {
                if ((flags & AI_NUMERICHOST) != 0
                    || oct_dns_lookup(node, &address) != 0) {
                    free(info);
                    return EAI_NONAME;
                }
            }
        }
        struct sockaddr_in *ipv4 = calloc(1, sizeof(*ipv4));
        if (ipv4 == NULL) {
            free(info);
            return EAI_MEMORY;
        }
        ipv4->sin_family = AF_INET;
        ipv4->sin_port = htons(port);
        ipv4->sin_addr.s_addr = address;
        info->ai_addrlen = sizeof(*ipv4);
        info->ai_addr = (struct sockaddr *)ipv4;
        if (node != NULL) {
            memcpy(canonical, node, strlen(node) + 1);
        } else if (oct_format_ipv4(address, canonical, sizeof(canonical)) != 0) {
            free(ipv4);
            free(info);
            return EAI_FAIL;
        }
    } else if (family == AF_INET6) {
        uint8_t address[16];
        if (node == NULL) {
            /* `::` untuk server dan `::1` untuk klien. */
            memset(address, 0, sizeof(address));
            if ((flags & AI_PASSIVE) == 0) {
                address[15] = 1;
            }
        } else if (inet_pton(AF_INET6, node, address) != 1) {
            /* Alamat IPv4 numerik bukan kandidat AAAA. */
            if (inet_addr(node) != (in_addr_t)-1) {
                free(info);
                return EAI_NONAME;
            }
            if ((flags & AI_NUMERICHOST) != 0
                || oct_dns_lookup_aaaa(node, address) != 0) {
                free(info);
                return EAI_NONAME;
            }
        }
        struct sockaddr_in6 *ipv6 = calloc(1, sizeof(*ipv6));
        if (ipv6 == NULL) {
            free(info);
            return EAI_MEMORY;
        }
        ipv6->sin6_family = AF_INET6;
        ipv6->sin6_port = htons(port);
        memcpy(ipv6->sin6_addr.s6_addr, address, sizeof(address));
        info->ai_addrlen = sizeof(*ipv6);
        info->ai_addr = (struct sockaddr *)ipv6;
        if (node != NULL) {
            memcpy(canonical, node, strlen(node) + 1);
        } else if (oct_format_ipv6(address, canonical, sizeof(canonical)) != 0) {
            free(ipv6);
            free(info);
            return EAI_FAIL;
        }
    } else {
        free(info);
        return EAI_FAMILY;
    }
    if ((flags & AI_CANONNAME) != 0) {
        info->ai_canonname = strdup(canonical);
        if (info->ai_canonname == NULL) {
            free(info->ai_addr);
            free(info);
            return EAI_MEMORY;
        }
    }
    *output = info;
    return 0;
}

int getaddrinfo(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **result)
{
    if (result == NULL) {
        return EINVAL;
    }
    *result = NULL;

    int flags = hints != NULL ? hints->ai_flags : 0;
    if ((flags & ~(AI_PASSIVE | AI_CANONNAME | AI_NUMERICHOST
                   | AI_NUMERICSERV)) != 0) {
        return EAI_BADFLAGS;
    }
    /* ai_canonname pada hints adalah buffer keluaran milik pemanggil. */
    if (hints != NULL && hints->ai_canonname != NULL) {
        return EAI_BADFLAGS;
    }

    /* AF_UNSPEC mengembalikan entri IPv4 lalu IPv6 yang berhasil di-resolve. */
    int requested = hints != NULL ? hints->ai_family : AF_UNSPEC;
    if (requested != AF_UNSPEC && requested != AF_INET && requested != AF_INET6) {
        return EAI_FAMILY;
    }
    int socktype = hints != NULL && hints->ai_socktype != 0
        ? hints->ai_socktype
        : SOCK_STREAM;
    if (socktype != SOCK_STREAM && socktype != SOCK_DGRAM
        && socktype != SOCK_RAW) {
        return EAI_SOCKTYPE;
    }
    int protocol = hints != NULL ? hints->ai_protocol : 0;
    int default_protocol = socktype == SOCK_DGRAM ? IPPROTO_UDP : IPPROTO_TCP;
    if (protocol != 0 && protocol != default_protocol) {
        return EAI_SOCKTYPE;
    }

    unsigned short port = 0;
    if (oct_parse_numeric_service(service, &port) != 0) {
        return (flags & AI_NUMERICSERV) != 0 ? EAI_NONAME : EAI_SERVICE;
    }

    int families[2] = {AF_INET, AF_INET6};
    int family_count = requested == AF_UNSPEC ? 2 : 1;
    struct addrinfo *head = NULL;
    struct addrinfo *tail = NULL;
    int last_error = EAI_NONAME;
    for (int index = 0; index < family_count; ++index) {
        int family = requested == AF_UNSPEC ? families[index] : requested;
        struct addrinfo *entry = NULL;
        int code = oct_addrinfo_entry(family, node, port, flags, socktype,
                                      protocol != 0 ? protocol : default_protocol,
                                      &entry);
        if (code != 0) {
            if (code == EAI_MEMORY) {
                freeaddrinfo(head);
                return EAI_MEMORY;
            }
            /* Keluarga yang tidak punya hasil dilewati; keluarga lain tetap. */
            last_error = code;
            continue;
        }
        if (tail == NULL) {
            head = entry;
        } else {
            tail->ai_next = entry;
        }
        tail = entry;
    }
    if (head == NULL) {
        return last_error;
    }
    *result = head;
    return 0;
}

void freeaddrinfo(struct addrinfo *result)
{
    while (result != NULL) {
        struct addrinfo *next = result->ai_next;
        free(result->ai_canonname);
        free(result->ai_addr);
        free(result);
        result = next;
    }
}

int getnameinfo(const struct sockaddr *address, socklen_t address_length,
                char *host, socklen_t host_length,
                char *service, socklen_t service_length, int flags)
{
    if ((flags & ~(NI_NUMERICHOST | NI_NUMERICSERV)) != 0) {
        return EAI_BADFLAGS;
    }
    if (address == NULL || address_length < sizeof(sa_family_t)) {
        return EAI_FAMILY;
    }
    const struct sockaddr_in *ipv4 = (const struct sockaddr_in *)address;
    const struct sockaddr_in6 *ipv6 = (const struct sockaddr_in6 *)address;
    char text[OCT_DNS_MAX_CANON_NAME];
    unsigned short port_number;
    if (ipv4->sin_family == AF_INET
        && address_length >= sizeof(struct sockaddr_in)) {
        port_number = ntohs(ipv4->sin_port);
        int resolved = (flags & NI_NUMERICHOST) == 0
            && oct_dns_reverse(ipv4->sin_addr.s_addr, text, sizeof(text)) == 0;
        if (!resolved && oct_format_ipv4(ipv4->sin_addr.s_addr, text,
                                         sizeof(text)) != 0) {
            return EAI_FAIL;
        }
    } else if (ipv6->sin6_family == AF_INET6
               && address_length >= sizeof(struct sockaddr_in6)) {
        port_number = ntohs(ipv6->sin6_port);
        if (oct_format_ipv6(ipv6->sin6_addr.s6_addr, text, sizeof(text)) != 0) {
            return EAI_FAIL;
        }
    } else {
        return EAI_FAMILY;
    }

    if (host != NULL && host_length != 0) {
        size_t length = strlen(text);
        if (length + 1 > (size_t)host_length) {
            return EAI_OVERFLOW;
        }
        memcpy(host, text, length + 1);
    }
    if (service != NULL && service_length != 0) {
        char number[8];
        int length = snprintf(number, sizeof(number), "%u",
                              (unsigned)port_number);
        if (length < 0 || (size_t)length >= sizeof(number)) {
            return EAI_FAIL;
        }
        if ((size_t)length + 1 > (size_t)service_length) {
            return EAI_OVERFLOW;
        }
        memcpy(service, number, (size_t)length + 1);
    }
    return 0;
}

static struct hostent oct_numeric_host;
static char oct_numeric_host_name[OCT_DNS_MAX_CANON_NAME];
static char *oct_numeric_host_aliases[1];
static char *oct_numeric_host_addresses[2];
static struct in_addr oct_numeric_host_address;

static struct hostent *oct_host_entry(const char *name, in_addr_t address)
{
    size_t name_length = strlen(name);
    if (name_length >= sizeof(oct_numeric_host_name)) {
        h_errno = NO_RECOVERY;
        return NULL;
    }
    memcpy(oct_numeric_host_name, name, name_length + 1);
    oct_numeric_host_address.s_addr = address;
    oct_numeric_host_aliases[0] = NULL;
    oct_numeric_host_addresses[0] = (char *)&oct_numeric_host_address;
    oct_numeric_host_addresses[1] = NULL;
    oct_numeric_host.h_name = oct_numeric_host_name;
    oct_numeric_host.h_aliases = oct_numeric_host_aliases;
    oct_numeric_host.h_addrtype = AF_INET;
    oct_numeric_host.h_length = (int)sizeof(oct_numeric_host_address);
    oct_numeric_host.h_addr_list = oct_numeric_host_addresses;
    return &oct_numeric_host;
}

static struct hostent *oct_numeric_host_entry(in_addr_t address)
{
    char text[16];
    if (oct_format_ipv4(address, text, sizeof(text)) != 0) {
        h_errno = NO_RECOVERY;
        return NULL;
    }
    return oct_host_entry(text, address);
}

struct hostent *gethostbyname(const char *name)
{
    if (name == NULL || name[0] == '\0') {
        h_errno = HOST_NOT_FOUND;
        return NULL;
    }
    in_addr_t address = inet_addr(name);
    if (address == (in_addr_t)-1) {
        if (oct_dns_lookup(name, &address) != 0) {
            h_errno = HOST_NOT_FOUND;
            return NULL;
        }
        return oct_host_entry(name, address);
    }
    return oct_numeric_host_entry(address);
}

struct hostent *gethostbyaddr(const void *address, socklen_t length, int family)
{
    if (address == NULL || family != AF_INET
        || length < sizeof(struct in_addr)) {
        h_errno = HOST_NOT_FOUND;
        return NULL;
    }
    in_addr_t value;
    memcpy(&value, address, sizeof(value));
    char name[OCT_DNS_MAX_CANON_NAME];
    if (oct_dns_reverse(value, name, sizeof(name)) == 0) {
        return oct_host_entry(name, value);
    }
    return oct_numeric_host_entry(value);
}

const char *gai_strerror(int error)
{
    switch (error) {
    case EAI_FAMILY: return "Address family not supported";
    case EAI_MEMORY: return "Out of memory";
    case EAI_NONAME: return "Host or service not found";
    case EAI_BADFLAGS: return "Invalid flags";
    case EAI_SERVICE: return "Service not supported";
    case EAI_SOCKTYPE: return "Socket type not supported";
    case EAI_OVERFLOW: return "Buffer too small";
    case EAI_FAIL: return "Address resolution failed";
    default: return "Address resolution error";
    }
}

static void oct_tls_initialize_tcb(void *thread_pointer)
{
    if (thread_pointer != NULL) {
        *(uintptr_t *)thread_pointer = (uintptr_t)thread_pointer;
    }
}

static void *oct_tls_allocate(void)
{
    uintptr_t template_start;
    uintptr_t filesz;
    uintptr_t size;
    uintptr_t alignment;
    uintptr_t aligned_size;
    if (oct_tls_layout(&template_start, &filesz, &size, &alignment,
                       &aligned_size) != 0 || aligned_size == 0) {
        return NULL;
    }

    int64_t mapping = oct_syscall6(
        16, 0, aligned_size, PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS, (uint64_t)-1, 0);
    if (mapping < 0) {
        return NULL;
    }
    void *base = (void *)(uintptr_t)mapping;
    if ((uintptr_t)base % alignment != 0) {
        oct_syscall2(17, (uint64_t)(uintptr_t)base, aligned_size);
        return NULL;
    }
    oct_tls_copy(base, template_start, filesz, size);
    void *thread_pointer = (unsigned char *)base + aligned_size;
    oct_tls_initialize_tcb(thread_pointer);
    return thread_pointer;
}

static uintptr_t oct_tls_aligned_size(void)
{
    uintptr_t template_start;
    uintptr_t filesz;
    uintptr_t size;
    uintptr_t alignment;
    uintptr_t aligned_size;
    if (oct_tls_layout(&template_start, &filesz, &size, &alignment,
                       &aligned_size) != 0) {
        return 0;
    }
    (void)filesz;
    (void)size;
    (void)alignment;
    return aligned_size;
}

static void oct_tls_release(void *thread_pointer)
{
    uintptr_t aligned_size = oct_tls_aligned_size();
    if (thread_pointer != NULL && aligned_size != 0) {
        oct_syscall2(17, (uint64_t)(uintptr_t)thread_pointer - aligned_size,
                     aligned_size);
    }
}

int __oct_tls_initialize(void)
{
    void *thread_pointer = oct_tls_allocate();
    if (oct_tls_aligned_size() != 0 && thread_pointer == NULL) {
        return -1;
    }
    int64_t result = oct_set_thread_pointer(thread_pointer);
    if (result < 0) {
        oct_tls_release(thread_pointer);
        return (int)result;
    }
    return 0;
}

typedef struct ThreadStart {
    void *(*function)(void *);
    void *argument;
} ThreadStart;

static void oct_pthread_run_key_destructors(void);
static void oct_thread_run_tls_destructors(void);

#define OCT_PTHREAD_STACK_SIZE (64u * 1024u)

static void oct_pthread_start(void *opaque) __attribute__((noreturn));

static void oct_pthread_start(void *opaque)
{
    /*
     * Rust's pthread entry trampoline must run ThreadInit::init() before any
     * allocator activity on this thread. In particular, do not touch errno or
     * free the argument wrapper before calling the Rust entry point: Rust's
     * global allocator may consult std::thread::current(), which is installed
     * by ThreadInit::init().
     */
    ThreadStart start = *(ThreadStart *)opaque;
    void *result = start.function(start.argument);
    free(opaque);
    /* C++ thread_local destructors run before the POSIX key destructors,
       matching glibc's thread exit order. */
    oct_thread_run_tls_destructors();
    /* errno/h_errno accessors initialize their per-TID state lazily. */
    oct_pthread_run_key_destructors();
    oct_thread_exit(result);
    for (;;) {
        __asm__ volatile ("pause");
    }
}

int pthread_create(pthread_t *thread, const pthread_attr_t *attributes,
                   void *(*start_routine)(void *), void *argument)
{
    if (thread == NULL || start_routine == NULL) {
        return EINVAL;
    }
    size_t stack_size = OCT_PTHREAD_STACK_SIZE;
    int detached = 0;
    if (attributes != NULL) {
        if (attributes->opaque != PTHREAD_CREATE_JOINABLE
            && attributes->opaque != PTHREAD_CREATE_DETACHED) {
            return EINVAL;
        }
        detached = attributes->opaque == PTHREAD_CREATE_DETACHED;
        if (attributes->stack_size < PTHREAD_STACK_MIN) {
            return EINVAL;
        }
        stack_size = attributes->stack_size;
    }

    if (stack_size > SIZE_MAX - 4095) {
        return EINVAL;
    }
    size_t stack_mapping_size = (stack_size + 4095) & ~(size_t)4095;
    uintptr_t tls_aligned_size = oct_tls_aligned_size();
    if (tls_aligned_size > SIZE_MAX - stack_mapping_size - 4095) {
        return EINVAL;
    }
    size_t mapping_size = stack_mapping_size + (size_t)tls_aligned_size;
    mapping_size = (mapping_size + 4095) & ~(size_t)4095;
    void *mapping = mmap(NULL, mapping_size, PROT_READ | PROT_WRITE,
                         MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mapping == MAP_FAILED) {
        return errno;
    }

    void *thread_pointer = NULL;
    if (tls_aligned_size != 0) {
        uintptr_t template_start;
        uintptr_t filesz;
        uintptr_t tls_size;
        uintptr_t alignment;
        uintptr_t checked_size;
        if (oct_tls_layout(&template_start, &filesz, &tls_size, &alignment,
                           &checked_size) != 0
            || checked_size != tls_aligned_size) {
            munmap(mapping, mapping_size);
            return EINVAL;
        }
        void *tls_base = (unsigned char *)mapping + stack_mapping_size;
        oct_tls_copy(tls_base, template_start, filesz, tls_size);
        thread_pointer = (unsigned char *)tls_base + tls_aligned_size;
        oct_tls_initialize_tcb(thread_pointer);
    }

    ThreadStart *start = malloc(sizeof(*start));
    if (start == NULL) {
        munmap(mapping, mapping_size);
        return errno;
    }
    start->function = start_routine;
    start->argument = argument;
    int64_t result = oct_thread_create(
        oct_pthread_start, start,
        (unsigned char *)mapping + stack_mapping_size,
        thread_pointer, mapping, mapping_size);
    if (result < 0) {
        free(start);
        munmap(mapping, mapping_size);
        return (int)-result;
    }
    *thread = (pthread_t)result;
    if (detached) {
        int error = pthread_detach(*thread);
        if (error != 0) {
            void *ignored = NULL;
            (void)pthread_join(*thread, &ignored);
            return error;
        }
    }
    return 0;
}

int pthread_join(pthread_t thread, void **result)
{
    int64_t status = oct_thread_join(thread, result);
    return status < 0 ? (int)-status : 0;
}

pthread_t pthread_self(void)
{
    int64_t thread = oct_thread_self();
    return thread < 0 ? 0 : (pthread_t)thread;
}

void pthread_exit(void *result)
{
    oct_thread_run_tls_destructors();
    oct_pthread_run_key_destructors();
    oct_thread_exit(result);
    for (;;) {
        __asm__ volatile ("pause");
    }
}

static int oct_futex_wait_uint(volatile unsigned int *address,
                               unsigned int expected)
{
    return (int)oct_futex((uint32_t *)address, 0 /* FUTEX_WAIT */, expected);
}

static void oct_futex_wake_uint(volatile unsigned int *address, int count)
{
    oct_futex((uint32_t *)address, 1 /* FUTEX_WAKE */, (uint32_t)count);
}

/* Tata letak kata futex mutex robust harus sama dengan yang dipakai kernel. */
#define OCT_FUTEX_WAITERS 0x80000000u
#define OCT_FUTEX_OWNER_DIED 0x40000000u
#define OCT_FUTEX_TID_MASK 0x3fffffffu
#define OCT_MUTEX_FLAG_ROBUST 0x1u
#define OCT_MUTEX_FLAG_NOTRECOVERABLE 0x2u
#define OCT_ROBUST_HEAD_LENGTH 24u
#define OCT_MAX_ROBUST_HEADS 64u

/* `robust_list_head` per thread; isi 24 byte pertama adalah kontrak kernel. */
struct oct_robust_head {
    struct oct_pthread_robust_entry *next;
    int64_t futex_offset;
    uint64_t list_op_pending;
    unsigned long tid;
    int registered;
};

static struct oct_robust_head oct_robust_heads[OCT_MAX_ROBUST_HEADS];
static volatile int oct_robust_head_lock;

static void oct_robust_head_lock_acquire(void)
{
    while (__sync_lock_test_and_set(&oct_robust_head_lock, 1)) {
        __asm__ volatile ("pause");
    }
}

static struct oct_robust_head *oct_robust_head_for_self(void)
{
    unsigned long tid = (unsigned long)pthread_self();
    if (tid == 0) {
        return NULL;
    }
    for (unsigned int i = 0; i < OCT_MAX_ROBUST_HEADS; ++i) {
        if (oct_robust_heads[i].tid == tid && oct_robust_heads[i].registered) {
            return &oct_robust_heads[i];
        }
    }

    oct_robust_head_lock_acquire();
    struct oct_robust_head *head = NULL;
    for (unsigned int i = 0; i < OCT_MAX_ROBUST_HEADS; ++i) {
        if (oct_robust_heads[i].tid == tid && oct_robust_heads[i].registered) {
            head = &oct_robust_heads[i];
            break;
        }
    }
    /* Slot milik task yang sudah tidak ada boleh dipakai ulang; TID kernel
     * monoton sehingga tidak bertabrakan dengan thread yang masih hidup. */
    for (unsigned int i = 0; head == NULL && i < OCT_MAX_ROBUST_HEADS; ++i) {
        unsigned long other = oct_robust_heads[i].tid;
        if (other == 0) {
            head = &oct_robust_heads[i];
            break;
        }
        uint64_t address = 0;
        uint64_t length = 0;
        if (oct_get_robust_list(other, &address, &length) == -ESRCH) {
            head = &oct_robust_heads[i];
        }
    }
    if (head != NULL) {
        head->next = (struct oct_pthread_robust_entry *)head;
        head->futex_offset = (int64_t)offsetof(pthread_mutex_t, state);
        head->list_op_pending = 0;
        head->tid = tid;
        head->registered = 0;
    }
    __sync_lock_release(&oct_robust_head_lock);
    if (head == NULL) {
        return NULL;
    }
    if (oct_set_robust_list((uint64_t)(uintptr_t)head, OCT_ROBUST_HEAD_LENGTH) < 0) {
        return NULL;
    }
    head->registered = 1;
    return head;
}

static void oct_robust_list_push(struct oct_robust_head *head,
                                 pthread_mutex_t *mutex)
{
    mutex->list.next = head->next;
    head->next = &mutex->list;
}

static void oct_robust_list_remove(struct oct_robust_head *head,
                                   pthread_mutex_t *mutex)
{
    struct oct_pthread_robust_entry **link = &head->next;
    while (*link != NULL && *link != (struct oct_pthread_robust_entry *)head) {
        if (*link == &mutex->list) {
            *link = mutex->list.next;
            break;
        }
        link = &(*link)->next;
    }
    mutex->list.next = NULL;
}

int pthread_mutexattr_init(pthread_mutexattr_t *attributes)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    attributes->robustness = PTHREAD_MUTEX_STALLED;
    return 0;
}

int pthread_mutexattr_destroy(pthread_mutexattr_t *attributes)
{
    return attributes == NULL ? EINVAL : 0;
}

/* OCTAGRAM currently supports only normal mutexes. The mutex implementation
 * already has normal (non-recursive) semantics, so this attribute is validated
 * but does not need additional storage in pthread_mutexattr_t. */
int pthread_mutexattr_settype(pthread_mutexattr_t *attributes, int type)
{
    if (attributes == NULL || type != PTHREAD_MUTEX_NORMAL) {
        return EINVAL;
    }
    return 0;
}

int pthread_mutexattr_setrobust(pthread_mutexattr_t *attributes, int robustness)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    if (robustness != PTHREAD_MUTEX_STALLED && robustness != PTHREAD_MUTEX_ROBUST) {
        return EINVAL;
    }
    attributes->robustness = robustness;
    return 0;
}

int pthread_mutexattr_getrobust(const pthread_mutexattr_t *attributes,
                                int *robustness)
{
    if (attributes == NULL || robustness == NULL) {
        return EINVAL;
    }
    *robustness = attributes->robustness;
    return 0;
}

int pthread_mutex_init(pthread_mutex_t *mutex,
                       const pthread_mutexattr_t *attributes)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    mutex->list.next = NULL;
    mutex->state = 0;
    mutex->flags = 0;
    if (attributes != NULL) {
        if (attributes->robustness == PTHREAD_MUTEX_ROBUST) {
            mutex->flags = OCT_MUTEX_FLAG_ROBUST;
        } else if (attributes->robustness != PTHREAD_MUTEX_STALLED) {
            return EINVAL;
        }
    }
    return 0;
}

int pthread_mutex_destroy(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    if ((mutex->flags & OCT_MUTEX_FLAG_ROBUST) != 0) {
        return (mutex->state & OCT_FUTEX_TID_MASK) == 0 ? 0 : EBUSY;
    }
    return mutex->state == 0 ? 0 : EBUSY;
}

/* Mutex robust menyimpan TID pemilik di kata futex supaya kernel bisa
 * menandainya FUTEX_OWNER_DIED saat thread pemilik keluar. */
static int oct_mutex_lock_robust(pthread_mutex_t *mutex)
{
    if ((mutex->flags & OCT_MUTEX_FLAG_NOTRECOVERABLE) != 0) {
        return ENOTRECOVERABLE;
    }
    struct oct_robust_head *head = oct_robust_head_for_self();
    if (head == NULL) {
        return EAGAIN;
    }
    unsigned int tid = (unsigned int)pthread_self() & OCT_FUTEX_TID_MASK;
    if (tid == 0) {
        return EINVAL;
    }
    for (;;) {
        unsigned int word = __atomic_load_n(&mutex->state, __ATOMIC_ACQUIRE);
        if ((word & OCT_FUTEX_TID_MASK) != 0) {
            if ((word & OCT_FUTEX_TID_MASK) == tid) {
                return EDEADLK;
            }
            if ((word & OCT_FUTEX_WAITERS) == 0) {
                __sync_bool_compare_and_swap(&mutex->state, word,
                                             word | OCT_FUTEX_WAITERS);
            }
            /* -EINTR/-EAGAIN hanya berarti ulangi; kunci tetap dicoba. */
            oct_futex_wait_uint((volatile unsigned int *)&mutex->state,
                                word | OCT_FUTEX_WAITERS);
            continue;
        }
        unsigned int desired =
            tid | (word & (OCT_FUTEX_WAITERS | OCT_FUTEX_OWNER_DIED));
        if (!__sync_bool_compare_and_swap(&mutex->state, word, desired)) {
            continue;
        }
        oct_robust_list_push(head, mutex);
        if ((mutex->flags & OCT_MUTEX_FLAG_NOTRECOVERABLE) != 0) {
            /* Balapan dengan unlock pemilik sebelumnya. */
            oct_robust_list_remove(head, mutex);
            if (__sync_lock_test_and_set(&mutex->state, 0) & OCT_FUTEX_WAITERS) {
                oct_futex_wake_uint((volatile unsigned int *)&mutex->state, 1);
            }
            return ENOTRECOVERABLE;
        }
        return (word & OCT_FUTEX_OWNER_DIED) != 0 ? EOWNERDEAD : 0;
    }
}

static int oct_mutex_trylock_robust(pthread_mutex_t *mutex)
{
    if ((mutex->flags & OCT_MUTEX_FLAG_NOTRECOVERABLE) != 0) {
        return ENOTRECOVERABLE;
    }
    struct oct_robust_head *head = oct_robust_head_for_self();
    if (head == NULL) {
        return EAGAIN;
    }
    unsigned int tid = (unsigned int)pthread_self() & OCT_FUTEX_TID_MASK;
    if (tid == 0) {
        return EINVAL;
    }
    unsigned int word = __atomic_load_n(&mutex->state, __ATOMIC_ACQUIRE);
    if ((word & OCT_FUTEX_TID_MASK) != 0) {
        return (word & OCT_FUTEX_TID_MASK) == tid ? EDEADLK : EBUSY;
    }
    unsigned int desired =
        tid | (word & (OCT_FUTEX_WAITERS | OCT_FUTEX_OWNER_DIED));
    if (!__sync_bool_compare_and_swap(&mutex->state, word, desired)) {
        return EBUSY;
    }
    oct_robust_list_push(head, mutex);
    return (word & OCT_FUTEX_OWNER_DIED) != 0 ? EOWNERDEAD : 0;
}

static int oct_mutex_unlock_robust(pthread_mutex_t *mutex)
{
    unsigned int tid = (unsigned int)pthread_self() & OCT_FUTEX_TID_MASK;
    unsigned int word = __atomic_load_n(&mutex->state, __ATOMIC_ACQUIRE);
    if (tid == 0 || (word & OCT_FUTEX_TID_MASK) != tid) {
        return EPERM;
    }
    if ((word & OCT_FUTEX_OWNER_DIED) != 0) {
        /* Pemilik sebelumnya tidak memanggil pthread_mutex_consistent(). */
        mutex->flags |= OCT_MUTEX_FLAG_NOTRECOVERABLE;
    } else {
        mutex->flags &= ~OCT_MUTEX_FLAG_NOTRECOVERABLE;
    }
    __atomic_store_n(&mutex->state, 0, __ATOMIC_RELEASE);
    if ((word & OCT_FUTEX_WAITERS) != 0) {
        oct_futex_wake_uint((volatile unsigned int *)&mutex->state, 1);
    }
    /* Kata futex sudah 0, jadi penanda kernel untuk entri yang masih tersisa
     * tidak lagi cocok; entri dilepas setelah kunci benar-benar bebas. */
    struct oct_robust_head *head = oct_robust_head_for_self();
    if (head != NULL) {
        oct_robust_list_remove(head, mutex);
    }
    return 0;
}

int pthread_mutex_consistent(pthread_mutex_t *mutex)
{
    if (mutex == NULL || (mutex->flags & OCT_MUTEX_FLAG_ROBUST) == 0) {
        return EINVAL;
    }
    unsigned int tid = (unsigned int)pthread_self() & OCT_FUTEX_TID_MASK;
    if (tid == 0) {
        return EINVAL;
    }
    for (;;) {
        unsigned int word = __atomic_load_n(&mutex->state, __ATOMIC_ACQUIRE);
        if ((word & OCT_FUTEX_TID_MASK) != tid ||
            (word & OCT_FUTEX_OWNER_DIED) == 0) {
            return EINVAL;
        }
        if (__sync_bool_compare_and_swap(&mutex->state, word,
                                         word & ~OCT_FUTEX_OWNER_DIED)) {
            return 0;
        }
    }
}

int pthread_mutex_lock(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    if ((mutex->flags & OCT_MUTEX_FLAG_ROBUST) != 0) {
        return oct_mutex_lock_robust(mutex);
    }
    if (__sync_bool_compare_and_swap(&mutex->state, 0, 1)) {
        return 0;
    }
    for (;;) {
        if (__sync_lock_test_and_set(&mutex->state, 2) == 0) {
            return 0;
        }
        while (mutex->state != 0) {
            /* -EINTR hanya berarti sinyal; kunci tetap harus diambil. */
            oct_futex_wait_uint((volatile unsigned int *)&mutex->state, 2);
        }
    }
}

int pthread_mutex_trylock(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    if ((mutex->flags & OCT_MUTEX_FLAG_ROBUST) != 0) {
        return oct_mutex_trylock_robust(mutex);
    }
    /* CAS, bukan xchg: menimpa state 2 akan menghilangkan wakeup untuk waiter. */
    return __sync_bool_compare_and_swap(&mutex->state, 0, 1) ? 0 : EBUSY;
}

int pthread_mutex_unlock(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    if ((mutex->flags & OCT_MUTEX_FLAG_ROBUST) != 0) {
        return oct_mutex_unlock_robust(mutex);
    }
    if (__sync_lock_test_and_set(&mutex->state, 0) == 2) {
        oct_futex_wake_uint((volatile unsigned int *)&mutex->state, 1);
    }
    return 0;
}

static uint64_t oct_realtime_milliseconds(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_REALTIME, &now) != 0) {
        return 0;
    }
    return (uint64_t)now.tv_sec * 1000u + (uint64_t)now.tv_nsec / 1000000u;
}

static int oct_absolute_milliseconds(const struct timespec *absolute,
                                     uint64_t *milliseconds)
{
    if (absolute == NULL || milliseconds == NULL || absolute->tv_sec < 0 ||
        absolute->tv_nsec < 0 || absolute->tv_nsec >= 1000000000) {
        return EINVAL;
    }
    *milliseconds = (uint64_t)absolute->tv_sec * 1000u +
                    (uint64_t)absolute->tv_nsec / 1000000u;
    return 0;
}

int pthread_cond_init(pthread_cond_t *cond, const pthread_condattr_t *attributes)
{
    if (cond == NULL) {
        return EINVAL;
    }
    (void)attributes;
    cond->sequence = 0;
    return 0;
}

int pthread_cond_destroy(pthread_cond_t *cond)
{
    return cond == NULL ? EINVAL : 0;
}

int pthread_cond_wait(pthread_cond_t *cond, pthread_mutex_t *mutex)
{
    if (cond == NULL || mutex == NULL) {
        return EINVAL;
    }
    uint32_t sequence = cond->sequence;
    pthread_mutex_unlock(mutex);
    for (;;) {
        int result = oct_futex_wait_uint(&cond->sequence, sequence);
        if (result == -EINTR) {
            continue;
        }
        break;
    }
    pthread_mutex_lock(mutex);
    return 0;
}

int pthread_cond_timedwait(pthread_cond_t *cond, pthread_mutex_t *mutex,
                           const struct timespec *absolute_time)
{
    if (cond == NULL || mutex == NULL) {
        return EINVAL;
    }
    uint64_t deadline;
    int conversion = oct_absolute_milliseconds(absolute_time, &deadline);
    if (conversion != 0) {
        return conversion;
    }
    uint32_t sequence = cond->sequence;
    pthread_mutex_unlock(mutex);
    int result = 0;
    for (;;) {
        uint64_t now = oct_realtime_milliseconds();
        if (now >= deadline) {
            result = ETIMEDOUT;
            break;
        }
        uint64_t remaining = deadline - now;
        struct oct_timespec timeout;
        timeout.tv_sec = (int64_t)(remaining / 1000u);
        timeout.tv_nsec = (int64_t)((remaining % 1000u) * 1000000u);
        int rc = (int)oct_futex_wait_timeout(
            (uint32_t *)&cond->sequence, sequence, &timeout);
        if (rc == -ETIMEDOUT) {
            result = ETIMEDOUT;
            break;
        }
        if (rc != -EINTR) {
            break;
        }
    }
    pthread_mutex_lock(mutex);
    return result;
}

int pthread_cond_signal(pthread_cond_t *cond)
{
    if (cond == NULL) {
        return EINVAL;
    }
    __sync_fetch_and_add(&cond->sequence, 1);
    oct_futex_wake_uint(&cond->sequence, 1);
    return 0;
}

int pthread_cond_broadcast(pthread_cond_t *cond)
{
    if (cond == NULL) {
        return EINVAL;
    }
    __sync_fetch_and_add(&cond->sequence, 1);
    oct_futex_wake_uint(&cond->sequence, 2147483647);
    return 0;
}

int pthread_condattr_init(pthread_condattr_t *attributes)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    attributes->opaque = 0;
    return 0;
}

int pthread_condattr_destroy(pthread_condattr_t *attributes)
{
    return attributes == NULL ? EINVAL : 0;
}

int pthread_condattr_setclock(pthread_condattr_t *attributes, clockid_t clock)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    attributes->opaque = (int)clock;
    return 0;
}

int pthread_detach(pthread_t thread)
{
    if (thread == 0) {
        return ESRCH;
    }
    int64_t status = oct_thread_detach(thread);
    return status < 0 ? (int)-status : 0;
}

int pthread_attr_init(pthread_attr_t *attributes)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    attributes->opaque = PTHREAD_CREATE_JOINABLE;
    attributes->stack_size = OCT_PTHREAD_STACK_SIZE;
    return 0;
}

int pthread_attr_destroy(pthread_attr_t *attributes)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    attributes->opaque = 0;
    attributes->stack_size = 0;
    return 0;
}

int pthread_attr_setstacksize(pthread_attr_t *attributes, size_t stack_size)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    if (stack_size < PTHREAD_STACK_MIN || stack_size > SIZE_MAX - 4095) {
        return EINVAL;
    }
    attributes->stack_size = stack_size;
    return 0;
}

int pthread_attr_getstacksize(const pthread_attr_t *attributes, size_t *stack_size)
{
    if (attributes == NULL || stack_size == NULL || attributes->stack_size == 0) {
        return EINVAL;
    }
    *stack_size = attributes->stack_size;
    return 0;
}

int pthread_attr_setdetachstate(pthread_attr_t *attributes, int state)
{
    if (attributes == NULL ||
        (state != PTHREAD_CREATE_JOINABLE && state != PTHREAD_CREATE_DETACHED)) {
        return EINVAL;
    }
    attributes->opaque = (uintptr_t)state;
    return 0;
}

int pthread_attr_getdetachstate(const pthread_attr_t *attributes, int *state)
{
    if (attributes == NULL || state == NULL) {
        return EINVAL;
    }
    *state = attributes->opaque == PTHREAD_CREATE_DETACHED
        ? PTHREAD_CREATE_DETACHED
        : PTHREAD_CREATE_JOINABLE;
    return 0;
}

 #define OCT_PTHREAD_KEYS_MAX 64u
#define OCT_PTHREAD_DESTRUCTOR_ITERATIONS 4u

typedef struct OctPthreadKey {
    void (*destructor)(void *);
    uint64_t generation;
    int active;
} OctPthreadKey;

static OctPthreadKey oct_pthread_keys[OCT_PTHREAD_KEYS_MAX];
static volatile int oct_pthread_key_lock;

#if !defined(OCTAGRAM_SHARED)
static __thread void *oct_pthread_key_values_static[OCT_PTHREAD_KEYS_MAX];
static __thread uint64_t oct_pthread_key_generations_static[OCT_PTHREAD_KEYS_MAX];
#endif

static void oct_pthread_key_lock_acquire(void)
{
    while (__sync_lock_test_and_set(&oct_pthread_key_lock, 1)) {
        __asm__ volatile ("pause");
    }
}

static void oct_pthread_key_lock_release(void)
{
    __sync_lock_release(&oct_pthread_key_lock);
}

static void **oct_pthread_key_values_current(void)
{
#if defined(OCTAGRAM_SHARED)
    return oct_runtime_state_for_thread()->key_values;
#else
    return oct_pthread_key_values_static;
#endif
}

static uint64_t *oct_pthread_key_generations_current(void)
{
#if defined(OCTAGRAM_SHARED)
    return oct_runtime_state_for_thread()->key_generations;
#else
    return oct_pthread_key_generations_static;
#endif
}

/* One-time initialization used by libc++ (std::call_once) and by guarded
 * local statics. States: 0 = untouched, 1 = initializer running, 2 = done.
 * The loser of the race yields instead of blocking, which is enough for the
 * cooperative/preemptive OCTAGRAM scheduler. */
int pthread_once(pthread_once_t *once, void (*initializer)(void))
{
    if (once == NULL || initializer == NULL) {
        return EINVAL;
    }
    for (;;) {
        int state = __atomic_load_n(&once->state, __ATOMIC_ACQUIRE);
        if (state == 2) {
            return 0;
        }
        int expected = 0;
        if (__atomic_compare_exchange_n(&once->state, &expected, 1, 0,
                                        __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
            initializer();
            __atomic_store_n(&once->state, 2, __ATOMIC_RELEASE);
            return 0;
        }
        sched_yield();
    }
}

int pthread_key_create(pthread_key_t *key, void (*destructor)(void *))
{
    if (key == NULL) {
        return EINVAL;
    }
    oct_pthread_key_lock_acquire();
    for (unsigned int index = 0; index < OCT_PTHREAD_KEYS_MAX; ++index) {
        OctPthreadKey *entry = &oct_pthread_keys[index];
        if (entry->active) {
            continue;
        }
        ++entry->generation;
        if (entry->generation == 0) {
            ++entry->generation;
        }
        entry->destructor = destructor;
        entry->active = 1;
        *key = index;
        oct_pthread_key_lock_release();
        return 0;
    }
    oct_pthread_key_lock_release();
    return EAGAIN;
}

int pthread_key_delete(pthread_key_t key)
{
    if (key >= OCT_PTHREAD_KEYS_MAX) {
        return EINVAL;
    }
    oct_pthread_key_lock_acquire();
    if (!oct_pthread_keys[key].active) {
        oct_pthread_key_lock_release();
        return EINVAL;
    }
    oct_pthread_keys[key].active = 0;
    oct_pthread_keys[key].destructor = NULL;
    ++oct_pthread_keys[key].generation;
    if (oct_pthread_keys[key].generation == 0) {
        ++oct_pthread_keys[key].generation;
    }
    oct_pthread_key_lock_release();
    return 0;
}

void *pthread_getspecific(pthread_key_t key)
{
    if (key >= OCT_PTHREAD_KEYS_MAX) {
        return NULL;
    }
    oct_pthread_key_lock_acquire();
    int active = oct_pthread_keys[key].active;
    uint64_t generation = oct_pthread_keys[key].generation;
    oct_pthread_key_lock_release();
    if (!active) {
        return NULL;
    }
    uint64_t *generations = oct_pthread_key_generations_current();
    return generations[key] == generation
        ? oct_pthread_key_values_current()[key] : NULL;
}

int pthread_setspecific(pthread_key_t key, const void *value)
{
    if (key >= OCT_PTHREAD_KEYS_MAX) {
        return EINVAL;
    }
    oct_pthread_key_lock_acquire();
    if (!oct_pthread_keys[key].active) {
        oct_pthread_key_lock_release();
        return EINVAL;
    }
    uint64_t generation = oct_pthread_keys[key].generation;
    void **values = oct_pthread_key_values_current();
    uint64_t *generations = oct_pthread_key_generations_current();
    values[key] = (void *)value;
    generations[key] = generation;
    oct_pthread_key_lock_release();
    return 0;
}
/* C++ thread_local objects with non-trivial destructors are registered here by
 * the compiler (`__cxa_thread_atexit`) and unwound at thread exit. The chain is
 * per thread because the nodes live in thread-local storage. */
typedef struct OctThreadExitNode {
    struct OctThreadExitNode *next;
    void (*destructor)(void *);
    void *object;
} OctThreadExitNode;

static __thread OctThreadExitNode *oct_thread_exit_chain;

int __cxa_thread_atexit(void (*destructor)(void *), void *object, void *dso)
{
    (void)dso;
    if (destructor == NULL) {
        return -1;
    }
    OctThreadExitNode *node = malloc(sizeof(*node));
    if (node == NULL) {
        return -1;
    }
    node->destructor = destructor;
    node->object = object;
    node->next = oct_thread_exit_chain;
    oct_thread_exit_chain = node;
    return 0;
}

/* glibc's internal spelling; some C++ runtimes call this one directly. */
int __cxa_thread_atexit_impl(void (*destructor)(void *), void *object, void *dso)
{
    return __cxa_thread_atexit(destructor, object, dso);
}

static void oct_thread_run_tls_destructors(void)
{
    /* A destructor may register another one, so drain until the chain is empty. */
    while (oct_thread_exit_chain != NULL) {
        OctThreadExitNode *node = oct_thread_exit_chain;
        oct_thread_exit_chain = node->next;
        void (*destructor)(void *) = node->destructor;
        void *object = node->object;
        free(node);
        if (destructor != NULL) {
            destructor(object);
        }
    }
}

static void oct_pthread_run_key_destructors(void)
{
    void **values = oct_pthread_key_values_current();
    for (unsigned int iteration = 0;
         iteration < OCT_PTHREAD_DESTRUCTOR_ITERATIONS; ++iteration) {
        int called = 0;
        uint64_t *generations = oct_pthread_key_generations_current();
        for (unsigned int key = 0; key < OCT_PTHREAD_KEYS_MAX; ++key) {
            void *value = values[key];
            if (value == NULL) {
                continue;
            }
            oct_pthread_key_lock_acquire();
            int valid = oct_pthread_keys[key].active &&
                generations[key] == oct_pthread_keys[key].generation;
            void (*destructor)(void *) = valid
                ? oct_pthread_keys[key].destructor : NULL;
            oct_pthread_key_lock_release();
            values[key] = NULL;
            generations[key] = 0;
            if (destructor != NULL) {
                called = 1;
                destructor(value);
            }
        }
        if (!called) {
            break;
        }
    }
}

int sem_init(sem_t *semaphore, int shared, unsigned int value)
{
    if (semaphore == NULL) {
        return EINVAL;
    }
    if (shared != 0) {
        return ENOSYS;
    }
    semaphore->count = value;
    return 0;
}

int sem_destroy(sem_t *semaphore)
{
    return semaphore == NULL ? EINVAL : 0;
}

int sem_trywait(sem_t *semaphore)
{
    if (semaphore == NULL) {
        return EINVAL;
    }
    for (;;) {
        unsigned int value = semaphore->count;
        if (value == 0) {
            errno = EAGAIN;
            return -1;
        }
        if (__sync_bool_compare_and_swap(&semaphore->count, value, value - 1)) {
            return 0;
        }
    }
}

int sem_wait(sem_t *semaphore)
{
    if (semaphore == NULL) {
        return EINVAL;
    }
    for (;;) {
        unsigned int value = semaphore->count;
        if (value > 0) {
            if (__sync_bool_compare_and_swap(&semaphore->count, value,
                                             value - 1)) {
                return 0;
            }
            continue;
        }
        int result = oct_futex_wait_uint(&semaphore->count, 0);
        if (result == -EINTR) {
            continue;
        }
        if (result != 0 && result != -EAGAIN) {
            errno = -result;
            return -1;
        }
    }
}

int sem_timedwait(sem_t *semaphore, const struct timespec *absolute_time)
{
    if (semaphore == NULL) {
        return EINVAL;
    }
    uint64_t deadline;
    int conversion = oct_absolute_milliseconds(absolute_time, &deadline);
    if (conversion != 0) {
        errno = conversion;
        return -1;
    }
    for (;;) {
        unsigned int value = semaphore->count;
        if (value > 0) {
            if (__sync_bool_compare_and_swap(&semaphore->count, value,
                                             value - 1)) {
                return 0;
            }
            continue;
        }
        uint64_t now = oct_realtime_milliseconds();
        if (now >= deadline) {
            errno = ETIMEDOUT;
            return -1;
        }
        uint64_t remaining = deadline - now;
        struct oct_timespec timeout;
        timeout.tv_sec = (int64_t)(remaining / 1000u);
        timeout.tv_nsec = (int64_t)((remaining % 1000u) * 1000000u);
        int result = (int)oct_futex_wait_timeout(
            (uint32_t *)&semaphore->count, 0, &timeout);
        if (result == -ETIMEDOUT) {
            errno = ETIMEDOUT;
            return -1;
        }
        if (result != 0 && result != -EAGAIN && result != -EINTR) {
            errno = -result;
            return -1;
        }
    }
}

int sem_post(sem_t *semaphore)
{
    if (semaphore == NULL) {
        return EINVAL;
    }
    __sync_fetch_and_add(&semaphore->count, 1);
    oct_futex_wake_uint(&semaphore->count, 1);
    return 0;
}

int sem_getvalue(sem_t *semaphore, int *value)
{
    if (semaphore == NULL || value == NULL) {
        return EINVAL;
    }
    *value = (int)semaphore->count;
    return 0;
}

void *mmap(void *address, size_t length, int protection, int flags,
           int file_descriptor, off_t offset)
{
    if (offset < 0) {
        errno = EINVAL;
        return MAP_FAILED;
    }
    int64_t result = oct_syscall6(
        16, (uint64_t)(uintptr_t)address, length, (uint64_t)(unsigned int)protection,
        (uint64_t)(unsigned int)flags, (uint64_t)(int64_t)file_descriptor,
        (uint64_t)offset);
    if (result < 0) {
        posix_result(result);
        return MAP_FAILED;
    }
    return (void *)(uintptr_t)result;
}

int munmap(void *address, size_t length)
{
    return (int)posix_result(oct_syscall2(
        17, (uint64_t)(uintptr_t)address, (uint64_t)length));
}

int mprotect(void *address, size_t length, int protection)
{
    return (int)posix_result(oct_syscall3(
        52, (uint64_t)(uintptr_t)address, (uint64_t)length,
        (uint64_t)(unsigned int)protection));
}

int msync(void *address, size_t length, int flags)
{
    return (int)posix_result(oct_syscall3(
        51, (uint64_t)(uintptr_t)address, (uint64_t)length,
        (uint64_t)(unsigned int)flags));
}

int mincore(void *address, size_t length, unsigned char *vector)
{
    return (int)posix_result(oct_syscall3(
        53, (uint64_t)(uintptr_t)address, (uint64_t)length,
        (uint64_t)(uintptr_t)vector));
}

int mlock(const void *address, size_t length)
{
    return (int)posix_result(oct_syscall2(
        54, (uint64_t)(uintptr_t)address, (uint64_t)length));
}

int munlock(const void *address, size_t length)
{
    return (int)posix_result(oct_syscall2(
        55, (uint64_t)(uintptr_t)address, (uint64_t)length));
}

int mlockall(int flags)
{
    return (int)posix_result(oct_syscall1(
        56, (uint64_t)(unsigned int)flags));
}

int munlockall(void)
{
    return (int)posix_result(oct_syscall0(57));
}

int posix_madvise(void *address, size_t length, int advice)
{
    int64_t result = oct_syscall3(
        58, (uint64_t)(uintptr_t)address, (uint64_t)length,
        (uint64_t)(unsigned int)advice);
    return result < 0 ? (int)-result : 0;
}

void *memcpy(void *destination, const void *source, size_t count)
{
    unsigned char *out = destination;
    const unsigned char *in = source;
    for (size_t i = 0; i < count; ++i) {
        out[i] = in[i];
    }
    return destination;
}

void *memmove(void *destination, const void *source, size_t count)
{
    unsigned char *out = destination;
    const unsigned char *in = source;
    if ((uintptr_t)out <= (uintptr_t)in) {
        for (size_t i = 0; i < count; ++i) {
            out[i] = in[i];
        }
    } else {
        for (size_t i = count; i != 0; --i) {
            out[i - 1] = in[i - 1];
        }
    }
    return destination;
}

void *memset(void *destination, int value, size_t count)
{
    unsigned char *out = destination;
    for (size_t i = 0; i < count; ++i) {
        out[i] = (unsigned char)value;
    }
    return destination;
}

void *memchr(const void *memory, int character, size_t count)
{
    const unsigned char *bytes = memory;
    unsigned char target = (unsigned char)character;
    for (size_t index = 0; index < count; ++index) {
        if (bytes[index] == target) {
            return (void *)(bytes + index);
        }
    }
    return NULL;
}

void *memrchr(const void *memory, int character, size_t count)
{
    const unsigned char *bytes = memory;
    unsigned char target = (unsigned char)character;
    if (count == 0) {
        return NULL;
    }
    for (size_t index = count; index > 0; --index) {
        if (bytes[index - 1] == target) {
            return (void *)(bytes + index - 1);
        }
    }
    return NULL;
}

char *strcpy(char *destination, const char *source)
{
    char *result = destination;
    while ((*destination++ = *source++) != '\0') {
    }
    return result;
}

char *strcat(char *destination, const char *source)
{
    strcpy(destination + strlen(destination), source);
    return destination;
}

char *strncat(char *destination, const char *source, size_t count)
{
    size_t destination_length = strlen(destination);
    size_t index = 0;
    while (index < count && source[index] != '\0') {
        destination[destination_length + index] = source[index];
        ++index;
    }
    destination[destination_length + index] = '\0';
    return destination;
}

char *strncpy(char *destination, const char *source, size_t count)
{
    size_t index = 0;
    while (index < count && source[index] != '\0') {
        destination[index] = source[index];
        ++index;
    }
    while (index < count) {
        destination[index++] = '\0';
    }
    return destination;
}

int memcmp(const void *left, const void *right, size_t count)
{
    const unsigned char *a = left;
    const unsigned char *b = right;
    for (size_t i = 0; i < count; ++i) {
        if (a[i] != b[i]) {
            return (int)a[i] - (int)b[i];
        }
    }
    return 0;
}

size_t strlen(const char *text)
{
    size_t length = 0;
    while (text[length] != '\0') {
        ++length;
    }
    return length;
}

size_t strspn(const char *text, const char *accept)
{
    size_t length = 0;
    while (text[length] != '\0' &&
           strchr(accept, (unsigned char)text[length]) != NULL) {
        ++length;
    }
    return length;
}

size_t strcspn(const char *text, const char *reject)
{
    size_t length = 0;
    while (text[length] != '\0' &&
           strchr(reject, (unsigned char)text[length]) == NULL) {
        ++length;
    }
    return length;
}

int strcmp(const char *left, const char *right)
{
    while (*left != '\0' && *left == *right) {
        ++left;
        ++right;
    }
    return (unsigned char)*left - (unsigned char)*right;
}

int strncmp(const char *left, const char *right, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        unsigned char a = (unsigned char)left[index];
        unsigned char b = (unsigned char)right[index];
        if (a != b || a == '\0') {
            return (int)a - (int)b;
        }
    }
    return 0;
}

int strncasecmp(const char *left, const char *right, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        unsigned char a = (unsigned char)tolower((unsigned char)left[index]);
        unsigned char b = (unsigned char)tolower((unsigned char)right[index]);
        if (a != b || a == '\0') {
            return (int)a - (int)b;
        }
    }
    return 0;
}

int atoi(const char *text)
{
    return (int)strtol(text, NULL, 10);
}

int abs(int value)
{
    return value < 0 ? -value : value;
}

char *strchr(const char *text, int character)
{
    char target = (char)character;
    for (;;) {
        if (*text == target) {
            return (char *)text;
        }
        if (*text == '\0') {
            return NULL;
        }
        ++text;
    }
}

char *strrchr(const char *text, int character)
{
    const char *found = NULL;
    char target = (char)character;
    for (;;) {
        if (*text == target) {
            found = text;
        }
        if (*text == '\0') {
            return (char *)found;
        }
        ++text;
    }
}

char *strpbrk(const char *text, const char *accept)
{
    for (; *text != '\0'; ++text) {
        if (strchr(accept, (unsigned char)*text) != NULL) {
            return (char *)text;
        }
    }
    return NULL;
}

char *strtok_r(char *text, const char *delimiters, char **save_pointer)
{
    if (save_pointer == NULL) {
        return NULL;
    }
    if (text == NULL) {
        text = *save_pointer;
    }
    if (text == NULL) {
        return NULL;
    }

    text += strspn(text, delimiters);
    if (*text == '\0') {
        *save_pointer = text;
        return NULL;
    }

    char *token = text;
    text += strcspn(text, delimiters);
    if (*text != '\0') {
        *text++ = '\0';
    }
    *save_pointer = text;
    return token;
}

static char *strtok_save_pointer;

char *strtok(char *text, const char *delimiters)
{
    return strtok_r(text, delimiters, &strtok_save_pointer);
}

char *strstr(const char *text, const char *substring)
{
    if (*substring == '\0') {
        return (char *)text;
    }
    for (; *text != '\0'; ++text) {
        if (*text == *substring && strncmp(text, substring, strlen(substring)) == 0) {
            return (char *)text;
        }
    }
    return NULL;
}

char *strndup(const char *text, size_t count)
{
    size_t length = 0;
    while (length < count && text[length] != '\0') {
        ++length;
    }
    if (length == SIZE_MAX) {
        return NULL;
    }
    char *copy = malloc(length + 1);
    if (copy == NULL) {
        return NULL;
    }
    memcpy(copy, text, length);
    copy[length] = '\0';
    return copy;
}

char *strdup(const char *text)
{
    return strndup(text, strlen(text));
}

int strerror_r(int error, char *buffer, size_t length)
{
    if (buffer == NULL) {
        return EINVAL;
    }
    if (length == 0) {
        return ERANGE;
    }
    const char *message = strerror(error);
    size_t needed = strlen(message) + 1;
    if (needed > length) {
        memcpy(buffer, message, length - 1);
        buffer[length - 1] = '\0';
        return ERANGE;
    }
    memcpy(buffer, message, needed);
    return 0;
}

int chroot(const char *path)
{
    (void)path;
    errno = ENOSYS;
    return -1;
}

char *strerror(int error)
{
    switch (error) {
    case EACCES: return "permission denied";
    case EBADF: return "bad file descriptor";
    case EEXIST: return "file exists";
    case EINVAL: return "invalid argument";
    case EIO: return "I/O error";
    case EISDIR: return "is a directory";
    case ELOOP: return "too many symbolic links";
    case EMFILE: return "too many open files";
    case ENAMETOOLONG: return "name too long";
    case ENOENT: return "no such file or directory";
    case ENOMEM: return "out of memory";
    case ENOSPC: return "no space left";
    case ENOSYS: return "function not implemented";
    case ENOTDIR: return "not a directory";
    case EROFS: return "read-only filesystem";
    case ESPIPE: return "invalid seek";
    case ENOTSUP: return "operation not supported";
    default: return "unknown error";
    }
}

char *strsignal(int signal_number)
{
    switch (signal_number) {
    case SIGINT: return "Interrupt";
    case SIGABRT: return "Aborted";
    case SIGFPE: return "Floating point exception";
    case SIGILL: return "Illegal instruction";
    case SIGSEGV: return "Segmentation fault";
    case SIGTERM: return "Terminated";
    case SIGKILL: return "Killed";
    default: return "Unknown signal";
    }
}

unsigned long strtoul(const char *text, char **end, int base)
{
    if (base != 0 && (base < 2 || base > 36)) {
        errno = EINVAL;
        if (end != NULL) {
            *end = (char *)text;
        }
        return 0;
    }

    const char *cursor = text;
    while (isspace((unsigned char)*cursor)) {
        ++cursor;
    }
    int negative = 0;
    if (*cursor == '+' || *cursor == '-') {
        negative = *cursor == '-';
        ++cursor;
    }
    if ((base == 0 || base == 16) && cursor[0] == '0' &&
        (cursor[1] == 'x' || cursor[1] == 'X')) {
        cursor += 2;
        if (base == 0) {
            base = 16;
        }
    } else if (base == 0 && cursor[0] == '0' &&
               (cursor[1] == 'b' || cursor[1] == 'B')) {
        cursor += 2;
        base = 2;
    } else if (base == 0) {
        base = cursor[0] == '0' ? 8 : 10;
    }

    const char *digits = cursor;
    unsigned long value = 0;
    int overflow = 0;
    for (;;) {
        int digit;
        unsigned char character = (unsigned char)*cursor;
        if (character >= '0' && character <= '9') {
            digit = character - '0';
        } else if (character >= 'a' && character <= 'z') {
            digit = character - 'a' + 10;
        } else if (character >= 'A' && character <= 'Z') {
            digit = character - 'A' + 10;
        } else {
            break;
        }
        if (digit >= base) {
            break;
        }
        if (value > (UINT64_MAX - (unsigned long)digit) / (unsigned long)base) {
            overflow = 1;
            value = UINT64_MAX;
        } else if (!overflow) {
            value = value * (unsigned long)base + (unsigned long)digit;
        }
        ++cursor;
    }
    if (cursor == digits) {
        if (end != NULL) {
            *end = (char *)text;
        }
        return 0;
    }
    if (end != NULL) {
        *end = (char *)cursor;
    }
    if (overflow) {
        errno = ERANGE;
    }
    return negative ? 0UL - value : value;
}

long strtol(const char *text, char **end, int base)
{
    char *parsed_end;
    unsigned long value = strtoul(text, &parsed_end, base);
    if (end != NULL) {
        *end = parsed_end;
    }
    if (parsed_end == text) {
        return 0;
    }
    const char *cursor = text;
    while (isspace((unsigned char)*cursor)) {
        ++cursor;
    }
    if (*cursor == '-') {
        unsigned long limit = (unsigned long)LONG_MAX + 1;
        if (value > limit) {
            errno = ERANGE;
            return LONG_MIN;
        }
        return value == limit ? LONG_MIN : -(long)value;
    }
    if (value > (unsigned long)LONG_MAX) {
        errno = ERANGE;
        return LONG_MAX;
    }
    return (long)value;
}

unsigned long long strtoull(const char *text, char **end, int base)
{
    return (unsigned long long)strtoul(text, end, base);
}

long long strtoll(const char *text, char **end, int base)
{
    return (long long)strtol(text, end, base);
}

long double strtold(const char *text, char **end)
{
    const char *cursor = text;
    while (isspace((unsigned char)*cursor)) {
        ++cursor;
    }
    int sign = 1;
    if (*cursor == '+' || *cursor == '-') {
        sign = *cursor == '-' ? -1 : 1;
        ++cursor;
    }

    int radix = 10;
    int exponent_radix = 10;
    if (cursor[0] == '0' && (cursor[1] == 'x' || cursor[1] == 'X')) {
        cursor += 2;
        radix = 16;
        exponent_radix = 2;
    }
    long double value = 0.0L;
    int saw_digit = 0;
    while (1) {
        unsigned char character = (unsigned char)*cursor;
        int digit = character >= '0' && character <= '9' ? character - '0' :
                    character >= 'a' && character <= 'f' ? character - 'a' + 10 :
                    character >= 'A' && character <= 'F' ? character - 'A' + 10 : radix;
        if (digit >= radix) {
            break;
        }
        value = value * radix + digit;
        saw_digit = 1;
        ++cursor;
    }

    if (*cursor == '.') {
        ++cursor;
        long double place = 1.0L / radix;
        while (1) {
            unsigned char character = (unsigned char)*cursor;
            int digit = character >= '0' && character <= '9' ? character - '0' :
                        character >= 'a' && character <= 'f' ? character - 'a' + 10 :
                        character >= 'A' && character <= 'F' ? character - 'A' + 10 : radix;
            if (digit >= radix) {
                break;
            }
            value += digit * place;
            place /= radix;
            saw_digit = 1;
            ++cursor;
        }
    }
    if (!saw_digit) {
        if (end != NULL) {
            *end = (char *)text;
        }
        return 0.0L;
    }

    int exponent = 0;
    if ((exponent_radix == 10 && (*cursor == 'e' || *cursor == 'E')) ||
        (exponent_radix == 2 && (*cursor == 'p' || *cursor == 'P'))) {
        ++cursor;
        int exponent_sign = 1;
        if (*cursor == '+' || *cursor == '-') {
            exponent_sign = *cursor == '-' ? -1 : 1;
            ++cursor;
        }
        while (isdigit((unsigned char)*cursor) && exponent < 4096) {
            exponent = exponent * 10 + (*cursor++ - '0');
        }
        exponent *= exponent_sign;
    }
    while (exponent > 0) {
        value *= exponent_radix;
        --exponent;
    }
    while (exponent < 0) {
        value /= exponent_radix;
        ++exponent;
    }
    if (end != NULL) {
        *end = (char *)cursor;
    }
    return sign < 0 ? -value : value;
}

double strtod(const char *text, char **end)
{
    return (double)strtold(text, end);
}

double atof(const char *text)
{
    return strtod(text, NULL);
}

float strtof(const char *text, char **end)
{
    return (float)strtold(text, end);
}

char *getenv(const char *name)
{
    if (name == NULL || name[0] == '\0' || environ == NULL) {
        return NULL;
    }

    size_t name_length = strlen(name);
    for (char **entry = environ; *entry != NULL; ++entry) {
        size_t index = 0;
        while (index < name_length && (*entry)[index] != '\0' &&
               (*entry)[index] == name[index]) {
            ++index;
        }
        if (index == name_length && (*entry)[index] == '=') {
            return &(*entry)[index + 1];
        }
    }
    return NULL;
}

static int valid_environment_name(const char *name)
{
    if (name == NULL || name[0] == '\0') {
        return 0;
    }
    if (!((*name >= 'A' && *name <= 'Z') ||
          (*name >= 'a' && *name <= 'z') || *name == '_')) {
        return 0;
    }
    for (++name; *name != '\0'; ++name) {
        if (!((*name >= 'A' && *name <= 'Z') ||
              (*name >= 'a' && *name <= 'z') ||
              (*name >= '0' && *name <= '9') || *name == '_')) {
            return 0;
        }
    }
    return 1;
}

static int environment_entry_matches(const char *entry, const char *name,
                                     size_t name_length)
{
    size_t index = 0;
    while (index < name_length && entry[index] == name[index]) {
        ++index;
    }
    return index == name_length && entry[index] == '=';
}

static int current_environment_count(size_t *count)
{
    size_t entries = 0;
    if (environ != NULL) {
        while (entries < 64 && environ[entries] != NULL) {
            ++entries;
        }
        if (entries == 64 && environ[entries] != NULL) {
            errno = E2BIG;
            return -1;
        }
    }
    *count = entries;
    return 0;
}

static int install_environment(char **environment)
{
    environ = environment;
    return 0;
}

int setenv(const char *name, const char *value, int overwrite)
{
    if (!valid_environment_name(name) || value == NULL) {
        errno = EINVAL;
        return -1;
    }

    size_t count;
    if (current_environment_count(&count) < 0) {
        return -1;
    }
    size_t name_length = strlen(name);
    int found = 0;
    if (environ != NULL) {
        for (size_t index = 0; index < count; ++index) {
            if (environment_entry_matches(environ[index], name, name_length)) {
                found = 1;
                break;
            }
        }
    }
    if (found && !overwrite) {
        return 0;
    }
    if (!found && count == 64) {
        errno = E2BIG;
        return -1;
    }

    size_t value_length = strlen(value);
    if (name_length > SIZE_MAX - 2 ||
        value_length > SIZE_MAX - name_length - 2) {
        errno = ENOMEM;
        return -1;
    }
    size_t entry_size = name_length + value_length + 2;
    char *new_entry = malloc(entry_size);
    if (new_entry == NULL) {
        errno = ENOMEM;
        return -1;
    }
    memcpy(new_entry, name, name_length);
    new_entry[name_length] = '=';
    memcpy(new_entry + name_length + 1, value, value_length + 1);

    char **new_environment = malloc((count + 2) * sizeof(*new_environment));
    if (new_environment == NULL) {
        errno = ENOMEM;
        return -1;
    }
    size_t output = 0;
    int inserted = 0;
    if (environ != NULL) {
        for (size_t index = 0; index < count; ++index) {
            if (!environment_entry_matches(environ[index], name, name_length)) {
                new_environment[output++] = environ[index];
            } else if (!inserted) {
                new_environment[output++] = new_entry;
                inserted = 1;
            }
        }
    }
    if (!inserted) {
        new_environment[output++] = new_entry;
    }
    new_environment[output] = NULL;
    return install_environment(new_environment);
}

int unsetenv(const char *name)
{
    if (!valid_environment_name(name)) {
        errno = EINVAL;
        return -1;
    }

    size_t count;
    if (current_environment_count(&count) < 0) {
        return -1;
    }
    size_t name_length = strlen(name);
    int found = 0;
    if (environ != NULL) {
        for (size_t index = 0; index < count; ++index) {
            found |= environment_entry_matches(environ[index], name, name_length);
        }
    }
    if (!found) {
        return 0;
    }

    char **new_environment = malloc((count + 1) * sizeof(*new_environment));
    if (new_environment == NULL) {
        errno = ENOMEM;
        return -1;
    }
    size_t output = 0;
    for (size_t index = 0; index < count; ++index) {
        if (!environment_entry_matches(environ[index], name, name_length)) {
            new_environment[output++] = environ[index];
        }
    }
    new_environment[output] = NULL;
    return install_environment(new_environment);
}

/*
 * Create a unique temporary file. O_EXCL is enforced by the kernel while
 * holding the VFS lock, so a pre-existing path cannot be overwritten.
 */
int mkstemp(char *template)
{
    static uint64_t sequence;
    static const char alphabet[] = "0123456789abcdefghijklmnopqrstuvwxyz";
    if (template == NULL) {
        errno = EINVAL;
        return -1;
    }

    size_t length = strlen(template);
    if (length < 6) {
        errno = EINVAL;
        return -1;
    }
    char *suffix = template + length - 6;
    for (size_t i = 0; i < 6; ++i) {
        if (suffix[i] != 'X') {
            errno = EINVAL;
            return -1;
        }
    }

    struct timespec now = {0, 0};
    (void)clock_gettime(CLOCK_REALTIME, &now);
    uint64_t seed = ((uint64_t)(uint32_t)getpid() << 32) ^
                    (uint64_t)now.tv_sec ^
                    ((uint64_t)now.tv_nsec << 1);

    for (unsigned int attempt = 0; attempt < 256; ++attempt) {
        uint64_t value = seed +
            __atomic_add_fetch(&sequence, 1, __ATOMIC_RELAXED) +
            ((uint64_t)attempt * UINT64_C(0x9e3779b97f4a7c15));
        for (size_t i = 0; i < 6; ++i) {
            suffix[i] = alphabet[value % 36u];
            value /= 36u;
        }
        int fd = open(template, O_RDWR | O_CREAT | O_EXCL, 0600);
        if (fd >= 0) {
            return fd;
        }
        if (errno != EEXIST) {
            return -1;
        }
    }

    errno = EEXIST;
    return -1;
}

/* POSIX tmpfile: unlink the unique file while its descriptor remains open. */
FILE *tmpfile(void)
{
    char path[] = "/tmp/octagram-tmp-XXXXXX";
    int fd = mkstemp(path);
    if (fd < 0) return NULL;
    if (unlink(path) < 0) {
        int saved_errno = errno;
        close(fd);
        errno = saved_errno;
        return NULL;
    }
    FILE *stream = fdopen(fd, "w+");
    if (stream == NULL) {
        int saved_errno = errno;
        close(fd);
        errno = saved_errno;
        return NULL;
    }
    return stream;
}

int open(const char *path, int flags, ...)
{
    mode_t mode = 0644;
    if ((unsigned int)flags & O_CREAT) {
        va_list arguments;
        va_start(arguments, flags);
        mode = (mode_t)va_arg(arguments, unsigned int);
        va_end(arguments);
    }
    return (int)posix_result(oct_open_mode(
        path, (uint64_t)(unsigned int)flags, (uint32_t)mode));
}

int openat(int dirfd, const char *path, int flags, ...)
{
    mode_t mode = 0644;
    if ((unsigned int)flags & O_CREAT) {
        va_list arguments;
        va_start(arguments, flags);
        mode = (mode_t)va_arg(arguments, unsigned int);
        va_end(arguments);
    }
    return (int)posix_result(oct_openat(
        dirfd, path, (uint64_t)(unsigned int)flags, (uint32_t)mode));
}

int close(int fd)
{
    return (int)posix_result(oct_close(fd));
}

int socket(int domain, int type, int protocol)
{
    int flags = type & (SOCK_CLOEXEC | SOCK_NONBLOCK);
    type &= ~(SOCK_CLOEXEC | SOCK_NONBLOCK);
    int fd;
    if (domain == AF_INET && type == SOCK_DGRAM) {
        fd = (int)posix_result(oct_udp_socket());
    } else {
        fd = (int)posix_result(oct_socket(domain, type, protocol));
    }
    if (fd < 0) {
        return fd;
    }
    if ((flags & SOCK_CLOEXEC) != 0) {
        fcntl(fd, F_SETFD, FD_CLOEXEC);
    }
    if ((flags & SOCK_NONBLOCK) != 0) {
        fcntl(fd, F_SETFL, O_NONBLOCK);
    }
    return fd;
}

int bind(int socket_fd, const struct sockaddr *address, socklen_t address_length)
{
    if (address != NULL && address_length >= sizeof(struct sockaddr_in)) {
        const struct sockaddr_in *ipv4 = (const struct sockaddr_in *)address;
        if (ipv4->sin_family == AF_INET) {
            return (int)posix_result(oct_udp_bind(socket_fd,
                                                  ntohs(ipv4->sin_port)));
        }
    }
    return (int)posix_result(oct_bind(socket_fd, address, address_length));
}

int listen(int socket_fd, int backlog)
{
    return (int)posix_result(oct_listen(socket_fd, backlog));
}

int accept(int socket_fd, struct sockaddr *address, socklen_t *address_length)
{
    for (;;) {
        int64_t result = oct_accept(socket_fd, address, address_length);
        if (result != -EAGAIN) {
            return (int)posix_result(result);
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (int)posix_result(yield_result);
        }
    }
}

int connect(int socket_fd, const struct sockaddr *address,
            socklen_t address_length)
{
    for (;;) {
        int64_t result = oct_connect(socket_fd, address, address_length);
        if (result != -EAGAIN) {
            return (int)posix_result(result);
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (int)posix_result(yield_result);
        }
    }
}

ssize_t send(int socket_fd, const void *buffer, size_t length, int flags)
{
    if (length > OCT_MAX_SYSCALL_IO) {
        length = OCT_MAX_SYSCALL_IO;
    }
    for (;;) {
        int64_t result = oct_send(socket_fd, buffer, length, flags);
        if (result != -EAGAIN || (flags & MSG_DONTWAIT) != 0) {
            return (ssize_t)posix_result(result);
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (ssize_t)posix_result(yield_result);
        }
    }
}

static ssize_t recv_once(int socket_fd, void *buffer, size_t length, int flags)
{
    for (;;) {
        int64_t result = oct_recv(socket_fd, buffer, length, flags);
        if (result != -EAGAIN || (flags & MSG_DONTWAIT) != 0) {
            return (ssize_t)posix_result(result);
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (ssize_t)posix_result(yield_result);
        }
    }
}

ssize_t recv(int socket_fd, void *buffer, size_t length, int flags)
{
    if (length > OCT_MAX_SYSCALL_IO) {
        length = OCT_MAX_SYSCALL_IO;
    }
    if ((flags & MSG_WAITALL) == 0 || (flags & MSG_PEEK) != 0) {
        return recv_once(socket_fd, buffer, length, flags);
    }

    size_t received = 0;
    while (received < length) {
        ssize_t result = recv_once(
            socket_fd, (unsigned char *)buffer + received,
            length - received, flags & ~MSG_WAITALL);
        if (result < 0) {
            return received == 0 ? -1 : (ssize_t)received;
        }
        if (result == 0) {
            break;
        }
        received += (size_t)result;
    }
    return (ssize_t)received;
}

int shutdown(int socket_fd, int how)
{
    return (int)posix_result(oct_shutdown(socket_fd, how));
}

int getsockname(int socket_fd, struct sockaddr *address,
                socklen_t *address_length)
{
    return (int)posix_result(oct_getsockname(socket_fd, address, address_length));
}

int getpeername(int socket_fd, struct sockaddr *address,
                socklen_t *address_length)
{
    return (int)posix_result(oct_getpeername(socket_fd, address, address_length));
}

ssize_t sendto(int socket_fd, const void *buffer, size_t length, int flags,
               const struct sockaddr *destination, socklen_t destination_length)
{
    if (length > OCT_MAX_SYSCALL_IO) {
        length = OCT_MAX_SYSCALL_IO;
    }
    if (destination == NULL ||
        destination_length < sizeof(struct sockaddr_in)) {
        errno = EINVAL;
        return -1;
    }
    const struct sockaddr_in *ipv4 = (const struct sockaddr_in *)destination;
    if (ipv4->sin_family != AF_INET) {
        errno = EAFNOSUPPORT;
        return -1;
    }
    for (;;) {
        int64_t result = oct_udp_sendto(
            socket_fd,
            ntohl(ipv4->sin_addr.s_addr),
            ntohs(ipv4->sin_port),
            buffer,
            length);
        if (result != -EAGAIN || (flags & MSG_DONTWAIT) != 0) {
            return (ssize_t)posix_result(result);
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (ssize_t)posix_result(yield_result);
        }
    }
}

ssize_t recvfrom(int socket_fd, void *buffer, size_t length, int flags,
                 struct sockaddr *source, socklen_t *source_length)
{
    if (length > OCT_MAX_SYSCALL_IO) {
        length = OCT_MAX_SYSCALL_IO;
    }
    struct sockaddr_in ipv4;
    uint8_t source_ip[4] = {0};
    uint16_t source_port = 0;
    memset(&ipv4, 0, sizeof(ipv4));
    for (;;) {
        int64_t result = oct_udp_recvfrom(
            socket_fd,
            buffer,
            length,
            source != NULL ? source_ip : 0,
            source != NULL ? &source_port : 0);
        if (result != -EAGAIN || (flags & MSG_DONTWAIT) != 0) {
            if (result < 0) {
                return (ssize_t)posix_result(result);
            }
            /* Kernel melaporkan panjang datagram asli; normalnya kembalikan
               jumlah byte yang benar-benar disalin, kecuali MSG_TRUNC. */
            size_t copied = (size_t)result > length ? length : (size_t)result;
            if (source != NULL) {
                ipv4.sin_family = AF_INET;
                ipv4.sin_port = htons(source_port);
                ipv4.sin_addr.s_addr =
                    htonl(((uint32_t)source_ip[0] << 24) |
                          ((uint32_t)source_ip[1] << 16) |
                          ((uint32_t)source_ip[2] << 8) |
                          (uint32_t)source_ip[3]);
                size_t copy_length = sizeof(ipv4);
                if (source_length != NULL && *source_length < copy_length) {
                    copy_length = *source_length;
                }
                memcpy(source, &ipv4, copy_length);
                if (source_length != NULL) {
                    *source_length = (socklen_t)copy_length;
                }
            }
            if ((flags & MSG_TRUNC) != 0) {
                return (ssize_t)result;
            }
            return (ssize_t)copied;
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (ssize_t)posix_result(yield_result);
        }
    }
}

#define OCT_MAX_MESSAGE_IOVECS 1024

static int oct_validate_message(const struct msghdr *message)
{
    if (message == NULL) {
        errno = EFAULT;
        return -1;
    }
    if (message->msg_iov == NULL && message->msg_iovlen != 0) {
        errno = EFAULT;
        return -1;
    }
    if (message->msg_iovlen > OCT_MAX_MESSAGE_IOVECS) {
        errno = EMSGSIZE;
        return -1;
    }
    return 0;
}

static int oct_message_gather(const struct msghdr *message, size_t *length,
                              char **buffer)
{
    *length = 0;
    *buffer = NULL;
    for (size_t index = 0; index < message->msg_iovlen; ++index) {
        const struct iovec *vector = &message->msg_iov[index];
        if (vector->iov_base == NULL && vector->iov_len != 0) {
            errno = EFAULT;
            return -1;
        }
        if (vector->iov_len > SIZE_MAX - *length) {
            errno = EMSGSIZE;
            return -1;
        }
        *length += vector->iov_len;
    }
    if (*length == 0) {
        return 0;
    }

    char *gathered = malloc(*length);
    if (gathered == NULL) {
        errno = ENOMEM;
        return -1;
    }
    size_t offset = 0;
    for (size_t index = 0; index < message->msg_iovlen; ++index) {
        const struct iovec *vector = &message->msg_iov[index];
        if (vector->iov_len != 0) {
            memcpy(gathered + offset, vector->iov_base, vector->iov_len);
            offset += vector->iov_len;
        }
    }
    *buffer = gathered;
    return 0;
}

static ssize_t oct_message_write(int socket_fd, const char *data,
                                 size_t length, int flags, int use_send)
{
    size_t written = 0;
    while (written < length) {
        ssize_t result = use_send
            ? send(socket_fd, data + written, length - written, flags)
            : write(socket_fd, data + written, length - written);
        if (result < 0) {
            return written == 0 ? -1 : (ssize_t)written;
        }
        if (result == 0) {
            break;
        }
        written += (size_t)result;
    }
    return (ssize_t)written;
}

ssize_t sendmsg(int socket_fd, const struct msghdr *message, int flags)
{
    if (oct_validate_message(message) != 0) {
        return -1;
    }
    /* SCM_RIGHTS/SCM_CREDENTIALS belum didukung; pemanggil harus tahu. */
    if (message->msg_control != NULL || message->msg_controllen != 0) {
        errno = ENOTSUP;
        return -1;
    }

    size_t length = 0;
    char *gathered = NULL;
    if (oct_message_gather(message, &length, &gathered) != 0) {
        return -1;
    }
    const char *data = gathered != NULL ? gathered : "";

    ssize_t result;
    if (message->msg_name != NULL) {
        if (length > OCT_MAX_SYSCALL_IO) {
            free(gathered);
            errno = EMSGSIZE;
            return -1;
        }
        result = sendto(socket_fd, data, length, flags, message->msg_name,
                        message->msg_namelen);
    } else {
        int use_send = 0;
        int type = 0;
        socklen_t length_size = sizeof(type);
        if (getsockopt(socket_fd, SOL_SOCKET, SO_TYPE, &type,
                       &length_size) == 0) {
            /* Datagram tanpa msg_name dikirim ke peer hasil connect(). */
            use_send = 1;
        } else if (errno != ENOTSOCK) {
            free(gathered);
            return -1;
        }
        result = oct_message_write(socket_fd, data, length, flags, use_send);
    }
    free(gathered);
    return result;
}

static int oct_message_capacity(const struct msghdr *message, size_t *capacity)
{
    size_t total = 0;
    for (size_t index = 0; index < message->msg_iovlen; ++index) {
        const struct iovec *vector = &message->msg_iov[index];
        if (vector->iov_base == NULL && vector->iov_len != 0) {
            errno = EFAULT;
            return -1;
        }
        if (vector->iov_len > SIZE_MAX - total) {
            errno = EMSGSIZE;
            return -1;
        }
        total += vector->iov_len;
    }
    *capacity = total > OCT_MAX_SYSCALL_IO ? OCT_MAX_SYSCALL_IO : total;
    return 0;
}

ssize_t recvmsg(int socket_fd, struct msghdr *message, int flags)
{
    if (oct_validate_message(message) != 0) {
        return -1;
    }
    size_t capacity = 0;
    if (oct_message_capacity(message, &capacity) != 0) {
        return -1;
    }

    char scratch[OCT_MAX_SYSCALL_IO];
    ssize_t result;
    if (message->msg_name != NULL) {
        socklen_t address_length = message->msg_namelen;
        result = recvfrom(socket_fd, scratch, capacity, flags | MSG_TRUNC,
                          message->msg_name, &address_length);
        if (result >= 0) {
            message->msg_namelen = address_length;
        }
    } else {
        int type = 0;
        socklen_t length_size = sizeof(type);
        if (getsockopt(socket_fd, SOL_SOCKET, SO_TYPE, &type,
                       &length_size) == 0) {
            result = type == SOCK_DGRAM
                ? recvfrom(socket_fd, scratch, capacity, flags | MSG_TRUNC, NULL, NULL)
                : recv(socket_fd, scratch, capacity, flags);
        } else if (errno == ENOTSOCK) {
            result = read(socket_fd, scratch, capacity);
        } else {
            return -1;
        }
    }
    if (result < 0) {
        return -1;
    }

    /* Ancillary data belum didukung, jadi buffer kontrol tidak pernah diisi
       dan MSG_CTRUNC tidak pernah dilaporkan. */
    size_t full_length = (size_t)result;
    int truncated = full_length > capacity;
    message->msg_controllen = 0;
    message->msg_flags = truncated ? MSG_TRUNC : 0;

    size_t remaining = truncated ? capacity : full_length;
    size_t offset = 0;
    for (size_t index = 0; index < message->msg_iovlen && remaining != 0;
         ++index) {
        struct iovec *vector = &message->msg_iov[index];
        size_t chunk = vector->iov_len < remaining ? vector->iov_len : remaining;
        if (chunk != 0) {
            memcpy(vector->iov_base, scratch + offset, chunk);
            offset += chunk;
            remaining -= chunk;
        }
    }
    if ((flags & MSG_TRUNC) != 0) {
        return (ssize_t)full_length;
    }
    return (ssize_t)(truncated ? capacity : full_length);
}

DIR *opendir(const char *path)
{
    int fd = (int)posix_result(oct_opendir(path));
    if (fd < 0) {
        return NULL;
    }
    DIR *directory = malloc(sizeof(*directory));
    if (directory == NULL) {
        close(fd);
        errno = ENOMEM;
        return NULL;
    }
    directory->fd = fd;
    return directory;
}

/*
 * POSIX fdopendir() takes ownership of fd only after successful conversion.
 * Keep fd open on validation/allocation failure so callers can recover it.
 */
DIR *fdopendir(int fd)
{
    if (fd < 0) {
        errno = EBADF;
        return NULL;
    }

    struct stat metadata;
    if (fstat(fd, &metadata) < 0) {
        return NULL;
    }
    if (!S_ISDIR(metadata.st_mode)) {
        errno = ENOTDIR;
        return NULL;
    }

    DIR *directory = malloc(sizeof(*directory));
    if (directory == NULL) {
        errno = ENOMEM;
        return NULL;
    }

    directory->fd = fd;
    return directory;
}

struct dirent *readdir(DIR *directory)
{
    if (directory == NULL) {
        errno = EBADF;
        return NULL;
    }
    int64_t result = oct_readdir(directory->fd, &directory->entry);
    if (result < 0) {
        posix_result(result);
        return NULL;
    }
    return result == 0 ? NULL : &directory->entry;
}

int closedir(DIR *directory)
{
    if (directory == NULL) {
        errno = EBADF;
        return -1;
    }
    if (close(directory->fd) < 0) {
        return -1;
    }
    free(directory);
    return 0;
}

int dirfd(DIR *directory)
{
    if (directory == NULL) {
        errno = EBADF;
        return -1;
    }
    return directory->fd;
}

static int oct_descriptor_is_nonblocking(int fd)
{
    int64_t flags = oct_fcntl(fd, F_GETFL, 0);
    return flags >= 0 && (flags & OCT_O_NONBLOCK) != 0;
}

ssize_t read(int fd, void *buffer, size_t count)
{
    if (count > OCT_MAX_SYSCALL_IO) {
        count = OCT_MAX_SYSCALL_IO;
    }
    for (;;) {
        int64_t result = oct_read(fd, buffer, count);
        if (result != -EAGAIN) {
            return (ssize_t)posix_result(result);
        }
        if (oct_descriptor_is_nonblocking(fd)) {
            return (ssize_t)posix_result(result);
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (ssize_t)posix_result(yield_result);
        }
    }
}

ssize_t write(int fd, const void *buffer, size_t count)
{
    if (count > OCT_MAX_SYSCALL_IO) {
        count = OCT_MAX_SYSCALL_IO;
    }
    for (;;) {
        int64_t result = oct_write(fd, buffer, count);
        if (result != -EAGAIN) {
            return (ssize_t)posix_result(result);
        }
        if (oct_descriptor_is_nonblocking(fd)) {
            return (ssize_t)posix_result(result);
        }
        int64_t yield_result = oct_yield();
        if (yield_result < 0) {
            return (ssize_t)posix_result(yield_result);
        }
    }
}

off_t lseek(int fd, off_t offset, int whence)
{
    return (off_t)posix_result(oct_lseek(fd, offset, whence));
}

int unlink(const char *path)
{
    return (int)posix_result(oct_unlink(path));
}

int unlinkat(int dirfd, const char *path, int flags)
{
    return (int)posix_result(oct_unlinkat(dirfd, path, flags));
}

int link(const char *oldpath, const char *newpath)
{
    return linkat(OCT_AT_FDCWD, oldpath, OCT_AT_FDCWD, newpath, 0);
}

int linkat(int olddirfd, const char *oldpath, int newdirfd,
           const char *newpath, int flags)
{
    if (flags != 0) {
        errno = EINVAL;
        return -1;
    }
    return (int)posix_result(oct_linkat(olddirfd, oldpath, newdirfd,
                                        newpath, flags));
}

int rmdir(const char *path)
{
    return (int)posix_result(oct_rmdir(path));
}

int symlink(const char *target, const char *link_path)
{
    return (int)posix_result(oct_symlink(target, link_path));
}

int symlinkat(const char *target, int dirfd, const char *link_path)
{
    return (int)posix_result(oct_symlinkat(target, dirfd, link_path));
}

ssize_t readlink(const char *path, char *buffer, size_t size)
{
    return (ssize_t)posix_result(oct_readlink(path, buffer, size));
}

ssize_t readlinkat(int dirfd, const char *path, char *buffer, size_t size)
{
    return (ssize_t)posix_result(oct_readlinkat(dirfd, path, buffer, size));
}

int mkdir(const char *path, mode_t mode)
{
    return (int)posix_result(oct_mkdir_mode(path, mode));
}

int mkdirat(int dirfd, const char *path, mode_t mode)
{
    return (int)posix_result(oct_mkdirat(dirfd, path, mode));
}

int stat(const char *path, struct stat *result)
{
    if (result == NULL) {
        errno = EFAULT;
        return -1;
    }
    struct oct_stat metadata;
    if (posix_result(oct_stat_path(path, &metadata)) < 0) {
        return -1;
    }
    if (metadata.size > INT64_MAX) {
        errno = EOVERFLOW;
        return -1;
    }
    memset(result, 0, sizeof(*result));
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
    result->st_mtime = (time_t)metadata.modified_seconds;
    return 0;
}

int lstat(const char *path, struct stat *result)
{
    if (result == NULL) {
        errno = EFAULT;
        return -1;
    }
    struct oct_stat metadata;
    if (posix_result(oct_lstat_path(path, &metadata)) < 0) {
        return -1;
    }
    if (metadata.size > INT64_MAX) {
        errno = EOVERFLOW;
        return -1;
    }
    memset(result, 0, sizeof(*result));
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
    result->st_mtime = (time_t)metadata.modified_seconds;
    return 0;
}

int fstat(int fd, struct stat *result)
{
    if (result == NULL) {
        errno = EFAULT;
        return -1;
    }
    struct oct_stat metadata;
    if (posix_result(oct_fstat(fd, &metadata)) < 0) {
        return -1;
    }
    if (metadata.size > INT64_MAX) {
        errno = EOVERFLOW;
        return -1;
    }
    memset(result, 0, sizeof(*result));
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
    result->st_mtime = (time_t)metadata.modified_seconds;
    return 0;
}

int fstatat(int dirfd, const char *path, struct stat *result, int flags)
{
    if (result == NULL) {
        errno = EFAULT;
        return -1;
    }
    struct oct_stat metadata;
    if (posix_result(oct_fstatat(dirfd, path, &metadata, flags)) < 0) {
        return -1;
    }
    if (metadata.size > INT64_MAX) {
        errno = EOVERFLOW;
        return -1;
    }
    memset(result, 0, sizeof(*result));
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
    result->st_mtime = (time_t)metadata.modified_seconds;
    return 0;
}

int access(const char *path, int mode)
{
    if (mode & ~(R_OK | W_OK | X_OK)) {
        errno = EINVAL;
        return -1;
    }
    return (int)posix_result(oct_access(path, (uint64_t)(unsigned int)mode));
}

int rename(const char *source, const char *target)
{
    return (int)posix_result(oct_rename(source, target));
}

int renameat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath)
{
    return (int)posix_result(oct_renameat(olddirfd, oldpath, newdirfd, newpath));
}

mode_t umask(mode_t mask)
{
    return (mode_t)posix_result(oct_umask(mask));
}

int chmod(const char *path, mode_t mode)
{
    return (int)posix_result(oct_chmod(path, mode));
}

int fchmod(int fd, mode_t mode)
{
    return (int)posix_result(oct_fchmod(fd, mode));
}

int chdir(const char *path)
{
    return (int)posix_result(oct_chdir(path));
}

char *getcwd(char *buffer, size_t size)
{
    return posix_result(oct_getcwd(buffer, size)) < 0 ? NULL : buffer;
}

pid_t getpid(void)
{
    return (pid_t)posix_result(oct_getpid());
}

long syscall(long number, ...)
{
    if (number == 186) {
        return (long)getpid();
    }
    if (number == 218) {
        va_list arguments;
        va_start(arguments, number);
        (void)va_arg(arguments, void *);
        va_end(arguments);
        return (long)getpid();
    }
    (void)number;
    errno = ENOSYS;
    return -1;
}

int isatty(int fd)
{
    struct termios termios_state;
    if (tcgetattr(fd, &termios_state) == 0) {
        return 1;
    }
    errno = ENOTTY;
    return 0;
}

int pipe(int descriptors[2])
{
    return (int)posix_result(oct_pipe(descriptors));
}

int pipe2(int descriptors[2], int flags)
{
    return (int)posix_result(oct_pipe2(descriptors, flags));
}

int dup2(int source_fd, int target_fd)
{
    return (int)posix_result(oct_dup2(source_fd, target_fd));
}

int dup(int fd)
{
    return (int)posix_result(oct_dup(fd));
}

int fcntl(int fd, int command, ...)
{
    uint64_t argument = 0;
    if (command == F_DUPFD || command == F_DUPFD_CLOEXEC ||
        command == 1030 || command == F_SETFD || command == F_SETFL) {
        va_list arguments;
        va_start(arguments, command);
        argument = (uint64_t)(unsigned int)va_arg(arguments, int);
        va_end(arguments);
    }
    return (int)posix_result(oct_fcntl(fd, command, argument));
}

int flock(int fd, int operation)
{
    return (int)posix_result(oct_flock(fd, operation));
}

int ftruncate(int fd, off_t length)
{
    if (length < 0) {
        errno = EINVAL;
        return -1;
    }
    return (int)posix_result(oct_ftruncate(fd, (size_t)length));
}

int ioctl(int fd, unsigned long request, ...)
{
    va_list args;
    va_start(args, request);
    void *argp = va_arg(args, void *);
    va_end(args);

    return (int)posix_result(oct_ioctl(fd, (uint64_t)request, argp));
}

static int count_spawn_arguments(char *const arguments[], size_t *count)
{
    if (arguments == NULL) {
        return EINVAL;
    }
    size_t argument_count = 0;
    while (argument_count < 64 && arguments[argument_count] != NULL) {
        ++argument_count;
    }
    if (argument_count == 0) {
        return EINVAL;
    }
    if (argument_count == 64 && arguments[argument_count] != NULL) {
        return E2BIG;
    }
    *count = argument_count;
    return 0;
}

#define OCTAGRAM_SPAWN_SUPPORTED_FLAGS                                          \
    (POSIX_SPAWN_RESETIDS | POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_SETSIGDEF       \
     | POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_SETSID                              \
     | POSIX_SPAWN_SETSCHEDPARAM | POSIX_SPAWN_SETSCHEDULER)

/* Runs in the forked child before execve; failures are reported to the parent. */
static int apply_spawn_file_actions(
    const posix_spawn_file_actions_t *file_actions)
{
    if (file_actions == NULL) {
        return 0;
    }
    if (file_actions->count < 0 ||
        (size_t)file_actions->count > file_actions->capacity ||
        (file_actions->count != 0 && file_actions->actions == NULL)) {
        return EINVAL;
    }
    for (int index = 0; index < file_actions->count; ++index) {
        const struct octagram_spawn_file_action *action =
            &file_actions->actions[index];
        switch (action->kind) {
        case OCTAGRAM_SPAWN_ACTION_CLOSE:
            if (close(action->fd) != 0) {
                return errno;
            }
            break;
        case OCTAGRAM_SPAWN_ACTION_DUP2:
            if (dup2(action->fd, action->target_fd) < 0) {
                return errno;
            }
            break;
        case OCTAGRAM_SPAWN_ACTION_OPEN: {
            int opened = open(action->path, action->flags, action->mode);
            if (opened < 0) {
                return errno;
            }
            if (opened != action->target_fd) {
                if (dup2(opened, action->target_fd) < 0) {
                    int error = errno;
                    close(opened);
                    return error;
                }
                close(opened);
            }
            break;
        }
        case OCTAGRAM_SPAWN_ACTION_CHDIR:
            if (chdir(action->path) != 0) return errno;
            break;
        default:
            return EINVAL;
        }
    }
    return 0;
}

/* Runs in the forked child before execve; failures are reported to the parent. */
static int apply_spawn_attributes(const posix_spawnattr_t *attributes)
{
    if (attributes == NULL) {
        return 0;
    }
    if ((attributes->flags & (unsigned short)~OCTAGRAM_SPAWN_SUPPORTED_FLAGS) !=
        0) {
        return ENOTSUP;
    }
    if ((attributes->flags & POSIX_SPAWN_SETPGROUP) != 0 &&
        (attributes->flags & POSIX_SPAWN_SETSID) != 0) return EINVAL;
    if ((attributes->flags & POSIX_SPAWN_RESETIDS) != 0) {
        /* Turunkan group lebih dulu, lalu uid efektif ke id nyata. */
        if (setgid(getgid()) != 0) {
            return errno;
        }
        if (setuid(getuid()) != 0) {
            return errno;
        }
    }
    if ((attributes->flags & POSIX_SPAWN_SETSID) != 0 && setsid() < 0) return errno;
    if ((attributes->flags & POSIX_SPAWN_SETPGROUP) != 0) {
        if (setpgid(0, attributes->pgroup) != 0) {
            return errno;
        }
    }
    if ((attributes->flags & POSIX_SPAWN_SETSIGMASK) != 0) {
        if (sigprocmask(SIG_SETMASK, &attributes->sigmask, NULL) != 0) {
            return errno;
        }
    }
    if ((attributes->flags & POSIX_SPAWN_SETSIGDEF) != 0) {
        struct sigaction default_action;
        memset(&default_action, 0, sizeof(default_action));
        default_action.sa_handler = SIG_DFL;
        for (int signal_number = 1; signal_number < NSIG; ++signal_number) {
            if (signal_number == SIGKILL || signal_number == SIGSTOP) {
                continue;
            }
            if (sigismember(&attributes->sigdefault, signal_number) == 1 &&
                sigaction(signal_number, &default_action, NULL) != 0) {
                return errno;
            }
        }
    }
    if ((attributes->flags & POSIX_SPAWN_SETSCHEDULER) != 0) {
        struct sched_param param = attributes->schedparam;
        if (sched_setscheduler(0, attributes->scheduler, &param) != 0) {
            return errno;
        }
    }
    if ((attributes->flags & POSIX_SPAWN_SETSCHEDPARAM) != 0) {
        struct sched_param param = attributes->schedparam;
        if (sched_setparam(0, &param) != 0) {
            return errno;
        }
    }
    return 0;
}

static int posix_spawn_impl(pid_t *pid, const char *path,
                            const posix_spawn_file_actions_t *file_actions,
                            const posix_spawnattr_t *attributes,
                            char *const arguments[], char *const environment[])
{
    if (pid == NULL || path == NULL || path[0] == '\0') {
        return EINVAL;
    }
    if (file_actions != NULL &&
        (file_actions->count < 0 ||
         (size_t)file_actions->count > file_actions->capacity ||
         (file_actions->count != 0 && file_actions->actions == NULL))) {
        return EINVAL;
    }
    if (attributes != NULL &&
        (attributes->flags & (unsigned short)~OCTAGRAM_SPAWN_SUPPORTED_FLAGS) !=
            0) {
        return ENOTSUP;
    }
    size_t argument_count;
    int error = count_spawn_arguments(arguments, &argument_count);
    if (error != 0) {
        return error;
    }
    static char *empty_environment[] = {NULL};
    if (environment == NULL) {
        environment = environ != NULL ? environ : empty_environment;
    }

    int descriptors[2];
    if (pipe(descriptors) < 0) {
        return errno;
    }
    if (fcntl(descriptors[0], F_SETFD, FD_CLOEXEC) < 0 ||
        fcntl(descriptors[1], F_SETFD, FD_CLOEXEC) < 0) {
        int error = errno;
        close(descriptors[0]);
        close(descriptors[1]);
        return error;
    }

    pid_t child = fork();
    if (child < 0) {
        int error = errno;
        close(descriptors[0]);
        close(descriptors[1]);
        return error;
    }
    if (child == 0) {
        close(descriptors[0]);
        int error = apply_spawn_file_actions(file_actions);
        if (error == 0) {
            error = apply_spawn_attributes(attributes);
        }
        if (error == 0) {
            execve(path, arguments, environment);
            error = errno;
        }
        (void)write(descriptors[1], &error, sizeof(error));
        close(descriptors[1]);
        exit(127);
    }

    close(descriptors[1]);
    int child_error = 0;
    size_t received = 0;
    while (received < sizeof(child_error)) {
        ssize_t count = read(descriptors[0],
                             (unsigned char *)&child_error + received,
                             sizeof(child_error) - received);
        if (count < 0) {
            int error = errno;
            close(descriptors[0]);
            (void)waitpid(child, NULL, 0);
            return error;
        }
        if (count == 0) {
            break;
        }
        received += (size_t)count;
    }
    close(descriptors[0]);
    if (received != 0) {
        (void)waitpid(child, NULL, 0);
        return received == sizeof(child_error) ? child_error : EIO;
    }

    *pid = child;
    return 0;
}

int posix_spawn(pid_t *restrict pid, const char *restrict path,
                const posix_spawn_file_actions_t *restrict file_actions,
                const posix_spawnattr_t *restrict attributes,
                char *const arguments[restrict],
                char *const environment[restrict])
{
    return posix_spawn_impl(pid, path, file_actions, attributes, arguments,
                            environment);
}

int posix_spawnp(pid_t *restrict pid, const char *restrict file,
                 const posix_spawn_file_actions_t *restrict file_actions,
                 const posix_spawnattr_t *restrict attributes,
                 char *const arguments[restrict],
                 char *const environment[restrict])
{
    if (file == NULL || file[0] == '\0') {
        return EINVAL;
    }
    for (const char *cursor = file; *cursor != '\0'; ++cursor) {
        if (*cursor == '/') {
            return posix_spawn_impl(pid, file, file_actions, attributes,
                                    arguments, environment);
        }
    }

    const char *path = getenv("PATH");
    if (path == NULL) {
        path = "/bin:/usr/bin:/usr/local/bin";
    }
    size_t file_length = strlen(file);
    const char *component = path;
    int access_error = 0;
    int last_error = ENOENT;
    for (;;) {
        const char *separator = component;
        while (*separator != '\0' && *separator != ':') {
            ++separator;
        }
        size_t directory_length = (size_t)(separator - component);
        int add_separator = directory_length != 0 &&
                            component[directory_length - 1] != '/';
        size_t candidate_length = directory_length +
                                  (size_t)add_separator + file_length;
        if (candidate_length >= 256) {
            last_error = ENAMETOOLONG;
        } else {
            char candidate[256];
            size_t cursor = 0;
            if (directory_length != 0) {
                memcpy(candidate, component, directory_length);
                cursor = directory_length;
                if (add_separator) {
                    candidate[cursor++] = '/';
                }
            }
            memcpy(candidate + cursor, file, file_length);
            candidate[cursor + file_length] = '\0';
            int error = posix_spawn_impl(pid, candidate, file_actions,
                                         attributes, arguments, environment);
            if (error == 0) {
                return 0;
            }
            if (error == EACCES) {
                access_error = error;
            } else if (error != ENOENT && error != ENOTDIR) {
                return error;
            } else {
                last_error = error;
            }
        }
        if (*separator == '\0') {
            break;
        }
        component = separator + 1;
    }
    return access_error != 0 ? access_error : last_error;
}

static struct octagram_spawn_file_action *spawn_file_action_append(
    posix_spawn_file_actions_t *actions)
{
    if (actions == NULL || actions->count < 0 ||
        (size_t)actions->count > actions->capacity) {
        return NULL;
    }
    if ((size_t)actions->count == actions->capacity) {
        size_t next_capacity = actions->capacity == 0
            ? OCTAGRAM_SPAWN_INITIAL_FILE_ACTION_CAPACITY
            : actions->capacity * 2;
        if (next_capacity < actions->capacity ||
            next_capacity > SIZE_MAX / sizeof(*actions->actions)) {
            return NULL;
        }
        void *next = realloc(actions->actions,
                             next_capacity * sizeof(*actions->actions));
        if (next == NULL) {
            return NULL;
        }
        actions->actions = next;
        actions->capacity = next_capacity;
    }
    struct octagram_spawn_file_action *action =
        &actions->actions[actions->count++];
    memset(action, 0, sizeof(*action));
    return action;
}

int posix_spawn_file_actions_init(posix_spawn_file_actions_t *actions)
{
    if (actions == NULL) {
        return EINVAL;
    }
    actions->count = 0;
    actions->capacity = 0;
    actions->actions = NULL;
    return 0;
}

int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *actions)
{
    if (actions == NULL) {
        return EINVAL;
    }
    if (actions->count < 0 ||
        (size_t)actions->count > actions->capacity ||
        (actions->count != 0 && actions->actions == NULL)) {
        return EINVAL;
    }
    for (int index = 0; index < actions->count; ++index) {
        free(actions->actions[index].path);
        actions->actions[index].path = NULL;
    }
    free(actions->actions);
    actions->actions = NULL;
    actions->count = 0;
    actions->capacity = 0;
    return 0;
}

int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *actions,
                                      int fd)
{
    if (actions == NULL || fd < 0) {
        return EINVAL;
    }
    struct octagram_spawn_file_action *action =
        spawn_file_action_append(actions);
    if (action == NULL) {
        return ENOMEM;
    }
    action->kind = OCTAGRAM_SPAWN_ACTION_CLOSE;
    action->fd = fd;
    return 0;
}

int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *actions,
                                     int source_fd, int target_fd)
{
    if (actions == NULL || source_fd < 0 || target_fd < 0) {
        return EINVAL;
    }
    struct octagram_spawn_file_action *action =
        spawn_file_action_append(actions);
    if (action == NULL) {
        return ENOMEM;
    }
    action->kind = OCTAGRAM_SPAWN_ACTION_DUP2;
    action->fd = source_fd;
    action->target_fd = target_fd;
    return 0;
}

int posix_spawn_file_actions_addopen(posix_spawn_file_actions_t *actions,
                                     int fd, const char *path, int flags,
                                     mode_t mode)
{
    if (actions == NULL || path == NULL || path[0] == '\0' || fd < 0) {
        return EINVAL;
    }
    size_t length = strlen(path);
    if (length >= OCTAGRAM_SPAWN_MAX_PATH) {
        return ENAMETOOLONG;
    }
    char *path_copy = malloc(length + 1);
    if (path_copy == NULL) {
        return ENOMEM;
    }
    memcpy(path_copy, path, length + 1);
    struct octagram_spawn_file_action *action =
        spawn_file_action_append(actions);
    if (action == NULL) {
        free(path_copy);
        return ENOMEM;
    }
    action->kind = OCTAGRAM_SPAWN_ACTION_OPEN;
    action->fd = fd;
    action->target_fd = fd;
    action->flags = flags;
    action->mode = mode;
    action->path = path_copy;
    return 0;
}

int posix_spawn_file_actions_addchdir(posix_spawn_file_actions_t *actions,
                                      const char *path)
{
    if (actions == NULL || path == NULL || path[0] == '\0') return EINVAL;
    size_t length = strlen(path);
    if (length >= OCTAGRAM_SPAWN_MAX_PATH) return ENAMETOOLONG;
    char *copy = malloc(length + 1);
    if (copy == NULL) return ENOMEM;
    memcpy(copy, path, length + 1);
    struct octagram_spawn_file_action *action = spawn_file_action_append(actions);
    if (action == NULL) { free(copy); return ENOMEM; }
    action->kind = OCTAGRAM_SPAWN_ACTION_CHDIR;
    action->path = copy;
    return 0;
}

int posix_spawn_file_actions_addchdir_np(posix_spawn_file_actions_t *actions,
                                         const char *path)
{
    return posix_spawn_file_actions_addchdir(actions, path);
}

int posix_spawnattr_init(posix_spawnattr_t *attributes)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    memset(attributes, 0, sizeof(*attributes));
    attributes->scheduler = SCHED_OTHER;
    attributes->schedparam.sched_priority = 0;
    return 0;
}

int posix_spawnattr_destroy(posix_spawnattr_t *attributes)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    memset(attributes, 0, sizeof(*attributes));
    return 0;
}

int posix_spawnattr_setflags(posix_spawnattr_t *attributes, short flags)
{
    if (attributes == NULL) {
        return EINVAL;
    }
    attributes->flags = (unsigned short)flags;
    return 0;
}

int posix_spawnattr_getflags(const posix_spawnattr_t *attributes,
                             short *flags)
{
    if (attributes == NULL || flags == NULL) {
        return EINVAL;
    }
    *flags = (short)attributes->flags;
    return 0;
}

int posix_spawnattr_setpgroup(posix_spawnattr_t *attributes, pid_t group)
{
    if (attributes == NULL || group < 0) {
        return EINVAL;
    }
    attributes->pgroup = group;
    return 0;
}

int posix_spawnattr_getpgroup(const posix_spawnattr_t *attributes,
                              pid_t *group)
{
    if (attributes == NULL || group == NULL) {
        return EINVAL;
    }
    *group = attributes->pgroup;
    return 0;
}

int posix_spawnattr_setsigdefault(posix_spawnattr_t *attributes,
                                  const sigset_t *mask)
{
    if (attributes == NULL || mask == NULL) {
        return EINVAL;
    }
    attributes->sigdefault = *mask;
    return 0;
}

int posix_spawnattr_getsigdefault(const posix_spawnattr_t *attributes,
                                  sigset_t *mask)
{
    if (attributes == NULL || mask == NULL) {
        return EINVAL;
    }
    *mask = attributes->sigdefault;
    return 0;
}

int posix_spawnattr_setsigmask(posix_spawnattr_t *attributes,
                               const sigset_t *mask)
{
    if (attributes == NULL || mask == NULL) {
        return EINVAL;
    }
    attributes->sigmask = *mask;
    return 0;
}

int posix_spawnattr_getsigmask(const posix_spawnattr_t *attributes,
                               sigset_t *mask)
{
    if (attributes == NULL || mask == NULL) {
        return EINVAL;
    }
    *mask = attributes->sigmask;
    return 0;
}

int posix_spawnattr_setschedparam(posix_spawnattr_t *attributes,
                                  const struct sched_param *param)
{
    if (attributes == NULL || param == NULL) {
        return EINVAL;
    }
    attributes->schedparam = *param;
    return 0;
}

int posix_spawnattr_getschedparam(const posix_spawnattr_t *attributes,
                                  struct sched_param *param)
{
    if (attributes == NULL || param == NULL) {
        return EINVAL;
    }
    *param = attributes->schedparam;
    return 0;
}

int posix_spawnattr_setscheduler(posix_spawnattr_t *attributes, int policy)
{
    if (attributes == NULL || policy < 0) {
        return EINVAL;
    }
    attributes->scheduler = policy;
    return 0;
}

int posix_spawnattr_getscheduler(const posix_spawnattr_t *attributes,
                                 int *policy)
{
    if (attributes == NULL || policy == NULL) {
        return EINVAL;
    }
    *policy = attributes->scheduler;
    return 0;
}

pid_t wait3(int *status, int options, struct rusage *usage)
{
    if (usage != NULL) {
        memset(usage, 0, sizeof(*usage));
    }
    return waitpid(-1, status, options);
}

pid_t wait4(pid_t pid, int *status, int options, struct rusage *usage)
{
    if (usage != NULL) {
        memset(usage, 0, sizeof(*usage));
    }
    return waitpid(pid, status, options);
}

pid_t waitpid(pid_t pid, int *status, int options)
{
    if (options < 0 || (options & ~WNOHANG) != 0 ||
        (pid <= 0 && pid != -1)) {
        errno = EINVAL;
        return -1;
    }
    uint64_t raw_status = 0;
    uint64_t target = pid == -1 ? OCT_WAIT_ANY : (uint64_t)pid;
    int64_t result = oct_waitpid_options(target,
                                         status != NULL ? &raw_status : NULL,
                                         (uint64_t)options);
    if (result < 0) {
        errno = (int)-result;
        return -1;
    }
    if (status != NULL && result > 0) {
        *status = (int)raw_status;
    }
    return (pid_t)result;
}

pid_t wait(int *status)
{
    return waitpid(-1, status, 0);
}

size_t confstr(int name, char *buffer, size_t size)
{
    static const char posix_version[] = "202405L";
    (void)name;
    if (buffer == NULL) {
        return sizeof(posix_version);
    }
    if (size == 0) {
        return 0;
    }
    size_t length = sizeof(posix_version) - 1;
    size_t copied = length < size ? length : size;
    memcpy(buffer, posix_version, copied);
    if (copied < size) {
        buffer[copied] = '\0';
    }
    return length + 1;
}

char *getlogin(void)
{
    static char login[] = "octagram";
    return login;
}

int getloadavg(double load[], int count)
{
    if (load == NULL || count < 0 || count > 3) {
        errno = count < 0 ? EINVAL : EFAULT;
        return -1;
    }
    for (int index = 0; index < count; ++index) {
        load[index] = 0.0;
    }
    return count;
}

int lockf(int fd, int operation, off_t length)
{
    (void)fd;
    (void)operation;
    (void)length;
    errno = ENOSYS;
    return -1;
}

int mkfifo(const char *path, mode_t mode)
{
    (void)mode;
    errno = path == NULL ? EFAULT : ENOSYS;
    return -1;
}

int mkfifoat(int dirfd, const char *path, mode_t mode)
{
    (void)dirfd;
    (void)mode;
    errno = path == NULL ? EFAULT : ENOSYS;
    return -1;
}

int mknod(const char *path, mode_t mode, dev_t device)
{
    (void)mode;
    (void)device;
    errno = path == NULL ? EFAULT : ENOSYS;
    return -1;
}

int mknodat(int dirfd, const char *path, mode_t mode, dev_t device)
{
    (void)dirfd;
    (void)mode;
    (void)device;
    errno = path == NULL ? EFAULT : ENOSYS;
    return -1;
}

int lchmod(const char *path, mode_t mode)
{
    return chmod(path, mode);
}

int execv(const char *path, char *const argv[])
{
    return execve(path, argv, environ);
}

int sethostname(const char *name, size_t size)
{
    (void)name;
    (void)size;
    errno = EPERM;
    return -1;
}

static int stream_is_open(FILE *stream)
{
    return stream != NULL && (stream->flags & STREAM_CLOSED) == 0;
}

static int stream_failure(FILE *stream, int error)
{
    if (stream != NULL) {
        stream->flags |= STREAM_ERROR;
    }
    errno = error;
    return EOF;
}

static int parse_file_mode(const char *mode, int *open_flags,
                           unsigned int *stream_flags)
{
    if (mode == NULL || mode[0] == '\0') {
        errno = EINVAL;
        return -1;
    }

    int flags;
    unsigned int access;
    switch (mode[0]) {
    case 'r':
        flags = O_RDONLY;
        access = STREAM_READABLE;
        break;
    case 'w':
        flags = O_WRONLY | O_CREAT | O_TRUNC;
        access = STREAM_WRITABLE;
        break;
    case 'a':
        flags = O_WRONLY | O_CREAT | O_APPEND;
        access = STREAM_WRITABLE;
        break;
    default:
        errno = EINVAL;
        return -1;
    }

    int plus = 0;
    for (const char *cursor = mode + 1; *cursor != '\0'; ++cursor) {
        if (*cursor == '+' && !plus) {
            plus = 1;
        } else if (*cursor != 'b') {
            errno = EINVAL;
            return -1;
        }
    }
    if (plus) {
        flags = (flags & ~O_ACCMODE) | O_RDWR;
        access = STREAM_READABLE | STREAM_WRITABLE;
    }
    *open_flags = flags;
    *stream_flags = access;
    return 0;
}

FILE *fdopen(int fd, const char *mode)
{
    int open_flags;
    unsigned int stream_flags;
    if (fd < 0 || parse_file_mode(mode, &open_flags, &stream_flags) < 0) {
        if (fd < 0) {
            errno = EBADF;
        }
        return NULL;
    }
    (void)open_flags;
    FILE *stream = malloc(sizeof(*stream));
    if (stream == NULL) {
        errno = ENOMEM;
        return NULL;
    }
    stream->fd = fd;
    stream->flags = stream_flags;
    stream->pushed_character = 0;
    return stream;
}

FILE *fopen(const char *path, const char *mode)
{
    if (path == NULL) {
        errno = EINVAL;
        return NULL;
    }
    int flags;
    unsigned int stream_flags;
    if (parse_file_mode(mode, &flags, &stream_flags) < 0) {
        return NULL;
    }
    (void)stream_flags;
    int fd = open(path, flags, 0666);
    if (fd < 0) {
        return NULL;
    }
    FILE *stream = fdopen(fd, mode);
    if (stream == NULL) {
        int error = errno;
        close(fd);
        errno = error;
    }
    return stream;
}

int fclose(FILE *stream)
{
    if (!stream_is_open(stream)) {
        return stream_failure(stream, EBADF);
    }
    int result = close(stream->fd);
    stream->flags |= STREAM_CLOSED;
    stream->fd = -1;
    return result;
}

int fflush(FILE *stream)
{
    if (stream == NULL) {
        return 0;
    }
    return stream_is_open(stream) ? 0 : stream_failure(stream, EBADF);
}

int setvbuf(FILE *stream, char *buffer, int mode, size_t size)
{
    (void)stream;
    (void)buffer;
    (void)mode;
    (void)size;
    return 0;
}

void perror(const char *message)
{
    if (message != NULL && message[0] != 0) {
        fprintf(stderr, "%s: ", message);
    }
    fprintf(stderr, "%s\n", strerror(errno));
}

/* panic=abort has no language-level unwinding. Keep std's optional
 * backtrace formatting linkable and report an empty trace until OCTAGRAM
 * grows a native unwinder. */
int _Unwind_Backtrace(int (*trace)(void *, void *), void *context)
{
    (void)trace;
    (void)context;
    return 5; /* _URC_END_OF_STACK */
}

uintptr_t _Unwind_GetIP(void *context)
{
    (void)context;
    return 0;
}

size_t fread(void *buffer, size_t size, size_t count, FILE *stream)
{
    if (size == 0 || count == 0) {
        return 0;
    }
    if (!stream_is_open(stream)) {
        stream_failure(stream, EBADF);
        return 0;
    }
    if ((stream->flags & STREAM_READABLE) == 0) {
        stream_failure(stream, EBADF);
        return 0;
    }
    if (count > SIZE_MAX / size) {
        stream_failure(stream, EOVERFLOW);
        return 0;
    }

    size_t requested = size * count;
    size_t completed = 0;
    unsigned char *output = buffer;
    while (completed < requested) {
        if ((stream->flags & STREAM_PUSHBACK) != 0) {
            output[completed++] = (unsigned char)stream->pushed_character;
            stream->flags &= ~STREAM_PUSHBACK;
            continue;
        }
        ssize_t result = read(stream->fd, output + completed, requested - completed);
        if (result < 0) {
            stream->flags |= STREAM_ERROR;
            break;
        }
        if (result == 0) {
            stream->flags |= STREAM_EOF;
            break;
        }
        completed += (size_t)result;
    }
    return completed / size;
}

size_t fwrite(const void *buffer, size_t size, size_t count, FILE *stream)
{
    if (size == 0 || count == 0) {
        return 0;
    }
    if (!stream_is_open(stream)) {
        stream_failure(stream, EBADF);
        return 0;
    }
    if ((stream->flags & STREAM_WRITABLE) == 0) {
        stream_failure(stream, EBADF);
        return 0;
    }
    if (count > SIZE_MAX / size) {
        stream_failure(stream, EOVERFLOW);
        return 0;
    }

    size_t requested = size * count;
    size_t completed = 0;
    const unsigned char *input = buffer;
    while (completed < requested) {
        ssize_t result = write(stream->fd, input + completed, requested - completed);
        if (result < 0) {
            stream->flags |= STREAM_ERROR;
            break;
        }
        if (result == 0) {
            stream_failure(stream, EIO);
            break;
        }
        completed += (size_t)result;
    }
    return completed / size;
}

int fgetc(FILE *stream)
{
    unsigned char character;
    return fread(&character, 1, 1, stream) == 1 ? character : EOF;
}

int getc(FILE *stream)
{
    return fgetc(stream);
}

int getchar(void)
{
    return fgetc(stdin);
}

char *fgets(char *buffer, int size, FILE *stream)
{
    if (buffer == NULL || size <= 0) {
        stream_failure(stream, EINVAL);
        return NULL;
    }
    if (size == 1) {
        buffer[0] = '\0';
        return buffer;
    }
    int length = 0;
    while (length < size - 1) {
        int character = fgetc(stream);
        if (character == EOF) {
            break;
        }
        buffer[length++] = (char)character;
        if (character == '\n') {
            break;
        }
    }
    if (length == 0) {
        return NULL;
    }
    buffer[length] = '\0';
    return buffer;
}

int ungetc(int character, FILE *stream)
{
    if (character == EOF) {
        return EOF;
    }
    if (!stream_is_open(stream)) {
        return stream_failure(stream, EBADF);
    }
    if ((stream->flags & STREAM_READABLE) == 0 ||
        (stream->flags & STREAM_PUSHBACK) != 0) {
        return stream_failure(stream, EINVAL);
    }
    stream->pushed_character = (unsigned char)character;
    stream->flags = (stream->flags | STREAM_PUSHBACK) & ~STREAM_EOF;
    return (unsigned char)character;
}

int fputc(int character, FILE *stream)
{
    unsigned char byte = (unsigned char)character;
    return fwrite(&byte, 1, 1, stream) == 1 ? byte : EOF;
}

int putc(int character, FILE *stream)
{
    return fputc(character, stream);
}

int putchar(int character)
{
    return fputc(character, stdout);
}

int fputs(const char *text, FILE *stream)
{
    if (text == NULL) {
        return stream_failure(stream, EINVAL);
    }
    size_t length = strlen(text);
    return fwrite(text, 1, length, stream) == length ? 0 : EOF;
}

int puts(const char *text)
{
    if (fputs(text, stdout) == EOF || fputc('\n', stdout) == EOF) {
        return EOF;
    }
    return 0;
}

int fseek(FILE *stream, long offset, int whence)
{
    if (!stream_is_open(stream)) {
        return stream_failure(stream, EBADF);
    }
    off_t wide_offset = (off_t)offset;
    if ((long)wide_offset != offset) {
        return stream_failure(stream, EOVERFLOW);
    }
    if (lseek(stream->fd, wide_offset, whence) < 0) {
        stream->flags |= STREAM_ERROR;
        return -1;
    }
    stream->flags &= ~(STREAM_EOF | STREAM_PUSHBACK);
    return 0;
}

long ftell(FILE *stream)
{
    if (!stream_is_open(stream)) {
        stream_failure(stream, EBADF);
        return -1;
    }
    off_t position = lseek(stream->fd, 0, SEEK_CUR);
    if (position < 0) {
        stream->flags |= STREAM_ERROR;
        return -1;
    }
    if ((stream->flags & STREAM_PUSHBACK) != 0) {
        --position;
    }
    if ((off_t)(long)position != position) {
        stream_failure(stream, EOVERFLOW);
        return -1;
    }
    return (long)position;
}

void rewind(FILE *stream)
{
    if (fseek(stream, 0, SEEK_SET) == 0) {
        clearerr(stream);
    }
}

int feof(FILE *stream)
{
    return stream != NULL && (stream->flags & STREAM_EOF) != 0;
}

int ferror(FILE *stream)
{
    return stream != NULL && (stream->flags & STREAM_ERROR) != 0;
}

void clearerr(FILE *stream)
{
    if (stream != NULL) {
        stream->flags &= ~(STREAM_EOF | STREAM_ERROR);
    }
}

int fileno(FILE *stream)
{
    if (!stream_is_open(stream)) {
        stream_failure(stream, EBADF);
        return -1;
    }
    return stream->fd;
}

typedef struct {
    FILE *stream;
    char *buffer;
    size_t capacity;
    size_t length;
    int to_buffer;
    int failed;
} FormatOutput;

enum FormatLength {
    FORMAT_DEFAULT,
    FORMAT_HH,
    FORMAT_H,
    FORMAT_L,
    FORMAT_LL,
    FORMAT_Z,
    FORMAT_T,
    FORMAT_J,
    FORMAT_CAP_L
};

static int format_output_write(FormatOutput *output, const char *bytes,
                               size_t count)
{
    if (output->failed) {
        return -1;
    }
    if (count > SIZE_MAX - output->length) {
        errno = EOVERFLOW;
        output->failed = 1;
        return -1;
    }
    if (output->to_buffer) {
        if (output->capacity != 0 && output->length < output->capacity - 1) {
            size_t available = output->capacity - 1 - output->length;
            size_t copied = count < available ? count : available;
            memcpy(output->buffer + output->length, bytes, copied);
        }
    } else if (fwrite(bytes, 1, count, output->stream) != count) {
        output->failed = 1;
        return -1;
    }
    output->length += count;
    return 0;
}

static int format_output_repeat(FormatOutput *output, char character,
                                size_t count)
{
    char repeated[64];
    memset(repeated, character, sizeof(repeated));
    while (count != 0) {
        size_t chunk = count < sizeof(repeated) ? count : sizeof(repeated);
        if (format_output_write(output, repeated, chunk) < 0) {
            return -1;
        }
        count -= chunk;
    }
    return 0;
}

static int parse_format_number(const char **cursor, int *number)
{
    int value = 0;
    while (**cursor >= '0' && **cursor <= '9') {
        int digit = **cursor - '0';
        if (value > (INT_MAX - digit) / 10) {
            errno = EOVERFLOW;
            return -1;
        }
        value = value * 10 + digit;
        ++*cursor;
    }
    *number = value;
    return 0;
}

static int format_integer(FormatOutput *output, uintmax_t value, int negative,
                          unsigned int base, int uppercase, int width,
                          int precision, int left, int zero, int plus,
                          int space, int alternate, int pointer)
{
    char digits[sizeof(uintmax_t) * 8];
    const char *alphabet = uppercase ? "0123456789ABCDEF" : "0123456789abcdef";
    size_t digit_count = 0;
    if (value != 0 || precision != 0) {
        do {
            digits[digit_count++] = alphabet[value % base];
            value /= base;
        } while (value != 0);
    }

    size_t leading_zeroes = precision > 0 && (size_t)precision > digit_count
                                ? (size_t)precision - digit_count
                                : 0;
    if (base == 8 && alternate &&
        (digit_count == 0 || digits[digit_count - 1] != '0') &&
        leading_zeroes == 0) {
        leading_zeroes = 1;
    }

    char sign = negative ? '-' : (plus ? '+' : (space ? ' ' : '\0'));
    const char *prefix = "";
    size_t prefix_length = 0;
    if (pointer || (alternate && base == 16 && digit_count != 0 &&
                    !(digit_count == 1 && digits[0] == '0'))) {
        prefix = uppercase ? "0X" : "0x";
        prefix_length = 2;
    }
    size_t total = digit_count + leading_zeroes + prefix_length +
                   (sign != '\0' ? 1u : 0u);
    size_t padding = width > 0 && (size_t)width > total
                         ? (size_t)width - total
                         : 0;
    if (!left && (!zero || precision >= 0) &&
        format_output_repeat(output, ' ', padding) < 0) {
        return -1;
    }
    if (sign != '\0' && format_output_write(output, &sign, 1) < 0) {
        return -1;
    }
    if (prefix_length != 0 &&
        format_output_write(output, prefix, prefix_length) < 0) {
        return -1;
    }
    if (!left && zero && precision < 0) {
        leading_zeroes += padding;
        padding = 0;
    }
    if (format_output_repeat(output, '0', leading_zeroes) < 0) {
        return -1;
    }
    while (digit_count != 0) {
        char digit = digits[--digit_count];
        if (format_output_write(output, &digit, 1) < 0) {
            return -1;
        }
    }
    return left ? format_output_repeat(output, ' ', padding) : 0;
}

static int format_v(FormatOutput *output, const char *format, va_list arguments)
{
    if (format == NULL) {
        errno = EINVAL;
        return -1;
    }
    while (*format != '\0') {
        const char *literal = format;
        while (*format != '\0' && *format != '%') {
            ++format;
        }
        if (format_output_write(output, literal, (size_t)(format - literal)) < 0) {
            return -1;
        }
        if (*format == '\0') {
            break;
        }
        ++format;

        int left = 0;
        int plus = 0;
        int space = 0;
        int alternate = 0;
        int zero = 0;
        int parse_flags = 1;
        while (parse_flags) {
            switch (*format) {
            case '-': left = 1; ++format; break;
            case '+': plus = 1; ++format; break;
            case ' ': space = 1; ++format; break;
            case '#': alternate = 1; ++format; break;
            case '0': zero = 1; ++format; break;
            default: parse_flags = 0; break;
            }
        }

        int width = 0;
        if (*format == '*') {
            width = va_arg(arguments, int);
            ++format;
            if (width < 0) {
                if (width == INT_MIN) {
                    errno = EOVERFLOW;
                    return -1;
                }
                left = 1;
                width = -width;
            }
        } else if (parse_format_number(&format, &width) < 0) {
            return -1;
        }

        int precision = -1;
        if (*format == '.') {
            ++format;
            precision = 0;
            if (*format == '*') {
                precision = va_arg(arguments, int);
                ++format;
                if (precision < 0) {
                    precision = -1;
                }
            } else if (parse_format_number(&format, &precision) < 0) {
                return -1;
            }
        }

        enum FormatLength length = FORMAT_DEFAULT;
        if (format[0] == 'h' && format[1] == 'h') {
            length = FORMAT_HH;
            format += 2;
        } else if (*format == 'h') {
            length = FORMAT_H;
            ++format;
        } else if (format[0] == 'l' && format[1] == 'l') {
            length = FORMAT_LL;
            format += 2;
        } else if (*format == 'l') {
            length = FORMAT_L;
            ++format;
        } else if (*format == 'z') {
            length = FORMAT_Z;
            ++format;
        } else if (*format == 't') {
            length = FORMAT_T;
            ++format;
        } else if (*format == 'j') {
            length = FORMAT_J;
            ++format;
        } else if (*format == 'L') {
            length = FORMAT_CAP_L;
            ++format;
        }

        char conversion = *format;
        if (conversion == '\0') {
            errno = EINVAL;
            return -1;
        }
        ++format;
        if (conversion == '%') {
            if (format_output_write(output, "%", 1) < 0) {
                return -1;
            }
            continue;
        }
        if (conversion == 's') {
            if (length != FORMAT_DEFAULT) {
                errno = EINVAL;
                return -1;
            }
            const char *text = va_arg(arguments, const char *);
            if (text == NULL) {
                text = "(null)";
            }
            size_t text_length = strlen(text);
            if (precision >= 0 && text_length > (size_t)precision) {
                text_length = (size_t)precision;
            }
            size_t padding = width > 0 && (size_t)width > text_length
                                 ? (size_t)width - text_length
                                 : 0;
            if (!left && format_output_repeat(output, ' ', padding) < 0) {
                return -1;
            }
            if (format_output_write(output, text, text_length) < 0) {
                return -1;
            }
            if (left && format_output_repeat(output, ' ', padding) < 0) {
                return -1;
            }
            continue;
        }
        if (conversion == 'c') {
            char character = (char)va_arg(arguments, int);
            size_t padding = width > 1 ? (size_t)width - 1 : 0;
            if (!left && format_output_repeat(output, ' ', padding) < 0) {
                return -1;
            }
            if (format_output_write(output, &character, 1) < 0) {
                return -1;
            }
            if (left && format_output_repeat(output, ' ', padding) < 0) {
                return -1;
            }
            continue;
        }
        if (conversion == 'p') {
            uintptr_t value = (uintptr_t)va_arg(arguments, void *);
            if (format_integer(output, value, 0, 16, 0, width, precision,
                               left, zero, 0, 0, 1, 1) < 0) {
                return -1;
            }
            continue;
        }
        if (conversion == 'd' || conversion == 'i') {
            intmax_t value;
            switch (length) {
            case FORMAT_HH: value = (signed char)va_arg(arguments, int); break;
            case FORMAT_H: value = (short)va_arg(arguments, int); break;
            case FORMAT_L: value = va_arg(arguments, long); break;
            case FORMAT_LL: value = va_arg(arguments, long long); break;
            case FORMAT_Z: value = va_arg(arguments, ssize_t); break;
            case FORMAT_T: value = va_arg(arguments, ptrdiff_t); break;
            case FORMAT_J: value = va_arg(arguments, intmax_t); break;
            default: value = va_arg(arguments, int); break;
            }
            int negative = value < 0;
            uintmax_t magnitude = negative
                                      ? (uintmax_t)(-(value + 1)) + 1
                                      : (uintmax_t)value;
            if (format_integer(output, magnitude, negative, 10, 0, width,
                               precision, left, zero, plus, space, 0, 0) < 0) {
                return -1;
            }
            continue;
        }
        if (conversion == 'u' || conversion == 'o' ||
            conversion == 'x' || conversion == 'X') {
            uintmax_t value;
            switch (length) {
            case FORMAT_HH: value = (unsigned char)va_arg(arguments, int); break;
            case FORMAT_H: value = (unsigned short)va_arg(arguments, int); break;
            case FORMAT_L: value = va_arg(arguments, unsigned long); break;
            case FORMAT_LL: value = va_arg(arguments, unsigned long long); break;
            case FORMAT_Z: value = va_arg(arguments, size_t); break;
            case FORMAT_T: value = va_arg(arguments, uintptr_t); break;
            case FORMAT_J: value = va_arg(arguments, uintmax_t); break;
            default: value = va_arg(arguments, unsigned int); break;
            }
            unsigned int base = conversion == 'o' ? 8 :
                                (conversion == 'x' || conversion == 'X') ? 16 : 10;
            if (format_integer(output, value, 0, base, conversion == 'X',
                               width, precision, left, zero, 0, 0, alternate,
                               0) < 0) {
                return -1;
            }
            continue;
        }
        errno = EINVAL;
        return -1;
    }
    if (output->failed || output->length > INT_MAX) {
        if (!output->failed) {
            errno = EOVERFLOW;
        }
        return -1;
    }
    return (int)output->length;
}

typedef struct {
    void *context;
    int (*get)(void *context);
    int (*unget)(int character, void *context);
    size_t consumed;
    int input_failure;
} ScanInput;

typedef struct {
    const unsigned char *text;
    size_t position;
} ScanString;

static int scan_file_get(void *context)
{
    return fgetc((FILE *)context);
}

static int scan_file_unget(int character, void *context)
{
    return ungetc(character, (FILE *)context);
}

static int scan_string_get(void *context)
{
    ScanString *input = context;
    unsigned char character = input->text[input->position];
    if (character == '\0') {
        return EOF;
    }
    ++input->position;
    return character;
}

static int scan_string_unget(int character, void *context)
{
    ScanString *input = context;
    if (input->position == 0 || input->text[input->position - 1] != character) {
        return EOF;
    }
    --input->position;
    return character;
}

static int scan_get(ScanInput *input)
{
    int character = input->get(input->context);
    if (character == EOF) {
        input->input_failure = 1;
    } else {
        ++input->consumed;
    }
    return character;
}

static int scan_unget(ScanInput *input, int character)
{
    if (character == EOF) {
        return EOF;
    }
    int result = input->unget(character, input->context);
    if (result != EOF) {
        --input->consumed;
        input->input_failure = 0;
    }
    return result;
}

static int scan_peek(ScanInput *input)
{
    int character = scan_get(input);
    if (character != EOF) {
        scan_unget(input, character);
    }
    return character;
}

static void scan_skip_space(ScanInput *input)
{
    int character;
    while ((character = scan_get(input)) != EOF) {
        if (!isspace((unsigned char)character)) {
            scan_unget(input, character);
            return;
        }
    }
}

static int scan_digit(int character)
{
    if (character >= '0' && character <= '9') {
        return character - '0';
    }
    if (character >= 'a' && character <= 'z') {
        return character - 'a' + 10;
    }
    if (character >= 'A' && character <= 'Z') {
        return character - 'A' + 10;
    }
    return -1;
}

static void scan_store_signed(void *destination, enum FormatLength length,
                              long long value)
{
    switch (length) {
    case FORMAT_HH: *(signed char *)destination = (signed char)value; break;
    case FORMAT_H: *(short *)destination = (short)value; break;
    case FORMAT_L: *(long *)destination = (long)value; break;
    case FORMAT_LL: *(long long *)destination = value; break;
    case FORMAT_Z: *(ssize_t *)destination = (ssize_t)value; break;
    case FORMAT_T: *(ptrdiff_t *)destination = (ptrdiff_t)value; break;
    case FORMAT_J: *(intmax_t *)destination = (intmax_t)value; break;
    default: *(int *)destination = (int)value; break;
    }
}

static void scan_store_unsigned(void *destination, enum FormatLength length,
                                unsigned long long value)
{
    switch (length) {
    case FORMAT_HH: *(unsigned char *)destination = (unsigned char)value; break;
    case FORMAT_H: *(unsigned short *)destination = (unsigned short)value; break;
    case FORMAT_L: *(unsigned long *)destination = (unsigned long)value; break;
    case FORMAT_LL: *(unsigned long long *)destination = value; break;
    case FORMAT_Z: *(size_t *)destination = (size_t)value; break;
    case FORMAT_T: *(uintptr_t *)destination = (uintptr_t)value; break;
    case FORMAT_J: *(uintmax_t *)destination = (uintmax_t)value; break;
    default: *(unsigned int *)destination = (unsigned int)value; break;
    }
}

static int scan_integer(ScanInput *input, int width, int conversion,
                        enum FormatLength length, int suppressed,
                        va_list arguments)
{
    if (width <= 0) {
        width = INT_MAX;
    }
    scan_skip_space(input);
    int remaining = width;
    int negative = 0;
    int character = scan_peek(input);
    if (character == '+' || character == '-') {
        negative = character == '-';
        scan_get(input);
        --remaining;
    }
    int base = conversion == 'o' ? 8 :
               conversion == 'x' || conversion == 'X' || conversion == 'p' ? 16 :
               conversion == 'i' ? 0 : 10;
    if (base == 0) {
        base = scan_peek(input) == '0' ? 8 : 10;
    }
    if ((base == 0 || base == 16 || conversion == 'i') && remaining >= 2 &&
        scan_peek(input) == '0') {
        scan_get(input);
        --remaining;
        int prefix = scan_peek(input);
        if ((prefix == 'x' || prefix == 'X') && remaining >= 1) {
            scan_get(input);
            --remaining;
            int next_digit = scan_digit(scan_peek(input));
            if ((conversion == 'i' || conversion == 'x' ||
                 conversion == 'X' || conversion == 'p') &&
                next_digit >= 0 && next_digit < 16) {
                base = 16;
            } else {
                scan_unget(input, prefix);
                if (!suppressed) {
                    void *destination = va_arg(arguments, void *);
                    if (conversion == 'd' || conversion == 'i') {
                        scan_store_signed(destination, length, 0);
                    } else {
                        scan_store_unsigned(destination, length, 0);
                    }
                }
                return 1;
            }
        } else {
            unsigned long long value = 0;
            int digits = 1;
            while (remaining > 0) {
                character = scan_peek(input);
                int digit = scan_digit(character);
                if (digit < 0 || digit >= base) {
                    break;
                }
                scan_get(input);
                --remaining;
                if (value <= (ULLONG_MAX - (unsigned int)digit) / (unsigned int)base) {
                    value = value * (unsigned int)base + (unsigned int)digit;
                }
                ++digits;
            }
            if (!suppressed) {
                void *destination = va_arg(arguments, void *);
                if (conversion == 'd' || conversion == 'i') {
                    long long signed_value = (long long)value;
                    scan_store_signed(destination, length,
                                      negative ? -signed_value : signed_value);
                } else {
                    scan_store_unsigned(destination, length,
                                        negative ? 0ULL - value : value);
                }
            }
            return digits;
        }
    }

    unsigned long long value = 0;
    int digits = 0;
    while (remaining > 0) {
        character = scan_peek(input);
        int digit = scan_digit(character);
        if (digit < 0 || digit >= base) {
            break;
        }
        scan_get(input);
        --remaining;
        if (value <= (ULLONG_MAX - (unsigned int)digit) / (unsigned int)base) {
            value = value * (unsigned int)base + (unsigned int)digit;
        }
        ++digits;
    }
    if (digits == 0) {
        if (negative) {
            scan_unget(input, '-');
        } else if (width > remaining) {
            scan_unget(input, '+');
        }
        return 0;
    }
    if (!suppressed) {
        void *destination = va_arg(arguments, void *);
        if (conversion == 'd' || conversion == 'i') {
            long long signed_value = (long long)value;
            scan_store_signed(destination, length,
                              negative ? -signed_value : signed_value);
        } else if (conversion == 'p') {
            *(void **)destination = (void *)(uintptr_t)value;
        } else {
            scan_store_unsigned(destination, length,
                                negative ? 0ULL - value : value);
        }
    }
    return 1;
}

static int scan_float(ScanInput *input, int width, enum FormatLength length,
                      int suppressed, va_list arguments)
{
    if (length != FORMAT_DEFAULT && length != FORMAT_L &&
        length != FORMAT_CAP_L) {
        errno = EINVAL;
        return -1;
    }
    scan_skip_space(input);
    if (width <= 0) {
        width = INT_MAX;
    }
    int remaining = width;
    int negative = 0;
    int sign_character = 0;
    int character = scan_peek(input);
    if (character == '+' || character == '-') {
        sign_character = character;
        negative = character == '-';
        scan_get(input);
        --remaining;
    }

    int hexadecimal = 0;
    int digits = 0;
    int base = 10;
    long double value = 0.0L;
    long double place = 1.0L;
    if (remaining > 0 && scan_peek(input) == '0') {
        scan_get(input);
        --remaining;
        ++digits;
        character = scan_peek(input);
        if (remaining > 0 && (character == 'x' || character == 'X')) {
            scan_get(input);
            --remaining;
            hexadecimal = 1;
            digits = 0;
            base = 16;
        }
    }

    while (remaining > 0) {
        character = scan_peek(input);
        int digit = scan_digit(character);
        if (digit < 0 || digit >= base) {
            break;
        }
        scan_get(input);
        --remaining;
        ++digits;
        value = value * base + digit;
    }
    if (remaining > 0 && scan_peek(input) == '.') {
        scan_get(input);
        --remaining;
        while (remaining > 0) {
            character = scan_peek(input);
            int digit = scan_digit(character);
            if (digit < 0 || digit >= base) {
                break;
            }
            scan_get(input);
            --remaining;
            ++digits;
            place /= base;
            value += digit * place;
        }
    }
    if (digits == 0) {
        if (sign_character != 0) {
            scan_unget(input, sign_character);
        }
        return 0;
    }

    int exponent_marker = hexadecimal
                              ? (scan_peek(input) == 'p' || scan_peek(input) == 'P')
                              : (scan_peek(input) == 'e' || scan_peek(input) == 'E');
    if (remaining > 0 && exponent_marker) {
        scan_get(input);
        --remaining;
        int exponent_negative = 0;
        if (remaining > 0 &&
            (scan_peek(input) == '+' || scan_peek(input) == '-')) {
            character = scan_get(input);
            --remaining;
            exponent_negative = character == '-';
        }
        int exponent_digits = 0;
        int exponent = 0;
        while (remaining > 0 &&
               (character = scan_peek(input)) >= '0' && character <= '9') {
            character = scan_get(input);
            --remaining;
            ++exponent_digits;
            if (exponent < 4096) {
                exponent = exponent * 10 + character - '0';
                if (exponent > 4096) {
                    exponent = 4096;
                }
            }
        }
        if (exponent_digits == 0) {
            return 0;
        }
        int exponent_base = hexadecimal ? 2 : 10;
        while (exponent > 0) {
            if (exponent_negative) {
                value /= exponent_base;
            } else {
                value *= exponent_base;
            }
            --exponent;
        }
    }

    if (negative) {
        value = -value;
    }
    if (!suppressed) {
        if (length == FORMAT_DEFAULT) {
            *va_arg(arguments, float *) = (float)value;
        } else if (length == FORMAT_L) {
            *va_arg(arguments, double *) = (double)value;
        } else {
            *va_arg(arguments, long double *) = value;
        }
    }
    return 1;
}

static int scan_build_set(const char **format, unsigned char set[256])
{
    memset(set, 0, 256);
    int invert = 0;
    if (**format == '^') {
        invert = 1;
        ++*format;
    }
    if (**format == ']') {
        set[(unsigned char)**format] = 1;
        ++*format;
    }
    int members = set[(unsigned char)']'];
    while (**format != '\0' && **format != ']') {
        unsigned char first = (unsigned char)*(*format)++;
        if (**format == '-' && (*format)[1] != '\0' && (*format)[1] != ']') {
            ++*format;
            unsigned char last = (unsigned char)*(*format)++;
            if (first <= last) {
                for (unsigned int value = first; value <= last; ++value) {
                    set[value] = 1;
                }
            } else {
                set[first] = 1;
                set[(unsigned char)'-'] = 1;
                set[last] = 1;
            }
        } else {
            set[first] = 1;
        }
        ++members;
    }
    if (**format != ']' || members == 0) {
        return -1;
    }
    ++*format;
    if (invert) {
        for (size_t index = 0; index < 256; ++index) {
            set[index] = !set[index];
        }
    }
    return 0;
}

static int scan_v(ScanInput *input, const char *format, va_list arguments)
{
    if (format == NULL || input == NULL) {
        errno = EINVAL;
        return EOF;
    }
    int assignments = 0;
    while (*format != '\0') {
        if (isspace((unsigned char)*format)) {
            while (isspace((unsigned char)*format)) {
                ++format;
            }
            scan_skip_space(input);
            continue;
        }
        if (*format != '%') {
            int character = scan_get(input);
            if (character == EOF) {
                break;
            }
            if (character != (unsigned char)*format) {
                scan_unget(input, character);
                break;
            }
            ++format;
            continue;
        }
        ++format;
        if (*format == '%') {
            int character = scan_get(input);
            if (character != '%') {
                if (character != EOF) {
                    scan_unget(input, character);
                }
                break;
            }
            ++format;
            continue;
        }

        int suppressed = 0;
        if (*format == '*') {
            suppressed = 1;
            ++format;
        }
        int width = 0;
        if (parse_format_number(&format, &width) < 0) {
            return EOF;
        }
        enum FormatLength length = FORMAT_DEFAULT;
        if (format[0] == 'h' && format[1] == 'h') {
            length = FORMAT_HH;
            format += 2;
        } else if (*format == 'h') {
            length = FORMAT_H;
            ++format;
        } else if (format[0] == 'l' && format[1] == 'l') {
            length = FORMAT_LL;
            format += 2;
        } else if (*format == 'l') {
            length = FORMAT_L;
            ++format;
        } else if (*format == 'z') {
            length = FORMAT_Z;
            ++format;
        } else if (*format == 't') {
            length = FORMAT_T;
            ++format;
        } else if (*format == 'j') {
            length = FORMAT_J;
            ++format;
        } else if (*format == 'L') {
            length = FORMAT_CAP_L;
            ++format;
        }
        char conversion = *format;
        if (conversion == '\0') {
            errno = EINVAL;
            return EOF;
        }
        ++format;

        if (conversion == 'n') {
            if (!suppressed) {
                scan_store_signed(va_arg(arguments, void *), length,
                                  (long long)input->consumed);
            }
            continue;
        }
        if (conversion == 'd' || conversion == 'i' || conversion == 'o' ||
            conversion == 'u' || conversion == 'x' || conversion == 'X' ||
            conversion == 'p') {
            int result = scan_integer(input, width, conversion, length,
                                      suppressed, arguments);
            if (result == 0) {
                break;
            }
            assignments += !suppressed;
            continue;
        }
        if (conversion == 'a' || conversion == 'A' || conversion == 'e' ||
            conversion == 'E' || conversion == 'f' || conversion == 'F' ||
            conversion == 'g' || conversion == 'G') {
            int result = scan_float(input, width, length, suppressed, arguments);
            if (result <= 0) {
                if (result < 0) {
                    return EOF;
                }
                break;
            }
            assignments += !suppressed;
            continue;
        }
        if (conversion == 's' || conversion == 'c' || conversion == '[') {
            unsigned char set[256];
            if (conversion == '[' && scan_build_set(&format, set) < 0) {
                errno = EINVAL;
                return EOF;
            }
            if (conversion != 'c') {
                scan_skip_space(input);
            }
            int limit = width == 0 ? (conversion == 'c' ? 1 : INT_MAX) : width;
            char *destination = suppressed ? NULL : va_arg(arguments, char *);
            int matched = 0;
            while (matched < limit) {
                int character = scan_peek(input);
                if (character == EOF) {
                    break;
                }
                int accepts = conversion == 'c' ||
                              (conversion == 's' &&
                               !isspace((unsigned char)character)) ||
                              (conversion == '[' && set[(unsigned char)character]);
                if (!accepts) {
                    break;
                }
                scan_get(input);
                if (!suppressed) {
                    destination[matched] = (char)character;
                }
                ++matched;
            }
            if (matched == 0 || (conversion == 'c' && matched != limit)) {
                break;
            }
            if (!suppressed && conversion != 'c') {
                destination[matched] = '\0';
            }
            assignments += !suppressed;
            continue;
        }
        errno = EINVAL;
        return EOF;
    }
    if (assignments == 0 && input->input_failure) {
        return EOF;
    }
    return assignments;
}

int vfscanf(FILE *stream, const char *format, va_list arguments)
{
    if (!stream_is_open(stream) || (stream->flags & STREAM_READABLE) == 0) {
        return stream_failure(stream, EBADF);
    }
    ScanInput input = {stream, scan_file_get, scan_file_unget, 0, 0};
    int result = scan_v(&input, format, arguments);
    if (input.input_failure) {
        stream->flags |= STREAM_EOF;
    }
    return result;
}

int fscanf(FILE *stream, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vfscanf(stream, format, arguments);
    va_end(arguments);
    return result;
}

int vscanf(const char *format, va_list arguments)
{
    return vfscanf(stdin, format, arguments);
}

int scanf(const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vscanf(format, arguments);
    va_end(arguments);
    return result;
}

int vsscanf(const char *text, const char *format, va_list arguments)
{
    if (text == NULL) {
        errno = EINVAL;
        return EOF;
    }
    ScanString source = {(const unsigned char *)text, 0};
    ScanInput input = {&source, scan_string_get, scan_string_unget, 0, 0};
    return scan_v(&input, format, arguments);
}

int sscanf(const char *text, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vsscanf(text, format, arguments);
    va_end(arguments);
    return result;
}

int vfprintf(FILE *stream, const char *format, va_list arguments)
{
    if (!stream_is_open(stream) || (stream->flags & STREAM_WRITABLE) == 0) {
        return stream_failure(stream, EBADF);
    }
    FormatOutput output = {stream, NULL, 0, 0, 0, 0};
    return format_v(&output, format, arguments);
}

int fprintf(FILE *stream, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vfprintf(stream, format, arguments);
    va_end(arguments);
    return result;
}

int vprintf(const char *format, va_list arguments)
{
    return vfprintf(stdout, format, arguments);
}

int printf(const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vprintf(format, arguments);
    va_end(arguments);
    return result;
}

int vsnprintf(char *buffer, size_t size, const char *format, va_list arguments)
{
    if (size != 0 && buffer == NULL) {
        errno = EINVAL;
        return -1;
    }
    FormatOutput output = {NULL, buffer, size, 0, 1, 0};
    int result = format_v(&output, format, arguments);
    if (size != 0) {
        size_t terminator = output.length < size ? output.length : size - 1;
        buffer[terminator] = '\0';
    }
    return result;
}

int snprintf(char *buffer, size_t size, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vsnprintf(buffer, size, format, arguments);
    va_end(arguments);
    return result;
}

int vsprintf(char *buffer, const char *format, va_list arguments)
{
    if (buffer == NULL) {
        errno = EINVAL;
        return -1;
    }
    return vsnprintf(buffer, SIZE_MAX, format, arguments);
}

int sprintf(char *buffer, const char *format, ...)
{
    va_list arguments;
    va_start(arguments, format);
    int result = vsprintf(buffer, format, arguments);
    va_end(arguments);
    return result;
}

static volatile int oct_allocator_lock;

static void oct_allocator_lock_acquire(void)
{
    while (__sync_lock_test_and_set(&oct_allocator_lock, 1)) {
        __asm__ volatile ("pause");
    }
}

static void oct_allocator_lock_release(void)
{
    __sync_lock_release(&oct_allocator_lock);
}

void *malloc(size_t size)
{
    if (size == 0) {
        size = 1;
    }
    if (size > SIZE_MAX - 15) {
        errno = ENOMEM;
        return NULL;
    }
    size = (size + 15) & ~(size_t)15;

    for (;;) {
        oct_allocator_lock_acquire();
        AllocationHeader **free_link = &free_list;
        while (*free_link != NULL) {
            AllocationHeader *header = *free_link;
            if (header->size >= size) {
                if (header->size - size >= sizeof(*header) + 16) {
                    AllocationHeader *remainder =
                        (AllocationHeader *)((unsigned char *)(header + 1) + size);
                    remainder->size = header->size - size - sizeof(*header);
                    remainder->next_free = header->next_free;
                    remainder->allocation_base = remainder;
                    *free_link = remainder;
                    header->size = size;
                } else {
                    *free_link = header->next_free;
                }
                header->next_free = NULL;
                oct_allocator_lock_release();
                return header + 1;
            }
            free_link = &header->next_free;
        }
        oct_allocator_lock_release();

        if (size > SIZE_MAX - sizeof(AllocationHeader)) {
            errno = ENOMEM;
            return NULL;
        }
        size_t requested = size + sizeof(AllocationHeader);
        size_t mapping_size = requested > 65536 ? requested : 65536;
        if (mapping_size > SIZE_MAX - 4095) {
            errno = ENOMEM;
            return NULL;
        }
        mapping_size = (mapping_size + 4095) & ~(size_t)4095;
        void *mapping = mmap(NULL, mapping_size, PROT_READ | PROT_WRITE,
                             MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (mapping == MAP_FAILED) {
            return NULL;
        }
        AllocationHeader *header = mapping;
        header->size = mapping_size - sizeof(*header);
        header->allocation_base = header;
        oct_allocator_lock_acquire();
        header->next_free = free_list;
        free_list = header;
        oct_allocator_lock_release();
    }
}

void *realloc(void *pointer, size_t size)
{
    if (pointer == NULL) {
        return malloc(size);
    }
    if (size == 0) {
        free(pointer);
        return NULL;
    }

    AllocationHeader *header = (AllocationHeader *)pointer - 1;
    size_t old_size = header->size;
    if (size <= old_size) {
        return pointer;
    }
    void *result = malloc(size);
    if (result != NULL) {
        memcpy(result, pointer, old_size);
        free(pointer);
    }
    return result;
}

void free(void *pointer)
{
    if (pointer == NULL) {
        return;
    }
    AllocationHeader *header = (AllocationHeader *)pointer - 1;
    AllocationHeader *allocation = header->allocation_base != NULL
        ? header->allocation_base : header;
    oct_allocator_lock_acquire();
    allocation->next_free = free_list;
    free_list = allocation;
    oct_allocator_lock_release();
}

int posix_memalign(void **result, size_t alignment, size_t size)
{
    if (result == NULL || alignment < sizeof(void *) ||
        (alignment & (alignment - 1)) != 0 ||
        alignment % sizeof(void *) != 0) {
        return EINVAL;
    }
    if (size == 0) {
        size = 1;
    }
    if (size > SIZE_MAX - alignment - sizeof(AllocationHeader)) {
        return ENOMEM;
    }

    size_t request = size + alignment + sizeof(AllocationHeader);
    void *base_pointer = malloc(request);
    if (base_pointer == NULL) {
        return ENOMEM;
    }
    AllocationHeader *base = (AllocationHeader *)base_pointer - 1;
    uintptr_t first = (uintptr_t)base_pointer + sizeof(AllocationHeader);
    uintptr_t aligned = (first + alignment - 1) & ~(uintptr_t)(alignment - 1);
    AllocationHeader *header = (AllocationHeader *)aligned - 1;
    header->size = (size + 15) & ~(size_t)15;
    header->next_free = NULL;
    header->allocation_base = base;
    *result = (void *)aligned;
    return 0;
}

void *calloc(size_t count, size_t size)
{
    if (size != 0 && count > SIZE_MAX / size) {
        errno = ENOMEM;
        return NULL;
    }
    size_t total = count * size;
    void *result = malloc(total);
    if (result != NULL) {
        memset(result, 0, total);
    }
    return result;
}

static void swap_items(unsigned char *left, unsigned char *right, size_t size)
{
    for (size_t i = 0; i < size; ++i) {
        unsigned char byte = left[i];
        left[i] = right[i];
        right[i] = byte;
    }
}

static void sift_down(unsigned char *items, size_t root, size_t count,
                      size_t size, int (*compare)(const void *, const void *))
{
    while (root < count / 2) {
        size_t child = root * 2 + 1;
        if (child + 1 < count &&
            compare(items + child * size, items + (child + 1) * size) < 0) {
            ++child;
        }
        if (compare(items + root * size, items + child * size) >= 0) {
            return;
        }
        swap_items(items + root * size, items + child * size, size);
        root = child;
    }
}

void qsort(void *base, size_t count, size_t size,
           int (*compare)(const void *, const void *))
{
    unsigned char *items = base;
    if (count < 2 || size == 0 || compare == NULL) {
        return;
    }
    for (size_t start = count / 2; start != 0;) {
        sift_down(items, --start, count, size, compare);
    }
    for (size_t end = count; end > 1; --end) {
        swap_items(items, items + (end - 1) * size, size);
        sift_down(items, 0, end - 1, size, compare);
    }
}

#if defined(OCTAGRAM_SHARED)
#define OCT_MAX_RUNTIME_THREAD_STATES 128u

static OctRuntimeThreadState oct_runtime_thread_states[OCT_MAX_RUNTIME_THREAD_STATES];
static OctRuntimeThreadState oct_runtime_fallback_state;
static volatile int oct_runtime_state_lock;

static void oct_runtime_state_lock_acquire(void)
{
    while (__sync_lock_test_and_set(&oct_runtime_state_lock, 1)) {
        __asm__ volatile ("pause");
    }
}

static void oct_runtime_state_lock_release(void)
{
    __sync_lock_release(&oct_runtime_state_lock);
}

static OctRuntimeThreadState *oct_runtime_state_for_thread(void)
{
    int64_t current_tid = oct_thread_self();
    if (current_tid <= 0) {
        return &oct_runtime_fallback_state;
    }
    uint64_t tid = (uint64_t)current_tid;
    int64_t current_pid = oct_getpid();
    if (current_pid <= 0) {
        return &oct_runtime_fallback_state;
    }
    uint64_t pid = (uint64_t)current_pid;

    for (unsigned int i = 0; i < OCT_MAX_RUNTIME_THREAD_STATES; ++i) {
        if (oct_runtime_thread_states[i].pid == pid &&
            oct_runtime_thread_states[i].tid == tid) {
            return &oct_runtime_thread_states[i];
        }
    }

    oct_runtime_state_lock_acquire();
    OctRuntimeThreadState *slot = NULL;

    for (unsigned int i = 0; i < OCT_MAX_RUNTIME_THREAD_STATES; ++i) {
        if (oct_runtime_thread_states[i].pid == pid &&
            oct_runtime_thread_states[i].tid == tid) {
            slot = &oct_runtime_thread_states[i];
            break;
        }
    }

    for (unsigned int i = 0; slot == NULL &&
         i < OCT_MAX_RUNTIME_THREAD_STATES; ++i) {
        if (oct_runtime_thread_states[i].tid == 0 ||
            oct_runtime_thread_states[i].pid != pid) {
            slot = &oct_runtime_thread_states[i];
        }
    }

    for (unsigned int i = 0; slot == NULL &&
         i < OCT_MAX_RUNTIME_THREAD_STATES; ++i) {
        uint64_t other_tid = oct_runtime_thread_states[i].tid;
        if (other_tid == 0) {
            continue;
        }
        uint64_t robust_head = 0;
        uint64_t robust_length = 0;
        if (oct_get_robust_list(other_tid, &robust_head, &robust_length) == -ESRCH) {
            slot = &oct_runtime_thread_states[i];
        }
    }

    if (slot != NULL && (slot->pid != pid || slot->tid != tid)) {
        slot->errno_value = 0;
        slot->h_errno_value = 0;
        for (unsigned int key = 0; key < 64; ++key) {
            slot->key_values[key] = NULL;
            slot->key_generations[key] = 0;
        }
        __sync_synchronize();
        slot->pid = pid;
        slot->tid = tid;
    }
    oct_runtime_state_lock_release();

    /* The kernel scheduler limit is 64 tasks; PID tagging also lets a
     * forked child reclaim entries copied from its parent address space. */
    return slot != NULL ? slot : &oct_runtime_fallback_state;
}
#endif

int *errno_location(void)
{
    return __errno_location();
}

int *__errno_location(void)
{
#if defined(OCTAGRAM_SHARED)
    return &oct_runtime_state_for_thread()->errno_value;
#else
    return &errno;
#endif
}

int *__h_errno_location(void)
{
#if defined(OCTAGRAM_SHARED)
    return &oct_runtime_state_for_thread()->h_errno_value;
#else
    return &h_errno;
#endif
}

void __assert_fail(const char *expression, const char *file, unsigned int line,
                   const char *function)
{
    fprintf(stderr, "%s:%u: %s: Assertion `%s' failed\n", file, line,
            function, expression);
    abort();
}

char *realpath(const char *path, char *resolved_path)
{
    if (path == NULL) {
        errno = EFAULT;
        return NULL;
    }

    char absolute[4096];
    if (path[0] == '/') {
        if (strlen(path) >= sizeof(absolute)) {
            errno = ENAMETOOLONG;
            return NULL;
        }
        memcpy(absolute, path, strlen(path) + 1);
    } else {
        char *cwd = getcwd(absolute, sizeof(absolute));
        if (cwd == NULL) {
            return NULL;
        }
        size_t cwd_length = strlen(absolute);
        size_t path_length = strlen(path);
        if (cwd_length + 1 + path_length >= sizeof(absolute)) {
            errno = ENAMETOOLONG;
            return NULL;
        }
        if (cwd_length > 1) {
            absolute[cwd_length++] = '/';
        }
        memcpy(absolute + cwd_length, path, path_length + 1);
    }

    char output[4096];
    output[0] = '/';
    output[1] = '\0';
    const char *cursor = absolute;
    while (*cursor == '/') {
        ++cursor;
    }
    for (;;) {
        const char *end = cursor;
        while (*end != '\0' && *end != '/') {
            ++end;
        }
        size_t component_length = (size_t)(end - cursor);
        if (component_length == 0) {
            if (*end == '\0') {
                break;
            }
            cursor = end + 1;
            continue;
        }
        char component[256];
        if (component_length >= sizeof(component)) {
            errno = ENAMETOOLONG;
            return NULL;
        }
        memcpy(component, cursor, component_length);
        component[component_length] = '\0';
        if (strcmp(component, ".") == 0) {
        } else if (strcmp(component, "..") == 0) {
            char *last = strrchr(output, '/');
            if (last != NULL && last != output) {
                *last = '\0';
            } else if (last == output) {
                output[1] = '\0';
            }
        } else {
            size_t output_length = strlen(output);
            if (output_length != 1) {
                if (output_length + 1 + component_length >= sizeof(output)) {
                    errno = ENAMETOOLONG;
                    return NULL;
                }
                output[output_length++] = '/';
            } else if (component_length + 1 >= sizeof(output)) {
                errno = ENAMETOOLONG;
                return NULL;
            }
            memcpy(output + output_length, component, component_length + 1);
        }
        if (*end == '\0') {
            break;
        }
        cursor = end + 1;
    }

    struct stat metadata;
    if (stat(output, &metadata) != 0) {
        return NULL;
    }
    if (resolved_path == NULL) {
        resolved_path = malloc(strlen(output) + 1);
        if (resolved_path == NULL) {
            errno = ENOMEM;
            return NULL;
        }
    }
    size_t output_length = strlen(output);
    if (resolved_path != NULL) {
        memcpy(resolved_path, output, output_length + 1);
    }
    return resolved_path;
}

int remove(const char *path)
{
    return unlink(path);
}

int clock_gettime(clockid_t clock_id, struct timespec *time_value)
{
    if (time_value == NULL) {
        errno = EFAULT;
        return -1;
    }
    return (int)posix_result(oct_clock_gettime(clock_id, time_value));
}

time_t time(time_t *result)
{
    struct timespec current;
    if (clock_gettime(CLOCK_REALTIME, &current) != 0) {
        return (time_t)-1;
    }
    if (result != NULL) {
        *result = current.tv_sec;
    }
    return current.tv_sec;
}

static int is_leap_year(int64_t year)
{
    return year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);
}

static int days_in_month(int64_t year, int month)
{
    static const int month_lengths[] = {
        31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31
    };
    return month == 1 && is_leap_year(year) ? 29 : month_lengths[month];
}

struct tm *gmtime(const time_t *time_value)
{
    static struct tm result;
    if (time_value == NULL) {
        errno = EINVAL;
        return NULL;
    }

    int64_t days = *time_value / 86400;
    int64_t day_seconds = *time_value % 86400;
    if (day_seconds < 0) {
        day_seconds += 86400;
        --days;
    }

    int64_t shifted_days = days + 719468;
    int64_t era = (shifted_days >= 0 ? shifted_days : shifted_days - 146096) / 146097;
    unsigned int day_of_era = (unsigned int)(shifted_days - era * 146097);
    unsigned int year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 -
         day_of_era / 146096) / 365;
    int64_t year = (int64_t)year_of_era + era * 400;
    unsigned int day_of_year =
        day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    unsigned int month_prime = (5 * day_of_year + 2) / 153;
    unsigned int day =
        day_of_year - (153 * month_prime + 2) / 5 + 1;
    int month = (int)month_prime + (month_prime < 10 ? 3 : -9);
    year += month <= 2;
    if (year - 1900 < INT_MIN || year - 1900 > INT_MAX) {
        errno = EOVERFLOW;
        return NULL;
    }

    int year_day = (int)day - 1;
    for (int previous_month = 0; previous_month < month - 1; ++previous_month) {
        year_day += days_in_month(year, previous_month);
    }
    int week_day = (int)((days + 4) % 7);
    if (week_day < 0) {
        week_day += 7;
    }

    result.tm_sec = (int)(day_seconds % 60);
    result.tm_min = (int)(day_seconds / 60 % 60);
    result.tm_hour = (int)(day_seconds / 3600);
    result.tm_mday = (int)day;
    result.tm_mon = month - 1;
    result.tm_year = (int)(year - 1900);
    result.tm_wday = week_day;
    result.tm_yday = year_day;
    result.tm_isdst = 0;
    return &result;
}

struct tm *localtime(const time_t *time_value)
{
    return gmtime(time_value);
}

char *ctime(const time_t *time_value)
{
    static char buffer[32];
    static const char *weekdays[] = {
        "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"
    };
    static const char *months[] = {
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
    };
    struct tm *value = localtime(time_value);
    if (value == NULL || value->tm_wday < 0 || value->tm_wday > 6 ||
        value->tm_mon < 0 || value->tm_mon > 11) {
        errno = EINVAL;
        return NULL;
    }
    snprintf(buffer, sizeof(buffer), "%s %s %2d %02d:%02d:%02d %d\n",
             weekdays[value->tm_wday], months[value->tm_mon], value->tm_mday,
             value->tm_hour, value->tm_min, value->tm_sec, value->tm_year + 1900);
    return buffer;
}

struct tm *gmtime_r(const time_t *time_value, struct tm *result)
{
    struct tm *converted = gmtime(time_value);
    if (converted == NULL || result == NULL) {
        return NULL;
    }
    *result = *converted;
    return result;
}

struct tm *localtime_r(const time_t *time_value, struct tm *result)
{
    struct tm *converted = localtime(time_value);
    if (converted == NULL || result == NULL) {
        return NULL;
    }
    *result = *converted;
    return result;
}

long double ldexpl(long double value, int exponent)
{
    if (exponent > 16384) {
        return __builtin_huge_vall();
    }
    if (exponent < -16384) {
        return 0.0L;
    }
    while (exponent > 0) {
        value *= 2.0L;
        --exponent;
    }
    while (exponent < 0) {
        value /= 2.0L;
        ++exponent;
    }
    return value;
}

long sysconf(int name)
{
    int64_t result = oct_sysconf(name);
    return result < 0 ? -1 : (long)result;
}

int execvp(const char *file, char *const arguments[])
{
    if (file == NULL || file[0] == '\0') {
        errno = ENOENT;
        return -1;
    }
    for (const char *cursor = file; *cursor != '\0'; ++cursor) {
        if (*cursor == '/') {
            return execve(file, arguments, environ);
        }
    }

    const char *path = getenv("PATH");
    if (path == NULL) {
        path = "/bin:/usr/bin:/usr/local/bin";
    }
    /* An explicitly empty PATH contains one empty component, which means
     * search the current working directory. Do not treat it as unset. */
    int last_error = ENOENT;
    int access_error = 0;
    const char *component = path;
    size_t file_length = strlen(file);
    for (;;) {
        const char *separator = component;
        while (*separator != '\0' && *separator != ':') {
            ++separator;
        }
        size_t directory_length = (size_t)(separator - component);
        int add_separator = directory_length != 0 &&
                            component[directory_length - 1] != '/';
        size_t candidate_length = directory_length +
                                  (size_t)add_separator + file_length;
        if (candidate_length >= 4096) {
            last_error = ENAMETOOLONG;
        } else {
            char candidate[4096];
            size_t cursor = 0;
            if (directory_length != 0) {
                memcpy(candidate, component, directory_length);
                cursor = directory_length;
                if (add_separator) {
                    candidate[cursor++] = '/';
                }
            }
            memcpy(candidate + cursor, file, file_length);
            candidate[cursor + file_length] = '\0';

            struct stat metadata;
            if (stat(candidate, &metadata) == 0) {
                if (!S_ISREG(metadata.st_mode)) {
                    /* Skip directories and other non-regular PATH entries.
                     * Do not continue here: the loop tail must advance to the
                     * next PATH component or a directory can cause a hang. */
                    last_error = EACCES;
                } else if (access(candidate, X_OK) != 0) {
                    access_error = errno;
                    if (*separator == '\0') {
                        break;
                    }
                } else {
                    return execve(candidate, arguments, environ);
                }
            } else if (errno == EACCES) {
                access_error = errno;
            } else {
                last_error = errno;
            }
        }
        if (*separator == '\0') {
            break;
        }
        component = separator + 1;
    }
    errno = access_error != 0 ? access_error : last_error;
    return -1;
}

int execve(const char *path, char *const arguments[], char *const environment[])
{
    return (int)posix_result(oct_execve(path, arguments, environment));
}

pid_t fork(void)
{
    /* Serialize libc metadata before the kernel snapshots this address space.
     * The child inherits these locks held and releases its private copies below. */
    oct_allocator_lock_acquire();
    oct_pthread_key_lock_acquire();
    oct_robust_head_lock_acquire();
#if defined(OCTAGRAM_SHARED)
    oct_runtime_state_lock_acquire();
#endif
    int64_t raw_result = oct_fork();
    __sync_lock_release(&oct_robust_head_lock);
    __sync_lock_release(&oct_pthread_key_lock);
    __sync_lock_release(&oct_allocator_lock);
#if defined(OCTAGRAM_SHARED)
    __sync_lock_release(&oct_runtime_state_lock);
#endif
    return (pid_t)posix_result(raw_result);
}

FILE *freopen(const char *path, const char *mode, FILE *stream)
{
    (void)path;
    (void)mode;
    (void)stream;
    errno = ENOSYS;
    return NULL;
}

#define OCT_MAX_ATEXIT_HANDLERS 64

/* Satu daftar untuk atexit dan ABI C++ (__cxa_atexit) agar urutannya tetap
 * terbalik sesuai urutan registrasi. */
typedef struct {
    void (*function)(void *);
    void *argument;
    void *dso;
    void (*plain)(void);
} OctExitHandler;

static OctExitHandler oct_atexit_handlers[OCT_MAX_ATEXIT_HANDLERS];
static unsigned int oct_atexit_count;
static int oct_atexit_running;

int __cxa_atexit(void (*function)(void *), void *argument, void *dso)
{
    if (function == NULL || oct_atexit_count >= OCT_MAX_ATEXIT_HANDLERS) {
        return -1;
    }
    OctExitHandler *handler = &oct_atexit_handlers[oct_atexit_count++];
    handler->function = function;
    handler->argument = argument;
    handler->dso = dso;
    handler->plain = NULL;
    return 0;
}

void __cxa_finalize(void *dso)
{
    /* Jalankan terbalik; entri yang cocok dikosongkan agar tidak berjalan dua
     * kali saat exit() atau dlclose() berikutnya. */
    for (unsigned int index = oct_atexit_count; index != 0; --index) {
        OctExitHandler *handler = &oct_atexit_handlers[index - 1];
        if (handler->plain == NULL && handler->function == NULL) {
            continue;
        }
        if (dso != NULL && handler->dso != dso) {
            continue;
        }
        void (*plain)(void) = handler->plain;
        void (*function)(void *) = handler->function;
        void *argument = handler->argument;
        handler->plain = NULL;
        handler->function = NULL;
        if (plain != NULL) {
            plain();
        } else {
            function(argument);
        }
    }
}

int atexit(void (*function)(void))
{
    if (function == NULL || oct_atexit_count >= OCT_MAX_ATEXIT_HANDLERS) {
        return -1;
    }
    OctExitHandler *handler = &oct_atexit_handlers[oct_atexit_count++];
    handler->plain = function;
    handler->function = NULL;
    handler->argument = NULL;
    handler->dso = NULL;
    return 0;
}

__attribute__((noreturn)) void exit(int status)
{
    /* Destruktor thread_local milik thread utama berjalan saat exit(). */
    oct_thread_run_tls_destructors();
    /* Handler dijalankan terbalik; exit() dari dalam handler tidak mengulang. */
    if (!oct_atexit_running) {
        oct_atexit_running = 1;
        __cxa_finalize(NULL);
    }
    oct_exit(status);
    for (;;) {
        __asm__ volatile ("pause");
    }
}

__attribute__((noreturn)) void abort(void)
{
    oct_exit(127);
    for (;;) {
        __asm__ volatile ("pause");
    }
}

/* --- Kompatibilitas POSIX untuk runtime Python/Java --- */

uid_t getuid(void)
{
    return (uid_t)oct_getuid();
}

gid_t getgid(void)
{
    return (gid_t)oct_getgid();
}

uid_t geteuid(void)
{
    return (uid_t)oct_geteuid();
}

gid_t getegid(void)
{
    return (gid_t)oct_getegid();
}

int uname(struct utsname *name)
{
    if (name == NULL) {
        errno = EFAULT;
        return -1;
    }
    return (int)posix_result(oct_uname((struct oct_utsname *)name));
}

static uintptr_t program_break;

static int refresh_program_break(void)
{
    int64_t current = oct_brk(0);
    if (current < 0) {
        errno = (int)-current;
        return -1;
    }
    program_break = (uintptr_t)current;
    return 0;
}

int brk(void *address)
{
    if (program_break == 0 && refresh_program_break() != 0) {
        return -1;
    }
    int64_t result = oct_brk((uint64_t)(uintptr_t)address);
    if (result < 0) {
        errno = (int)-result;
        return -1;
    }
    program_break = (uintptr_t)result;
    if ((uintptr_t)address > program_break) {
        errno = ENOMEM;
        return -1;
    }
    return 0;
}

void *sbrk(intptr_t increment)
{
    if (program_break == 0 && refresh_program_break() != 0) {
        return (void *)-1;
    }
    uintptr_t previous = program_break;
    if (increment == 0) {
        return (void *)previous;
    }
    if (increment > 0) {
        uintptr_t requested = previous + (uintptr_t)increment;
        if (requested < previous) {
            errno = ENOMEM;
            return (void *)-1;
        }
        int64_t result = oct_brk(requested);
        if (result < 0) {
            errno = (int)-result;
            return (void *)-1;
        }
        if ((uintptr_t)result != requested) {
            errno = ENOMEM;
            return (void *)-1;
        }
        program_break = requested;
        return (void *)previous;
    }
    /* Kernel belum menyusutkan heap; break yang dilaporkan kernel dipakai. */
    uintptr_t requested = previous - (uintptr_t)(-increment);
    int64_t result = oct_brk(requested);
    if (result < 0) {
        errno = (int)-result;
        return (void *)-1;
    }
    program_break = (uintptr_t)result;
    return (void *)previous;
}

int gettimeofday(struct timeval *time_value, void *timezone)
{
    (void)timezone;
    if (time_value == NULL) {
        errno = EFAULT;
        return -1;
    }
    return (int)posix_result(oct_gettimeofday((struct oct_timeval *)time_value));
}

int nanosleep(const struct timespec *request, struct timespec *remaining)
{
    (void)remaining;
    if (request == NULL) {
        errno = EFAULT;
        return -1;
    }
    return (int)posix_result(oct_nanosleep((const struct oct_timespec *)request));
}

unsigned int sleep(unsigned int seconds)
{
    struct timespec request;
    request.tv_sec = (time_t)seconds;
    request.tv_nsec = 0;
    if (nanosleep(&request, NULL) == 0) {
        return 0;
    }
    return errno == EINTR ? seconds : 0;
}

int usleep(useconds_t microseconds)
{
    struct timespec request;
    request.tv_sec = (time_t)(microseconds / 1000000u);
    request.tv_nsec = (long)(microseconds % 1000000u) * 1000L;
    return nanosleep(&request, NULL);
}

sighandler_t signal(int signal_number, sighandler_t handler)
{
    return (sighandler_t)(intptr_t)posix_result(
        oct_signal(signal_number, (void *)(uintptr_t)handler));
}

/* Dipanggil saat handler selesai: meminta kernel memulihkan konteks yang
   disimpan di stack sebelum handler dijalankan. */
__attribute__((naked, used)) static void oct_signal_restorer(void)
{
    __asm__ volatile("mov $113, %rax\n\tint $0x80\n\tud2");
}

int sigaction(int signal_number, const struct sigaction *action,
              struct sigaction *old_action)
{
    if (action == NULL) {
        return (int)posix_result(oct_sigaction(signal_number, NULL, old_action));
    }
    struct sigaction copy = *action;
    if (copy.sa_restorer == NULL) {
        copy.sa_restorer = oct_signal_restorer;
    }
    return (int)posix_result(oct_sigaction(signal_number, &copy, old_action));
}

int sigprocmask(int how, const sigset_t *set, sigset_t *old_set)
{
    return (int)posix_result(oct_sigprocmask(how, set, old_set));
}

int kill(pid_t pid, int signal_number)
{
    return (int)posix_result(oct_kill(pid, signal_number));
}

int sigpending(sigset_t *set)
{
    return (int)posix_result(oct_sigpending(set));
}

int sigsuspend(const sigset_t *mask)
{
    return (int)posix_result(oct_sigsuspend(mask));
}

int pthread_sigmask(int how, const sigset_t *set, sigset_t *old_set)
{
    return sigprocmask(how, set, old_set);
}

int sigaltstack(const stack_t *new_stack, stack_t *old_stack)
{
    return (int)posix_result(oct_sigaltstack(new_stack, old_stack));
}

int sigwait(const sigset_t *set, int *signal_number)
{
    if (set == NULL || signal_number == NULL) {
        errno = EFAULT;
        return -1;
    }
    sigset_t pending = 0;
    if (sigpending(&pending) != 0) {
        return -1;
    }
    sigset_t ready = pending & *set;
    for (int number = 1; number < NSIG; ++number) {
        if ((ready & ((sigset_t)1 << (number - 1))) != 0) {
            *signal_number = number;
            return 0;
        }
    }
    errno = EAGAIN;
    return -1;
}

int sigwaitinfo(const sigset_t *set, siginfo_t *info)
{
    int signal_number = 0;
    if (sigwait(set, &signal_number) != 0) {
        return -1;
    }
    if (info != NULL) {
        memset(info, 0, sizeof(*info));
        info->si_signo = signal_number;
    }
    return signal_number;
}

int sigtimedwait(const sigset_t *set, siginfo_t *info,
                 const struct timespec *timeout)
{
    (void)timeout;
    return sigwaitinfo(set, info);
}

int raise(int signal_number)
{
    return kill(getpid(), signal_number);
}

int poll(struct pollfd *fds, nfds_t nfds, int timeout)
{
    return (int)posix_result(oct_poll((struct oct_pollfd *)fds, (uint64_t)nfds, (int64_t)timeout));
}

int select(int nfds, fd_set *readfds, fd_set *writefds,
           fd_set *exceptfds, struct timeval *timeout)
{
    if (nfds < 0 || nfds > FD_SETSIZE) {
        errno = EINVAL;
        return -1;
    }
    return (int)posix_result(oct_select((uint64_t)(unsigned int)nfds,
                                        readfds, writefds, exceptfds,
                                        (struct oct_timeval *)timeout));
}

int setsockopt(int socket_fd, int level, int option_name,
               const void *option_value, socklen_t option_length)
{
    return (int)posix_result(oct_setsockopt(socket_fd, level, option_name,
                                            option_value, option_length));
}

int getsockopt(int socket_fd, int level, int option_name,
               void *option_value, socklen_t *option_length)
{
    return (int)posix_result(oct_getsockopt(socket_fd, level, option_name,
                                            option_value, option_length));
}

int putenv(char *entry)
{
    if (entry == NULL) {
        errno = EINVAL;
        return -1;
    }
    char *equals = strchr(entry, '=');
    if (equals == NULL) {
        return unsetenv(entry);
    }
    size_t name_length = (size_t)(equals - entry);
    if (name_length == 0 || name_length > 255) {
        errno = EINVAL;
        return -1;
    }
    char name[256];
    memcpy(name, entry, name_length);
    name[name_length] = '\0';
    /* setenv menyalin string, sehingga buffer pemanggil tetap boleh berubah. */
    return setenv(name, equals + 1, 1);
}

pid_t getppid(void)
{
    return (pid_t)oct_getppid();
}

int fsync(int fd)
{
    return (int)posix_result(oct_fsync(fd));
}

int fdatasync(int fd)
{
    return (int)posix_result(oct_fdatasync(fd));
}

ssize_t pread(int fd, void *buffer, size_t count, off_t offset)
{
    if (buffer == NULL && count != 0) {
        errno = EFAULT;
        return -1;
    }
    if (count > OCT_MAX_SYSCALL_IO) {
        count = OCT_MAX_SYSCALL_IO;
    }
    return (ssize_t)posix_result(oct_pread(fd, buffer, count, (uint64_t)offset));
}

ssize_t pwrite(int fd, const void *buffer, size_t count, off_t offset)
{
    if (buffer == NULL && count != 0) {
        errno = EFAULT;
        return -1;
    }
    if (count > OCT_MAX_SYSCALL_IO) {
        count = OCT_MAX_SYSCALL_IO;
    }
    return (ssize_t)posix_result(oct_pwrite(fd, buffer, count, (uint64_t)offset));
}

ssize_t readv(int fd, const struct iovec *vectors, int vector_count)
{
    if (vectors == NULL || vector_count < 0) {
        errno = vectors == NULL && vector_count != 0 ? EFAULT : EINVAL;
        return -1;
    }
    return (ssize_t)posix_result(oct_readv(fd, vectors, (uint64_t)vector_count));
}

ssize_t writev(int fd, const struct iovec *vectors, int vector_count)
{
    if (vectors == NULL || vector_count < 0) {
        errno = vectors == NULL && vector_count != 0 ? EFAULT : EINVAL;
        return -1;
    }
    return (ssize_t)posix_result(oct_writev(fd, vectors, (uint64_t)vector_count));
}

int dup3(int source_fd, int target_fd, int flags)
{
    return (int)posix_result(oct_dup3(source_fd, target_fd, flags));
}

int truncate(const char *path, off_t length)
{
    if (path == NULL) {
        errno = EFAULT;
        return -1;
    }
    int fd = open(path, O_WRONLY);
    if (fd < 0) {
        return -1;
    }
    int result = ftruncate(fd, length);
    int close_result = close(fd);
    if (result != 0) {
        return result;
    }
    return close_result;
}

int close_range(unsigned int first, unsigned int last, int flags)
{
    if (last < first) {
        errno = EINVAL;
        return -1;
    }
    if (flags != 0) {
        errno = ENOTSUP;
        return -1;
    }
    for (unsigned int fd = first; fd <= last; ++fd) {
        int result = close((int)fd);
        if (result != 0 && errno != EBADF) {
            return -1;
        }
        if (fd == last) {
            break;
        }
    }
    return 0;
}

ssize_t preadv(int fd, const struct iovec *vectors, int vector_count,
               off_t offset)
{
    if (vectors == NULL || vector_count < 0) {
        errno = vectors == NULL && vector_count != 0 ? EFAULT : EINVAL;
        return -1;
    }
    ssize_t total = 0;
    for (int index = 0; index < vector_count; ++index) {
        if (vectors[index].iov_len == 0) {
            continue;
        }
        ssize_t result = pread(fd, vectors[index].iov_base,
                               vectors[index].iov_len, offset + total);
        if (result < 0) {
            return result;
        }
        total += result;
        if ((size_t)result != vectors[index].iov_len) {
            break;
        }
    }
    return total;
}

ssize_t pwritev(int fd, const struct iovec *vectors, int vector_count,
                off_t offset)
{
    if (vectors == NULL || vector_count < 0) {
        errno = vectors == NULL && vector_count != 0 ? EFAULT : EINVAL;
        return -1;
    }
    ssize_t total = 0;
    for (int index = 0; index < vector_count; ++index) {
        if (vectors[index].iov_len == 0) {
            continue;
        }
        ssize_t result = pwrite(fd, vectors[index].iov_base,
                                vectors[index].iov_len, offset + total);
        if (result < 0) {
            return result;
        }
        total += result;
        if ((size_t)result != vectors[index].iov_len) {
            break;
        }
    }
    return total;
}

ssize_t preadv2(int fd, const struct iovec *vectors, int vector_count,
                off_t offset, int flags)
{
    if (flags != 0) {
        errno = ENOTSUP;
        return -1;
    }
    if (offset == (off_t)-1) {
        return readv(fd, vectors, vector_count);
    }
    if (offset < 0) {
        errno = EINVAL;
        return -1;
    }
    return preadv(fd, vectors, vector_count, offset);
}

ssize_t pwritev2(int fd, const struct iovec *vectors, int vector_count,
                 off_t offset, int flags)
{
    if (flags != 0) {
        errno = ENOTSUP;
        return -1;
    }
    if (offset == (off_t)-1) {
        return writev(fd, vectors, vector_count);
    }
    if (offset < 0) {
        errno = EINVAL;
        return -1;
    }
    return pwritev(fd, vectors, vector_count, offset);
}

int getrlimit(int resource, struct rlimit *limit)
{
    if (limit == NULL) {
        errno = EFAULT;
        return -1;
    }
    (void)resource;
    limit->rlim_cur = RLIM_INFINITY;
    limit->rlim_max = RLIM_INFINITY;
    int64_t result = oct_getrlimit(resource, limit);
    return (int)posix_result(result);
}

int setrlimit(int resource, const struct rlimit *limit)
{
    if (limit == NULL) {
        errno = EFAULT;
        return -1;
    }
    return (int)posix_result(oct_setrlimit(resource, limit));
}

int getrusage(int who, struct rusage *usage)
{
    if (usage == NULL) {
        errno = EFAULT;
        return -1;
    }
    memset(usage, 0, sizeof(*usage));
    return (int)posix_result(oct_getrusage(who, usage));
}

clock_t times(struct tms *process_times)
{
    if (process_times == NULL) {
        return (clock_t)posix_result(oct_times(NULL));
    }
    memset(process_times, 0, sizeof(*process_times));
    int64_t result = oct_times(process_times);
    return (clock_t)posix_result(result);
}

/* ---------- dynamic loading (dlopen/dlsym/dlclose) ---------- */

/* Diisi oleh loader dinamis saat startup; NULL pada program statis. */
void *(*__oct_dlopen_hook)(const char *path);
void *(*__oct_dlsym_hook)(void *handle, const char *symbol);
void *(*__oct_dlsym_next_hook)(const char *symbol, uintptr_t caller);
int (*__oct_dlclose_hook)(void *handle);

static char oct_dl_error_buffer[160];
static int oct_dl_error_pending;

static void *oct_dl_error(const char *message)
{
    size_t length = strlen(message);
    if (length >= sizeof(oct_dl_error_buffer)) {
        length = sizeof(oct_dl_error_buffer) - 1;
    }
    memcpy(oct_dl_error_buffer, message, length);
    oct_dl_error_buffer[length] = '\0';
    oct_dl_error_pending = 1;
    return NULL;
}

/* Weak: TinyCC menyediakan stub dlopen sendiri saat CONFIG_TCC_STATIC. */
__attribute__((weak)) char *dlerror(void)
{
    if (!oct_dl_error_pending) {
        return NULL;
    }
    oct_dl_error_pending = 0;
    return oct_dl_error_buffer;
}

__attribute__((weak)) void *dlopen(const char *path, int flags)
{
    if (path == NULL || path[0] == '\0') {
        return oct_dl_error("invalid library path");
    }
    if ((flags & ~(RTLD_LAZY | RTLD_NOW | RTLD_LOCAL | RTLD_GLOBAL)) != 0) {
        return oct_dl_error("invalid dlopen flags");
    }
    if (__oct_dlopen_hook == NULL) {
        return oct_dl_error("dynamic loading is unavailable");
    }
    void *handle = __oct_dlopen_hook(path);
    if (handle == NULL) {
        return oct_dl_error("cannot load shared library");
    }
    return handle;
}

__attribute__((weak)) void *dlsym(void *handle, const char *symbol)
{
    if (symbol == NULL || symbol[0] == '\0') {
        return oct_dl_error("invalid dlsym arguments");
    }
    if (handle == RTLD_NEXT) {
        if (__oct_dlsym_next_hook == NULL) {
            return oct_dl_error("dynamic loading is unavailable");
        }
        void *next = __oct_dlsym_next_hook(
            symbol, (uintptr_t)__builtin_return_address(0));
        if (next == NULL) {
            return oct_dl_error("symbol not found");
        }
        return next;
    }
    if (__oct_dlsym_hook == NULL) {
        return oct_dl_error("dynamic loading is unavailable");
    }
    void *address = __oct_dlsym_hook(handle, symbol);
    if (address == NULL) {
        return oct_dl_error("symbol not found");
    }
    return address;
}

__attribute__((weak)) int dlclose(void *handle)
{
    if (handle == NULL) {
        oct_dl_error("invalid handle");
        return -1;
    }
    if (__oct_dlclose_hook == NULL) {
        oct_dl_error("dynamic loading is unavailable");
        return -1;
    }
    if (__oct_dlclose_hook(handle) != 0) {
        oct_dl_error("cannot unload shared library");
        return -1;
    }
    return 0;
}

int setuid(uid_t uid)
{
    int64_t result = oct_setuid((uint64_t)uid);
    return result < 0 ? (int)posix_result(result) : 0;
}

int seteuid(uid_t uid)
{
    int64_t result = oct_seteuid((uint64_t)uid);
    return result < 0 ? (int)posix_result(result) : 0;
}

int setgid(gid_t gid)
{
    int64_t result = oct_setgid((uint64_t)gid);
    return result < 0 ? (int)posix_result(result) : 0;
}

int setegid(gid_t gid)
{
    int64_t result = oct_setegid((uint64_t)gid);
    return result < 0 ? (int)posix_result(result) : 0;
}

int capget(pid_t pid, uint64_t *mask)
{
    if (mask == NULL) {
        errno = EINVAL;
        return -1;
    }
    int64_t result = oct_capget(pid < 0 ? 0 : (uint64_t)pid, mask);
    return result < 0 ? (int)posix_result(result) : 0;
}

int capset(pid_t pid, uint64_t mask)
{
    int64_t result = oct_capset(pid < 0 ? 0 : (uint64_t)pid, mask);
    return result < 0 ? (int)posix_result(result) : 0;
}

int setreuid(uid_t real, uid_t effective)
{
    return real == 0 && effective == 0 ? 0 : (errno = EPERM, -1);
}

int setregid(gid_t real, gid_t effective)
{
    return real == 0 && effective == 0 ? 0 : (errno = EPERM, -1);
}

int setresuid(uid_t real, uid_t effective, uid_t saved)
{
    return real == 0 && effective == 0 && saved == 0
               ? 0
               : (errno = EPERM, -1);
}

int setresgid(gid_t real, gid_t effective, gid_t saved)
{
    return real == 0 && effective == 0 && saved == 0
               ? 0
               : (errno = EPERM, -1);
}

int getresuid(uid_t *real, uid_t *effective, uid_t *saved)
{
    if (real == NULL || effective == NULL || saved == NULL) {
        errno = EFAULT;
        return -1;
    }
    *real = 0;
    *effective = 0;
    *saved = 0;
    return 0;
}

int getresgid(gid_t *real, gid_t *effective, gid_t *saved)
{
    if (real == NULL || effective == NULL || saved == NULL) {
        errno = EFAULT;
        return -1;
    }
    *real = 0;
    *effective = 0;
    *saved = 0;
    return 0;
}

int getgroups(int size, gid_t *groups)
{
    if (size == 0) {
        return 1;
    }
    if (size < 0 || groups == NULL) {
        errno = size < 0 ? EINVAL : EFAULT;
        return -1;
    }
    if (size < 1) {
        errno = EINVAL;
        return -1;
    }
    groups[0] = 0;
    return 1;
}

int setgroups(size_t size, const gid_t *groups)
{
    if (size == 0) {
        return 0;
    }
    if (groups == NULL) {
        errno = EFAULT;
        return -1;
    }
    for (size_t index = 0; index < size; ++index) {
        if (groups[index] != 0) {
            errno = EPERM;
            return -1;
        }
    }
    return 0;
}

pid_t getpgid(pid_t pid)
{
    (void)pid;
    int64_t result = oct_getpgrp();
    return result < 0 ? (pid_t)posix_result(result) : (pid_t)result;
}

pid_t getpgrp(void)
{
    return getpid();
}

int setpgid(pid_t pid, pid_t pgid)
{
    return (int)posix_result(oct_setpgid((uint64_t)(pid < 0 ? 0 : pid),
                                         (uint64_t)(pgid < 0 ? 0 : pgid)));
}

pid_t getsid(pid_t pid)
{
    int64_t result = oct_getsid(pid < 0 ? 0 : (uint64_t)pid);
    return result < 0 ? (pid_t)posix_result(result) : (pid_t)result;
}

pid_t setsid(void)
{
    int64_t result = oct_setsid();
    return result < 0 ? (pid_t)posix_result(result) : (pid_t)result;
}

#define OCT_PRIORITY_MIN (-20)
#define OCT_PRIORITY_MAX 19

int sched_setscheduler(pid_t pid, int policy, const struct sched_param *param)
{
    if (param == NULL) {
        errno = EINVAL;
        return -1;
    }
    int64_t result = oct_sched_setscheduler(pid < 0 ? 0 : (uint64_t)pid,
                                            (int64_t)policy, param);
    return result < 0 ? (int)posix_result(result) : 0;
}

int sched_getscheduler(pid_t pid)
{
    int64_t result = oct_sched_getscheduler(pid < 0 ? 0 : (uint64_t)pid);
    return result < 0 ? (int)posix_result(result) : (int)result;
}

int sched_setparam(pid_t pid, const struct sched_param *param)
{
    if (param == NULL) {
        errno = EINVAL;
        return -1;
    }
    int64_t result = oct_sched_setparam(pid < 0 ? 0 : (uint64_t)pid, param);
    return result < 0 ? (int)posix_result(result) : 0;
}

int sched_getparam(pid_t pid, struct sched_param *param)
{
    if (param == NULL) {
        errno = EINVAL;
        return -1;
    }
    int64_t result = oct_sched_getparam(pid < 0 ? 0 : (uint64_t)pid, param);
    return result < 0 ? (int)posix_result(result) : 0;
}

int sched_get_priority_max(int policy)
{
    if (policy == SCHED_FIFO || policy == SCHED_RR) {
        return 99;
    }
    if (policy == SCHED_OTHER) {
        return 0;
    }
    errno = EINVAL;
    return -1;
}

int sched_get_priority_min(int policy)
{
    if (policy == SCHED_FIFO || policy == SCHED_RR) {
        return 1;
    }
    if (policy == SCHED_OTHER) {
        return 0;
    }
    errno = EINVAL;
    return -1;
}

int sched_yield(void)
{
    int64_t result = oct_yield();
    return result < 0 ? (int)posix_result(result) : 0;
}

int getpriority(int which, id_t who)
{
    /* Kernel mengembalikan nice yang dibias +20 agar selalu non-negatif. */
    int64_t biased = oct_getpriority((int64_t)which, (uint64_t)(int64_t)who);
    if (biased < 0) {
        return (int)posix_result(biased);
    }
    /* POSIX mengembalikan 20 - nice agar nilainya selalu positif. */
    return (int)(40 - biased);
}

int setpriority(int which, id_t who, int priority)
{
    int64_t result = oct_setpriority((int64_t)which, (uint64_t)(int64_t)who,
                                     (int64_t)priority);
    return result < 0 ? (int)posix_result(result) : 0;
}

int nice(int increment)
{
    int64_t biased = oct_getpriority(PRIO_PROCESS, 0);
    if (biased < 0) {
        return (int)posix_result(biased);
    }
    int64_t target = (biased + OCT_PRIORITY_MIN) + (int64_t)increment;
    if (target < OCT_PRIORITY_MIN) {
        target = OCT_PRIORITY_MIN;
    } else if (target > OCT_PRIORITY_MAX) {
        target = OCT_PRIORITY_MAX;
    }
    int64_t result = oct_setpriority(PRIO_PROCESS, 0, target);
    if (result < 0) {
        return (int)posix_result(result);
    }
    return (int)(20 - target);
}

int chown(const char *path, uid_t owner, gid_t group)
{
    (void)path;
    if (owner != 0 || group != 0) {
        errno = EPERM;
        return -1;
    }
    return 0;
}

int lchown(const char *path, uid_t owner, gid_t group)
{
    return chown(path, owner, group);
}

int fchown(int fd, uid_t owner, gid_t group)
{
    (void)fd;
    if (owner != 0 || group != 0) {
        errno = EPERM;
        return -1;
    }
    return 0;
}

unsigned int alarm(unsigned int seconds)
{
    (void)seconds;
    return 0;
}

int pause(void)
{
    sigset_t set = 0;
    errno = 0;
    sigsuspend(&set);
    return -1;
}

char *ctermid(char *name)
{
    static char cterm_name[] = "/dev/tty";
    if (name != NULL) {
        memcpy(name, cterm_name, sizeof(cterm_name));
        return name;
    }
    return cterm_name;
}

char *ttyname(int fd)
{
    static char name[] = "/dev/tty";
    return isatty(fd) ? name : NULL;
}

int ttyname_r(int fd, char *buffer, size_t size)
{
    if (buffer == NULL) {
        return EFAULT;
    }
    if (!isatty(fd)) {
        return ENOTTY;
    }
    static const char name[] = "/dev/tty";
    if (size < sizeof(name)) {
        return ERANGE;
    }
    memcpy(buffer, name, sizeof(name));
    return 0;
}

long fpathconf(int fd, int name)
{
    (void)fd;
    (void)name;
    errno = 0;
    return -1;
}

long pathconf(const char *path, int name)
{
    (void)path;
    (void)name;
    errno = 0;
    return -1;
}

int faccessat(int dirfd, const char *path, int mode, int flags)
{
    (void)dirfd;
    (void)flags;
    return access(path, mode);
}

int fchmodat(int dirfd, const char *path, mode_t mode, int flags)
{
    (void)dirfd;
    (void)flags;
    return chmod(path, mode);
}

int fchownat(int dirfd, const char *path, uid_t owner, gid_t group, int flags)
{
    (void)dirfd;
    (void)flags;
    return chown(path, owner, group);
}

int utimensat(int dirfd, const char *path, const struct timespec *times, int flags)
{
    (void)dirfd;
    (void)path;
    (void)times;
    (void)flags;
    errno = ENOSYS;
    return -1;
}

int futimens(int fd, const struct timespec times[2])
{
    (void)fd;
    (void)times;
    errno = ENOSYS;
    return -1;
}

int _exit(int status)
{
    exit(status);
}

void sync(void)
{
}

int gethostname(char *name, size_t size)
{
    if (name == NULL) {
        errno = EFAULT;
        return -1;
    }
    static const char hostname[] = "octagram";
    if (size < sizeof(hostname)) {
        errno = ERANGE;
        return -1;
    }
    memcpy(name, hostname, sizeof(hostname));
    return 0;
}

int system(const char *command)
{
    (void)command;
    errno = ENOSYS;
    return -1;
}

int clock_getres(clockid_t clock_id, struct timespec *resolution)
{
    if (resolution == NULL) {
        errno = EFAULT;
        return -1;
    }
    return (int)posix_result(
        oct_clock_getres(clock_id, (struct oct_timespec *)resolution));
}

ssize_t getrandom(void *buffer, size_t length, unsigned int flags)
{
    if (buffer == NULL && length != 0) {
        errno = EFAULT;
        return -1;
    }
    unsigned char *bytes = buffer;
    size_t done = 0;
    while (done < length) {
        size_t chunk = length - done;
        if (chunk > 256) {
            chunk = 256;
        }
        int64_t result = oct_getrandom(bytes + done, chunk, flags);
        if (result < 0) {
            return done == 0 ? (ssize_t)posix_result(result) : (ssize_t)done;
        }
        if (result == 0) {
            break;
        }
        done += (size_t)result;
    }
    return (ssize_t)done;
}

int epoll_create1(int flags)
{
    return (int)posix_result(oct_epoll_create1(flags));
}

int epoll_ctl(int epfd, int operation, int fd, struct epoll_event *event)
{
    return (int)posix_result(
        oct_epoll_ctl(epfd, operation, fd, (void *)event));
}

int epoll_wait(int epfd, struct epoll_event *events, int maxevents,
               int timeout)
{
    return (int)posix_result(
        oct_epoll_wait(epfd, (void *)events, maxevents, timeout));
}

int eventfd(unsigned int initial_value, int flags)
{
    return (int)posix_result(oct_eventfd2(initial_value, flags));
}

int eventfd_read(int fd, uint64_t *value)
{
    if (value == NULL) {
        errno = EFAULT;
        return -1;
    }
    ssize_t result = read(fd, value, sizeof(*value));
    return result == (ssize_t)sizeof(*value) ? 0 : -1;
}

int eventfd_write(int fd, uint64_t value)
{
    ssize_t result = write(fd, &value, sizeof(value));
    return result == (ssize_t)sizeof(value) ? 0 : -1;
}

int timerfd_create(clockid_t clock_id, int flags)
{
    return (int)posix_result(oct_timerfd_create(clock_id, flags));
}

int timerfd_settime(int fd, int flags, const struct itimerspec *new_value,
                    struct itimerspec *old_value)
{
    return (int)posix_result(
        oct_timerfd_settime(fd, flags, (const void *)new_value,
                            (void *)old_value));
}

int timerfd_gettime(int fd, struct itimerspec *current_value)
{
    return (int)posix_result(oct_timerfd_gettime(fd, (void *)current_value));
}

int socketpair(int domain, int socket_type, int protocol, int sv[2])
{
    return (int)posix_result(
        oct_socketpair(domain, socket_type, protocol, (void *)sv));
}

int clock_nanosleep(clockid_t clock_id, int flags,
                    const struct timespec *request, struct timespec *remaining)
{
    return (int)posix_result(oct_clock_nanosleep(
        clock_id, flags, (const struct oct_timespec *)request,
        (struct oct_timespec *)remaining));
}

/* ---------- locale dan konversi multibyte UTF-8 ---------- */

static struct lconv oct_locale_conv = {
    .decimal_point = ".",
    .thousands_sep = "",
    .grouping = "",
    .int_curr_symbol = "",
    .currency_symbol = "",
    .mon_decimal_point = "",
    .mon_thousands_sep = "",
    .mon_grouping = "",
    .positive_sign = "",
    .negative_sign = "",
    .int_frac_digits = 127,
    .frac_digits = 127,
    .p_cs_precedes = 127,
    .p_sep_by_space = 127,
    .n_cs_precedes = 127,
    .n_sep_by_space = 127,
    .p_sign_posn = 127,
    .n_sign_posn = 127,
    .int_p_cs_precedes = 127,
    .int_p_sep_by_space = 127,
    .int_n_cs_precedes = 127,
    .int_n_sep_by_space = 127,
    .int_p_sign_posn = 127,
    .int_n_sign_posn = 127
};

char *setlocale(int category, const char *locale)
{
    static char current[32] = "C";
    (void)category;
    if (locale == NULL) {
        return current;
    }
    if (locale[0] == '\0') {
        const char *environment = getenv("LC_ALL");
        if (environment == NULL || environment[0] == '\0') {
            environment = getenv("LC_CTYPE");
        }
        if (environment == NULL || environment[0] == '\0') {
            environment = getenv("LANG");
        }
        if (environment != NULL &&
            (strstr(environment, "UTF-8") != NULL ||
             strstr(environment, "utf8") != NULL ||
             strstr(environment, "UTF8") != NULL)) {
            strcpy(current, "C.UTF-8");
        } else {
            strcpy(current, "C");
        }
        return current;
    }
    if (strcmp(locale, "C") == 0 || strcmp(locale, "POSIX") == 0) {
        strcpy(current, "C");
        return current;
    }
    if (strcmp(locale, "C.UTF-8") == 0 || strcmp(locale, "C.utf8") == 0 ||
        strcmp(locale, "UTF-8") == 0 || strcmp(locale, "utf8") == 0 ||
        strcmp(locale, "en_US.UTF-8") == 0) {
        strcpy(current, "C.UTF-8");
        return current;
    }
    return NULL;
}

struct lconv *localeconv(void)
{
    return &oct_locale_conv;
}

/* 0 = berhasil, 1 = butuh byte tambahan, -1 = urutan tidak valid. */
static int oct_utf8_inspect(const unsigned char *bytes, int available,
                            uint32_t *code_point, int *length)
{
    if (available == 0) {
        return 1;
    }
    unsigned char first = bytes[0];
    if (first < 0x80) {
        *code_point = first;
        *length = 1;
        return 0;
    }
    int needed;
    uint32_t minimum;
    uint32_t value;
    if (first >= 0xC2 && first <= 0xDF) {
        needed = 2;
        minimum = 0x80;
        value = first & 0x1F;
    } else if (first >= 0xE0 && first <= 0xEF) {
        needed = 3;
        minimum = 0x800;
        value = first & 0x0F;
    } else if (first >= 0xF0 && first <= 0xF4) {
        needed = 4;
        minimum = 0x10000;
        value = first & 0x07;
    } else {
        return -1;
    }
    for (int index = 1; index < needed; ++index) {
        if (index >= available) {
            return 1;
        }
        unsigned char next = bytes[index];
        if ((next & 0xC0) != 0x80) {
            return -1;
        }
        value = (value << 6) | (next & 0x3F);
    }
    if (value < minimum || value > 0x10FFFF ||
        (value >= 0xD800 && value <= 0xDFFF)) {
        return -1;
    }
    *code_point = value;
    *length = needed;
    return 0;
}

size_t mbrtowc(wchar_t *restrict wide, const char *restrict input, size_t count,
               mbstate_t *restrict state)
{
    static mbstate_t fallback_state;
    if (state == NULL) {
        state = &fallback_state;
    }
    if (input == NULL) {
        memset(state, 0, sizeof(*state));
        return 0;
    }
    unsigned char buffer[8];
    int have = state->count;
    if (have < 0 || have > 4) {
        have = 0;
    }
    memcpy(buffer, state->bytes, (size_t)have);
    size_t added = 0;
    for (;;) {
        uint32_t code_point = 0;
        int length = 0;
        int status = oct_utf8_inspect(buffer, have + (int)added, &code_point,
                                      &length);
        if (status == 0) {
            size_t used = (size_t)(length - have);
            memset(state, 0, sizeof(*state));
            if (wide != NULL) {
                *wide = (wchar_t)code_point;
            }
            return code_point == 0 ? 0 : used;
        }
        if (status < 0) {
            memset(state, 0, sizeof(*state));
            errno = EILSEQ;
            return (size_t)-1;
        }
        if (added >= count) {
            state->count = have + (int)added;
            memcpy(state->bytes, buffer, (size_t)state->count);
            return (size_t)-2;
        }
        buffer[have + (int)added] = (unsigned char)input[added];
        ++added;
    }
}

static size_t oct_utf8_encode(uint32_t code_point, unsigned char *output)
{
    if (code_point <= 0x7F) {
        output[0] = (unsigned char)code_point;
        return 1;
    }
    if (code_point <= 0x7FF) {
        output[0] = (unsigned char)(0xC0 | (code_point >> 6));
        output[1] = (unsigned char)(0x80 | (code_point & 0x3F));
        return 2;
    }
    if (code_point <= 0xFFFF) {
        output[0] = (unsigned char)(0xE0 | (code_point >> 12));
        output[1] = (unsigned char)(0x80 | ((code_point >> 6) & 0x3F));
        output[2] = (unsigned char)(0x80 | (code_point & 0x3F));
        return 3;
    }
    output[0] = (unsigned char)(0xF0 | (code_point >> 18));
    output[1] = (unsigned char)(0x80 | ((code_point >> 12) & 0x3F));
    output[2] = (unsigned char)(0x80 | ((code_point >> 6) & 0x3F));
    output[3] = (unsigned char)(0x80 | (code_point & 0x3F));
    return 4;
}

size_t wcrtomb(char *restrict output, wchar_t wide, mbstate_t *restrict state)
{
    char scratch[4];
    (void)state;
    if (output == NULL) {
        output = scratch;
    }
    uint32_t code_point = (uint32_t)wide;
    if (code_point > 0x10FFFF ||
        (code_point >= 0xD800 && code_point <= 0xDFFF)) {
        errno = EILSEQ;
        return (size_t)-1;
    }
    return oct_utf8_encode(code_point, (unsigned char *)output);
}

size_t wcslen(const wchar_t *text)
{
    size_t length = 0;
    while (text[length] != 0) {
        ++length;
    }
    return length;
}

int wcscmp(const wchar_t *left, const wchar_t *right)
{
    while (*left != 0 && *left == *right) {
        ++left;
        ++right;
    }
    return (*left > *right) - (*left < *right);
}

int wcsncmp(const wchar_t *left, const wchar_t *right, size_t count)
{
    for (size_t index = 0; index < count; ++index) {
        if (left[index] != right[index]) {
            return (left[index] > right[index]) - (left[index] < right[index]);
        }
        if (left[index] == 0) {
            return 0;
        }
    }
    return 0;
}

wchar_t *wcscpy(wchar_t *destination, const wchar_t *source)
{
    wchar_t *result = destination;
    while ((*destination++ = *source++) != 0) {}
    return result;
}

wchar_t *wcsncpy(wchar_t *destination, const wchar_t *source, size_t count)
{
    size_t index = 0;
    for (; index < count && source[index] != 0; ++index) {
        destination[index] = source[index];
    }
    for (; index < count; ++index) {
        destination[index] = 0;
    }
    return destination;
}

wchar_t *wcschr(const wchar_t *text, wchar_t character)
{
    for (size_t index = 0;; ++index) {
        if (text[index] == character) {
            return (wchar_t *)(text + index);
        }
        if (text[index] == 0) {
            return NULL;
        }
    }
}

wchar_t *wcsrchr(const wchar_t *text, wchar_t character)
{
    const wchar_t *match = NULL;
    for (size_t index = 0;; ++index) {
        if (text[index] == character) {
            match = text + index;
        }
        if (text[index] == 0) {
            return (wchar_t *)match;
        }
    }
}

int wcscoll(const wchar_t *left, const wchar_t *right)
{
    return wcscmp(left, right);
}

long wcstol(const wchar_t *text, wchar_t **end, int base)
{
    char narrow[256];
    size_t index = 0;
    while (text[index] != 0 && index + 1 < sizeof(narrow)) {
        if (text[index] > 127) {
            errno = EILSEQ;
            if (end != NULL) {
                *end = (wchar_t *)text;
            }
            return 0;
        }
        narrow[index] = (char)text[index];
        ++index;
    }
    narrow[index] = 0;
    char *narrow_end = NULL;
    long value = strtol(narrow, &narrow_end, base);
    if (end != NULL) {
        *end = (wchar_t *)(text + (narrow_end - narrow));
    }
    return value;
}

wchar_t *wcstok(wchar_t *text, const wchar_t *delimiter, wchar_t **context)
{
    wchar_t *current = text != NULL ? text : (context != NULL ? *context : NULL);
    if (current == NULL) {
        return NULL;
    }
    while (*current != 0) {
        if (wcschr(delimiter, *current) == NULL) {
            break;
        }
        ++current;
    }
    if (*current == 0) {
        if (context != NULL) {
            *context = current;
        }
        return NULL;
    }
    wchar_t *result = current;
    while (*current != 0) {
        if (wcschr(delimiter, *current) != NULL) {
            *current++ = 0;
            if (context != NULL) {
                *context = current;
            }
            return result;
        }

        __attribute__((used)) wchar_t *wmemcpy(wchar_t *destination, const wchar_t *source, size_t count)
        {
            for (size_t index = 0; index < count; ++index) {
                destination[index] = source[index];
            }
            return destination;
        }

        __attribute__((used)) wchar_t *wmemmove(wchar_t *destination, const wchar_t *source, size_t count)
        {
            if (destination < source) {
                for (size_t index = 0; index < count; ++index) {
                    destination[index] = source[index];
                }
            } else {
                for (size_t index = count; index > 0; --index) {
                    destination[index - 1] = source[index - 1];
                }
            }
            return destination;
        }

        __attribute__((used)) int wmemcmp(const wchar_t *left, const wchar_t *right, size_t count)
        {
            for (size_t index = 0; index < count; ++index) {
                if (left[index] != right[index]) {
                    return (left[index] > right[index]) - (left[index] < right[index]);
                }
            }
            return 0;
        }

        __attribute__((used)) wchar_t *wmemchr(const wchar_t *memory, wchar_t character, size_t count)
        {
            for (size_t index = 0; index < count; ++index) {
                if (memory[index] == character) {
                    return (wchar_t *)(memory + index);
                }
            }
            return NULL;
        }
        ++current;
    }
    if (context != NULL) {
        *context = current;
    }
    return result;
}

size_t wcstombs(char *destination, const wchar_t *source, size_t count)
{
    mbstate_t state = {0};
    size_t used = 0;
    for (size_t index = 0; source[index] != 0; ++index) {
        char encoded[4];
        size_t length = wcrtomb(encoded, source[index], &state);
        if (length == (size_t)-1 || used + length > count) {
            return (size_t)-1;
        }
        if (destination != NULL) {
            memcpy(destination + used, encoded, length);
        }
        used += length;
    }
    if (destination != NULL && used < count) {
        destination[used] = 0;
    }
    return used;
}

size_t mbstowcs(wchar_t *destination, const char *source, size_t count)
{
    mbstate_t state = {0};
    size_t used = 0;
    size_t source_index = 0;
    while (source[source_index] != 0 && used < count) {
        wchar_t wide;
        size_t length = mbrtowc(&wide, source + source_index, 4, &state);
        if (length == (size_t)-1 || length == 0) {
            return (size_t)-1;
        }
        if (destination != NULL) {
            destination[used] = wide;
        }
        source_index += length;
        ++used;
    }
    if (destination != NULL && used < count) {
        destination[used] = 0;
    }
    return used;
}

size_t c32rtomb(char *restrict output, char32_t wide, mbstate_t *restrict state)
{
    return wcrtomb(output, (wchar_t)wide, state);
}

size_t c16rtomb(char *restrict output, char16_t unit, mbstate_t *restrict state)
{
    static mbstate_t fallback_state;
    if (state == NULL) {
        state = &fallback_state;
    }
    if (output == NULL) {
        state->pending = 0;
        return 1;
    }
    uint32_t value = unit;
    if (value >= 0xD800 && value <= 0xDBFF) {
        if (state->pending != 0) {
            state->pending = 0;
            errno = EILSEQ;
            return (size_t)-1;
        }
        state->pending = value;
        return 0;
    }
    if (value >= 0xDC00 && value <= 0xDFFF) {
        if (state->pending == 0) {
            errno = EILSEQ;
            return (size_t)-1;
        }
        uint32_t code_point = 0x10000 +
            ((state->pending - 0xD800) << 10) + (value - 0xDC00);
        state->pending = 0;
        return oct_utf8_encode(code_point, (unsigned char *)output);
    }
    if (state->pending != 0) {
        state->pending = 0;
        errno = EILSEQ;
        return (size_t)-1;
    }
    return oct_utf8_encode(value, (unsigned char *)output);
}

/* ---------- termios ---------- */

int tcgetattr(int fd, struct termios *termios_state)
{
    if (termios_state == NULL) {
        errno = EFAULT;
        return -1;
    }
    return ioctl(fd, TCGETS, termios_state) == 0 ? 0 : -1;
}

int tcsetattr(int fd, int action, const struct termios *termios_state)
{
    if (termios_state == NULL) {
        errno = EFAULT;
        return -1;
    }
    unsigned long request;
    switch (action) {
    case TCSANOW:
        request = TCSETS;
        break;
    case TCSADRAIN:
        request = TCSETSW;
        break;
    case TCSAFLUSH:
        request = TCSETSF;
        break;
    default:
        errno = EINVAL;
        return -1;
    }
    return ioctl(fd, request, (void *)termios_state) == 0 ? 0 : -1;
}

void cfmakeraw(struct termios *termios_state)
{
    if (termios_state == NULL) {
        return;
    }
    termios_state->c_iflag &=
        ~(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    termios_state->c_oflag &= ~OPOST;
    termios_state->c_lflag &= ~(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
    termios_state->c_cflag &= ~(CSIZE | PARENB);
    termios_state->c_cflag |= CS8;
    termios_state->c_cc[VMIN] = 1;
    termios_state->c_cc[VTIME] = 0;
}

speed_t cfgetispeed(const struct termios *termios_state)
{
    return termios_state == NULL ? 0 : termios_state->c_ispeed;
}

speed_t cfgetospeed(const struct termios *termios_state)
{
    return termios_state == NULL ? 0 : termios_state->c_ospeed;
}

int cfsetispeed(struct termios *termios_state, speed_t speed)
{
    if (termios_state == NULL) {
        errno = EINVAL;
        return -1;
    }
    termios_state->c_ispeed = speed;
    return 0;
}

int cfsetospeed(struct termios *termios_state, speed_t speed)
{
    if (termios_state == NULL) {
        errno = EINVAL;
        return -1;
    }
    termios_state->c_ospeed = speed;
    return 0;
}


/* Database akun: /etc/passwd dan /etc/group. Getter non-reentrant memakai
   penyimpanan statis seperti POSIX; varian _r memakai buffer pemanggil. */

#define OCTAGRAM_PASSWD_PATH "/etc/passwd"
#define OCTAGRAM_GROUP_PATH "/etc/group"
#define OCTAGRAM_ACCOUNT_LINE_LENGTH 256

static struct passwd octagram_passwd_entry;
static char octagram_passwd_buffer[OCTAGRAM_ACCOUNT_LINE_LENGTH];
static FILE *octagram_passwd_stream;

static struct group octagram_group_entry;
static char octagram_group_buffer[OCTAGRAM_ACCOUNT_LINE_LENGTH];
static FILE *octagram_group_stream;

static char octagram_group_placeholder_password[] = "x";

/* Pilih offset tabel pointer di akhir buffer agar tetap sejajar pointer. */
static size_t octagram_group_table_offset(const char *buffer, size_t size,
                                          size_t table_bytes)
{
    const size_t alignment = sizeof(char *);
    size_t offset = (size - table_bytes) & ~(alignment - 1);
    size_t misalignment =
        (size_t)(((uintptr_t)buffer + offset) & (alignment - 1));
    if (misalignment != 0) {
        offset -= alignment - misalignment;
    }
    return offset;
}

static int octagram_split_colon_fields(char *line, char **fields, int maximum)
{
    int count = 0;
    char *cursor = line;
    while (count < maximum) {
        char *separator = strchr(cursor, ':');
        fields[count++] = cursor;
        if (separator == NULL) {
            break;
        }
        *separator = '\0';
        cursor = separator + 1;
    }
    if (count > 0) {
        char *newline = strchr(fields[count - 1], '\n');
        if (newline != NULL) {
            *newline = '\0';
        }
    }
    return count;
}

static int octagram_parse_account_id(const char *text, unsigned long *value)
{
    char *end = NULL;
    if (text == NULL || text[0] == '\0') {
        return -1;
    }
    unsigned long parsed = strtoul(text, &end, 10);
    if (end == text || end == NULL || *end != '\0') {
        return -1;
    }
    *value = parsed;
    return 0;
}

/* Mengurai satu baris /etc/passwd yang sudah disalin ke buffer. */
static int octagram_read_passwd_line(char *buffer, struct passwd *entry)
{
    char *fields[7];
    if (octagram_split_colon_fields(buffer, fields, 7) != 7) {
        return -1;
    }
    unsigned long uid;
    unsigned long gid;
    if (octagram_parse_account_id(fields[2], &uid) != 0 ||
        octagram_parse_account_id(fields[3], &gid) != 0) {
        return -1;
    }
    entry->pw_name = fields[0];
    entry->pw_passwd = fields[1];
    entry->pw_uid = (uid_t)uid;
    entry->pw_gid = (gid_t)gid;
    entry->pw_gecos = fields[4];
    entry->pw_dir = fields[5];
    entry->pw_shell = fields[6];
    return 0;
}

static int octagram_lookup_passwd(const char *name, int by_uid, uid_t uid,
                                  struct passwd *entry, char *buffer,
                                  size_t size, struct passwd **result)
{
    *result = NULL;
    FILE *stream = fopen(OCTAGRAM_PASSWD_PATH, "r");
    if (stream == NULL) {
        int error = errno;
        return error != 0 ? error : ENOENT;
    }
    int status = 0;
    char line[OCTAGRAM_ACCOUNT_LINE_LENGTH];
    while (fgets(line, sizeof(line), stream) != NULL) {
        size_t length = strlen(line);
        if (length == 0 || line[0] == '#') {
            continue;
        }
        if (length + 1 > size) {
            status = ERANGE;
            break;
        }
        memcpy(buffer, line, length + 1);
        struct passwd candidate;
        if (octagram_read_passwd_line(buffer, &candidate) != 0) {
            continue;
        }
        int matches = by_uid
                          ? candidate.pw_uid == uid
                          : (name != NULL &&
                             strcmp(candidate.pw_name, name) == 0);
        if (!matches) {
            continue;
        }
        *entry = candidate;
        *result = entry;
        break;
    }
    fclose(stream);
    return status;
}

int getpwnam_r(const char *name, struct passwd *entry, char *buffer,
               size_t size, struct passwd **result)
{
    if (name == NULL || entry == NULL || buffer == NULL || result == NULL) {
        return EINVAL;
    }
    return octagram_lookup_passwd(name, 0, 0, entry, buffer, size, result);
}

int getpwuid_r(uid_t uid, struct passwd *entry, char *buffer, size_t size,
               struct passwd **result)
{
    if (entry == NULL || buffer == NULL || result == NULL) {
        return EINVAL;
    }
    return octagram_lookup_passwd(NULL, 1, uid, entry, buffer, size, result);
}

struct passwd *getpwnam(const char *name)
{
    if (name == NULL) {
        errno = EINVAL;
        return NULL;
    }
    struct passwd *result = NULL;
    int status = octagram_lookup_passwd(
        name, 0, 0, &octagram_passwd_entry, octagram_passwd_buffer,
        sizeof(octagram_passwd_buffer), &result);
    if (status != 0) {
        errno = status;
        return NULL;
    }
    errno = 0;
    return result;
}

struct passwd *getpwuid(uid_t uid)
{
    struct passwd *result = NULL;
    int status = octagram_lookup_passwd(
        NULL, 1, uid, &octagram_passwd_entry, octagram_passwd_buffer,
        sizeof(octagram_passwd_buffer), &result);
    if (status != 0) {
        errno = status;
        return NULL;
    }
    errno = 0;
    return result;
}

struct passwd *getpwent(void)
{
    if (octagram_passwd_stream == NULL) {
        octagram_passwd_stream = fopen(OCTAGRAM_PASSWD_PATH, "r");
        if (octagram_passwd_stream == NULL) {
            return NULL;
        }
    }
    char line[OCTAGRAM_ACCOUNT_LINE_LENGTH];
    while (fgets(line, sizeof(line), octagram_passwd_stream) != NULL) {
        size_t length = strlen(line);
        if (length == 0 || line[0] == '#' ||
            length + 1 > sizeof(octagram_passwd_buffer)) {
            continue;
        }
        memcpy(octagram_passwd_buffer, line, length + 1);
        if (octagram_read_passwd_line(octagram_passwd_buffer,
                                      &octagram_passwd_entry) == 0) {
            return &octagram_passwd_entry;
        }
    }
    return NULL;
}

void setpwent(void)
{
    if (octagram_passwd_stream != NULL) {
        rewind(octagram_passwd_stream);
    }
}

void endpwent(void)
{
    if (octagram_passwd_stream != NULL) {
        fclose(octagram_passwd_stream);
        octagram_passwd_stream = NULL;
    }
}

/* Mengurai satu baris /etc/group. Daftar anggota disimpan di akhir buffer
   agar tetap berada dalam penyimpanan pemanggil. 0 = berhasil, -1 = baris
   tidak valid, -2 = buffer tidak cukup. */
static int octagram_read_group_line(char *buffer, size_t size,
                                    struct group *entry)
{
    size_t text_length = strlen(buffer) + 1;
    char *fields[4];
    if (octagram_split_colon_fields(buffer, fields, 4) != 4) {
        return -1;
    }
    unsigned long gid;
    if (octagram_parse_account_id(fields[2], &gid) != 0) {
        return -1;
    }
    size_t member_count = 0;
    if (fields[3][0] != '\0') {
        member_count = 1;
        for (char *cursor = fields[3]; *cursor != '\0'; ++cursor) {
            if (*cursor == ',') {
                ++member_count;
            }
        }
    }
    size_t table_bytes = (member_count + 1) * sizeof(char *);
    if (size < table_bytes) {
        return -2;
    }
    size_t table_offset = octagram_group_table_offset(buffer, size, table_bytes);
    if (table_offset < text_length) {
        return -2;
    }
    char **members = (char **)(void *)(buffer + table_offset);
    size_t index = 0;
    char *cursor = fields[3];
    while (*cursor != '\0' && index < member_count) {
        members[index++] = cursor;
        char *separator = strchr(cursor, ',');
        if (separator == NULL) {
            break;
        }
        *separator = '\0';
        cursor = separator + 1;
    }
    while (index <= member_count) {
        members[index++] = NULL;
    }
    entry->gr_name = fields[0];
    entry->gr_passwd = fields[1];
    entry->gr_gid = (gid_t)gid;
    entry->gr_mem = members;
    return 0;
}

/* Cadangan ketika /etc/group belum ada: pakai grup utama dari /etc/passwd. */
static int octagram_lookup_primary_group(const char *name, int by_gid,
                                         gid_t gid, struct group *entry,
                                         char *buffer, size_t size,
                                         struct group **result)
{
    FILE *stream = fopen(OCTAGRAM_PASSWD_PATH, "r");
    if (stream == NULL) {
        int error = errno;
        return error != 0 ? error : ENOENT;
    }
    int status = 0;
    char line[OCTAGRAM_ACCOUNT_LINE_LENGTH];
    while (fgets(line, sizeof(line), stream) != NULL) {
        size_t length = strlen(line);
        if (length == 0 || line[0] == '#') {
            continue;
        }
        if (length + 1 > size) {
            status = ERANGE;
            break;
        }
        memcpy(buffer, line, length + 1);
        struct passwd candidate;
        if (octagram_read_passwd_line(buffer, &candidate) != 0) {
            continue;
        }
        int matches = by_gid
                          ? candidate.pw_gid == gid
                          : (name != NULL &&
                             strcmp(candidate.pw_name, name) == 0);
        if (!matches) {
            continue;
        }
        size_t name_length = strlen(candidate.pw_name) + 1;
        size_t table_bytes = sizeof(char *);
        if (size < table_bytes) {
            status = ERANGE;
            break;
        }
        size_t table_offset =
            octagram_group_table_offset(buffer, size, table_bytes);
        if (table_offset < name_length) {
            status = ERANGE;
            break;
        }
        char **members = (char **)(void *)(buffer + table_offset);
        members[0] = NULL;
        entry->gr_name = candidate.pw_name;
        entry->gr_passwd = octagram_group_placeholder_password;
        entry->gr_gid = candidate.pw_gid;
        entry->gr_mem = members;
        *result = entry;
        break;
    }
    fclose(stream);
    return status;
}

static int octagram_lookup_group(const char *name, int by_gid, gid_t gid,
                                 struct group *entry, char *buffer,
                                 size_t size, struct group **result)
{
    *result = NULL;
    FILE *stream = fopen(OCTAGRAM_GROUP_PATH, "r");
    if (stream == NULL) {
        return octagram_lookup_primary_group(name, by_gid, gid, entry, buffer,
                                             size, result);
    }
    int status = 0;
    char line[OCTAGRAM_ACCOUNT_LINE_LENGTH];
    while (fgets(line, sizeof(line), stream) != NULL) {
        size_t length = strlen(line);
        if (length == 0 || line[0] == '#') {
            continue;
        }
        if (length + 1 > size) {
            status = ERANGE;
            break;
        }
        memcpy(buffer, line, length + 1);
        struct group candidate;
        int parsed = octagram_read_group_line(buffer, size, &candidate);
        if (parsed == -2) {
            status = ERANGE;
            break;
        }
        if (parsed != 0) {
            continue;
        }
        int matches = by_gid
                          ? candidate.gr_gid == gid
                          : (name != NULL &&
                             strcmp(candidate.gr_name, name) == 0);
        if (!matches) {
            continue;
        }
        *entry = candidate;
        *result = entry;
        break;
    }
    fclose(stream);
    return status;
}

int getgrnam_r(const char *name, struct group *entry, char *buffer,
               size_t size, struct group **result)
{
    if (name == NULL || entry == NULL || buffer == NULL || result == NULL) {
        return EINVAL;
    }
    return octagram_lookup_group(name, 0, 0, entry, buffer, size, result);
}

int getgrgid_r(gid_t gid, struct group *entry, char *buffer, size_t size,
               struct group **result)
{
    if (entry == NULL || buffer == NULL || result == NULL) {
        return EINVAL;
    }
    return octagram_lookup_group(NULL, 1, gid, entry, buffer, size, result);
}

struct group *getgrnam(const char *name)
{
    if (name == NULL) {
        errno = EINVAL;
        return NULL;
    }
    struct group *result = NULL;
    int status = octagram_lookup_group(
        name, 0, 0, &octagram_group_entry, octagram_group_buffer,
        sizeof(octagram_group_buffer), &result);
    if (status != 0) {
        errno = status;
        return NULL;
    }
    errno = 0;
    return result;
}

struct group *getgrgid(gid_t gid)
{
    struct group *result = NULL;
    int status = octagram_lookup_group(
        NULL, 1, gid, &octagram_group_entry, octagram_group_buffer,
        sizeof(octagram_group_buffer), &result);
    if (status != 0) {
        errno = status;
        return NULL;
    }
    errno = 0;
    return result;
}

struct group *getgrent(void)
{
    if (octagram_group_stream == NULL) {
        octagram_group_stream = fopen(OCTAGRAM_GROUP_PATH, "r");
        if (octagram_group_stream == NULL) {
            return NULL;
        }
    }
    char line[OCTAGRAM_ACCOUNT_LINE_LENGTH];
    while (fgets(line, sizeof(line), octagram_group_stream) != NULL) {
        size_t length = strlen(line);
        if (length == 0 || line[0] == '#' ||
            length + 1 > sizeof(octagram_group_buffer)) {
            continue;
        }
        memcpy(octagram_group_buffer, line, length + 1);
        if (octagram_read_group_line(octagram_group_buffer,
                                     sizeof(octagram_group_buffer),
                                     &octagram_group_entry) == 0) {
            return &octagram_group_entry;
        }
    }
    return NULL;
}

void setgrent(void)
{
    if (octagram_group_stream != NULL) {
        rewind(octagram_group_stream);
    }
}

void endgrent(void)
{
    if (octagram_group_stream != NULL) {
        fclose(octagram_group_stream);
        octagram_group_stream = NULL;
    }
}

/* C11 <stdlib.h> remainder: integer division results, absolute values, the
 * pseudo-random generator and binary search. libc++'s <cstdlib> wrapper
 * requires the C header to declare every one of these. */
div_t div(int numerator, int denominator)
{
    div_t result = {0, 0};
    if (denominator == 0) {
        return result;
    }
    result.quot = numerator / denominator;
    result.rem = numerator % denominator;
    return result;
}

ldiv_t ldiv(long numerator, long denominator)
{
    ldiv_t result = {0, 0};
    if (denominator == 0) {
        return result;
    }
    result.quot = numerator / denominator;
    result.rem = numerator % denominator;
    return result;
}

lldiv_t lldiv(long long numerator, long long denominator)
{
    lldiv_t result = {0, 0};
    if (denominator == 0) {
        return result;
    }
    result.quot = numerator / denominator;
    result.rem = numerator % denominator;
    return result;
}

long atol(const char *text)
{
    return strtol(text, NULL, 10);
}

long long atoll(const char *text)
{
    return strtoll(text, NULL, 10);
}

long labs(long value)
{
    return value < 0 ? -value : value;
}

long long llabs(long long value)
{
    return value < 0 ? -value : value;
}

/* POSIX rand_r's generator, so a seed gives the same sequence everywhere. */
static unsigned long oct_rand_state = 1;

void srand(unsigned int seed)
{
    oct_rand_state = seed;
}

int rand(void)
{
    oct_rand_state = oct_rand_state * 1103515245ul + 12345ul;
    return (int)((oct_rand_state / 65536ul) % 32768ul);
}

void *bsearch(const void *key, const void *base, size_t count, size_t size,
              int (*compare)(const void *, const void *))
{
    const unsigned char *bytes = (const unsigned char *)base;
    size_t low = 0;
    size_t high = count;
    while (low < high) {
        size_t middle = low + (high - low) / 2;
        const void *element = bytes + middle * size;
        int order = compare(key, element);
        if (order == 0) {
            return (void *)element;
        }
        if (order < 0) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    return NULL;
}

void _Exit(int status)
{
    _exit(status);
}

/* Multibyte conversions for the ASCII/Latin-1 locale: one byte per character. */
int mblen(const char *text, size_t count)
{
    if (text == NULL) {
        return 0;
    }
    if (count == 0) {
        return -1;
    }
    return text[0] == '\0' ? 0 : 1;
}

int mbtowc(wchar_t *output, const char *text, size_t count)
{
    if (text == NULL) {
        return 0;
    }
    if (count == 0) {
        return -1;
    }
    unsigned char byte = (unsigned char)text[0];
    if (output != NULL) {
        *output = (wchar_t)byte;
    }
    return byte == 0 ? 0 : 1;
}

int wctomb(char *output, wchar_t character)
{
    if (output == NULL) {
        return 0;
    }
    if (character < 0 || character > 0xff) {
        return -1;
    }
    output[0] = (char)character;
    return 1;
}

void *aligned_alloc(size_t alignment, size_t size)
{
    void *result = NULL;
    if (posix_memalign(&result, alignment, size) != 0) {
        return NULL;
    }
    return result;
}

#define OCT_MAX_QUICK_EXIT_HANDLERS 32
static void (*oct_quick_exit_handlers[OCT_MAX_QUICK_EXIT_HANDLERS])(void);
static int oct_quick_exit_count;

int at_quick_exit(void (*function)(void))
{
    if (function == NULL || oct_quick_exit_count >= OCT_MAX_QUICK_EXIT_HANDLERS) {
        return -1;
    }
    oct_quick_exit_handlers[oct_quick_exit_count++] = function;
    return 0;
}

__attribute__((noreturn)) void quick_exit(int status)
{
    while (oct_quick_exit_count > 0) {
        void (*function)(void) = oct_quick_exit_handlers[--oct_quick_exit_count];
        if (function != NULL) {
            function();
        }
    }
    _exit(status);
}

/* The OCTAGRAM locale is ASCII/Latin-1, so collation is byte order and no
 * transformation is needed. */
int strcoll(const char *left, const char *right)
{
    return strcmp(left, right);
}

size_t strxfrm(char *destination, const char *source, size_t count)
{
    const size_t length = strlen(source);
    if (count > 0) {
        size_t copied = length < count - 1 ? length : count - 1;
        memcpy(destination, source, copied);
        destination[copied] = '\0';
    }
    return length;
}

/* Process CPU time in microseconds, as CLOCKS_PER_SEC is 1000000. */
clock_t clock(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &now) != 0) {
        return (clock_t)-1;
    }
    return (clock_t)now.tv_sec * (clock_t)CLOCKS_PER_SEC + (clock_t)(now.tv_nsec / 1000);
}

double difftime(time_t later, time_t earlier)
{
    return (double)(later - earlier);
}

int fgetpos(FILE *stream, fpos_t *position)
{
    if (position == NULL) {
        return -1;
    }
    const long offset = ftell(stream);
    if (offset < 0) {
        return -1;
    }
    *position = (fpos_t)offset;
    return 0;
}

int fsetpos(FILE *stream, const fpos_t *position)
{
    if (position == NULL) {
        return -1;
    }
    return fseek(stream, (long)*position, SEEK_SET);
}

/* mktime for a UTC-only environment: normalise the calendar fields, reject
 * obviously invalid ones, and convert to seconds since the epoch. */
time_t mktime(struct tm *value)
{
    if (value == NULL) {
        return (time_t)-1;
    }
    long long year = (long long)value->tm_year + 1900;
    long long month = value->tm_mon;
    year += month / 12;
    month %= 12;
    if (month < 0) {
        month += 12;
        --year;
    }
    const int day = value->tm_mday;
    const int hour = value->tm_hour;
    const int minute = value->tm_min;
    const int second = value->tm_sec;
    if (day < 1 || day > 31 || hour < 0 || hour > 23 || minute < 0 || minute > 59 ||
        second < 0 || second > 60) {
        return (time_t)-1;
    }
    /* Days since 1970-01-01 for the first of the month. */
    long long days = 0;
    if (year >= 1970) {
        for (long long y = 1970; y < year; ++y) {
            days += (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) ? 366 : 365;
        }
    } else {
        for (long long y = year; y < 1970; ++y) {
            days -= (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) ? 366 : 365;
        }
    }
    static const int month_days[12] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    const int leap = (year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)) ? 1 : 0;
    for (long long m = 0; m < month; ++m) {
        days += month_days[m] + (m == 1 ? leap : 0);
    }
    days += day - 1;
    const time_t result = (time_t)(days * 86400 + hour * 3600 + minute * 60 + second);
    struct tm normalised;
    if (gmtime_r(&result, &normalised) != NULL) {
        *value = normalised;
    }
    return result;
}

/* asctime writes into a static buffer; the caller must consume it immediately,
 * exactly as the C standard specifies. */
char *asctime(const struct tm *value)
{
    static char buffer[26];
    if (value == NULL) {
        return NULL;
    }
    static const char *const weekdays[7] = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};
    static const char *const months[12] = {"Jan", "Feb", "Mar", "Apr", "May", "Jun",
                                           "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
    const int weekday = (value->tm_wday >= 0 && value->tm_wday < 7) ? value->tm_wday : 0;
    const int month = (value->tm_mon >= 0 && value->tm_mon < 12) ? value->tm_mon : 0;
    snprintf(buffer, sizeof buffer, "%s %s %2d %02d:%02d:%02d %d\n", weekdays[weekday],
             months[month], value->tm_mday, value->tm_hour, value->tm_min, value->tm_sec,
             value->tm_year + 1900);
    return buffer;
}

/* tmpnam only needs to exist for header compatibility; it returns a name that is
 * unique per call but carries no security guarantee. */
char *tmpnam(char *buffer)
{
    static char fallback[32];
    static unsigned int counter;
    char *target = buffer != NULL ? buffer : fallback;
    snprintf(target, buffer != NULL ? L_tmpnam : sizeof fallback, "/tmp/octagram-%u.tmp", counter++);
    return target;
}

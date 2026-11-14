#include "octagram.h"

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stddef.h>
#include <stdlib.h>
#include <spawn.h>
#include <pthread.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

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
#if defined(OCTAGRAM_TLS_ERRNO) && !defined(OCTAGRAM_SHARED)
__thread int errno;
#else
int errno;
#endif

extern unsigned char __oct_tls_start[] __attribute__((weak));
extern const uintptr_t __oct_tls_filesz[] __attribute__((weak));
extern const uintptr_t __oct_tls_size[] __attribute__((weak));
extern const uintptr_t __oct_tls_align[] __attribute__((weak));

typedef struct AllocationHeader {
    size_t size;
    struct AllocationHeader *next_free;
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

static int oct_tls_layout(uintptr_t *filesz, uintptr_t *size,
                          uintptr_t *alignment, uintptr_t *aligned_size)
{
    *size = __oct_tls_size == NULL ? 0 : *__oct_tls_size;
    *filesz = __oct_tls_filesz == NULL ? 0 : *__oct_tls_filesz;
    *alignment = __oct_tls_align == NULL ? 0 : *__oct_tls_align;
    if (*size == 0) {
        *aligned_size = 0;
        return 0;
    }
    if (__oct_tls_start == NULL || *filesz > *size
        || *alignment == 0 || *alignment > 4096
        || (*alignment & (*alignment - 1)) != 0
        || *size > SIZE_MAX - (*alignment - 1)) {
        return -1;
    }
    *aligned_size = (*size + *alignment - 1) & ~(*alignment - 1);
    return 0;
}

static void oct_tls_copy(void *destination, uintptr_t filesz, uintptr_t size)
{
    if (filesz != 0) {
        memcpy(destination, __oct_tls_start, filesz);
    }
    if (size > filesz) {
        memset((unsigned char *)destination + filesz, 0, size - filesz);
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
    uintptr_t filesz;
    uintptr_t size;
    uintptr_t alignment;
    uintptr_t aligned_size;
    if (oct_tls_layout(&filesz, &size, &alignment, &aligned_size) != 0
        || aligned_size == 0) {
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
    oct_tls_copy(base, filesz, size);
    void *thread_pointer = (unsigned char *)base + aligned_size;
    oct_tls_initialize_tcb(thread_pointer);
    return thread_pointer;
}

static uintptr_t oct_tls_aligned_size(void)
{
    uintptr_t filesz;
    uintptr_t size;
    uintptr_t alignment;
    uintptr_t aligned_size;
    if (oct_tls_layout(&filesz, &size, &alignment, &aligned_size) != 0) {
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

#define OCT_PTHREAD_STACK_SIZE (64u * 1024u)

static void oct_pthread_start(void *opaque) __attribute__((noreturn));

static void oct_pthread_start(void *opaque)
{
    ThreadStart start = *(ThreadStart *)opaque;
    free(opaque);
    void *result = start.function(start.argument);
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
    if (attributes != NULL) {
        return ENOTSUP;
    }

    uintptr_t tls_aligned_size = oct_tls_aligned_size();
    if (tls_aligned_size > SIZE_MAX - OCT_PTHREAD_STACK_SIZE - 4095) {
        return EINVAL;
    }
    size_t mapping_size = OCT_PTHREAD_STACK_SIZE + (size_t)tls_aligned_size;
    mapping_size = (mapping_size + 4095) & ~(size_t)4095;
    void *mapping = mmap(NULL, mapping_size, PROT_READ | PROT_WRITE,
                         MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mapping == MAP_FAILED) {
        return errno;
    }

    void *thread_pointer = NULL;
    if (tls_aligned_size != 0) {
        uintptr_t filesz;
        uintptr_t tls_size;
        uintptr_t alignment;
        uintptr_t checked_size;
        if (oct_tls_layout(&filesz, &tls_size, &alignment, &checked_size) != 0
            || checked_size != tls_aligned_size) {
            munmap(mapping, mapping_size);
            return EINVAL;
        }
        void *tls_base = (unsigned char *)mapping + OCT_PTHREAD_STACK_SIZE;
        oct_tls_copy(tls_base, filesz, tls_size);
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
        (unsigned char *)mapping + OCT_PTHREAD_STACK_SIZE,
        thread_pointer, mapping, mapping_size);
    if (result < 0) {
        free(start);
        munmap(mapping, mapping_size);
        return (int)-result;
    }
    *thread = (pthread_t)result;
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
    oct_thread_exit(result);
    for (;;) {
        __asm__ volatile ("pause");
    }
}

int pthread_mutex_init(pthread_mutex_t *mutex, const void *attributes)
{
    if (mutex == NULL || attributes != NULL) {
        return EINVAL;
    }
    mutex->state = 0;
    return 0;
}

int pthread_mutex_destroy(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    return mutex->state == 0 ? 0 : EBUSY;
}

int pthread_mutex_lock(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    while (__sync_lock_test_and_set(&mutex->state, 1) != 0) {
        __asm__ volatile ("pause");
    }
    return 0;
}

int pthread_mutex_trylock(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    return __sync_lock_test_and_set(&mutex->state, 1) == 0 ? 0 : EBUSY;
}

int pthread_mutex_unlock(pthread_mutex_t *mutex)
{
    if (mutex == NULL) {
        return EINVAL;
    }
    if (__sync_lock_test_and_set(&mutex->state, 0) == 0) {
        return EINVAL;
    }
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
    case ENOTDIR: return "not a directory";
    case EROFS: return "read-only filesystem";
    case ESPIPE: return "invalid seek";
    default: return "unknown error";
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

ssize_t read(int fd, void *buffer, size_t count)
{
    if (count > OCT_MAX_SYSCALL_IO) {
        count = OCT_MAX_SYSCALL_IO;
    }
    return (ssize_t)posix_result(oct_read(fd, buffer, count));
}

ssize_t write(int fd, const void *buffer, size_t count)
{
    if (count > OCT_MAX_SYSCALL_IO) {
        count = OCT_MAX_SYSCALL_IO;
    }
    return (ssize_t)posix_result(oct_write(fd, buffer, count));
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
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
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
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
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
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
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
    result->st_size = (off_t)metadata.size;
    result->st_mode = (mode_t)metadata.mode;
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

int isatty(int fd)
{
    return (int)posix_result(oct_isatty(fd));
}

int pipe(int descriptors[2])
{
    return (int)posix_result(oct_pipe(descriptors));
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
    if (command == F_DUPFD || command == F_SETFD || command == F_SETFL) {
        va_list arguments;
        va_start(arguments, command);
        argument = (uint64_t)(unsigned int)va_arg(arguments, int);
        va_end(arguments);
    }
    return (int)posix_result(oct_fcntl(fd, command, argument));
}

int ftruncate(int fd, off_t length)
{
    if (length < 0) {
        errno = EINVAL;
        return -1;
    }
    return (int)posix_result(oct_ftruncate(fd, (size_t)length));
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

static int posix_spawn_impl(pid_t *pid, const char *path,
                            const posix_spawn_file_actions_t *file_actions,
                            const posix_spawnattr_t *attributes,
                            char *const arguments[], char *const environment[])
{
    if (pid == NULL || path == NULL || path[0] == '\0') {
        return EINVAL;
    }
    if (file_actions != NULL || attributes != NULL) {
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
        execve(path, arguments, environment);
        int error = errno;
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
        *status = (int)((raw_status & 0xff) << 8);
    }
    return (pid_t)result;
}

pid_t wait(int *status)
{
    return waitpid(-1, status, 0);
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
    AllocationHeader **free_link = &free_list;
    while (*free_link != NULL) {
        AllocationHeader *header = *free_link;
        if (header->size >= size) {
            if (header->size - size >= sizeof(*header) + 16) {
                AllocationHeader *remainder =
                    (AllocationHeader *)((unsigned char *)(header + 1) + size);
                remainder->size = header->size - size - sizeof(*header);
                remainder->next_free = header->next_free;
                *free_link = remainder;
                header->size = size;
            } else {
                *free_link = header->next_free;
            }
            header->next_free = NULL;
            return header + 1;
            }
        free_link = &header->next_free;
        }

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
    header->next_free = free_list;
    free_list = header;
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
    header->next_free = free_list;
    free_list = header;
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

int *__errno_location(void)
{
    return &errno;
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
    (void)path;
    (void)resolved_path;
    errno = ENOSYS;
    return NULL;
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
    (void)name;
    return 4096;
}

int execvp(const char *file, char *const arguments[])
{
    (void)file;
    (void)arguments;
    errno = ENOSYS;
    return -1;
}

int execve(const char *path, char *const arguments[], char *const environment[])
{
    return (int)posix_result(oct_execve(path, arguments, environment));
}

pid_t fork(void)
{
    return (pid_t)posix_result(oct_fork());
}

FILE *freopen(const char *path, const char *mode, FILE *stream)
{
    (void)path;
    (void)mode;
    (void)stream;
    errno = ENOSYS;
    return NULL;
}

__attribute__((noreturn)) void exit(int status)
{
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

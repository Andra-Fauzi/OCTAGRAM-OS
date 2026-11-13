#include "octagram.h"

#include <stdlib.h>
#include <string.h>

static uintptr_t heap_next;

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

int strcmp(const char *left, const char *right)
{
    while (*left != '\0' && *left == *right) {
        ++left;
        ++right;
    }
    return (unsigned char)*left - (unsigned char)*right;
}

void *malloc(size_t size)
{
    if (size == 0) {
        size = 1;
    }
    if (heap_next == 0) {
        int64_t initial = oct_brk(0);
        if (initial < 0) {
            return NULL;
        }
        heap_next = (uintptr_t)initial;
    }
    if (heap_next > UINTPTR_MAX - 15) {
        return NULL;
    }
    uintptr_t start = (heap_next + 15) & ~(uintptr_t)15;
    if (size > UINTPTR_MAX - start) {
        return NULL;
    }
    uintptr_t end = start + size;
    if (oct_brk(end) < 0) {
        return NULL;
    }
    heap_next = end;
    return (void *)start;
}

void free(void *pointer)
{
    (void)pointer;
}

void *calloc(size_t count, size_t size)
{
    if (size != 0 && count > SIZE_MAX / size) {
        return NULL;
    }
    size_t total = count * size;
    void *result = malloc(total);
    if (result != NULL) {
        memset(result, 0, total);
    }
    return result;
}

__attribute__((noreturn)) void abort(void)
{
    oct_exit(127);
    for (;;) {
        __asm__ volatile ("pause");
    }
}

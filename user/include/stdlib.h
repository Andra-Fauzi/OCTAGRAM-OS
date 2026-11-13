#ifndef OCTAGRAM_STDLIB_H
#define OCTAGRAM_STDLIB_H

#include <stddef.h>

void *malloc(size_t size);
void free(void *pointer);
void *calloc(size_t count, size_t size);
__attribute__((noreturn)) void abort(void);

#endif

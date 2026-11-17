#ifndef OCTAGRAM_WCHAR_H
#define OCTAGRAM_WCHAR_H

#include <stddef.h>

typedef struct {
    int count;
    unsigned char bytes[4];
    unsigned int pending;
} mbstate_t;

size_t mbrtowc(wchar_t *restrict wide, const char *restrict input, size_t count,
               mbstate_t *restrict state);
size_t wcrtomb(char *restrict output, wchar_t wide, mbstate_t *restrict state);

#endif

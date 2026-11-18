#ifndef OCTAGRAM_WCHAR_H
#define OCTAGRAM_WCHAR_H

#include <stddef.h>

typedef struct {
    int count;
    unsigned char bytes[4];
    unsigned int pending;
} mbstate_t;

typedef unsigned int wint_t;

size_t mbrtowc(wchar_t *restrict wide, const char *restrict input, size_t count,
               mbstate_t *restrict state);
size_t wcrtomb(char *restrict output, wchar_t wide, mbstate_t *restrict state);
size_t wcslen(const wchar_t *text);
int wcscmp(const wchar_t *left, const wchar_t *right);
int wcsncmp(const wchar_t *left, const wchar_t *right, size_t count);
wchar_t *wcscpy(wchar_t *destination, const wchar_t *source);
wchar_t *wcsncpy(wchar_t *destination, const wchar_t *source, size_t count);
wchar_t *wcschr(const wchar_t *text, wchar_t character);
wchar_t *wcsrchr(const wchar_t *text, wchar_t character);
int wcscoll(const wchar_t *left, const wchar_t *right);
size_t wcstombs(char *destination, const wchar_t *source, size_t count);
size_t mbstowcs(wchar_t *destination, const char *source, size_t count);
wchar_t *wmemcpy(wchar_t *destination, const wchar_t *source, size_t count);
wchar_t *wmemmove(wchar_t *destination, const wchar_t *source, size_t count);
int wmemcmp(const wchar_t *left, const wchar_t *right, size_t count);
wchar_t *wmemchr(const wchar_t *memory, wchar_t character, size_t count);
long wcstol(const wchar_t *text, wchar_t **end, int base);
wchar_t *wcstok(wchar_t *text, const wchar_t *delimiter,
                wchar_t **context);

#endif

#ifndef OCTAGRAM_UCHAR_H
#define OCTAGRAM_UCHAR_H

#include <stdint.h>
#include <wchar.h>

typedef uint16_t char16_t;
typedef uint32_t char32_t;

size_t c16rtomb(char *restrict output, char16_t unit, mbstate_t *restrict state);
size_t c32rtomb(char *restrict output, char32_t wide, mbstate_t *restrict state);

#endif

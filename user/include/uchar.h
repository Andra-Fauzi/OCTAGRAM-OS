#ifndef OCTAGRAM_UCHAR_H
#define OCTAGRAM_UCHAR_H

#include <stdint.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif

/* char16_t/char32_t are keywords in C++, so only C needs the typedefs. */
#ifndef __cplusplus
typedef uint16_t char16_t;
typedef uint32_t char32_t;
#endif

size_t mbrtoc16(char16_t *restrict unit, const char *restrict input, size_t count,
                mbstate_t *restrict state);
size_t mbrtoc32(char32_t *restrict wide, const char *restrict input, size_t count,
                mbstate_t *restrict state);
size_t c16rtomb(char *restrict output, char16_t unit, mbstate_t *restrict state);
size_t c32rtomb(char *restrict output, char32_t wide, mbstate_t *restrict state);

#ifdef __cplusplus
}
#endif

#endif

#ifndef OCTAGRAM_STRINGS_H
#define OCTAGRAM_STRINGS_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* POSIX case-insensitive byte-string comparisons. */
int strcasecmp(const char *left, const char *right);
int strncasecmp(const char *left, const char *right, size_t count);

#ifdef __cplusplus
}
#endif

#endif

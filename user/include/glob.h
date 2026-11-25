#ifndef OCTAGRAM_GLOB_H
#define OCTAGRAM_GLOB_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define GLOB_ERR 0x0001      /* stop on read errors */
#define GLOB_MARK 0x0002     /* append '/' to directory matches */
#define GLOB_NOSORT 0x0004   /* do not sort the result */
#define GLOB_DOOFFS 0x0008   /* leave gl_offs empty slots at the front */
#define GLOB_NOCHECK 0x0010  /* return the pattern when nothing matches */
#define GLOB_APPEND 0x0020   /* append to an existing glob_t */
#define GLOB_NOESCAPE 0x0040 /* backslash does not escape */

#define GLOB_NOSPACE 1
#define GLOB_ABORTED 2
#define GLOB_NOMATCH 3
#define GLOB_NOSYS 4

typedef struct {
    size_t gl_pathc; /* number of matched paths */
    char **gl_pathv; /* matched paths, NULL terminated */
    size_t gl_offs;  /* reserved slots at the front of gl_pathv */
    int gl_flags;
} glob_t;

int glob(const char *pattern, int flags, int (*error_function)(const char *, int),
         glob_t *result);
void globfree(glob_t *result);

#ifdef __cplusplus
}
#endif

#endif

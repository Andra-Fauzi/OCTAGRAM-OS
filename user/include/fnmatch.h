#ifndef OCTAGRAM_FNMATCH_H
#define OCTAGRAM_FNMATCH_H

#ifdef __cplusplus
extern "C" {
#endif

#define FNM_NOMATCH 1
#define FNM_PATHNAME 0x01 /* wildcards never match '/' */
#define FNM_NOESCAPE 0x02 /* treat backslash literally */
#define FNM_PERIOD 0x04   /* a leading '.' must be matched explicitly */
#define FNM_FILE_NAME FNM_PATHNAME
#define FNM_LEADING_DIR 0x08 /* ignore a trailing "/..." in string */
#define FNM_CASEFOLD 0x10    /* case-insensitive matching */

/*
 * Shell-style pattern matching. Supports '*', '?', '[...]' with ranges and
 * negation, backslash escapes, and the FNM_PATHNAME/FNM_NOESCAPE/FNM_PERIOD
 * flags. Returns 0 on a match and FNM_NOMATCH otherwise.
 */
int fnmatch(const char *pattern, const char *string, int flags);

#ifdef __cplusplus
}
#endif

#endif

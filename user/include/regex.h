#ifndef OCTAGRAM_REGEX_H
#define OCTAGRAM_REGEX_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned long reg_syntax_t;

typedef struct {
    size_t re_nsub;   /* number of parenthesised subexpressions */
    void *re_program; /* compiled program, private */
    int re_cflags;
    char *re_pattern; /* pattern copy for regerror */
    int re_error;
} regex_t;

typedef struct {
    long rm_so; /* byte offset of the start, or -1 */
    long rm_eo; /* byte offset one past the end, or -1 */
} regmatch_t;

/* regcomp flags */
#define REG_EXTENDED 1
#define REG_ICASE 2
#define REG_NEWLINE 4
#define REG_NOSUB 8

/* regexec flags */
#define REG_NOTBOL 1
#define REG_NOTEOL 2
#define REG_STARTEND 4

/* Error codes returned by regcomp/regexec */
#define REG_NOERROR 0
#define REG_NOMATCH 1
#define REG_BADPAT 2
#define REG_ECOLLATE 3
#define REG_ECTYPE 4
#define REG_EESCAPE 5
#define REG_ESUBREG 6
#define REG_EBRACK 7
#define REG_EPAREN 8
#define REG_EBRACE 9
#define REG_BADBR 10
#define REG_ERANGE 11
#define REG_ESPACE 12
#define REG_ESIZE 13
#define REG_ERPAREN 14

#define RE_NREGS 32

/* Compiles `pattern`. Extended syntax (ERE) is selected with REG_EXTENDED;
 * without it the pattern uses POSIX basic syntax with the GNU operators
 * \| \+ \? and interval \{m,n\}. */
int regcomp(regex_t *expression, const char *pattern, int flags);

/* Finds the leftmost match, and the longest one starting there. `matches[0]`
 * receives the whole match; further slots get the groups. Unmatched groups
 * report -1 in both fields. */
int regexec(const regex_t *expression, const char *string, size_t match_count,
            regmatch_t matches[], int flags);

/* Writes a message for `error_code` into `buffer` (truncating as needed) and
 * returns the length the full message would have had. */
size_t regerror(int error_code, const regex_t *expression, char *buffer,
                size_t length);

void regfree(regex_t *expression);

#ifdef __cplusplus
}
#endif
#endif

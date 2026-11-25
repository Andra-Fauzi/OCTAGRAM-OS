#ifndef OCTAGRAM_WORDEXP_H
#define OCTAGRAM_WORDEXP_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    size_t we_wordc;
    char **we_wordv;
    size_t we_offs;
} wordexp_t;

#define WRDE_APPEND 0x01
#define WRDE_DOOFFS 0x02
#define WRDE_NOCMD 0x04
#define WRDE_REUSE 0x08
#define WRDE_SHOWERR 0x10
#define WRDE_UNDEF 0x20

enum {
    WRDE_NOSPACE = 1,
    WRDE_BADCHAR,
    WRDE_BADVAL,
    WRDE_CMDSUB,
    WRDE_SYNTAX
};

/* Shell-style word expansion: quoting, $VAR/${VAR}, ~ and pathname globbing.
 * Command substitution is not implemented and reports WRDE_CMDSUB. */
int wordexp(const char *words, wordexp_t *result, int flags);
void wordfree(wordexp_t *result);

#ifdef __cplusplus
}
#endif
#endif

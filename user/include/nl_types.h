#ifndef OCTAGRAM_NL_TYPES_H
#define OCTAGRAM_NL_TYPES_H

#ifdef __cplusplus
extern "C" {
#endif

/* POSIX message catalogs. OCTAGRAM has no catalog files, so `catopen` reports
 * failure -- which the standard defines as "use the strings compiled into the
 * program" -- and `catgets` always returns the caller's default message. */

typedef void *nl_catd;
typedef int nl_item;

#define NL_SETD 1
#define NL_CAT_LOCALE 1

nl_catd catopen(const char *name, int flag);
char *catgets(nl_catd catalog, int set_number, int message_number,
              const char *message);
int catclose(nl_catd catalog);

#ifdef __cplusplus
}
#endif

#endif

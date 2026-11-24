#ifndef OCTAGRAM_WCTYPE_H
#define OCTAGRAM_WCTYPE_H

#include <stddef.h>
#include <wchar.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned long wctype_t;
typedef unsigned long wctrans_t;

/* Character classes understood by wctype(): names follow the C standard
 * ("alpha", "digit", "space", ...). Classification is ASCII plus Latin-1,
 * matching the current single-locale ctype implementation. */
wctype_t wctype(const char *name);
int iswctype(wint_t character, wctype_t description);

int iswalnum(wint_t character);
int iswalpha(wint_t character);
int iswblank(wint_t character);
int iswcntrl(wint_t character);
int iswdigit(wint_t character);
int iswgraph(wint_t character);
int iswlower(wint_t character);
int iswprint(wint_t character);
int iswpunct(wint_t character);
int iswspace(wint_t character);
int iswupper(wint_t character);
int iswxdigit(wint_t character);

wint_t towlower(wint_t character);
wint_t towupper(wint_t character);

/* Character mappings understood by wctrans(): "tolower" and "toupper". */
wctrans_t wctrans(const char *name);
wint_t towctrans(wint_t character, wctrans_t mapping);

#ifdef __cplusplus
}
#endif

#endif

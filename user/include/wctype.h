#ifndef OCTAGRAM_WCTYPE_H
#define OCTAGRAM_WCTYPE_H

#include <locale.h>
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

/* POSIX.1-2008 locale-aware wide-character classes. libc++'s locale facets
 * call these through _LIBCPP_GET_C_LOCALE; OCTAGRAM has only the C locale, so
 * the locale argument is ignored and the call forwards to the plain form. */
int iswalnum_l(wint_t character, locale_t locale);
int iswalpha_l(wint_t character, locale_t locale);
int iswblank_l(wint_t character, locale_t locale);
int iswcntrl_l(wint_t character, locale_t locale);
int iswdigit_l(wint_t character, locale_t locale);
int iswgraph_l(wint_t character, locale_t locale);
int iswlower_l(wint_t character, locale_t locale);
int iswprint_l(wint_t character, locale_t locale);
int iswpunct_l(wint_t character, locale_t locale);
int iswspace_l(wint_t character, locale_t locale);
int iswupper_l(wint_t character, locale_t locale);
int iswxdigit_l(wint_t character, locale_t locale);
int iswctype_l(wint_t character, wctype_t description, locale_t locale);
wint_t towlower_l(wint_t character, locale_t locale);
wint_t towupper_l(wint_t character, locale_t locale);

#ifdef __cplusplus
}
#endif

#endif

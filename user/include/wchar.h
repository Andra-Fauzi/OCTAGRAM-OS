#ifndef OCTAGRAM_WCHAR_H
#define OCTAGRAM_WCHAR_H

#include <locale.h>
#include <stdarg.h>
#include <stddef.h>

/* <wchar.h> declares these itself in C99; libc++'s <cwchar> wrapper then
 * re-declares every one of them in namespace std. */
typedef struct octagram_FILE FILE;
struct tm;

/* `restrict` is a C keyword, not C++. Headers below rely on this shim so the
 * same source stays valid for both front ends. */
#ifdef __cplusplus
#define restrict __restrict
extern "C" {
#endif

typedef struct {
    int count;
    unsigned char bytes[4];
    unsigned int pending;
} mbstate_t;

typedef unsigned int wint_t;
#define WEOF ((wint_t)0xffffffffu)

size_t mbrtowc(wchar_t *restrict wide, const char *restrict input, size_t count,
               mbstate_t *restrict state);
size_t wcrtomb(char *restrict output, wchar_t wide, mbstate_t *restrict state);
size_t wcslen(const wchar_t *text);
int wcscmp(const wchar_t *left, const wchar_t *right);
int wcsncmp(const wchar_t *left, const wchar_t *right, size_t count);
wchar_t *wcscpy(wchar_t *destination, const wchar_t *source);
wchar_t *wcsncpy(wchar_t *destination, const wchar_t *source, size_t count);
wchar_t *wcschr(const wchar_t *text, wchar_t character);
wchar_t *wcsrchr(const wchar_t *text, wchar_t character);
int wcscoll(const wchar_t *left, const wchar_t *right);
size_t wcstombs(char *destination, const wchar_t *source, size_t count);
size_t mbstowcs(wchar_t *destination, const char *source, size_t count);
wchar_t *wmemcpy(wchar_t *destination, const wchar_t *source, size_t count);
wchar_t *wmemmove(wchar_t *destination, const wchar_t *source, size_t count);
int wmemcmp(const wchar_t *left, const wchar_t *right, size_t count);
wchar_t *wmemchr(const wchar_t *memory, wchar_t character, size_t count);
long wcstol(const wchar_t *text, wchar_t **end, int base);
long long wcstoll(const wchar_t *text, wchar_t **end, int base);
unsigned long wcstoul(const wchar_t *text, wchar_t **end, int base);
unsigned long long wcstoull(const wchar_t *text, wchar_t **end, int base);
double wcstod(const wchar_t *text, wchar_t **end);
float wcstof(const wchar_t *text, wchar_t **end);
long double wcstold(const wchar_t *text, wchar_t **end);
long wcstol_l(const wchar_t *text, wchar_t **end, int base, locale_t locale);
long long wcstoll_l(const wchar_t *text, wchar_t **end, int base, locale_t locale);
unsigned long wcstoul_l(const wchar_t *text, wchar_t **end, int base, locale_t locale);
unsigned long long wcstoull_l(const wchar_t *text, wchar_t **end, int base,
                              locale_t locale);
double wcstod_l(const wchar_t *text, wchar_t **end, locale_t locale);
float wcstof_l(const wchar_t *text, wchar_t **end, locale_t locale);
long double wcstold_l(const wchar_t *text, wchar_t **end, locale_t locale);
wchar_t *wcstok(wchar_t *text, const wchar_t *delimiter,
                wchar_t **context);
wchar_t *wcscat(wchar_t *destination, const wchar_t *source);
wchar_t *wcsncat(wchar_t *destination, const wchar_t *source, size_t count);
wchar_t *wcsstr(const wchar_t *text, const wchar_t *needle);
wchar_t *wcspbrk(const wchar_t *text, const wchar_t *accept);
size_t wcscspn(const wchar_t *text, const wchar_t *reject);
size_t wcsspn(const wchar_t *text, const wchar_t *accept);
size_t wcsxfrm(wchar_t *destination, const wchar_t *source, size_t count);
wchar_t *wmemset(wchar_t *destination, wchar_t character, size_t count);
wint_t btowc(int character);
int wctob(wint_t character);
int mbsinit(const mbstate_t *state);
size_t mbrlen(const char *input, size_t count, mbstate_t *state);
size_t mbsrtowcs(wchar_t *destination, const char **source, size_t count,
                 mbstate_t *state);
size_t mbsnrtowcs(wchar_t *destination, const char **source, size_t byte_count,
                  size_t count, mbstate_t *state);
size_t wcsrtombs(char *destination, const wchar_t **source, size_t count,
                 mbstate_t *state);
size_t wcsnrtombs(char *destination, const wchar_t **source, size_t wide_count,
                  size_t count, mbstate_t *state);

/* Wide stdio and wcsftime are declared for header compatibility but are not
 * implemented yet: calling one is a link error rather than a silent stub. */
int fwprintf(FILE *stream, const wchar_t *format, ...);
int fwscanf(FILE *stream, const wchar_t *format, ...);
int swprintf(wchar_t *buffer, size_t count, const wchar_t *format, ...);
int swscanf(const wchar_t *buffer, const wchar_t *format, ...);
int vfwprintf(FILE *stream, const wchar_t *format, va_list arguments);
int vswprintf(wchar_t *buffer, size_t count, const wchar_t *format, va_list arguments);
int vfwscanf(FILE *stream, const wchar_t *format, va_list arguments);
int vswscanf(const wchar_t *buffer, const wchar_t *format, va_list arguments);
int wprintf(const wchar_t *format, ...);
int wscanf(const wchar_t *format, ...);
int vwprintf(const wchar_t *format, va_list arguments);
int vwscanf(const wchar_t *format, va_list arguments);
wint_t fgetwc(FILE *stream);
wchar_t *fgetws(wchar_t *buffer, int count, FILE *stream);
wint_t fputwc(wchar_t character, FILE *stream);
int fputws(const wchar_t *text, FILE *stream);
int fwide(FILE *stream, int mode);
wint_t getwc(FILE *stream);
wint_t putwc(wchar_t character, FILE *stream);
wint_t ungetwc(wint_t character, FILE *stream);
wint_t getwchar(void);
wint_t putwchar(wchar_t character);
size_t wcsftime(wchar_t *buffer, size_t count, const wchar_t *format,
                const struct tm *time_value);

#ifdef __cplusplus
}
#endif

#endif

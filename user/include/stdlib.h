#ifndef OCTAGRAM_STDLIB_H
#define OCTAGRAM_STDLIB_H

#include <locale.h>
#include <stddef.h>

#ifndef MB_CUR_MAX
#define MB_CUR_MAX 4
#endif
#ifndef MB_LEN_MAX
#define MB_LEN_MAX 4
#endif

#ifdef __cplusplus
extern "C" {
#endif

extern char **environ;

void *malloc(size_t size);
int posix_memalign(void **result, size_t alignment, size_t size);
void *realloc(void *pointer, size_t size);
void free(void *pointer);
void *calloc(size_t count, size_t size);
int atexit(void (*function)(void));
int __cxa_atexit(void (*function)(void *), void *argument, void *dso);
void __cxa_finalize(void *dso);
__attribute__((noreturn)) void exit(int status);
__attribute__((noreturn)) void abort(void);
unsigned long strtoul(const char *text, char **end, int base);
long strtol(const char *text, char **end, int base);
unsigned long long strtoull(const char *text, char **end, int base);
long long strtoll(const char *text, char **end, int base);
double strtod(const char *text, char **end);
double atof(const char *text);
float strtof(const char *text, char **end);
long double strtold(const char *text, char **end);

/* POSIX.1-2008 locale-aware conversions; the locale argument is ignored because
 * OCTAGRAM has only the C locale. */
long strtol_l(const char *text, char **end, int base, locale_t locale);
long long strtoll_l(const char *text, char **end, int base, locale_t locale);
unsigned long strtoul_l(const char *text, char **end, int base, locale_t locale);
unsigned long long strtoull_l(const char *text, char **end, int base, locale_t locale);
double strtod_l(const char *text, char **end, locale_t locale);
float strtof_l(const char *text, char **end, locale_t locale);
long double strtold_l(const char *text, char **end, locale_t locale);
char *getenv(const char *name);
// `template` is a C++ keyword, so the parameter needs a neutral name for
// C++ translation units that include this header.
int mkstemp(char *pattern);
int setenv(const char *name, const char *value, int overwrite);
int unsetenv(const char *name);
int putenv(char *entry);
int atoi(const char *text);
long atol(const char *text);
long long atoll(const char *text);
long labs(long value);
long long llabs(long long value);

/* C11 division results, and the quicksort/binary-search helpers libc++
 * (through <cstdlib>) requires the C header to declare. */
typedef struct {
    int quot;
    int rem;
} div_t;
typedef struct {
    long quot;
    long rem;
} ldiv_t;
typedef struct {
    long long quot;
    long long rem;
} lldiv_t;

div_t div(int numerator, int denominator);
ldiv_t ldiv(long numerator, long denominator);
lldiv_t lldiv(long long numerator, long long denominator);

#define RAND_MAX 0x7fffffff
int rand(void);
void srand(unsigned int seed);

void *bsearch(const void *key, const void *base, size_t count, size_t size,
              int (*compare)(const void *, const void *));
__attribute__((noreturn)) void _Exit(int status);
int at_quick_exit(void (*function)(void));
__attribute__((noreturn)) void quick_exit(int status);
void *aligned_alloc(size_t alignment, size_t size);

/* Multibyte conversion; the OCTAGRAM locale is ASCII/Latin-1, so one byte is
 * one character. */
int mblen(const char *text, size_t count);
size_t mbstowcs(wchar_t *destination, const char *source, size_t count);
size_t wcstombs(char *destination, const wchar_t *source, size_t count);
int mbtowc(wchar_t *output, const char *text, size_t count);
int wctomb(char *output, wchar_t character);
char *realpath(const char *path, char *resolved_path);
int remove(const char *path);
void qsort(void *base, size_t count, size_t size,
           int (*compare)(const void *, const void *));
int abs(int value);
void *reallocarray(void *pointer, size_t count, size_t size);
void qsort_r(void *base, size_t count, size_t size,
             int (*compare)(const void *, const void *, void *), void *argument);
char *mkdtemp(char *path_template);
size_t malloc_usable_size(void *pointer);
int system(const char *command);

/* Splits a comma-separated option string against a NULL-terminated token list.
 * Returns the token index, or -1 for an unknown option. */
int getsubopt(char **option_pointer, char *const *tokens, char **value_pointer);

#ifdef __cplusplus
}
#endif

#endif

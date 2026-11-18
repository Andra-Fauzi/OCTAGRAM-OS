#ifndef OCTAGRAM_STDLIB_H
#define OCTAGRAM_STDLIB_H

#include <stddef.h>

#ifndef MB_CUR_MAX
#define MB_CUR_MAX 4
#endif
#ifndef MB_LEN_MAX
#define MB_LEN_MAX 4
#endif

extern char **environ;

void *malloc(size_t size);
void *realloc(void *pointer, size_t size);
void free(void *pointer);
void *calloc(size_t count, size_t size);
__attribute__((noreturn)) void exit(int status);
__attribute__((noreturn)) void abort(void);
unsigned long strtoul(const char *text, char **end, int base);
long strtol(const char *text, char **end, int base);
unsigned long long strtoull(const char *text, char **end, int base);
long long strtoll(const char *text, char **end, int base);
double strtod(const char *text, char **end);
float strtof(const char *text, char **end);
long double strtold(const char *text, char **end);
char *getenv(const char *name);
int setenv(const char *name, const char *value, int overwrite);
int unsetenv(const char *name);
int putenv(char *entry);
int atoi(const char *text);
char *realpath(const char *path, char *resolved_path);
int remove(const char *path);
void qsort(void *base, size_t count, size_t size,
           int (*compare)(const void *, const void *));
int abs(int value);
int system(const char *command);

#endif

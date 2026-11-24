#ifndef OCTAGRAM_STRING_H
#define OCTAGRAM_STRING_H

#ifdef __cplusplus
extern "C" {
#endif

#include <locale.h>
#include <stddef.h>

void *memcpy(void *destination, const void *source, size_t count);
void *memmove(void *destination, const void *source, size_t count);
void *memset(void *destination, int value, size_t count);
void *memchr(const void *memory, int character, size_t count);
void *memrchr(const void *memory, int character, size_t count);
char *strcpy(char *destination, const char *source);
char *strcat(char *destination, const char *source);
char *strncat(char *destination, const char *source, size_t count);
char *strncpy(char *destination, const char *source, size_t count);
int memcmp(const void *left, const void *right, size_t count);
size_t strlen(const char *text);
size_t strspn(const char *text, const char *accept);
int strcoll(const char *left, const char *right);
size_t strxfrm(char *destination, const char *source, size_t count);
int strcoll_l(const char *left, const char *right, locale_t locale);
size_t strxfrm_l(char *destination, const char *source, size_t count, locale_t locale);
size_t strcspn(const char *text, const char *reject);
int strcmp(const char *left, const char *right);
int strncmp(const char *left, const char *right, size_t count);
int strncasecmp(const char *left, const char *right, size_t count);
char *strchr(const char *text, int character);
char *strrchr(const char *text, int character);
char *strpbrk(const char *text, const char *accept);
char *strstr(const char *text, const char *substring);
char *strtok(char *text, const char *delimiters);
char *strtok_r(char *text, const char *delimiters, char **save_pointer);
char *strdup(const char *text);
char *strndup(const char *text, size_t count);
int strerror_r(int error, char *buffer, size_t length);
char *strerror(int error);
char *strsignal(int signal_number);


#ifdef __cplusplus
}
#endif
#endif

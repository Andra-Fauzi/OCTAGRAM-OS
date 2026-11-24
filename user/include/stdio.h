#ifndef OCTAGRAM_STDIO_H
#define OCTAGRAM_STDIO_H

#include <stdarg.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct octagram_FILE FILE;

#define EOF (-1)
#define BUFSIZ 4096
#define _IONBF 0
#define _IOLBF 1
#define _IOFBF 2
#ifndef SEEK_SET
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#endif

extern FILE *stdin;
extern FILE *stdout;
extern FILE *stderr;

int fclose(FILE *stream);
int fflush(FILE *stream);
int setvbuf(FILE *stream, char *buffer, int mode, size_t size);
void perror(const char *message);
FILE *fdopen(int fd, const char *mode);
FILE *tmpfile(void);
FILE *fopen(const char *path, const char *mode);
FILE *freopen(const char *path, const char *mode, FILE *stream);
int rename(const char *source, const char *target);
int renameat(int olddirfd, const char *oldpath, int newdirfd, const char *newpath);
int fseek(FILE *stream, long offset, int whence);
long ftell(FILE *stream);
void rewind(FILE *stream);
size_t fread(void *buffer, size_t size, size_t count, FILE *stream);
size_t fwrite(const void *buffer, size_t size, size_t count, FILE *stream);
int fgetc(FILE *stream);
int getc(FILE *stream);
int getchar(void);
char *fgets(char *buffer, int size, FILE *stream);
int ungetc(int character, FILE *stream);
int fputc(int character, FILE *stream);
int putc(int character, FILE *stream);
int putchar(int character);
int fputs(const char *text, FILE *stream);
int puts(const char *text);
int feof(FILE *stream);
int ferror(FILE *stream);
void clearerr(FILE *stream);
int fileno(FILE *stream);
int vfscanf(FILE *stream, const char *format, va_list arguments);
int fscanf(FILE *stream, const char *format, ...);
int vscanf(const char *format, va_list arguments);
int scanf(const char *format, ...);
int vsscanf(const char *text, const char *format, va_list arguments);
int sscanf(const char *text, const char *format, ...);
int vfprintf(FILE *stream, const char *format, va_list arguments);
int fprintf(FILE *stream, const char *format, ...);
int vprintf(const char *format, va_list arguments);
int printf(const char *format, ...);
int vsnprintf(char *buffer, size_t size, const char *format, va_list arguments);
int snprintf(char *buffer, size_t size, const char *format, ...);
int vsprintf(char *buffer, const char *format, va_list arguments);
int sprintf(char *buffer, const char *format, ...);

#ifdef __cplusplus
}
#endif

#endif

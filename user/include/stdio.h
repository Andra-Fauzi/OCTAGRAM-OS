#ifndef OCTAGRAM_STDIO_H
#define OCTAGRAM_STDIO_H

#include <stdarg.h>
#include <stddef.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct octagram_FILE FILE;
/* C99 file position; OCTAGRAM tracks offsets as 64-bit values. */
typedef long fpos_t;

/* OCTAGRAM streams are unbuffered, so setbuf/setvbuf cannot fail. */
int remove(const char *path);
#define L_tmpnam 32
char *tmpnam(char *buffer);
int fgetpos(FILE *stream, fpos_t *position);
int fsetpos(FILE *stream, const fpos_t *position);
void setbuf(FILE *stream, char *buffer);
int setvbuf(FILE *stream, char *buffer, int mode, size_t size);

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
int fseeko(FILE *stream, off_t offset, int whence);
off_t ftello(FILE *stream);
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

/* GNU extensions used by libc++'s locale and chrono formatting. */
int asprintf(char **result, const char *format, ...);
int vasprintf(char **result, const char *format, va_list arguments);
ssize_t getline(char **line, size_t *capacity, FILE *stream);
ssize_t getdelim(char **line, size_t *capacity, int delimiter, FILE *stream);
int snprintf(char *buffer, size_t size, const char *format, ...);
int vsprintf(char *buffer, const char *format, va_list arguments);
int sprintf(char *buffer, const char *format, ...);

#ifdef __cplusplus
}
#endif

#endif

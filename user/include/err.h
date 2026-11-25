#ifndef OCTAGRAM_ERR_H
#define OCTAGRAM_ERR_H

#include <stdarg.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * BSD-style fatal/warning diagnostics. The "x" variants omit strerror, and
 * the "err" variants exit with the given status after printing.
 */
void err(int status, const char *format, ...)
    __attribute__((format(printf, 2, 3), noreturn));
void errx(int status, const char *format, ...)
    __attribute__((format(printf, 2, 3), noreturn));
void warn(const char *format, ...) __attribute__((format(printf, 1, 2)));
void warnx(const char *format, ...) __attribute__((format(printf, 1, 2)));

void verr(int status, const char *format, va_list arguments)
    __attribute__((noreturn));
void verrx(int status, const char *format, va_list arguments)
    __attribute__((noreturn));
void vwarn(const char *format, va_list arguments);
void vwarnx(const char *format, va_list arguments);

#ifdef __cplusplus
}
#endif

#endif

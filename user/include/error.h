#ifndef OCTAGRAM_ERROR_H
#define OCTAGRAM_ERROR_H

#ifdef __cplusplus
extern "C" {
#endif

/*
 * glibc-style diagnostics. The message goes to stderr as
 * "program: message: strerror(error_number)", and a non-zero status exits
 * the process after printing.
 */
void error(int status, int error_number, const char *format, ...)
    __attribute__((format(printf, 3, 4)));
void error_at_line(int status, int error_number, const char *file_name,
                   unsigned int line_number, const char *format, ...)
    __attribute__((format(printf, 5, 6)));

extern unsigned int error_message_count;
extern int error_one_per_line;

/* glibc exposes the process name as these two variables; error() and err.h
 * use them. They are empty until a caller (or crt0) sets them. */
extern char *program_invocation_name;
extern char *program_invocation_short_name;

#ifdef __cplusplus
}
#endif

#endif

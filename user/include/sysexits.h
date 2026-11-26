#ifndef OCTAGRAM_SYSEXITS_H
#define OCTAGRAM_SYSEXITS_H

/* BSD sysexits.h status codes.
 *
 * POSIX does not require this header, but portable programs use the EX_*
 * values as process exit codes; LLVM's llvm/Support/ExitCodes.h turns each
 * missing one into an #error, and the C++ tools that include it (llvm-nm,
 * llc, rustc) only need the constants. They are plain integers, so there is
 * nothing to implement in the runtime.
 */

#define EX_OK 0
#define EX__BASE 64
#define EX_USAGE 64
#define EX_DATAERR 65
#define EX_NOINPUT 66
#define EX_NOUSER 67
#define EX_NOHOST 68
#define EX_UNAVAILABLE 69
#define EX_SOFTWARE 70
#define EX_OSERR 71
#define EX_OSFILE 72
#define EX_CANTCREAT 73
#define EX_IOERR 74
#define EX_TEMPFAIL 75
#define EX_PROTOCOL 76
#define EX_NOPERM 77
#define EX_CONFIG 78
#define EX__MAX 78

#endif

#ifndef OCTAGRAM_LIMITS_H
#define OCTAGRAM_LIMITS_H

/* x86_64 OCTAGRAM userspace limits. Keep these independent of host headers. */
#define CHAR_BIT 8
#define CHAR_MIN (-128)
#define CHAR_MAX 127
#define SCHAR_MIN (-128)
#define SCHAR_MAX 127
#define UCHAR_MAX 255
#define SHRT_MIN (-32768)
#define SHRT_MAX 32767
#define USHRT_MAX 65535
#define INT_MIN (-2147483647 - 1)
#define INT_MAX 2147483647
#define UINT_MAX 4294967295U
#define LONG_MIN (-9223372036854775807L - 1)
#define LONG_MAX 9223372036854775807L
#define ULONG_MAX 18446744073709551615UL
#define LLONG_MIN (-9223372036854775807LL - 1)
#define LLONG_MAX 9223372036854775807LL
#define ULLONG_MAX 18446744073709551615ULL

#define PATH_MAX 4096
#define NAME_MAX 255
/* These mirror the kernel's per-process limits in kernel/src/elf.rs
 * (MAX_OPEN_FILES, MAX_ARGUMENT_BYTES, MAX_SCHEDULED_PROCESSES). A descriptor
 * table or allocation sized from a larger value would be rejected by the
 * kernel at the first syscall past the real limit. */
#define OPEN_MAX 64
#define ARG_MAX 65536
#define CHILD_MAX 64
#define NGROUPS_MAX 65536
#define PIPE_BUF 4096
#define HOST_NAME_MAX 255
#define LOGIN_NAME_MAX 256
#define SSIZE_MAX LONG_MAX

#endif

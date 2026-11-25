#ifndef OCTAGRAM_SYS_TIME_H
#define OCTAGRAM_SYS_TIME_H

#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif


struct timeval {
    time_t tv_sec;
    suseconds_t tv_usec;
};

int gettimeofday(struct timeval *time_value, void *timezone);

/* POSIX utimes: kept next to gettimeofday because posix_compat.h for
 * <filesystem> includes this header. OCTAGRAM has no timestamp syscall, so it
 * reports ENOSYS rather than pretending the update happened. */
struct timeval;
int utimes(const char *path, const struct timeval times[2]);


#ifdef __cplusplus
}
#endif

#endif

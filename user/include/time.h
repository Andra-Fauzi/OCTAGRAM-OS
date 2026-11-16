#ifndef OCTAGRAM_TIME_H
#define OCTAGRAM_TIME_H

#include <sys/types.h>

#define CLOCK_REALTIME 0

struct timespec {
    time_t tv_sec;
    long tv_nsec;
};

// The vendored TinyCC compiles one translation unit ("tccrun.c") that includes
// this header first and then a host header (`<signal.h>`) which pulls glibc's
// `bits/types/struct_timespec.h`. That header is guarded by `_STRUCT_TIMESPEC`,
// so claiming it here keeps glibc from redefining `struct timespec`.
#ifndef _STRUCT_TIMESPEC
#define _STRUCT_TIMESPEC 1
#endif

struct tm {
    int tm_sec;
    int tm_min;
    int tm_hour;
    int tm_mday;
    int tm_mon;
    int tm_year;
    int tm_wday;
    int tm_yday;
    int tm_isdst;
};

time_t time(time_t *result);
int clock_gettime(clockid_t clock_id, struct timespec *time_value);
struct tm *gmtime(const time_t *time_value);
struct tm *localtime(const time_t *time_value);

#endif

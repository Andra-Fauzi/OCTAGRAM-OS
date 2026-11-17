#ifndef OCTAGRAM_SYS_TIME_H
#define OCTAGRAM_SYS_TIME_H

#include <sys/types.h>

struct timeval {
    time_t tv_sec;
    suseconds_t tv_usec;
};

int gettimeofday(struct timeval *time_value, void *timezone);

#endif

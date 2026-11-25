#ifndef OCTAGRAM_SYS_TIMERFD_H
#define OCTAGRAM_SYS_TIMERFD_H

#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TFD_CLOEXEC 02000000
#define TFD_NONBLOCK 00004000
#define TFD_TIMER_ABSTIME 1

struct itimerspec {
    struct timespec it_interval;
    struct timespec it_value;
};

int timerfd_create(clockid_t clock_id, int flags);
int timerfd_settime(int fd, int flags, const struct itimerspec *new_value,
                    struct itimerspec *old_value);
int timerfd_gettime(int fd, struct itimerspec *current_value);


#ifdef __cplusplus
}
#endif
#endif

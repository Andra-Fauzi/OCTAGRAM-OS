#ifndef OCTAGRAM_SCHED_H
#define OCTAGRAM_SCHED_H

#ifdef __cplusplus
extern "C" {
#endif

#include <sys/types.h>

#define SCHED_OTHER 0
#define SCHED_FIFO 1
#define SCHED_RR 2

struct sched_param {
    int sched_priority;
};

/* Kernel OCTAGRAM mendukung SCHED_OTHER (sched_priority 0, prioritas diatur
 * lewat nice/setpriority) serta SCHED_FIFO/SCHED_RR dengan sched_priority 1..99.
 * Memakai kebijakan RT atau mengubah task lain butuh CAP_SCHED_NICE. */
int sched_setscheduler(pid_t pid, int policy,
                       const struct sched_param *param);
int sched_getscheduler(pid_t pid);
int sched_getparam(pid_t pid, struct sched_param *param);
int sched_setparam(pid_t pid, const struct sched_param *param);
int sched_get_priority_max(int policy);
int sched_get_priority_min(int policy);
int sched_yield(void);


#ifdef __cplusplus
}
#endif
#endif

#ifndef OCTAGRAM_SCHED_H
#define OCTAGRAM_SCHED_H

#include <sys/types.h>

#define SCHED_OTHER 0
#define SCHED_FIFO 1
#define SCHED_RR 2

struct sched_param {
    int sched_priority;
};

/* Kernel OCTAGRAM hanya mendukung SCHED_OTHER dengan prioritas 0; prioritas
 * sebenarnya diatur lewat nice (setpriority). SCHED_FIFO/SCHED_RR ditolak
 * dengan EPERM. */
int sched_setscheduler(pid_t pid, int policy,
                       const struct sched_param *param);
int sched_getscheduler(pid_t pid);
int sched_getparam(pid_t pid, struct sched_param *param);
int sched_setparam(pid_t pid, const struct sched_param *param);
int sched_get_priority_max(int policy);
int sched_get_priority_min(int policy);
int sched_yield(void);

#endif

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

/* CPU affinity. OCTAGRAM schedules on a single CPU and has no affinity
 * syscall, so the mask always contains exactly CPU 0. */
#define CPU_SETSIZE 128
#define __CPU_BITS_PER_WORD (8 * sizeof(unsigned long))
#define __CPU_WORDS (CPU_SETSIZE / __CPU_BITS_PER_WORD)

typedef struct {
    unsigned long bits[__CPU_WORDS];
} cpu_set_t;

#define CPU_ZERO(set) __builtin_memset((set), 0, sizeof(cpu_set_t))
#define CPU_SET(cpu, set) \
    ((void)((set)->bits[(cpu) / __CPU_BITS_PER_WORD] |= \
            1ul << ((cpu) % __CPU_BITS_PER_WORD)))
#define CPU_CLR(cpu, set) \
    ((void)((set)->bits[(cpu) / __CPU_BITS_PER_WORD] &= \
            ~(1ul << ((cpu) % __CPU_BITS_PER_WORD))))
#define CPU_ISSET(cpu, set) \
    (((set)->bits[(cpu) / __CPU_BITS_PER_WORD] & \
      (1ul << ((cpu) % __CPU_BITS_PER_WORD))) != 0)
static inline int oct_cpu_count(const cpu_set_t *set)
{
    int count = 0;
    for (int index = 0; index < CPU_SETSIZE; index++) {
        if (CPU_ISSET(index, set)) {
            count++;
        }
    }
    return count;
}

#define CPU_COUNT(set) oct_cpu_count((set))

int sched_getaffinity(pid_t pid, size_t cpusetsize, cpu_set_t *set);
int sched_setaffinity(pid_t pid, size_t cpusetsize, const cpu_set_t *set);


#ifdef __cplusplus
}
#endif
#endif

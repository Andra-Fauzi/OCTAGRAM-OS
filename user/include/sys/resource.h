#ifndef OCTAGRAM_SYS_RESOURCE_H
#define OCTAGRAM_SYS_RESOURCE_H

#include <stdint.h>
#include <sys/types.h>
#include <sys/time.h>

#define RLIMIT_CPU 0
#define RLIMIT_FSIZE 1
#define RLIMIT_DATA 2
#define RLIMIT_STACK 3
#define RLIMIT_CORE 4
#define RLIMIT_RSS 5
#define RLIMIT_NPROC 6
#define RLIMIT_NOFILE 7
#define RLIMIT_MEMLOCK 8
#define RLIMIT_AS 9
#define RLIMIT_LOCKS 10
#define RLIMIT_SIGPENDING 11
#define RLIMIT_MSGQUEUE 12
#define RLIMIT_NICE 13
#define RLIMIT_RTPRIO 14
#define RLIMIT_RTTIME 15
#define RLIM_INFINITY ((uint64_t)-1)

/* Prioritas nice: makin kecil nilainya, makin tinggi prioritasnya. */
#define PRIO_PROCESS 0
#define PRIO_PGRP 1
#define PRIO_USER 2

#define RUSAGE_SELF 0
#define RUSAGE_CHILDREN -1
#define RUSAGE_THREAD 1

struct rlimit {
    uint64_t rlim_cur;
    uint64_t rlim_max;
};

struct rusage {
    struct timeval ru_utime;
    struct timeval ru_stime;
};

int getrlimit(int resource, struct rlimit *limit);
int setrlimit(int resource, const struct rlimit *limit);
int getrusage(int who, struct rusage *usage);

/* PRIO_PROCESS, PRIO_PGRP, dan PRIO_USER didukung; `who` 0 = pemanggil.
 * Mengubah prioritas task lain butuh CAP_SCHED_NICE atau euid yang sama dengan
 * arah penurunan prioritas saja (nice membesar). */
int getpriority(int which, id_t who);
int setpriority(int which, id_t who, int priority);

#endif

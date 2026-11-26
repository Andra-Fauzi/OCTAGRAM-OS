#ifndef OCTAGRAM_SYS_RESOURCE_H
#define OCTAGRAM_SYS_RESOURCE_H

#include <stdint.h>
#include <sys/types.h>
#include <sys/time.h>

#ifdef __cplusplus
extern "C" {
#endif

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

/* Resource limit type. glibc uses unsigned long on x86-64, so the struct below
 * keeps the same ABI while portable code that names rlim_t still compiles. */
typedef uint64_t rlim_t;

/* Prioritas nice: makin kecil nilainya, makin tinggi prioritasnya. */
#define PRIO_PROCESS 0
#define PRIO_PGRP 1
#define PRIO_USER 2

#define RUSAGE_SELF 0
#define RUSAGE_CHILDREN -1
#define RUSAGE_THREAD 1

struct rlimit {
    rlim_t rlim_cur;
    rlim_t rlim_max;
};

/* Linux x86-64 layout. The kernel fills the two time values and libc zeroes
 * the rest, so the additional accounting counters read as "not measured"
 * instead of leaving fields out that portable code (LLVM's Process.inc reads
 * ru_maxrss) expects to exist. */
struct rusage {
    struct timeval ru_utime;
    struct timeval ru_stime;
    long ru_maxrss;
    long ru_ixrss;
    long ru_idrss;
    long ru_isrss;
    long ru_minflt;
    long ru_majflt;
    long ru_nswap;
    long ru_inblock;
    long ru_oublock;
    long ru_msgsnd;
    long ru_msgrcv;
    long ru_nsignals;
    long ru_nvcsw;
    long ru_nivcsw;
};

int getrlimit(int resource, struct rlimit *limit);
int setrlimit(int resource, const struct rlimit *limit);
int getrusage(int who, struct rusage *usage);

/* PRIO_PROCESS, PRIO_PGRP, dan PRIO_USER didukung; `who` 0 = pemanggil.
 * Mengubah prioritas task lain butuh CAP_SCHED_NICE atau euid yang sama dengan
 * arah penurunan prioritas saja (nice membesar). */
int getpriority(int which, id_t who);
int setpriority(int which, id_t who, int priority);


#ifdef __cplusplus
}
#endif
#endif

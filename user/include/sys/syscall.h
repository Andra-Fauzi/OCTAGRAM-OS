#ifndef OCTAGRAM_SYS_SYSCALL_H
#define OCTAGRAM_SYS_SYSCALL_H

#define SYS_gettid 186
#define SYS_set_tid_address 218
#define SYS_rt_sigaction 13
#define SYS_rt_sigprocmask 14
#define SYS_mmap 9
#define SYS_munmap 11
#define SYS_futex 202

long syscall(long number, ...);

#endif

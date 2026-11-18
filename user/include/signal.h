#ifndef OCTAGRAM_SIGNAL_H
#define OCTAGRAM_SIGNAL_H

// The vendored TinyCC compiles tccrun.c with TARGETOS_OCTAGRAM and needs the
// host <signal.h> and <sys/ucontext.h> pair, so its build keeps the system
// header. Programs built against the OCTAGRAM runtime get the definitions
// below, which match OCTAGRAM syscalls 91-93 and 113-116.
#ifdef TARGETOS_OCTAGRAM

#include_next <signal.h>

#else

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
#include <time.h>

typedef int sig_atomic_t;
typedef uint64_t sigset_t;
typedef void (*sighandler_t)(int);
typedef struct {
    void *ss_sp;
    int ss_flags;
    size_t ss_size;
} stack_t;
typedef struct {
    int si_signo;
    int si_errno;
    int si_code;
    pid_t si_pid;
    uid_t si_uid;
} siginfo_t;

#define SIG_DFL ((sighandler_t)0)
#define SIG_IGN ((sighandler_t)1)
#define SIG_ERR ((sighandler_t)-1)

#define SIGHUP 1
#define SIGINT 2
#define SIGQUIT 3
#define SIGILL 4
#define SIGTRAP 5
#define SIGABRT 6
#define SIGBUS 7
#define SIGFPE 8
#define SIGKILL 9
#define SIGUSR1 10
#define SIGSEGV 11
#define SIGUSR2 12
#define SIGPIPE 13
#define SIGALRM 14
#define SIGTERM 15
#define SIGSTKFLT 16
#define SIGCHLD 17
#define SIGCONT 18
#define SIGSTOP 19
#define SIGTSTP 20
#define SIGTTIN 21
#define SIGTTOU 22
#define SIGURG 23
#define SIGXCPU 24
#define SIGXFSZ 25
#define SIGVTALRM 26
#define SIGPROF 27
#define SIGWINCH 28
#define SIGIO 29
#define SIGPWR 30
#define SIGSYS 31
#define NSIG 32

#define SIG_BLOCK 0
#define SIG_UNBLOCK 1
#define SIG_SETMASK 2

#define SA_NOCLDSTOP 0x00000001
#define SA_SIGINFO 0x00000004
#define SA_RESTART 0x10000000
#define SA_RESETHAND 0x80000000
#define SA_ONSTACK 0x08000000

#define SS_ONSTACK 1
#define SS_DISABLE 2
#define MINSIGSTKSZ 2048
#define SIGSTKSZ 8192
#define SA_NODEFER 0x40000000
#define SIGSTKSZ 8192

struct sigaction {
    union {
        sighandler_t sa_handler;
        void (*sa_sigaction)(int, void *, void *);
    };
    sigset_t sa_mask;
    int sa_flags;
    void (*sa_restorer)(void);
};

int kill(pid_t pid, int signal_number);
int raise(int signal_number);
sighandler_t signal(int signal_number, sighandler_t handler);
int sigaction(int signal_number, const struct sigaction *action,
              struct sigaction *old_action);
int sigprocmask(int how, const sigset_t *set, sigset_t *old_set);
int sigpending(sigset_t *set);
int sigsuspend(const sigset_t *mask);
int sigtimedwait(const sigset_t *set, siginfo_t *info,
                 const struct timespec *timeout);
int sigwait(const sigset_t *set, int *signal_number);
int sigwaitinfo(const sigset_t *set, siginfo_t *info);
int pthread_sigmask(int how, const sigset_t *set, sigset_t *old_set);
int sigaltstack(const stack_t *new_stack, stack_t *old_stack);

static inline int sigemptyset(sigset_t *set)
{
    if (set == NULL) {
        return -1;
    }
    *set = 0;
    return 0;
}

static inline int sigfillset(sigset_t *set)
{
    if (set == NULL) {
        return -1;
    }
    *set = ~(sigset_t)0;
    return 0;
}

static inline int sigaddset(sigset_t *set, int signal_number)
{
    if (set == NULL || signal_number < 1 || signal_number >= NSIG) {
        return -1;
    }
    *set |= (sigset_t)1 << (signal_number - 1);
    return 0;
}

static inline int sigdelset(sigset_t *set, int signal_number)
{
    if (set == NULL || signal_number < 1 || signal_number >= NSIG) {
        return -1;
    }
    *set &= ~((sigset_t)1 << (signal_number - 1));
    return 0;
}

static inline int sigismember(const sigset_t *set, int signal_number)
{
    if (set == NULL || signal_number < 1 || signal_number >= NSIG) {
        return -1;
    }
    return (*set & ((sigset_t)1 << (signal_number - 1))) != 0;
}

#endif /* TARGETOS_OCTAGRAM */

#endif

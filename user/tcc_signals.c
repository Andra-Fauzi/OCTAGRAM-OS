/*
 * Signal stubs for the vendored TinyCC build.
 *
 * `tccrun.c` installs signal handlers so it can print a backtrace when a
 * compiled program faults, and it reaches those handlers through the host's
 * `<signal.h>`. OCTAGRAM-OS is freestanding, so the declarations exist but no
 * implementation does, and `tcc.elf` would link with unresolved references.
 *
 * Catching a fault and resuming the compiler is not something a kernel-side
 * compiler can do the way a hosted process can, so these are no-ops: the
 * handlers TinyCC installs would never run. `sigaction`'s return value is the
 * only one `tccrun.c` looks at, and 0 means "installed".
 */

struct tcc_sigset;
struct tcc_sigaction;

int sigemptyset(struct tcc_sigset *set) {
    (void)set;
    return 0;
}

int sigaddset(struct tcc_sigset *set, int signum) {
    (void)set;
    (void)signum;
    return 0;
}

int sigprocmask(int how, const struct tcc_sigset *set, struct tcc_sigset *old) {
    (void)how;
    (void)set;
    (void)old;
    return 0;
}

int sigaction(int signum, const struct tcc_sigaction *action, struct tcc_sigaction *old) {
    (void)signum;
    (void)action;
    (void)old;
    return 0;
}

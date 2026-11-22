#ifndef OCTAGRAM_SYS_WAIT_H
#define OCTAGRAM_SYS_WAIT_H

#include <sys/types.h>
#include <sys/resource.h>

#define WNOHANG 1
#define WUNTRACED 2
#define WCONTINUED 8
#define WSTOPPED 2
#define WEXITED 4
#define WNOWAIT 0x01000000
#define WIFEXITED(status) (1)
#define WEXITSTATUS(status) ((status) & 0xff)
#define WIFSIGNALED(status) (0)
#define WTERMSIG(status) (0)
#define WIFSTOPPED(status) (0)
#define WSTOPSIG(status) (0)
#define WIFCONTINUED(status) (0)
#define WCOREDUMP(status) (0)

pid_t wait(int *status);
pid_t waitpid(pid_t pid, int *status, int options);
pid_t wait3(int *status, int options, struct rusage *usage);
pid_t wait4(pid_t pid, int *status, int options, struct rusage *usage);

#endif

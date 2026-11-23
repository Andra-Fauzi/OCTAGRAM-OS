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
#define WIFEXITED(status) (((status) & 0x7f) == 0)
#define WEXITSTATUS(status) (((status) >> 8) & 0xff)
#define WIFSIGNALED(status) (((status) & 0x7f) != 0 && ((status) & 0x7f) != 0x7f)
#define WTERMSIG(status) ((status) & 0x7f)
#define WIFSTOPPED(status) (((status) & 0xff) == 0x7f)
#define WSTOPSIG(status) (((status) >> 8) & 0xff)
#define WIFCONTINUED(status) ((status) == 0xffff)
#define WCOREDUMP(status) (((status) & 0x80) != 0)

pid_t wait(int *status);
pid_t waitpid(pid_t pid, int *status, int options);
pid_t wait3(int *status, int options, struct rusage *usage);
pid_t wait4(pid_t pid, int *status, int options, struct rusage *usage);

#endif

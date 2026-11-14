#ifndef OCTAGRAM_SYS_WAIT_H
#define OCTAGRAM_SYS_WAIT_H

#include <sys/types.h>

#define WIFEXITED(status) (((status) & 0x7f) == 0)
#define WEXITSTATUS(status) (((status) >> 8) & 0xff)

pid_t wait(int *status);
pid_t waitpid(pid_t pid, int *status, int options);

#endif

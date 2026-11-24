#ifndef OCTAGRAM_SYS_FILE_H
#define OCTAGRAM_SYS_FILE_H

#include <sys/types.h>

#define LOCK_SH 1
#define LOCK_EX 2
#define LOCK_NB 4
#define LOCK_UN 8

int flock(int fd, int operation);
int lockf(int fd, int operation, off_t length);

#endif

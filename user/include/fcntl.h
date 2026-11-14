#ifndef OCTAGRAM_FCNTL_H
#define OCTAGRAM_FCNTL_H

#include "octagram.h"

#define O_RDONLY OCT_O_RDONLY
#define O_WRONLY OCT_O_WRONLY
#define O_RDWR OCT_O_RDWR
#define O_ACCMODE 3
#define O_CREAT OCT_O_CREAT
#define O_TRUNC OCT_O_TRUNC
#define O_APPEND OCT_O_APPEND

int open(const char *path, int flags, ...);

#endif

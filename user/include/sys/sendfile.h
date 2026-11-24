#ifndef OCTAGRAM_SYS_SENDFILE_H
#define OCTAGRAM_SYS_SENDFILE_H

#include <stddef.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

ssize_t sendfile(int out_fd, int in_fd, off_t *offset, size_t count);

#ifdef __cplusplus
}
#endif

#endif

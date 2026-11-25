#ifndef OCTAGRAM_SYS_RANDOM_H
#define OCTAGRAM_SYS_RANDOM_H

#include <stddef.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

#define GRND_NONBLOCK 0x0001
#define GRND_RANDOM 0x0002

ssize_t getrandom(void *buffer, size_t length, unsigned int flags);


#ifdef __cplusplus
}
#endif
#endif

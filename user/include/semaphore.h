#ifndef OCTAGRAM_SEMAPHORE_H
#define OCTAGRAM_SEMAPHORE_H

/* The counting semaphore itself (sem_t and the wait/post family) lives in
 * <sys/semaphore.h>, backed by the futex implementation in runtime.c. */
#include <sys/semaphore.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SEM_FAILED ((sem_t *)0)
#define SEM_VALUE_MAX 2147483647

/* Named semaphores live in a small process-local table: the name is the key, so
 * two processes opening the same name get independent semaphores. Cross-process
 * named semaphores need a kernel object OCTAGRAM does not have yet. */
sem_t *sem_open(const char *name, int flags, ...);
int sem_close(sem_t *semaphore);
int sem_unlink(const char *name);

#ifdef __cplusplus
}
#endif
#endif

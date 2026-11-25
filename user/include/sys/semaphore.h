#ifndef OCTAGRAM_SYS_SEMAPHORE_H
#define OCTAGRAM_SYS_SEMAPHORE_H

#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    volatile unsigned int count;
} sem_t;

int sem_init(sem_t *semaphore, int shared, unsigned int value);
int sem_destroy(sem_t *semaphore);
int sem_wait(sem_t *semaphore);
int sem_trywait(sem_t *semaphore);
int sem_timedwait(sem_t *semaphore, const struct timespec *absolute_time);
int sem_post(sem_t *semaphore);
int sem_getvalue(sem_t *semaphore, int *value);


#ifdef __cplusplus
}
#endif
#endif

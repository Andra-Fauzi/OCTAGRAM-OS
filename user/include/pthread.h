#ifndef OCTAGRAM_PTHREAD_H
#define OCTAGRAM_PTHREAD_H

#include <stddef.h>
#include <stdint.h>

typedef uint64_t pthread_t;
typedef struct {
    volatile int state;
} pthread_mutex_t;
typedef struct {
    uintptr_t opaque;
} pthread_attr_t;

#define PTHREAD_MUTEX_INITIALIZER {0}

int pthread_create(pthread_t *thread, const pthread_attr_t *attributes,
                   void *(*start_routine)(void *), void *argument);
int pthread_join(pthread_t thread, void **result);
pthread_t pthread_self(void);
__attribute__((noreturn)) void pthread_exit(void *result);

int pthread_mutex_init(pthread_mutex_t *mutex, const void *attributes);
int pthread_mutex_destroy(pthread_mutex_t *mutex);
int pthread_mutex_lock(pthread_mutex_t *mutex);
int pthread_mutex_trylock(pthread_mutex_t *mutex);
int pthread_mutex_unlock(pthread_mutex_t *mutex);

#endif

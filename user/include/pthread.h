#ifndef OCTAGRAM_PTHREAD_H
#define OCTAGRAM_PTHREAD_H

#include <stddef.h>
#include <stdint.h>
#include <time.h>

typedef uint64_t pthread_t;
typedef struct {
    volatile int state;
} pthread_mutex_t;
typedef struct {
    uintptr_t opaque;
} pthread_attr_t;
typedef struct {
    volatile unsigned int sequence;
} pthread_cond_t;
typedef struct {
    int opaque;
} pthread_condattr_t;
typedef unsigned int pthread_key_t;

#define PTHREAD_MUTEX_INITIALIZER {0}
#define PTHREAD_COND_INITIALIZER {0}

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

int pthread_cond_init(pthread_cond_t *cond, const pthread_condattr_t *attributes);
int pthread_cond_destroy(pthread_cond_t *cond);
int pthread_cond_wait(pthread_cond_t *cond, pthread_mutex_t *mutex);
int pthread_cond_timedwait(pthread_cond_t *cond, pthread_mutex_t *mutex,
                           const struct timespec *absolute_time);
int pthread_cond_signal(pthread_cond_t *cond);
int pthread_cond_broadcast(pthread_cond_t *cond);

int pthread_key_create(pthread_key_t *key, void (*destructor)(void *));
int pthread_key_delete(pthread_key_t key);
void *pthread_getspecific(pthread_key_t key);
int pthread_setspecific(pthread_key_t key, const void *value);
int pthread_condattr_init(pthread_condattr_t *attributes);
int pthread_condattr_destroy(pthread_condattr_t *attributes);
int pthread_condattr_setclock(pthread_condattr_t *attributes, clockid_t clock);
int pthread_detach(pthread_t thread);
int pthread_attr_init(pthread_attr_t *attributes);
int pthread_attr_destroy(pthread_attr_t *attributes);
int pthread_attr_setdetachstate(pthread_attr_t *attributes, int state);
int pthread_attr_getdetachstate(const pthread_attr_t *attributes, int *state);
#define PTHREAD_CREATE_JOINABLE 0
#define PTHREAD_CREATE_DETACHED 1

#endif

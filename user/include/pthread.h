#ifndef OCTAGRAM_PTHREAD_H
#define OCTAGRAM_PTHREAD_H

#include <stddef.h>
#include <stdint.h>
#include <time.h>

typedef uint64_t pthread_t;

/* Tautan daftar robust kernel; harus berada di offset 0 dari mutex. */
struct oct_pthread_robust_entry {
    struct oct_pthread_robust_entry *next;
};

typedef struct {
    struct oct_pthread_robust_entry list; /* offset 0 */
    volatile unsigned int state;          /* offset 8: kata futex */
    volatile unsigned int flags;
} pthread_mutex_t;
typedef struct {
    uintptr_t opaque;
    size_t stack_size;
} pthread_attr_t;
typedef struct {
    volatile unsigned int sequence;
} pthread_cond_t;
typedef struct {
    int opaque;
} pthread_condattr_t;
typedef unsigned int pthread_key_t;

typedef struct {
    int robustness;
} pthread_mutexattr_t;

#define PTHREAD_MUTEX_INITIALIZER {{0}, 0, 0}
#define PTHREAD_COND_INITIALIZER {0}

#define PTHREAD_MUTEX_STALLED 0
#define PTHREAD_MUTEX_NORMAL 0
#define PTHREAD_MUTEX_DEFAULT PTHREAD_MUTEX_NORMAL
#define PTHREAD_MUTEX_ROBUST 1
#define PTHREAD_KEYS_MAX 64
#define PTHREAD_DESTRUCTOR_ITERATIONS 4

int pthread_create(pthread_t *thread, const pthread_attr_t *attributes,
                   void *(*start_routine)(void *), void *argument);
int pthread_join(pthread_t thread, void **result);
pthread_t pthread_self(void);
__attribute__((noreturn)) void pthread_exit(void *result);

int pthread_mutex_init(pthread_mutex_t *mutex,
                       const pthread_mutexattr_t *attributes);
int pthread_mutex_destroy(pthread_mutex_t *mutex);
int pthread_mutex_lock(pthread_mutex_t *mutex);
int pthread_mutex_trylock(pthread_mutex_t *mutex);
int pthread_mutex_unlock(pthread_mutex_t *mutex);
int pthread_mutex_consistent(pthread_mutex_t *mutex);

int pthread_mutexattr_init(pthread_mutexattr_t *attributes);
int pthread_mutexattr_destroy(pthread_mutexattr_t *attributes);
int pthread_mutexattr_settype(pthread_mutexattr_t *attributes, int type);
int pthread_mutexattr_setrobust(pthread_mutexattr_t *attributes,
                                int robustness);
int pthread_mutexattr_getrobust(const pthread_mutexattr_t *attributes,
                                int *robustness);

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
int pthread_attr_setstacksize(pthread_attr_t *attributes, size_t stack_size);
int pthread_attr_getstacksize(const pthread_attr_t *attributes, size_t *stack_size);
#define PTHREAD_CREATE_JOINABLE 0
#define PTHREAD_CREATE_DETACHED 1
#define PTHREAD_STACK_MIN 16384

#endif

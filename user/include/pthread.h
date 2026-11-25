#ifndef OCTAGRAM_PTHREAD_H
#define OCTAGRAM_PTHREAD_H

#include <stddef.h>
#include <stdint.h>
#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint64_t pthread_t;

/* Tautan daftar robust kernel; harus berada di offset 0 dari mutex. */
struct oct_pthread_robust_entry {
    struct oct_pthread_robust_entry *next;
};

typedef struct {
    struct oct_pthread_robust_entry list; /* offset 0 */
    volatile unsigned int state;          /* offset 8: kata futex */
    volatile unsigned int flags;
    volatile unsigned int count; /* recursion depth of a recursive mutex */
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
    int state;
} pthread_once_t;

typedef struct {
    int robustness;
    int type;
} pthread_mutexattr_t;

/* Reader/writer lock built from a futex mutex plus one futex condition
 * variable. `gate` is broadcast on every transition that can let a waiter
 * proceed, so readers and writers share a single wakeup channel. */
typedef struct {
    pthread_mutex_t mutex;
    pthread_cond_t gate;
    unsigned int readers; /* active readers */
    unsigned int writers; /* writers waiting for the lock */
    int writer_active;
    int prefer_writer;
} pthread_rwlock_t;

typedef struct {
    int kind;
} pthread_rwlockattr_t;

#define PTHREAD_RWLOCK_INITIALIZER \
    { { { 0 }, 0, 0, 0 }, { 0 }, 0, 0, 0, 0 }
#define PTHREAD_RWLOCK_PREFER_READER_NP 0
#define PTHREAD_RWLOCK_PREFER_WRITER_NP 1
#define PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP 2

#define PTHREAD_MUTEX_INITIALIZER {{0}, 0, 0, 0}
#define PTHREAD_ONCE_INIT {0}
#define PTHREAD_COND_INITIALIZER {0}

#define PTHREAD_MUTEX_STALLED 0
#define PTHREAD_MUTEX_NORMAL 0
#define PTHREAD_MUTEX_DEFAULT PTHREAD_MUTEX_NORMAL
#define PTHREAD_MUTEX_ROBUST 1
#define PTHREAD_MUTEX_RECURSIVE 1
#define PTHREAD_MUTEX_ERRORCHECK 2
#define PTHREAD_MUTEX_ADAPTIVE_NP 3
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
int pthread_mutexattr_gettype(const pthread_mutexattr_t *attributes, int *type);
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

int pthread_once(pthread_once_t *once, void (*initializer)(void));
int pthread_key_create(pthread_key_t *key, void (*destructor)(void *));
int pthread_key_delete(pthread_key_t key);
void *pthread_getspecific(pthread_key_t key);
int pthread_setspecific(pthread_key_t key, const void *value);
int pthread_condattr_init(pthread_condattr_t *attributes);
int pthread_condattr_destroy(pthread_condattr_t *attributes);
int pthread_condattr_setclock(pthread_condattr_t *attributes, clockid_t clock);
int pthread_detach(pthread_t thread);

int pthread_mutex_timedlock(pthread_mutex_t *mutex,
                            const struct timespec *absolute_time);
int pthread_cond_clockwait(pthread_cond_t *cond, pthread_mutex_t *mutex,
                           clockid_t clock,
                           const struct timespec *absolute_time);

int pthread_rwlock_init(pthread_rwlock_t *lock,
                        const pthread_rwlockattr_t *attributes);
int pthread_rwlock_destroy(pthread_rwlock_t *lock);
int pthread_rwlock_rdlock(pthread_rwlock_t *lock);
int pthread_rwlock_tryrdlock(pthread_rwlock_t *lock);
int pthread_rwlock_timedrdlock(pthread_rwlock_t *lock,
                               const struct timespec *absolute_time);
int pthread_rwlock_wrlock(pthread_rwlock_t *lock);
int pthread_rwlock_trywrlock(pthread_rwlock_t *lock);
int pthread_rwlock_timedwrlock(pthread_rwlock_t *lock,
                               const struct timespec *absolute_time);
int pthread_rwlock_unlock(pthread_rwlock_t *lock);
int pthread_rwlockattr_init(pthread_rwlockattr_t *attributes);
int pthread_rwlockattr_destroy(pthread_rwlockattr_t *attributes);
int pthread_rwlockattr_setkind_np(pthread_rwlockattr_t *attributes, int kind);
int pthread_rwlockattr_getkind_np(const pthread_rwlockattr_t *attributes,
                                  int *kind);
int pthread_attr_init(pthread_attr_t *attributes);
int pthread_attr_destroy(pthread_attr_t *attributes);
int pthread_attr_setdetachstate(pthread_attr_t *attributes, int state);
int pthread_attr_getdetachstate(const pthread_attr_t *attributes, int *state);
int pthread_attr_setstacksize(pthread_attr_t *attributes, size_t stack_size);
int pthread_attr_getstacksize(const pthread_attr_t *attributes, size_t *stack_size);
#define PTHREAD_CREATE_JOINABLE 0
#define PTHREAD_CREATE_DETACHED 1
#define PTHREAD_STACK_MIN 16384

#ifdef __cplusplus
}
#endif

#endif

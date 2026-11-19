#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>

static pthread_key_t value_key;
static volatile unsigned int destructor_count;
static volatile unsigned int destructor_sum;

typedef struct WorkerArgument {
    unsigned int value;
} WorkerArgument;

static void value_destructor(void *opaque)
{
    unsigned int value = (unsigned int)(uintptr_t)opaque;
    __sync_fetch_and_add(&destructor_count, 1);
    __sync_fetch_and_add(&destructor_sum, value);
}

static void *worker(void *opaque)
{
    const WorkerArgument *argument = opaque;
    void *expected = (void *)(uintptr_t)argument->value;
    if (pthread_setspecific(value_key, expected) != 0) {
        return (void *)(uintptr_t)1;
    }
    if (sched_yield() != 0) {
        return (void *)(uintptr_t)2;
    }
    if (pthread_getspecific(value_key) != expected) {
        return (void *)(uintptr_t)3;
    }
    return expected;
}

int main(void)
{
    if (pthread_key_create(&value_key, value_destructor) != 0) {
        return 1;
    }
    if (pthread_setspecific(value_key, (void *)(uintptr_t)17) != 0) {
        return 2;
    }

    WorkerArgument arguments[2] = {{7}, {11}};
    pthread_t threads[2];
    if (pthread_create(&threads[0], NULL, worker, &arguments[0]) != 0 ||
        pthread_create(&threads[1], NULL, worker, &arguments[1]) != 0) {
        return 3;
    }

    void *first = NULL;
    void *second = NULL;
    if (pthread_join(threads[0], &first) != 0 ||
        pthread_join(threads[1], &second) != 0) {
        return 4;
    }

    if (first != (void *)(uintptr_t)7 ||
        second != (void *)(uintptr_t)11 ||
        pthread_getspecific(value_key) != (void *)(uintptr_t)17 ||
        destructor_count != 2 || destructor_sum != 18) {
        return 5;
    }

    if (pthread_key_delete(value_key) != 0 ||
        pthread_getspecific(value_key) != NULL ||
        pthread_setspecific(value_key, (void *)(uintptr_t)1) != EINVAL) {
        return 6;
    }

    puts("pthread TSD isolation and destructors: PASS");
    return 0;
}

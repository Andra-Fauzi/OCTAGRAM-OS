#include <pthread.h>
#include <stdint.h>
#include <stdio.h>

static __thread int tls_value = 41;
static pthread_mutex_t counter_lock = PTHREAD_MUTEX_INITIALIZER;
static int shared_counter;

typedef struct WorkerArgument {
    int value;
} WorkerArgument;

static void *worker(void *opaque)
{
    const WorkerArgument *argument = opaque;
    if (tls_value != 41) {
        return (void *)(uintptr_t)1;
    }

    tls_value = argument->value;
    for (int i = 0; i < 1000; ++i) {
        if (pthread_mutex_lock(&counter_lock) != 0) {
            return (void *)(uintptr_t)2;
        }
        ++shared_counter;
        pthread_mutex_unlock(&counter_lock);
    }
    return tls_value == argument->value
        ? (void *)(uintptr_t)argument->value
        : (void *)(uintptr_t)3;
}

int main(void)
{
    WorkerArgument arguments[] = {{7}, {11}};
    pthread_t threads[2];
    pthread_attr_t attributes;
    size_t configured_stack_size = 0;
    if (pthread_attr_init(&attributes) != 0
        || pthread_attr_setstacksize(&attributes, 96u * 1024u) != 0
        || pthread_attr_getstacksize(&attributes, &configured_stack_size) != 0
        || configured_stack_size != 96u * 1024u) {
        return 1;
    }
    if (pthread_create(&threads[0], &attributes, worker, &arguments[0]) != 0
        || pthread_create(&threads[1], &attributes, worker, &arguments[1]) != 0) {
        return 1;
    }

    (void)pthread_attr_destroy(&attributes);
    void *first_result = NULL;
    void *second_result = NULL;
    if (pthread_join(threads[0], &first_result) != 0
        || pthread_join(threads[1], &second_result) != 0) {
        return 2;
    }
    if (first_result != (void *)(uintptr_t)7
        || second_result != (void *)(uintptr_t)11
        || shared_counter != 2000
        || tls_value != 41
        || pthread_self() == 0) {
        return 3;
    }

    puts("preemptive threads and TLS passed");
    return 0;
}

#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>

static __thread int tls_value = 41;
static pthread_mutex_t counter_lock = PTHREAD_MUTEX_INITIALIZER;
static int shared_counter;
static volatile int detached_worker_done;
static volatile int attr_detached_worker_done;

static void *detached_worker(void *opaque)
{
    volatile int *done = opaque;
    *done = 1;
    return NULL;
}

typedef struct WorkerArgument {
    int value;
} WorkerArgument;

/*
 * Keep an SSE2 register live across kernel_yield. This forces a kernel
 * handoff and return, while avoiding a C function call that would make XMM15
 * subject to the SysV caller-saved register rule.
 */
static int verify_xmm_across_kernel_yield(uint64_t pattern)
{
    uint64_t observed;
    __asm__ volatile(
        "movq %1, %%xmm15\n\t"
        "mov $65, %%eax\n\t"
        "int $0x80\n\t"
        "movq %%xmm15, %0\n\t"
        : "=&r"(observed)
        : "r"(pattern)
        : "rax", "rcx", "r11", "xmm15", "cc", "memory");
    return observed == pattern;
}

/*
 * Keep XMM15 live for long enough that the timer should preempt this worker
 * while another pthread is runnable.  The other worker uses a different
 * pattern, so a missing per-thread FPU save/restore is observable.
 */
static int verify_xmm_across_preemption(uint64_t pattern)
{
    uint64_t observed;
    const uint32_t spins = 30000000u;
    __asm__ volatile(
        "movq %1, %%xmm15\n\t"
        "movl %2, %%ecx\n\t"
        "1: pause\n\t"
        "decl %%ecx\n\t"
        "jnz 1b\n\t"
        "movq %%xmm15, %0\n\t"
        : "=&r"(observed)
        : "r"(pattern), "r"(spins)
        : "rcx", "xmm15", "cc", "memory");
    return observed == pattern;
}

static void *worker(void *opaque)
{
    const WorkerArgument *argument = opaque;
    if (tls_value != 41) {
        return (void *)(uintptr_t)1;
    }

    const uint64_t xmm_pattern = 0x4f43544147524100ULL | (uint64_t)argument->value;
    if (!verify_xmm_across_kernel_yield(xmm_pattern)) {
        return (void *)(uintptr_t)4;
    }
    if (!verify_xmm_across_preemption(xmm_pattern)) {
        return (void *)(uintptr_t)5;
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
    pthread_mutexattr_t mutex_attributes;
    if (pthread_mutexattr_init(&mutex_attributes) != 0
        || pthread_mutexattr_settype(&mutex_attributes, PTHREAD_MUTEX_NORMAL) != 0
        || pthread_mutexattr_settype(&mutex_attributes, -1) != EINVAL
        || pthread_mutexattr_destroy(&mutex_attributes) != 0) {
        return 1;
    }

    pthread_condattr_t condition_attributes;
    if (pthread_condattr_init(&condition_attributes) != 0
        || pthread_condattr_setclock(&condition_attributes, CLOCK_MONOTONIC) != 0
        || pthread_condattr_destroy(&condition_attributes) != 0) {
        return 1;
    }

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

    pthread_t detached_thread;
    if (pthread_create(&detached_thread, NULL, detached_worker,
                       (void *)&detached_worker_done) != 0
        || pthread_detach(detached_thread) != 0
        || pthread_join(detached_thread, NULL) != EINVAL) {
        return 4;
    }
    for (volatile unsigned int spin = 0;
         spin < 10000000u && !detached_worker_done; ++spin) {
        __asm__ volatile ("pause");
    }
    if (!detached_worker_done) {
        return 5;
    }

    pthread_attr_t detached_attributes;
    pthread_t attr_detached_thread;
    if (pthread_attr_init(&detached_attributes) != 0
        || pthread_attr_setdetachstate(&detached_attributes,
                                       PTHREAD_CREATE_DETACHED) != 0
        || pthread_create(&attr_detached_thread, &detached_attributes,
                         detached_worker, (void *)&attr_detached_worker_done) != 0
        || pthread_attr_destroy(&detached_attributes) != 0) {
        return 6;
    }
    for (volatile unsigned int spin = 0;
         spin < 10000000u && !attr_detached_worker_done; ++spin) {
        __asm__ volatile ("pause");
    }
    if (!attr_detached_worker_done) {
        return 7;
    }

    puts("preemptive threads, TLS, SSE context, and detached pthreads passed");
    return 0;
}

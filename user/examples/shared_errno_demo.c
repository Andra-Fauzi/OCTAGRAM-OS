#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define ITERATIONS 1200

typedef struct WorkerArgument {
    int expected_errno;
    int expected_h_errno;
} WorkerArgument;

static void *worker(void *opaque)
{
    const WorkerArgument *argument = opaque;

    for (unsigned int i = 0; i < ITERATIONS; ++i) {
        int descriptor = open("/.__octagram_shared_errno_missing__", O_RDONLY);
        if (descriptor >= 0) {
            close(descriptor);
            return (void *)(uintptr_t)1;
        }
        if (errno != ENOENT) {
            return (void *)(uintptr_t)2;
        }

        errno = argument->expected_errno;
        h_errno = argument->expected_h_errno;

        size_t size = 24u + (i % 257u);
        unsigned char *allocation = malloc(size);
        if (allocation == NULL) {
            return (void *)(uintptr_t)3;
        }
        memset(allocation, (int)(i & 0xffu), size);
        free(allocation);

        if (sched_yield() != 0) {
            return (void *)(uintptr_t)4;
        }
        if (errno != argument->expected_errno ||
            h_errno != argument->expected_h_errno) {
            return (void *)(uintptr_t)5;
        }
    }
    return NULL;
}

int main(void)
{
    WorkerArgument arguments[2] = {
        {E2BIG, HOST_NOT_FOUND},
        {EOVERFLOW, TRY_AGAIN}
    };
    pthread_t threads[2];
    void *results[2] = {NULL, NULL};

    errno = EDOM;
    h_errno = NO_RECOVERY;
    if (pthread_create(&threads[0], NULL, worker, &arguments[0]) != 0 ||
        pthread_create(&threads[1], NULL, worker, &arguments[1]) != 0) {
        return 1;
    }

    for (unsigned int i = 0; i < ITERATIONS; ++i) {
        void *allocation = malloc(32u + (i % 193u));
        if (allocation == NULL) {
            return 2;
        }
        free(allocation);
        if (sched_yield() != 0) {
            return 3;
        }
        if (errno != EDOM || h_errno != NO_RECOVERY) {
            return 4;
        }
    }

    if (pthread_join(threads[0], &results[0]) != 0 ||
        pthread_join(threads[1], &results[1]) != 0) {
        return 5;
    }
    if (results[0] != NULL || results[1] != NULL ||
        errno != EDOM || h_errno != NO_RECOVERY) {
        return 6;
    }

    const char message[] = "shared errno/h_errno + allocator: PASS\n";
    return write(1, message, sizeof(message) - 1) < 0;
}

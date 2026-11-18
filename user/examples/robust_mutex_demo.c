/* Demo mutex robust: thread pemilik keluar tanpa unlock, lalu pengunci
 * berikutnya mendapat EOWNERDEAD (atau ENOTRECOVERABLE bila pemilik
 * sebelumnya tidak memanggil pthread_mutex_consistent). */
#include <errno.h>
#include <pthread.h>
#include <stdio.h>

static pthread_mutex_t robust_mutex;

static void *holder(void *argument)
{
    (void)argument;
    if (pthread_mutex_lock(&robust_mutex) != 0) {
        return NULL;
    }
    /* Kembali tanpa unlock: kernel menandai FUTEX_OWNER_DIED. */
    return NULL;
}

static int run_holder(void)
{
    pthread_t thread;
    int status = pthread_create(&thread, NULL, holder, NULL);
    if (status != 0) {
        printf("pthread_create gagal: %d\n", status);
        return status;
    }
    void *result = NULL;
    status = pthread_join(thread, &result);
    if (status != 0) {
        printf("pthread_join gagal: %d\n", status);
    }
    return status;
}

int main(void)
{
    pthread_mutexattr_t attributes;
    if (pthread_mutexattr_init(&attributes) != 0) {
        return 1;
    }
    if (pthread_mutexattr_setrobust(&attributes, PTHREAD_MUTEX_ROBUST) != 0) {
        return 2;
    }
    if (pthread_mutex_init(&robust_mutex, &attributes) != 0) {
        return 3;
    }
    pthread_mutexattr_destroy(&attributes);

    if (run_holder() != 0) {
        return 4;
    }
    int status = pthread_mutex_lock(&robust_mutex);
    printf("lock setelah pemilik keluar: %d (EOWNERDEAD=%d)\n", status,
           EOWNERDEAD);
    if (status != EOWNERDEAD) {
        return 5;
    }
    if (pthread_mutex_consistent(&robust_mutex) != 0) {
        return 6;
    }
    if (pthread_mutex_unlock(&robust_mutex) != 0) {
        return 7;
    }
    status = pthread_mutex_lock(&robust_mutex);
    if (status != 0) {
        return 8;
    }
    if (pthread_mutex_unlock(&robust_mutex) != 0) {
        return 9;
    }

    if (run_holder() != 0) {
        return 10;
    }
    status = pthread_mutex_lock(&robust_mutex);
    if (status != EOWNERDEAD) {
        return 11;
    }
    /* Sengaja tanpa pthread_mutex_consistent(). */
    if (pthread_mutex_unlock(&robust_mutex) != 0) {
        return 12;
    }
    status = pthread_mutex_lock(&robust_mutex);
    printf("lock setelah pemilik gagal konsisten: %d (ENOTRECOVERABLE=%d)\n",
           status, ENOTRECOVERABLE);
    if (status != ENOTRECOVERABLE) {
        return 13;
    }

    pthread_mutex_destroy(&robust_mutex);
    printf("robust mutex OK\n");
    return 0;
}

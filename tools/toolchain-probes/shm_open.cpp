#include <errno.h>
#include <fcntl.h>
#include <sys/mman.h>

/* The preflight links this probe; the guest can also run it to verify that
 * unavailable named shared memory is reported explicitly as ENOSYS. */
int main()
{
    errno = 0;
    if (shm_open("/octagram-runtime-probe", O_CREAT | O_RDWR, 0600) != -1 ||
        errno != ENOSYS) {
        return 1;
    }

    errno = 0;
    if (shm_unlink("/octagram-runtime-probe") != -1 || errno != ENOSYS) {
        return 2;
    }

    return 0;
}

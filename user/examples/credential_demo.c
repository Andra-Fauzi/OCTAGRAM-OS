#include <errno.h>
#include <stdio.h>
#include <sys/capability.h>
#include <unistd.h>

/* Memeriksa metadata kredensial/kemampuan: root memiliki CAP_ALL, menurunkan
 * uid ke 1000 mencabut semua kemampuan, dan non-root tidak bisa naik kembali. */
int main(void)
{
    uint64_t mask = 0;
    if (getuid() != 0 || geteuid() != 0) {
        return 1;
    }
    if (capget(0, &mask) != 0 || mask != CAP_ALL) {
        return 2;
    }
    if (setuid(1000) != 0 || getuid() != 1000 || geteuid() != 1000) {
        return 3;
    }
    if (capget(0, &mask) != 0 || mask != 0) {
        return 4;
    }
    errno = 0;
    if (setuid(0) == 0 || errno != EPERM) {
        return 5;
    }
    errno = 0;
    if (capset(0, CAP_ALL) == 0 || errno != EPERM) {
        return 6;
    }
    return printf("credential/capability metadata passed\n") < 0 ? 7 : 0;
}

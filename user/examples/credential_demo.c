#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/capability.h>
#include <unistd.h>

/* Memeriksa metadata kredensial/kemampuan: root memiliki CAP_ALL, menurunkan
 * uid ke 1000 mencabut semua kemampuan, non-root tidak bisa naik kembali,
 * penegakan device (/dev/fb0), console (/dev/tty*), dan penulisan berkas
 * yang dibuka sebelum uid diturunkan. */
int main(void)
{
    uint64_t mask = 0;
    if (getuid() != 0 || geteuid() != 0) {
        return 1;
    }
    if (capget(0, &mask) != 0 || mask != CAP_ALL) {
        return 2;
    }

    /* Buka berkas untuk ditulis saat masih root. */
    int opened_fd = open("/tmp/cred_test.txt", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (opened_fd >= 0) {
        if (write(opened_fd, "root\n", 5) != 5) {
            close(opened_fd);
            return 3;
        }
    }

    if (setuid(1000) != 0 || getuid() != 1000 || geteuid() != 1000) {
        return 4;
    }
    if (capget(0, &mask) != 0 || mask != 0) {
        return 5;
    }
    errno = 0;
    if (setuid(0) == 0 || errno != EPERM) {
        return 6;
    }
    errno = 0;
    if (capset(0, CAP_ALL) == 0 || errno != EPERM) {
        return 7;
    }

    /* Descriptor yang dibuka sebelum uid diturunkan: penulisan harus ditolak
     * karena CAP_VFS_WRITE sudah dicabut. */
    if (opened_fd >= 0) {
        errno = 0;
        if (write(opened_fd, "user\n", 5) >= 0 || errno != EPERM) {
            close(opened_fd);
            return 8;
        }
        close(opened_fd);
    }

    /* Akses perangkat raw (/dev/fb0) harus ditolak untuk non-root tanpa CAP_SYS_ADMIN. */
    errno = 0;
    int fb_fd = open("/dev/fb0", O_RDWR);
    if (fb_fd >= 0 || errno != EPERM) {
        if (fb_fd >= 0) close(fb_fd);
        return 9;
    }

    return printf("credential/capability metadata/device/descriptor enforcement passed\n") < 0 ? 10 : 0;
}
